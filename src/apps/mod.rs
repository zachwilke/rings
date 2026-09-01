//! Application awareness: recognise what an application's files *do*, so a
//! large directory reads as "PostgreSQL table data" instead of an opaque
//! 31 GB, and so rings can say how to reclaim the space rather than offer to
//! unlink something load-bearing.
//!
//! Two tiers, both cheap:
//!
//!   * **structural** — marker files and directory layout decide the
//!     application and every file's role. No I/O beyond the walk that has
//!     already happened.
//!   * **probe** — a bounded header read (at most [`PROBE_BYTES`]) for
//!     formats that describe their own free space. SQLite's 100-byte header
//!     gives the exact number of bytes `VACUUM` would hand back.
//!
//! Nothing here ever contacts a database server or reads a row. That keeps
//! the analysis working on the volumes where a disk tool is most useful:
//! backup mounts, snapshots, detached disks, and stopped containers.

pub mod detect;
pub mod mysql;
pub mod postgres;
pub mod sqlite;
pub mod sqlserver;

pub use detect::{annotate, summarize, DbEntry, Options};

/// An application whose on-disk layout rings understands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AppKind {
    Sqlite,
    Postgres,
    /// MySQL and MariaDB share a data directory layout; the flavour is
    /// reported in the probe detail rather than split into two kinds,
    /// because every piece of advice below is identical for both.
    Mysql,
    SqlServer,
}

impl AppKind {
    /// Stable machine-readable tag. Goes to CSV and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            AppKind::Sqlite => "sqlite",
            AppKind::Postgres => "postgres",
            AppKind::Mysql => "mysql",
            AppKind::SqlServer => "sqlserver",
        }
    }

    /// How the engine is spelled on screen.
    pub fn label(self) -> &'static str {
        match self {
            AppKind::Sqlite => "SQLite",
            AppKind::Postgres => "PostgreSQL",
            AppKind::Mysql => "MySQL/MariaDB",
            AppKind::SqlServer => "SQL Server",
        }
    }

    /// Whether protection travels up to enclosing directories.
    ///
    /// A server's data directory is jointly load-bearing: `rm -rf` on the
    /// directory *above* a PostgreSQL cluster destroys it just as thoroughly
    /// as deleting `base/`, so the refusal has to reach the parent a person
    /// actually selects.
    ///
    /// A lone SQLite file is not like that. Propagating from a `-shm` file
    /// would make `~/.cache` undeletable, which would break the tool's main
    /// job to protect something the operator is entitled to throw away.
    pub fn propagates_guard(self) -> bool {
        match self {
            AppKind::Sqlite => false,
            AppKind::Postgres | AppKind::Mysql | AppKind::SqlServer => true,
        }
    }
}

impl std::fmt::Display for AppKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a file does inside its application. The role, not the extension,
/// decides whether the space is reclaimable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Role {
    /// Live table / heap / index data. Unlinking it destroys the database.
    Data,
    /// Crash-recovery log: WAL, InnoDB redo, SQL Server `.ldf`.
    Wal,
    /// Replication / point-in-time log: MySQL binary and relay logs. Kept
    /// apart from [`Role::Wal`] because the remedy is completely different —
    /// these are purged on a retention policy, and they are the single most
    /// common reason a MySQL server fills its disk.
    Binlog,
    /// Query spill: sort and hash overflow written under memory pressure,
    /// and engine scratch tablespaces.
    TempSpill,
    /// Server log output.
    Log,
    /// Database backups sitting on the same volume as the data.
    Backup,
    /// Version stamps, config, pid files, sockets, transaction status.
    Meta,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Data => "data",
            Role::Wal => "wal",
            Role::Binlog => "binlog",
            Role::TempSpill => "spill",
            Role::Log => "log",
            Role::Backup => "backup",
            Role::Meta => "meta",
        }
    }

    /// Every role, for exhaustiveness tests.
    pub const ALL: &'static [Role] = &[
        Role::Data,
        Role::Wal,
        Role::Binlog,
        Role::TempSpill,
        Role::Log,
        Role::Backup,
        Role::Meta,
    ];
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How the space comes back — which is *not* the same question as whether
/// rings may delete the file. `pg_wal` is recoverable by configuration and
/// must never be unlinked; a stale SQLite cache is recoverable by `VACUUM`
/// and may be thrown away wholesale. Keeping these on one axis is what let
/// an earlier version print "deleting segments corrupts the cluster" on a
/// row it would then happily queue for deletion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reclaim {
    /// Structural. The space is not recoverable; the files are the database.
    Never,
    /// Recoverable by running something, not by removing the file.
    Command,
    /// Ordinary waste: removing the file is the correct move.
    Safe,
}

impl Reclaim {
    /// Stable machine-readable tag for CSV and JSON.
    pub fn as_str(self) -> &'static str {
        match self {
            Reclaim::Never => "never",
            Reclaim::Command => "command",
            Reclaim::Safe => "safe",
        }
    }
}

/// Static advice for one (application, role) pair. Held as `&'static str` so
/// annotating a million-node tree costs no allocation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Guidance {
    pub reclaim: Reclaim,
    /// True when rings must refuse to delete the file. Independent of
    /// `reclaim`: see the note on [`Reclaim`].
    pub protected: bool,
    /// Why the space is here.
    pub why: &'static str,
    /// What to do about it.
    pub action: &'static str,
}

impl Guidance {
    /// How urgently the row should read. Protection outranks everything:
    /// a file you cannot delete is the strongest thing to say about it.
    pub fn tone(&self) -> Tone {
        if self.protected {
            Tone::Protected
        } else if self.reclaim == Reclaim::Safe {
            Tone::Reclaimable
        } else {
            Tone::Advisory
        }
    }
}

/// Display weight for a row: drives both colour and glyph.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
    /// Cannot be deleted by rings.
    Protected,
    /// Deletable, but a command is the better move.
    Advisory,
    /// Ordinary waste.
    Reclaimable,
}

/// Advice for a role. Total by construction: each engine module answers for
/// every role, so a role added later cannot silently default to something
/// permissive.
pub fn guidance(kind: AppKind, role: Role) -> Guidance {
    match kind {
        AppKind::Sqlite => sqlite::guidance(role),
        AppKind::Postgres => postgres::guidance(role),
        AppKind::Mysql => mysql::guidance(role),
        AppKind::SqlServer => sqlserver::guidance(role),
    }
}

/// Two bytes on every [`crate::scan::Node`]: which application owns the file
/// and what it does there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppTag {
    pub kind: AppKind,
    pub role: Role,
}

impl AppTag {
    pub fn new(kind: AppKind, role: Role) -> Self {
        Self { kind, role }
    }

    pub fn guidance(self) -> Guidance {
        guidance(self.kind, self.role)
    }

    pub fn reclaim(self) -> Reclaim {
        self.guidance().reclaim
    }

    /// True when unlinking the file would destroy data or break a running
    /// service. Drives the delete safeguard, so it stays conservative.
    pub fn is_protected(self) -> bool {
        self.guidance().protected
    }

    /// One-line refusal shown when a protected path is marked.
    pub fn refusal(self) -> String {
        let g = self.guidance();
        format!("{} {} — {}", self.kind.label(), g.why, g.action)
    }
}

/// Result of reading a file's own header, or of totalling a server's data
/// directory. Sparse: only recognised nodes appear, so a scan with no
/// databases pays nothing.
#[derive(Clone, Debug)]
pub struct Probe {
    /// Node this describes.
    pub node: usize,
    pub kind: AppKind,
    pub detail: ProbeDetail,
}

#[derive(Clone, Debug)]
pub enum ProbeDetail {
    Sqlite(sqlite::Header),
    Postgres(postgres::Cluster),
    Mysql(mysql::Server),
    SqlServer(sqlserver::Instance),
}

impl Probe {
    /// Bytes a maintenance command would return to the filesystem.
    pub fn reclaimable_bytes(&self) -> u64 {
        match &self.detail {
            ProbeDetail::Sqlite(h) => h.freelist_bytes(),
            ProbeDetail::Postgres(c) => c.reclaimable_bytes(),
            ProbeDetail::Mysql(s) => s.reclaimable_bytes(),
            ProbeDetail::SqlServer(i) => i.reclaimable_bytes(),
        }
    }
}

/// Header bytes read per probed file. One page on every platform rings runs on.
pub const PROBE_BYTES: usize = 4096;

/// Files below this are not probed: a database small enough to be dust is
/// not the reason a disk is full.
pub const PROBE_MIN_BYTES: u64 = 1 << 20;

/// Ceiling on probes per scan, so a directory of a million candidate files
/// cannot turn a walk into a read storm.
pub const PROBE_MAX_FILES: usize = 4096;

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: &[AppKind] = &[
        AppKind::Sqlite,
        AppKind::Postgres,
        AppKind::Mysql,
        AppKind::SqlServer,
    ];

    #[test]
    fn every_engine_answers_for_every_role() {
        // Catches a role added without advice, which would otherwise fall
        // through to whatever the last match arm happened to be.
        for &kind in KINDS {
            for &role in Role::ALL {
                let g = guidance(kind, role);
                assert!(!g.why.is_empty(), "{kind:?}/{role:?} has no why");
                assert!(!g.action.is_empty(), "{kind:?}/{role:?} has no action");
            }
        }
    }

    #[test]
    fn nothing_that_breaks_a_running_server_is_deletable() {
        // The property the whole safeguard rests on: for every server
        // engine, live data and its logs are refused.
        for &kind in &[AppKind::Postgres, AppKind::Mysql, AppKind::SqlServer] {
            for &role in &[Role::Data, Role::Wal, Role::Binlog, Role::Meta] {
                assert!(
                    guidance(kind, role).protected,
                    "{kind:?}/{role:?} must be refused"
                );
            }
        }
    }

    #[test]
    fn protection_and_reclaim_are_separate_axes() {
        // The bug this split exists to prevent: `pg_wal` is recoverable by
        // configuration *and* must never be unlinked. One field cannot say
        // both.
        let wal = guidance(AppKind::Postgres, Role::Wal);
        assert_eq!(wal.reclaim, Reclaim::Command);
        assert!(wal.protected, "advice said corrupting, code allowed it");
        assert!(wal.action.contains("max_wal_size"));

        // And the mirror case: a stale SQLite cache is reclaimed by VACUUM
        // but the operator may still decide the file itself is disposable.
        let db = guidance(AppKind::Sqlite, Role::Data);
        assert_eq!(db.reclaim, Reclaim::Command);
        assert!(!db.protected, "a disposable cache database stays markable");
    }

    #[test]
    fn spill_logs_and_backups_stay_reclaimable() {
        for &kind in KINDS {
            for &role in &[Role::TempSpill, Role::Log, Role::Backup] {
                let g = guidance(kind, role);
                if g.reclaim == Reclaim::Safe {
                    assert!(
                        !g.protected,
                        "{kind:?}/{role:?} is safe to remove but refused"
                    );
                }
            }
        }
    }

    #[test]
    fn only_server_layouts_protect_their_parents() {
        assert!(AppKind::Postgres.propagates_guard());
        assert!(AppKind::Mysql.propagates_guard());
        assert!(AppKind::SqlServer.propagates_guard());
        // Otherwise a `-shm` file would make the whole cache tree undeletable.
        assert!(!AppKind::Sqlite.propagates_guard());
    }

    #[test]
    fn tone_puts_protection_first() {
        assert_eq!(
            guidance(AppKind::Postgres, Role::Data).tone(),
            Tone::Protected
        );
        assert_eq!(
            guidance(AppKind::Postgres, Role::TempSpill).tone(),
            Tone::Reclaimable
        );
        assert_eq!(guidance(AppKind::Sqlite, Role::Data).tone(), Tone::Advisory);
    }
}
