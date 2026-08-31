//! Polar sunburst drawn with half-block cells into the term buffer.

use std::f64::consts::TAU;

use crate::classify::Category;
use crate::constants::{MIN_SLICE_FRACTION, SUNBURST_HOLE, SUNBURST_RINGS};
use crate::scan::Tree;
use crate::term::{Buffer, Cell, Rect, Rgb};
use crate::tui::theme::{brighten, category_color, dim_color, BG, PALETTE, SMALLER};

#[derive(Clone, Debug)]
pub struct Slice {
    pub start: f64,
    pub end: f64,
    pub ring: usize,
    pub node: usize,
    pub color: Rgb,
    pub grouped: bool,
}

pub fn build_slices(
    tree: &Tree,
    current: usize,
    apparent: bool,
    selected: Option<usize>,
) -> Vec<Slice> {
    let mut slices = Vec::new();
    layout(tree, current, 0.0, 1.0, 0, apparent, 0, selected, &mut slices);
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
    out: &mut Vec<Slice>,
) {
    if ring >= SUNBURST_RINGS {
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
        if frac < MIN_SLICE_FRACTION {
            small_span += frac * span;
            continue;
        }
        let slice_end = (cursor + frac * span).min(end);
        let color = slice_color(tree, cid, color_i, ring, selected);
        out.push(Slice {
            start: cursor,
            end: slice_end,
            ring,
            node: cid,
            color,
            grouped: false,
        });
        layout(
            tree, cid, cursor, slice_end, ring + 1, apparent, color_i, selected, out,
        );
        cursor = slice_end;
        color_i += 1;
    }
    if small_span > 0.0 && cursor < end {
        out.push(Slice {
            start: cursor,
            end: (cursor + small_span).min(end),
            ring,
            node: id,
            color: SMALLER,
            grouped: true,
        });
    }
}

fn slice_color(
    tree: &Tree,
    id: usize,
    color_i: usize,
    ring: usize,
    selected: Option<usize>,
) -> Rgb {
    let cat = tree.get(id).category;
    let base = if cat != Category::Normal {
        category_color(cat)
    } else {
        PALETTE[color_i % PALETTE.len()]
    };
    let mut c = dim_color(base, ring);
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

fn polar(dx: f64, dy: f64) -> Option<(usize, f64)> {
    let r = (dx * dx + dy * dy).sqrt();
    if r < SUNBURST_HOLE || r > 1.0 {
        return None;
    }
    let mut t = dy.atan2(dx) + std::f64::consts::FRAC_PI_2;
    if t < 0.0 {
        t += TAU;
    }
    let angle = t / TAU;
    let ring_span = 1.0 - SUNBURST_HOLE;
    let ring = (((r - SUNBURST_HOLE) / ring_span) * SUNBURST_RINGS as f64).floor() as usize;
    Some((ring.min(SUNBURST_RINGS - 1), angle))
}

/// Disk geometry inside a cell rect. Half-block pixels are roughly square
/// (a cell is ~1:2), so a true circle uses one radius in (cols, 2×rows)
/// pixel space, centered, with a margin so the disk breathes.
struct Geometry {
    cx: f64,
    cy_px: f64,
    radius: f64,
}

fn geometry(area: Rect) -> Option<Geometry> {
    if area.width < 6 || area.height < 4 {
        return None;
    }
    let w = area.width as f64;
    let h_px = (area.height as f64) * 2.0;
    let radius = ((w.min(h_px)) / 2.0 - 2.0).max(2.0);
    Some(Geometry {
        cx: area.x as f64 + w / 2.0,
        cy_px: h_px / 2.0,
        radius,
    })
}

/// Map a terminal cell to a slice for mouse hits.
pub fn hit_slice<'a>(slices: &'a [Slice], area: Rect, x: u16, y: u16) -> Option<&'a Slice> {
    if !area.contains(x, y) {
        return None;
    }
    let geo = geometry(area)?;
    let dx = (x as f64 + 0.5 - geo.cx) / geo.radius;
    let dy = (((y - area.y) as f64) * 2.0 + 1.0 - geo.cy_px) / geo.radius;
    let (ring, angle) = polar(dx, dy)?;
    slices
        .iter()
        .rev()
        .find(|s| !s.grouped && s.ring == ring && angle >= s.start && angle < s.end)
        .or_else(|| {
            slices
                .iter()
                .rev()
                .find(|s| !s.grouped && angle >= s.start && angle < s.end && s.ring <= ring)
        })
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
            let dx = (x as f64 + 0.5 - geo.cx) / geo.radius;
            let upper = sample(slices, dx, ((ty * 2) as f64 + 0.5 - geo.cy_px) / geo.radius);
            let lower = sample(
                slices,
                dx,
                ((ty * 2 + 1) as f64 + 0.5 - geo.cy_px) / geo.radius,
            );
            paint_half(buf, x, y, upper, lower);
        }
    }
}

fn sample(slices: &[Slice], nx: f64, ny: f64) -> Option<Rgb> {
    let (ring, angle) = polar(nx, ny)?;
    slices
        .iter()
        .rev()
        .find(|s| s.ring == ring && angle >= s.start && angle < s.end)
        .map(|s| s.color)
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
