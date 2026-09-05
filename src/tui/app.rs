use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::apps::{summarize, DbEntry};
use crate::constants::{DOUBLE_CLICK, TUI_EXPORT_FILENAME};
use crate::csv_export::write_csv;
use crate::delete::{commit, needs_typed_confirm, Collector, CollectorItem, Confirm};
use crate::dto::waste_hits;
use crate::scan::{Progress, Tree};
use crate::settings::Settings;
use crate::size::human_bytes;
use crate::sys;
use crate::tui::picker::Picker;

/// Rows the list scrolls by; the browse list uses the same page.
pub const LIST_PAGE: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Picker,
    Scanning,
    Browse,
    Findings,
    Databases,
    Collector,
    Confirm { typed: String },
    Help,
    Settings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Findings,
    Databases,
    Collector,
    Export,
    Quit,
    Back,
    ConfirmDelete,
    Cancel,
    Mark,
    Help,
    Scan,
    /// Leave a finished scan for the directory picker.
    Picker,
    /// Return to the scan the picker was opened from.
    BackToScan,
    /// Install the offered GitHub Release and re-exec.
    ApplyUpdate,
    DismissUpdate,
}

/// How the browse view draws the tree. Both layouts consume the same
/// `Slice` list from `sunburst::build_slices`; only the projection and the
/// panel split differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    /// Braille disc on the left, child list on the right.
    Sunburst,
    /// Full-width icicle above the child list. Names fit inside the bars,
    /// and it costs a quarter of the rows.
    Icicle,
}

impl Layout {
    pub fn next(self) -> Layout {
        match self {
            Layout::Sunburst => Layout::Icicle,
            Layout::Icicle => Layout::Sunburst,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Layout::Sunburst => "sunburst",
            Layout::Icicle => "icicle",
        }
    }
}

/// What the pointer is over. Drawn as a subtle highlight; never moves
/// the selection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Hover {
    Row(usize),
    Slice(usize),
    Button(Action),
    Menu(usize),
    Crumb(usize),
}

/// One entry in the right-click context menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MenuAction {
    Open,
    ScanHere,
    ToggleMark,
    Delete,
    Cancel,
}

/// Right-click menu, drawn over whatever view is underneath.
#[derive(Clone, Debug)]
pub struct Menu {
    pub x: u16,
    pub y: u16,
    pub title: String,
    pub items: Vec<(MenuAction, String)>,
    pub selected: usize,
}

/// Cursor arithmetic shared by every list: clamp `i + delta` into `0..len`.
pub fn step(i: usize, delta: isize, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    (i as isize)
        .saturating_add(delta)
        .clamp(0, len as isize - 1) as usize
}

/// Smallest scroll change that keeps `sel` inside a `page`-row window.
pub fn scroll_to_show(sel: usize, offset: usize, page: usize) -> usize {
    if sel < offset {
        sel
    } else if sel >= offset + page {
        sel.saturating_sub(page.saturating_sub(1))
    } else {
        offset
    }
}

/// Display name: the root shows as `/`, everything else by basename.
pub fn node_label(n: &crate::scan::Node) -> String {
    if n.path.as_os_str() == Path::new("/").as_os_str() {
        "/".to_string()
    } else {
        n.name.clone()
    }
}

impl Menu {
    pub fn width(&self) -> u16 {
        let widest = self
            .items
            .iter()
            .map(|(_, label)| label.chars().count())
            .chain(std::iter::once(self.title.chars().count()))
            .max()
            .unwrap_or(0)
            .min(48);
        (widest as u16).saturating_add(4)
    }

    pub fn height(&self) -> u16 {
        self.items.len() as u16 + 2
    }
}

pub struct App {
    pub tree: Option<Tree>,
    /// Waste hits of `tree`, largest first. Computed once per tree.
    pub findings: Vec<usize>,
    /// Application findings of `tree`, largest first. Computed once per tree.
    pub databases: Vec<DbEntry>,
    pub cwd: Vec<usize>,
    pub selected: usize,
    pub list_offset: usize,
    pub findings_selected: usize,
    pub databases_selected: usize,
    pub collector: Collector,
    pub view: View,
    pub previous_view: View,
    pub apparent: bool,
    pub layout: Layout,
    pub progress: Option<Progress>,
    pub status: String,
    pub is_root: bool,
    pub last_click: Option<(Instant, u16, u16)>,
    pub quit: bool,
    pub scan_path: PathBuf,
    pub started: Instant,
    pub picker: Option<Picker>,
    /// Set by the picker; the event loop spawns the walk for this path.
    pub pending_scan: Option<PathBuf>,
    pub menu: Option<Menu>,
    pub hover: Option<Hover>,
    pub settings: Settings,
    pub settings_selected: usize,
    pub settings_edit: Option<String>,
    pub update_offer: Option<crate::update::UpdateOffer>,
    pub update_popup: bool,
    /// After leaving raw mode, download this tag and re-exec.
    pub pending_apply: Option<(String, &'static str)>,
}

impl App {
    pub fn new(scan_path: PathBuf, apparent: bool) -> Self {
        Self {
            tree: None,
            findings: Vec::new(),
            databases: Vec::new(),
            cwd: Vec::new(),
            selected: 0,
            list_offset: 0,
            findings_selected: 0,
            databases_selected: 0,
            collector: Collector::new(),
            view: View::Scanning,
            previous_view: View::Browse,
            apparent,
            layout: Layout::Sunburst,
            progress: None,
            status: String::new(),
            is_root: sys::running_as_root(),
            last_click: None,
            quit: false,
            scan_path,
            started: Instant::now(),
            picker: None,
            pending_scan: None,
            menu: None,
            hover: None,
            settings: Settings::load(),
            settings_selected: 0,
            settings_edit: None,
            update_offer: None,
            update_popup: false,
            pending_apply: None,
        }
    }

    /// Install a finished scan and derive everything cached from it.
    pub fn set_tree(&mut self, tree: Tree) {
        self.findings = waste_hits(&tree);
        self.databases = summarize(&tree);
        self.tree = Some(tree);
    }

    fn refresh_findings(&mut self) {
        self.findings = self.tree.as_ref().map(waste_hits).unwrap_or_default();
        self.databases = self.tree.as_ref().map(summarize).unwrap_or_default();
    }

    /// Length of the list the cursor lives in for this view.
    pub fn list_len(&self) -> usize {
        match self.view {
            View::Picker => self.picker.as_ref().map_or(0, |p| p.entries.len()),
            View::Findings => self.findings.len(),
            View::Databases => self.databases.len(),
            View::Collector => self.collector.len(),
            View::Browse | View::Confirm { .. } => self.current_children().len(),
            _ => 0,
        }
    }

    /// Put the cursor on row `i` of the list in view.
    pub fn select_row(&mut self, i: usize) {
        match self.view {
            View::Picker => {
                if let Some(p) = self.picker.as_mut() {
                    p.move_to(i);
                }
            }
            View::Findings => self.findings_selected = i,
            View::Databases => self.databases_selected = i,
            _ => self.selected = i,
        }
    }

    pub fn hovered_row(&self, i: usize) -> bool {
        self.hover == Some(Hover::Row(i))
    }

    /// Footer tooltip for a hovered slice: path · size · share of its parent.
    pub fn hover_line(&self) -> Option<String> {
        let Some(Hover::Slice(id)) = self.hover else {
            return None;
        };
        let tree = self.tree.as_ref()?;
        let node = tree.get(id);
        let size = node.display_size(self.apparent);
        let share = node.parent.map(|p| {
            let parent = tree.get(p);
            let total = parent.display_size(self.apparent).max(1);
            format!(
                "  ·  {:.1}% of {}",
                size as f64 * 100.0 / total as f64,
                node_label(parent)
            )
        });
        Some(format!(
            "{}  ·  {}{}",
            node.path.display(),
            human_bytes(size),
            share.unwrap_or_default()
        ))
    }

    /// Start in the directory picker instead of scanning straight away.
    pub fn open_picker(&mut self, dir: &Path) {
        match Picker::open(dir) {
            Ok(picker) => {
                self.picker = Some(picker);
                self.status.clear();
            }
            Err(e) => {
                self.picker = Picker::open(Path::new("/")).ok();
                self.status = e;
            }
        }
        self.view = View::Picker;
    }

    /// Reopen the picker from a scan, starting at the directory in view.
    /// The tree stays put so Esc can drop straight back into it.
    pub fn open_picker_from_scan(&mut self) {
        let start = match self.tree.as_ref() {
            Some(tree) => {
                let node = tree.get(tree.node_at(&self.cwd));
                if node.is_dir {
                    node.path.clone()
                } else {
                    node.path.parent().unwrap_or(&node.path).to_path_buf()
                }
            }
            None => self.scan_path.clone(),
        };
        self.menu = None;
        self.open_picker(&start);
    }

    /// Picker over a scan with staged marks: say what a new scan costs.
    pub fn picker_marks_warning(&self) -> Option<String> {
        if !matches!(self.view, View::Picker) || self.tree.is_none() || self.collector.is_empty() {
            return None;
        }
        let n = self.collector.len();
        Some(format!(
            "{n} marked item{} ({}) will be dropped by a new scan · Esc keeps them",
            if n == 1 { "" } else { "s" },
            human_bytes(self.collector.total_bytes())
        ))
    }

    /// Back to the scan the picker interrupted, if there is still one.
    pub fn resume_scan(&mut self) {
        if self.tree.is_some() {
            self.picker = None;
            self.status.clear();
            self.view = View::Browse;
        }
    }

    /// Apply a picker navigation result: errors become the status line.
    fn picker_apply(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.status.clear(),
            Err(e) => self.status = e,
        }
    }

    pub fn picker_enter(&mut self) {
        if let Some(r) = self.picker.as_mut().map(Picker::enter) {
            self.picker_apply(r);
        }
    }

    pub fn picker_up(&mut self) {
        if let Some(r) = self.picker.as_mut().map(Picker::up) {
            self.picker_apply(r);
        }
    }

    /// Queue the highlighted directory (or the current one) for the walker.
    pub fn scan_picked(&mut self) {
        if let Some(p) = self.picker.as_ref() {
            self.pending_scan = Some(p.scan_target().to_path_buf());
        }
    }

    /// Hand the walker a fresh root: reset every per-scan piece of state.
    pub fn begin_scan(&mut self, path: PathBuf) {
        self.scan_path = path;
        self.tree = None;
        self.findings.clear();
        self.databases.clear();
        self.collector.clear();
        self.progress = None;
        self.cwd.clear();
        self.selected = 0;
        self.findings_selected = 0;
        self.databases_selected = 0;
        self.list_offset = 0;
        self.picker = None;
        self.status.clear();
        self.started = Instant::now();
        self.view = View::Scanning;
    }

    /// Build the context menu for whatever the cursor is on. The caller has
    /// already moved the selection there, so every item acts on the selection.
    pub fn open_menu(&mut self, x: u16, y: u16) {
        let (title, items) = match self.view {
            View::Picker => self.picker_menu_items(),
            View::Browse | View::Findings | View::Collector => self.node_menu_items(),
            _ => return,
        };
        if items.is_empty() {
            return;
        }
        self.menu = Some(Menu {
            x,
            y,
            title,
            items,
            selected: 0,
        });
    }

    fn picker_menu_items(&self) -> (String, Vec<(MenuAction, String)>) {
        let Some(picker) = self.picker.as_ref() else {
            return (String::new(), Vec::new());
        };
        let entry = picker.selected_entry();
        // Name only: the footer already carries the full path.
        let title = match entry {
            Some(e) => e.name.clone(),
            None => picker.dir.display().to_string(),
        };
        let mut items = Vec::new();
        match entry {
            Some(e) if e.is_dir => {
                items.push((MenuAction::ScanHere, format!("Scan {}", e.name)));
                items.push((MenuAction::Open, format!("Open {}", e.name)));
            }
            _ => {
                items.push((MenuAction::ScanHere, "Scan this directory".to_string()));
            }
        }
        items.push((MenuAction::Cancel, "Cancel".to_string()));
        (title, items)
    }

    fn node_menu_items(&self) -> (String, Vec<(MenuAction, String)>) {
        let (Some(tree), Some(id)) = (self.tree.as_ref(), self.selected_node_id()) else {
            return (String::new(), Vec::new());
        };
        let node = tree.get(id);
        let title = format!(
            "{}  ·  {}",
            node.name,
            human_bytes(node.display_size(self.apparent))
        );
        let mut items = Vec::new();
        if matches!(self.view, View::Browse) && node.is_dir && !node.children.is_empty() {
            items.push((MenuAction::Open, "Open".to_string()));
        }
        let what = if node.is_dir { "directory" } else { "file" };
        if self.collector.contains_path(&node.path) {
            items.push((MenuAction::ToggleMark, "Remove from collector".to_string()));
        } else {
            items.push((MenuAction::ToggleMark, format!("Mark {what} for delete")));
        }
        // Delete commits the whole collector; say so when it is not just this item.
        let others = self.collector.len() - usize::from(self.collector.contains_path(&node.path));
        let delete_label = if others > 0 {
            format!("Delete {what}… (with {others} already marked)")
        } else {
            format!("Delete {what}…")
        };
        items.push((MenuAction::Delete, delete_label));
        items.push((MenuAction::Cancel, "Cancel".to_string()));
        (title, items)
    }

    pub fn menu_move(&mut self, delta: isize) {
        if let Some(menu) = self.menu.as_mut() {
            menu.selected = step(menu.selected, delta, menu.items.len());
        }
    }

    pub fn menu_select(&mut self, index: usize) {
        if let Some(menu) = self.menu.as_mut() {
            menu.selected = index.min(menu.items.len().saturating_sub(1));
        }
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// Run the highlighted menu item and close the menu.
    pub fn menu_activate(&mut self) {
        let Some(menu) = self.menu.take() else {
            return;
        };
        let Some(&(action, _)) = menu.items.get(menu.selected) else {
            return;
        };
        match action {
            MenuAction::Open => self.drill(),
            MenuAction::ScanHere => self.scan_picked(),
            MenuAction::ToggleMark => self.toggle_mark_selected(),
            MenuAction::Delete => self.delete_selected(),
            MenuAction::Cancel => {}
        }
    }

    /// Mark the selection if needed, then go straight to the confirm modal.
    fn delete_selected(&mut self) {
        let Some(path) = self
            .selected_node_id()
            .and_then(|id| Some(self.tree.as_ref()?.get(id).path.clone()))
        else {
            return;
        };
        if !self.collector.contains_path(&path) {
            self.toggle_mark_selected(); // refusals leave their reason in the status
        }
        if self.collector.contains_path(&path) {
            self.begin_confirm();
        }
    }

    /// Swap the browse map. Says which one it landed on, because the
    /// change is large enough that a silent toggle reads as a glitch.
    pub fn cycle_layout(&mut self) {
        self.layout = self.layout.next();
        self.status = format!("layout: {}", self.layout.label());
    }

    pub fn open_help(&mut self) {
        if matches!(self.view, View::Help) {
            return;
        }
        self.previous_view = self.view.clone();
        self.view = View::Help;
    }

    /// Leave the help overlay for whatever view opened it.
    pub fn close_help(&mut self) {
        self.view = self.previous_view.clone();
    }

    pub fn open_settings(&mut self) {
        self.previous_view = self.view.clone();
        self.settings_selected = 0;
        self.settings_edit = None;
        self.view = View::Settings;
    }

    pub fn close_settings(&mut self) {
        self.settings_edit = None;
        self.view = self.previous_view.clone();
    }

    pub fn offer_update(&mut self, offer: crate::update::UpdateOffer) {
        self.update_offer = Some(offer);
        self.update_popup = true;
    }

    pub fn dismiss_update(&mut self) {
        self.update_popup = false;
        self.update_offer = None;
    }

    pub fn accept_update(&mut self) {
        let Some(offer) = self.update_offer.as_ref() else {
            return;
        };
        if offer.writable {
            self.pending_apply = Some((offer.tag.clone(), offer.asset));
            self.update_popup = false;
            self.quit = true;
        } else {
            self.status = format!("not writable — {}", crate::update::installer_hint());
            self.update_popup = false;
        }
    }

    pub fn settings_move(&mut self, delta: isize) {
        self.settings_selected = step(self.settings_selected, delta, 2);
    }

    pub fn settings_cycle_theme(&mut self, delta: isize) {
        self.settings.cycle_theme(delta);
        let _ = crate::tui::theme::set(&self.settings.theme);
        self.save_settings_status();
    }

    pub fn settings_activate(&mut self) {
        if self.settings_selected == 0 {
            self.settings_cycle_theme(1);
        } else {
            self.settings_edit = Some(self.settings.export_dir.display().to_string());
            self.status = "editing export folder · Enter save · Esc cancel".into();
        }
    }

    pub fn settings_type(&mut self, ch: char) {
        if let Some(value) = &mut self.settings_edit {
            if !ch.is_control() {
                value.push(ch);
            }
        }
    }

    pub fn settings_backspace(&mut self) {
        if let Some(value) = &mut self.settings_edit {
            value.pop();
        }
    }

    pub fn settings_commit_edit(&mut self) {
        let Some(value) = self.settings_edit.take() else {
            return;
        };
        match self.settings.set_export_dir(&value) {
            Ok(()) => self.save_settings_status(),
            Err(e) => {
                self.status = e;
                self.settings_edit = Some(value);
            }
        }
    }

    fn save_settings_status(&mut self) {
        match self.settings.save() {
            Ok(path) => self.status = format!("settings saved to {}", path.display()),
            Err(e) => self.status = format!("settings changed for this session; save failed: {e}"),
        }
    }

    /// Animation frame for the scan spinner, ~8 fps.
    pub fn spin_frame(&self, frames: usize) -> usize {
        (self.started.elapsed().as_millis() / 120) as usize % frames
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    pub fn current_children(&self) -> &[usize] {
        match self.tree.as_ref() {
            Some(tree) => &tree.get(tree.node_at(&self.cwd)).children,
            None => &[],
        }
    }

    pub fn selected_id(&self) -> Option<usize> {
        self.current_children().get(self.selected).copied()
    }

    /// Node under the cursor in the current view (list or collector).
    pub fn selected_node_id(&self) -> Option<usize> {
        match self.view {
            View::Findings => self.findings.get(self.findings_selected).copied(),
            View::Databases => self.databases.get(self.databases_selected).map(|e| e.node),
            View::Collector => self.collector.items().get(self.selected).map(|i| i.node_id),
            View::Browse | View::Confirm { .. } => self.selected_id(),
            _ => None,
        }
    }

    pub fn selected_path(&self) -> Option<String> {
        let tree = self.tree.as_ref()?;
        let id = self.selected_node_id()?;
        Some(tree.get(id).path.display().to_string())
    }

    /// Hold every cursor inside its own list. Each view keeps a separate
    /// index, so they must be clamped against separate lengths.
    pub fn clamp_selection(&mut self) {
        self.selected = self
            .selected
            .min(self.current_children().len().saturating_sub(1));
        self.findings_selected = self
            .findings_selected
            .min(self.findings.len().saturating_sub(1));
        self.databases_selected = self
            .databases_selected
            .min(self.databases.len().saturating_sub(1));
    }

    /// Move the cursor of whatever list is in view; the picker keeps its own.
    pub fn move_sel(&mut self, delta: isize) {
        match self.view {
            View::Picker => {
                if let Some(p) = self.picker.as_mut() {
                    p.move_sel(delta);
                }
            }
            View::Browse | View::Findings | View::Collector | View::Databases => {
                let len = self.list_len();
                let idx = match self.view {
                    View::Findings => &mut self.findings_selected,
                    View::Databases => &mut self.databases_selected,
                    _ => &mut self.selected,
                };
                *idx = step(*idx, delta, len);
                self.list_offset = scroll_to_show(*idx, self.list_offset, LIST_PAGE);
            }
            _ => {}
        }
    }

    pub fn drill(&mut self) {
        match self.view {
            View::Findings => {
                if let Some(id) = self.finding_ids().get(self.findings_selected).copied() {
                    self.focus_node(id);
                    self.view = View::Browse;
                }
            }
            View::Browse => {
                let Some(id) = self.selected_id() else {
                    return;
                };
                let Some(tree) = self.tree.as_ref() else {
                    return;
                };
                if tree.get(id).is_dir && !tree.get(id).children.is_empty() {
                    self.cwd.push(self.selected);
                    self.selected = 0;
                    self.list_offset = 0;
                    self.status.clear();
                }
            }
            View::Databases => {
                if let Some(node) = self.databases.get(self.databases_selected).map(|e| e.node) {
                    self.focus_node(node);
                }
            }
            View::Picker => self.picker_enter(),
            _ => {}
        }
    }

    pub fn go_up(&mut self) {
        match self.view {
            View::Help => self.close_help(),
            View::Settings => self.close_settings(),
            View::Findings | View::Collector | View::Databases => {
                self.view = View::Browse;
            }
            View::Confirm { .. } => {
                self.view = View::Collector;
            }
            View::Browse => self.go_up_browse(),
            _ => {}
        }
    }

    /// Go up one directory, keeping the folder we left selected.
    pub fn go_up_browse(&mut self) {
        if let Some(pos) = self.cwd.pop() {
            self.selected = pos;
            self.list_offset = 0;
            self.status.clear();
        }
    }

    pub fn focus_node(&mut self, id: usize) {
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let chain = tree.ancestors(id);
        self.cwd.clear();
        self.selected = 0;
        let mut cur = chain[0];
        for &next in chain.iter().skip(1) {
            let Some(pos) = tree.nodes[cur].children.iter().position(|&c| c == next) else {
                break;
            };
            if next == id {
                self.selected = pos;
                break;
            }
            self.cwd.push(pos);
            cur = next;
        }
        self.list_offset = 0;
        self.view = View::Browse;
    }

    pub fn toggle_mark_selected(&mut self) {
        if !matches!(
            self.view,
            View::Browse | View::Findings | View::Collector | View::Databases
        ) {
            return;
        }
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let id = self.selected_node_id();
        let Some(id) = id else {
            return;
        };
        if id == tree.root {
            self.status = "cannot mark the scan root".into();
            return;
        }
        let n = tree.get(id);
        let item = CollectorItem {
            path: n.path.clone(),
            is_dir: n.is_dir,
            size_bytes: n.display_size(self.apparent),
            category: n.category,
            node_id: id,
            guard: n.guard,
        };
        let size = human_bytes(n.display_size(self.apparent));
        let name = n.name.clone();
        match self.collector.toggle(item) {
            Ok(true) => {
                self.status = format!(
                    "marked {name} ({size}) · {} in collector · c review · x delete",
                    self.collector.len()
                );
            }
            Ok(false) => {
                self.status = format!("unmarked {name} · {} in collector", self.collector.len());
            }
            Err(r) => {
                self.status = format!("refused: {}", r.reason);
            }
        }
    }

    pub fn finding_ids(&self) -> &[usize] {
        &self.findings
    }

    pub fn open_findings(&mut self) {
        self.previous_view = self.view.clone();
        self.view = View::Findings;
        self.findings_selected = 0;
        self.list_offset = 0;
        self.status.clear();
    }

    pub fn open_databases(&mut self) {
        self.previous_view = self.view.clone();
        self.view = View::Databases;
        self.databases_selected = 0;
        self.list_offset = 0;
        self.status = self.databases_status();
    }

    /// Headline for the databases view: what can be reclaimed without
    /// deleting anything that holds data.
    pub fn databases_status(&self) -> String {
        if self.databases.is_empty() {
            return "no PostgreSQL clusters or SQLite databases in this scan".into();
        }
        let reclaimable: u64 = self.databases.iter().map(|e| e.reclaimable).sum();
        format!(
            "{} entries · {} reclaimable without removing data",
            self.databases.len(),
            human_bytes(reclaimable)
        )
    }

    pub fn open_collector(&mut self) {
        self.previous_view = self.view.clone();
        self.view = View::Collector;
        self.selected = 0;
        self.list_offset = 0;
        self.status.clear();
    }

    pub fn begin_confirm(&mut self) {
        if self.collector.is_empty() {
            self.status = "collector is empty".into();
            return;
        }
        self.view = View::Confirm {
            typed: String::new(),
        };
        if needs_typed_confirm() {
            self.status = format!(
                "type {} to permanently delete {} ({})",
                crate::constants::DELETE_CONFIRM_PHRASE,
                self.collector.len(),
                human_bytes(self.collector.total_bytes())
            );
        } else {
            self.status = format!(
                "Enter to move {} ({}) to trash",
                self.collector.len(),
                human_bytes(self.collector.total_bytes())
            );
        }
    }

    pub fn confirm_type(&mut self, ch: char) {
        if let View::Confirm { typed } = &mut self.view {
            if ch.is_ascii() && !ch.is_control() {
                typed.push(ch);
            }
        }
    }

    pub fn confirm_backspace(&mut self) {
        if let View::Confirm { typed } = &mut self.view {
            typed.pop();
        }
    }

    pub fn commit_if_ready(&mut self) {
        let confirm = match &self.view {
            View::Confirm { typed } => {
                if needs_typed_confirm() {
                    Confirm::TypedPhrase(typed.clone())
                } else {
                    Confirm::TrashAndEnter
                }
            }
            _ => return,
        };
        match commit(&self.collector, &confirm) {
            Ok(result) => {
                if let Some(tree) = self.tree.as_mut() {
                    for path in &result.deleted {
                        if let Some(id) = tree.nodes.iter().position(|n| n.path == *path) {
                            tree.detach(id);
                        }
                    }
                }
                self.refresh_findings();
                let failed = result.failed.len();
                self.status = format!(
                    "deleted {} · {} failed{}",
                    result.deleted.len(),
                    failed,
                    crate::delete::delete_log_path()
                        .map(|p| format!(" · logged to {}", p.display()))
                        .unwrap_or_default()
                );
                self.collector.clear();
                self.view = View::Browse;
                self.clamp_selection();
            }
            Err(e) => {
                self.status = e;
            }
        }
    }

    pub fn export_current(&mut self) -> Result<PathBuf, String> {
        let tree = self
            .tree
            .as_ref()
            .ok_or_else(|| "scan is not finished".to_string())?;
        let id = tree.node_at(&self.cwd);
        std::fs::create_dir_all(&self.settings.export_dir)
            .map_err(|e| format!("cannot create export folder: {e}"))?;
        let dest = self.settings.export_dir.join(TUI_EXPORT_FILENAME);
        let n = write_csv(&dest, tree, id, &self.collector.paths_set())?;
        self.status = format!("wrote {n} rows to {}", dest.display());
        Ok(dest)
    }

    pub fn breadcrumb(&self) -> Vec<(String, usize)> {
        let Some(tree) = self.tree.as_ref() else {
            return vec![(self.scan_path.display().to_string(), 0)];
        };
        let id = tree.node_at(&self.cwd);
        tree.ancestors(id)
            .into_iter()
            .map(|nid| (node_label(tree.get(nid)), nid))
            .collect()
    }

    pub fn register_click(&mut self, x: u16, y: u16) -> bool {
        let now = Instant::now();
        let dbl = self
            .last_click
            .map(|(t, lx, ly)| lx == x && ly == y && now.duration_since(t) <= DOUBLE_CLICK)
            .unwrap_or(false);
        self.last_click = Some((now, x, y));
        dbl
    }

    pub fn not_root_hint(&self) -> Option<String> {
        if self.is_root {
            return None;
        }
        let errors = self
            .tree
            .as_ref()
            .map(|t| t.stats.errors + t.stats.permission_denied)
            .unwrap_or(0);
        Some(sys::not_privileged_hint(errors))
    }
}
