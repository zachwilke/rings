mod app;
mod draw;
mod picker;
mod sunburst;
mod theme;

use std::path::PathBuf;
use std::sync::mpsc;

use crate::scan::{scan_with_progress, WalkEvent, WalkOptions};
use crate::term::{self, Buffer, Event, Key, Term};

use self::app::{Action, App, View};
use self::draw::{row_index_at, HitMap};
use self::theme::BG;

const POLL_MS: i32 = 80;

/// `path` is the scan root. `None` opens the directory picker at the current
/// directory so `rings` alone browses before it walks.
pub fn run(path: Option<PathBuf>, opts: WalkOptions, apparent: bool) -> Result<(), String> {
    let start = path.clone().unwrap_or_else(|| PathBuf::from("."));
    let mut app = App::new(start, apparent);
    match path {
        Some(p) => app.pending_scan = Some(p),
        None => app.open_picker(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    }

    let term = Term::enter().map_err(|e| format!("cannot enter raw mode: {e}"))?;
    let result = event_loop(&term, &mut app, opts);
    drop(term);
    result
}

fn spawn_scan(path: PathBuf, opts: WalkOptions) -> mpsc::Receiver<WalkEvent> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || scan_with_progress(path, opts, tx));
    rx
}

fn event_loop(term: &Term, app: &mut App, opts: WalkOptions) -> Result<(), String> {
    let mut prev: Option<Buffer> = None;
    let mut hits = HitMap::empty();
    let mut dirty = true;
    let mut rx: Option<mpsc::Receiver<WalkEvent>> = None;

    loop {
        if let Some(path) = app.pending_scan.take() {
            rx = Some(spawn_scan(path.clone(), opts.clone()));
            app.begin_scan(path);
            dirty = true;
        }
        while let Some(Ok(ev)) = rx.as_ref().map(|r| r.try_recv()) {
            dirty = true;
            match ev {
                WalkEvent::Progress(p) => app.progress = Some(p),
                WalkEvent::Done(Ok(tree)) => {
                    if !app.is_root {
                        app.status = crate::sys::not_privileged_status(tree.stats.errors);
                    }
                    app.tree = Some(tree);
                    app.view = View::Browse;
                }
                WalkEvent::Done(Err(e)) => {
                    return Err(e);
                }
            }
        }

        let (w, h) = term.size();
        if prev.as_ref().is_none_or(|p| p.width != w || p.height != h) {
            dirty = true;
        }
        if dirty {
            let mut buf = Buffer::new(w, h, BG);
            hits = draw::draw(&mut buf, app);
            term::flush_diff(&buf, prev.as_ref()).map_err(|e| e.to_string())?;
            prev = Some(buf);
            dirty = false;
        }

        if app.quit {
            return Ok(());
        }

        for ev in term::poll_event(POLL_MS) {
            dirty = true;
            match ev {
                Event::Key(key) => handle_key(app, key),
                Event::Click { x, y } => handle_click(app, &hits, x, y),
                Event::RightClick { x, y } => handle_right_click(app, &hits, x, y),
            }
            if app.quit {
                return Ok(());
            }
        }
        if matches!(app.view, View::Scanning) {
            dirty = true; // keep the spinner alive
        }
    }
}

fn handle_key(app: &mut App, key: Key) {
    if app.menu.is_some() {
        match key {
            Key::Char('j') | Key::Down => app.menu_move(1),
            Key::Char('k') | Key::Up => app.menu_move(-1),
            Key::Enter => app.menu_activate(),
            _ => app.close_menu(),
        }
        return;
    }

    if matches!(app.view, View::Picker) {
        handle_picker_key(app, key);
        return;
    }

    if matches!(app.view, View::Confirm { .. }) {
        match key {
            Key::Esc => app.view = View::Collector,
            Key::Backspace => app.confirm_backspace(),
            Key::Enter => app.commit_if_ready(),
            Key::Char(c) => app.confirm_type(c),
            _ => {}
        }
        return;
    }

    if matches!(app.view, View::Help) {
        if matches!(
            key,
            Key::Esc | Key::Char('h') | Key::Char('q') | Key::Char('?') | Key::F1 | Key::Backspace
        ) {
            app.close_help();
        }
        return;
    }

    match key {
        Key::Char('q') => app.quit = true,
        Key::Char('?') | Key::F1 => app.open_help(),
        Key::Char('j') | Key::Down => app.move_sel(1),
        Key::Char('k') | Key::Up => app.move_sel(-1),
        Key::PageDown => app.move_sel(10),
        Key::PageUp => app.move_sel(-10),
        Key::Enter => app.drill(),
        Key::Backspace | Key::Left | Key::Char('h') => go_back(app),
        Key::Char(' ') | Key::Char('d') => app.toggle_mark_selected(),
        Key::Char('-') => app.open_picker_from_scan(),
        Key::Char('f') => app.open_findings(),
        Key::Char('c') => app.open_collector(),
        Key::Char('x') => {
            if matches!(app.view, View::Collector) {
                app.begin_confirm();
            }
        }
        Key::Char('e') => {
            if let Err(e) = app.export_current() {
                app.status = e;
            }
        }
        Key::Esc => {
            if !matches!(app.view, View::Browse | View::Scanning) {
                app.view = View::Browse;
            }
        }
        _ => {}
    }
}

/// Vim-style movement over the directory listing. `s` starts the scan.
fn handle_picker_key(app: &mut App, key: Key) {
    match key {
        Key::Char('q') => app.quit = true,
        Key::Char('?') | Key::F1 => app.open_help(),
        Key::Char('j') | Key::Down => app.picker_move(1),
        Key::Char('k') | Key::Up => app.picker_move(-1),
        Key::PageDown => app.picker_move(10),
        Key::PageUp => app.picker_move(-10),
        Key::Char('g') => app.picker_move(isize::MIN / 2),
        Key::Char('G') => app.picker_move(isize::MAX / 2),
        Key::Enter | Key::Char('l') | Key::Right => app.picker_enter(),
        Key::Char('h') | Key::Left | Key::Backspace => app.picker_up(),
        Key::Char('s') => app.scan_picked(),
        Key::Esc => app.resume_scan(),
        _ => {}
    }
}

fn go_back(app: &mut App) {
    match app.view {
        View::Picker => app.picker_up(),
        View::Browse => app.go_up_browse(),
        _ => app.go_up(),
    }
}

/// Right-click selects what is under the cursor, then opens the menu there.
fn handle_right_click(app: &mut App, hits: &HitMap, x: u16, y: u16) {
    app.close_menu();
    if !select_at(app, hits, x, y) {
        return;
    }
    app.open_menu(x, y);
}

/// Move the selection to the row or slice under the cursor. False when the
/// cursor is not over anything selectable.
fn select_at(app: &mut App, hits: &HitMap, x: u16, y: u16) -> bool {
    match app.view {
        View::Picker => {
            let Some(picker) = app.picker.as_ref() else {
                return false;
            };
            match row_index_at(hits.list, y, picker.offset, picker.entries.len()) {
                Some(i) => {
                    app.picker_select(i);
                    true
                }
                None => false,
            }
        }
        View::Browse | View::Confirm { .. } => {
            if let Some(slice) = sunburst::hit_slice(&hits.slices, hits.sunburst, x, y) {
                let node = slice.node;
                app.focus_node(node);
                return true;
            }
            match row_index_at(hits.list, y, app.list_offset, app.current_children().len()) {
                Some(i) => {
                    app.selected = i;
                    true
                }
                None => false,
            }
        }
        View::Findings => {
            match row_index_at(hits.list, y, app.list_offset, app.finding_ids().len()) {
                Some(i) => {
                    app.findings_selected = i;
                    true
                }
                None => false,
            }
        }
        View::Collector => match row_index_at(hits.list, y, app.list_offset, app.collector.len()) {
            Some(i) => {
                app.selected = i;
                true
            }
            None => false,
        },
        _ => false,
    }
}

fn handle_click(app: &mut App, hits: &HitMap, x: u16, y: u16) {
    if app.menu.is_some() {
        for (rect, index) in &hits.menu {
            if rect.contains(x, y) {
                app.menu_select(*index);
                app.menu_activate();
                return;
            }
        }
        app.close_menu();
        return;
    }
    if matches!(app.view, View::Help) {
        app.close_help();
        return;
    }
    let dbl = app.register_click(x, y);

    for (rect, action) in &hits.buttons {
        if rect.contains(x, y) {
            do_action(app, *action);
            return;
        }
    }
    for (rect, nid) in &hits.crumbs {
        if rect.contains(x, y) {
            let target = *nid;
            app.focus_node(target);
            if let Some(tree) = app.tree() {
                if tree.get(target).is_dir && tree.node_at(&app.cwd) != target {
                    app.drill();
                }
            }
            return;
        }
    }

    if !select_at(app, hits, x, y) || !dbl {
        return;
    }
    match app.view {
        View::Picker => app.picker_enter(),
        View::Browse | View::Findings | View::Confirm { .. } => app.drill(),
        _ => {}
    }
}

fn do_action(app: &mut App, action: Action) {
    match action {
        Action::Findings => app.open_findings(),
        Action::Collector => app.open_collector(),
        Action::Export => {
            if let Err(e) = app.export_current() {
                app.status = e;
            }
        }
        Action::Quit => app.quit = true,
        Action::Back => go_back(app),
        Action::ConfirmDelete => {
            if matches!(app.view, View::Confirm { .. }) {
                app.commit_if_ready();
            } else {
                app.begin_confirm();
            }
        }
        Action::Cancel => {
            app.view = View::Collector;
        }
        Action::Mark => app.toggle_mark_selected(),
        Action::Help => app.open_help(),
        Action::Scan => app.scan_picked(),
        Action::Picker => app.open_picker_from_scan(),
        Action::BackToScan => app.resume_scan(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn browse_view_shows_sunburst_and_child_list() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("big.dat"), vec![b'x'; 8192]).unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("inner"), vec![b'y'; 2048]).unwrap();

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert!(
            screen.contains("big.dat"),
            "child list should name the large file:\n{screen}"
        );
        assert!(
            screen.contains("subdir"),
            "child list should name the directory:\n{screen}"
        );
        assert!(screen.contains("rings"), "header:\n{screen}");
        assert!(
            screen.contains("? help"),
            "footer should hint ? help:\n{screen}"
        );
        let has_dots = screen.chars().any(sunburst::is_braille);
        assert!(has_dots, "sunburst should paint braille dots:\n{screen}");
        assert!(
            !hits.slices.is_empty(),
            "slices should exist for hit testing"
        );
        assert!(!hits.buttons.is_empty(), "footer buttons should exist");
    }

    #[test]
    fn click_selects_list_row_and_button_quits() {
        let tmp = tempfile::TempDir::new().unwrap();
        for i in 0..4 {
            fs::write(
                tmp.path().join(format!("f{i}.dat")),
                vec![b'x'; 4096 * (i + 1)],
            )
            .unwrap();
        }
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);

        // Click the second list row.
        let y = hits.list.y + 1;
        handle_click(&mut app, &hits, hits.list.x + 2, y);
        assert_eq!(app.selected, 1, "click should select the second row");

        // Click a slice: selection follows the sunburst.
        let slice_area = hits.sunburst;
        let cx = slice_area.x + slice_area.width / 2;
        let cy = slice_area.y + 1; // top of the disk, ring area
        handle_click(&mut app, &hits, cx, cy);

        // Click the Quit button.
        let quit = hits
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Quit)
            .expect("quit button");
        handle_click(&mut app, &hits, quit.0.x, quit.0.y);
        assert!(app.quit, "clicking Quit should set quit");
    }

    #[test]
    fn footer_chips_have_gaps_and_skip_hit_count() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.open_findings();

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert!(
            screen.contains("Temp & cache"),
            "findings header:\n{screen}"
        );
        assert!(
            screen.contains("hits"),
            "hit count belongs in the view header:\n{screen}"
        );
        assert!(
            !screen.contains("temp/cache/log hits"),
            "footer must not repeat the standing hit dump:\n{screen}"
        );
        assert!(
            screen.contains("? help"),
            "thin hint row still names ? help:\n{screen}"
        );
        let path = app.selected_path().expect("selected findings path");
        assert!(
            screen.contains(&path) || screen.contains(&draw::truncate(&path, 90)),
            "selected path should sit on the last footer line:\n{screen}"
        );

        let mut edges: Vec<(u16, u16)> =
            hits.buttons.iter().map(|(r, _)| (r.x, r.right())).collect();
        edges.sort_by_key(|(x, _)| *x);
        for pair in edges.windows(2) {
            assert!(
                pair[1].0 >= pair[0].1 + crate::constants::CHIP_GAP,
                "chips need a gap, got {} then {} (gap {})",
                pair[0].1,
                pair[1].0,
                pair[1].0.saturating_sub(pair[0].1)
            );
        }
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Quit),
            "Quit chip stays clickable"
        );
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Findings),
            "Temp & cache chip stays clickable"
        );
    }

    #[test]
    fn narrow_footer_drops_export_before_quit() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(48, 20, BG);
        let hits = draw::draw(&mut buf, &app);
        let actions: Vec<Action> = hits.buttons.iter().map(|(_, a)| *a).collect();
        assert!(
            actions.contains(&Action::Quit),
            "Quit is a keeper on a narrow row: {actions:?}"
        );
        assert!(
            !actions.contains(&Action::Export),
            "Export is first to drop when chips overflow: {actions:?}"
        );
        assert!(
            actions.contains(&Action::Findings),
            "Temp & cache stays: {actions:?}"
        );
    }

    #[test]
    fn help_overlay_lists_every_binding_and_the_logo() {
        let mut app = App::new(std::path::PathBuf::from("."), false);
        app.view = View::Help;
        let mut buf = Buffer::new(80, 24, BG);
        draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert!(screen.contains('◎'), "shared logo center:\n{screen}");
        assert!(screen.contains('╭'), "nested rings:\n{screen}");
        for needle in [
            "move selection",
            "drill into",
            "go up",
            "mark or unmark",
            "Temp & cache",
            "delete collector",
            "confirm delete",
            "export current view",
            "?  F1",
            "quit",
            "Mouse",
            "double-click",
        ] {
            assert!(screen.contains(needle), "help missing {needle}:\n{screen}");
        }
    }

    #[test]
    fn question_and_f1_toggle_help() {
        let mut app = App::new(std::path::PathBuf::from("."), false);
        app.view = View::Browse;
        handle_key(&mut app, Key::Char('?'));
        assert_eq!(app.view, View::Help);
        handle_key(&mut app, Key::F1);
        assert_eq!(app.view, View::Browse);
        handle_key(&mut app, Key::F1);
        assert_eq!(app.view, View::Help);
        handle_key(&mut app, Key::Esc);
        assert_eq!(app.view, View::Browse);
    }

    #[test]
    fn picker_lists_entries_and_offers_a_scan_chip() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("projects")).unwrap();
        fs::write(tmp.path().join("notes.txt"), b"hi").unwrap();

        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());
        assert_eq!(app.view, View::Picker);

        let mut buf = Buffer::new(90, 24, BG);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert!(
            screen.contains("Pick a directory to scan"),
            "picker title:\n{screen}"
        );
        assert!(screen.contains("projects/"), "directory row:\n{screen}");
        assert!(screen.contains("notes.txt"), "file row:\n{screen}");
        assert!(
            screen.contains("s scans"),
            "footer names the target:\n{screen}"
        );
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Scan),
            "a Scan chip is clickable"
        );
    }

    #[test]
    fn picker_navigates_and_s_queues_the_scan() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("projects").join("rings")).unwrap();

        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());

        handle_key(&mut app, Key::Enter); // into projects/
        assert!(
            app.picker.as_ref().unwrap().dir.ends_with("projects"),
            "Enter opens the highlighted directory"
        );

        handle_key(&mut app, Key::Char('s'));
        let queued = app.pending_scan.clone().expect("s queues a scan");
        assert!(
            queued.ends_with("rings"),
            "s scans the highlighted directory, got {}",
            queued.display()
        );
        app.pending_scan = None;

        handle_key(&mut app, Key::Char('h')); // back up
        assert!(app
            .picker
            .as_ref()
            .unwrap()
            .dir
            .ends_with(tmp.path().file_name().unwrap()));

        handle_key(&mut app, Key::Char('q'));
        assert!(app.quit);
    }

    #[test]
    fn clicking_a_picker_row_selects_and_double_click_opens() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("alpha").join("inner")).unwrap();
        fs::create_dir(tmp.path().join("beta")).unwrap();

        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());
        let mut buf = Buffer::new(90, 24, BG);
        let hits = draw::draw(&mut buf, &app);

        handle_click(&mut app, &hits, hits.list.x + 2, hits.list.y + 1);
        assert_eq!(app.picker.as_ref().unwrap().selected, 1, "second row");

        handle_click(&mut app, &hits, hits.list.x + 2, hits.list.y);
        handle_click(&mut app, &hits, hits.list.x + 2, hits.list.y);
        assert!(
            app.picker.as_ref().unwrap().dir.ends_with("alpha"),
            "double-click opens the directory"
        );
    }

    #[test]
    fn help_from_the_picker_returns_to_the_picker() {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());

        handle_key(&mut app, Key::Char('?'));
        assert_eq!(app.view, View::Help);
        handle_key(&mut app, Key::Esc);
        assert_eq!(app.view, View::Picker, "help returns where it came from");

        // A click anywhere closes the overlay too.
        handle_key(&mut app, Key::F1);
        let mut buf = Buffer::new(80, 24, BG);
        let hits = draw::draw(&mut buf, &app);
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Back),
            "help has a Close button"
        );
        handle_click(&mut app, &hits, 4, 4);
        assert_eq!(app.view, View::Picker);
    }

    #[test]
    fn right_click_opens_a_context_menu_that_marks_and_deletes() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        let row_y = hits.list.y + 1;
        handle_right_click(&mut app, &hits, hits.list.x + 3, row_y);

        let menu = app.menu.as_ref().expect("right-click opens a menu");
        let labels: Vec<String> = menu.items.iter().map(|(_, l)| l.clone()).collect();
        assert!(
            labels.iter().any(|l| l.starts_with("Mark ")),
            "menu marks the target: {labels:?}"
        );
        assert!(
            labels.iter().any(|l| l.starts_with("Delete ")),
            "menu deletes the target: {labels:?}"
        );
        assert_eq!(app.selected, 1, "right-click selects the row it opened on");

        // The menu paints over the view and its rows are clickable.
        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        assert!(!hits.menu.is_empty(), "menu rows are hit-tested");
        let screen = buf.text();
        assert!(screen.contains("Delete "), "menu is painted:\n{screen}");

        let marked_path = app.selected_path().expect("a selected path");
        let mark_row = hits
            .menu
            .iter()
            .find(|(_, i)| {
                matches!(
                    app.menu.as_ref().unwrap().items[*i].0,
                    crate::tui::app::MenuAction::Mark
                )
            })
            .expect("a Mark row")
            .0;
        handle_click(&mut app, &hits, mark_row.x + 1, mark_row.y);
        assert!(app.menu.is_none(), "activating an item closes the menu");
        assert!(
            app.collector
                .contains_path(std::path::Path::new(&marked_path)),
            "clicking Mark collects {marked_path}"
        );
    }

    #[test]
    fn context_menu_delete_marks_then_asks_to_confirm() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        let path = app.selected_path().expect("a selected path");

        let delete_i = app
            .menu
            .as_ref()
            .unwrap()
            .items
            .iter()
            .position(|(a, _)| *a == crate::tui::app::MenuAction::Delete)
            .expect("a Delete item");
        app.menu_select(delete_i);
        app.menu_activate();

        assert!(
            matches!(app.view, View::Confirm { .. }),
            "Delete goes to the confirm modal, never straight to unlink"
        );
        assert!(
            app.collector.contains_path(std::path::Path::new(&path)),
            "the target is in the collector, still on disk"
        );
        assert!(
            std::path::Path::new(&path).exists(),
            "nothing is removed before the confirm"
        );
    }

    #[test]
    fn escape_closes_the_context_menu_without_acting() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        assert!(app.menu.is_some());

        handle_key(&mut app, Key::Esc);
        assert!(app.menu.is_none(), "Esc closes the menu");
        assert!(app.collector.is_empty(), "and marks nothing");
    }

    #[test]
    fn right_click_in_the_picker_offers_scan_and_open() {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("data")).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());

        let mut buf = Buffer::new(90, 24, BG);
        let hits = draw::draw(&mut buf, &app);
        handle_right_click(&mut app, &hits, hits.list.x + 2, hits.list.y);

        let menu = app.menu.as_ref().expect("picker menu");
        let actions: Vec<crate::tui::app::MenuAction> =
            menu.items.iter().map(|(a, _)| *a).collect();
        assert!(actions.contains(&crate::tui::app::MenuAction::ScanHere));
        assert!(actions.contains(&crate::tui::app::MenuAction::Open));

        app.menu_select(0);
        app.menu_activate();
        assert!(
            app.pending_scan.as_ref().unwrap().ends_with("data"),
            "Scan queues the directory the menu opened on"
        );
    }

    #[test]
    fn dash_reopens_the_picker_at_the_directory_in_view_and_esc_returns() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;
        app.drill(); // into the largest child

        let browsing = app
            .tree()
            .map(|t| t.get(t.node_at(&app.cwd)).path.clone())
            .unwrap();
        handle_key(&mut app, Key::Char('-'));
        assert_eq!(app.view, View::Picker);
        assert_eq!(
            app.picker.as_ref().unwrap().dir,
            std::path::absolute(&browsing).unwrap(),
            "the picker opens where we were browsing"
        );
        assert!(app.tree.is_some(), "the scan is kept while we look around");

        handle_key(&mut app, Key::Esc);
        assert_eq!(app.view, View::Browse, "Esc drops back into the scan");
        assert!(!app.cwd.is_empty(), "and keeps the place we had drilled to");
    }

    #[test]
    fn picker_from_a_scan_offers_back_to_scan_and_can_rescan() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;
        handle_key(&mut app, Key::Char('-'));

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        let back = hits
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::BackToScan)
            .expect("a Back to scan chip once a tree exists");
        assert!(
            buf.text().contains("Back to scan"),
            "the way back is named in the footer:\n{}",
            buf.text()
        );

        handle_click(&mut app, &hits, back.0.x, back.0.y);
        assert_eq!(app.view, View::Browse);

        // Picking a different directory starts a fresh scan.
        handle_key(&mut app, Key::Char('-'));
        handle_key(&mut app, Key::Char('s'));
        assert!(app.pending_scan.is_some(), "s queues the new root");
    }

    #[test]
    fn browse_footer_keeps_the_picker_chip_and_the_help_hint() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Picker),
            "the Picker chip is clickable from a scan"
        );
        assert!(
            buf.text().contains("? help"),
            "the standing hint survives the extra chip:\n{}",
            buf.text()
        );
    }

    fn write_nested_fixture(root: &std::path::Path) {
        let dirs = [
            "usr/lib/x11",
            "usr/share/doc",
            "usr/share/man",
            "usr/bin",
            "var/log/journal",
            "var/cache/apt",
            "var/tmp",
            "home/alice/.cache",
            "home/alice/src/rings/target",
            "home/bob",
            "etc",
        ];
        for d in dirs {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let files: &[(&str, usize)] = &[
            ("usr/lib/x11/libX11.so", 9000),
            ("usr/lib/x11/libXext.so", 4000),
            ("usr/lib/libfoo.so", 400),
            ("usr/lib/libbar.so", 300),
            ("usr/lib/libtiny.so", 80),
            ("usr/share/doc/readme", 5000),
            ("usr/share/man/ls.1", 3500),
            ("usr/share/misc", 1800),
            ("usr/bin/ls", 6000),
            ("var/log/journal/system.journal", 7000),
            ("var/log/syslog", 2500),
            ("var/log/kern.log", 700),
            ("var/cache/apt/pkg", 4500),
            ("var/cache/font", 2200),
            ("var/tmp/sess", 1800),
            ("var/tmp/upload", 1600),
            ("home/alice/.cache/thumb", 3500),
            ("home/alice/src/rings/target/debug", 2200),
            ("home/bob/notes", 3500),
            ("etc/passwd", 3500),
            ("crash.core", 1800),
        ];
        for (path, n) in files {
            std::fs::write(root.join(path), vec![b'x'; *n]).unwrap();
        }
    }

    fn write_ppm(buf: &Buffer, path: &std::path::Path, scale: u32) {
        const COLS: u32 = 2;
        const ROWS: u32 = 4;
        let w = buf.width as u32 * COLS * scale;
        let h = buf.height as u32 * ROWS * scale;
        let mut px = vec![0u8; (w * h * 3) as usize];
        for y in 0..buf.height {
            for x in 0..buf.width {
                let cell = buf
                    .get(x, y)
                    .cloned()
                    .unwrap_or(crate::term::Cell::blank(BG));
                let dots = sunburst::raster_cell(cell);
                for row in 0..ROWS as usize {
                    for col in 0..COLS as usize {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                let i = ((((y as u32 * ROWS + row as u32) * scale + sy) * w
                                    + (x as u32 * COLS + col as u32) * scale
                                    + sx)
                                    * 3) as usize;
                                let c = dots[row][col];
                                px[i] = c.0;
                                px[i + 1] = c.1;
                                px[i + 2] = c.2;
                            }
                        }
                    }
                }
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend(px);
        std::fs::write(path, out).unwrap();
    }

    fn dump_view_ppm(app: &App, stem: &str) -> (String, String) {
        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, app);
        let dir = std::env::temp_dir();
        let ppm = dir.join(format!("rings-{stem}-current.ppm"));
        write_ppm(&buf, &ppm, 3);
        let text = dir.join(format!("rings-{stem}-current.txt"));
        std::fs::write(&text, buf.text()).unwrap();
        eprintln!(
            "{stem} dump {} slices rings_for={} → {} and {}",
            hits.slices.len(),
            sunburst::rings_for(hits.sunburst),
            ppm.display(),
            text.display()
        );
        (buf.text(), text.display().to_string())
    }

    #[test]
    fn dump_browse_view_ppm() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.tree = Some(tree);
        app.view = View::Browse;
        app.selected = 1;
        dump_view_ppm(&app, "browse");
        app.open_findings();
        dump_view_ppm(&app, "findings");
        let mut wide = Buffer::new(140, 28, BG);
        draw::draw(&mut wide, &app);
        let wide_ppm = std::env::temp_dir().join("rings-findings-wide-current.ppm");
        write_ppm(&wide, &wide_ppm, 3);
        let wide_path = std::env::temp_dir().join("rings-findings-wide-current.txt");
        std::fs::write(&wide_path, wide.text()).unwrap();
        eprintln!(
            "wide findings dump → {} and {}",
            wide_ppm.display(),
            wide_path.display()
        );
    }

    #[test]
    fn first_paint_shows_the_shared_logo() {
        let mut app = App::new(std::path::PathBuf::from("/var"), false);
        app.view = View::Scanning;
        let mut buf = Buffer::new(80, 24, BG);
        draw::draw(&mut buf, &app);
        let screen = buf.text();
        assert!(
            screen.contains('◎'),
            "scan first paint should show the logo:\n{screen}"
        );
        assert!(
            screen.contains('╭'),
            "nested rings on first paint:\n{screen}"
        );
    }
}
