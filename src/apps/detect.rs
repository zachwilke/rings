//! The annotation pass: turn a finished tree into an application-aware one.
//!
//! Runs once, after `Tree::recompute`, and never touches the walk itself —
//! the hot path in `scan/walk.rs` stays exactly as fast as it was.
//!
//! Structural detection and role assignment always run, because they are what
//! the delete safeguard reads; only the header reads are optional. Turning
//! probing off with `--no-app-probe` costs you the measured numbers, never
//! the protection.

use std::collections::BTreeSet;

use super::{mysql, postgres, sqlite, sqlserver, AppKind, AppTag, Probe, ProbeDetail, Role};
use super::{PROBE_MAX_FILES, PROBE_MIN_BYTES};
use crate::scan::Tree;
use crate::size::human_bytes;

/// How much work the annotation pass may do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// Read file headers. Structural detection and the safeguards run either
    /// way; this gates content reads only.
    pub probe: bool,
    /// Files below this are never probed.
    pub min_probe_bytes: u64,
    /// Ceiling on header reads per scan.
    pub max_probes: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            probe: true,
            min_probe_bytes: PROBE_MIN_BYTES,
            max_probes: PROBE_MAX_FILES,
        }
    }
}

impl Options {
    /// Structural detection only — no file contents are read.
    pub fn structural_only() -> Self {
        Self {
            probe: false,
            ..Self::default()
        }
    }
}

/// Tag every node rings recognises, and record what the probes measured.
pub fn annotate(tree: &mut Tree, opts: &Options) {
    // Server layouts first: they claim whole directories, and tagging them
    // up front keeps the file-level passes from re-examining a million
    // relation files one at a time.
    postgres_pass(tree, opts);
    mysql_pass(tree);
    sqlserver_pass(tree);
    if opts.probe {
        sqlite_pass(tree, opts);
    }
    propagate_guards(tree);
}

/// Push protection upward: a directory holding live database files is itself
/// undeletable, however far down they sit.
///
/// Without this, tagging `base/` as protected would still leave
/// `/var/lib/postgresql` — its untagged grandparent — markable, and one
/// confirm would take the cluster with it.
///
/// Relies on the same invariant as [`Tree::recompute`]: the walk pushes a
/// parent before any of its children, so a reverse pass sees every child
/// before the node that owns it.
fn propagate_guards(tree: &mut Tree) {
    for i in (0..tree.nodes.len()).rev() {
        let own = tree.nodes[i].app.filter(|t| t.is_protected());
        let guard = match own {
            Some(tag) => Some(tag),
            None => {
                // First protected descendant explains the refusal.
                let mut inherited = None;
                for k in 0..tree.nodes[i].children.len() {
                    let c = tree.nodes[i].children[k];
                    // Only server layouts pull protection up to a parent:
                    // a jointly load-bearing data directory dies to an
                    // `rm -rf` one level above it. A lone database file
                    // does not get to make its whole cache tree undeletable.
                    if let Some(tag) = tree.nodes[c].guard {
                        if tag.kind.propagates_guard() {
                            inherited = Some(tag);
                            break;
                        }
                    }
                }
                inherited
            }
        };
        tree.nodes[i].guard = guard;
    }
}

// ---------------------------------------------------------------- postgres

fn postgres_pass(tree: &mut Tree, opts: &Options) {
    for root in cluster_roots(tree) {
        let cluster = tag_cluster(tree, root, opts);
        tree.probes.push(Probe {
            node: root,
            kind: AppKind::Postgres,
            detail: ProbeDetail::Postgres(cluster),
        });
    }
}

/// Directories holding `PG_VERSION` + `base/` + `global/` together.
fn cluster_roots(tree: &Tree) -> Vec<usize> {
    let mut roots = Vec::new();
    for id in 0..tree.nodes.len() {
        if !tree.nodes[id].is_dir {
            continue;
        }
        let (mut version, mut base, mut global) = (false, false, false);
        for &c in &tree.nodes[id].children {
            let child = &tree.nodes[c];
            match child.name.as_str() {
                postgres::MARKER_VERSION => version = !child.is_dir,
                postgres::MARKER_BASE => base = child.is_dir,
                postgres::MARKER_GLOBAL => global = child.is_dir,
                _ => {}
            }
        }
        if version && base && global {
            roots.push(id);
        }
    }
    roots
}

/// Tag the whole cluster subtree and total it by role in one traversal.
fn tag_cluster(tree: &mut Tree, root: usize, opts: &Options) -> postgres::Cluster {
    let mut cluster = postgres::Cluster {
        version: if opts.probe {
            postgres::read_version(&tree.nodes[root].path)
        } else {
            None
        },
        ..postgres::Cluster::default()
    };

    tree.nodes[root].app = Some(AppTag::new(AppKind::Postgres, Role::Meta));
    cluster.meta_bytes = cluster.meta_bytes.saturating_add(tree.nodes[root].own_used);

    // Depth-first, carrying the role inherited from the top-level entry the
    // node sits under. `pgsql_tmp` overrides that at any depth, because spill
    // lives at `base/pgsql_tmp` and `base/<db>/pgsql_tmp` alike.
    let mut stack: Vec<(usize, Option<Role>)> = vec![(root, None)];
    while let Some((id, inherited)) = stack.pop() {
        let children = tree.nodes[id].children.clone();
        for c in children {
            let role = if tree.nodes[c].name == postgres::SPILL_DIR {
                Role::TempSpill
            } else if let Some(r) = inherited {
                r
            } else {
                postgres::top_role(&tree.nodes[c].name)
            };
            tree.nodes[c].app = Some(AppTag::new(AppKind::Postgres, role));

            let own = tree.nodes[c].own_used;
            match role {
                Role::Data => cluster.data_bytes = cluster.data_bytes.saturating_add(own),
                Role::Wal => {
                    cluster.wal_bytes = cluster.wal_bytes.saturating_add(own);
                    if !tree.nodes[c].is_dir && postgres::is_wal_segment(&tree.nodes[c].name) {
                        cluster.wal_segments += 1;
                    }
                }
                Role::TempSpill => cluster.temp_bytes = cluster.temp_bytes.saturating_add(own),
                Role::Log => cluster.log_bytes = cluster.log_bytes.saturating_add(own),
                Role::Meta => cluster.meta_bytes = cluster.meta_bytes.saturating_add(own),
            }

            if tree.nodes[c].is_dir {
                stack.push((c, Some(role)));
            }
        }
    }
    cluster
}

// ------------------------------------------------------------------- mysql

fn mysql_pass(tree: &mut Tree) {
    for root in mysql_roots(tree) {
        let server = tag_mysql(tree, root);
        tree.probes.push(Probe {
            node: root,
            kind: AppKind::Mysql,
            detail: ProbeDetail::Mysql(server),
        });
    }
}

/// Directories holding `ibdata1`: the InnoDB system tablespace exists in
/// every MySQL and MariaDB installation, so it identifies a datadir on its
/// own without any path guessing.
fn mysql_roots(tree: &Tree) -> Vec<usize> {
    let mut roots = Vec::new();
    for id in 0..tree.nodes.len() {
        if !tree.nodes[id].is_dir {
            continue;
        }
        for &c in &tree.nodes[id].children {
            let child = &tree.nodes[c];
            if !child.is_dir && child.name == mysql::MARKER_SYSTEM_TABLESPACE {
                roots.push(id);
                break;
            }
        }
    }
    roots
}

fn tag_mysql(tree: &mut Tree, root: usize) -> mysql::Server {
    // MariaDB ships the Aria control file; MySQL never has.
    let mut flavor = "MySQL";
    for k in 0..tree.nodes[root].children.len() {
        let c = tree.nodes[root].children[k];
        if tree.nodes[c].name == mysql::MARKER_MARIADB {
            flavor = "MariaDB";
            break;
        }
    }
    let mut server = mysql::Server {
        flavor,
        ..mysql::Server::default()
    };

    tree.nodes[root].app = Some(AppTag::new(AppKind::Mysql, Role::Meta));
    server.meta_bytes = server.meta_bytes.saturating_add(tree.nodes[root].own_used);

    let mut stack: Vec<(usize, Option<Role>)> = vec![(root, None)];
    while let Some((id, inherited)) = stack.pop() {
        for c in tree.nodes[id].children.clone() {
            let role = if tree.nodes[c].name == mysql::TEMP_DIR {
                Role::TempSpill
            } else if let Some(r) = inherited {
                r
            } else {
                mysql::top_role(&tree.nodes[c].name)
            };
            tree.nodes[c].app = Some(AppTag::new(AppKind::Mysql, role));

            let own = tree.nodes[c].own_used;
            match role {
                Role::Data => server.data_bytes = server.data_bytes.saturating_add(own),
                Role::Wal => server.wal_bytes = server.wal_bytes.saturating_add(own),
                Role::Binlog => {
                    server.binlog_bytes = server.binlog_bytes.saturating_add(own);
                    if !tree.nodes[c].is_dir {
                        server.binlog_files += 1;
                    }
                }
                Role::TempSpill => server.temp_bytes = server.temp_bytes.saturating_add(own),
                Role::Log => server.log_bytes = server.log_bytes.saturating_add(own),
                Role::Backup => server.backup_bytes = server.backup_bytes.saturating_add(own),
                Role::Meta => server.meta_bytes = server.meta_bytes.saturating_add(own),
            }

            if tree.nodes[c].is_dir {
                stack.push((c, Some(role)));
            }
        }
    }
    server
}

// --------------------------------------------------------------- sqlserver

/// SQL Server has no canonical data directory — files sit wherever the DBA
/// pointed them — so recognition is per file, then grouped by the directory
/// they landed in.
fn sqlserver_pass(tree: &mut Tree) {
    let mut tagged: Vec<usize> = Vec::new();
    for id in 0..tree.nodes.len() {
        if tree.nodes[id].is_dir || tree.nodes[id].app.is_some() {
            continue;
        }
        if let Some(role) = sqlserver::role_for(&tree.nodes[id].name) {
            tree.nodes[id].app = Some(AppTag::new(AppKind::SqlServer, role));
            tagged.push(id);
        }
    }
    if tagged.is_empty() {
        return;
    }

    let mut dirs: Vec<usize> = tagged
        .iter()
        .filter_map(|&id| tree.nodes[id].parent)
        .collect();
    dirs.sort_unstable();
    dirs.dedup();

    for &dir in &dirs {
        // `.bak` is far too common a suffix to claim on its own; a data file
        // in the same directory is what makes it SQL Server's.
        for k in 0..tree.nodes[dir].children.len() {
            let c = tree.nodes[dir].children[k];
            if tree.nodes[c].is_dir || tree.nodes[c].app.is_some() {
                continue;
            }
            if sqlserver::backup_extension(&tree.nodes[c].name) {
                tree.nodes[c].app = Some(AppTag::new(AppKind::SqlServer, Role::Backup));
            }
        }

        let mut inst = sqlserver::Instance::default();
        for k in 0..tree.nodes[dir].children.len() {
            let c = tree.nodes[dir].children[k];
            let Some(tag) = tree.nodes[c].app else { continue };
            if tag.kind != AppKind::SqlServer {
                continue;
            }
            let bytes = tree.nodes[c].own_used;
            match tag.role {
                Role::Data => {
                    inst.data_bytes = inst.data_bytes.saturating_add(bytes);
                    inst.data_files += 1;
                }
                Role::Wal => {
                    inst.log_bytes = inst.log_bytes.saturating_add(bytes);
                    inst.log_files += 1;
                }
                Role::TempSpill => inst.temp_bytes = inst.temp_bytes.saturating_add(bytes),
                Role::Backup => inst.backup_bytes = inst.backup_bytes.saturating_add(bytes),
                _ => {}
            }
        }
        tree.probes.push(Probe {
            node: dir,
            kind: AppKind::SqlServer,
            detail: ProbeDetail::SqlServer(inst),
        });
    }
}

// ------------------------------------------------------------------ sqlite

fn sqlite_pass(tree: &mut Tree, opts: &Options) {
    let mut attempts = 0usize;
    let mut found: Vec<(usize, sqlite::Header)> = Vec::new();

    for id in sqlite_candidates(tree, opts) {
        if attempts >= opts.max_probes {
            break;
        }
        attempts += 1;
        let (path, len) = {
            let n = &tree.nodes[id];
            (n.path.clone(), n.own_apparent)
        };
        if let Some(header) = sqlite::probe(&path, len) {
            found.push((id, header));
        }
    }

    for (id, header) in found {
        tree.nodes[id].app = Some(AppTag::new(AppKind::Sqlite, Role::Data));
        tag_sidecars(tree, id);
        tree.probes.push(Probe {
            node: id,
            kind: AppKind::Sqlite,
            detail: ProbeDetail::Sqlite(header),
        });
    }
}

/// Files worth opening: anything past the size floor, plus the base file of
/// any large sidecar. A 2 GB `-wal` beside a 4 MB database is precisely the
/// case worth catching, and the database itself is under the floor.
fn sqlite_candidates(tree: &Tree, opts: &Options) -> Vec<usize> {
    let mut out = Vec::new();
    let mut seen: BTreeSet<usize> = BTreeSet::new();

    for id in 0..tree.nodes.len() {
        let n = &tree.nodes[id];
        if n.is_dir || n.app.is_some() || n.own_apparent < opts.min_probe_bytes {
            continue;
        }
        let target = match sqlite::sidecar_of(&n.name) {
            Some((base, _)) => sibling_named(tree, id, base),
            None => Some(id),
        };
        let Some(t) = target else { continue };
        if tree.nodes[t].is_dir || tree.nodes[t].app.is_some() {
            continue;
        }
        if seen.insert(t) {
            out.push(t);
        }
    }
    out
}

fn sibling_named(tree: &Tree, id: usize, name: &str) -> Option<usize> {
    let parent = tree.nodes[id].parent?;
    tree.nodes[parent]
        .children
        .iter()
        .copied()
        .find(|&c| tree.nodes[c].name == name)
}

/// Tag `-wal` / `-shm` / `-journal` beside a database rings just identified.
fn tag_sidecars(tree: &mut Tree, db: usize) {
    let Some(parent) = tree.nodes[db].parent else {
        return;
    };
    let base = tree.nodes[db].name.clone();
    for c in tree.nodes[parent].children.clone() {
        if c == db || tree.nodes[c].is_dir || tree.nodes[c].app.is_some() {
            continue;
        }
        // Scoped so the borrow ends before the tag is written.
        let role = {
            match sqlite::sidecar_of(&tree.nodes[c].name) {
                Some((b, kind)) if b == base => Some(kind.role()),
                _ => None,
            }
        };
        if let Some(role) = role {
            tree.nodes[c].app = Some(AppTag::new(AppKind::Sqlite, role));
        }
    }
}

// --------------------------------------------------------------- summarise

/// One line in the databases view.
#[derive(Clone, Debug)]
pub struct DbEntry {
    /// Node to jump to when the row is opened.
    pub node: usize,
    pub kind: AppKind,
    pub role: Role,
    /// What to show as the row's name.
    pub label: String,
    pub bytes: u64,
    /// Bytes a maintenance command would return to the filesystem.
    pub reclaimable: u64,
    /// Measured detail, when a probe produced any.
    pub detail: Option<String>,
}

impl DbEntry {
    pub fn guidance(&self) -> super::Guidance {
        super::guidance(self.kind, self.role)
    }
}

/// Rows for the databases view, largest first.
pub fn summarize(tree: &Tree) -> Vec<DbEntry> {
    let mut out = Vec::new();
    for probe in &tree.probes {
        match &probe.detail {
            ProbeDetail::Postgres(cluster) => postgres_entries(tree, probe.node, cluster, &mut out),
            ProbeDetail::Mysql(server) => mysql_entries(tree, probe.node, server, &mut out),
            ProbeDetail::SqlServer(inst) => sqlserver_entries(tree, probe.node, inst, &mut out),
            ProbeDetail::Sqlite(header) => sqlite_entries(tree, probe.node, header, &mut out),
        }
    }
    out.sort_by(|a, b| {
        b.bytes
            .cmp(&a.bytes)
            .then_with(|| a.label.cmp(&b.label))
    });
    out
}

fn postgres_entries(
    tree: &Tree,
    root: usize,
    cluster: &postgres::Cluster,
    out: &mut Vec<DbEntry>,
) {
    let anchors = role_anchors(tree, root);
    let base = tree.nodes[root].path.display().to_string();

    let rows = [
        (Role::Data, cluster.data_bytes, 0u64, None),
        (
            Role::Wal,
            cluster.wal_bytes,
            0,
            Some(cluster.wal_summary()),
        ),
        (
            Role::TempSpill,
            cluster.temp_bytes,
            cluster.temp_bytes,
            None,
        ),
        (Role::Log, cluster.log_bytes, cluster.log_bytes, None),
    ];

    for (role, bytes, reclaimable, detail) in rows {
        if bytes == 0 {
            continue;
        }
        let node = anchors.get(&role).copied().unwrap_or(root);
        let label = if node == root {
            format!("{base}  ({role})")
        } else {
            tree.nodes[node].path.display().to_string()
        };
        out.push(DbEntry {
            node,
            kind: AppKind::Postgres,
            role,
            label,
            bytes,
            reclaimable,
            detail,
        });
    }
}

fn mysql_entries(tree: &Tree, root: usize, server: &mysql::Server, out: &mut Vec<DbEntry>) {
    let anchors = role_anchors(tree, root);
    let base = tree.nodes[root].path.display().to_string();

    let binlog_detail = if server.binlogs_look_unpurged() {
        Some(format!(
            "{} — no expiry policy is taking effect",
            server.binlog_summary()
        ))
    } else if server.binlog_bytes > 0 {
        Some(server.binlog_summary())
    } else {
        None
    };

    let rows = [
        (Role::Data, server.data_bytes, 0u64, None),
        (Role::Wal, server.wal_bytes, 0, None),
        (
            Role::Binlog,
            server.binlog_bytes,
            server.binlog_bytes,
            binlog_detail,
        ),
        (
            Role::TempSpill,
            server.temp_bytes,
            server.temp_bytes,
            Some("recreated at the next restart".to_string()),
        ),
        (Role::Log, server.log_bytes, server.log_bytes, None),
        (Role::Backup, server.backup_bytes, server.backup_bytes, None),
    ];

    for (role, bytes, reclaimable, detail) in rows {
        if bytes == 0 {
            continue;
        }
        let node = anchors.get(&role).copied().unwrap_or(root);
        let label = if node == root {
            format!("{base}  ({role})")
        } else {
            tree.nodes[node].path.display().to_string()
        };
        out.push(DbEntry {
            node,
            kind: AppKind::Mysql,
            role,
            label,
            bytes,
            reclaimable,
            detail,
        });
    }
}

/// SQL Server gets a row per transaction log rather than one rolled-up WAL
/// line: a runaway `.ldf` belongs to one database, and naming it is most of
/// the value.
fn sqlserver_entries(
    tree: &Tree,
    dir: usize,
    inst: &sqlserver::Instance,
    out: &mut Vec<DbEntry>,
) {
    let dir_path = tree.nodes[dir].path.display().to_string();

    for &c in &tree.nodes[dir].children {
        let Some(tag) = tree.nodes[c].app else { continue };
        if tag.kind != AppKind::SqlServer || tag.role != Role::Wal {
            continue;
        }
        let name = &tree.nodes[c].name;
        let stem = name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name);
        let data_bytes = sibling_data_bytes(tree, dir, sqlserver::log_base(stem));
        let bytes = tree.nodes[c].own_used;
        out.push(DbEntry {
            node: c,
            kind: AppKind::SqlServer,
            role: Role::Wal,
            label: tree.nodes[c].path.display().to_string(),
            bytes,
            reclaimable: 0,
            detail: sqlserver::log_ratio_note(bytes, data_bytes),
        });
    }

    let rows = [
        (Role::Data, inst.data_bytes, 0u64),
        (Role::TempSpill, inst.temp_bytes, 0),
        (Role::Backup, inst.backup_bytes, inst.backup_bytes),
    ];
    for (role, bytes, reclaimable) in rows {
        if bytes == 0 {
            continue;
        }
        out.push(DbEntry {
            node: dir,
            kind: AppKind::SqlServer,
            role,
            label: format!("{dir_path}  ({role})"),
            bytes,
            reclaimable,
            detail: None,
        });
    }
}

/// Size of the `.mdf` / `.ndf` a transaction log belongs to, or 0.
fn sibling_data_bytes(tree: &Tree, dir: usize, base: &str) -> u64 {
    for &c in &tree.nodes[dir].children {
        let n = &tree.nodes[c];
        if n.is_dir {
            continue;
        }
        let Some((stem, ext)) = n.name.rsplit_once('.') else {
            continue;
        };
        if stem.eq_ignore_ascii_case(base)
            && matches!(ext.to_ascii_lowercase().as_str(), "mdf" | "ndf")
        {
            return n.own_used;
        }
    }
    0
}

/// First node of each role inside a cluster, for drill-in from the view.
fn role_anchors(tree: &Tree, root: usize) -> std::collections::BTreeMap<Role, usize> {
    let mut anchors = std::collections::BTreeMap::new();
    let mut queue = vec![root];
    let mut cursor = 0usize;
    while cursor < queue.len() {
        let id = queue[cursor];
        cursor += 1;
        for &c in &tree.nodes[id].children {
            if let Some(tag) = tree.nodes[c].app {
                anchors.entry(tag.role).or_insert(c);
            }
            if tree.nodes[c].is_dir {
                queue.push(c);
            }
        }
    }
    anchors
}

fn sqlite_entries(tree: &Tree, db: usize, header: &sqlite::Header, out: &mut Vec<DbEntry>) {
    let n = &tree.nodes[db];
    out.push(DbEntry {
        node: db,
        kind: AppKind::Sqlite,
        role: Role::Data,
        label: n.path.display().to_string(),
        bytes: n.own_used,
        reclaimable: header.freelist_bytes(),
        detail: Some(header.summary()),
    });

    // A write-ahead log bigger than its database means a checkpoint has not
    // run — worth its own line, because the fix is different.
    let Some(parent) = n.parent else { return };
    let base = n.name.clone();
    let db_bytes = n.own_used;
    for &c in &tree.nodes[parent].children {
        let sib = &tree.nodes[c];
        let Some(tag) = sib.app else { continue };
        if tag.kind != AppKind::Sqlite || tag.role != Role::Wal {
            continue;
        }
        if sqlite::sidecar_of(&sib.name).map(|(b, _)| b) != Some(base.as_str()) {
            continue;
        }
        out.push(DbEntry {
            node: c,
            kind: AppKind::Sqlite,
            role: Role::Wal,
            label: sib.path.display().to_string(),
            bytes: sib.own_used,
            reclaimable: sib.own_used,
            detail: Some(if sib.own_used > db_bytes {
                format!("larger than the database ({})", human_bytes(db_bytes))
            } else {
                "checkpoint pending".to_string()
            }),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delete::{Collector, CollectorItem};
    use crate::scan::{scan, WalkOptions};
    use std::fs;
    use std::path::Path;
    use tempfile::TempDir;

    /// A data directory with one of everything that matters.
    fn make_cluster(root: &Path) {
        fs::create_dir_all(root.join("base").join("16384")).unwrap();
        fs::create_dir_all(root.join("base").join("pgsql_tmp")).unwrap();
        fs::create_dir_all(root.join("global")).unwrap();
        fs::create_dir_all(root.join("pg_wal")).unwrap();
        fs::create_dir_all(root.join("pg_xact")).unwrap();
        fs::create_dir_all(root.join("log")).unwrap();

        fs::write(root.join("PG_VERSION"), "16\n").unwrap();
        fs::write(root.join("postgresql.conf"), vec![b'#'; 128]).unwrap();
        fs::write(root.join("base").join("16384").join("2836"), vec![0u8; 4096]).unwrap();
        fs::write(root.join("global").join("1262"), vec![0u8; 1024]).unwrap();
        fs::write(
            root.join("pg_wal").join("000000010000000000000001"),
            vec![0u8; 2048],
        )
        .unwrap();
        fs::write(
            root.join("base").join("pgsql_tmp").join("pgsql_tmp99.0"),
            vec![0u8; 8192],
        )
        .unwrap();
        fs::write(root.join("log").join("postgresql.log"), vec![0u8; 512]).unwrap();
    }

    fn node_at<'a>(tree: &'a crate::scan::Tree, suffix: &str) -> &'a crate::scan::Node {
        tree.nodes
            .iter()
            .find(|n| n.path.ends_with(suffix))
            .unwrap_or_else(|| panic!("no node for {suffix}"))
    }

    /// Minimal valid SQLite header padded out to `pages` pages.
    fn sqlite_file(page_size: u16, pages: u32, freelist: u32) -> Vec<u8> {
        let mut b = vec![0u8; 100];
        b[..16].copy_from_slice(b"SQLite format 3\0");
        b[16..18].copy_from_slice(&page_size.to_be_bytes());
        b[24..28].copy_from_slice(&1u32.to_be_bytes());
        b[28..32].copy_from_slice(&pages.to_be_bytes());
        b[36..40].copy_from_slice(&freelist.to_be_bytes());
        b[92..96].copy_from_slice(&1u32.to_be_bytes());
        b.resize(page_size as usize * pages as usize, 0);
        b
    }

    #[test]
    fn detects_a_cluster_and_roles_its_files() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("pgdata");
        make_cluster(&data);
        let tree = scan(&data, WalkOptions::default()).unwrap();

        let tag = |suffix: &str| node_at(&tree, suffix).app.expect(suffix);
        assert_eq!(tag("base").kind, AppKind::Postgres);
        assert_eq!(tag("base").role, Role::Data);
        assert_eq!(tag("base/16384/2836").role, Role::Data, "relation file");
        assert_eq!(tag("global/1262").role, Role::Data);
        assert_eq!(tag("pg_wal").role, Role::Wal);
        assert_eq!(tag("pg_wal/000000010000000000000001").role, Role::Wal);
        assert_eq!(tag("log/postgresql.log").role, Role::Log);
        assert_eq!(tag("pg_xact").role, Role::Meta);
        assert_eq!(tag("postgresql.conf").role, Role::Meta);

        // Spill is nested two levels under `base/`, and must not inherit Data.
        assert_eq!(tag("base/pgsql_tmp").role, Role::TempSpill);
        assert_eq!(tag("base/pgsql_tmp/pgsql_tmp99.0").role, Role::TempSpill);
    }

    #[test]
    fn cluster_probe_totals_by_role_and_reads_the_version() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("pgdata");
        make_cluster(&data);
        let tree = scan(&data, WalkOptions::default()).unwrap();

        assert_eq!(tree.probes.len(), 1, "one cluster");
        let ProbeDetail::Postgres(cluster) = &tree.probes[0].detail else {
            panic!("expected a postgres probe");
        };
        assert_eq!(cluster.version.as_deref(), Some("16"));
        assert_eq!(cluster.wal_segments, 1);
        assert!(cluster.data_bytes > 0);
        assert!(cluster.temp_bytes > 0);
        assert!(cluster.log_bytes > 0);
        // The headline claim: what is reclaimable excludes table data entirely.
        assert!(cluster.reclaimable_bytes() < cluster.data_bytes + cluster.wal_bytes);
    }

    #[test]
    fn structural_detection_needs_no_file_reads() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("pgdata");
        make_cluster(&data);
        let opts = WalkOptions {
            apps: Options::structural_only(),
            ..WalkOptions::default()
        };
        let tree = scan(&data, opts).unwrap();

        // Roles and protection survive with probing off — only the measured
        // extras go away.
        assert_eq!(node_at(&tree, "base").app.unwrap().role, Role::Data);
        assert!(node_at(&tree, "base").guard.is_some());
        let ProbeDetail::Postgres(cluster) = &tree.probes[0].detail else {
            panic!("expected a postgres probe");
        };
        assert_eq!(cluster.version, None, "PG_VERSION is a content read");
    }

    #[test]
    fn protection_reaches_the_parent_of_a_cluster() {
        // The failure this exists to prevent: `base/` is protected, but the
        // grandparent nobody tagged is what a user actually selects.
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("var").join("lib").join("postgresql");
        make_cluster(&data);
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();

        for suffix in ["var", "var/lib", "var/lib/postgresql", "base", "global"] {
            assert!(
                node_at(&tree, suffix).guard.is_some(),
                "{suffix} must be guarded"
            );
        }
        // Spill is inside the cluster and still freely markable.
        assert!(
            node_at(&tree, "base/pgsql_tmp").guard.is_none(),
            "query spill is ordinary waste"
        );
    }

    #[test]
    fn the_collector_refuses_a_guarded_path() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("pgdata");
        make_cluster(&data);
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();

        let mut collector = Collector::new();
        let guarded = node_at(&tree, "pgdata");
        let err = collector
            .mark(CollectorItem {
                path: guarded.path.clone(),
                is_dir: true,
                size_bytes: guarded.used,
                category: guarded.category,
                node_id: 0,
                guard: guarded.guard,
            })
            .expect_err("marking a cluster must be refused");
        assert!(err.reason.contains("PostgreSQL"), "{}", err.reason);
        assert!(collector.is_empty());

        // And the spill directory next to it still goes in.
        let spill = node_at(&tree, "base/pgsql_tmp");
        collector
            .mark(CollectorItem {
                path: spill.path.clone(),
                is_dir: true,
                size_bytes: spill.used,
                category: spill.category,
                node_id: 1,
                guard: spill.guard,
            })
            .expect("spill is reclaimable");
        assert_eq!(collector.len(), 1);
    }

    #[test]
    fn finds_sqlite_by_header_not_by_extension() {
        let tmp = TempDir::new().unwrap();
        // No extension at all — Chrome and Firefox both ship databases this way.
        let db = tmp.path().join("History");
        fs::write(&db, sqlite_file(4096, 400, 152)).unwrap();
        // Looks like a database, is not one.
        fs::write(tmp.path().join("cache.db"), vec![b'z'; 400 * 4096]).unwrap();

        let opts = WalkOptions {
            apps: Options {
                min_probe_bytes: 0,
                ..Options::default()
            },
            ..WalkOptions::default()
        };
        let tree = scan(tmp.path(), opts).unwrap();

        assert_eq!(node_at(&tree, "History").app.unwrap().kind, AppKind::Sqlite);
        assert!(node_at(&tree, "cache.db").app.is_none(), "decoy");

        let ProbeDetail::Sqlite(header) = &tree.probes[0].detail else {
            panic!("expected a sqlite probe");
        };
        assert_eq!(header.freelist_bytes(), 152 * 4096);
        assert_eq!(tree.probes[0].reclaimable_bytes(), 152 * 4096);
    }

    #[test]
    fn a_large_wal_promotes_its_small_database_for_probing() {
        // The actionable shape: a 4 KB database with a 2 MB write-ahead log
        // that never got checkpointed. The database is under the probe floor,
        // so it is only found by way of its sidecar.
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("app.db"), sqlite_file(4096, 1, 0)).unwrap();
        fs::write(tmp.path().join("app.db-wal"), vec![0u8; 2 << 20]).unwrap();

        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();

        assert_eq!(node_at(&tree, "app.db").app.unwrap().role, Role::Data);
        let wal = node_at(&tree, "app.db-wal").app.expect("wal tagged");
        assert_eq!(wal.role, Role::Wal);
        assert_eq!(wal.kind, AppKind::Sqlite);

        let entries = summarize(&tree);
        assert!(
            entries.iter().any(|e| e.role == Role::Wal && e.reclaimable > 0),
            "the WAL is the row worth showing: {entries:?}"
        );
    }

    #[test]
    fn sqlite_protects_nothing_and_does_not_guard_its_directory() {
        let tmp = TempDir::new().unwrap();
        let cache = tmp.path().join("cache");
        fs::create_dir(&cache).unwrap();
        fs::write(cache.join("app.db"), sqlite_file(4096, 300, 100)).unwrap();
        fs::write(cache.join("app.db-shm"), vec![0u8; 32768]).unwrap();
        fs::write(cache.join("app.db-wal"), vec![0u8; 65536]).unwrap();

        let opts = WalkOptions {
            apps: Options {
                min_probe_bytes: 0,
                ..Options::default()
            },
            ..WalkOptions::default()
        };
        let tree = scan(tmp.path(), opts).unwrap();

        assert_eq!(node_at(&tree, "cache/app.db").app.unwrap().role, Role::Data);
        assert_eq!(
            node_at(&tree, "cache/app.db-wal").app.unwrap().role,
            Role::Wal
        );
        assert_eq!(
            node_at(&tree, "cache/app.db-shm").app.unwrap().role,
            Role::Meta
        );

        // A lone database is a file, not a service. Deleting a stale cache
        // is something people legitimately point rings at, and protecting
        // the sidecars would make the whole cache directory unmarkable.
        for suffix in [
            "cache/app.db",
            "cache/app.db-wal",
            "cache/app.db-shm",
            "cache",
        ] {
            assert!(node_at(&tree, suffix).guard.is_none(), "{suffix}");
        }
    }

    /// A MySQL or MariaDB data directory with the usual furniture.
    fn make_mysql(root: &Path, mariadb: bool) {
        fs::create_dir_all(root.join("mysql")).unwrap();
        fs::create_dir_all(root.join("wordpress")).unwrap();
        fs::write(root.join("ibdata1"), vec![0u8; 65_536]).unwrap();
        fs::write(root.join("ib_logfile0"), vec![0u8; 16_384]).unwrap();
        fs::write(root.join("ibtmp1"), vec![0u8; 12_288]).unwrap();
        fs::write(root.join("auto.cnf"), b"[auto]\n").unwrap();
        fs::write(root.join("db01.err"), vec![b'e'; 2048]).unwrap();
        fs::write(
            root.join("wordpress").join("wp_posts.ibd"),
            vec![0u8; 32_768],
        )
        .unwrap();
        for i in 1..=3u32 {
            fs::write(root.join(format!("mysql-bin.{i:06}")), vec![0u8; 40_960]).unwrap();
        }
        fs::write(root.join("mysql-bin.index"), b"./mysql-bin.000001\n").unwrap();
        if mariadb {
            fs::write(root.join("aria_log_control"), vec![0u8; 64]).unwrap();
        }
    }

    #[test]
    fn detects_a_datadir_and_roles_its_files() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("mysql-data");
        make_mysql(&data, false);
        let tree = scan(&data, WalkOptions::default()).unwrap();

        let tag = |suffix: &str| node_at(&tree, suffix).app.expect(suffix);
        assert_eq!(tag("ibdata1").kind, AppKind::Mysql);
        assert_eq!(tag("ibdata1").role, Role::Data);
        assert_eq!(tag("ib_logfile0").role, Role::Wal);
        assert_eq!(tag("ibtmp1").role, Role::TempSpill);
        assert_eq!(tag("mysql-bin.000001").role, Role::Binlog);
        assert_eq!(tag("mysql-bin.index").role, Role::Binlog);
        assert_eq!(tag("db01.err").role, Role::Log);
        assert_eq!(tag("auto.cnf").role, Role::Meta);
        // Schema directories, and files inside them, are data.
        assert_eq!(tag("wordpress").role, Role::Data);
        assert_eq!(tag("wordpress/wp_posts.ibd").role, Role::Data);
    }

    #[test]
    fn tells_mariadb_from_mysql_and_counts_binary_logs() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("maria");
        make_mysql(&data, true);
        let tree = scan(&data, WalkOptions::default()).unwrap();

        let ProbeDetail::Mysql(server) = &tree.probes[0].detail else {
            panic!("expected a mysql probe");
        };
        assert_eq!(server.flavor, "MariaDB", "aria_log_control is the tell");
        assert_eq!(server.binlog_files, 4, "three segments plus the index");
        assert!(server.binlog_bytes > 0);
        // Binary logs, ibtmp1 and the error log — never the tables.
        assert!(server.reclaimable_bytes() > 0);
        assert!(server.reclaimable_bytes() < server.data_bytes + server.binlog_bytes + 1);
    }

    #[test]
    fn binary_logs_are_refused_but_the_error_log_is_not() {
        let tmp = TempDir::new().unwrap();
        make_mysql(&tmp.path().join("mysql-data"), false);
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();

        // Deleting a binlog by hand leaves the .index describing files that
        // are gone, and breaks every replica.
        assert!(node_at(&tree, "mysql-bin.000001").guard.is_some());
        assert!(node_at(&tree, "ibdata1").guard.is_some());
        assert!(node_at(&tree, "ibtmp1").guard.is_some());
        // The datadir's parent inherits the refusal.
        assert!(node_at(&tree, "mysql-data").guard.is_some());
        // Rotatable output does not.
        assert!(node_at(&tree, "db01.err").guard.is_none());
    }

    #[test]
    fn sql_server_flags_a_log_that_outgrew_its_data_file() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("MSSQL").join("DATA");
        fs::create_dir_all(&data).unwrap();
        fs::write(data.join("orders.mdf"), vec![0u8; 65_536]).unwrap();
        fs::write(data.join("orders_log.ldf"), vec![0u8; 262_144]).unwrap();
        fs::write(data.join("tempdb.mdf"), vec![0u8; 8192]).unwrap();
        fs::write(data.join("orders_full.bak"), vec![0u8; 32_768]).unwrap();
        fs::write(data.join("readme.txt"), vec![b'x'; 16]).unwrap();

        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();

        let tag = |suffix: &str| node_at(&tree, suffix).app.expect(suffix);
        assert_eq!(tag("orders.mdf").role, Role::Data);
        assert_eq!(tag("orders_log.ldf").role, Role::Wal);
        assert_eq!(tag("tempdb.mdf").role, Role::TempSpill);
        // `.bak` is claimed only because a data file sits beside it.
        assert_eq!(tag("orders_full.bak").role, Role::Backup);
        assert!(node_at(&tree, "readme.txt").app.is_none());

        assert!(node_at(&tree, "orders.mdf").guard.is_some());
        assert!(node_at(&tree, "orders_log.ldf").guard.is_some());
        assert!(node_at(&tree, "orders_full.bak").guard.is_none());
        // Files scattered without a data directory still protect the folder
        // they landed in, and the one above it.
        assert!(node_at(&tree, "MSSQL/DATA").guard.is_some());
        assert!(node_at(&tree, "MSSQL").guard.is_some());

        let entries = summarize(&tree);
        let log = entries
            .iter()
            .find(|e| e.kind == AppKind::SqlServer && e.role == Role::Wal)
            .expect("a row for the transaction log");
        let detail = log.detail.as_deref().unwrap_or_default();
        assert!(
            detail.contains("not being truncated"),
            "the runaway log must say why: {detail:?}"
        );
        assert_eq!(log.reclaimable, 0, "rings does not guess a log's right size");
    }

    #[test]
    fn a_stray_bak_file_is_not_claimed_on_its_own() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("notes.bak"), vec![0u8; 4096]).unwrap();
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();
        assert!(node_at(&tree, "notes.bak").app.is_none());
        assert!(tree.probes.is_empty());
    }

    #[test]
    fn summarize_orders_largest_first_and_never_offers_table_data() {
        let tmp = TempDir::new().unwrap();
        let data = tmp.path().join("pgdata");
        make_cluster(&data);
        let tree = scan(&data, WalkOptions::default()).unwrap();

        let entries = summarize(&tree);
        assert!(!entries.is_empty());
        for pair in entries.windows(2) {
            assert!(pair[0].bytes >= pair[1].bytes, "largest first");
        }
        for entry in &entries {
            if entry.role == Role::Data || entry.role == Role::Wal {
                assert_eq!(
                    entry.reclaimable, 0,
                    "{} must not advertise reclaimable bytes",
                    entry.label
                );
            }
        }
    }

    #[test]
    fn a_scan_with_no_databases_costs_nothing() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("src")).unwrap();
        fs::write(tmp.path().join("src").join("main.rs"), vec![b'x'; 4096]).unwrap();

        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();
        assert!(tree.probes.is_empty());
        assert!(tree.nodes.iter().all(|n| n.app.is_none()));
        assert!(tree.nodes.iter().all(|n| n.guard.is_none()));
        assert!(summarize(&tree).is_empty());
    }
}
