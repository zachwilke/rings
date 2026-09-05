//! Skip rules: virtual filesystems, other mounts, symlink policy.

use std::path::Path;

#[cfg(unix)]
use crate::constants::SPECIAL_SKIP_PATHS;
#[cfg(windows)]
use crate::constants::{SPECIAL_SKIP_COMPONENTS, SPECIAL_SKIP_FILES};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    Special,
    OtherFilesystem,
}

pub fn is_special_path(path: &Path) -> bool {
    #[cfg(unix)]
    {
        let raw = path.to_string_lossy();
        let s = raw.trim_end_matches('/');
        for special in SPECIAL_SKIP_PATHS {
            if s == *special || s.starts_with(&format!("{special}/")) {
                return true;
            }
        }
        false
    }
    #[cfg(windows)]
    {
        windows_special(path)
    }
}

#[cfg(windows)]
fn windows_special(path: &Path) -> bool {
    for c in path.components() {
        if let std::path::Component::Normal(name) = c {
            let n = name.to_string_lossy();
            if SPECIAL_SKIP_COMPONENTS
                .iter()
                .any(|s| n.eq_ignore_ascii_case(s))
            {
                return true;
            }
        }
    }
    if let Some(name) = path.file_name() {
        let n = name.to_string_lossy();
        if SPECIAL_SKIP_FILES.iter().any(|s| n.eq_ignore_ascii_case(s)) {
            return true;
        }
    }
    false
}

/// `true` when `--one-file-system` should refuse to descend.
pub fn is_other_filesystem(one_file_system: bool, root_dev: u64, entry_dev: u64) -> bool {
    one_file_system && root_dev != entry_dev
}

pub fn skip_reason(
    path: &Path,
    scan_root: &Path,
    one_file_system: bool,
    root_dev: u64,
    entry_dev: u64,
) -> Option<SkipReason> {
    if is_special_path(path) || is_apfs_data_alias(path, scan_root) {
        return Some(SkipReason::Special);
    }
    if is_other_filesystem(one_file_system, root_dev, entry_dev) {
        return Some(SkipReason::OtherFilesystem);
    }
    None
}

/// `/System/Volumes/Data` is the APFS data volume. On a scan of `/` it is
/// already visible through firmlinks (`/Users`, `/Applications`, `/Library`,
/// `/private`, …). Walking both copies every file. Skip the alias unless
/// Data itself (or a path under it) is the scan root.
pub fn is_apfs_data_alias(path: &Path, scan_root: &Path) -> bool {
    let path = unix_abs(path);
    if path != "/System/Volumes/Data" {
        return false;
    }
    let root = unix_abs(scan_root);
    root != "/System/Volumes/Data" && !root.starts_with("/System/Volumes/Data/")
}

fn unix_abs(path: &Path) -> String {
    let mut s = path.to_string_lossy().replace('\\', "/");
    if s.len() > 1 && s.ends_with('/') {
        s.pop();
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[cfg(unix)]
    #[test]
    fn special_paths_match_exact_and_children() {
        assert!(is_special_path(Path::new("/proc")));
        assert!(is_special_path(Path::new("/proc/1")));
        assert!(is_special_path(Path::new("/sys/class")));
        assert!(is_special_path(Path::new("/dev/sda")));
        assert!(is_special_path(Path::new("/run/user/1000")));
        assert!(!is_special_path(Path::new("/home/proc")));
        assert!(!is_special_path(Path::new("/tmp")));
        assert!(!is_special_path(Path::new("/var")));
    }

    #[cfg(windows)]
    #[test]
    fn special_paths_match_windows_junk() {
        assert!(
            !is_special_path(Path::new(r"C:\$Recycle.Bin")),
            "recycle bin is reclaimable space, not a virtual mount"
        );
        assert!(is_special_path(Path::new(r"D:\System Volume Information")));
        assert!(is_special_path(Path::new(r"C:\pagefile.sys")));
        assert!(is_special_path(Path::new(r"C:\hiberfil.sys")));
        assert!(!is_special_path(Path::new(r"C:\Users\zach")));
        assert!(!is_special_path(Path::new(r"C:\Temp")));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_skips_synthetic_volumes_not_the_data_volume() {
        assert!(is_special_path(Path::new("/System/Volumes/Preboot")));
        assert!(is_special_path(Path::new("/System/Volumes/VM")));
        assert!(is_special_path(Path::new("/System/Volumes/Update")));
        assert!(is_special_path(Path::new("/System/Volumes/Data/home")));
        assert!(
            !is_special_path(Path::new("/System/Volumes/Data")),
            "Data is the user volume; only skip it as a firmlink alias"
        );
        assert!(is_apfs_data_alias(
            Path::new("/System/Volumes/Data"),
            Path::new("/")
        ));
        assert!(is_apfs_data_alias(
            Path::new("/System/Volumes/Data"),
            Path::new("/Users")
        ));
        assert!(
            !is_apfs_data_alias(
                Path::new("/System/Volumes/Data"),
                Path::new("/System/Volumes/Data")
            ),
            "scanning Data itself must walk it"
        );
        assert!(!is_apfs_data_alias(
            Path::new("/System/Volumes/Data"),
            Path::new("/System/Volumes/Data/Users/zach")
        ));
        assert!(!is_apfs_data_alias(
            Path::new("/Users/zach"),
            Path::new("/")
        ));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn linux_skips_snap_mounts() {
        assert!(is_special_path(Path::new("/snap")));
        assert!(is_special_path(Path::new("/snap/firefox/current")));
        assert!(!is_special_path(Path::new("/var/lib/snapd")));
    }

    #[test]
    fn one_file_system_uses_device_ids() {
        assert!(!is_other_filesystem(true, 1, 1));
        assert!(is_other_filesystem(true, 1, 2));
        assert!(!is_other_filesystem(false, 1, 2));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_is_other_filesystem_from_tmp() {
        use crate::sys;
        let tmp_dev = sys::path_dev(
            std::path::Path::new("/tmp"),
            &std::fs::metadata("/tmp").expect("tmp"),
        );
        let proc_dev = sys::path_dev(
            std::path::Path::new("/proc"),
            &std::fs::metadata("/proc").expect("proc"),
        );
        assert_ne!(
            tmp_dev, proc_dev,
            "/tmp and /proc must be different devices on Linux"
        );
        assert!(is_other_filesystem(true, tmp_dev, proc_dev));
        assert_eq!(
            skip_reason(Path::new("/proc"), Path::new("/"), true, tmp_dev, proc_dev),
            Some(SkipReason::Special)
        );
        assert_eq!(
            skip_reason(
                Path::new("/mnt/other"),
                Path::new("/"),
                true,
                tmp_dev,
                proc_dev
            ),
            Some(SkipReason::OtherFilesystem)
        );
    }
}
