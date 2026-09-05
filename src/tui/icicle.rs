//! Icicle: the same layout as the sunburst, in Cartesian coordinates.
//!
//! Every [`Slice`] already carries `start` and `end` as fractions of the
//! whole span, plus `ring` as depth. That is an icicle specification exactly
//! as it stands — only the projection differs. The sunburst sends
//! `(ring, angle)` to a polar disc; this sends it to `(row, column)`.
//! `build_slices`, the grouping thresholds, the depth dimming and the colour
//! rules are shared verbatim, so the two views cannot drift apart.
//!
//! What the terminal gets back is text. A wedge cannot hold its own name, so
//! the sunburst spends the one thing a terminal is unambiguously good at on
//! nothing; four rows here identify more of the tree than the whole disc
//! does, in a quarter of the space.

use crate::constants::{ICICLE_LABEL_MIN, ICICLE_ROWS_MAX, ICICLE_ROWS_MIN};
use crate::scan::Tree;
use crate::size::human_bytes;
use crate::term::{Buffer, Rect};
use crate::tui::app::node_label;
use crate::tui::draw::truncate;
use crate::tui::sunburst::Slice;
use crate::tui::theme;

/// Leading hairline drawn in the cell that holds a slice's left edge, so
/// neighbours of a similar hue still read apart.
const EDGE: char = '\u{258f}';

/// Depth this panel can show, or 0 when the body is too small to spend rows
/// on a map at all — the caller then draws a list-only browse view.
pub fn rows_for(area: Rect) -> usize {
    if area.height < 8 || area.width < 12 {
        return 0;
    }
    // Reserve a base bar, a rule, and two rows of usable list.
    let budget = area.height.saturating_sub(4) as usize;
    (budget / 2)
        .clamp(ICICLE_ROWS_MIN, ICICLE_ROWS_MAX)
        .min(budget)
}

/// Cell to `(depth, fraction)`. The entire projection is these two lines,
/// which is what makes this view cheap: it is `sunburst::polar` replaced.
fn cell_to_ring_angle(area: Rect, x: u16, y: u16) -> Option<(usize, f64)> {
    if area.width == 0 || !area.contains(x, y) {
        return None;
    }
    let ring = (y - area.y) as usize;
    let angle = (f64::from(x - area.x) + 0.5) / f64::from(area.width);
    Some((ring, angle))
}

/// Deepest slice covering `(ring, angle)`.
///
/// Unlike the sunburst's lookup this never lets an inner slice fill outward.
/// The sunburst does that so empty outer rings do not hollow the disc; an
/// icicle wants the opposite, because blank space under a bar is how a flame
/// graph says "leaf", and repeating the parent there would make a file look
/// like it had children.
fn at(slices: &[Slice], ring: usize, angle: f64, allow_grouped: bool) -> Option<&Slice> {
    slices.iter().rev().find(|s| {
        (allow_grouped || !s.grouped) && s.ring == ring && angle >= s.start && angle < s.end
    })
}

/// Map a cell to a slice for mouse hits. Grouped leftovers stay unclickable,
/// matching the sunburst's contract exactly.
pub fn hit_slice<'a>(slices: &'a [Slice], area: Rect, x: u16, y: u16) -> Option<&'a Slice> {
    let (ring, angle) = cell_to_ring_angle(area, x, y)?;
    at(slices, ring, angle, false)
}

/// Paint the bars, then write names into the ones wide enough to hold one.
pub fn render(
    buf: &mut Buffer,
    area: Rect,
    slices: &[Slice],
    tree: &Tree,
    apparent: bool,
    selected: Option<usize>,
) {
    fill(buf, area, slices);
    label(buf, area, slices, tree, apparent, selected);
}

fn fill(buf: &mut Buffer, area: Rect, slices: &[Slice]) {
    if area.width == 0 {
        return;
    }
    let w = f64::from(area.width);
    for row in 0..area.height {
        let ring = row as usize;
        let y = area.y + row;
        for col in 0..area.width {
            let angle = (f64::from(col) + 0.5) / w;
            // Sampling per cell rather than per slice keeps paint and hit
            // testing on one code path, the way the sunburst does.
            let Some(slice) = at(slices, ring, angle, true) else {
                continue;
            };
            let holds_edge = slice.start >= f64::from(col) / w;
            let (ch, fg) = if holds_edge {
                (EDGE, theme::separator_color(slice.color))
            } else {
                (' ', slice.color)
            };
            buf.set(area.x + col, y, ch, fg, slice.color);
        }
    }
}

fn label(
    buf: &mut Buffer,
    area: Rect,
    slices: &[Slice],
    tree: &Tree,
    apparent: bool,
    selected: Option<usize>,
) {
    if area.width == 0 {
        return;
    }
    let w = f64::from(area.width);
    for slice in slices {
        if slice.grouped || slice.ring >= area.height as usize {
            continue;
        }
        let x0 = (slice.start * w).round() as u16;
        let x1 = (slice.end * w).round() as u16;
        let span = x1.saturating_sub(x0);
        if span < ICICLE_LABEL_MIN {
            continue;
        }
        // One cell in, past the leading hairline, and one spare at the end.
        let room = span.saturating_sub(2) as usize;
        if room == 0 {
            continue;
        }

        let node = tree.get(slice.node);
        let name = node_label(node);
        let size = human_bytes(node.display_size(apparent));
        let full = format!("{name}  {size}");
        let text = if full.chars().count() <= room {
            full
        } else {
            truncate(&name, room).into_owned()
        };

        buf.print_styled(
            area.x + x0 + 1,
            area.y + slice.ring as u16,
            &text,
            theme::contrast_on(slice.color),
            slice.color,
            selected == Some(slice.node),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scan, WalkOptions};
    use crate::tui::sunburst::build_slices;
    use std::fs;

    fn area(w: u16, h: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    }

    /// A tree with one obviously dominant branch, so spans are predictable.
    fn fixture() -> (tempfile::TempDir, Tree) {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("big").join("inner")).unwrap();
        fs::create_dir(tmp.path().join("small")).unwrap();
        fs::write(
            tmp.path().join("big").join("inner").join("blob"),
            vec![b'x'; 200_000],
        )
        .unwrap();
        fs::write(tmp.path().join("small").join("note"), vec![b'y'; 20_000]).unwrap();
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();
        (tmp, tree)
    }

    #[test]
    fn rows_scale_with_the_panel_and_vanish_when_tiny() {
        assert_eq!(rows_for(area(80, 4)), 0, "no room for a map");
        assert_eq!(rows_for(area(8, 40)), 0, "too narrow to label anything");
        assert_eq!(rows_for(area(80, 8)), ICICLE_ROWS_MIN);
        assert_eq!(rows_for(area(80, 40)), ICICLE_ROWS_MAX, "capped");
        let mid = rows_for(area(80, 16));
        assert!(
            (ICICLE_ROWS_MIN..=ICICLE_ROWS_MAX).contains(&mid),
            "got {mid}"
        );
        // Whatever it returns must leave room for the base bar, the rule,
        // and at least one row of list beneath it.
        for h in 8..40u16 {
            let rows = rows_for(area(80, h));
            assert!(rows + 3 <= h as usize, "{rows} rows do not fit height {h}");
        }
    }

    #[test]
    fn paint_and_hit_agree_on_every_painted_cell() {
        // The same lockstep guarantee the sunburst has, and the reason both
        // views sample through one lookup instead of drawing per slice.
        let th = theme::current();
        let (_tmp, tree) = fixture();
        let rect = area(60, 5);
        let slices = build_slices(&tree, tree.root, false, None, rect.height as usize);
        let mut buf = Buffer::new(rect.width, rect.height, th.bg);
        fill(&mut buf, rect, &slices);

        let mut hits = 0usize;
        for y in rect.y..rect.bottom() {
            for x in rect.x..rect.right() {
                let cell = buf.get(x, y).unwrap();
                let painted = cell.bg != th.bg;
                match hit_slice(&slices, rect, x, y) {
                    Some(_) => {
                        assert!(painted, "hit an unpainted cell at ({x},{y})");
                        hits += 1;
                    }
                    None => {
                        // Only a grouped remainder may be painted but unhittable.
                        if painted {
                            let (ring, angle) = cell_to_ring_angle(rect, x, y).unwrap();
                            assert!(
                                at(&slices, ring, angle, true).is_some_and(|s| s.grouped),
                                "painted but unreachable at ({x},{y})"
                            );
                        }
                    }
                }
            }
        }
        assert!(hits > 30, "the panel should be mostly covered, hits={hits}");
    }

    #[test]
    fn a_leaf_leaves_the_rows_below_it_blank() {
        // The difference from the sunburst's fill-outward rule: nothing may
        // appear under a file, or it would read as having children.
        let th = theme::current();
        let (_tmp, tree) = fixture();
        let rect = area(60, 6);
        let slices = build_slices(&tree, tree.root, false, None, rect.height as usize);
        let deepest = slices.iter().map(|s| s.ring).max().unwrap_or(0);
        let mut buf = Buffer::new(rect.width, rect.height, th.bg);
        fill(&mut buf, rect, &slices);

        for y in (deepest as u16 + 1)..rect.height {
            for x in 0..rect.width {
                assert_eq!(
                    buf.get(x, y).unwrap().bg,
                    th.bg,
                    "row {y} is past the deepest slice and must stay empty"
                );
            }
        }
    }

    #[test]
    fn wide_bars_carry_their_name_and_narrow_ones_do_not() {
        let th = theme::current();
        let (_tmp, tree) = fixture();
        let rect = area(70, 4);
        let slices = build_slices(&tree, tree.root, false, None, rect.height as usize);
        let mut buf = Buffer::new(rect.width, rect.height, th.bg);
        render(&mut buf, rect, &slices, &tree, false, None);
        let screen = buf.text();

        assert!(
            screen.contains("big"),
            "the dominant branch is named:\n{screen}"
        );
        assert!(
            screen.contains("inner"),
            "and so is its child a row down:\n{screen}"
        );
        // Names are written *into* the map, which is the whole point.
        assert!(
            screen.lines().next().unwrap_or_default().contains("big"),
            "row 0 holds the top-level names:\n{screen}"
        );
    }

    #[test]
    fn labels_stay_inside_their_own_span() {
        let th = theme::current();
        let (_tmp, tree) = fixture();
        let rect = area(70, 4);
        let slices = build_slices(&tree, tree.root, false, None, rect.height as usize);
        let mut buf = Buffer::new(rect.width, rect.height, th.bg);
        render(&mut buf, rect, &slices, &tree, false, None);

        // Every cell carrying a glyph must belong to the slice whose label
        // put it there; a label that overran would land on a neighbour's fill.
        for slice in slices.iter().filter(|s| !s.grouped) {
            let y = rect.y + slice.ring as u16;
            if y >= rect.bottom() {
                continue;
            }
            let x0 = (slice.start * f64::from(rect.width)).round() as u16;
            let x1 = (slice.end * f64::from(rect.width)).round() as u16;
            for x in x0..x1.min(rect.width) {
                let cell = buf.get(x, y).unwrap();
                assert_ne!(cell.bg, th.bg, "gap inside a slice at ({x},{y})");
            }
        }
    }

    #[test]
    fn an_empty_panel_paints_nothing_and_hits_nothing() {
        let th = theme::current();
        let mut buf = Buffer::new(10, 3, th.bg);
        let rect = area(0, 3);
        fill(&mut buf, rect, &[]);
        assert!(hit_slice(&[], rect, 0, 0).is_none());
        assert_eq!(buf.get(0, 0).unwrap().bg, th.bg);
    }
}
