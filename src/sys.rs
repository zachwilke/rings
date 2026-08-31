//! Process, TTY, and filesystem metadata. libc on Unix; a few Win32 calls
//! on Windows. No extra crates — keeps the musl binary tiny.

use std::fs::Metadata;
use std::path::PathBuf;

#[cfg(unix)]
mod imp {
    pub fn running_as_root() -> bool {
        unsafe { libc::geteuid() == 0 }
    }

    pub fn stdout_is_tty() -> bool {
        unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
    }

    pub fn stderr_is_tty() -> bool {
        unsafe { libc::isatty(libc::STDERR_FILENO) == 1 }
    }

    pub fn stdin_is_tty() -> bool {
        unsafe { libc::isatty(libc::STDIN_FILENO) == 1 }
    }
}

#[cfg(windows)]
mod imp {
    use crate::sys::win32;

    pub fn running_as_root() -> bool {
        unsafe { win32::IsUserAnAdmin() != 0 }
    }

    pub fn stdout_is_tty() -> bool {
        is_console(win32::STD_OUTPUT_HANDLE)
    }

    pub fn stderr_is_tty() -> bool {
        is_console(win32::STD_ERROR_HANDLE)
    }

    pub fn stdin_is_tty() -> bool {
        is_console(win32::STD_INPUT_HANDLE)
    }

    fn is_console(std_handle: u32) -> bool {
        unsafe {
            let h = win32::GetStdHandle(std_handle);
            if h.is_null() || h == win32::INVALID_HANDLE_VALUE {
                return false;
            }
            let mut mode = 0u32;
            win32::GetConsoleMode(h, &mut mode) != 0
        }
    }
}

#[cfg(windows)]
pub(crate) mod win32 {
    use std::ffi::c_void;

    pub type Handle = *mut c_void;

    pub const STD_INPUT_HANDLE: u32 = -10i32 as u32;
    pub const STD_OUTPUT_HANDLE: u32 = -11i32 as u32;
    pub const STD_ERROR_HANDLE: u32 = -12i32 as u32;
    pub const INVALID_HANDLE_VALUE: Handle = -1isize as Handle;
    pub const WAIT_OBJECT_0: u32 = 0;

    pub const ENABLE_PROCESSED_OUTPUT: u32 = 0x0001;
    pub const ENABLE_WRAP_AT_EOL_OUTPUT: u32 = 0x0002;
    pub const ENABLE_VIRTUAL_TERMINAL_PROCESSING: u32 = 0x0004;
    pub const DISABLE_NEWLINE_AUTO_RETURN: u32 = 0x0008;

    pub const ENABLE_PROCESSED_INPUT: u32 = 0x0001;
    pub const ENABLE_LINE_INPUT: u32 = 0x0002;
    pub const ENABLE_ECHO_INPUT: u32 = 0x0004;
    pub const ENABLE_WINDOW_INPUT: u32 = 0x0008;
    pub const ENABLE_MOUSE_INPUT: u32 = 0x0010;
    pub const ENABLE_EXTENDED_FLAGS: u32 = 0x0080;

    pub const KEY_EVENT: u16 = 0x0001;
    pub const MOUSE_EVENT: u16 = 0x0002;

    pub const FROM_LEFT_1ST_BUTTON_PRESSED: u32 = 0x0001;
    pub const MOUSE_MOVED: u32 = 0x0001;
    pub const DOUBLE_CLICK: u32 = 0x0002;

    pub const LEFT_CTRL_PRESSED: u32 = 0x0008;
    pub const RIGHT_CTRL_PRESSED: u32 = 0x0004;

    pub const VK_BACK: u16 = 0x08;
    pub const VK_RETURN: u16 = 0x0D;
    pub const VK_ESCAPE: u16 = 0x1B;
    pub const VK_PRIOR: u16 = 0x21;
    pub const VK_NEXT: u16 = 0x22;
    pub const VK_LEFT: u16 = 0x25;
    pub const VK_UP: u16 = 0x26;
    pub const VK_RIGHT: u16 = 0x27;
    pub const VK_DOWN: u16 = 0x28;
    pub const VK_F1: u16 = 0x70;
    pub const VK_C: u16 = 0x43;

    pub const FO_DELETE: u32 = 0x0003;
    pub const FOF_SILENT: u16 = 0x0004;
    pub const FOF_NOCONFIRMATION: u16 = 0x0010;
    pub const FOF_ALLOWUNDO: u16 = 0x0040;
    pub const FOF_NOERRORUI: u16 = 0x0400;

    pub const CP_UTF8: u32 = 65001;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct Coord {
        pub x: i16,
        pub y: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct SmallRect {
        pub left: i16,
        pub top: i16,
        pub right: i16,
        pub bottom: i16,
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct ConsoleScreenBufferInfo {
        pub size: Coord,
        pub cursor: Coord,
        pub attributes: u16,
        pub window: SmallRect,
        pub max_window: Coord,
    }

    /// `INPUT_RECORD` without a C union: 2-byte type, 2-byte pad, 16-byte payload.
    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct InputRecord {
        pub event_type: u16,
        pub _pad: u16,
        pub payload: [u8; 16],
    }

    #[repr(C)]
    pub struct ShFileOpStructW {
        pub hwnd: Handle,
        pub w_func: u32,
        pub p_from: *const u16,
        pub p_to: *const u16,
        pub f_flags: u16,
        pub f_any_operations_aborted: i32,
        pub h_name_mappings: Handle,
        pub lpsz_progress_title: *const u16,
    }

    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetStdHandle(n_std_handle: u32) -> Handle;
        pub fn GetConsoleMode(h: Handle, mode: *mut u32) -> i32;
        pub fn SetConsoleMode(h: Handle, mode: u32) -> i32;
        pub fn GetConsoleScreenBufferInfo(h: Handle, info: *mut ConsoleScreenBufferInfo) -> i32;
        pub fn SetConsoleOutputCP(codepage: u32) -> i32;
        pub fn SetConsoleCP(codepage: u32) -> i32;
        pub fn WaitForSingleObject(h: Handle, ms: u32) -> u32;
        pub fn ReadConsoleInputW(h: Handle, buf: *mut InputRecord, len: u32, read: *mut u32)
            -> i32;
        pub fn ReadFile(
            h: Handle,
            buf: *mut u8,
            len: u32,
            read: *mut u32,
            overlapped: *mut c_void,
        ) -> i32;
    }

    #[link(name = "shell32")]
    extern "system" {
        pub fn IsUserAnAdmin() -> i32;
        pub fn SHFileOperationW(op: *mut ShFileOpStructW) -> i32;
    }

    pub fn key_event(payload: &[u8; 16]) -> (bool, u16, u16, u32) {
        let key_down = i32::from_le_bytes(payload[0..4].try_into().unwrap()) != 0;
        let vk = u16::from_le_bytes(payload[6..8].try_into().unwrap());
        let uchar = u16::from_le_bytes(payload[10..12].try_into().unwrap());
        let ctrl = u32::from_le_bytes(payload[12..16].try_into().unwrap());
        (key_down, vk, uchar, ctrl)
    }

    pub fn mouse_event(payload: &[u8; 16]) -> (i16, i16, u32, u32) {
        let x = i16::from_le_bytes(payload[0..2].try_into().unwrap());
        let y = i16::from_le_bytes(payload[2..4].try_into().unwrap());
        let buttons = u32::from_le_bytes(payload[4..8].try_into().unwrap());
        let flags = u32::from_le_bytes(payload[12..16].try_into().unwrap());
        (x, y, buttons, flags)
    }
}

pub use imp::{running_as_root, stderr_is_tty, stdin_is_tty, stdout_is_tty};

/// Device id for `--one-file-system`. Unix uses `st_dev`. Windows uses the
/// drive letter (or a UNC sentinel) — `MetadataExt::volume_serial_number`
/// is not available on every Windows Rust target.
pub fn path_dev(path: &std::path::Path, meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        let _ = path;
        use std::os::unix::fs::MetadataExt;
        meta.dev()
    }
    #[cfg(windows)]
    {
        let _ = meta;
        windows_volume_id(path)
    }
}

#[cfg(windows)]
fn windows_volume_id(path: &std::path::Path) -> u64 {
    let raw = path.to_string_lossy();
    let s = raw.strip_prefix(r"\\?\").unwrap_or(raw.as_ref());
    let b = s.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        return b[0].to_ascii_uppercase() as u64;
    }
    if s.starts_with('\\') && s.as_bytes().get(1) == Some(&b'\\') {
        return 1;
    }
    0
}

pub fn meta_ino(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.ino()
    }
    #[cfg(windows)]
    {
        // `file_index` is still unstable (`windows_by_handle`). Hardlinks
        // are rare on NTFS waste scans; skip dedup rather than take a
        // nightly-only API.
        let _ = meta;
        0
    }
}

pub fn meta_nlink(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        meta.nlink()
    }
    #[cfg(windows)]
    {
        let _ = meta;
        1
    }
}

pub fn meta_size(meta: &Metadata) -> u64 {
    meta.len()
}

/// Allocated bytes on Unix (`st_blocks * 512`); file size on Windows.
pub fn meta_used(meta: &Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        crate::size::used_from_blocks(meta.blocks())
    }
    #[cfg(windows)]
    {
        meta.len()
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

pub fn full_disk_hint() -> &'static str {
    if cfg!(windows) {
        "not elevated — run PowerShell as Administrator: rings.exe C:\\"
    } else {
        "not running as root — sudo rings / to see the whole disk"
    }
}

pub fn scan_banner_privileged() -> &'static str {
    if cfg!(windows) {
        "scanning as Administrator · other volumes skipped"
    } else if cfg!(target_os = "macos") {
        "scanning as root · other filesystems skipped · /dev skipped"
    } else {
        "scanning as root · other filesystems skipped · /proc /sys /dev /run skipped"
    }
}

pub fn scan_banner_unprivileged() -> &'static str {
    if cfg!(windows) {
        "not elevated · readable paths only · run as Administrator for C:\\"
    } else {
        "not root · readable paths only · sudo rings / for a full-disk scan"
    }
}

pub fn not_privileged_status(errors: u64) -> String {
    if cfg!(windows) {
        if errors > 0 {
            format!(
                "not elevated — run as Administrator to include restricted dirs ({errors} errors)"
            )
        } else {
            "not elevated — run as Administrator to scan the whole disk".into()
        }
    } else if errors > 0 {
        format!("not root — sudo rings / to include restricted dirs ({errors} errors)")
    } else {
        "not root — sudo rings / to scan the whole disk".into()
    }
}

pub fn not_privileged_hint(errors: u64) -> String {
    if cfg!(windows) {
        format!(
            "not elevated — run as Administrator for a full-disk scan{}",
            if errors > 0 {
                format!(" ({errors} unreadable)")
            } else {
                String::new()
            }
        )
    } else {
        format!(
            "not root — sudo rings / for a full-disk scan{}",
            if errors > 0 {
                format!(" ({errors} unreadable)")
            } else {
                String::new()
            }
        )
    }
}
