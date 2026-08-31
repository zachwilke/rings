//! Tiny Linux helpers (no extra runtime).

pub fn running_as_root() -> bool {
    unsafe { libc::geteuid() == 0 }
}
