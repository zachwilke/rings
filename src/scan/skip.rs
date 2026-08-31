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
    one_file_system: bool,
    root_dev: u64,
    entry_dev: u64,
) -> Option<SkipReason> {
    if is_special_path(path) {
        return Some(SkipReason::Special);
    }
    if is_other_filesystem(one_file_system, root_dev, entry_dev) {
        return Some(SkipReason::OtherFilesystem);
    }
    None
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
        assert!(is_special_path(Path::new(r"C:\$Recycle.Bin")));
        assert!(is_special_path(Path::new(r"C:\$Recycle.Bin\S-1-5-18")));
        assert!(is_special_path(Path::new(r"D:\System Volume Information")));
        assert!(is_special_path(Path::new(r"C:\pagefile.sys")));
        assert!(is_special_path(Path::new(r"C:\hiberfil.sys")));
        assert!(!is_special_path(Path::new(r"C:\Users\zach")));
        assert!(!is_special_path(Path::new(r"C:\Temp")));
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
            skip_reason(Path::new("/proc"), true, tmp_dev, proc_dev),
            Some(SkipReason::Special)
        );
        assert_eq!(
            skip_reason(Path::new("/mnt/other"), true, tmp_dev, proc_dev),
            Some(SkipReason::OtherFilesystem)
        );
    }
}
