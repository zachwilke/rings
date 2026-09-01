//! Iterative filesystem walk. Does not follow directory symlinks.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use crate::apps;
use crate::classify::classify;
use crate::constants::PROGRESS_EVERY_ENTRIES;
use crate::scan::skip::{is_special_path, skip_reason, SkipReason};
use crate::scan::tree::{Node, ScanStats, Tree};
use crate::sys;

#[derive(Clone, Debug)]
pub struct WalkOptions {
    pub one_file_system: bool,
    /// How much application analysis to do once the walk finishes.
    pub apps: apps::Options,
    /// When set, treat this as the root device instead of the start path's `st_dev`.
    /// Used by tests to prove other-filesystem skipping without a bind mount.
    pub root_dev_override: Option<u64>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            one_file_system: true,
            apps: apps::Options::default(),
            root_dev_override: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Progress {
    pub files: u64,
    pub dirs: u64,
    pub errors: u64,
    pub current: PathBuf,
}

pub enum WalkEvent {
    Progress(Progress),
    Done(Result<Tree, String>),
}

pub fn scan(path: &Path, opts: WalkOptions) -> Result<Tree, String> {
    scan_inner(path, opts, None)
}

/// Walk on a background thread; the receiver yields progress, then `Done`.
pub fn spawn_scan(path: PathBuf, opts: WalkOptions) -> std::sync::mpsc::Receiver<WalkEvent> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || scan_with_progress(path, opts, tx));
    rx
}

pub fn scan_with_progress(path: PathBuf, opts: WalkOptions, tx: Sender<WalkEvent>) {
    match scan_inner(&path, opts, Some(&tx)) {
        Ok(tree) => {
            let _ = tx.send(WalkEvent::Done(Ok(tree)));
        }
        Err(e) => {
            let _ = tx.send(WalkEvent::Done(Err(e)));
        }
    }
}

fn scan_inner(
    path: &Path,
    opts: WalkOptions,
    tx: Option<&Sender<WalkEvent>>,
) -> Result<Tree, String> {
    let start =
        fs::symlink_metadata(path).map_err(|e| format!("cannot stat {}: {e}", path.display()))?;

    let root_dev = opts
        .root_dev_override
        .unwrap_or_else(|| sys::path_dev(path, &start));
    let mut tree = Tree {
        nodes: Vec::new(),
        root: 0,
        stats: ScanStats::default(),
        probes: Vec::new(),
    };
    let mut seen_inodes: HashSet<(u64, u64)> = HashSet::new();

    let root_id = push_node(
        &mut tree,
        &mut seen_inodes,
        path.to_path_buf(),
        name_of(path),
        None,
        &start,
    );
    tree.root = root_id;

    // Never treat the scan root as "other filesystem" — that would skip the
    // whole walk. Other-fs and special skips apply to descendants.
    if start.is_dir() && !is_special_path(path) {
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            let dir_path = tree.nodes[id].path.clone();
            let entries = read_children(&dir_path, &mut tree.stats);
            for (child_path, meta) in entries {
                if let Some(reason) = skip_reason(
                    &child_path,
                    opts.one_file_system,
                    root_dev,
                    sys::path_dev(&child_path, &meta),
                ) {
                    match reason {
                        SkipReason::Special => {
                            tree.stats.skipped_special += 1;
                            continue;
                        }
                        SkipReason::OtherFilesystem if meta.is_dir() => {
                            // Mount point: do not descend.
                            tree.stats.skipped_other_fs += 1;
                            continue;
                        }
                        SkipReason::OtherFilesystem => {
                            // Regular file with a different st_dev — common on
                            // overlayfs (Docker). Count it; there is nothing
                            // to descend into.
                        }
                    }
                }

                let child_name = name_of(&child_path);
                let child_id = push_node(
                    &mut tree,
                    &mut seen_inodes,
                    child_path,
                    child_name,
                    Some(id),
                    &meta,
                );
                tree.nodes[id].children.push(child_id);
                if tree.nodes[child_id].is_dir {
                    stack.push(child_id);
                }
                maybe_progress(&tree.stats, &tree.nodes[child_id].path, tx);
            }
        }
    } else if start.is_dir() && is_special_path(path) {
        tree.stats.skipped_special += 1;
    }

    tree.recompute();
    apps::annotate(&mut tree, &opts.apps);
    if let Some(tx) = tx {
        let _ = tx.send(WalkEvent::Progress(Progress {
            files: tree.stats.files,
            dirs: tree.stats.dirs,
            errors: tree.stats.errors,
            current: path.to_path_buf(),
        }));
    }
    Ok(tree)
}

fn read_children(dir: &Path, stats: &mut ScanStats) -> Vec<(PathBuf, fs::Metadata)> {
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
            Ok(ent) => match fs::symlink_metadata(ent.path()) {
                Ok(meta) => out.push((ent.path(), meta)),
                Err(e) => note_error(stats, &e),
            },
            Err(e) => note_error(stats, &e),
        }
    }
    out
}

fn note_error(stats: &mut ScanStats, err: &io::Error) {
    stats.errors += 1;
    if err.kind() == io::ErrorKind::PermissionDenied {
        stats.permission_denied += 1;
    }
}

fn maybe_progress(stats: &ScanStats, current: &Path, tx: Option<&Sender<WalkEvent>>) {
    let total = stats.files + stats.dirs;
    if total == 0 || total % PROGRESS_EVERY_ENTRIES != 0 {
        return;
    }
    if let Some(tx) = tx {
        let _ = tx.send(WalkEvent::Progress(Progress {
            files: stats.files,
            dirs: stats.dirs,
            errors: stats.errors,
            current: current.to_path_buf(),
        }));
    }
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

fn push_node(
    tree: &mut Tree,
    seen: &mut HashSet<(u64, u64)>,
    path: PathBuf,
    name: String,
    parent: Option<usize>,
    meta: &fs::Metadata,
) -> usize {
    let is_dir = meta.is_dir();
    let mut own_used = sys::meta_used(meta);
    let mut own_apparent = sys::meta_size(meta);

    if !is_dir {
        // Only multi-link inodes can repeat; tracking every file would cost
        // ~50 MB of set on a million-file scan for nothing.
        let nlink = sys::meta_nlink(meta);
        let ino = sys::meta_ino(meta);
        if nlink > 1 && ino != 0 && !seen.insert((sys::path_dev(&path, meta), ino)) {
            own_used = 0;
            own_apparent = 0;
            tree.stats.hardlinks_deduped += 1;
        }
        tree.stats.files += 1;
    } else {
        tree.stats.dirs += 1;
    }

    let name = if name.is_empty() {
        name_of(&path)
    } else {
        name
    };

    let node = Node {
        name,
        category: classify(&path),
        app: None,
        guard: None,
        path,
        parent,
        children: Vec::new(),
        is_dir,
        own_used,
        own_apparent,
        used: own_used,
        apparent: own_apparent,
        nlink: sys::meta_nlink(meta),
    };
    let id = tree.nodes.len();
    tree.nodes.push(node);
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn write_file(path: &Path, bytes: usize) {
        fs::write(path, vec![b'x'; bytes]).unwrap();
    }

    #[test]
    fn aggregates_nested_sizes() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("sub")).unwrap();
        write_file(&root.join("a.bin"), 1000);
        write_file(&root.join("sub").join("b.bin"), 4000);

        let tree = scan(root, WalkOptions::default()).unwrap();
        let a = tree.nodes.iter().find(|n| n.name == "a.bin").unwrap();
        let b = tree.nodes.iter().find(|n| n.name == "b.bin").unwrap();
        let sub = tree.nodes.iter().find(|n| n.name == "sub").unwrap();

        assert_eq!(a.apparent, 1000);
        assert_eq!(b.apparent, 4000);
        assert_eq!(sub.apparent, sub.own_apparent + 4000);
        assert_eq!(
            tree.root_node().apparent,
            tree.root_node().own_apparent + a.apparent + sub.apparent
        );
        assert_eq!(
            tree.root_node().used,
            tree.root_node().own_used
                + tree.nodes[tree.root_node().children[0]].used
                + tree.nodes[tree.root_node().children[1]].used
        );
    }

    #[cfg(unix)]
    #[test]
    fn hardlinks_count_size_once() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        let a = root.join("one.dat");
        let b = root.join("two.dat");
        write_file(&a, 8000);
        fs::hard_link(&a, &b).unwrap();

        let tree = scan(root, WalkOptions::default()).unwrap();
        assert_eq!(tree.stats.hardlinks_deduped, 1);

        let one = tree.nodes.iter().find(|n| n.name == "one.dat").unwrap();
        let two = tree.nodes.iter().find(|n| n.name == "two.dat").unwrap();
        assert!(
            (one.apparent == 8000 && two.apparent == 0)
                || (two.apparent == 8000 && one.apparent == 0)
        );
        let file_apparent = one.apparent + two.apparent;
        assert_eq!(file_apparent, 8000);
        assert_eq!(
            tree.root_node().apparent,
            tree.root_node().own_apparent + 8000
        );
    }

    #[test]
    fn one_file_system_skips_foreign_device() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("keep")).unwrap();
        write_file(&root.join("keep").join("x"), 100);
        fs::create_dir(root.join("foreign")).unwrap();
        write_file(&root.join("foreign").join("y"), 100);

        let real_dev = sys::path_dev(root, &fs::metadata(root).unwrap());
        let fake_dev = if real_dev == 0 { 1 } else { 0 };

        let tree = scan(
            root,
            WalkOptions {
                one_file_system: true,
                root_dev_override: Some(fake_dev),
                ..WalkOptions::default()
            },
        )
        .unwrap();

        assert!(
            tree.stats.skipped_other_fs >= 2,
            "children on the real device must be skipped: {:?}",
            tree.stats
        );
        assert!(
            tree.root_node().children.is_empty(),
            "no children should be attached when every entry is other-fs"
        );
    }

    #[test]
    fn does_not_follow_symlink_directories() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("real")).unwrap();
        write_file(&root.join("real").join("secret"), 2000);
        #[cfg(unix)]
        std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
        #[cfg(windows)]
        {
            if std::os::windows::fs::symlink_dir(root.join("real"), root.join("link")).is_err() {
                return;
            }
        }

        let tree = scan(root, WalkOptions::default()).unwrap();
        let link = tree.nodes.iter().find(|n| n.name == "link").unwrap();
        assert!(!link.is_dir);
        assert!(
            !tree
                .nodes
                .iter()
                .any(|n| n.name == "secret" && n.path.starts_with(root.join("link"))),
            "must not walk through the symlink"
        );
    }
}
