//! Keyboard + SGR mouse input without crossterm.

#[cfg(unix)]
use std::io::Read;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Backspace,
    Esc,
    PageUp,
    PageDown,
    F1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Event {
    Key(Key),
    /// Left button press at 0-based cell coordinates.
    Click {
        x: u16,
        y: u16,
    },
    /// Right button press: opens the context menu.
    RightClick {
        x: u16,
        y: u16,
    },
    /// Scroll wheel: `delta` is -1 for up, +1 for down.
    Wheel {
        x: u16,
        y: u16,
        delta: i8,
    },
    /// Pointer moved (any-motion tracking): drives hover highlights.
    Move {
        x: u16,
        y: u16,
    },
}

/// Wait up to `timeout_ms` for input. Returns all events decoded from the
/// bytes that arrived (escape sequences can batch).
#[cfg(unix)]
pub fn poll_event(timeout_ms: i32) -> Vec<Event> {
    let mut fds = libc::pollfd {
        fd: libc::STDIN_FILENO,
        events: libc::POLLIN,
        revents: 0,
    };
    let n = unsafe { libc::poll(&mut fds, 1, timeout_ms) };
    if n <= 0 || fds.revents & libc::POLLIN == 0 {
        return Vec::new();
    }
    let mut raw = [0u8; 512];
    let read = std::io::stdin().lock().read(&mut raw).unwrap_or(0);
    decode(&raw[..read])
}

#[cfg(windows)]
pub fn poll_event(timeout_ms: i32) -> Vec<Event> {
    super::win::poll_event(timeout_ms)
}

pub fn decode(bytes: &[u8]) -> Vec<Event> {
    let mut events = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == 0x1b {
            let (ev, used) = decode_escape(&bytes[i..]);
            if let Some(ev) = ev {
                events.push(ev);
            }
            i += used;
            continue;
        }
        i += 1;
        match b {
            b'\r' | b'\n' => events.push(Event::Key(Key::Enter)),
            0x7f | 0x08 => events.push(Event::Key(Key::Backspace)),
            0x03 => events.push(Event::Key(Key::Char('q'))), // Ctrl-C quits
            b if b >= 0x20 && b < 0x7f => {
                events.push(Event::Key(Key::Char(b as char)));
            }
            _ => {}
        }
    }
    events
}

/// Returns the decoded event (if any) and how many bytes were consumed.
fn decode_escape(bytes: &[u8]) -> (Option<Event>, usize) {
    if bytes.len() == 1 {
        return (Some(Event::Key(Key::Esc)), 1);
    }
    if bytes[1] == b'O' && bytes.len() >= 3 && bytes[2] == b'P' {
        // SS3 F1 (xterm / application mode)
        return (Some(Event::Key(Key::F1)), 3);
    }
    if bytes[1] != b'[' {
        // Alt+key or stray escape: treat as Esc, consume the pair.
        return (Some(Event::Key(Key::Esc)), 2);
    }
    if bytes.len() >= 3 {
        match bytes[2] {
            b'A' => return (Some(Event::Key(Key::Up)), 3),
            b'B' => return (Some(Event::Key(Key::Down)), 3),
            b'C' => return (Some(Event::Key(Key::Right)), 3),
            b'D' => return (Some(Event::Key(Key::Left)), 3),
            b'<' => return decode_sgr_mouse(bytes),
            b'5' | b'6' if bytes.len() >= 4 && bytes[3] == b'~' => {
                let key = if bytes[2] == b'5' {
                    Key::PageUp
                } else {
                    Key::PageDown
                };
                return (Some(Event::Key(key)), 4);
            }
            b'1' if bytes.len() >= 5 && bytes[3] == b'1' && bytes[4] == b'~' => {
                return (Some(Event::Key(Key::F1)), 5);
            }
            _ => {}
        }
    }
    // Unknown CSI: consume through its final byte (0x40..=0x7e).
    let mut used = 2;
    while used < bytes.len() {
        let b = bytes[used];
        used += 1;
        if (0x40..=0x7e).contains(&b) {
            break;
        }
    }
    (None, used)
}

/// `ESC [ < button ; col ; row (M|m)` — M is press, m is release.
fn decode_sgr_mouse(bytes: &[u8]) -> (Option<Event>, usize) {
    let mut fields = [0u32; 3];
    let mut field = 0;
    let mut i = 3;
    while i < bytes.len() {
        let b = bytes[i];
        i += 1;
        match b {
            b'0'..=b'9' if field < 3 => {
                fields[field] = fields[field] * 10 + (b - b'0') as u32;
            }
            b';' => field += 1,
            b'M' => {
                if field != 2 {
                    return (None, i);
                }
                // 1-based coords
                let x = fields[1].saturating_sub(1).min(u16::MAX as u32) as u16;
                let y = fields[2].saturating_sub(1).min(u16::MAX as u32) as u16;
                let code = fields[0];
                let ev = if code & 0b100_0000 != 0 {
                    // Wheel: 64 up, 65 down (modifier bits 4/8/16 ignored).
                    match code & 0b11 {
                        0 => Some(Event::Wheel { x, y, delta: -1 }),
                        1 => Some(Event::Wheel { x, y, delta: 1 }),
                        _ => None,
                    }
                } else if code & 0b10_0000 != 0 {
                    Some(Event::Move { x, y })
                } else {
                    match code & 0b11 {
                        0 => Some(Event::Click { x, y }),
                        2 => Some(Event::RightClick { x, y }),
                        _ => None,
                    }
                };
                return (ev, i);
            }
            b'm' => return (None, i),
            _ => return (None, i),
        }
    }
    (None, bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_plain_keys() {
        assert_eq!(decode(b"j"), vec![Event::Key(Key::Char('j'))]);
        assert_eq!(decode(b"\r"), vec![Event::Key(Key::Enter)]);
        assert_eq!(decode(&[0x7f]), vec![Event::Key(Key::Backspace)]);
        assert_eq!(decode(&[0x1b]), vec![Event::Key(Key::Esc)]);
    }

    #[test]
    fn decodes_arrows_and_paging() {
        assert_eq!(decode(b"\x1b[A"), vec![Event::Key(Key::Up)]);
        assert_eq!(decode(b"\x1b[B"), vec![Event::Key(Key::Down)]);
        assert_eq!(decode(b"\x1b[5~"), vec![Event::Key(Key::PageUp)]);
        assert_eq!(decode(b"\x1b[6~"), vec![Event::Key(Key::PageDown)]);
        assert_eq!(decode(b"\x1bOP"), vec![Event::Key(Key::F1)]);
        assert_eq!(decode(b"\x1b[11~"), vec![Event::Key(Key::F1)]);
    }

    #[test]
    fn decodes_sgr_left_click_press_only() {
        assert_eq!(decode(b"\x1b[<0;10;5M"), vec![Event::Click { x: 9, y: 4 }]);
        // release ignored
        assert_eq!(decode(b"\x1b[<0;10;5m"), vec![]);
    }

    #[test]
    fn decodes_wheel_and_motion() {
        assert_eq!(
            decode(b"\x1b[<64;10;5M"),
            vec![Event::Wheel {
                x: 9,
                y: 4,
                delta: -1
            }]
        );
        assert_eq!(
            decode(b"\x1b[<65;10;5M"),
            vec![Event::Wheel {
                x: 9,
                y: 4,
                delta: 1
            }]
        );
        assert_eq!(
            decode(b"\x1b[<35;10;5M"),
            vec![Event::Move { x: 9, y: 4 }],
            "motion with no button held"
        );
        assert_eq!(
            decode(b"\x1b[<32;10;5M"),
            vec![Event::Move { x: 9, y: 4 }],
            "a drag still moves the pointer"
        );
        assert_eq!(decode(b"\x1b[<1;10;5M"), vec![], "middle button ignored");
    }

    #[test]
    fn decodes_sgr_right_click_press_only() {
        assert_eq!(
            decode(b"\x1b[<2;10;5M"),
            vec![Event::RightClick { x: 9, y: 4 }]
        );
        assert_eq!(decode(b"\x1b[<2;10;5m"), vec![], "release ignored");
        assert_eq!(decode(b"\x1b[<1;10;5M"), vec![], "middle button ignored");
    }

    #[test]
    fn decodes_batched_sequences() {
        let ev = decode(b"j\x1b[A\x1b[<0;3;2Mq");
        assert_eq!(
            ev,
            vec![
                Event::Key(Key::Char('j')),
                Event::Key(Key::Up),
                Event::Click { x: 2, y: 1 },
                Event::Key(Key::Char('q')),
            ]
        );
    }
}
