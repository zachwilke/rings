//! Minimal terminal layer: raw mode, alternate screen, SGR mouse, and a
//! diffed cell buffer. Replaces ratatui + crossterm with std + libc (Unix)
//! or a handful of Win32 calls (Windows).

mod input;
mod screen;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod win;

pub use input::{decode, poll_event, Event, Key};
pub use screen::{flush_diff, Buffer, Cell, Rect};

mod color;
pub use color::{color_depth, detect_color_depth, set_color_depth, ColorDepth};

#[cfg(unix)]
pub use unix::{terminal_size, Term};
#[cfg(windows)]
pub use win::{terminal_size, Term};

use std::sync::atomic::{AtomicBool, Ordering};

/// 24-bit color. Terminals without truecolor still parse the sequence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

/// Alt screen, hidden cursor, button + any-motion mouse reporting in SGR form.
#[cfg(unix)]
const ENTER_SEQ: &str = "\x1b[?1049h\x1b[?25l\x1b[?1000h\x1b[?1003h\x1b[?1006h";
#[cfg(unix)]
const LEAVE_SEQ: &str = "\x1b[?1006l\x1b[?1003l\x1b[?1000l\x1b[?25h\x1b[?1049l\x1b[0m";

static RAW_ACTIVE: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn restore() {
    unix::restore();
}

#[cfg(windows)]
fn restore() {
    win::restore();
}

/// With panic=abort the hook still runs; leave the terminal usable.
fn install_panic_restore() {
    static INSTALLED: AtomicBool = AtomicBool::new(false);
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prev(info);
    }));
}
