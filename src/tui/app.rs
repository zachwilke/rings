use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::constants::{DOUBLE_CLICK, TUI_EXPORT_FILENAME};
use crate::csv_export::write_csv;
use crate::delete::{
    commit, needs_typed_confirm, Collector, CollectorItem, Confirm,
};
use crate::dto::waste_hits;
use crate::scan::{Progress, Tree};
use crate::size::human_bytes;
use crate::unix;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Scanning,
    Browse,
    Findings,
    Collector,
    Confirm { typed: String },
    Help,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Findings,
    Collector,
    Export,
    Quit,
    Back,
    ConfirmDelete,
    Cancel,
    Mark,
    Help,
}

pub struct App {
    pub tree: Option<Tree>,
    pub cwd: Vec<usize>,
    pub selected: usize,
    pub list_offset: usize,
    pub findings_selected: usize,
    pub collector: Collector,
    pub view: View,
    pub previous_view: View,
    pub apparent: bool,
    pub progress: Option<Progress>,
    pub status: String,
    pub is_root: bool,
    pub last_click: Option<(Instant, u16, u16)>,
    pub quit: bool,
    pub scan_path: PathBuf,
    pub started: Instant,
}

impl App {
    pub fn new(scan_path: PathBuf, apparent: bool) -> Self {
        Self {
            tree: None,
            cwd: Vec::new(),
            selected: 0,
            list_offset: 0,
            findings_selected: 0,
            collector: Collector::new(),
            view: View::Scanning,
            previous_view: View::Browse,
            apparent,
            progress: None,
            status: String::new(),
            is_root: unix::running_as_root(),
            last_click: None,
            quit: false,
            scan_path,
            started: Instant::now(),
        }
    }

    /// Animation frame for the scan spinner, ~8 fps.
    pub fn spin_frame(&self, frames: usize) -> usize {
        (self.started.elapsed().as_millis() / 120) as usize % frames
    }

    pub fn tree(&self) -> Option<&Tree> {
        self.tree.as_ref()
    }

    pub fn current_children(&self) -> Vec<usize> {
        let Some(tree) = self.tree.as_ref() else {
            return Vec::new();
        };
        let id = tree.node_at(&self.cwd);
        tree.get(id).children.clone()
    }

    pub fn selected_id(&self) -> Option<usize> {
        let kids = self.current_children();
        kids.get(self.selected).copied()
    }

    pub fn clamp_selection(&mut self) {
        let n = match self.view {
            View::Findings => self.finding_ids().len(),
            View::Collector => self.collector.len(),
            _ => self.current_children().len(),
        };
        if n == 0 {
            self.selected = 0;
            self.findings_selected = 0;
            return;
        }
        if self.selected >= n {
            self.selected = n - 1;
        }
        if self.findings_selected >= n {
            self.findings_selected = n - 1;
        }
    }

    pub fn move_sel(&mut self, delta: isize) {
        let n = match self.view {
            View::Findings => self.finding_ids().len(),
            View::Collector => self.collector.len(),
            View::Confirm { .. } | View::Help | View::Scanning => return,
            _ => self.current_children().len(),
        };
        if n == 0 {
            return;
        }
        let idx = match self.view {
            View::Findings => &mut self.findings_selected,
            _ => &mut self.selected,
        };
        let next = *idx as isize + delta;
        *idx = next.clamp(0, (n as isize) - 1) as usize;
        self.ensure_visible(8);
    }

    fn ensure_visible(&mut self, page: usize) {
        let sel = match self.view {
            View::Findings => self.findings_selected,
            _ => self.selected,
        };
        if sel < self.list_offset {
            self.list_offset = sel;
        } else if sel >= self.list_offset + page {
            self.list_offset = sel.saturating_sub(page.saturating_sub(1));
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
            _ => {}
        }
    }

    pub fn go_up(&mut self) {
        match self.view {
            View::Findings | View::Collector | View::Help => {
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
        let Some(tree) = self.tree.as_ref() else {
            return;
        };
        let id = match self.view {
            View::Findings => self.finding_ids().get(self.findings_selected).copied(),
            View::Collector => self.collector.items().get(self.selected).map(|i| i.node_id),
            View::Browse => self.selected_id(),
            _ => None,
        };
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
        };
        match self.collector.toggle(item) {
            Ok(true) => {
                self.status = format!("marked {} for delete", n.path.display());
            }
            Ok(false) => {
                self.status = format!("unmarked {}", n.path.display());
            }
            Err(r) => {
                self.status = format!("refused: {}", r.reason);
            }
        }
    }

    pub fn finding_ids(&self) -> Vec<usize> {
        match self.tree.as_ref() {
            Some(t) => waste_hits(t),
            None => Vec::new(),
        }
    }

    pub fn open_findings(&mut self) {
        self.previous_view = self.view.clone();
        self.view = View::Findings;
        self.findings_selected = 0;
        self.list_offset = 0;
        let n = self.finding_ids().len();
        self.status = format!("{n} temp/cache/log hits — nothing deleted");
    }

    pub fn open_collector(&mut self) {
        self.previous_view = self.view.clone();
        self.view = View::Collector;
        self.selected = 0;
        self.list_offset = 0;
        self.status = format!(
            "collector · {} · {}",
            self.collector.len(),
            human_bytes(self.collector.total_bytes())
        );
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
                let failed = result.failed.len();
                self.status = format!(
                    "deleted {} · {} failed · logged to stderr{}",
                    result.deleted.len(),
                    failed,
                    crate::delete::delete_log_path()
                        .map(|p| format!(" and {}", p.display()))
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
        let tree = self.tree.as_ref().ok_or_else(|| "scan is not finished".to_string())?;
        let id = tree.node_at(&self.cwd);
        let dest = PathBuf::from(TUI_EXPORT_FILENAME);
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
            .map(|nid| {
                let n = tree.get(nid);
                let label = if n.path.as_os_str() == Path::new("/").as_os_str() {
                    "/".to_string()
                } else {
                    n.name.clone()
                };
                (label, nid)
            })
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
        Some(format!(
            "not root — sudo rings / for a full-disk scan{}",
            if errors > 0 {
                format!(" ({errors} unreadable)")
            } else {
                String::new()
            }
        ))
    }
}
