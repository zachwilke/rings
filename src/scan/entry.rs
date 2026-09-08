//! Directory listing used by the walker.
//!
//! `std::fs::DirEntry::metadata` is `lstat` / `fstatat(AT_SYMLINK_NOFOLLOW)`
//! (and the cached `FindFirstFile` record on Windows). That is the same
//! symlink semantics as `fs::symlink_metadata`, without building the full
//! path just to stat it.
//!
//! On macOS, `getattrlistbulk` can fill names + file sizes in one syscall
//! per buffer. Directories and anything missing attributes fall back to
//! `lstat` so used-byte accounting stays exact.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::scan::tree::ScanStats;
use crate::sys;

#[cfg(target_os = "macos")]
use crate::scan::darwin;

/// One directory child with everything the walker needs from a stat.
#[derive(Debug)]
pub struct DirEntryInfo {
    pub path: PathBuf,
    pub is_walkable_dir: bool,
    pub used: u64,
    pub apparent: u64,
    pub nlink: u64,
    pub ino: u64,
    pub dev: u64,
}

impl DirEntryInfo {
    pub fn from_meta(path: PathBuf, meta: &fs::Metadata) -> Self {
        Self {
            is_walkable_dir: sys::is_walkable_dir(meta),
            used: sys::meta_used(&path, meta),
            apparent: sys::meta_size(meta),
            nlink: sys::meta_nlink(meta),
            ino: sys::meta_ino(meta),
            dev: sys::path_dev(&path, meta),
            path,
        }
    }
}

pub fn note_error(stats: &mut ScanStats, err: &io::Error) {
    stats.errors += 1;
    if err.kind() == io::ErrorKind::PermissionDenied {
        stats.permission_denied += 1;
    }
}

/// Children of `dir`. Never follows directory symlinks (lstat / no-follow).
pub fn read_dir_entries(dir: &Path, stats: &mut ScanStats) -> Vec<DirEntryInfo> {
    #[cfg(target_os = "macos")]
    {
        if darwin::bulk_enabled() {
            match darwin::read_dir_bulk(dir, stats) {
                Ok(entries) => return entries,
                Err(_) => {
                    // Unsupported fs, parse failure, or open error — std path.
                }
            }
        }
    }
    read_dir_entries_std(dir, stats)
}

pub fn read_dir_entries_std(dir: &Path, stats: &mut ScanStats) -> Vec<DirEntryInfo> {
    let rd = match fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => {
            note_error(stats, &e);
            return Vec::new();
        }
    };
    let mut out = Vec::new();
    for ent in rd {
        match ent {
            Ok(ent) => match ent.metadata() {
                Ok(meta) => out.push(DirEntryInfo::from_meta(ent.path(), &meta)),
                Err(e) => note_error(stats, &e),
            },
            Err(e) => note_error(stats, &e),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn direntry_metadata_matches_symlink_metadata() {
        let tmp = TempDir::new().unwrap();
        let file = tmp.path().join("a.bin");
        fs::write(&file, vec![b'x'; 1234]).unwrap();
        fs::create_dir(tmp.path().join("sub")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file, tmp.path().join("link")).unwrap();

        let mut stats = ScanStats::default();
        let entries = read_dir_entries_std(tmp.path(), &mut stats);
        assert_eq!(stats.errors, 0);
        assert!(entries.len() >= 2);

        for info in &entries {
            let meta = fs::symlink_metadata(&info.path).unwrap();
            let expect = DirEntryInfo::from_meta(info.path.clone(), &meta);
            assert_eq!(info.is_walkable_dir, expect.is_walkable_dir, "{:?}", info.path);
            assert_eq!(info.apparent, expect.apparent, "{:?}", info.path);
            assert_eq!(info.used, expect.used, "{:?}", info.path);
            assert_eq!(info.nlink, expect.nlink, "{:?}", info.path);
            assert_eq!(info.ino, expect.ino, "{:?}", info.path);
            assert_eq!(info.dev, expect.dev, "{:?}", info.path);
        }
    }
}
