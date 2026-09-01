//! MySQL and MariaDB data directory recognition.
//!
//! One marker settles it: `ibdata1`. The InnoDB system tablespace exists in
//! every installation of both servers, with or without
//! `innodb_file_per_table`, so it identifies a datadir wherever the packager
//! put it — `/var/lib/mysql`, `/var/lib/mysql-files`, a Docker volume, or a
//! copy someone rsynced onto a backup disk.
//!
//! The finding that matters here is almost never the tables. It is the
//! **binary logs**. A server with `log_bin` on and no expiry policy will
//! quietly accumulate `mysql-bin.000001`, `.000002`, … until the volume is
//! full, and the fix is `PURGE BINARY LOGS` plus
//! `binlog_expire_logs_seconds` — never `rm`, which leaves the `.index` file
//! describing files that no longer exist and breaks replication.

use super::{Guidance, Reclaim, Role};
use crate::size::human_bytes;

/// Marker that identifies a data directory.
pub const MARKER_SYSTEM_TABLESPACE: &str = "ibdata1";

/// Present only on MariaDB: the Aria engine's control file, used for system
/// tables from 10.4 onward.
pub const MARKER_MARIADB: &str = "aria_log_control";

/// Scratch tablespace. Grows under load and only shrinks on restart.
pub const TEMP_TABLESPACE: &str = "ibtmp1";

/// MySQL 8.0.30+ moved the redo log into its own directory.
pub const REDO_DIR: &str = "#innodb_redo";
pub const TEMP_DIR: &str = "#innodb_temp";

pub fn guidance(role: Role) -> Guidance {
    match role {
        Role::Data => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "InnoDB tablespaces and schema directories",
            action: "never delete — OPTIMIZE TABLE rebuilds a fragmented table; only innodb_file_per_table returns space to the disk",
        },
        Role::Wal => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "InnoDB redo log, sized by configuration and reused in place",
            action: "never delete — it is a fixed cost; change innodb_redo_log_capacity (or innodb_log_file_size) to resize it",
        },
        Role::Binlog => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "binary and relay logs: replication and point-in-time recovery",
            action: "PURGE BINARY LOGS BEFORE NOW() - INTERVAL 7 DAY, then set binlog_expire_logs_seconds — deleting the files by hand breaks the .index and any replica",
        },
        Role::TempSpill => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "temporary tablespace grown by a large sort, join, or ALTER",
            action: "never delete while the server runs — it is recreated at restart, which is the only way to shrink it",
        },
        Role::Log => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "error log and slow query log",
            action: "safe to rotate or remove — then FLUSH LOGS so the server reopens them",
        },
        Role::Backup => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "dump or archive kept inside the data directory",
            action: "safe to move off this volume once you have confirmed another copy exists",
        },
        Role::Meta => Guidance {
            reclaim: Reclaim::Never,
            protected: true,
            why: "server identity and state: auto.cnf, pid, socket, buffer pool dump",
            action: "never delete — auto.cnf holds the server UUID that replication depends on",
        },
    }
}

/// True for a binary, relay, or index log file.
///
/// MySQL numbers its logs with a six-digit suffix (`mysql-bin.000001`,
/// `binlog.000001`, `host-relay-bin.000003`) and keeps a companion
/// `.index` listing them.
pub fn is_binlog(name: &str) -> bool {
    let Some((stem, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let looks_like_log_stem = stem.ends_with("bin") || stem.ends_with("binlog");
    if !looks_like_log_stem {
        return false;
    }
    // `…-bin.000001` — a numbered segment.
    if ext.len() >= 6 && ext.bytes().all(|b| b.is_ascii_digit()) {
        return true;
    }
    // `…-bin.index` — the manifest the server reads at startup.
    ext == "index"
}

/// Role for a top-level entry of the data directory.
///
/// Unknown entries resolve to [`Role::Data`], which is protected. A datadir
/// holds databases; assuming an unrecognised entry inside one is disposable
/// is the wrong way to be wrong.
pub fn top_role(name: &str) -> Role {
    if is_binlog(name) {
        return Role::Binlog;
    }
    match name {
        TEMP_TABLESPACE | TEMP_DIR => return Role::TempSpill,
        "ib_logfile0" | "ib_logfile1" | REDO_DIR => return Role::Wal,
        "ib_buffer_pool" | "auto.cnf" | "mysql_upgrade_info" => return Role::Meta,
        _ => {}
    }
    if name.starts_with("ibdata") || name.starts_with("undo_") || name.starts_with("#innodb_undo") {
        return Role::Data;
    }
    // MariaDB's Aria log; small, and rebuilt by aria_chk, not by hand.
    if name.starts_with("aria_log") {
        return Role::Meta;
    }
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("err") | Some("log") => Role::Log,
        Some("pid") | Some("sock") | Some("cnf") | Some("flag") => Role::Meta,
        Some("sql") | Some("gz") | Some("zst") | Some("xz") | Some("bak") => Role::Backup,
        // Schema directories, `.ibd`, `.MYD`, `.MYI`, `.frm`, and anything
        // a future version adds.
        _ => Role::Data,
    }
}

/// What a scan learned about one server.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Server {
    /// `"MariaDB"` when the Aria control file is present, else `"MySQL"`.
    pub flavor: &'static str,
    pub data_bytes: u64,
    pub wal_bytes: u64,
    pub binlog_bytes: u64,
    pub binlog_files: u64,
    pub temp_bytes: u64,
    pub log_bytes: u64,
    pub backup_bytes: u64,
    pub meta_bytes: u64,
}

impl Server {
    pub fn label(&self) -> String {
        if self.flavor.is_empty() {
            "MySQL/MariaDB".to_string()
        } else {
            self.flavor.to_string()
        }
    }

    /// Space a documented action returns without touching table data:
    /// purge the binary logs, restart to reset `ibtmp1`, rotate the logs,
    /// move the dumps off the volume.
    pub fn reclaimable_bytes(&self) -> u64 {
        self.binlog_bytes
            .saturating_add(self.temp_bytes)
            .saturating_add(self.log_bytes)
            .saturating_add(self.backup_bytes)
    }

    /// `128 files · 2.00 GB`.
    pub fn binlog_summary(&self) -> String {
        if self.binlog_files == 0 {
            return human_bytes(self.binlog_bytes);
        }
        format!(
            "{} files · {}",
            crate::size::group_u64(self.binlog_files),
            human_bytes(self.binlog_bytes)
        )
    }

    /// True when the binary logs have outgrown the data they describe —
    /// the shape of a server with `log_bin` on and no expiry set.
    pub fn binlogs_look_unpurged(&self) -> bool {
        self.binlog_bytes > self.data_bytes.max(1 << 30)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_numbered_and_index_logs() {
        assert!(is_binlog("mysql-bin.000001"));
        assert!(is_binlog("binlog.000042"));
        assert!(is_binlog("db01-relay-bin.000003"));
        assert!(is_binlog("mysql-bin.index"));
        assert!(is_binlog("binlog.index"));

        assert!(!is_binlog("ibdata1"));
        assert!(!is_binlog("ib_logfile0"));
        assert!(!is_binlog("mysql-bin"), "no suffix");
        assert!(!is_binlog("notes.index"), "stem is not a log name");
        assert!(!is_binlog("something.000001"), "stem is not a log name");
    }

    #[test]
    fn top_level_entries_map_to_roles() {
        assert_eq!(top_role("ibdata1"), Role::Data);
        assert_eq!(top_role("ibdata2"), Role::Data);
        assert_eq!(top_role("undo_001"), Role::Data);
        assert_eq!(top_role("ib_logfile0"), Role::Wal);
        assert_eq!(top_role("#innodb_redo"), Role::Wal);
        assert_eq!(top_role("ibtmp1"), Role::TempSpill);
        assert_eq!(top_role("#innodb_temp"), Role::TempSpill);
        assert_eq!(top_role("mysql-bin.000001"), Role::Binlog);
        assert_eq!(top_role("mysql-bin.index"), Role::Binlog);
        assert_eq!(top_role("db01.err"), Role::Log);
        assert_eq!(top_role("db01-slow.log"), Role::Log);
        assert_eq!(top_role("auto.cnf"), Role::Meta);
        assert_eq!(top_role("aria_log_control"), Role::Meta);
        assert_eq!(top_role("mysql.sock"), Role::Meta);
        assert_eq!(top_role("nightly.sql.gz"), Role::Backup);
        // Schema directories and anything unrecognised.
        assert_eq!(top_role("mysql"), Role::Data);
        assert_eq!(top_role("wordpress"), Role::Data);
        assert_eq!(top_role("something_new_in_9_0"), Role::Data);
    }

    #[test]
    fn binary_logs_are_protected_but_still_reclaimable() {
        // Both halves matter: rings must refuse to unlink them, and must
        // still count them toward what can be freed.
        let g = guidance(Role::Binlog);
        assert!(g.protected, "rm breaks the .index and any replica");
        assert_eq!(g.reclaim, Reclaim::Command);
        assert!(g.action.contains("PURGE BINARY LOGS"));
    }

    #[test]
    fn reclaimable_excludes_tables_and_redo() {
        let s = Server {
            flavor: "MariaDB",
            data_bytes: 40 << 30,
            wal_bytes: 2 << 30,
            binlog_bytes: 60 << 30,
            binlog_files: 480,
            temp_bytes: 1 << 30,
            log_bytes: 512 << 20,
            backup_bytes: 0,
            meta_bytes: 4096,
        };
        assert_eq!(
            s.reclaimable_bytes(),
            (60 << 30) + (1 << 30) + (512 << 20),
            "binlogs, ibtmp1 and logs — not the 40 GB of tables or the redo log"
        );
        assert!(s.binlogs_look_unpurged());
        assert_eq!(s.label(), "MariaDB");
        assert!(s.binlog_summary().contains("480 files"));
    }

    #[test]
    fn a_tidy_server_does_not_raise_the_binlog_flag() {
        let s = Server {
            flavor: "MySQL",
            data_bytes: 40 << 30,
            binlog_bytes: 512 << 20,
            binlog_files: 4,
            ..Server::default()
        };
        assert!(!s.binlogs_look_unpurged());
    }
}
