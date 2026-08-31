//! Windows console: VT output (truecolor sunburst) + ReadConsoleInput for keys
//! and mouse. No extra crates.

use std::io::{self, Write};
use std::sync::atomic::Ordering;

use crate::sys::win32;
use crate::term::{Event, Key};

use super::{install_panic_restore, RAW_ACTIVE};

const ENTER_SEQ: &str = "\x1b[?1049h\x1b[?25l";
const LEAVE_SEQ: &str = "\x1b[?25h\x1b[?1049l\x1b[0m";

struct Saved {
    in_mode: u32,
    out_mode: u32,
}

static mut ORIG: Option<Saved> = None;

/// RAII guard: VT alt screen + raw-ish console input, restores on drop.
pub struct Term;

impl Term {
    pub fn enter() -> io::Result<Term> {
        unsafe {
            let hin = win32::GetStdHandle(win32::STD_INPUT_HANDLE);
            let hout = win32::GetStdHandle(win32::STD_OUTPUT_HANDLE);
            if hin.is_null()
                || hin == win32::INVALID_HANDLE_VALUE
                || hout.is_null()
                || hout == win32::INVALID_HANDLE_VALUE
            {
                return Err(io::Error::new(
                    io::ErrorKind::Other,
                    "no console (use rings.exe --plain)",
                ));
            }

            let mut in_mode = 0u32;
            let mut out_mode = 0u32;
            if win32::GetConsoleMode(hin, &mut in_mode) == 0
                || win32::GetConsoleMode(hout, &mut out_mode) == 0
            {
                return Err(io::Error::last_os_error());
            }
            ORIG = Some(Saved { in_mode, out_mode });

            let _ = win32::SetConsoleOutputCP(win32::CP_UTF8);
            let _ = win32::SetConsoleCP(win32::CP_UTF8);

            let new_out = out_mode
                | win32::ENABLE_PROCESSED_OUTPUT
                | win32::ENABLE_WRAP_AT_EOL_OUTPUT
                | win32::ENABLE_VIRTUAL_TERMINAL_PROCESSING
                | win32::DISABLE_NEWLINE_AUTO_RETURN;
            if win32::SetConsoleMode(hout, new_out) == 0 {
                return Err(io::Error::last_os_error());
            }

            // Extended flags without Quick Edit so mouse clicks reach us.
            let new_in = win32::ENABLE_EXTENDED_FLAGS
                | win32::ENABLE_MOUSE_INPUT
                | win32::ENABLE_WINDOW_INPUT;
            if win32::SetConsoleMode(hin, new_in) == 0 {
                let _ = win32::SetConsoleMode(hout, out_mode);
                return Err(io::Error::last_os_error());
            }
        }
        RAW_ACTIVE.store(true, Ordering::SeqCst);
        install_panic_restore();
        let mut out = io::stdout().lock();
        out.write_all(ENTER_SEQ.as_bytes())?;
        out.flush()?;
        Ok(Term)
    }

    pub fn size(&self) -> (u16, u16) {
        terminal_size()
    }
}

impl Drop for Term {
    fn drop(&mut self) {
        restore();
    }
}

pub fn terminal_size() -> (u16, u16) {
    unsafe {
        let hout = win32::GetStdHandle(win32::STD_OUTPUT_HANDLE);
        let mut info = std::mem::zeroed::<win32::ConsoleScreenBufferInfo>();
        if win32::GetConsoleScreenBufferInfo(hout, &mut info) != 0 {
            let w = (info.window.right - info.window.left + 1) as u16;
            let h = (info.window.bottom - info.window.top + 1) as u16;
            if w > 0 && h > 0 {
                return (w, h);
            }
        }
    }
    (80, 24)
}

pub fn restore() {
    if !RAW_ACTIVE.swap(false, Ordering::SeqCst) {
        return;
    }
    let mut out = io::stdout().lock();
    let _ = out.write_all(LEAVE_SEQ.as_bytes());
    let _ = out.flush();
    unsafe {
        let saved = (*(&raw mut ORIG)).take();
        if let Some(saved) = saved {
            let hin = win32::GetStdHandle(win32::STD_INPUT_HANDLE);
            let hout = win32::GetStdHandle(win32::STD_OUTPUT_HANDLE);
            let _ = win32::SetConsoleMode(hin, saved.in_mode);
            let _ = win32::SetConsoleMode(hout, saved.out_mode);
        }
    }
}

pub fn poll_event(timeout_ms: i32) -> Vec<Event> {
    unsafe {
        let hin = win32::GetStdHandle(win32::STD_INPUT_HANDLE);
        let wait_ms = if timeout_ms < 0 {
            u32::MAX
        } else {
            timeout_ms as u32
        };
        if win32::WaitForSingleObject(hin, wait_ms) != win32::WAIT_OBJECT_0 {
            return Vec::new();
        }
        let mut recs = [std::mem::zeroed::<win32::InputRecord>(); 32];
        let mut n = 0u32;
        if win32::ReadConsoleInputW(hin, recs.as_mut_ptr(), 32, &mut n) == 0 {
            return Vec::new();
        }
        decode_records(&recs[..n as usize])
    }
}

fn decode_records(recs: &[win32::InputRecord]) -> Vec<Event> {
    let mut events = Vec::new();
    let (_, win_top) = window_origin();
    for rec in recs {
        match rec.event_type {
            t if t == win32::KEY_EVENT => {
                let (down, vk, uchar, ctrl) = win32::key_event(&rec.payload);
                if !down {
                    continue;
                }
                if (ctrl & (win32::LEFT_CTRL_PRESSED | win32::RIGHT_CTRL_PRESSED)) != 0
                    && vk == win32::VK_C
                {
                    events.push(Event::Key(Key::Char('q')));
                    continue;
                }
                if let Some(key) = map_vk(vk, uchar) {
                    events.push(Event::Key(key));
                }
            }
            t if t == win32::MOUSE_EVENT => {
                let (x, y, buttons, flags) = win32::mouse_event(&rec.payload);
                // DOUBLE_CLICK is the second press; the TUI times two
                // Click events, so deliver it. Ignore motion-only records.
                let press = (buttons & win32::FROM_LEFT_1ST_BUTTON_PRESSED) != 0
                    && (flags & win32::MOUSE_MOVED) == 0;
                if press {
                    let col = x.saturating_sub(0) as u16;
                    let row = (y as i32 - win_top as i32).max(0) as u16;
                    events.push(Event::Click { x: col, y: row });
                }
            }
            _ => {}
        }
    }
    events
}

fn window_origin() -> (i16, i16) {
    unsafe {
        let hout = win32::GetStdHandle(win32::STD_OUTPUT_HANDLE);
        let mut info = std::mem::zeroed::<win32::ConsoleScreenBufferInfo>();
        if win32::GetConsoleScreenBufferInfo(hout, &mut info) != 0 {
            return (info.window.left, info.window.top);
        }
    }
    (0, 0)
}

fn map_vk(vk: u16, uchar: u16) -> Option<Key> {
    match vk {
        win32::VK_UP => Some(Key::Up),
        win32::VK_DOWN => Some(Key::Down),
        win32::VK_LEFT => Some(Key::Left),
        win32::VK_RIGHT => Some(Key::Right),
        win32::VK_RETURN => Some(Key::Enter),
        win32::VK_BACK => Some(Key::Backspace),
        win32::VK_ESCAPE => Some(Key::Esc),
        win32::VK_PRIOR => Some(Key::PageUp),
        win32::VK_NEXT => Some(Key::PageDown),
        win32::VK_F1 => Some(Key::F1),
        _ => {
            if uchar >= 0x20 && uchar < 0x7f {
                Some(Key::Char(uchar as u8 as char))
            } else {
                None
            }
        }
    }
}
