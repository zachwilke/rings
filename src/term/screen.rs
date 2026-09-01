//! Cell buffer with diffed ANSI output.

use std::io::{self, Write};

use std::fmt::Write as _;

use super::{color, ColorDepth, Rgb};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl Rect {
    pub const ZERO: Rect = Rect {
        x: 0,
        y: 0,
        width: 0,
        height: 0,
    };

    pub fn contains(&self, x: u16, y: u16) -> bool {
        x >= self.x
            && x < self.x.saturating_add(self.width)
            && y >= self.y
            && y < self.y.saturating_add(self.height)
    }

    pub fn right(&self) -> u16 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(&self) -> u16 {
        self.y.saturating_add(self.height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
}

impl Cell {
    pub fn blank(bg: Rgb) -> Cell {
        Cell {
            ch: ' ',
            fg: bg,
            bg,
            bold: false,
        }
    }
}

pub struct Buffer {
    pub width: u16,
    pub height: u16,
    cells: Vec<Cell>,
}

impl Buffer {
    pub fn new(width: u16, height: u16, bg: Rgb) -> Buffer {
        Buffer {
            width,
            height,
            cells: vec![Cell::blank(bg); width as usize * height as usize],
        }
    }

    pub fn area(&self) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: self.width,
            height: self.height,
        }
    }

    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some(y as usize * self.width as usize + x as usize)
    }

    pub fn get(&self, x: u16, y: u16) -> Option<&Cell> {
        self.idx(x, y).map(|i| &self.cells[i])
    }

    pub fn set(&mut self, x: u16, y: u16, ch: char, fg: Rgb, bg: Rgb) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = Cell {
                ch,
                fg,
                bg,
                bold: false,
            };
        }
    }

    pub fn set_cell(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = cell;
        }
    }

    /// Write text starting at (x, y); clips at the buffer edge.
    /// Returns the x position after the last written char.
    pub fn print(&mut self, x: u16, y: u16, text: &str, fg: Rgb, bg: Rgb) -> u16 {
        self.print_styled(x, y, text, fg, bg, false)
    }

    pub fn print_styled(
        &mut self,
        x: u16,
        y: u16,
        text: &str,
        fg: Rgb,
        bg: Rgb,
        bold: bool,
    ) -> u16 {
        let mut cx = x;
        for ch in text.chars() {
            if cx >= self.width {
                break;
            }
            self.set_cell(cx, y, Cell { ch, fg, bg, bold });
            cx = cx.saturating_add(1);
        }
        cx
    }

    pub fn fill(&mut self, rect: Rect, bg: Rgb) {
        for y in rect.y..rect.bottom().min(self.height) {
            for x in rect.x..rect.right().min(self.width) {
                self.set(x, y, ' ', bg, bg);
            }
        }
    }

    /// Plain-text dump for tests.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for y in 0..self.height {
            for x in 0..self.width {
                out.push(self.get(x, y).map(|c| c.ch).unwrap_or(' '));
            }
            out.push('\n');
        }
        out
    }
}

/// Emit only cells that differ from `prev`. `prev = None` repaints everything.
pub fn flush_diff(buf: &Buffer, prev: Option<&Buffer>) -> io::Result<()> {
    let depth = color::color_depth();
    let base_bg = buf.get(0, 0).map(|c| c.bg).unwrap_or(Rgb(0, 0, 0));
    let mut out = String::with_capacity(4096);
    // Compare what the terminal would see, so shades that quantize to the
    // same palette slot do not each cost an escape sequence.
    let mut last_fg: Option<u32> = None;
    let mut last_bg: Option<u32> = None;
    let mut last_bold = false;
    let mut last_reverse = false;
    let mut cursor: Option<(u16, u16)> = None;

    let same_shape = prev.is_some_and(|p| p.width == buf.width && p.height == buf.height);

    for y in 0..buf.height {
        for x in 0..buf.width {
            let cell = buf.get(x, y).copied().unwrap_or(Cell::blank(Rgb(0, 0, 0)));
            if same_shape {
                if let Some(p) = prev.and_then(|p| p.get(x, y)) {
                    if *p == cell {
                        continue;
                    }
                }
            }
            let need_move = cursor != Some((x, y));
            if need_move {
                let _ = write!(out, "\x1b[{};{}H", y + 1, x + 1);
            }
            if last_bold != cell.bold {
                out.push_str(if cell.bold { "\x1b[1m" } else { "\x1b[22m" });
                last_bold = cell.bold;
            }
            if depth == ColorDepth::Mono {
                // No color: reverse video stands in for any non-base background.
                let reverse = cell.bg != base_bg;
                if last_reverse != reverse {
                    out.push_str(if reverse { "\x1b[7m" } else { "\x1b[27m" });
                    last_reverse = reverse;
                }
            } else {
                let fg = color::color_key(depth, cell.fg);
                if last_fg != Some(fg) {
                    color::push_sgr(&mut out, depth, cell.fg, true);
                    last_fg = Some(fg);
                }
                let bg = color::color_key(depth, cell.bg);
                if last_bg != Some(bg) {
                    color::push_sgr(&mut out, depth, cell.bg, false);
                    last_bg = Some(bg);
                }
            }
            out.push(cell.ch);
            cursor = Some((x.saturating_add(1), y));
        }
    }

    let mut stdout = io::stdout().lock();
    stdout.write_all(out.as_bytes())?;
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn print_clips_and_text_dumps() {
        let bg = Rgb(0, 0, 0);
        let mut b = Buffer::new(5, 2, bg);
        b.print(3, 0, "abc", Rgb(255, 255, 255), bg);
        b.print(0, 1, "xy", Rgb(255, 255, 255), bg);
        let t = b.text();
        assert_eq!(t, "   ab\nxy   \n");
    }

    #[test]
    fn rect_contains() {
        let r = Rect {
            x: 2,
            y: 3,
            width: 4,
            height: 2,
        };
        assert!(r.contains(2, 3));
        assert!(r.contains(5, 4));
        assert!(!r.contains(6, 4));
        assert!(!r.contains(2, 5));
    }
}
