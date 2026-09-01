//! PostgreSQL cluster recognition from directory layout alone.
//!
//! A data directory is identified by three things existing side by side:
//! `PG_VERSION`, `base/`, and `global/`. That test is layout-based, so it
//! finds a cluster wherever the packager put it — Debian's
//! `/var/lib/postgresql/<ver>/<cluster>`, RHEL's `/var/lib/pgsql/<ver>/data`,
//! the Docker image's `/var/lib/postgresql/data`, a tablespace on a second
//! disk, or a `pg_basebackup` sitting on a backup mount — without rings
//! hard-coding a single one of those paths.
//!
//! Once a cluster is found, the top-level directory a file sits under decides
//! its role, and the role decides whether the space is reclaimable. The
//! important distinction this draws: `base/` is 31 GB you must never touch,
//! `pg_wal/` is 1 GB that is bounded by configuration rather than garbage,
//! and `pgsql_tmp/` is spill that says `work_mem` is set too low.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use super::{Guidance, Reclaim, Role};
use crate::size::human_bytes;

/// A cluster is a service, and every file in the data directory is part of
/// it. Data, WAL, and metadata are all refused; only spill and logs are
/// ordinary waste.
pub fn guidance(role: Role) -> Guidance {
    match role {
        Role::Data => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "live table, index, and catalog data",
            action: "never delete \u{2014} reclaim inside the server with VACUUM, or pg_repack to return space to the disk",
        },
        Role::Wal => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "write-ahead log segments, needed for crash recovery and replicas",
            action: "never delete \u{2014} bounded by max_wal_size; if it keeps growing, check for a stale replication slot or a failing archive_command",
        },
        Role::TempSpill => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "query spill files: a sort or hash exceeded work_mem",
            action: "cleared at restart \u{2014} raise work_mem to stop the churn",
        },
        Role::Log => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "server log output",
            action: "safe to rotate or remove \u{2014} tune log_rotation_age and log_rotation_size",
        },
        Role::Backup => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "base backup or dump kept beside the cluster",
            action: "safe to move off this volume once you have confirmed another copy exists",
        },
        Role::Binlog => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "replication artefact inside the data directory",
            action: "never delete \u{2014} managed by the server, not by hand",
        },
        Role::Meta => Guidance {
            reclaim: Reclaim::Never,
            protected: true,
            why: "cluster metadata: version stamp, config, transaction status",
            action: "never delete \u{2014} tiny, and the cluster will not start without it",
        },
    }
}

/// Default WAL segment size. Configurable at `initdb --wal-segsize`, so it is
/// used only to explain a segment count, never to compute one.
pub const WAL_SEGMENT_BYTES: u64 = 16 * 1024 * 1024;

/// Files whose presence together marks a data directory.
pub const MARKER_VERSION: &str = "PG_VERSION";
pub const MARKER_BASE: &str = "base";
pub const MARKER_GLOBAL: &str = "global";

/// Directory that holds query spill, at any depth inside a cluster.
pub const SPILL_DIR: &str = "pgsql_tmp";

/// Role for a top-level entry of the data directory.
///
/// Unknown entries resolve to [`Role::Meta`], which is protected. Defaulting
/// an unrecognised file inside a live cluster to "do not delete" is the only
/// safe direction to be wrong in.
pub fn top_role(name: &str) -> Role {
    match name {
        // Live relation data, cluster catalogs, and tablespace symlinks.
        MARKER_BASE | MARKER_GLOBAL | "pg_tblspc" => Role::Data,
        // Write-ahead log: `pg_wal` from v10, `pg_xlog` before it.
        "pg_wal" | "pg_xlog" => Role::Wal,
        // Server log output, wherever the packager pointed it.
        "log" | "pg_log" => Role::Log,
        // Statistics scratch, rewritten constantly and rebuilt on start.
        "pg_stat_tmp" => Role::TempSpill,
        _ => Role::Meta,
    }
}

/// True for a WAL segment file name: 24 hex digits, no extension.
/// `.partial`, `.backup`, and `.history` files deliberately do not match.
pub fn is_wal_segment(name: &str) -> bool {
    name.len() == 24 && name.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Read `PG_VERSION`. Bounded to 16 bytes: the file holds a major version
/// such as `16` or `9.6`, and anything else is not a cluster stamp.
pub fn read_version(data_dir: &Path) -> Option<String> {
    let mut f = File::open(data_dir.join(MARKER_VERSION)).ok()?;
    let mut buf = [0u8; 16];
    let n = f.read(&mut buf).ok()?;
    let text = std::str::from_utf8(&buf[..n]).ok()?.trim();
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some(text.to_string())
}

/// What a scan learned about one cluster.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cluster {
    /// Major version from `PG_VERSION`, when it could be read.
    pub version: Option<String>,
    pub data_bytes: u64,
    pub wal_bytes: u64,
    pub wal_segments: u64,
    pub temp_bytes: u64,
    pub log_bytes: u64,
    pub meta_bytes: u64,
}

impl Cluster {
    pub fn total_bytes(&self) -> u64 {
        self.data_bytes
            .saturating_add(self.wal_bytes)
            .saturating_add(self.temp_bytes)
            .saturating_add(self.log_bytes)
            .saturating_add(self.meta_bytes)
    }

    /// Space a maintenance action would actually free. Spill and logs only —
    /// `base/` is never counted, however large it is.
    pub fn reclaimable_bytes(&self) -> u64 {
        self.temp_bytes.saturating_add(self.log_bytes)
    }

    pub fn label(&self) -> String {
        match &self.version {
            Some(v) => format!("PostgreSQL {v}"),
            None => "PostgreSQL".to_string(),
        }
    }

    /// `64 segments · 1.00 GB`, or just the size when nothing counted.
    pub fn wal_summary(&self) -> String {
        if self.wal_segments == 0 {
            return human_bytes(self.wal_bytes);
        }
        format!(
            "{} segments · {}",
            crate::size::group_u64(self.wal_segments),
            human_bytes(self.wal_bytes)
        )
    }

    /// True when the WAL has grown past a handful of segments, which is the
    /// signal worth surfacing: a stale replication slot or an archive command
    /// that is failing will pin segments indefinitely.
    pub fn wal_looks_retained(&self) -> bool {
        self.wal_bytes > 8 * WAL_SEGMENT_BYTES
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_directories_map_to_roles() {
        assert_eq!(top_role("base"), Role::Data);
        assert_eq!(top_role("global"), Role::Data);
        assert_eq!(top_role("pg_tblspc"), Role::Data);
        assert_eq!(top_role("pg_wal"), Role::Wal);
        assert_eq!(top_role("pg_xlog"), Role::Wal, "9.x spelling");
        assert_eq!(top_role("log"), Role::Log);
        assert_eq!(top_role("pg_log"), Role::Log);
        assert_eq!(top_role("pg_stat_tmp"), Role::TempSpill);
    }

    #[test]
    fn unknown_entries_default_to_protected() {
        // Being wrong towards "do not delete" is the only safe direction.
        for name in [
            "pg_xact",
            "pg_multixact",
            "pg_replslot",
            "postgresql.conf",
            "postmaster.pid",
            "something_a_future_version_adds",
        ] {
            assert_eq!(top_role(name), Role::Meta, "{name}");
            assert!(
                super::super::AppTag::new(super::super::AppKind::Postgres, top_role(name))
                    .is_protected(),
                "{name} must not be deletable"
            );
        }
    }

    #[test]
    fn wal_segment_names_are_24_hex_digits() {
        assert!(is_wal_segment("000000010000000000000001"));
        assert!(is_wal_segment("0000000100000B0A000000FF"));
        assert!(!is_wal_segment("000000010000000000000001.partial"));
        assert!(!is_wal_segment("00000001.history"));
        assert!(!is_wal_segment("archive_status"));
        assert!(!is_wal_segment(""));
        assert!(!is_wal_segment("00000001000000000000000"), "23 digits");
    }

    #[test]
    fn reads_a_version_stamp() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join(MARKER_VERSION), "16\n").unwrap();
        assert_eq!(read_version(tmp.path()).as_deref(), Some("16"));

        std::fs::write(tmp.path().join(MARKER_VERSION), "9.6\n").unwrap();
        assert_eq!(read_version(tmp.path()).as_deref(), Some("9.6"));

        // Not a version stamp: refused rather than shown as one.
        std::fs::write(tmp.path().join(MARKER_VERSION), "not a version").unwrap();
        assert_eq!(read_version(tmp.path()), None);
    }

    #[test]
    fn reclaimable_never_counts_table_data() {
        let c = Cluster {
            version: Some("16".into()),
            data_bytes: 31 * 1024 * 1024 * 1024,
            wal_bytes: 1024 * 1024 * 1024,
            wal_segments: 64,
            temp_bytes: 3 * 1024 * 1024 * 1024,
            log_bytes: 512 * 1024 * 1024,
            meta_bytes: 1024,
        };
        // 3 GB spill + 512 MB logs — the 31 GB of tables is untouchable and
        // the 1 GB of WAL is not garbage either.
        assert_eq!(
            c.reclaimable_bytes(),
            3 * 1024 * 1024 * 1024 + 512 * 1024 * 1024
        );
        assert!(c.reclaimable_bytes() < c.data_bytes);
        assert!(c.wal_looks_retained());
        assert_eq!(c.label(), "PostgreSQL 16");
        assert!(c.wal_summary().contains("64 segments"));
    }
}
