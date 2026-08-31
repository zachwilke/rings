//! Marked-for-delete list. Nothing is unlinked until `commit` after confirm.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::classify::Category;
use crate::delete::safeguard::{is_safeguarded, refuse_reason, SafeguardRefuse};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CollectorItem {
    pub path: PathBuf,
    pub is_dir: bool,
    pub size_bytes: u64,
    pub category: Category,
    pub node_id: usize,
}

#[derive(Clone, Debug, Default)]
pub struct Collector {
    items: Vec<CollectorItem>,
}

impl Collector {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    pub fn items(&self) -> &[CollectorItem] {
        &self.items
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.size_bytes).sum()
    }

    pub fn contains_path(&self, path: &Path) -> bool {
        self.items.iter().any(|i| i.path == path)
    }

    /// Add a path. Safeguarded paths are refused and not stored.
    pub fn mark(&mut self, item: CollectorItem) -> Result<(), SafeguardRefuse> {
        if let Some(r) = refuse_reason(&item.path) {
            return Err(r);
        }
        if self.contains_path(&item.path) {
            return Ok(());
        }
        // Drop children already marked if we now mark a parent.
        self.items
            .retain(|existing| !existing.path.starts_with(&item.path));
        // Skip if an ancestor is already marked.
        if self
            .items
            .iter()
            .any(|existing| item.path.starts_with(&existing.path))
        {
            return Ok(());
        }
        self.items.push(item);
        Ok(())
    }

    pub fn unmark(&mut self, path: &Path) {
        self.items.retain(|i| i.path != path);
    }

    pub fn toggle(&mut self, item: CollectorItem) -> Result<bool, SafeguardRefuse> {
        if self.contains_path(&item.path) {
            self.unmark(&item.path);
            return Ok(false);
        }
        self.mark(item)?;
        Ok(true)
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn paths_set(&self) -> BTreeSet<PathBuf> {
        self.items.iter().map(|i| i.path.clone()).collect()
    }
}

/// How the operator confirmed. `commit` requires this; building a collector does not delete.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Confirm {
    /// Typed the safeguard phrase (required as root, or when trash is unavailable).
    TypedPhrase(String),
    /// Non-root operator accepted a trash move after reviewing the list.
    TrashAndEnter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommitMode {
    Trash,
    Unlink,
}

#[derive(Clone, Debug)]
pub struct CommitResult {
    pub deleted: Vec<PathBuf>,
    pub failed: Vec<(PathBuf, String)>,
    pub mode: CommitMode,
}

pub fn needs_typed_confirm() -> bool {
    crate::sys::running_as_root() || !trash_dir().is_some_and(|p| can_use_trash(&p))
}

fn trash_dir() -> Option<PathBuf> {
    if crate::sys::running_as_root() {
        return None;
    }
    #[cfg(windows)]
    {
        // Recycle Bin is not a folder we write into; a sentinel means "available".
        return Some(PathBuf::from("Recycle.Bin"));
    }
    #[cfg(target_os = "macos")]
    {
        return Some(crate::sys::home_dir()?.join(crate::constants::MACOS_TRASH_REL));
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(crate::sys::home_dir()?.join(crate::constants::TRASH_REL))
    }
}

fn can_use_trash(dir: &Path) -> bool {
    #[cfg(windows)]
    {
        let _ = dir;
        return true;
    }
    #[cfg(target_os = "macos")]
    {
        if dir.exists() {
            return dir.is_dir();
        }
        std::fs::create_dir_all(dir).is_ok()
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        if dir.exists() {
            return dir.is_dir();
        }
        std::fs::create_dir_all(dir.join("files")).is_ok()
            && std::fs::create_dir_all(dir.join("info")).is_ok()
    }
}

pub fn confirm_is_valid(confirm: &Confirm) -> Result<CommitMode, String> {
    match confirm {
        Confirm::TypedPhrase(s) => {
            if s == crate::constants::DELETE_CONFIRM_PHRASE {
                Ok(CommitMode::Unlink)
            } else {
                Err(format!(
                    "type {} to permanently delete",
                    crate::constants::DELETE_CONFIRM_PHRASE
                ))
            }
        }
        Confirm::TrashAndEnter => {
            if needs_typed_confirm() {
                Err("trash is unavailable; type DELETE to permanently remove".into())
            } else {
                Ok(CommitMode::Trash)
            }
        }
    }
}

/// Unlink or trash collector items. No-op unless `confirm` is valid.
pub fn commit(collector: &Collector, confirm: &Confirm) -> Result<CommitResult, String> {
    let mode = confirm_is_valid(confirm)?;
    if collector.is_empty() {
        return Ok(CommitResult {
            deleted: Vec::new(),
            failed: Vec::new(),
            mode,
        });
    }

    for item in collector.items() {
        if is_safeguarded(&item.path) {
            return Err(format!(
                "refusing commit: {} is safeguarded",
                item.path.display()
            ));
        }
    }

    let mut result = CommitResult {
        deleted: Vec::new(),
        failed: Vec::new(),
        mode,
    };

    for item in collector.items() {
        let outcome = match mode {
            CommitMode::Trash => move_to_trash(&item.path),
            CommitMode::Unlink => unlink(&item.path, item.is_dir),
        };
        match outcome {
            Ok(()) => {
                log_delete(&item.path, item.size_bytes, mode);
                result.deleted.push(item.path.clone());
            }
            Err(e) => result.failed.push((item.path.clone(), e)),
        }
    }
    Ok(result)
}

fn unlink(path: &Path, is_dir: bool) -> Result<(), String> {
    let res = if is_dir {
        std::fs::remove_dir_all(path)
    } else {
        std::fs::remove_file(path)
    };
    res.map_err(|e| e.to_string())
}

fn move_to_trash(path: &Path) -> Result<(), String> {
    #[cfg(windows)]
    {
        return recycle_bin(path);
    }
    #[cfg(target_os = "macos")]
    {
        return macos_trash(path);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let trash = trash_dir().ok_or_else(|| "no trash directory".to_string())?;
        let files = trash.join("files");
        let info = trash.join("info");
        std::fs::create_dir_all(&files).map_err(|e| e.to_string())?;
        std::fs::create_dir_all(&info).map_err(|e| e.to_string())?;

        let base = path
            .file_name()
            .ok_or_else(|| "path has no file name".to_string())?;
        let mut dest_name = base.to_os_string();
        let mut n = 1u32;
        while files.join(&dest_name).exists()
            || info
                .join(format!("{}.trashinfo", dest_name.to_string_lossy()))
                .exists()
        {
            dest_name = format!("{}-{n}", base.to_string_lossy()).into();
            n += 1;
            if n > 10_000 {
                return Err("too many trash name collisions".into());
            }
        }

        let dest = files.join(&dest_name);
        std::fs::rename(path, &dest).or_else(|_| copy_then_remove(path, &dest))?;

        let abs = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        };
        let body = format!(
            "[Trash Info]\nPath={}\nDeletionDate={}\n",
            abs.display(),
            now_utc_iso()
        );
        let info_name = format!("{}.trashinfo", dest_name.to_string_lossy());
        std::fs::write(info.join(info_name), body).map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn macos_trash(path: &Path) -> Result<(), String> {
    let trash = trash_dir().ok_or_else(|| "no trash directory".to_string())?;
    std::fs::create_dir_all(&trash).map_err(|e| e.to_string())?;
    let base = path
        .file_name()
        .ok_or_else(|| "path has no file name".to_string())?;
    let mut dest_name = base.to_os_string();
    let mut n = 1u32;
    while trash.join(&dest_name).exists() {
        dest_name = format!("{}-{n}", base.to_string_lossy()).into();
        n += 1;
        if n > 10_000 {
            return Err("too many trash name collisions".into());
        }
    }
    let dest = trash.join(&dest_name);
    std::fs::rename(path, &dest).or_else(|_| copy_then_remove(path, &dest))
}

#[cfg(not(windows))]
fn copy_then_remove(from: &Path, to: &Path) -> Result<(), String> {
    if from.is_dir() {
        copy_dir(from, to)?;
        std::fs::remove_dir_all(from).map_err(|e| e.to_string())
    } else {
        std::fs::copy(from, to).map_err(|e| e.to_string())?;
        std::fs::remove_file(from).map_err(|e| e.to_string())
    }
}

#[cfg(not(windows))]
fn copy_dir(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|e| e.to_string())?;
    for ent in std::fs::read_dir(from).map_err(|e| e.to_string())? {
        let ent = ent.map_err(|e| e.to_string())?;
        let dest = to.join(ent.file_name());
        let meta = ent.metadata().map_err(|e| e.to_string())?;
        if meta.is_dir() {
            copy_dir(&ent.path(), &dest)?;
        } else {
            std::fs::copy(ent.path(), dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// `YYYY-MM-DDThh:mm:ss` (UTC) from the UNIX clock, no chrono needed.
/// Civil-from-days algorithm by Howard Hinnant.
#[cfg(all(unix, not(target_os = "macos")))]
fn now_utc_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (h, m, s) = (tod / 3600, (tod / 60) % 60, tod % 60);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mo <= 2 { y + 1 } else { y };

    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{s:02}")
}

/// Append to the delete log. Never writes to stderr: the TUI owns the
/// screen, and anything printed there lands on top of the sunburst.
fn log_delete(path: &Path, size: u64, mode: CommitMode) {
    let line = format!(
        "rings deleted {} ({}) via {:?}\n",
        path.display(),
        crate::size::human_bytes(size),
        mode
    );
    if let Some(log_path) = delete_log_path() {
        if let Some(parent) = log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
        {
            use std::io::Write;
            let _ = f.write_all(line.as_bytes());
        }
    }
}

pub fn delete_log_path() -> Option<PathBuf> {
    if crate::sys::running_as_root() {
        #[cfg(windows)]
        {
            return Some(PathBuf::from(r"C:\ProgramData\rings\delete.log"));
        }
        #[cfg(not(windows))]
        {
            return Some(PathBuf::from(crate::constants::DELETE_LOG_ROOT));
        }
    }
    #[cfg(windows)]
    {
        let base = std::env::var_os("LOCALAPPDATA").or_else(|| std::env::var_os("USERPROFILE"))?;
        Some(PathBuf::from(base).join(r"rings\delete.log"))
    }
    #[cfg(not(windows))]
    {
        Some(crate::sys::home_dir()?.join(crate::constants::DELETE_LOG_USER_REL))
    }
}

/// Send `path` to the Windows Recycle Bin (`FO_DELETE` + `FOF_ALLOWUNDO`).
#[cfg(windows)]
fn recycle_bin(path: &Path) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;

    use crate::sys::win32;

    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .map(|cwd| cwd.join(path))
                .unwrap_or_else(|_| path.to_path_buf())
        }
    });
    let mut display = abs.to_string_lossy().replace('/', "\\");
    if let Some(stripped) = display.strip_prefix(r"\\?\") {
        display = stripped.to_string();
    }
    let mut wide: Vec<u16> = std::ffi::OsString::from(&display).encode_wide().collect();
    wide.push(0);
    wide.push(0);

    let mut op = win32::ShFileOpStructW {
        hwnd: std::ptr::null_mut(),
        w_func: win32::FO_DELETE,
        p_from: wide.as_ptr(),
        p_to: std::ptr::null(),
        f_flags: win32::FOF_ALLOWUNDO
            | win32::FOF_NOCONFIRMATION
            | win32::FOF_SILENT
            | win32::FOF_NOERRORUI,
        f_any_operations_aborted: 0,
        h_name_mappings: std::ptr::null_mut(),
        lpsz_progress_title: std::ptr::null(),
    };
    let rc = unsafe { win32::SHFileOperationW(&mut op) };
    if rc != 0 || op.f_any_operations_aborted != 0 {
        return Err(format!("Recycle Bin failed (code {rc})"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Category;
    use std::fs;
    use tempfile::TempDir;

    fn item(path: &Path, size: u64, is_dir: bool) -> CollectorItem {
        CollectorItem {
            path: path.to_path_buf(),
            is_dir,
            size_bytes: size,
            category: Category::Temp,
            node_id: 0,
        }
    }

    #[test]
    fn mark_does_not_unlink() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("keep.txt");
        fs::write(&f, "hello").unwrap();
        let mut c = Collector::new();
        c.mark(item(&f, 5, false)).unwrap();
        assert!(f.exists(), "collector must not delete on mark");
        assert_eq!(c.len(), 1);
        assert_eq!(c.total_bytes(), 5);
    }

    #[test]
    fn refuse_safeguarded_paths() {
        let mut c = Collector::new();
        #[cfg(unix)]
        let blocked = Path::new("/etc");
        #[cfg(windows)]
        let blocked = Path::new(r"C:\Windows");
        let err = c.mark(item(blocked, 1, true)).unwrap_err();
        assert!(err.reason.contains("safeguarded"));
        assert!(c.is_empty());
    }

    #[test]
    fn commit_requires_valid_confirm() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("x.txt");
        fs::write(&f, "zz").unwrap();
        let mut c = Collector::new();
        c.mark(item(&f, 2, false)).unwrap();

        let bad = commit(&c, &Confirm::TypedPhrase("please".into()));
        assert!(bad.is_err());
        assert!(f.exists(), "invalid confirm must not unlink");

        if needs_typed_confirm() {
            let ok = commit(
                &c,
                &Confirm::TypedPhrase(crate::constants::DELETE_CONFIRM_PHRASE.into()),
            )
            .unwrap();
            assert_eq!(ok.deleted.len(), 1);
            assert!(!f.exists());
        } else {
            let ok = commit(&c, &Confirm::TrashAndEnter).unwrap();
            assert_eq!(ok.deleted.len(), 1);
            assert!(!f.exists());
        }
    }

    #[test]
    fn typed_delete_unlinks() {
        let tmp = TempDir::new().unwrap();
        let f = tmp.path().join("gone.txt");
        fs::write(&f, "bye").unwrap();
        let mut c = Collector::new();
        c.mark(item(&f, 3, false)).unwrap();
        let result = commit(
            &c,
            &Confirm::TypedPhrase(crate::constants::DELETE_CONFIRM_PHRASE.into()),
        )
        .unwrap();
        assert_eq!(result.mode, CommitMode::Unlink);
        assert!(!f.exists());
        assert_eq!(result.deleted.len(), 1);
    }
}
