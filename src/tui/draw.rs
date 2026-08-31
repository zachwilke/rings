use crate::cli::KEY_LINES;
use crate::constants::{CHIP_GAP, DELETE_CONFIRM_PHRASE, FOOTER_H};
use crate::delete::needs_typed_confirm;
use crate::logo;
use crate::size::{group_u64, human_bytes};
use crate::term::{Buffer, Cell, Rect, Rgb};
use crate::tui::app::{Action, App, View};
use crate::tui::sunburst::{self, Slice};
use crate::tui::theme::{
    self, category_color, ACCENT, BG, CHIP, DANGER, MUTED, PALETTE, PANEL, SELECT_BG, SMALLER,
    TEXT, WARN,
};

pub struct HitMap {
    pub sunburst: Rect,
    pub list: Rect,
    pub buttons: Vec<(Rect, Action)>,
    pub crumbs: Vec<(Rect, usize)>,
    pub slices: Vec<Slice>,
}

impl HitMap {
    pub fn empty() -> Self {
        Self {
            sunburst: Rect::ZERO,
            list: Rect::ZERO,
            buttons: Vec::new(),
            crumbs: Vec::new(),
            slices: Vec::new(),
        }
    }
}

pub fn draw(buf: &mut Buffer, app: &App) -> HitMap {
    buf.fill(buf.area(), BG);
    match app.view {
        View::Scanning => {
            draw_scan(buf, app);
            HitMap::empty()
        }
        View::Help => {
            draw_help(buf);
            HitMap::empty()
        }
        View::Confirm { .. } => {
            let mut hits = draw_main(buf, app);
            draw_confirm_modal(buf, app, &mut hits);
            hits
        }
        _ => draw_main(buf, app),
    }
}

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn draw_scan(buf: &mut Buffer, app: &App) {
    let x = buf.print_styled(1, 0, "◎ rings ", TEXT, BG, true);
    buf.print(x, 0, "scanning", MUTED, BG);

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
    let x = buf.print(px, cy, spinner, ACCENT, BG);
    buf.print(
        x + 1,
        cy,
        &truncate(&path_line, buf.width.saturating_sub(4) as usize),
        ACCENT,
        BG,
    );

    let counts = format!(
        "{} files   {} dirs   {} errors",
        group_u64(files),
        group_u64(dirs),
        group_u64(errors)
    );
    let cx = (buf.width.saturating_sub(counts.chars().count() as u16)) / 2;
    buf.print_styled(cx, cy.saturating_add(2), &counts, TEXT, BG, true);

    let shown = truncate(&current, buf.width.saturating_sub(6) as usize);
    let sx = (buf.width.saturating_sub(shown.chars().count() as u16)) / 2;
    buf.print(sx, cy.saturating_add(3), &shown, MUTED, BG);

    let hint = if app.is_root {
        crate::sys::scan_banner_privileged()
    } else {
        crate::sys::scan_banner_unprivileged()
    };
    let y = buf.height.saturating_sub(2);
    let hx = (buf.width.saturating_sub(hint.chars().count() as u16)) / 2;
    buf.print(hx, y, hint, MUTED, BG);
}

fn draw_main(buf: &mut Buffer, app: &App) -> HitMap {
    let header = Rect {
        x: 0,
        y: 0,
        width: buf.width,
        height: 1,
    };
    let footer_h = FOOTER_H.min(buf.height);
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

    let crumbs = draw_header(buf, app, header);
    let mut hits = HitMap {
        sunburst: Rect::ZERO,
        list: Rect::ZERO,
        buttons: Vec::new(),
        crumbs,
        slices: Vec::new(),
    };

    match app.view {
        View::Findings => draw_findings(buf, app, body, &mut hits),
        View::Collector => draw_collector(buf, app, body, &mut hits),
        _ => draw_browse(buf, app, body, &mut hits),
    }

    hits.buttons = draw_footer(buf, app, footer);
    hits
}

fn draw_header(buf: &mut Buffer, app: &App, area: Rect) -> Vec<(Rect, usize)> {
    let mut crumbs = Vec::new();
    let mut x = buf.print_styled(area.x + 1, area.y, "◎ rings ", TEXT, BG, true);
    for (i, (label, nid)) in app.breadcrumb().iter().enumerate() {
        if i > 0 {
            x = buf.print(x, area.y, " › ", MUTED, BG);
        }
        let shown = truncate(label, 24);
        let w = shown.chars().count() as u16;
        let end = buf.print(x, area.y, &shown, ACCENT, BG);
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
    hits.sunburst = left;
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
    let selected = app.selected_id();
    let rings = sunburst::rings_for(left);
    let slices = sunburst::build_slices(tree, current, app.apparent, selected, rings);
    sunburst::render(buf, left, &slices);
    hits.slices = slices;

    // Size label in the hole.
    let node = tree.get(current);
    let label = human_bytes(node.display_size(app.apparent));
    let name = truncate(&node.name, 18);
    if left.height > 4 {
        let cy = left.y + left.height / 2;
        let lx = left.x + (left.width.saturating_sub(label.chars().count() as u16)) / 2;
        buf.print(lx, cy.saturating_sub(1), &label, ACCENT, BG);
        let nx = left.x + (left.width.saturating_sub(name.chars().count() as u16)) / 2;
        buf.print(nx, cy, &name, MUTED, BG);
    }

    for y in right.y..right.bottom() {
        buf.print(right.x, y, "│", MUTED, BG);
    }
    draw_child_list(buf, app, hits.list);
}

fn draw_child_list(buf: &mut Buffer, app: &App, area: Rect) {
    let Some(tree) = app.tree() else {
        return;
    };
    let current = tree.node_at(&app.cwd);
    let kids = &tree.get(current).children;
    if kids.is_empty() {
        buf.print(area.x + 1, area.y, "empty", MUTED, BG);
        return;
    }
    let h = area.height as usize;
    let start = app.list_offset.min(kids.len().saturating_sub(1));
    for (row, (i, &cid)) in kids.iter().enumerate().skip(start).take(h).enumerate() {
        let y = area.y + row as u16;
        let n = tree.get(cid);
        let sel = i == app.selected;
        let row_bg = if sel { SELECT_BG } else { BG };
        if sel {
            buf.fill(
                Rect {
                    x: area.x,
                    y,
                    width: area.width,
                    height: 1,
                },
                row_bg,
            );
        }
        let color = if n.category.is_waste() {
            category_color(n.category)
        } else {
            theme::PALETTE[i % theme::PALETTE.len()]
        };
        let dot = if app.collector.contains_path(&n.path) {
            "●"
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
        buf.print_styled(x, y, &name, TEXT, row_bg, sel);
        let sx = area.right().saturating_sub(size_w);
        buf.print(sx, y, &size, if sel { TEXT } else { MUTED }, row_bg);
    }
}

fn draw_findings(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let Some(tree) = app.tree() else {
        return;
    };
    let ids = app.finding_ids();
    let title = format!(
        " Temp & cache · {} hits (inspect, then mark — never auto-deleted) ",
        ids.len()
    );
    let inner = draw_box(buf, area, &title, ACCENT, MUTED);
    hits.list = inner;

    if ids.is_empty() {
        buf.print(
            inner.x,
            inner.y,
            "No temp, cache, log, journal, or crash paths in this scan.",
            MUTED,
            BG,
        );
        return;
    }

    let h = inner.height as usize;
    let start = app.list_offset.min(ids.len().saturating_sub(1));
    for (row, (i, &id)) in ids.iter().enumerate().skip(start).take(h).enumerate() {
        let y = inner.y + row as u16;
        let n = tree.get(id);
        let sel = i == app.findings_selected;
        let row_bg = if sel { SELECT_BG } else { BG };
        if sel {
            buf.fill(
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
                row_bg,
            );
        }
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
        let path = truncate(&n.path.to_string_lossy(), path_w);
        buf.print_styled(x, y, &path, TEXT, row_bg, sel);
        buf.print(
            inner.right().saturating_sub(size_w),
            y,
            &size,
            if sel { TEXT } else { MUTED },
            row_bg,
        );
    }
}

fn draw_collector(buf: &mut Buffer, app: &App, area: Rect, hits: &mut HitMap) {
    let title = format!(
        " Collector · {} · {} — nothing deleted until you confirm ",
        app.collector.len(),
        human_bytes(app.collector.total_bytes())
    );
    let inner = draw_box(buf, area, &title, WARN, WARN);
    hits.list = inner;

    if app.collector.is_empty() {
        buf.print(
            inner.x,
            inner.y,
            "Empty. Mark items with Space or d from the sunburst or Temp & cache view.",
            MUTED,
            BG,
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
        let row_bg = if sel { SELECT_BG } else { BG };
        if sel {
            buf.fill(
                Rect {
                    x: inner.x,
                    y,
                    width: inner.width,
                    height: 1,
                },
                row_bg,
            );
        }
        let size = human_bytes(item.size_bytes);
        let size_w = size.chars().count() as u16 + 1;
        let x = buf.print(inner.x, y, " ● ", DANGER, row_bg);
        let path_w = inner.width.saturating_sub(3 + size_w + 1) as usize;
        let path = truncate(&item.path.to_string_lossy(), path_w);
        buf.print_styled(x, y, &path, TEXT, row_bg, sel);
        buf.print(
            inner.right().saturating_sub(size_w),
            y,
            &size,
            if sel { TEXT } else { MUTED },
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
    let Some(tree) = app.tree() else {
        return;
    };
    let n = tree.get(tree.node_at(&app.cwd));
    let mut x = buf.print(1, y, "used ", MUTED, BG);
    x = buf.print_styled(x, y, &human_bytes(n.used), TEXT, BG, true);
    x = buf.print(x, y, "  ·  apparent ", MUTED, BG);
    x = buf.print_styled(x, y, &human_bytes(n.apparent), TEXT, BG, true);
    let stats = format!(
        "  ·  {} files · {} dirs · {} errors · {} hardlinks skipped  ",
        group_u64(tree.stats.files),
        group_u64(tree.stats.dirs),
        group_u64(tree.stats.errors),
        group_u64(tree.stats.hardlinks_deduped)
    );
    x = buf.print(x, y, &stats, MUTED, BG);
    if let Some(hint) = app.not_root_hint() {
        buf.print(
            x,
            y,
            &truncate(&hint, width.saturating_sub(x) as usize),
            WARN,
            BG,
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
        action: Action::Collector,
        label: format!(" Collector ({}) ", app.collector.len()),
        keep: 2,
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
        (View::Findings, Action::Findings) | (View::Collector, Action::Collector)
    )
}

fn draw_footer_chrome(buf: &mut Buffer, app: &App, y: u16, width: u16) -> Vec<(Rect, Action)> {
    let budget = width.saturating_sub(2);
    let chips = fit_chips(footer_chips(app), budget);
    let mut buttons = Vec::new();
    let mut x = 1u16;
    for chip in &chips {
        let w = chip.label.chars().count() as u16;
        if x.saturating_add(w) > width {
            break;
        }
        let (fg, bg_c, bold) = if chip.action == Action::ConfirmDelete {
            (BG, DANGER, true)
        } else if chip_is_active(app, chip.action) {
            (BG, ACCENT, true)
        } else {
            (TEXT, CHIP, false)
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
    for (i, (key, verb)) in hints.iter().enumerate() {
        if i > 0 {
            x = buf.print(x, y, " · ", SMALLER, BG);
        }
        x = buf.print_styled(x, y, key, TEXT, BG, true);
        x = buf.print(x, y, " ", MUTED, BG);
        x = buf.print(x, y, verb, MUTED, BG);
    }
}

fn draw_footer_path(buf: &mut Buffer, app: &App, y: u16, width: u16) {
    let line = if !app.status.is_empty() {
        app.status.clone()
    } else {
        app.selected_path().unwrap_or_default()
    };
    if line.is_empty() {
        return;
    }
    buf.print(
        1,
        y,
        &truncate(&line, width.saturating_sub(2) as usize),
        MUTED,
        BG,
    );
}

fn draw_confirm_modal(buf: &mut Buffer, app: &App, hits: &mut HitMap) {
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
    buf.fill(rect, BG);
    let inner = draw_box(buf, rect, " Confirm delete ", DANGER, DANGER);

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
                TEXT,
                BG,
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
    buf.print(cancel.x, by, " Cancel ", TEXT, PANEL);
    let ok = Rect {
        x: inner.x + 11,
        y: by,
        width: 10,
        height: 1,
    };
    buf.print_styled(ok.x, by, "  Delete  ", BG, DANGER, true);
    hits.buttons.push((cancel, Action::Cancel));
    hits.buttons.push((ok, Action::ConfirmDelete));
}

fn draw_help(buf: &mut Buffer) {
    let (lw, lh) = logo::size();
    // Use the full screen so a 24-row SSH session still shows every binding.
    let rect = Rect {
        x: 0,
        y: 0,
        width: buf.width,
        height: buf.height,
    };
    buf.fill(rect, BG);
    let inner = draw_box(buf, rect, " keys  ·  ? and F1 ", ACCENT, ACCENT);

    let lx = inner.x + (inner.width.saturating_sub(lw)) / 2;
    paint_logo(buf, lx, inner.y, true);

    let mut y = inner.y.saturating_add(lh).saturating_add(1);
    for line in KEY_LINES {
        if y >= inner.bottom() {
            break;
        }
        buf.print(
            inner.x + 2,
            y,
            &truncate(line, inner.width.saturating_sub(4) as usize),
            if line.is_empty() || *line == "Mouse" {
                MUTED
            } else {
                TEXT
            },
            BG,
        );
        y = y.saturating_add(1);
    }
}

/// Color the shared logo: gold rays, nested ring hues, bright center.
fn paint_logo(buf: &mut Buffer, x: u16, y: u16, color: bool) {
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
                (TEXT, false)
            };
            buf.set_cell(
                cx,
                row,
                Cell {
                    ch,
                    fg,
                    bg: BG,
                    bold,
                },
            );
            cx = cx.saturating_add(1);
        }
    }
}

fn logo_glyph_color(ch: char, col: usize, width: usize) -> (Rgb, bool) {
    let mid = width / 2;
    let dist = col.abs_diff(mid);
    match ch {
        '◎' => (ACCENT, true),
        '╭' | '╮' | '╰' | '╯' if dist <= 3 => (PALETTE[0], true),
        '╭' | '╮' | '╰' | '╯' => (PALETTE[1], false),
        '─' | '│' if dist <= 3 => (PALETTE[0], false),
        '─' | '│' if dist <= 6 => (PALETTE[1], false),
        '─' | '│' | '╲' | '╱' | '·' => (WARN, false),
        _ => (TEXT, false),
    }
}

/// Bordered box; returns the inner rect.
fn draw_box(buf: &mut Buffer, area: Rect, title: &str, title_fg: Rgb, border: Rgb) -> Rect {
    if area.width < 4 || area.height < 3 {
        return area;
    }
    let right = area.right() - 1;
    let bottom = area.bottom() - 1;
    buf.print(area.x, area.y, "┌", border, BG);
    buf.print(right, area.y, "┐", border, BG);
    buf.print(area.x, bottom, "└", border, BG);
    buf.print(right, bottom, "┘", border, BG);
    for x in (area.x + 1)..right {
        buf.print(x, area.y, "─", border, BG);
        buf.print(x, bottom, "─", border, BG);
    }
    for y in (area.y + 1)..bottom {
        buf.print(area.x, y, "│", border, BG);
        buf.print(right, y, "│", border, BG);
    }
    buf.print(
        area.x + 2,
        area.y,
        &truncate(title, area.width.saturating_sub(4) as usize),
        title_fg,
        BG,
    );
    Rect {
        x: area.x + 1,
        y: area.y + 1,
        width: area.width - 2,
        height: area.height - 2,
    }
}

pub fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    let count = s.chars().count();
    if count <= max {
        return s.to_string();
    }
    if max <= 1 {
        return "…".into();
    }
    let mut out: String = s.chars().take(max - 1).collect();
    out.push('…');
    out
}

pub fn row_index_at(list: Rect, y: u16, offset: usize, len: usize) -> Option<usize> {
    if y < list.y || y >= list.bottom() {
        return None;
    }
    let row = (y - list.y) as usize + offset;
    if row < len {
        Some(row)
    } else {
        None
    }
}
