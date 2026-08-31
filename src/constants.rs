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

/// Deepest ring count on a large panel.
pub const SUNBURST_RINGS_MAX: usize = 8;

/// Floor so a tiny terminal does not turn the disk into noise.
pub const SUNBURST_RINGS_MIN: usize = 4;

/// Target half-block pixels of radial thickness per ring.
pub const SUNBURST_RING_PX: f64 = 2.35;

/// Tightest inner hole, as a fraction of the outer radius.
pub const SUNBURST_HOLE_MIN: f64 = 0.13;

/// Loosest inner hole (tiny panels still need a well for the size label).
pub const SUNBURST_HOLE_MAX: f64 = 0.20;

/// Half-block pixels reserved for the center size label.
pub const SUNBURST_HOLE_LABEL_PX: f64 = 2.8;

/// Hair of empty cells around the disk so the edge does not clip.
pub const SUNBURST_MARGIN: f64 = 0.55;

/// Parent-relative floor; thinner children join "smaller objects".
pub const MIN_SLICE_FRACTION: f64 = 0.0065;

/// Absolute angular floor (fraction of the full circle) so dust stays grouped.
pub const MIN_SLICE_ANGLE: f64 = 0.0032;

/// Double-click window for mouse drill-in.
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// How many breadcrumb ancestors to keep visible before ellipsis.
pub const BREADCRUMB_MAX_PARTS: usize = 6;

pub const SMALLER_OBJECTS_LABEL: &str = "smaller objects";

pub const DELETE_LOG_ROOT: &str = "/var/log/rings-delete.log";
pub const DELETE_LOG_USER_REL: &str = ".local/share/rings/delete.log";
pub const TRASH_REL: &str = ".local/share/Trash";
pub const MACOS_TRASH_REL: &str = ".Trash";
