mod app;
mod draw;
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

pub fn run(path: PathBuf, opts: WalkOptions, apparent: bool) -> Result<(), String> {
    let (tx, rx) = mpsc::channel();
    let scan_path = path.clone();
    std::thread::spawn(move || scan_with_progress(scan_path, opts, tx));

    let mut app = App::new(path, apparent);
    let term = Term::enter().map_err(|e| format!("cannot enter raw mode: {e}"))?;
    let result = event_loop(&term, &mut app, &rx);
    drop(term);
    result
}

fn event_loop(term: &Term, app: &mut App, rx: &mpsc::Receiver<WalkEvent>) -> Result<(), String> {
    let mut prev: Option<Buffer> = None;
    let mut hits = HitMap::empty();
    let mut dirty = true;

    loop {
        while let Ok(ev) = rx.try_recv() {
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
            app.view = View::Browse;
        }
        return;
    }

    match key {
        Key::Char('q') => app.quit = true,
        Key::Char('?') | Key::F1 => app.view = View::Help,
        Key::Char('j') | Key::Down => app.move_sel(1),
        Key::Char('k') | Key::Up => app.move_sel(-1),
        Key::PageDown => app.move_sel(10),
        Key::PageUp => app.move_sel(-10),
        Key::Enter => app.drill(),
        Key::Backspace | Key::Left | Key::Char('h') => go_back(app),
        Key::Char(' ') | Key::Char('d') => app.toggle_mark_selected(),
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

fn go_back(app: &mut App) {
    if matches!(app.view, View::Browse) {
        app.go_up_browse();
    } else {
        app.go_up();
    }
}

fn handle_click(app: &mut App, hits: &HitMap, x: u16, y: u16) {
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

    match app.view {
        View::Browse | View::Confirm { .. } => {
            if let Some(slice) = sunburst::hit_slice(&hits.slices, hits.sunburst, x, y) {
                let node = slice.node;
                app.focus_node(node);
                if dbl {
                    app.drill();
                }
                return;
            }
            if let Some(i) =
                row_index_at(hits.list, y, app.list_offset, app.current_children().len())
            {
                app.selected = i;
                if dbl {
                    app.drill();
                }
            }
        }
        View::Findings => {
            if let Some(i) = row_index_at(hits.list, y, app.list_offset, app.finding_ids().len()) {
                app.findings_selected = i;
                if dbl {
                    app.drill();
                }
            }
        }
        View::Collector => {
            if let Some(i) = row_index_at(hits.list, y, app.list_offset, app.collector.len()) {
                app.selected = i;
            }
        }
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
        Action::Help => app.view = View::Help,
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
        let has_block = screen.contains('█') || screen.contains('▀') || screen.contains('▄');
        assert!(has_block, "sunburst should paint block cells:\n{screen}");
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
        let w = buf.width as u32 * scale;
        let h = buf.height as u32 * 2 * scale;
        let mut px = vec![0u8; (w * h * 3) as usize];
        for y in 0..buf.height {
            for x in 0..buf.width {
                let cell = buf
                    .get(x, y)
                    .cloned()
                    .unwrap_or(crate::term::Cell::blank(BG));
                let (top, bot) = match cell.ch {
                    '█' => (cell.fg, cell.fg),
                    '▀' => (cell.fg, cell.bg),
                    '▄' => (cell.bg, cell.fg),
                    _ => (cell.bg, cell.bg),
                };
                for sy in 0..scale {
                    for sx in 0..scale {
                        let put = |px: &mut [u8], x: u32, y: u32, c: crate::term::Rgb| {
                            let i = ((y * w + x) * 3) as usize;
                            px[i] = c.0;
                            px[i + 1] = c.1;
                            px[i + 2] = c.2;
                        };
                        put(
                            &mut px,
                            x as u32 * scale + sx,
                            y as u32 * 2 * scale + sy,
                            top,
                        );
                        put(
                            &mut px,
                            x as u32 * scale + sx,
                            y as u32 * 2 * scale + scale + sy,
                            bot,
                        );
                    }
                }
            }
        }
        let mut out = format!("P6\n{w} {h}\n255\n").into_bytes();
        out.extend(px);
        std::fs::write(path, out).unwrap();
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

        let mut buf = Buffer::new(100, 28, BG);
        let hits = draw::draw(&mut buf, &app);
        let path = std::env::temp_dir().join("rings-browse-current.ppm");
        write_ppm(&buf, &path, 3);
        let text = std::env::temp_dir().join("rings-browse-current.txt");
        std::fs::write(&text, buf.text()).unwrap();
        eprintln!(
            "browse dump {} slices rings_for={} → {} and {}",
            hits.slices.len(),
            sunburst::rings_for(hits.sunburst),
            path.display(),
            text.display()
        );
        assert!(!hits.slices.is_empty());
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
