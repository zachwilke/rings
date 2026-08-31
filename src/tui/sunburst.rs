//! Polar sunburst drawn with Braille 2×4 dots into the term buffer.

use std::f64::consts::TAU;

use crate::classify::Category;
use crate::constants::{
    MIN_SLICE_ANGLE, MIN_SLICE_FRACTION, SUNBURST_HOLE_LABEL_PX, SUNBURST_HOLE_MAX,
    SUNBURST_HOLE_MIN, SUNBURST_MARGIN, SUNBURST_RINGS_MAX, SUNBURST_RINGS_MIN, SUNBURST_RING_PX,
};
use crate::scan::Tree;
use crate::term::{Buffer, Cell, Rect, Rgb};
use crate::tui::theme::{self, brighten, category_color, dim_color, separator_color};

#[derive(Clone, Debug)]
pub struct Slice {
    pub start: f64,
    pub end: f64,
    pub ring: usize,
    pub node: usize,
    pub color: Rgb,
    pub grouped: bool,
}

/// How many rings this panel can hold without turning to noise.
pub fn rings_for(area: Rect) -> usize {
    geometry(area)
        .map(|g| g.rings)
        .unwrap_or(SUNBURST_RINGS_MIN)
}

pub fn build_slices(
    tree: &Tree,
    current: usize,
    apparent: bool,
    selected: Option<usize>,
    rings: usize,
) -> Vec<Slice> {
    let rings = rings.clamp(1, SUNBURST_RINGS_MAX);
    let mut slices = Vec::new();
    layout(
        tree,
        current,
        0.0,
        1.0,
        0,
        apparent,
        0,
        selected,
        rings,
        &mut slices,
    );
    slices
}

#[allow(clippy::too_many_arguments)]
fn layout(
    tree: &Tree,
    id: usize,
    start: f64,
    end: f64,
    ring: usize,
    apparent: bool,
    color_seed: usize,
    selected: Option<usize>,
    rings: usize,
    out: &mut Vec<Slice>,
) {
    let th = theme::current();
    if ring >= rings {
        return;
    }
    let children = &tree.get(id).children;
    if children.is_empty() {
        return;
    }
    let parent_size = tree.get(id).display_size(apparent).max(1);
    let span = end - start;
    let mut cursor = start;
    let mut small_span = 0.0;
    let mut color_i = color_seed;

    for &cid in children {
        let frac = tree.get(cid).display_size(apparent) as f64 / parent_size as f64;
        let slice_span = frac * span;
        if frac < MIN_SLICE_FRACTION || slice_span < MIN_SLICE_ANGLE {
            small_span += slice_span;
            continue;
        }
        let slice_end = (cursor + slice_span).min(end);
        let color = slice_color(tree, cid, color_i, ring, rings, selected);
        out.push(Slice {
            start: cursor,
            end: slice_end,
            ring,
            node: cid,
            color,
            grouped: false,
        });
        layout(
            tree,
            cid,
            cursor,
            slice_end,
            ring + 1,
            apparent,
            color_i,
            selected,
            rings,
            out,
        );
        cursor = slice_end;
        color_i += 1;
    }
    // Close the parent span so own-inode leftovers and dust share one
    // obvious remainder wedge instead of leaving a hole in the ring.
    if cursor < end && (small_span > 0.0 || cursor > start) {
        out.push(Slice {
            start: cursor,
            end,
            ring,
            node: id,
            color: dim_color(th.smaller, ring, rings),
            grouped: true,
        });
    }
}

fn slice_color(
    tree: &Tree,
    id: usize,
    color_i: usize,
    ring: usize,
    rings: usize,
    selected: Option<usize>,
) -> Rgb {
    let th = theme::current();
    let cat = tree.get(id).category;
    let base = if cat != Category::Normal {
        category_color(cat)
    } else {
        th.palette[color_i % th.palette.len()]
    };
    let mut c = dim_color(base, ring, rings);
    if selected == Some(id) || selected.is_some_and(|s| is_descendant(tree, s, id)) {
        c = brighten(c);
    }
    c
}

fn is_descendant(tree: &Tree, ancestor: usize, node: usize) -> bool {
    let mut id = node;
    while let Some(p) = tree.get(id).parent {
        if p == ancestor {
            return true;
        }
        id = p;
    }
    false
}

fn polar(dx: f64, dy: f64, hole: f64, rings: usize) -> Option<(usize, f64)> {
    let r = (dx * dx + dy * dy).sqrt();
    if r < hole || r > 1.0 {
        return None;
    }
    let mut t = dy.atan2(dx) + std::f64::consts::FRAC_PI_2;
    if t < 0.0 {
        t += TAU;
    }
    let angle = t / TAU;
    let ring_span = 1.0 - hole;
    if ring_span <= 0.0 || rings == 0 {
        return None;
    }
    let ring = (((r - hole) / ring_span) * rings as f64).floor() as usize;
    Some((ring.min(rings - 1), angle))
}

/// Braille cell: 2 dots wide × 4 tall. Same 1:2 aspect as half-blocks, 4× the samples.
const BRAILLE_COLS: usize = 2;
const BRAILLE_ROWS: usize = 4;

/// Unicode Braille bits in btop order (dots 1–8):
/// ```text
/// 1 4
/// 2 5
/// 3 6
/// 7 8
/// ```
const BRAILLE_DOT: [[u8; BRAILLE_COLS]; BRAILLE_ROWS] =
    [[0x01, 0x08], [0x02, 0x10], [0x04, 0x20], [0x40, 0x80]];

const BRAILLE_BASE: u32 = 0x2800;

/// Disk geometry inside a cell rect. Braille pixels are roughly square
/// (a cell is 2×4), so a true circle uses one radius in (2×cols, 4×rows)
/// pixel space. Hole and ring count are derived from that radius so paint
/// and hit testing stay on the same polar map.
struct Geometry {
    cx_px: f64,
    cy_px: f64,
    radius: f64,
    hole: f64,
    rings: usize,
}

fn hole_for(radius: f64) -> f64 {
    (SUNBURST_HOLE_LABEL_PX / radius).clamp(SUNBURST_HOLE_MIN, SUNBURST_HOLE_MAX)
}

fn rings_from_radius(radius: f64, hole: f64) -> usize {
    let usable = radius * (1.0 - hole);
    let n = (usable / SUNBURST_RING_PX).floor() as usize;
    n.clamp(SUNBURST_RINGS_MIN, SUNBURST_RINGS_MAX)
}

fn geometry(area: Rect) -> Option<Geometry> {
    if area.width < 6 || area.height < 4 {
        return None;
    }
    let w_px = area.width as f64 * BRAILLE_COLS as f64;
    let h_px = area.height as f64 * BRAILLE_ROWS as f64;
    let radius = ((w_px.min(h_px)) / 2.0 - SUNBURST_MARGIN).max(4.0);
    let hole = hole_for(radius);
    let rings = rings_from_radius(radius, hole);
    Some(Geometry {
        cx_px: w_px / 2.0,
        cy_px: h_px / 2.0,
        radius,
        hole,
        rings,
    })
}

fn norm(geo: &Geometry, px: f64, py: f64) -> (f64, f64) {
    ((px - geo.cx_px) / geo.radius, (py - geo.cy_px) / geo.radius)
}

fn sample_px(slices: &[Slice], geo: &Geometry, px: f64, py: f64) -> Option<Rgb> {
    let (dx, dy) = norm(geo, px, py);
    sample(slices, geo, dx, dy)
}

fn slice_at_px<'a>(
    slices: &'a [Slice],
    geo: &Geometry,
    px: f64,
    py: f64,
    allow_grouped: bool,
) -> Option<&'a Slice> {
    let (dx, dy) = norm(geo, px, py);
    let (ring, angle) = polar(dx, dy, geo.hole, geo.rings)?;
    slice_at(slices, ring, angle, allow_grouped)
}

/// Map a terminal cell to a slice for mouse hits.
/// Majority of the 8 braille samples, same polar map as paint. Grouped leftovers stay unclickable.
pub fn hit_slice<'a>(slices: &'a [Slice], area: Rect, x: u16, y: u16) -> Option<&'a Slice> {
    if !area.contains(x, y) {
        return None;
    }
    let geo = geometry(area)?;
    majority_slice(slices, &geo, x - area.x, y - area.y, false)
}

fn majority_slice<'a>(
    slices: &'a [Slice],
    geo: &Geometry,
    tx: u16,
    ty: u16,
    allow_grouped: bool,
) -> Option<&'a Slice> {
    let mut counts: Vec<(&'a Slice, usize)> = Vec::new();
    for row in 0..BRAILLE_ROWS {
        for col in 0..BRAILLE_COLS {
            let px = tx as f64 * BRAILLE_COLS as f64 + col as f64 + 0.5;
            let py = ty as f64 * BRAILLE_ROWS as f64 + row as f64 + 0.5;
            let Some(s) = slice_at_px(slices, geo, px, py, allow_grouped) else {
                continue;
            };
            if let Some(slot) = counts.iter_mut().find(|(c, _)| {
                c.node == s.node && c.ring == s.ring && c.start == s.start && c.end == s.end
            }) {
                slot.1 += 1;
            } else {
                counts.push((s, 1));
            }
        }
    }
    counts.into_iter().max_by_key(|(_, n)| *n).map(|(s, _)| s)
}

/// Paint with Braille: each cell is an 8-dot 2×4 pixel grid.
pub fn render(buf: &mut Buffer, area: Rect, slices: &[Slice]) {
    let Some(geo) = geometry(area) else {
        return;
    };
    let bg = theme::current().bg;
    for ty in 0..area.height {
        for tx in 0..area.width {
            let mut dots = [[None; BRAILLE_COLS]; BRAILLE_ROWS];
            for row in 0..BRAILLE_ROWS {
                for col in 0..BRAILLE_COLS {
                    let px = tx as f64 * BRAILLE_COLS as f64 + col as f64 + 0.5;
                    let py = ty as f64 * BRAILLE_ROWS as f64 + row as f64 + 0.5;
                    dots[row][col] = sample_px(slices, &geo, px, py);
                }
            }
            paint_braille(buf, area.x + tx, area.y + ty, dots, bg);
        }
    }
}

fn braille_char(bits: u8) -> char {
    char::from_u32(BRAILLE_BASE | u32::from(bits)).unwrap_or(' ')
}

/// True when `ch` is a Braille sunburst glyph (U+2800..U+28FF).
#[cfg(test)]
pub fn is_braille(ch: char) -> bool {
    (BRAILLE_BASE..=BRAILLE_BASE + 0xFF).contains(&(ch as u32))
}

/// Rasterize one terminal cell to a 2-wide × 4-tall pixel grid for PPM dumps.
/// Braille uses the 8 dots; leftover half-block glyphs (logo) stay compatible.
#[cfg(test)]
pub fn raster_cell(cell: Cell) -> [[Rgb; BRAILLE_COLS]; BRAILLE_ROWS] {
    if is_braille(cell.ch) {
        let bits = (cell.ch as u32).saturating_sub(BRAILLE_BASE) as u8;
        let mut out = [[cell.bg; BRAILLE_COLS]; BRAILLE_ROWS];
        for row in 0..BRAILLE_ROWS {
            for col in 0..BRAILLE_COLS {
                if bits & BRAILLE_DOT[row][col] != 0 {
                    out[row][col] = cell.fg;
                }
            }
        }
        return out;
    }
    let (top, bot) = match cell.ch {
        '█' => (cell.fg, cell.fg),
        '▀' => (cell.fg, cell.bg),
        '▄' => (cell.bg, cell.fg),
        _ => (cell.bg, cell.bg),
    };
    [[top, top], [top, top], [bot, bot], [bot, bot]]
}

/// Deepest slice at this angle on `ring`, or an inner slice that continues
/// outward (leaves fill to the rim so empty outer rings do not hollow the disk).
fn slice_at(slices: &[Slice], ring: usize, angle: f64, allow_grouped: bool) -> Option<&Slice> {
    slices
        .iter()
        .rev()
        .find(|s| {
            (allow_grouped || !s.grouped) && s.ring == ring && angle >= s.start && angle < s.end
        })
        .or_else(|| {
            slices.iter().rev().find(|s| {
                (allow_grouped || !s.grouped) && s.ring <= ring && angle >= s.start && angle < s.end
            })
        })
}

fn sample(slices: &[Slice], geo: &Geometry, nx: f64, ny: f64) -> Option<Rgb> {
    let r = (nx * nx + ny * ny).sqrt();
    let (ring, angle) = polar(nx, ny, geo.hole, geo.rings)?;
    let slice = slice_at(slices, ring, angle, true)?;
    let mut c = restep_dim(slice.color, slice.ring, ring, geo.rings);
    if near_wedge_edge(angle, slice.start, slice.end, r * geo.radius) {
        c = separator_color(c);
    }
    Some(c)
}

/// Re-apply the outer-ring fade when a leaf is painted past its own ring.
fn restep_dim(color: Rgb, from_ring: usize, to_ring: usize, rings: usize) -> Rgb {
    if to_ring <= from_ring {
        return color;
    }
    let last = rings.saturating_sub(1).max(1) as f32;
    let k0 = (1.0 - 0.38 * (from_ring as f32 / last).min(1.0)).max(0.05);
    let k1 = 1.0 - 0.38 * (to_ring as f32 / last).min(1.0);
    let t = k1 / k0;
    let Rgb(r, g, b) = color;
    Rgb(
        (r as f32 * t) as u8,
        (g as f32 * t) as u8,
        (b as f32 * t) as u8,
    )
}

fn near_wedge_edge(angle: f64, start: f64, end: f64, r_px: f64) -> bool {
    if r_px < 1.0 {
        return false;
    }
    let span = end - start;
    if span * r_px * TAU < 2.2 {
        return false;
    }
    let da = 0.42 / (r_px * TAU);
    ang_dist(angle, start) < da || ang_dist(angle, end) < da
}

fn ang_dist(a: f64, b: f64) -> f64 {
    let d = (a - b).abs();
    d.min(1.0 - d)
}

fn paint_braille(
    buf: &mut Buffer,
    x: u16,
    y: u16,
    dots: [[Option<Rgb>; BRAILLE_COLS]; BRAILLE_ROWS],
    bg: Rgb,
) {
    let mut colors: Vec<(Rgb, usize)> = Vec::new();
    let mut on_disk = 0usize;
    for row in dots.iter() {
        for sample in row.iter() {
            let Some(c) = *sample else {
                continue;
            };
            on_disk += 1;
            if let Some(slot) = colors.iter_mut().find(|(k, _)| *k == c) {
                slot.1 += 1;
            } else {
                colors.push((c, 1));
            }
        }
    }
    if on_disk == 0 {
        return;
    }
    colors.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0 .0.cmp(&b.0 .0)));
    let fg = colors[0].0;
    let all_on = on_disk == BRAILLE_COLS * BRAILLE_ROWS;
    let bg = if colors.len() == 2 && all_on {
        colors[1].0
    } else {
        bg
    };
    let mut bits = 0u8;
    for row in 0..BRAILLE_ROWS {
        for col in 0..BRAILLE_COLS {
            if dots[row][col] == Some(fg) {
                bits |= BRAILLE_DOT[row][col];
            }
        }
    }
    if bits == 0 {
        return;
    }
    buf.set_cell(
        x,
        y,
        Cell {
            ch: braille_char(bits),
            fg,
            bg,
            bold: false,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Category;
    use crate::scan::{Node, ScanStats, Tree};
    use std::path::{Path, PathBuf};

    fn node(
        name: &str,
        parent: Option<usize>,
        is_dir: bool,
        used: u64,
        category: Category,
        children: Vec<usize>,
    ) -> Node {
        let leaf = children.is_empty();
        Node {
            name: name.into(),
            path: PathBuf::from(name),
            parent,
            children,
            is_dir,
            own_used: if leaf { used } else { 64 },
            own_apparent: if leaf { used } else { 64 },
            used,
            apparent: used,
            category,
            nlink: if is_dir { 2 } else { 1 },
        }
    }

    /// Nested fixture that looks like a small server root: a few fat dirs,
    /// mid-size children, and dust that should group into "smaller objects".
    fn fixture_tree() -> Tree {
        // 0 root
        // 1 usr — 2 lib — 3 x11 — 31/32, plus locale/gcc/dust
        //       — 6 share — 7/8/9
        //       — 10 bin
        // 11 var — 12 log — 13 journal — 34, 14 syslog, 33 kern
        //        — 15 cache — 16 apt, 17 font
        //        — 18 tmp — 19/20
        // 21 home — 22 alice — 23 .cache, 24 src — 35 rings — 36 target
        //         — 25 bob
        // 26 etc
        // 27 crash
        let nodes = vec![
            node(
                "root",
                None,
                true,
                0,
                Category::Normal,
                vec![1, 11, 21, 26, 27],
            ),
            node("usr", Some(0), true, 0, Category::Normal, vec![2, 6, 10]),
            node(
                "lib",
                Some(1),
                true,
                0,
                Category::Normal,
                vec![3, 4, 5, 28, 29, 30],
            ),
            node("x11", Some(2), true, 900, Category::Normal, vec![31, 32]),
            node("locale", Some(2), true, 700, Category::Normal, vec![]),
            node("gcc", Some(2), true, 400, Category::Normal, vec![]),
            node("share", Some(1), true, 0, Category::Normal, vec![7, 8, 9]),
            node("doc", Some(6), true, 500, Category::Normal, vec![]),
            node("man", Some(6), true, 400, Category::Normal, vec![]),
            node("misc", Some(6), true, 200, Category::Normal, vec![]),
            node("bin", Some(1), true, 600, Category::Normal, vec![]),
            node("var", Some(0), true, 0, Category::Normal, vec![12, 15, 18]),
            node("log", Some(11), true, 0, Category::Log, vec![13, 14, 33]),
            node("journal", Some(12), true, 600, Category::Journal, vec![34]),
            node("syslog", Some(12), false, 250, Category::Log, vec![]),
            node("cache", Some(11), true, 0, Category::Cache, vec![16, 17]),
            node("apt", Some(15), true, 500, Category::Cache, vec![]),
            node("font", Some(15), true, 250, Category::Cache, vec![]),
            node("tmp", Some(11), true, 0, Category::Temp, vec![19, 20]),
            node("sess", Some(18), false, 200, Category::Temp, vec![]),
            node("upload", Some(18), false, 180, Category::Temp, vec![]),
            node("home", Some(0), true, 0, Category::Normal, vec![22, 25]),
            node("alice", Some(21), true, 0, Category::Normal, vec![23, 24]),
            node(".cache", Some(22), true, 400, Category::Cache, vec![]),
            node("src", Some(22), true, 350, Category::Normal, vec![35]),
            node("bob", Some(21), true, 400, Category::Normal, vec![]),
            node("etc", Some(0), true, 400, Category::Normal, vec![]),
            node("crash", Some(0), false, 200, Category::Crash, vec![]),
            node("libfoo.so", Some(2), false, 40, Category::Normal, vec![]),
            node("libbar.so", Some(2), false, 30, Category::Normal, vec![]),
            node("libtiny.so", Some(2), false, 20, Category::Normal, vec![]),
            node("libX11", Some(3), false, 500, Category::Normal, vec![]),
            node("libXext", Some(3), false, 280, Category::Normal, vec![]),
            node("kern.log", Some(12), false, 80, Category::Log, vec![]),
            node(
                "system.journal",
                Some(13),
                false,
                500,
                Category::Journal,
                vec![],
            ),
            node("rings", Some(24), true, 200, Category::Normal, vec![36]),
            node("target", Some(35), true, 160, Category::Normal, vec![37]),
            node("debug", Some(36), true, 120, Category::Normal, vec![]),
        ];
        let mut tree = Tree {
            nodes,
            root: 0,
            stats: ScanStats::default(),
        };
        tree.recompute();
        tree
    }

    fn dump_area() -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: 56,
            height: 24,
        }
    }

    fn render_fixture(selected: Option<usize>) -> (Buffer, Vec<Slice>, Rect) {
        let th = theme::current();
        let tree = fixture_tree();
        let area = dump_area();
        let rings = rings_for(area);
        let slices = build_slices(&tree, 0, false, selected, rings);
        let mut buf = Buffer::new(area.width, area.height, th.bg);
        render(&mut buf, area, &slices);
        (buf, slices, area)
    }

    /// Braille buffer → binary PPM (each cell is 2×4 pixels, scaled up).
    fn write_ppm(buf: &Buffer, path: &Path, scale: u32) {
        let th = theme::current();
        let w = buf.width as u32 * BRAILLE_COLS as u32 * scale;
        let h = buf.height as u32 * BRAILLE_ROWS as u32 * scale;
        let mut px = vec![0u8; (w * h * 3) as usize];
        for y in 0..buf.height {
            for x in 0..buf.width {
                let cell = buf.get(x, y).cloned().unwrap_or(Cell::blank(th.bg));
                let dots = raster_cell(cell);
                for row in 0..BRAILLE_ROWS {
                    for col in 0..BRAILLE_COLS {
                        for sy in 0..scale {
                            for sx in 0..scale {
                                put(
                                    &mut px,
                                    w,
                                    (x as u32 * BRAILLE_COLS as u32 + col as u32) * scale + sx,
                                    (y as u32 * BRAILLE_ROWS as u32 + row as u32) * scale + sy,
                                    dots[row][col],
                                );
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

    fn put(px: &mut [u8], w: u32, x: u32, y: u32, c: Rgb) {
        let i = ((y * w + x) * 3) as usize;
        px[i] = c.0;
        px[i + 1] = c.1;
        px[i + 2] = c.2;
    }

    fn grouping_tree() -> Tree {
        let nodes = vec![
            node("root", None, true, 0, Category::Normal, vec![1, 2, 3, 4, 5]),
            node("big", Some(0), true, 1000, Category::Normal, vec![]),
            node("mid", Some(0), true, 80, Category::Normal, vec![]),
            node("speck-a", Some(0), false, 2, Category::Normal, vec![]),
            node("speck-b", Some(0), false, 2, Category::Normal, vec![]),
            node("speck-c", Some(0), false, 2, Category::Normal, vec![]),
        ];
        let mut tree = Tree {
            nodes,
            root: 0,
            stats: ScanStats::default(),
        };
        tree.recompute();
        tree
    }

    #[test]
    fn large_panel_shows_deeper_rings() {
        let tree = fixture_tree();
        let area = dump_area();
        let rings = rings_for(area);
        assert!(
            (8..=10).contains(&rings),
            "56×24 panel should hold 8–10 rings, got {rings}"
        );
        let slices = build_slices(&tree, 0, false, None, rings);
        let max_ring = slices.iter().map(|s| s.ring).max().unwrap_or(0);
        assert!(
            max_ring >= 5,
            "nested fixture should paint at least 6 ring levels, max={max_ring}"
        );
        let deep = slices.iter().find(|s| !s.grouped && s.node == 37);
        assert!(
            deep.is_some(),
            "deep child `debug` should appear as its own wedge"
        );
    }

    #[test]
    fn braille_glyph_uses_unicode_dot_order() {
        let th = theme::current();
        assert_eq!(braille_char(0x01), '\u{2801}', "dot 1 is top-left");
        assert_eq!(braille_char(0x08), '\u{2808}', "dot 4 is top-right");
        assert_eq!(braille_char(0x40), '\u{2840}', "dot 7 is bottom-left");
        assert_eq!(braille_char(0x80), '\u{2880}', "dot 8 is bottom-right");
        assert_eq!(braille_char(0xFF), '\u{28FF}', "all eight dots");
        let dots = [
            [Some(th.palette[0]), None],
            [None, None],
            [None, None],
            [None, Some(th.palette[0])],
        ];
        let mut buf = Buffer::new(1, 1, th.bg);
        paint_braille(&mut buf, 0, 0, dots, th.bg);
        let ch = buf.get(0, 0).unwrap().ch;
        assert_eq!(ch, braille_char(0x01 | 0x80), "top-left + bottom-right");
        let raster = raster_cell(*buf.get(0, 0).unwrap());
        assert_eq!(raster[0][0], th.palette[0]);
        assert_eq!(raster[3][1], th.palette[0]);
        assert_eq!(raster[0][1], th.bg);
    }

    #[test]
    fn tiny_panel_stays_at_min_rings() {
        let tiny = Rect {
            x: 0,
            y: 0,
            width: 16,
            height: 8,
        };
        assert_eq!(rings_for(tiny), SUNBURST_RINGS_MIN);
    }

    #[test]
    fn small_real_children_are_own_slices() {
        let tree = fixture_tree();
        let slices = build_slices(&tree, 0, false, None, 8);
        let names: Vec<&str> = slices
            .iter()
            .filter(|s| !s.grouped)
            .map(|s| tree.get(s.node).name.as_str())
            .collect();
        assert!(
            names.contains(&"crash"),
            "crash dump should stay its own wedge: {names:?}"
        );
        assert!(
            names.contains(&"etc"),
            "etc should stay its own wedge: {names:?}"
        );
        assert!(
            names.contains(&"font"),
            "mid-size cache child should not vanish: {names:?}"
        );
        assert!(
            names.contains(&"misc"),
            "200-byte share child should appear: {names:?}"
        );
    }

    #[test]
    fn dust_joins_smaller_objects() {
        let th = theme::current();
        let tree = grouping_tree();
        let slices = build_slices(&tree, 0, false, None, 4);
        let own: Vec<&str> = slices
            .iter()
            .filter(|s| !s.grouped)
            .map(|s| tree.get(s.node).name.as_str())
            .collect();
        assert!(own.contains(&"big"));
        assert!(own.contains(&"mid"));
        assert!(
            !own.iter().any(|n| n.starts_with("speck")),
            "specks should not be their own wedges: {own:?}"
        );
        let grouped: Vec<&Slice> = slices.iter().filter(|s| s.grouped).collect();
        assert_eq!(grouped.len(), 1, "one leftover wedge: {grouped:?}");
        assert_eq!(grouped[0].node, 0, "grouped remainder points at the parent");
        assert_eq!(
            grouped[0].color,
            dim_color(th.smaller, 0, 4),
            "leftover keeps the muted smaller-objects color"
        );
    }

    #[test]
    fn paint_and_hit_use_the_same_polar_map() {
        let th = theme::current();
        let tree = fixture_tree();
        let area = dump_area();
        let rings = rings_for(area);
        let slices = build_slices(&tree, 0, false, Some(11), rings);
        let mut buf = Buffer::new(area.width, area.height, th.bg);
        render(&mut buf, area, &slices);
        let geo = geometry(area).unwrap();

        let mut painted = 0usize;
        let mut hits = 0usize;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let cell = buf.get(x, y).unwrap();
                if !is_braille(cell.ch) {
                    continue;
                }
                painted += 1;
                let Some(under) = majority_slice(&slices, &geo, x - area.x, y - area.y, false)
                else {
                    continue;
                };
                let hit =
                    hit_slice(&slices, area, x, y).expect("painted non-grouped cell must hit");
                assert_eq!(hit.node, under.node, "hit/paint mismatch at ({x},{y})");
                hits += 1;
            }
        }
        assert!(
            painted > 80,
            "disk should fill the panel, painted={painted}"
        );
        assert!(hits > 40, "enough lockstep samples, hits={hits}");
    }

    #[test]
    fn selected_slice_and_descendants_brighten() {
        let tree = fixture_tree();
        let plain = build_slices(&tree, 0, false, None, 8);
        let lit = build_slices(&tree, 0, false, Some(11), 8);
        let var_plain = plain.iter().find(|s| s.node == 11 && !s.grouped).unwrap();
        let var_lit = lit.iter().find(|s| s.node == 11 && !s.grouped).unwrap();
        assert_ne!(
            var_plain.color, var_lit.color,
            "selected var should brighten"
        );
        let log_plain = plain.iter().find(|s| s.node == 12 && !s.grouped).unwrap();
        let log_lit = lit.iter().find(|s| s.node == 12 && !s.grouped).unwrap();
        assert_ne!(
            log_plain.color, log_lit.color,
            "descendant log should brighten with var"
        );
        let usr_plain = plain.iter().find(|s| s.node == 1 && !s.grouped).unwrap();
        let usr_lit = lit.iter().find(|s| s.node == 1 && !s.grouped).unwrap();
        assert_eq!(
            usr_plain.color, usr_lit.color,
            "unrelated usr should stay the same"
        );
    }

    #[test]
    fn category_colors_stay_on_waste_slices() {
        let tree = fixture_tree();
        let slices = build_slices(&tree, 0, false, None, 8);
        let log = slices.iter().find(|s| s.node == 12 && !s.grouped).unwrap();
        let cache = slices.iter().find(|s| s.node == 15 && !s.grouped).unwrap();
        let crash = slices.iter().find(|s| s.node == 27 && !s.grouped).unwrap();
        assert_eq!(
            log.color,
            dim_color(category_color(Category::Log), log.ring, 8)
        );
        assert_eq!(
            cache.color,
            dim_color(category_color(Category::Cache), cache.ring, 8)
        );
        assert_eq!(
            crash.color,
            dim_color(category_color(Category::Crash), crash.ring, 8)
        );
    }

    #[test]
    fn disk_fills_the_panel() {
        let (buf, _, area) = render_fixture(None);
        let painted = buf.text().chars().filter(|c| is_braille(*c)).count();
        let cells = area.width as usize * area.height as usize;
        assert!(
            painted * 100 / cells > 50,
            "disk should pack the panel, painted {painted}/{cells}"
        );
    }

    #[test]
    fn dump_fixture_ppm() {
        let (buf, slices, _) = render_fixture(Some(11));
        let painted = buf.text().chars().filter(|c| is_braille(*c)).count();
        eprintln!("painted cells {painted}");
        let dir = std::env::temp_dir();
        let path = dir.join("rings-sunburst-current.ppm");
        write_ppm(&buf, &path, 4);
        let text = dir.join("rings-sunburst-current.txt");
        std::fs::write(&text, buf.text()).unwrap();
        eprintln!(
            "dumped {} slices (max ring {}, rings_for={}) → {} and {}",
            slices.len(),
            slices.iter().map(|s| s.ring).max().unwrap_or(0),
            rings_for(dump_area()),
            path.display(),
            text.display()
        );
    }
}
