//! Tiny Linux helpers (no extra runtime).

pub fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}

pub fn stdout_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

pub fn stderr_is_tty() -> bool {
    unsafe { libc::isatty(libc::STDERR_FILENO) == 1 }
}
