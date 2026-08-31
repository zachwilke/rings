//! Directory picker: choose a scan root when rings starts with no PATH.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

/// One directory listing plus the cursor over it. Directories sort first so
/// the next scan root is always near the top.
#[derive(Clone, Debug)]
pub struct Picker {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub selected: usize,
    pub offset: usize,
}

impl Picker {
    pub fn open(dir: &Path) -> Result<Picker, String> {
        let dir = absolute(dir);
        let entries = read_entries(&dir)?;
        Ok(Picker {
            dir,
            entries,
            selected: 0,
            offset: 0,
        })
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    /// Directory the scan would start from: the highlighted one, else this one.
    pub fn scan_target(&self) -> PathBuf {
        match self.selected_entry() {
            Some(e) if e.is_dir => e.path.clone(),
            _ => self.dir.clone(),
        }
    }

    pub fn move_sel(&mut self, delta: isize, page: usize) {
        if self.entries.is_empty() {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        let last = (self.entries.len() - 1) as isize;
        self.selected = (self.selected as isize + delta).clamp(0, last) as usize;
        self.ensure_visible(page);
    }

    pub fn move_to(&mut self, index: usize, page: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.selected = index.min(self.entries.len() - 1);
        self.ensure_visible(page);
    }

    fn ensure_visible(&mut self, page: usize) {
        if self.selected < self.offset {
            self.offset = self.selected;
        } else if self.selected >= self.offset + page {
            self.offset = self.selected.saturating_sub(page.saturating_sub(1));
        }
    }

    /// Descend into the highlighted directory. Files and unreadable
    /// directories leave the listing untouched.
    pub fn enter(&mut self, page: usize) -> Result<(), String> {
        let Some(entry) = self.selected_entry() else {
            return Ok(());
        };
        if !entry.is_dir {
            return Ok(());
        }
        let target = entry.path.clone();
        self.goto(&target, None, page)
    }

    /// Go up one level, keeping the directory we left highlighted.
    pub fn up(&mut self, page: usize) -> Result<(), String> {
        let Some(parent) = self.dir.parent().map(|p| p.to_path_buf()) else {
            return Ok(());
        };
        let leaving = self.dir.clone();
        self.goto(&parent, Some(&leaving), page)
    }

    fn goto(&mut self, dir: &Path, select: Option<&Path>, page: usize) -> Result<(), String> {
        let dir = absolute(dir);
        let entries = read_entries(&dir)?;
        self.dir = dir;
        self.entries = entries;
        self.selected = 0;
        self.offset = 0;
        if let Some(want) = select {
            if let Some(i) = self.entries.iter().position(|e| e.path == want) {
                self.move_to(i, page);
            }
        }
        Ok(())
    }
}

fn absolute(dir: &Path) -> PathBuf {
    std::path::absolute(dir).unwrap_or_else(|_| dir.to_path_buf())
}

fn read_entries(dir: &Path) -> Result<Vec<Entry>, String> {
    let rd = fs::read_dir(dir).map_err(|e| format!("cannot open {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for entry in rd.flatten() {
        let path = entry.path();
        let ft = entry.file_type().ok();
        let is_dir = match ft {
            Some(t) if t.is_dir() => true,
            // A symlink to a directory is still worth walking into here.
            Some(t) if t.is_symlink() => path.is_dir(),
            _ => false,
        };
        out.push(Entry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path,
            is_dir,
        });
    }
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn fixture() -> tempfile::TempDir {
        let tmp = tempfile::TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("zeta")).unwrap();
        fs::create_dir(tmp.path().join("alpha")).unwrap();
        fs::create_dir(tmp.path().join("alpha").join("inner")).unwrap();
        fs::write(tmp.path().join("a-file.txt"), b"x").unwrap();
        tmp
    }

    #[test]
    fn dirs_sort_before_files_case_insensitively() {
        let tmp = fixture();
        let p = Picker::open(tmp.path()).unwrap();
        let names: Vec<&str> = p.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["alpha", "zeta", "a-file.txt"]);
        assert!(p.entries[0].is_dir);
        assert!(!p.entries[2].is_dir);
    }

    #[test]
    fn enter_descends_and_up_reselects_the_dir_we_left() {
        let tmp = fixture();
        let mut p = Picker::open(tmp.path()).unwrap();
        p.enter(8).unwrap();
        assert_eq!(p.dir, absolute(&tmp.path().join("alpha")));
        assert_eq!(p.entries.len(), 1, "alpha holds one child");

        p.up(8).unwrap();
        assert_eq!(p.dir, absolute(tmp.path()));
        assert_eq!(
            p.selected_entry().map(|e| e.name.as_str()),
            Some("alpha"),
            "coming back up keeps the folder we left under the cursor"
        );
    }

    #[test]
    fn entering_a_file_is_a_no_op() {
        let tmp = fixture();
        let mut p = Picker::open(tmp.path()).unwrap();
        p.move_to(2, 8);
        assert_eq!(
            p.selected_entry().map(|e| e.name.as_str()),
            Some("a-file.txt")
        );
        p.enter(8).unwrap();
        assert_eq!(p.dir, absolute(tmp.path()), "files do not change directory");
    }

    #[test]
    fn scan_target_is_the_dir_under_the_cursor_else_the_current_dir() {
        let tmp = fixture();
        let mut p = Picker::open(tmp.path()).unwrap();
        assert_eq!(p.scan_target(), absolute(&tmp.path().join("alpha")));
        p.move_to(2, 8); // a-file.txt
        assert_eq!(
            p.scan_target(),
            absolute(tmp.path()),
            "a file under the cursor scans the directory itself"
        );
    }

    #[test]
    fn unreadable_directory_reports_an_error() {
        let tmp = fixture();
        let missing = tmp.path().join("nope");
        assert!(Picker::open(&missing).is_err());
    }

    #[test]
    fn move_sel_clamps_and_scrolls() {
        let tmp = fixture();
        let mut p = Picker::open(tmp.path()).unwrap();
        p.move_sel(-5, 2);
        assert_eq!(p.selected, 0);
        p.move_sel(99, 2);
        assert_eq!(p.selected, 2, "clamped to the last row");
        assert_eq!(p.offset, 1, "offset follows the cursor for a 2-row page");
    }
}
