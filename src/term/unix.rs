//! Unix raw mode via termios + ioctl. Shared by Linux and macOS.

use std::io::{self, Write};
use std::mem::MaybeUninit;
use std::sync::atomic::Ordering;

use super::{install_panic_restore, ENTER_SEQ, LEAVE_SEQ, RAW_ACTIVE};

static mut ORIG_TERMIOS: MaybeUninit<libc::termios> = MaybeUninit::uninit();

/// RAII guard: enters raw mode + alt screen, restores on drop.
pub struct Term;

impl Term {
    pub fn enter() -> io::Result<Term> {
        unsafe {
            let mut orig = MaybeUninit::<libc::termios>::uninit();
            if libc::tcgetattr(libc::STDIN_FILENO, orig.as_mut_ptr()) != 0 {
                return Err(io::Error::last_os_error());
            }
            let orig = orig.assume_init();
            (*(&raw mut ORIG_TERMIOS)).write(orig);
            let mut raw = orig;
            libc::cfmakeraw(&mut raw);
            if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &raw) != 0 {
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
        let mut ws: libc::winsize = std::mem::zeroed();
        if libc::ioctl(libc::STDOUT_FILENO, libc::TIOCGWINSZ, &mut ws) == 0
            && ws.ws_col > 0
            && ws.ws_row > 0
        {
            return (ws.ws_col, ws.ws_row);
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
        let orig = &raw const ORIG_TERMIOS;
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, (*orig).as_ptr());
    }
}
