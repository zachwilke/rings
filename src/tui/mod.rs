mod app;
mod draw;
mod icicle;
mod picker;
mod sunburst;
pub mod theme;

use std::path::PathBuf;
use std::sync::mpsc;

use crate::scan::{spawn_scan, WalkEvent, WalkOptions};
use crate::term::{self, Buffer, Event, Key, Rect, Term};

use self::app::{Action, App, Hover, Layout, View};
use self::draw::HitMap;

const POLL_MS: i32 = 80;

/// `path` is the scan root. `None` opens the directory picker at the current
/// directory so `rings` alone browses before it walks.
pub fn run(path: Option<PathBuf>, opts: WalkOptions, apparent: bool) -> Result<(), String> {
    let mut app = App::new(PathBuf::from("."), apparent);
    match path {
        Some(p) => app.pending_scan = Some(p),
        None => app.open_picker(&std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))),
    }

    term::set_color_depth(term::detect_color_depth());
    let term = Term::enter().map_err(|e| format!("cannot enter raw mode: {e}"))?;
    let result = event_loop(&term, &mut app, opts);
    drop(term);
    result
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
                    app.set_tree(tree);
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
            let mut buf = Buffer::new(w, h, theme::current().bg);
            hits = draw::draw(&mut buf, app);
            term::flush_diff(&buf, prev.as_ref()).map_err(|e| e.to_string())?;
            prev = Some(buf);
            dirty = false;
        }

        if app.quit {
            return Ok(());
        }

        // Motion floods; only the last position matters, and only a change repaints.
        let mut pointer = None;
        for ev in term::poll_event(POLL_MS) {
            match ev {
                Event::Move { x, y } => {
                    pointer = Some((x, y));
                    continue;
                }
                Event::Key(key) => handle_key(app, key),
                Event::Click { x, y } => handle_click(app, &hits, x, y),
                Event::RightClick { x, y } => handle_right_click(app, &hits, x, y),
                Event::Wheel { delta } => handle_wheel(app, delta),
            }
            dirty = true;
            if app.quit {
                return Ok(());
            }
        }
        if let Some((x, y)) = pointer {
            let hover = target_at(app, &hits, x, y);
            if hover != app.hover {
                app.hover = hover;
                dirty = true;
            }
        }
        if matches!(app.view, View::Scanning) {
            dirty = true; // keep the spinner alive
        }
    }
}

/// Keys every list view shares: quit, help, and cursor movement.
fn handle_common_key(app: &mut App, key: Key) -> bool {
    match key {
        Key::Char('q') => app.quit = true,
        Key::Char('?') | Key::F1 => app.open_help(),
        Key::Char('j') | Key::Down => app.move_sel(1),
        Key::Char('k') | Key::Up => app.move_sel(-1),
        Key::PageDown => app.move_sel(10),
        Key::PageUp => app.move_sel(-10),
        Key::Char('g') => app.move_sel(isize::MIN / 2),
        Key::Char('G') => app.move_sel(isize::MAX / 2),
        _ => return false,
    }
    true
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

    if handle_common_key(app, key) {
        return;
    }

    if matches!(app.view, View::Picker) {
        // Vim-style browsing over the directory listing. `s` starts the scan.
        match key {
            Key::Enter | Key::Char('l') | Key::Right => app.picker_enter(),
            Key::Char('h') | Key::Left | Key::Backspace => app.picker_up(),
            Key::Char('s') => app.scan_picked(),
            Key::Esc => app.resume_scan(),
            _ => {}
        }
        return;
    }

    match key {
        Key::Enter => app.drill(),
        Key::Backspace | Key::Left | Key::Char('h') => go_back(app),
        Key::Char(' ') | Key::Char('d') => app.toggle_mark_selected(),
        Key::Char('-') => app.open_picker_from_scan(),
        Key::Char('f') => app.open_findings(),
        Key::Char('L') => app.cycle_layout(),
        Key::Char('b') => {
            if matches!(app.view, View::Databases) {
                app.view = View::Browse;
            } else {
                app.open_databases();
            }
        }
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

fn go_back(app: &mut App) {
    match app.view {
        View::Picker => app.picker_up(),
        View::Browse => app.go_up_browse(),
        _ => app.go_up(),
    }
}

/// Wheel moves the cursor of whatever list is in view, like j/k.
fn handle_wheel(app: &mut App, delta: i8) {
    if app.menu.is_some() {
        app.menu_move(delta as isize);
    } else {
        app.move_sel(delta as isize);
    }
}

/// What is under the pointer. One precedence walk serves hover, click, and
/// right-click: an open menu takes everything, then buttons, breadcrumbs,
/// slices, and list rows. Views that are not clickable simply record no
/// targets, so a modal needs no special case here.
fn target_at(app: &App, hits: &HitMap, x: u16, y: u16) -> Option<Hover> {
    let find = |rects: &[(Rect, usize)]| {
        rects
            .iter()
            .find(|(r, _)| r.contains(x, y))
            .map(|(_, i)| *i)
    };
    if app.menu.is_some() {
        return find(&hits.menu).map(Hover::Menu);
    }
    if let Some((_, a)) = hits.buttons.iter().find(|(r, _)| r.contains(x, y)) {
        return Some(Hover::Button(*a));
    }
    if let Some(n) = find(&hits.crumbs) {
        return Some(Hover::Crumb(n));
    }
    // Both layouts hand back the same `Slice` list; only the projection
    // that put it on screen has to be undone.
    let slice = match hits.layout {
        Layout::Sunburst => sunburst::hit_slice(&hits.slices, hits.map, x, y),
        Layout::Icicle => icicle::hit_slice(&hits.slices, hits.map, x, y),
    };
    if let Some(slice) = slice {
        return Some(Hover::Slice(slice.node));
    }
    find(&hits.rows).map(Hover::Row)
}

/// Move the selection to the slice or row under the cursor.
fn select_target(app: &mut App, target: Hover) -> bool {
    match target {
        Hover::Slice(node) => app.focus_node(node),
        Hover::Row(i) => app.select_row(i),
        _ => return false,
    }
    true
}

/// Right-click selects what is under the cursor, then opens the menu there.
fn handle_right_click(app: &mut App, hits: &HitMap, x: u16, y: u16) {
    app.close_menu();
    let Some(target) = target_at(app, hits, x, y) else {
        return;
    };
    if select_target(app, target) {
        app.open_menu(x, y);
    }
}

fn handle_click(app: &mut App, hits: &HitMap, x: u16, y: u16) {
    if app.menu.is_some() {
        if let Some(Hover::Menu(i)) = target_at(app, hits, x, y) {
            app.menu_select(i);
            app.menu_activate();
        } else {
            app.close_menu();
        }
        return;
    }
    if matches!(app.view, View::Help) {
        app.close_help();
        return;
    }
    let dbl = app.register_click(x, y);
    match target_at(app, hits, x, y) {
        Some(Hover::Button(action)) => do_action(app, action),
        Some(Hover::Crumb(target)) => {
            app.focus_node(target);
            if let Some(tree) = app.tree() {
                if tree.get(target).is_dir && tree.node_at(&app.cwd) != target {
                    app.drill();
                }
            }
        }
        Some(target) => {
            if select_target(app, target) && dbl {
                app.drill();
            }
        }
        None => {}
    }
}

fn do_action(app: &mut App, action: Action) {
    match action {
        Action::Findings => app.open_findings(),
        Action::Databases => app.open_databases(),
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        fs::write(tmp.path().join("big.dat"), vec![b'x'; 8192]).unwrap();
        fs::create_dir(tmp.path().join("subdir")).unwrap();
        fs::write(tmp.path().join("subdir").join("inner"), vec![b'y'; 2048]).unwrap();

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
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
        let th = theme::current();
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
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);

        // Click the second list row.
        let y = hits.list.y + 1;
        handle_click(&mut app, &hits, hits.list.x + 2, y);
        assert_eq!(app.selected, 1, "click should select the second row");

        // Click a slice: selection follows the sunburst.
        let slice_area = hits.map;
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.open_findings();

        let mut buf = Buffer::new(100, 28, th.bg);
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
            screen.contains(&path) || screen.contains(draw::truncate(&path, 90).as_ref()),
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(48, 20, th.bg);
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
        let th = theme::current();
        let mut app = App::new(std::path::PathBuf::from("."), false);
        app.view = View::Help;

        // Roomy: logo plus every group in two columns.
        let mut buf = Buffer::new(100, 34, th.bg);
        draw::draw(&mut buf, &app);
        let screen = buf.text();
        assert!(screen.contains('◎'), "shared logo center:\n{screen}");
        assert!(screen.contains('╭'), "rounded chrome:\n{screen}");
        for group in crate::cli::KEY_GROUPS {
            assert!(
                screen.contains(group.title),
                "missing {}:\n{screen}",
                group.title
            );
            for (key, desc) in group.keys {
                assert!(screen.contains(desc), "missing {desc:?}:\n{screen}");
                assert!(screen.contains(key), "missing {key:?}:\n{screen}");
            }
        }
        assert!(screen.contains("Close"), "close chip:\n{screen}");

        // 80×24 over SSH: the logo yields so every binding still fits.
        let mut buf = Buffer::new(80, 24, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();
        assert!(
            !screen.contains('◎'),
            "logo gives way when short:\n{screen}"
        );
        for group in crate::cli::KEY_GROUPS {
            for (_, desc) in group.keys {
                assert!(screen.contains(desc), "80x24 missing {desc:?}:\n{screen}");
            }
        }
        assert!(hits.buttons.iter().any(|(_, a)| *a == Action::Back));
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("projects")).unwrap();
        fs::write(tmp.path().join("notes.txt"), b"hi").unwrap();

        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());
        assert_eq!(app.view, View::Picker);

        let mut buf = Buffer::new(90, 24, th.bg);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("alpha").join("inner")).unwrap();
        fs::create_dir(tmp.path().join("beta")).unwrap();

        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());
        let mut buf = Buffer::new(90, 24, th.bg);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());

        handle_key(&mut app, Key::Char('?'));
        assert_eq!(app.view, View::Help);
        handle_key(&mut app, Key::Esc);
        assert_eq!(app.view, View::Picker, "help returns where it came from");

        // A click anywhere closes the overlay too.
        handle_key(&mut app, Key::F1);
        let mut buf = Buffer::new(80, 24, th.bg);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
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
        let mut buf = Buffer::new(100, 28, th.bg);
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
                    crate::tui::app::MenuAction::ToggleMark
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        assert!(app.menu.is_some());

        handle_key(&mut app, Key::Esc);
        assert!(app.menu.is_none(), "Esc closes the menu");
        assert!(app.collector.is_empty(), "and marks nothing");
    }

    #[test]
    fn right_click_in_the_picker_offers_scan_and_open() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("data")).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());

        let mut buf = Buffer::new(90, 24, th.bg);
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
        app.set_tree(tree);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        handle_key(&mut app, Key::Char('-'));

        let mut buf = Buffer::new(100, 28, th.bg);
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
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
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

    #[test]
    fn wheel_moves_the_cursor_like_j_and_k() {
        let tmp = tempfile::TempDir::new().unwrap();
        for d in ["a", "b", "c"] {
            fs::create_dir(tmp.path().join(d)).unwrap();
        }
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.open_picker(tmp.path());
        handle_wheel(&mut app, 1);
        handle_wheel(&mut app, 1);
        assert_eq!(app.picker.as_ref().unwrap().selected, 2);
        handle_wheel(&mut app, -1);
        assert_eq!(app.picker.as_ref().unwrap().selected, 1);

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        app.set_tree(tree);
        app.view = View::Browse;
        handle_wheel(&mut app, 1);
        assert_eq!(app.selected, 1, "browse list scrolls too");
    }

    #[test]
    fn hover_highlights_a_row_without_selecting_it() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let (hx, hy) = (hits.list.x + 3, hits.list.y + 2);
        let hover = target_at(&app, &hits, hx, hy);
        assert_eq!(hover, Some(Hover::Row(2)));
        app.hover = hover;
        assert_eq!(app.selected, 0, "hover never moves the selection");

        let mut buf = Buffer::new(100, 28, th.bg);
        draw::draw(&mut buf, &app);
        assert_eq!(
            buf.get(hx, hy).unwrap().bg,
            th.hover_bg(),
            "hovered row tint"
        );
        assert_eq!(
            buf.get(hx, hits.list.y).unwrap().bg,
            th.select_bg,
            "selected row keeps its own color"
        );

        // Outside every hit target there is nothing to hover.
        assert_eq!(target_at(&app, &hits, 0, hits.list.y + 20), None);
    }

    #[test]
    fn hover_over_a_slice_puts_a_tooltip_in_the_footer() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let slice = hits.slices.iter().find(|s| !s.grouped).expect("a slice");
        // Hover the slice via the hit map rather than guessing a pixel.
        app.hover = Some(Hover::Slice(slice.node));
        let tip = app.hover_line().expect("tooltip");
        assert!(tip.contains("% of"), "{tip}");
        let mut buf = Buffer::new(100, 28, th.bg);
        draw::draw(&mut buf, &app);
        assert!(
            buf.text().contains("% of"),
            "footer shows the tooltip:\n{}",
            buf.text()
        );
    }

    #[test]
    fn chips_and_menu_rows_react_to_hover() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let quit = hits
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Quit)
            .unwrap()
            .0;
        app.hover = target_at(&app, &hits, quit.x + 1, quit.y);
        assert_eq!(app.hover, Some(Hover::Button(Action::Quit)));
        let mut buf = Buffer::new(100, 28, th.bg);
        draw::draw(&mut buf, &app);
        assert_eq!(buf.get(quit.x + 1, quit.y).unwrap().bg, th.select_bg);

        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let row = hits.menu[1].0;
        app.hover = target_at(&app, &hits, row.x + 1, row.y);
        assert_eq!(app.hover, Some(Hover::Menu(1)));
    }

    #[test]
    fn every_theme_renders_the_browse_view() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        for name in theme::names() {
            theme::set(name).unwrap();
            let th = theme::current();
            let mut app = App::new(tmp.path().to_path_buf(), false);
            app.set_tree(tree.clone());
            app.view = View::Browse;
            let mut buf = Buffer::new(100, 28, th.bg);
            let hits = draw::draw(&mut buf, &app);
            assert!(!hits.slices.is_empty(), "{name}: slices");
            assert!(buf.text().contains("usr"), "{name}: list");
        }
        theme::set("rings").unwrap();
    }

    #[test]
    fn confirm_modal_swallows_clicks_that_miss_its_buttons() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        app.toggle_mark_selected();
        app.begin_confirm();
        assert!(matches!(app.view, View::Confirm { .. }));

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let before = (app.selected, app.cwd.clone());

        // The typed-phrase box sits over the sunburst; clicking it must not
        // select a slice, and double-clicking must not drill.
        let cx = hits.map.x + hits.map.width / 2;
        let cy = hits.map.y + hits.map.height / 2;
        handle_click(&mut app, &hits, cx, cy);
        handle_click(&mut app, &hits, cx, cy);
        assert!(matches!(app.view, View::Confirm { .. }), "still confirming");
        assert_eq!(
            (app.selected, app.cwd.clone()),
            before,
            "nothing behind moved"
        );

        // Nor does the list, a breadcrumb, a right-click, or hover.
        handle_click(&mut app, &hits, hits.list.x + 2, hits.list.y + 1);
        assert_eq!(app.selected, before.0);
        handle_right_click(&mut app, &hits, hits.list.x + 2, hits.list.y + 1);
        assert!(app.menu.is_none(), "no context menu over a modal");
        assert_eq!(target_at(&app, &hits, cx, cy), None);
        assert_eq!(target_at(&app, &hits, hits.list.x + 2, hits.list.y), None);

        // Its own buttons still work.
        let cancel = hits
            .buttons
            .iter()
            .find(|(_, a)| *a == Action::Cancel)
            .unwrap()
            .0;
        assert_eq!(
            target_at(&app, &hits, cancel.x, cancel.y),
            Some(Hover::Button(Action::Cancel))
        );
        handle_click(&mut app, &hits, cancel.x, cancel.y);
        assert_eq!(app.view, View::Collector, "Cancel returns to the collector");
    }

    #[test]
    fn marking_explains_the_next_step() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        handle_key(&mut app, Key::Char(' '));
        assert!(app.status.starts_with("marked usr ("), "{}", app.status);
        assert!(app.status.contains("1 in collector"), "{}", app.status);
        assert!(app.status.contains("c review · x delete"), "{}", app.status);

        handle_key(&mut app, Key::Char(' '));
        assert!(app.status.starts_with("unmarked usr"), "{}", app.status);
        assert!(app.status.contains("0 in collector"), "{}", app.status);
    }

    #[test]
    fn context_menu_delete_names_what_else_is_already_marked() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);

        let label = |app: &App| {
            app.menu
                .as_ref()
                .unwrap()
                .items
                .iter()
                .find(|(a, _)| *a == crate::tui::app::MenuAction::Delete)
                .unwrap()
                .1
                .clone()
        };
        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        assert_eq!(label(&app), "Delete directory…", "nothing else staged");
        app.close_menu();

        app.toggle_mark_selected(); // stage row 0
        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y + 1);
        assert_eq!(
            label(&app),
            "Delete directory… (with 1 already marked)",
            "row 1's menu counts row 0"
        );
        app.close_menu();

        handle_right_click(&mut app, &hits, hits.list.x + 3, hits.list.y);
        assert_eq!(
            label(&app),
            "Delete directory…",
            "the marked item itself is not 'already marked' besides itself"
        );
    }

    #[test]
    fn picker_over_a_scan_warns_about_staged_marks() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        handle_key(&mut app, Key::Char('-'));
        assert_eq!(app.picker_marks_warning(), None, "no marks, no warning");

        handle_key(&mut app, Key::Esc);
        app.toggle_mark_selected();
        handle_key(&mut app, Key::Char('-'));
        let warning = app.picker_marks_warning().expect("warning with marks");
        assert!(warning.starts_with("1 marked item ("), "{warning}");
        assert!(warning.contains("Esc keeps them"), "{warning}");
        let mut buf = Buffer::new(100, 28, th.bg);
        draw::draw(&mut buf, &app);
        assert!(
            buf.text().contains("dropped by a new scan"),
            "{}",
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
        let th = theme::current();
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
                    .unwrap_or(crate::term::Cell::blank(th.bg));
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
        let th = theme::current();
        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, app);
        let dir = std::env::temp_dir();
        let ppm = dir.join(format!("rings-{stem}-current.ppm"));
        write_ppm(&buf, &ppm, 3);
        let text = dir.join(format!("rings-{stem}-current.txt"));
        std::fs::write(&text, buf.text()).unwrap();
        eprintln!(
            "{stem} dump {} slices rings_for={} → {} and {}",
            hits.slices.len(),
            sunburst::rings_for(hits.map),
            ppm.display(),
            text.display()
        );
        (buf.text(), text.display().to_string())
    }

    #[test]
    fn dump_browse_view_ppm() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        app.selected = 1;
        dump_view_ppm(&app, "browse");
        app.open_findings();
        dump_view_ppm(&app, "findings");
        let mut wide = Buffer::new(140, 28, th.bg);
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
        let th = theme::current();
        let mut app = App::new(std::path::PathBuf::from("/var"), false);
        app.view = View::Scanning;
        let mut buf = Buffer::new(80, 24, th.bg);
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

    /// Lay down a small but complete PostgreSQL data directory.
    fn write_cluster(data: &std::path::Path) {
        fs::create_dir_all(data.join("base").join("16384")).unwrap();
        fs::create_dir_all(data.join("base").join("pgsql_tmp")).unwrap();
        fs::create_dir_all(data.join("global")).unwrap();
        fs::create_dir_all(data.join("pg_wal")).unwrap();
        fs::write(data.join("PG_VERSION"), "16\n").unwrap();
        // Table data is comfortably the largest thing here, so it sorts first.
        fs::write(
            data.join("base").join("16384").join("2836"),
            vec![0u8; 262_144],
        )
        .unwrap();
        fs::write(data.join("global").join("1262"), vec![0u8; 4096]).unwrap();
        fs::write(
            data.join("pg_wal").join("000000010000000000000001"),
            vec![0u8; 8192],
        )
        .unwrap();
        fs::write(
            data.join("base").join("pgsql_tmp").join("pgsql_tmp1.0"),
            vec![0u8; 8192],
        )
        .unwrap();
    }

    #[test]
    fn databases_view_names_the_engine_and_says_what_to_run() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_cluster(&tmp.path().join("pgdata"));

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.open_databases();

        let mut buf = Buffer::new(110, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert!(screen.contains("Databases"), "view title:\n{screen}");
        assert!(screen.contains("postgres"), "engine column:\n{screen}");
        assert!(screen.contains("data"), "role column:\n{screen}");
        assert!(
            screen.contains("VACUUM"),
            "the detail line says what actually reclaims the space:\n{screen}"
        );
        assert!(!hits.rows.is_empty(), "rows must be hit-testable");
        assert!(
            hits.buttons.iter().any(|(_, a)| *a == Action::Databases),
            "the Databases chip is clickable"
        );
    }

    #[test]
    fn marking_live_table_data_is_refused_but_spill_is_not() {
        let tmp = tempfile::TempDir::new().unwrap();
        write_cluster(&tmp.path().join("pgdata"));

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.open_databases();

        let data_row = app
            .databases
            .iter()
            .position(|e| e.role == crate::apps::Role::Data)
            .expect("a base/ row");
        app.databases_selected = data_row;
        app.toggle_mark_selected();
        assert!(
            app.collector.is_empty(),
            "live table data must never reach the collector"
        );
        assert!(app.status.contains("refused"), "status: {}", app.status);
        assert!(
            app.status.contains("VACUUM"),
            "the refusal names the alternative: {}",
            app.status
        );

        // Query spill inside the same cluster is ordinary waste.
        let spill = app
            .databases
            .iter()
            .position(|e| e.role == crate::apps::Role::TempSpill)
            .expect("a pgsql_tmp row");
        app.databases_selected = spill;
        app.toggle_mark_selected();
        assert_eq!(app.collector.len(), 1, "status: {}", app.status);
    }

    #[test]
    fn a_new_scan_drops_the_collector_it_warned_about() {
        // `picker_marks_warning` promises marks are dropped by a new scan.
        // Before this they survived, holding node ids into a freed tree.
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("junk")).unwrap();
        fs::write(tmp.path().join("junk").join("big.tmp"), vec![b'x'; 8192]).unwrap();

        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        app.toggle_mark_selected();
        assert_eq!(app.collector.len(), 1, "something is marked");

        app.begin_scan(tmp.path().to_path_buf());
        assert!(
            app.collector.is_empty(),
            "a new scan drops marks, as the picker warning says it will"
        );
        assert!(app.databases.is_empty());
        assert!(app.findings.is_empty());
    }

    #[test]
    fn icicle_layout_writes_names_into_the_map_and_widens_the_list() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;

        let mut buf = Buffer::new(100, 28, th.bg);
        let sun = draw::draw(&mut buf, &app);
        assert_eq!(sun.layout, Layout::Sunburst, "sunburst is still the default");

        app.cycle_layout();
        assert_eq!(app.layout, Layout::Icicle);
        assert!(app.status.contains("icicle"), "the toggle says where it went");

        let mut buf = Buffer::new(100, 28, th.bg);
        let ice = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert_eq!(ice.layout, Layout::Icicle);
        assert!(!ice.slices.is_empty(), "the same slice list drives both maps");
        assert!(
            ice.list.width > sun.list.width,
            "the icicle gives the list the full width: {} vs {}",
            ice.list.width,
            sun.list.width
        );

        // The thing a sunburst structurally cannot do: names inside the map.
        let map_rows: Vec<&str> = screen
            .lines()
            .skip(ice.map.y as usize)
            .take(ice.map.height as usize)
            .collect();
        let map_text = map_rows.join("\n");
        for name in ["usr", "var"] {
            assert!(
                map_text.contains(name),
                "{name:?} should be written into the map itself:\n{map_text}"
            );
        }
    }

    #[test]
    fn clicking_an_icicle_bar_selects_exactly_that_node() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        app.cycle_layout();

        let mut buf = Buffer::new(100, 28, th.bg);
        let hits = draw::draw(&mut buf, &app);

        let x = hits.map.x + hits.map.width / 4;
        let y = hits.map.y;
        let want = icicle::hit_slice(&hits.slices, hits.map, x, y)
            .expect("a bar under the cursor")
            .node;

        handle_click(&mut app, &hits, x, y);
        assert_eq!(
            app.selected_node_id(),
            Some(want),
            "the click lands on the bar it was over"
        );

        // Hover uses the same lookup, so it must agree with the click.
        assert_eq!(target_at(&app, &hits, x, y), Some(Hover::Slice(want)));
    }

    #[test]
    fn a_short_body_drops_the_icicle_rather_than_squeezing_it() {
        let th = theme::current();
        let tmp = tempfile::TempDir::new().unwrap();
        write_nested_fixture(tmp.path());
        let tree = crate::scan::scan(tmp.path(), WalkOptions::default()).unwrap();
        let mut app = App::new(tmp.path().to_path_buf(), false);
        app.set_tree(tree);
        app.view = View::Browse;
        app.cycle_layout();

        // Header + 3-row footer leave almost nothing for the body.
        let mut buf = Buffer::new(80, 9, th.bg);
        let hits = draw::draw(&mut buf, &app);
        let screen = buf.text();

        assert_eq!(hits.map, crate::term::Rect::ZERO, "no map was drawn");
        assert!(
            !hits.rows.is_empty(),
            "the list still renders on its own:\n{screen}"
        );
    }

    #[test]
    fn layout_toggles_back_to_the_sunburst() {
        let mut app = App::new(std::path::PathBuf::from("."), false);
        assert_eq!(app.layout, Layout::Sunburst);
        app.cycle_layout();
        assert_eq!(app.layout, Layout::Icicle);
        app.cycle_layout();
        assert_eq!(app.layout, Layout::Sunburst);
    }
}
