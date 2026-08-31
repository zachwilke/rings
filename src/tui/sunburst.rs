//! Polar sunburst drawn with half-block cells into the term buffer.

use std::f64::consts::TAU;

use crate::classify::Category;
use crate::constants::{
    MIN_SLICE_ANGLE, MIN_SLICE_FRACTION, SUNBURST_HOLE_LABEL_PX, SUNBURST_HOLE_MAX,
    SUNBURST_HOLE_MIN, SUNBURST_MARGIN, SUNBURST_RINGS_MAX, SUNBURST_RINGS_MIN, SUNBURST_RING_PX,
};
use crate::scan::Tree;
use crate::term::{Buffer, Cell, Rect, Rgb};
use crate::tui::theme::{
    brighten, category_color, dim_color, separator_color, BG, PALETTE, SMALLER,
};

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
            color: dim_color(SMALLER, ring, rings),
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
    let cat = tree.get(id).category;
    let base = if cat != Category::Normal {
        category_color(cat)
    } else {
        PALETTE[color_i % PALETTE.len()]
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

/// Disk geometry inside a cell rect. Half-block pixels are roughly square
/// (a cell is ~1:2), so a true circle uses one radius in (cols, 2×rows)
/// pixel space. Hole and ring count are derived from that radius so paint
/// and hit testing stay on the same polar map.
struct Geometry {
    cx: f64,
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
    let w = area.width as f64;
    let h_px = (area.height as f64) * 2.0;
    let radius = ((w.min(h_px)) / 2.0 - SUNBURST_MARGIN).max(2.0);
    let hole = hole_for(radius);
    let rings = rings_from_radius(radius, hole);
    Some(Geometry {
        cx: area.x as f64 + w / 2.0,
        cy_px: h_px / 2.0,
        radius,
        hole,
        rings,
    })
}

fn cell_norm(geo: &Geometry, x: f64, y_px: f64) -> (f64, f64) {
    let dx = (x - geo.cx) / geo.radius;
    let dy = (y_px - geo.cy_px) / geo.radius;
    (dx, dy)
}

/// Map a terminal cell to a slice for mouse hits.
pub fn hit_slice<'a>(slices: &'a [Slice], area: Rect, x: u16, y: u16) -> Option<&'a Slice> {
    if !area.contains(x, y) {
        return None;
    }
    let geo = geometry(area)?;
    let (dx, dy) = cell_norm(&geo, x as f64 + 0.5, ((y - area.y) as f64) * 2.0 + 1.0);
    let (ring, angle) = polar(dx, dy, geo.hole, geo.rings)?;
    slice_at(slices, ring, angle, false)
}

/// Paint with half blocks: each cell is two vertical pixels.
pub fn render(buf: &mut Buffer, area: Rect, slices: &[Slice]) {
    let Some(geo) = geometry(area) else {
        return;
    };
    for ty in 0..area.height {
        for tx in 0..area.width {
            let x = area.x + tx;
            let y = area.y + ty;
            let (dx, _) = cell_norm(&geo, x as f64 + 0.5, 0.0);
            let upper = sample(
                slices,
                &geo,
                dx,
                ((ty * 2) as f64 + 0.5 - geo.cy_px) / geo.radius,
            );
            let lower = sample(
                slices,
                &geo,
                dx,
                ((ty * 2 + 1) as f64 + 0.5 - geo.cy_px) / geo.radius,
            );
            paint_half(buf, x, y, upper, lower);
        }
    }
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

fn paint_half(buf: &mut Buffer, x: u16, y: u16, upper: Option<Rgb>, lower: Option<Rgb>) {
    let cell = match (upper, lower) {
        (Some(u), Some(l)) if u == l => Cell {
            ch: '█',
            fg: u,
            bg: BG,
            bold: false,
        },
        (Some(u), Some(l)) => Cell {
            ch: '▀',
            fg: u,
            bg: l,
            bold: false,
        },
        (Some(u), None) => Cell {
            ch: '▀',
            fg: u,
            bg: BG,
            bold: false,
        },
        (None, Some(l)) => Cell {
            ch: '▄',
            fg: l,
            bg: BG,
            bold: false,
        },
        (None, None) => return,
    };
    buf.set_cell(x, y, cell);
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
        let tree = fixture_tree();
        let area = dump_area();
        let rings = rings_for(area);
        let slices = build_slices(&tree, 0, false, selected, rings);
        let mut buf = Buffer::new(area.width, area.height, BG);
        render(&mut buf, area, &slices);
        (buf, slices, area)
    }

    /// Half-block buffer → binary PPM (each cell is 1×2 pixels, scaled up).
    fn write_ppm(buf: &Buffer, path: &Path, scale: u32) {
        let w = buf.width as u32 * scale;
        let h = buf.height as u32 * 2 * scale;
        let mut px = vec![0u8; (w * h * 3) as usize];
        for y in 0..buf.height {
            for x in 0..buf.width {
                let cell = buf.get(x, y).cloned().unwrap_or(Cell::blank(BG));
                let (top, bot) = match cell.ch {
                    '█' => (cell.fg, cell.fg),
                    '▀' => (cell.fg, cell.bg),
                    '▄' => (cell.bg, cell.fg),
                    _ => (cell.bg, cell.bg),
                };
                for sy in 0..scale {
                    for sx in 0..scale {
                        put(
                            &mut px,
                            w,
                            x as u32 * scale + sx,
                            y as u32 * 2 * scale + sy,
                            top,
                        );
                        put(
                            &mut px,
                            w,
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
            (6..=8).contains(&rings),
            "56×24 panel should hold 6–8 rings, got {rings}"
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
            dim_color(SMALLER, 0, 4),
            "leftover keeps the muted smaller-objects color"
        );
    }

    #[test]
    fn paint_and_hit_use_the_same_polar_map() {
        let tree = fixture_tree();
        let area = dump_area();
        let rings = rings_for(area);
        let slices = build_slices(&tree, 0, false, Some(11), rings);
        let mut buf = Buffer::new(area.width, area.height, BG);
        render(&mut buf, area, &slices);
        let geo = geometry(area).unwrap();

        let mut painted = 0usize;
        let mut hits = 0usize;
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                let cell = buf.get(x, y).unwrap();
                if cell.ch == ' ' {
                    continue;
                }
                painted += 1;
                let (dx, dy) = cell_norm(&geo, x as f64 + 0.5, ((y - area.y) as f64) * 2.0 + 1.0);
                let Some((ring, angle)) = polar(dx, dy, geo.hole, geo.rings) else {
                    continue;
                };
                let under = slices
                    .iter()
                    .rev()
                    .find(|s| s.ring == ring && angle >= s.start && angle < s.end);
                let Some(under) = under else {
                    continue;
                };
                if under.grouped {
                    continue;
                }
                let hit =
                    hit_slice(&slices, area, x, y).expect("painted non-grouped cell must hit");
                assert_eq!(
                    hit.node, under.node,
                    "hit/paint mismatch at ({x},{y}) ring {ring} angle {angle:.3}"
                );
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
        let painted = buf.text().chars().filter(|c| "█▀▄".contains(*c)).count();
        let cells = area.width as usize * area.height as usize;
        assert!(
            painted * 100 / cells > 50,
            "disk should pack the panel, painted {painted}/{cells}"
        );
    }

    #[test]
    fn dump_fixture_ppm() {
        let (buf, slices, _) = render_fixture(Some(11));
        let painted = buf.text().chars().filter(|c| "█▀▄".contains(*c)).count();
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
