//! Stable values used across scan, TUI, delete, and export.

use std::time::Duration;

/// Unix `st_blocks` unit.
pub const BLOCK_BYTES: u64 = 512;

/// Files smaller than this are omitted from CSV/JSON unless they are waste hits.
pub const EXPORT_FILE_MIN_BYTES: u64 = 1_048_576;

/// How often the walker reports progress.
pub const PROGRESS_EVERY_ENTRIES: u64 = 256;

/// Virtual / unusable paths never descended into, even on an explicit root scan.
/// Linux-only mounts (`/proc` `/sys` `/run`) are listed on all Unix: they are
/// a no-op on macOS where those directories do not exist. `/dev` is skipped
/// on both Linux and macOS.
#[cfg(unix)]
pub const SPECIAL_SKIP_PATHS: &[&str] = &["/proc", "/sys", "/dev", "/run"];

/// Exact paths that must never be deleted.
#[cfg(unix)]
pub const SAFEGUARD_EXACT: &[&str] = &[
    "/",
    "/boot",
    "/etc",
    "/usr",
    "/bin",
    "/sbin",
    "/lib",
    "/lib64",
    "/System",
    "/private",
    "/private/etc",
];

/// Prefixes (must include trailing slash) that must never be deleted.
#[cfg(unix)]
pub const SAFEGUARD_PREFIXES: &[&str] = &[
    "/boot/",
    "/etc/",
    "/usr/",
    "/bin/",
    "/sbin/",
    "/lib/",
    "/lib64/",
    "/System/",
    "/private/etc/",
];

/// Windows component names (matched case-insensitively) that must not be walked.
#[cfg(windows)]
pub const SPECIAL_SKIP_COMPONENTS: &[&str] =
    &["$Recycle.Bin", "System Volume Information", "Recovery"];

#[cfg(windows)]
pub const SPECIAL_SKIP_FILES: &[&str] = &["pagefile.sys", "hiberfil.sys", "swapfile.sys"];

/// Phrase the operator must type to permanently unlink when trash is unavailable.
pub const DELETE_CONFIRM_PHRASE: &str = "DELETE";

/// Default CSV filename written from the TUI.
pub const TUI_EXPORT_FILENAME: &str = "rings-export.csv";

/// Rings drawn outward from the current directory.
pub const SUNBURST_RINGS: usize = 4;

/// Slices thinner than this fraction of the parent join "smaller objects".
pub const MIN_SLICE_FRACTION: f64 = 0.018;

/// Inner hole of the sunburst, as a fraction of the outer radius.
pub const SUNBURST_HOLE: f64 = 0.30;

/// Double-click window for mouse drill-in.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// How many breadcrumb ancestors to keep visible before ellipsis.
pub const BREADCRUMB_MAX_PARTS: usize = 6;

pub const SMALLER_OBJECTS_LABEL: &str = "smaller objects";

pub const DELETE_LOG_ROOT: &str = "/var/log/rings-delete.log";
pub const DELETE_LOG_USER_REL: &str = ".local/share/rings/delete.log";
pub const TRASH_REL: &str = ".local/share/Trash";
pub const MACOS_TRASH_REL: &str = ".Trash";
