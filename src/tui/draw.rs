use std::borrow::Cow;

use crate::apps::Tone;
use crate::cli::{KeyGroup, HELP_COL_W, HELP_KEY_W, KEY_GROUPS};
use crate::constants::{CHIP_GAP, DELETE_CONFIRM_PHRASE, FOOTER_H};
use crate::delete::needs_typed_confirm;
use crate::logo;
use crate::size::{group_u64, human_bytes};
use crate::term::{Buffer, Cell, Rect, Rgb};
use crate::tui::app::{node_label, Action, App, Hover, Layout, Menu, MenuAction, View};
use crate::tui::icicle;
use crate::tui::sunburst::{self, Slice};
use crate::tui::theme::{self, category_color};

pub struct HitMap {
    /// Rect the tree map occupies, whichever layout drew it.
    pub map: Rect,
    /// Which projection `map` and `slices` were built with, so hit testing
    /// can undo the same one.
    pub layout: Layout,
    pub list: Rect,
    pub buttons: Vec<(Rect, Action)>,
    pub crumbs: Vec<(Rect, usize)>,
    pub slices: Vec<Slice>,
    /// Visible list rows, by index into the list in view.
    pub rows: Vec<(Rect, usize)>,
    /// Context-menu rows, by item index.
    pub menu: Vec<(Rect, usize)>,
}

impl HitMap {
    pub fn empty() -> Self {
        Self {
            map: Rect::ZERO,
            layout: Layout::Sunburst,
            list: Rect::ZERO,
            buttons: Vec::new(),
            crumbs: Vec::new(),
            slices: Vec::new(),
            rows: Vec::new(),
            menu: Vec::new(),
        }
    }
}

pub fn draw(buf: &mut Buffer, app: &App) -> HitMap {
    let th = theme::current();
    buf.fill(buf.area(), th.bg);
    let mut hits = match app.view {
        View::Picker => draw_picker(buf, app),
        View::Scanning => {
            draw_scan(buf, app);
            HitMap::empty()
        }
        View::Help => draw_help(buf),
        View::Settings => {
            draw_settings(buf, app);
            HitMap::empty()
        }
        View::Confirm { .. } => {
            // The modal covers the view: only its own buttons are targets.
            draw_main(buf, app);
            let mut hits = HitMap::empty();
            draw_confirm_modal(buf, app, &mut hits);
            hits
        }
        _ => draw_main(buf, app),
    };
    if app.update_popup && !matches!(app.view, View::Confirm { .. }) {
        let mut modal = HitMap::empty();
        draw_update_modal(buf, app, &mut modal);
        hits = modal;
    }
    if let Some(menu) = app.menu.as_ref() {
        draw_menu(buf, menu, app.hover, &mut hits);
    }
    hits
}

/// Right-click menu, anchored at the cursor and clamped to the screen.
fn draw_menu(buf: &mut Buffer, menu: &Menu, hover: Option<Hover>, hits: &mut HitMap) {
    let th = theme::current();
    let w = menu.width().min(buf.width);
    let h = menu.height().min(buf.height);
    if w < 4 || h < 3 {
        return;
    }
    let rect = Rect {
        x: menu.x.min(buf.width.saturating_sub(w)),
        y: menu.y.min(buf.height.saturating_sub(h)),
        width: w,
        height: h,
    };
    buf.fill(rect, th.bg);
    let title = truncate(&menu.title, rect.width.saturating_sub(4) as usize);
    let inner = draw_box(buf, rect, &format!(" {title} "), th.muted, th.accent);

    for (i, (action, label)) in menu.items.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.bottom() {
            break;
        }
        let sel = i == menu.selected;
        let row_bg = row_background(th, sel, hover == Some(Hover::Menu(i)));
        let rect = Rect {
            x: inner.x,
            y,
            width: inner.width,
            height: 1,
        };
        buf.fill(rect, row_bg);
        let fg = match action {
            MenuAction::Delete => th.danger,
            MenuAction::Cancel => th.muted,
            _ => th.text,
        };
        let text = truncate(label, inner.width.saturating_sub(2) as usize);
        buf.print_styled(inner.x + 1, y, &text, fg, row_bg, sel);
        hits.menu.push((rect, i));
    }
}

fn draw_settings(buf: &mut Buffer, app: &App) {
    let th = theme::current();
    let (wm_w, wm_h) = logo::wordmark_size();
    let show_mark = buf.width >= wm_w + 10 && buf.height >= wm_h + 12;
    let w = if show_mark { (wm_w + 12).max(66) } else { 56 }.min(buf.width.saturating_sub(4));
    let h = if show_mark { 20 } else { 12 }.min(buf.height.saturating_sub(2));
    let rect = Rect {
        x: (buf.width.saturating_sub(w)) / 2,
        y: (buf.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    buf.fill(rect, th.panel);
    let inner = draw_box(buf, rect, "", th.accent, th.accent);

    let mut y = inner.y + 1;
    if show_mark && inner.width >= wm_w && inner.height >= wm_h + 8 {
        let mx = inner.x + inner.width.saturating_sub(wm_w) / 2;
        paint_wordmark(buf, mx, y, wordmark_glint_col(app, wm_w), th.panel);
        y = y.saturating_add(wm_h);
        y = y.saturating_add(1);
        let rule_w = (wm_w / 3).max(12).min(inner.width.saturating_sub(4));
        let rx = inner.x + inner.width.saturating_sub(rule_w) / 2;
        for i in 0..rule_w {
            buf.print(rx + i, y, "─", th.muted, th.panel);
        }
        y = y.saturating_add(2);
    } else {
        buf.print_styled(inner.x + 2, y, "settings", th.accent, th.panel, true);
        y = y.saturating_add(2);
    }

    let rows = [
        ("Theme", app.settings.theme.clone()),
        (
            "CSV export folder",
            app.settings_edit
                .clone()
                .unwrap_or_else(|| app.settings.export_dir.display().to_string()),
        ),
    ];
    for (i, (label, value)) in rows.iter().enumerate() {
        let row_y = y.saturating_add(i as u16);
        if row_y >= inner.bottom() {
            break;
        }
        let selected = app.settings_selected == i;
        let row_bg = if selected { th.select_bg } else { th.panel };
        buf.fill(
            Rect {
                x: inner.x + 1,
                y: row_y,
                width: inner.width.saturating_sub(2),
                height: 1,
            },
            row_bg,
        );
        let mark = if selected { "▸" } else { " " };
        let mut x = buf.print(inner.x + 2, row_y, mark, th.accent, row_bg);
        x = buf.print(x, row_y, " ", row_bg, row_bg);
        buf.print_styled(x, row_y, label, th.text, row_bg, selected);
        let marker = if i == 0 {
            format!("‹ {value} ›")
        } else if app.settings_edit.is_some() {
            format!("{value}▏")
        } else {
            value.clone()
        };
        let vx = (inner.x + 24).max(x + 2);
        let shown = truncate(&marker, inner.right().saturating_sub(vx + 2) as usize);
        buf.print(vx, row_y, &shown, th.accent, row_bg);
    }

    let note = if app.settings_edit.is_some() {
        "type a folder · ~ expands · Enter saves · Esc cancels"
    } else {
        "j/k select   h/l theme   Enter edit   m close"
    };
    let ny = inner.bottom().saturating_sub(2);
    if ny > y.saturating_add(2) {
        buf.print(
            inner.x + 2,
            ny,
            &truncate(note, inner.width.saturating_sub(4) as usize),
            th.muted,
            th.panel,
        );
    }
}

fn wordmark_glint_col(app: &App, width: u16) -> i32 {
    let span = width as i32 + 14;
    if span <= 0 {
        return 0;
    }
    (app.started.elapsed().as_millis() / 42) as i32 % span - 4
}

fn mix_rgb(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Rgb(lerp(a.0, b.0), lerp(a.1, b.1), lerp(a.2, b.2))
}

/// DCP-1 RINGS, one palette hue per letter, a specular glint walking across.
fn paint_wordmark(buf: &mut Buffer, x: u16, y: u16, glint: i32, bg: Rgb) {
    let th = theme::current();
    let shine = th.text;
    for (row, line) in logo::WORDMARK.iter().enumerate() {
        let py = y.saturating_add(row as u16);
        if py >= buf.height {
            break;
        }
        let mut cx = x;
        for (col, ch) in line.chars().enumerate() {
            if cx >= buf.width {
                break;
            }
            if ch != ' ' {
                let base = th.palette[logo::wordmark_letter(col) % th.palette.len()];
                let d = (col as i32 - glint).unsigned_abs();
                let (fg, bold) = if d <= 5 {
                    let t = (1.0 - d as f32 / 5.0).powi(2);
                    (mix_rgb(base, shine, t), d <= 2)
                } else {
                    (base, false)
                };
                buf.set_cell(cx, py, Cell { ch, fg, bg, bold });
            }
            cx = cx.saturating_add(1);
        }
    }
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn draw_scan(buf: &mut Buffer, app: &App) {
    let th = theme::current();
    let x = buf.print_styled(1, 0, "◎ rings ", th.text, th.bg, true);
    buf.print(x, 0, "scanning", th.muted, th.bg);

    let (files, dirs, errors, current) = match &app.progress {
        Some(p) => (p.files, p.dirs, p.errors, p.current.display().to_string()),
        None => (0, 0, 0, app.scan_path.display().to_string()),
    };

    let (lw, lh) = logo::size();
    let lx = buf.width.saturating_sub(lw) / 2;
    // Sit the mark above the spinner so the first paint is the sunburst, not a blank wait.
    let ly = (buf.height / 2).saturating_sub(lh / 2 + 3).max(1);
    paint_logo(buf, lx, ly, true);

    let cy = ly.saturating_add(lh).saturating_add(1);
    let spinner = SPINNER[app.spin_frame(SPINNER.len())];
    let path_line = app.scan_path.display().to_string();
    let px = (buf
        .width
        .saturating_sub(path_line.chars().count() as u16 + 2))
        / 2;
    let x = buf.print(px, cy, spinner, th.accent, th.bg);
    buf.print(
        x + 1,
        cy,
        &truncate(&path_line, buf.width.saturating_sub(4) as usize),
        th.accent,
        th.bg,
    );

    let counts = format!(
        "{} files   {} dirs   {} errors",
        group_u64(files),
        group_u64(dirs),
        group_u64(errors)
    );
    let cx = (buf.width.saturating_sub(counts.chars().count() as u16)) / 2;
    buf.print_styled(cx, cy.saturating_add(2), &counts, th.text, th.bg, true);

    let shown = truncate(&current, buf.width.saturating_sub(6) as usize);
    let sx = (buf.width.saturating_sub(shown.chars().count() as u16)) / 2;
    buf.print(sx, cy.saturating_add(3), &shown, th.muted, th.bg);

    let hint = if app.is_root {
        crate::sys::scan_banner_privileged()
    } else {
        crate::sys::scan_banner_unprivileged()
    };
    let y = buf.height.saturating_sub(2);
    let hx = (buf.width.saturating_sub(hint.chars().count() as u16)) / 2;
    buf.print(hx, y, hint, th.muted, th.bg);
}

/// Header row, body, and footer band of the standard screen.
fn frame(buf: &Buffer) -> (Rect, Rect, Rect) {
    let footer_h = FOOTER_H.min(buf.height);
    let header = Rect {
        x: 0,
        y: 0,
        width: buf.width,
        height: 1,
    };
    let body = Rect {
        x: 0,
        y: 1,
        width: buf.width,
        height: buf.height.saturating_sub(1 + footer_h),
    };
    let footer = Rect {
        x: 0,
        y: buf.height.saturating_sub(footer_h),
        width: buf.width,
        height: footer_h,
    };
    (header, body, footer)
}

fn draw_picker(buf: &mut Buffer, app: &App) -> HitMap {
    let th = theme::current();
    let (_, body, footer) = frame(buf);
    let mut hits = HitMap::empty();
    hits.buttons = draw_footer(buf, app, footer);
    let Some(picker) = app.picker.as_ref() else {
        return hits;
    };

    let x = buf.print_styled(1, 0, "◎ rings ", th.text, th.bg, true);
    buf.print(
        x,
        0,
        &truncate(
            &picker.dir.to_string_lossy(),
            buf.width.saturating_sub(x + 1) as usize,
        ),
        th.accent,
        th.bg,
    );

    let inner = draw_box(buf, body, " Pick a directory to scan ", th.accent, th.muted);
    hits.list = inner;
    if picker.entries.is_empty() {
        buf.print(
            inner.x + 1,
            inner.y,
            "empty — press s to scan this directory, h to go up",
            th.muted,
            th.bg,
        );
        return hits;
    }

    let h = inner.height as usize;
    let start = picker.offset.min(picker.entries.len().saturating_sub(1));
    for (row, (i, entry)) in picker
        .entries
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .enumerate()
    {
        let y = inner.y + row as u16;
        let sel = i == picker.selected;
        let row_bg = list_row(buf, &mut hits, inner, y, i, sel, app.hovered_row(i));
        let (glyph, color, fg) = if entry.is_dir {
            ("▸", th.accent, th.text)
        } else {
            ("·", th.smaller, th.muted)
        };
        let mut x = buf.print(inner.x, y, " ", color, row_bg);
        x = buf.print(x, y, glyph, color, row_bg);
        x = buf.print(x, y, " ", color, row_bg);
        let name_w = inner.right().saturating_sub(x + 1) as usize;
        if entry.is_dir {
            let shown = truncate(&entry.name, name_w.saturating_sub(1));
            let x = buf.print_styled(x, y, &shown, fg, row_bg, sel);
            buf.print(x, y, "/", fg, row_bg);
        } else {
            buf.print_styled(x, y, &truncate(&entry.name, name_w), fg, row_bg, sel);
        }
    }
    hits
}

fn draw_main(buf: &mut Buffer, app: &App) -> HitMap {
    let (header, body, footer) = frame(buf);
    let crumbs = draw_header(buf, app, header);
    let mut hits = HitMap {
        map: Rect::ZERO,
        layout: app.layout,
        list: Rect::ZERO,
        buttons: Vec::new(),
        crumbs,
        slices: Vec::new(),
        rows: Vec::new(),
        menu: Vec::new(),
    };

    match app.view {
        View::Findings => draw_findings(buf, app, body, &mut hits),
        View::Databases => draw_databases(buf, app, body, &mut hits),
        View::Collector => draw_collector(buf, app, body, &mut hits),
        _ => draw_browse(buf, app, body, &mut hits),
    }

    hits.buttons = draw_footer(buf, app, footer);
    hits
}

fn draw_header(buf: &mut Buffer, app: &App, area: Rect) -> Vec<(Rect, usize)> {
    let th = theme::current();
    let mut crumbs = Vec::new();
    let mut x = buf.print_styled(area.x + 1, area.y, "◎ rings ", th.text, th.bg, true);
    for (i, (label, nid)) in app.breadcrumb().iter().enumerate() {
        if i > 0 {
            x = buf.print(x, area.y, " › ", th.muted, th.bg);
        }
        let shown = truncate(label, 24);
        let w = shown.chars().count() as u16;
        let hovered = app.hover == Some(Hover::Crumb(*nid));
        let end = buf.print_styled(x, area.y, &shown, th.accent, th.bg, hovered);
        crumbs.push((
            Rect {
                x,
                y: area.y,
                width: w.max(1),
                height: 1,
            },
            *nid,
        ));
        x = end;
        if x >= area.right() {
            break;
        }
    }
    crumbs
}

fn draw_browse(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    match app.layout {
        Layout::Sunburst => draw_browse_sunburst(buf, app, area, hits),
        Layout::Icicle => draw_browse_icicle(buf, app, area, hits),
    }
}

/// Slices with the hovered node lit, ready to hand to a renderer.
fn browse_slices(app: &App, tree: &crate::scan::Tree, current: usize, rings: usize) -> Vec<Slice> {
    let mut slices = sunburst::build_slices(tree, current, app.apparent, app.selected_id(), rings);
    if let Some(Hover::Slice(node)) = app.hover {
        for s in slices.iter_mut().filter(|s| s.node == node && !s.grouped) {
            s.color = theme::emphasize(s.color);
        }
    }
    slices
}

/// Icicle across the top, child list beneath it at full width. The map costs
/// a handful of rows instead of a 58% column, so the list gets the rest.
fn draw_browse_icicle(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let budget = icicle::rows_for(area);
    let Some(tree) = app.tree() else {
        hits.list = area;
        return;
    };
    if budget == 0 {
        // Too small for a map: the list alone is the honest fallback, and
        // it is the same view the layout would degrade to anyway.
        hits.list = area;
        draw_child_list(buf, app, area, hits);
        return;
    }

    let current = tree.node_at(&app.cwd);
    let slices = browse_slices(app, tree, current, budget);
    // A shallow tree must not leave dead rows under its leaves: draw only
    // the depth that exists and hand the remainder to the list.
    let depth = slices.iter().map(|s| s.ring + 1).max().unwrap_or(0);
    let rows = depth.min(budget) as u16;

    // Base bar: everything below it is a share of this one.
    let node = tree.get(current);
    let base = format!(
        " {}  {} ",
        node_label(node),
        human_bytes(node.display_size(app.apparent))
    );
    let head = Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: 1,
    };
    buf.fill(head, th.panel);
    buf.print_styled(
        area.x,
        area.y,
        &truncate(&base, area.width as usize),
        th.accent,
        th.panel,
        true,
    );

    if rows == 0 {
        // A directory with nothing worth drawing still gets its base bar.
        let list = Rect {
            x: area.x + 1,
            y: area.y + 1,
            width: area.width.saturating_sub(1),
            height: area.height.saturating_sub(1),
        };
        hits.list = list;
        draw_child_list(buf, app, list, hits);
        return;
    }

    let map = Rect {
        x: area.x,
        y: area.y + 1,
        width: area.width,
        height: rows,
    };
    let list_y = map.bottom() + 1;
    let list = Rect {
        x: area.x + 1,
        y: list_y,
        width: area.width.saturating_sub(1),
        height: area.height.saturating_sub(list_y.saturating_sub(area.y)),
    };
    hits.map = map;
    hits.list = list;

    icicle::render(buf, map, &slices, tree, app.apparent, app.selected_id());
    hits.slices = slices;

    for x in area.x..area.right() {
        buf.print(x, map.bottom(), "\u{2500}", th.muted, th.bg);
    }
    draw_child_list(buf, app, list, hits);
}

fn draw_browse_sunburst(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let left_w = (area.width as u32 * 58 / 100) as u16;
    let left = Rect {
        x: area.x,
        y: area.y,
        width: left_w,
        height: area.height,
    };
    let right = Rect {
        x: area.x + left_w,
        y: area.y,
        width: area.width.saturating_sub(left_w),
        height: area.height,
    };
    hits.map = left;
    hits.list = Rect {
        x: right.x + 1,
        y: right.y,
        width: right.width.saturating_sub(1),
        height: right.height,
    };

    let Some(tree) = app.tree() else {
        return;
    };
    let current = tree.node_at(&app.cwd);
    let rings = sunburst::rings_for(left);
    let slices = browse_slices(app, tree, current, rings);
    sunburst::render(buf, left, &slices);
    hits.slices = slices;

    // Size label in the hole.
    let node = tree.get(current);
    let label = human_bytes(node.display_size(app.apparent));
    let name = truncate(&node.name, 18);
    if left.height > 4 {
        let cy = left.y + left.height / 2;
        let lx = left.x + (left.width.saturating_sub(label.chars().count() as u16)) / 2;
        buf.print(lx, cy.saturating_sub(1), &label, th.accent, th.bg);
        let nx = left.x + (left.width.saturating_sub(name.chars().count() as u16)) / 2;
        buf.print(nx, cy, &name, th.muted, th.bg);
    }

    for y in right.y..right.bottom() {
        buf.print(right.x, y, "│", th.muted, th.bg);
    }
    draw_child_list(buf, app, hits.list, hits);
}

fn draw_child_list(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let Some(tree) = app.tree() else {
        return;
    };
    let current = tree.node_at(&app.cwd);
    let kids = &tree.get(current).children;
    if kids.is_empty() {
        buf.print(area.x + 1, area.y, "empty", th.muted, th.bg);
        return;
    }
    let h = area.height as usize;
    let start = app.list_offset.min(kids.len().saturating_sub(1));
    for (row, (i, &cid)) in kids.iter().enumerate().skip(start).take(h).enumerate() {
        let y = area.y + row as u16;
        let n = tree.get(cid);
        let sel = i == app.selected;
        let row_bg = list_row(buf, hits, area, y, i, sel, app.hovered_row(i));
        // A guarded row is one the confirm modal would refuse, so say so here
        // rather than let someone find that out at the end of the flow.
        let color = if n.guard.is_some() {
            theme::tone_color(Tone::Protected)
        } else if n.category.is_waste() {
            category_color(n.category)
        } else {
            th.palette[i % th.palette.len()]
        };
        let dot = if app.collector.contains_path(&n.path) {
            "●"
        } else if n.guard.is_some() {
            "▪"
        } else {
            "·"
        };
        let size = human_bytes(n.display_size(app.apparent));
        let size_w = size.chars().count() as u16 + 1;
        let name_w = area.width.saturating_sub(size_w + 4) as usize;
        let name = truncate(&n.name, name_w);
        let mut x = buf.print(area.x, y, " ", color, row_bg);
        x = buf.print(x, y, dot, color, row_bg);
        x = buf.print(x, y, " ", color, row_bg);
        buf.print_styled(x, y, &name, th.text, row_bg, sel);
        let sx = area.right().saturating_sub(size_w);
        buf.print(sx, y, &size, if sel { th.text } else { th.muted }, row_bg);
    }
}

fn draw_findings(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let Some(tree) = app.tree() else {
        return;
    };
    let ids = app.finding_ids();
    let title = format!(
        " Temp & cache · {} hits (inspect, then mark — never auto-deleted) ",
        ids.len()
    );
    let inner = draw_box(buf, area, &title, th.accent, th.muted);
    hits.list = inner;

    if ids.is_empty() {
        buf.print(
            inner.x,
            inner.y,
            "No temp, cache, log, journal, or crash paths in this scan.",
            th.muted,
            th.bg,
        );
        return;
    }

    let h = inner.height as usize;
    let start = app.list_offset.min(ids.len().saturating_sub(1));
    for (row, (i, &id)) in ids.iter().enumerate().skip(start).take(h).enumerate() {
        let y = inner.y + row as u16;
        let n = tree.get(id);
        let sel = i == app.findings_selected;
        let row_bg = list_row(buf, hits, inner, y, i, sel, app.hovered_row(i));
        let color = category_color(n.category);
        let marked = if app.collector.contains_path(&n.path) {
            "●"
        } else {
            "·"
        };
        let size = human_bytes(n.display_size(app.apparent));
        let size_w = size.chars().count() as u16 + 1;
        let mut x = buf.print(inner.x, y, &format!(" {marked} "), color, row_bg);
        x = buf.print(x, y, &format!("{:<8}", n.category.label()), color, row_bg);
        let path_w = inner.width.saturating_sub(x - inner.x + size_w + 1) as usize;
        let raw = n.path.to_string_lossy();
        buf.print_styled(x, y, &truncate(&raw, path_w), th.text, row_bg, sel);
        buf.print(
            inner.right().saturating_sub(size_w),
            y,
            &size,
            if sel { th.text } else { th.muted },
            row_bg,
        );
    }
}

/// Glyph for a row's weight. Pairs with `theme::tone_color`.
fn tone_glyph(tone: Tone) -> &'static str {
    match tone {
        Tone::Protected => "\u{25cf}",
        Tone::Advisory => "\u{25b8}",
        Tone::Reclaimable => "\u{25cb}",
    }
}

/// Databases found by layout and by header probe. Unlike every other list in
/// rings this one is not a delete queue: most rows are things you must not
/// remove, so each carries the command that actually returns the space.
fn draw_databases(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let reclaimable: u64 = app.databases.iter().map(|e| e.reclaimable).sum();
    let title = format!(
        " Databases \u{b7} {} \u{b7} {} reclaimable without removing data ",
        app.databases.len(),
        human_bytes(reclaimable)
    );
    let inner = draw_box(buf, area, &title, th.accent, th.muted);

    if app.databases.is_empty() {
        hits.list = inner;
        buf.print(
            inner.x,
            inner.y,
            "No PostgreSQL clusters or SQLite databases in this scan.",
            th.muted,
            th.bg,
        );
        return;
    }

    // Reserve the last three rows for the selection's guidance: a rule, then
    // why the space is there and what to do about it.
    let detail_h: u16 = if inner.height >= 6 { 3 } else { 0 };
    let list = Rect {
        height: inner.height.saturating_sub(detail_h),
        ..inner
    };
    hits.list = list;

    let h = list.height as usize;
    let start = app.list_offset.min(app.databases.len().saturating_sub(1));
    for (row, (i, entry)) in app
        .databases
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .enumerate()
    {
        let y = list.y + row as u16;
        let sel = i == app.databases_selected;
        let row_bg = list_row(buf, hits, list, y, i, sel, app.hovered_row(i));
        let g = entry.guidance();
        let color = theme::tone_color(g.tone());

        let size = human_bytes(entry.bytes);
        let gain = if entry.reclaimable > 0 {
            format!("\u{2192} {}", human_bytes(entry.reclaimable))
        } else {
            String::new()
        };
        let right_w = size.chars().count() as u16 + gain.chars().count() as u16 + 3;

        let mut x = buf.print(
            list.x,
            y,
            &format!(" {} ", tone_glyph(g.tone())),
            color,
            row_bg,
        );
        x = buf.print(x, y, &format!("{:<9}", entry.kind.as_str()), color, row_bg);
        x = buf.print(
            x,
            y,
            &format!("{:<6}", entry.role.as_str()),
            th.muted,
            row_bg,
        );
        let label_w = list.width.saturating_sub(x - list.x + right_w) as usize;
        buf.print_styled(x, y, &truncate(&entry.label, label_w), th.text, row_bg, sel);

        let sx = list.right().saturating_sub(right_w);
        let after = buf.print(sx, y, &size, if sel { th.text } else { th.muted }, row_bg);
        if !gain.is_empty() {
            buf.print(after.saturating_add(1), y, &gain, th.accent, row_bg);
        }
    }

    if detail_h == 0 {
        return;
    }
    let Some(entry) = app.databases.get(app.databases_selected) else {
        return;
    };
    let g = entry.guidance();
    let dy = list.bottom();
    for x in inner.x..inner.right() {
        buf.print(x, dy, "\u{2500}", th.muted, th.bg);
    }
    let w = inner.width.saturating_sub(1) as usize;
    let why = match &entry.detail {
        Some(d) => format!("{} {} \u{b7} {}", entry.kind.label(), g.why, d),
        None => format!("{} {}", entry.kind.label(), g.why),
    };
    buf.print(inner.x, dy + 1, &truncate(&why, w), th.text, th.bg);
    buf.print(
        inner.x,
        dy + 2,
        &truncate(g.action, w),
        theme::tone_color(g.tone()),
        th.bg,
    );
}

fn draw_collector(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let th = theme::current();
    let title = format!(
        " Collector · {} · {} — nothing deleted until you confirm ",
        app.collector.len(),
        human_bytes(app.collector.total_bytes())
    );
    let inner = draw_box(buf, area, &title, th.warn, th.warn);
    hits.list = inner;

    if app.collector.is_empty() {
        buf.print(
            inner.x,
            inner.y,
            "Empty. Mark items with Space or d from the sunburst or Temp & cache view.",
            th.muted,
            th.bg,
        );
        return;
    }

    let h = inner.height as usize;
    let start = app.list_offset.min(app.collector.len().saturating_sub(1));
    for (row, (i, item)) in app
        .collector
        .items()
        .iter()
        .enumerate()
        .skip(start)
        .take(h)
        .enumerate()
    {
        let y = inner.y + row as u16;
        let sel = i == app.selected;
        let row_bg = list_row(buf, hits, inner, y, i, sel, app.hovered_row(i));
        let size = human_bytes(item.size_bytes);
        let size_w = size.chars().count() as u16 + 1;
        let x = buf.print(inner.x, y, " ● ", th.danger, row_bg);
        let path_w = inner.width.saturating_sub(3 + size_w + 1) as usize;
        let raw = item.path.to_string_lossy();
        buf.print_styled(x, y, &truncate(&raw, path_w), th.text, row_bg, sel);
        buf.print(
            inner.right().saturating_sub(size_w),
            y,
            &size,
            if sel { th.text } else { th.muted },
            row_bg,
        );
    }
}

fn draw_footer(buf: &mut Buffer, app: &App, area: Rect) -> Vec<(Rect, Action)> {
    if area.height == 0 {
        return Vec::new();
    }
    draw_footer_stats(buf, app, area.y, area.width);

    let mut buttons = Vec::new();
    if area.height >= 2 {
        buttons = draw_footer_chrome(buf, app, area.y + 1, area.width);
    }
    if area.height >= 3 {
        draw_footer_path(buf, app, area.y + 2, area.width);
    }
    buttons
}

fn draw_footer_stats(buf: &mut Buffer, app: &App, y: u16, width: u16) {
    let th = theme::current();
    if let (View::Picker, Some(picker)) = (&app.view, app.picker.as_ref()) {
        let dirs = picker.dir_count();
        let x = buf.print(
            1,
            y,
            &format!(
                "{} directories · {} entries  ·  s scans ",
                group_u64(dirs as u64),
                group_u64(picker.entries.len() as u64)
            ),
            th.muted,
            th.bg,
        );
        buf.print_styled(
            x,
            y,
            &truncate(
                &picker.scan_target().to_string_lossy(),
                width.saturating_sub(x + 1) as usize,
            ),
            th.text,
            th.bg,
            true,
        );
        return;
    }
    let Some(tree) = app.tree() else {
        return;
    };
    let n = tree.get(tree.node_at(&app.cwd));
    let mut x = buf.print(1, y, "used ", th.muted, th.bg);
    x = buf.print_styled(x, y, &human_bytes(n.used), th.text, th.bg, true);
    x = buf.print(x, y, "  ·  apparent ", th.muted, th.bg);
    x = buf.print_styled(x, y, &human_bytes(n.apparent), th.text, th.bg, true);
    let stats = format!(
        "  ·  {} files · {} dirs · {} errors · {} hardlinks skipped  ",
        group_u64(tree.stats.files),
        group_u64(tree.stats.dirs),
        group_u64(tree.stats.errors),
        group_u64(tree.stats.hardlinks_deduped)
    );
    x = buf.print(x, y, &stats, th.muted, th.bg);
    if let Some(hint) = app.not_root_hint() {
        buf.print(
            x,
            y,
            &truncate(&hint, width.saturating_sub(x) as usize),
            th.warn,
            th.bg,
        );
    }
}

struct Chip {
    action: Action,
    label: String,
    /// Lower is kept longer when the row overflows.
    keep: u8,
}

fn footer_chips(app: &App) -> Vec<Chip> {
    let mut chips = Vec::new();
    if matches!(app.view, View::Picker) {
        chips.push(Chip {
            action: Action::Scan,
            label: " Scan this ".into(),
            keep: 0,
        });
        chips.push(Chip {
            action: Action::Back,
            label: " Up ".into(),
            keep: 2,
        });
        if app.tree().is_some() {
            chips.push(Chip {
                action: Action::BackToScan,
                label: " Back to scan ".into(),
                keep: 1,
            });
        }
        chips.push(Chip {
            action: Action::Help,
            label: " Keys ".into(),
            keep: 3,
        });
        chips.push(Chip {
            action: Action::Quit,
            label: " Quit ".into(),
            keep: 1,
        });
        return chips;
    }
    if matches!(app.view, View::Collector) && !app.collector.is_empty() {
        chips.push(Chip {
            action: Action::ConfirmDelete,
            label: " Confirm delete ".into(),
            keep: 0,
        });
    }
    chips.push(Chip {
        action: Action::Findings,
        label: " Temp & cache ".into(),
        keep: 1,
    });
    chips.push(Chip {
        action: Action::Databases,
        label: " Databases ".into(),
        keep: 5,
    });
    chips.push(Chip {
        action: Action::Collector,
        label: format!(" Collector ({}) ", app.collector.len()),
        keep: 2,
    });
    chips.push(Chip {
        action: Action::Picker,
        label: " Picker ".into(),
        keep: 4,
    });
    chips.push(Chip {
        action: Action::Export,
        label: " Export CSV ".into(),
        keep: 7,
    });
    chips.push(Chip {
        action: Action::Mark,
        label: " Mark ".into(),
        keep: 5,
    });
    chips.push(Chip {
        action: Action::Back,
        label: " Back ".into(),
        keep: 4,
    });
    chips.push(Chip {
        action: Action::Help,
        label: " Keys ".into(),
        keep: 6,
    });
    chips.push(Chip {
        action: Action::Quit,
        label: " Quit ".into(),
        keep: 3,
    });
    chips
}

fn chips_width(chips: &[Chip]) -> u16 {
    if chips.is_empty() {
        return 0;
    }
    let labels: u16 = chips.iter().map(|c| c.label.chars().count() as u16).sum();
    labels + CHIP_GAP * (chips.len() as u16 - 1)
}

fn fit_chips(mut chips: Vec<Chip>, width: u16) -> Vec<Chip> {
    while chips.len() > 1 && chips_width(&chips) > width {
        let drop_i = chips
            .iter()
            .enumerate()
            .max_by_key(|(_, c)| c.keep)
            .map(|(i, _)| i)
            .unwrap_or(chips.len() - 1);
        chips.remove(drop_i);
    }
    chips
}

fn chip_is_active(app: &App, action: Action) -> bool {
    matches!(
        (&app.view, action),
        (View::Findings, Action::Findings)
            | (View::Databases, Action::Databases)
            | (View::Collector, Action::Collector)
    )
}

fn draw_footer_chrome(buf: &mut Buffer, app: &App, y: u16, width: u16) -> Vec<(Rect, Action)> {
    let th = theme::current();
    let budget = width.saturating_sub(2);
    let chips = fit_chips(footer_chips(app), budget);
    let mut buttons = Vec::new();
    let mut x = 1u16;
    for chip in &chips {
        let w = chip.label.chars().count() as u16;
        if x.saturating_add(w) > width {
            break;
        }
        let hovered = app.hover == Some(Hover::Button(chip.action));
        let (fg, bg_c, bold) = if chip.action == Action::ConfirmDelete {
            (th.bg, th.danger, true)
        } else if chip_is_active(app, chip.action) {
            (th.bg, th.accent, true)
        } else if hovered {
            (th.text, th.select_bg, true)
        } else {
            (th.text, th.chip, false)
        };
        buf.print_styled(x, y, &chip.label, fg, bg_c, bold);
        buttons.push((
            Rect {
                x,
                y,
                width: w,
                height: 1,
            },
            chip.action,
        ));
        x = x.saturating_add(w + CHIP_GAP);
    }

    let hints = footer_hints(app);
    let fallback = vec![("?", "help")];
    let chosen = if hints_fit(&hints, width, x) {
        hints
    } else if hints_fit(&fallback, width, x) {
        fallback
    } else {
        Vec::new()
    };
    if !chosen.is_empty() {
        let hint_x = width.saturating_sub(hints_width(&chosen) + 1);
        draw_hints(buf, hint_x, y, &chosen);
    }
    buttons
}

fn hints_fit(hints: &[(&str, &str)], width: u16, chips_end: u16) -> bool {
    let hint_w = hints_width(hints);
    if hint_w == 0 {
        return false;
    }
    let hint_x = width.saturating_sub(hint_w + 1);
    hint_x >= chips_end.saturating_add(2)
}

fn footer_hints(app: &App) -> Vec<(&'static str, &'static str)> {
    match app.view {
        View::Picker if app.tree().is_some() => vec![
            ("j/k", "move"),
            ("Enter", "open"),
            ("s", "scan"),
            ("Esc", "back to scan"),
        ],
        View::Picker => vec![
            ("j/k", "move"),
            ("Enter", "open"),
            ("h", "up"),
            ("s", "scan"),
            ("?", "help"),
        ],
        View::Collector => vec![
            ("j/k", "move"),
            ("Space", "unmark"),
            ("x", "confirm"),
            ("?", "help"),
        ],
        View::Findings => vec![
            ("j/k", "move"),
            ("Enter", "jump"),
            ("Space", "mark"),
            ("?", "help"),
        ],
        View::Databases => vec![
            ("j/k", "move"),
            ("Enter", "jump"),
            ("b", "close"),
            ("?", "help"),
        ],
        _ => vec![
            ("j/k", "move"),
            ("Enter", "drill"),
            ("Space", "mark"),
            ("?", "help"),
        ],
    }
}

fn hints_width(hints: &[(&str, &str)]) -> u16 {
    if hints.is_empty() {
        return 0;
    }
    let mut w = 0u16;
    for (i, (key, verb)) in hints.iter().enumerate() {
        if i > 0 {
            w = w.saturating_add(3); // " · "
        }
        w = w
            .saturating_add(key.chars().count() as u16)
            .saturating_add(1)
            .saturating_add(verb.chars().count() as u16);
    }
    w
}

fn draw_hints(buf: &mut Buffer, mut x: u16, y: u16, hints: &[(&str, &str)]) {
    let th = theme::current();
    for (i, (key, verb)) in hints.iter().enumerate() {
        if i > 0 {
            x = buf.print(x, y, " · ", th.smaller, th.bg);
        }
        x = buf.print_styled(x, y, key, th.text, th.bg, true);
        x = buf.print(x, y, " ", th.muted, th.bg);
        x = buf.print(x, y, verb, th.muted, th.bg);
    }
}

fn draw_footer_path(buf: &mut Buffer, app: &App, y: u16, width: u16) {
    let th = theme::current();
    let (line, color) = if !app.status.is_empty() {
        (app.status.clone(), th.muted)
    } else if let Some(tip) = app.hover_line() {
        (tip, th.muted)
    } else if let Some(warning) = app.picker_marks_warning() {
        (warning, th.warn)
    } else {
        (app.selected_path().unwrap_or_default(), th.muted)
    };
    if line.is_empty() {
        return;
    }
    buf.print(
        1,
        y,
        &truncate(&line, width.saturating_sub(2) as usize),
        color,
        th.bg,
    );
}

fn draw_confirm_modal(buf: &mut Buffer, app: &App, hits: &mut HitMap) {
    let th = theme::current();
    let View::Confirm { typed } = &app.view else {
        return;
    };
    let w = 64.min(buf.width.saturating_sub(4));
    let h = 12.min(buf.height.saturating_sub(2));
    let rect = Rect {
        x: (buf.width.saturating_sub(w)) / 2,
        y: (buf.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    buf.fill(rect, th.bg);
    let inner = draw_box(buf, rect, " Confirm delete ", th.danger, th.danger);

    let n = app.collector.len();
    let total = human_bytes(app.collector.total_bytes());
    let lines: Vec<String> = if needs_typed_confirm() {
        vec![
            format!("Permanently remove {n} items ({total})."),
            "Trash is unavailable as root (or no XDG trash).".into(),
            String::new(),
            format!("Type {DELETE_CONFIRM_PHRASE} then press Enter. Escape cancels."),
            String::new(),
            format!("> {typed}"),
        ]
    } else {
        vec![
            format!("Move {n} items ({total}) to trash."),
            String::new(),
            "Review the collector. Enter confirms.".into(),
            "Escape cancels.".into(),
            String::new(),
            format!("> {typed}"),
        ]
    };
    for (i, line) in lines.iter().enumerate() {
        if (i as u16) < inner.height {
            buf.print(
                inner.x + 1,
                inner.y + i as u16,
                &truncate(line, inner.width.saturating_sub(2) as usize),
                th.text,
                th.bg,
            );
        }
    }

    let by = inner.bottom().saturating_sub(1);
    let cancel = Rect {
        x: inner.x + 1,
        y: by,
        width: 8,
        height: 1,
    };
    buf.print(cancel.x, by, " Cancel ", th.text, th.panel);
    let ok = Rect {
        x: inner.x + 11,
        y: by,
        width: 10,
        height: 1,
    };
    buf.print_styled(ok.x, by, "  Delete  ", th.bg, th.danger, true);
    hits.buttons.push((cancel, Action::Cancel));
    hits.buttons.push((ok, Action::ConfirmDelete));
}

fn draw_update_modal(buf: &mut Buffer, app: &App, hits: &mut HitMap) {
    let Some(offer) = app.update_offer.as_ref() else {
        return;
    };
    let th = theme::current();
    let w = 62.min(buf.width.saturating_sub(4)).max(40);
    let h = 12.min(buf.height.saturating_sub(2));
    let rect = Rect {
        x: (buf.width.saturating_sub(w)) / 2,
        y: (buf.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    };
    buf.fill(rect, th.panel);
    let inner = draw_box(buf, rect, " update ", th.accent, th.accent);

    let current = crate::update::running_version();
    let lines: Vec<(String, Rgb)> = if offer.writable {
        vec![
            (format!("rings {} is out", offer.version), th.text),
            (format!("you have {current}"), th.muted),
            (String::new(), th.muted),
            ("Ctrl+U  update and restart".into(), th.accent),
            ("Esc     dismiss".into(), th.muted),
        ]
    } else {
        vec![
            (format!("rings {} is out", offer.version), th.text),
            (format!("you have {current}"), th.muted),
            (String::new(), th.muted),
            ("this install is not writable".into(), th.warn),
            (
                truncate(
                    crate::update::installer_hint(),
                    inner.width.saturating_sub(2) as usize,
                )
                .into_owned(),
                th.muted,
            ),
            ("Esc     dismiss".into(), th.muted),
        ]
    };
    for (i, (line, fg)) in lines.iter().enumerate() {
        let y = inner.y + i as u16;
        if y >= inner.bottom().saturating_sub(2) {
            break;
        }
        buf.print(
            inner.x + 2,
            y,
            &truncate(line, inner.width.saturating_sub(4) as usize),
            *fg,
            th.panel,
        );
    }

    let by = inner.bottom().saturating_sub(1);
    let dismiss = Rect {
        x: inner.x + 1,
        y: by,
        width: 9,
        height: 1,
    };
    buf.print(dismiss.x, by, " Dismiss ", th.text, th.chip);
    hits.buttons.push((dismiss, Action::DismissUpdate));
    if offer.writable {
        let ok = Rect {
            x: inner.x + 12,
            y: by,
            width: 8,
            height: 1,
        };
        buf.print_styled(ok.x, by, " Update ", th.bg, th.accent, true);
        hits.buttons.push((ok, Action::ApplyUpdate));
    }
}

/// Rows a group takes: title, keys, optional note.
fn group_height(g: &KeyGroup) -> u16 {
    1 + g.keys.len() as u16 + u16::from(g.note.is_some())
}

/// Rows a column of groups takes, with a blank line between groups.
fn column_height(groups: &[KeyGroup]) -> u16 {
    groups.iter().map(group_height).sum::<u16>() + groups.len().saturating_sub(1) as u16
}

/// Paint one group at `(x, y)`, clipped to `bottom`. Returns rows used.
fn draw_group(buf: &mut Buffer, x: u16, y: u16, bottom: u16, g: &KeyGroup) -> u16 {
    let th = theme::current();
    let mut ly = y;
    let mut line = |buf: &mut Buffer, f: &mut dyn FnMut(&mut Buffer, u16)| {
        if ly < bottom {
            f(buf, ly);
        }
        ly += 1;
    };
    line(buf, &mut |buf, ly| {
        buf.print_styled(x, ly, g.title, th.accent, th.bg, true);
    });
    for (k, d) in g.keys {
        line(buf, &mut |buf, ly| {
            buf.print_styled(x, ly, k, th.warn, th.bg, true);
            buf.print(x + HELP_KEY_W as u16 + 2, ly, d, th.text, th.bg);
        });
    }
    if let Some(n) = g.note {
        line(buf, &mut |buf, ly| {
            buf.print(x, ly, &truncate(n, HELP_COL_W), th.muted, th.bg);
        });
    }
    ly - y
}

/// Titled groups in two centered columns (one when narrow), the logo above
/// when there is room. Every binding is always visible on 80×24.
fn draw_help(buf: &mut Buffer) -> HitMap {
    let th = theme::current();
    let rect = Rect {
        x: 0,
        y: 0,
        width: buf.width,
        height: buf.height,
    };
    buf.fill(rect, th.bg);
    let inner = draw_box(buf, rect, " keys ", th.accent, th.accent);

    let col_w = HELP_COL_W as u16;
    let gap = if inner.width >= col_w * 2 + 6 { 4 } else { 2 };
    let two_col = inner.width >= col_w * 2 + gap + 2;
    // Split where the taller column is shortest.
    let split = if two_col {
        (1..KEY_GROUPS.len())
            .min_by_key(|&i| column_height(&KEY_GROUPS[..i]).max(column_height(&KEY_GROUPS[i..])))
            .unwrap_or(1)
    } else {
        KEY_GROUPS.len()
    };
    let (left, right) = KEY_GROUPS.split_at(split);
    let rows = column_height(left).max(column_height(right));
    let block_w = if two_col { col_w * 2 + gap } else { col_w };

    let (lw, lh) = logo::size();
    let footer_h = 2u16; // blank + close row
    let show_logo = inner.height > rows + footer_h + lh;
    let block_h = rows + footer_h + if show_logo { lh + 1 } else { 0 };
    let mut y = inner.y + inner.height.saturating_sub(block_h) / 2;
    let x0 = inner.x + inner.width.saturating_sub(block_w) / 2;

    if show_logo {
        paint_logo(buf, inner.x + inner.width.saturating_sub(lw) / 2, y, true);
        y += lh + 1;
    }

    for (cx, groups) in [(x0, left), (x0 + col_w + gap, right)] {
        let mut ly = y;
        for (i, g) in groups.iter().enumerate() {
            if i > 0 {
                ly += 1;
            }
            ly += draw_group(buf, cx, ly, inner.bottom(), g);
        }
    }

    let cy = (y + rows + 1).min(inner.bottom().saturating_sub(1));
    let close = Rect {
        x: x0,
        y: cy,
        width: 7,
        height: 1,
    };
    buf.print_styled(close.x, close.y, " Close ", th.bg, th.accent, true);
    draw_hints(buf, close.right() + 2, cy, &[("Esc", "or ? F1 closes")]);
    let mut hits = HitMap::empty();
    hits.buttons.push((close, Action::Back));
    hits
}

/// Color the shared logo: gold rays, nested ring hues, bright center.
fn paint_logo(buf: &mut Buffer, x: u16, y: u16, color: bool) {
    let th = theme::current();
    let (lw, _) = logo::size();
    for (i, line) in logo::lines().enumerate() {
        let row = y.saturating_add(i as u16);
        if row >= buf.height {
            break;
        }
        let mut cx = x;
        for (col, ch) in line.chars().enumerate() {
            if cx >= buf.width {
                break;
            }
            let (fg, bold) = if color {
                logo_glyph_color(ch, col, lw as usize)
            } else {
                (th.text, false)
            };
            buf.set_cell(
                cx,
                row,
                Cell {
                    ch,
                    fg,
                    bg: th.bg,
                    bold,
                },
            );
            cx = cx.saturating_add(1);
        }
    }
}

fn logo_glyph_color(ch: char, col: usize, width: usize) -> (Rgb, bool) {
    let th = theme::current();
    let mid = width / 2;
    let dist = col.abs_diff(mid);
    match ch {
        '◎' => (th.accent, true),
        '╭' | '╮' | '╰' | '╯' if dist <= 3 => (th.palette[0], true),
        '╭' | '╮' | '╰' | '╯' => (th.palette[1], false),
        '─' | '│' if dist <= 3 => (th.palette[0], false),
        '─' | '│' if dist <= 6 => (th.palette[1], false),
        '─' | '│' | '╲' | '╱' | '·' => (th.warn, false),
        _ => (th.text, false),
    }
}

/// Bordered box; returns the inner rect.
fn draw_box(buf: &mut Buffer, area: Rect, title: &str, title_fg: Rgb, border: Rgb) -> Rect {
    let th = theme::current();
    if area.width < 4 || area.height < 3 {
        return area;
    }
    let right = area.right() - 1;
    let bottom = area.bottom() - 1;
    buf.print(area.x, area.y, "╭", border, th.bg);
    buf.print(right, area.y, "╮", border, th.bg);
    buf.print(area.x, bottom, "╰", border, th.bg);
    buf.print(right, bottom, "╯", border, th.bg);
    for x in (area.x + 1)..right {
        buf.print(x, area.y, "─", border, th.bg);
        buf.print(x, bottom, "─", border, th.bg);
    }
    for y in (area.y + 1)..bottom {
        buf.print(area.x, y, "│", border, th.bg);
        buf.print(right, y, "│", border, th.bg);
    }
    buf.print(
        area.x + 2,
        area.y,
        &truncate(title, area.width.saturating_sub(4) as usize),
        title_fg,
        th.bg,
    );
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    }
}

/// Tint one list row (selection beats hover, hover is a whisper of it) and
/// register it for hit testing.
fn list_row(
    buf: &mut Buffer,
    hits: &mut HitMap,
    area: Rect,
    y: u16,
    index: usize,
    selected: bool,
    hovered: bool,
) -> Rgb {
    let th = theme::current();
    let bg = row_background(th, selected, hovered);
    let rect = Rect {
        x: area.x,
        y,
        width: area.width,
        height: 1,
    };
    if bg != th.bg {
        buf.fill(rect, bg);
    }
    hits.rows.push((rect, index));
    bg
}

/// Selection beats hover; hover is a whisper of the selection color.
fn row_background(th: &theme::Theme, selected: bool, hovered: bool) -> Rgb {
    if selected {
        th.select_bg
    } else if hovered {
        th.hover_bg()
    } else {
        th.bg
    }
}

/// Clip to `max` cells with an ellipsis. Borrows when it already fits, which
/// is the common case on every list row.
pub fn truncate(s: &str, max: usize) -> Cow<'_, str> {
    if max == 0 {
        return Cow::Borrowed("");
    }
    if s.chars().count() <= max {
        return Cow::Borrowed(s);
    }
    if max <= 1 {
        return Cow::Borrowed("…");
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    Cow::Owned(out)
}
