//! Parallel filesystem walk. Does not follow directory symlinks.
//!
//! Workers pull directories from a shared queue, stat children, and enqueue
//! subdirectories. IDs are assigned with a monotonic atomic so a parent is
//! always a lower index than its descendants (`Tree::recompute` and app
//! guard propagation rely on that). Hardlink dedup and tree assembly run
//! after the walk, on one thread.
//!
//! `RINGS_SCAN_THREADS` overrides the worker count (default: CPUs, cap 32).
//! One thread uses the original iterative DFS so tiny scans stay cheap.

use std::collections::{HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Condvar, Mutex};
use std::time::Instant;

use crate::apps;
use crate::classify::{classify, Category};
use crate::constants::{PROGRESS_EVERY_ENTRIES, SCAN_THREADS_DEFAULT_CAP};
use crate::scan::entry::{read_dir_entries, DirEntryInfo};
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
    /// Worker threads. `None` uses [`effective_threads`].
    pub threads: Option<usize>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        Self {
            one_file_system: true,
            apps: apps::Options::default(),
            root_dev_override: None,
            threads: None,
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

/// Worker count: option, then `RINGS_SCAN_THREADS`, then available parallelism.
pub fn effective_threads(opts: &WalkOptions) -> usize {
    if let Some(n) = opts.threads {
        return n.max(1);
    }
    if let Ok(s) = std::env::var("RINGS_SCAN_THREADS") {
        if let Ok(n) = s.parse::<usize>() {
            return n.max(1);
        }
    }
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .clamp(1, SCAN_THREADS_DEFAULT_CAP)
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
    let started = Instant::now();
    let threads = effective_threads(&opts);
    let start = std::fs::symlink_metadata(path)
        .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;

    let root_dev = opts
        .root_dev_override
        .unwrap_or_else(|| sys::path_dev(path, &start));

    let mut tree = if threads <= 1 {
        walk_serial(path, &start, root_dev, &opts, tx)?
    } else {
        walk_parallel(path, &start, root_dev, &opts, threads, tx)?
    };

    tree.recompute();
    apps::annotate(&mut tree, &opts.apps);
    if scan_timing_enabled() {
        let elapsed = started.elapsed();
        eprintln!(
            "rings: scanned {} files, {} dirs in {:.3}s ({} thread{})",
            tree.stats.files,
            tree.stats.dirs,
            elapsed.as_secs_f64(),
            threads,
            if threads == 1 { "" } else { "s" }
        );
    }
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

fn scan_timing_enabled() -> bool {
    matches!(
        std::env::var("RINGS_SCAN_TIMING").as_deref(),
        Ok("1") | Ok("true") | Ok("yes")
    )
}

fn walk_serial(
    path: &Path,
    start: &std::fs::Metadata,
    root_dev: u64,
    opts: &WalkOptions,
    tx: Option<&Sender<WalkEvent>>,
) -> Result<Tree, String> {
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
        name_of(path),
        None,
        fields_from_meta(path, start),
    );
    tree.root = root_id;

    // Descend from the scan root whenever it *looks* like a directory
    // (`is_dir`), even on a Windows junction: the operator named it.
    // Children still use `is_walkable_dir`, so we do not then chase
    // junctions or symlink-dirs underneath.
    if start.is_dir() && !is_special_path(path) {
        let mut stack = vec![root_id];
        while let Some(id) = stack.pop() {
            let dir_path = tree.nodes[id].path.clone();
            let entries = read_dir_entries(&dir_path, &mut tree.stats);
            tree.nodes[id].children.reserve(entries.len());
            for info in entries {
                if let Some(child_id) =
                    accept_child(&mut tree, &mut seen_inodes, path, opts, root_dev, id, info)
                {
                    if tree.nodes[child_id].is_dir {
                        stack.push(child_id);
                    }
                    maybe_progress(&tree.stats, &tree.nodes[child_id].path, tx);
                }
            }
        }
    } else if start.is_dir() && is_special_path(path) {
        tree.stats.skipped_special += 1;
    }

    Ok(tree)
}

struct Job {
    path: PathBuf,
    id: usize,
}

struct RawNode {
    id: usize,
    parent: Option<usize>,
    name: String,
    path: PathBuf,
    is_dir: bool,
    own_used: u64,
    own_apparent: u64,
    nlink: u64,
    ino: u64,
    dev: u64,
    category: Category,
}

struct LocalOut {
    nodes: Vec<RawNode>,
    stats: ScanStats,
}

struct Pool {
    queue: Mutex<VecDeque<Job>>,
    cond: Condvar,
    inflight: AtomicUsize,
    next_id: AtomicUsize,
    files: AtomicU64,
    dirs: AtomicU64,
    errors: AtomicU64,
    scan_root: PathBuf,
    root_dev: u64,
    one_file_system: bool,
    tx: Option<Sender<WalkEvent>>,
}

fn walk_parallel(
    path: &Path,
    start: &std::fs::Metadata,
    root_dev: u64,
    opts: &WalkOptions,
    threads: usize,
    tx: Option<&Sender<WalkEvent>>,
) -> Result<Tree, String> {
    let root_fields = fields_from_meta(path, start);
    let should_walk = start.is_dir() && !is_special_path(path);
    let root = RawNode {
        id: 0,
        parent: None,
        name: name_of(path),
        path: path.to_path_buf(),
        is_dir: root_fields.is_walkable_dir,
        own_used: root_fields.used,
        own_apparent: root_fields.apparent,
        nlink: root_fields.nlink,
        ino: root_fields.ino,
        dev: root_fields.dev,
        category: classify(path),
    };

    if start.is_dir() && is_special_path(path) {
        let mut stats = ScanStats {
            skipped_special: 1,
            ..ScanStats::default()
        };
        let node = raw_to_node(root, &mut HashSet::new(), &mut stats);
        return Ok(Tree {
            nodes: vec![node],
            root: 0,
            stats,
            probes: Vec::new(),
        });
    }

    if !should_walk {
        let mut stats = ScanStats::default();
        let mut seen = HashSet::new();
        let node = raw_to_node(root, &mut seen, &mut stats);
        return Ok(Tree {
            nodes: vec![node],
            root: 0,
            stats,
            probes: Vec::new(),
        });
    }

    let pool = Pool {
        queue: Mutex::new(VecDeque::from([Job {
            path: path.to_path_buf(),
            id: 0,
        }])),
        cond: Condvar::new(),
        inflight: AtomicUsize::new(1),
        next_id: AtomicUsize::new(1),
        files: AtomicU64::new(0),
        dirs: AtomicU64::new(1),
        errors: AtomicU64::new(0),
        scan_root: path.to_path_buf(),
        root_dev,
        one_file_system: opts.one_file_system,
        tx: tx.cloned(),
    };

    let locals = std::thread::scope(|scope| {
        let mut handles = Vec::new();
        for _ in 1..threads {
            handles.push(scope.spawn(|| worker(&pool)));
        }
        let mut outs = Vec::with_capacity(threads);
        outs.push(worker(&pool));
        for h in handles {
            match h.join() {
                Ok(o) => outs.push(o),
                Err(_) => outs.push(LocalOut {
                    nodes: Vec::new(),
                    stats: ScanStats {
                        errors: 1,
                        ..ScanStats::default()
                    },
                }),
            }
        }
        outs
    });

    Ok(assemble(root, locals))
}

fn worker(pool: &Pool) -> LocalOut {
    let mut out = LocalOut {
        nodes: Vec::new(),
        stats: ScanStats::default(),
    };
    while let Some(job) = take_job(pool) {
        walk_one(pool, job, &mut out);
        finish_job(pool);
    }
    out
}

fn take_job(pool: &Pool) -> Option<Job> {
    let mut q = pool.queue.lock().unwrap();
    loop {
        if let Some(job) = q.pop_front() {
            return Some(job);
        }
        if pool.inflight.load(Ordering::Acquire) == 0 {
            return None;
        }
        q = pool.cond.wait(q).unwrap();
    }
}

fn enqueue(pool: &Pool, job: Job) {
    pool.inflight.fetch_add(1, Ordering::Relaxed);
    pool.queue.lock().unwrap().push_back(job);
    pool.cond.notify_one();
}

fn finish_job(pool: &Pool) {
    let left = pool.inflight.fetch_sub(1, Ordering::AcqRel) - 1;
    if left == 0 {
        pool.cond.notify_all();
    }
}

fn walk_one(pool: &Pool, job: Job, out: &mut LocalOut) {
    let errors_before = out.stats.errors;
    let entries = read_dir_entries(&job.path, &mut out.stats);
    let new_errors = out.stats.errors - errors_before;
    if new_errors > 0 {
        pool.errors.fetch_add(new_errors, Ordering::Relaxed);
    }
    for info in entries {
        if let Some(reason) = skip_reason(
            &info.path,
            &pool.scan_root,
            pool.one_file_system,
            pool.root_dev,
            info.dev,
        ) {
            match reason {
                SkipReason::Special => {
                    out.stats.skipped_special += 1;
                    continue;
                }
                SkipReason::OtherFilesystem if info.is_walkable_dir => {
                    out.stats.skipped_other_fs += 1;
                    continue;
                }
                SkipReason::OtherFilesystem => {}
            }
        }

        let id = pool.next_id.fetch_add(1, Ordering::Relaxed);
        let is_dir = info.is_walkable_dir;
        let name = name_of(&info.path);
        let category = classify(&info.path);
        let raw = RawNode {
            id,
            parent: Some(job.id),
            name,
            path: info.path,
            is_dir,
            own_used: info.used,
            own_apparent: info.apparent,
            nlink: info.nlink,
            ino: info.ino,
            dev: info.dev,
            category,
        };
        if is_dir {
            enqueue(
                pool,
                Job {
                    path: raw.path.clone(),
                    id,
                },
            );
            pool.dirs.fetch_add(1, Ordering::Relaxed);
        } else {
            pool.files.fetch_add(1, Ordering::Relaxed);
        }
        maybe_progress_atomic(pool, &raw.path);
        out.nodes.push(raw);
    }
}

fn maybe_progress_atomic(pool: &Pool, current: &Path) {
    let Some(tx) = pool.tx.as_ref() else {
        return;
    };
    let total = pool.files.load(Ordering::Relaxed) + pool.dirs.load(Ordering::Relaxed);
    if total == 0 || total % PROGRESS_EVERY_ENTRIES != 0 {
        return;
    }
    let _ = tx.send(WalkEvent::Progress(Progress {
        files: pool.files.load(Ordering::Relaxed),
        dirs: pool.dirs.load(Ordering::Relaxed),
        errors: pool.errors.load(Ordering::Relaxed),
        current: current.to_path_buf(),
    }));
}

fn assemble(root: RawNode, locals: Vec<LocalOut>) -> Tree {
    let mut stats = ScanStats::default();
    let mut raws = vec![root];
    for mut loc in locals {
        stats.errors += loc.stats.errors;
        stats.permission_denied += loc.stats.permission_denied;
        stats.skipped_other_fs += loc.stats.skipped_other_fs;
        stats.skipped_special += loc.stats.skipped_special;
        raws.append(&mut loc.nodes);
    }

    let n = raws.iter().map(|r| r.id).max().unwrap_or(0) + 1;
    let mut slots: Vec<Option<RawNode>> = (0..n).map(|_| None).collect();
    for r in raws {
        let id = r.id;
        slots[id] = Some(r);
    }

    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    for slot in &slots {
        if let Some(r) = slot {
            if let Some(p) = r.parent {
                children[p].push(r.id);
            }
        }
    }

    let mut seen: HashSet<(u64, u64)> = HashSet::new();
    let mut nodes = Vec::with_capacity(n);
    for (i, slot) in slots.into_iter().enumerate() {
        let r = slot.expect("walk id hole");
        let mut node = raw_to_node(r, &mut seen, &mut stats);
        node.children = std::mem::take(&mut children[i]);
        nodes.push(node);
    }

    Tree {
        nodes,
        root: 0,
        stats,
        probes: Vec::new(),
    }
}

fn raw_to_node(r: RawNode, seen: &mut HashSet<(u64, u64)>, stats: &mut ScanStats) -> Node {
    let mut own_used = r.own_used;
    let mut own_apparent = r.own_apparent;
    if !r.is_dir {
        if r.nlink > 1 && r.ino != 0 && !seen.insert((r.dev, r.ino)) {
            own_used = 0;
            own_apparent = 0;
            stats.hardlinks_deduped += 1;
        }
        stats.files += 1;
    } else {
        stats.dirs += 1;
    }
    let name = if r.name.is_empty() {
        name_of(&r.path)
    } else {
        r.name
    };
    Node {
        category: r.category,
        app: None,
        guard: None,
        name,
        path: r.path,
        parent: r.parent,
        children: Vec::new(),
        is_dir: r.is_dir,
        own_used,
        own_apparent,
        used: own_used,
        apparent: own_apparent,
        nlink: r.nlink,
    }
}

fn accept_child(
    tree: &mut Tree,
    seen: &mut HashSet<(u64, u64)>,
    scan_root: &Path,
    opts: &WalkOptions,
    root_dev: u64,
    parent: usize,
    info: DirEntryInfo,
) -> Option<usize> {
    if let Some(reason) = skip_reason(
        &info.path,
        scan_root,
        opts.one_file_system,
        root_dev,
        info.dev,
    ) {
        match reason {
            SkipReason::Special => {
                tree.stats.skipped_special += 1;
                return None;
            }
            SkipReason::OtherFilesystem if info.is_walkable_dir => {
                tree.stats.skipped_other_fs += 1;
                return None;
            }
            SkipReason::OtherFilesystem => {}
        }
    }
    let child_name = name_of(&info.path);
    let child_id = push_node(tree, seen, child_name, Some(parent), info);
    tree.nodes[parent].children.push(child_id);
    Some(child_id)
}

fn fields_from_meta(path: &Path, meta: &std::fs::Metadata) -> DirEntryInfo {
    DirEntryInfo::from_meta(path.to_path_buf(), meta)
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
    name: String,
    parent: Option<usize>,
    info: DirEntryInfo,
) -> usize {
    let is_dir = info.is_walkable_dir;
    let mut own_used = info.used;
    let mut own_apparent = info.apparent;

    if !is_dir {
        // Only multi-link inodes can repeat; tracking every file would cost
        // ~50 MB of set on a million-file scan for nothing.
        if info.nlink > 1 && info.ino != 0 && !seen.insert((info.dev, info.ino)) {
            own_used = 0;
            own_apparent = 0;
            tree.stats.hardlinks_deduped += 1;
        }
        tree.stats.files += 1;
    } else {
        tree.stats.dirs += 1;
    }

    let name = if name.is_empty() {
        name_of(&info.path)
    } else {
        name
    };

    let node = Node {
        name,
        category: classify(&info.path),
        app: None,
        guard: None,
        path: info.path,
        parent,
        children: Vec::new(),
        is_dir,
        own_used,
        own_apparent,
        used: own_used,
        apparent: own_apparent,
        nlink: info.nlink,
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

    fn snapshot_dirs(tree: &Tree) -> Vec<(String, bool, u64, u64)> {
        let root = &tree.nodes[tree.root].path;
        let mut rows: Vec<_> = tree
            .nodes
            .iter()
            .filter(|n| n.is_dir)
            .map(|n| {
                let rel = n
                    .path
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| n.path.to_string_lossy().into_owned());
                (rel, n.is_dir, n.used, n.apparent)
            })
            .collect();
        rows.sort();
        rows
    }

    fn file_names(tree: &Tree) -> Vec<String> {
        let root = &tree.nodes[tree.root].path;
        let mut names: Vec<_> = tree
            .nodes
            .iter()
            .filter(|n| !n.is_dir)
            .map(|n| {
                n.path
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|_| n.name.clone())
            })
            .collect();
        names.sort();
        names
    }

    fn make_wide(root: &Path, tops: usize, mids: usize, files: usize) {
        for t in 0..tops {
            let top = root.join(format!("t{t:02}"));
            fs::create_dir(&top).unwrap();
            for m in 0..mids {
                let mid = top.join(format!("m{m:02}"));
                fs::create_dir(&mid).unwrap();
                for f in 0..files {
                    write_file(&mid.join(format!("f{f:02}.dat")), 16);
                }
            }
        }
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
    fn walkable_dir_is_a_plain_directory() {
        let tmp = TempDir::new().unwrap();
        let meta = std::fs::symlink_metadata(tmp.path()).unwrap();
        assert!(sys::is_walkable_dir(&meta));
        let f = tmp.path().join("f");
        write_file(&f, 1);
        assert!(!sys::is_walkable_dir(&std::fs::symlink_metadata(&f).unwrap()));
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

    #[test]
    fn parallel_matches_serial_on_wide_tree() {
        let tmp = TempDir::new().unwrap();
        make_wide(tmp.path(), 6, 4, 5);
        #[cfg(unix)]
        {
            let a = tmp.path().join("t00").join("m00").join("f00.dat");
            let b = tmp.path().join("link-dup.dat");
            fs::hard_link(&a, &b).unwrap();
        }

        let serial = scan(
            tmp.path(),
            WalkOptions {
                threads: Some(1),
                apps: apps::Options::structural_only(),
                ..WalkOptions::default()
            },
        )
        .unwrap();
        let parallel = scan(
            tmp.path(),
            WalkOptions {
                threads: Some(4),
                apps: apps::Options::structural_only(),
                ..WalkOptions::default()
            },
        )
        .unwrap();

        assert_eq!(serial.stats.files, parallel.stats.files);
        assert_eq!(serial.stats.dirs, parallel.stats.dirs);
        assert_eq!(serial.stats.errors, parallel.stats.errors);
        assert_eq!(serial.stats.skipped_special, parallel.stats.skipped_special);
        assert_eq!(
            serial.stats.skipped_other_fs,
            parallel.stats.skipped_other_fs
        );
        assert_eq!(
            serial.stats.hardlinks_deduped,
            parallel.stats.hardlinks_deduped
        );
        assert_eq!(serial.root_node().used, parallel.root_node().used);
        assert_eq!(serial.root_node().apparent, parallel.root_node().apparent);
        assert_eq!(snapshot_dirs(&serial), snapshot_dirs(&parallel));
        assert_eq!(file_names(&serial), file_names(&parallel));
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn release_wide_tree_parallel_beats_serial() {
        let tmp = TempDir::new().unwrap();
        // Wide tree: 24k files / 800 dirs so thread startup is not the whole cost.
        make_wide(tmp.path(), 40, 20, 30);
        let opts_base = WalkOptions {
            apps: apps::Options::structural_only(),
            ..WalkOptions::default()
        };

        let time_n = |threads: usize| {
            let t0 = Instant::now();
            let tree = scan(
                tmp.path(),
                WalkOptions {
                    threads: Some(threads),
                    ..opts_base.clone()
                },
            )
            .unwrap();
            (t0.elapsed(), tree.stats.files, tree.root_node().used)
        };

        let threads = effective_threads(&WalkOptions {
            threads: None,
            ..opts_base.clone()
        })
        .max(2);
        let _ = time_n(1);
        let _ = time_n(threads);
        let mut serials = Vec::new();
        let mut parallels = Vec::new();
        let mut files_s = 0;
        let mut used_s = 0;
        for _ in 0..3 {
            let (s, f, u) = time_n(1);
            serials.push(s);
            files_s = f;
            used_s = u;
            let (p, f2, u2) = time_n(threads);
            parallels.push(p);
            assert_eq!(f, f2);
            assert_eq!(u, u2);
        }
        serials.sort();
        parallels.sort();
        let serial = serials[1];
        let parallel = parallels[1];
        eprintln!(
            "scan bench: {files_s} files  serial={:.3}s  parallel={:.3}s  speedup={:.2}x",
            serial.as_secs_f64(),
            parallel.as_secs_f64(),
            serial.as_secs_f64() / parallel.as_secs_f64().max(1e-6)
        );
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1);
        if cpus >= 2 {
            assert!(
                parallel <= serial,
                "parallel ({parallel:?}) should beat serial ({serial:?}) on {cpus} cpus"
            );
        }
    }
}
