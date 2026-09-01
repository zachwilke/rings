//! Microsoft SQL Server file recognition.
//!
//! Unlike PostgreSQL and MySQL, SQL Server has no canonical data directory:
//! files live wherever the DBA pointed them, often spread across several
//! volumes on purpose. So detection here is by extension, which is safe
//! because `.mdf`, `.ndf`, and `.ldf` are unambiguous in a way that `.db`
//! never is.
//!
//! The finding worth having is the **transaction log**. A database left in
//! FULL recovery with no log backup job will grow its `.ldf` without bound
//! until the volume fills — a `.ldf` several times the size of its `.mdf` is
//! the classic shape of it. Shrinking the file is not the fix on its own:
//! the log has to be backed up first, or it will simply grow again.

use super::{Guidance, Reclaim, Role};
use crate::size::human_bytes;

pub fn guidance(role: Role) -> Guidance {
    match role {
        Role::Data => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "database data file (.mdf / .ndf)",
            action: "never delete — detach or DROP through the server; DBCC SHRINKFILE only after freeing space inside it",
        },
        Role::Wal => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "transaction log (.ldf)",
            action: "never delete — check log_reuse_wait_desc, back the log up (or switch to SIMPLE recovery), then DBCC SHRINKFILE",
        },
        Role::TempSpill => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "tempdb, grown by sorts, spills, and version store traffic",
            action: "never delete — it is rebuilt at every service restart, which is the only supported way to reset its size",
        },
        Role::Backup => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "database backup sitting on the same volume as the data",
            action: "safe to move off this volume once you have confirmed another copy exists — a backup beside the data protects against nothing",
        },
        Role::Log => Guidance {
            reclaim: Reclaim::Safe,
            protected: false,
            why: "SQL Server error log",
            action: "safe to remove the rolled-over copies — sp_cycle_errorlog starts a new one",
        },
        Role::Binlog => Guidance {
            reclaim: Reclaim::Command,
            protected: true,
            why: "replication or Change Data Capture artefact",
            action: "never delete — managed through the server, not by hand",
        },
        Role::Meta => Guidance {
            reclaim: Reclaim::Never,
            protected: true,
            why: "instance metadata",
            action: "never delete — the instance will not start without it",
        },
    }
}

/// A `.ldf` at least this many times its `.mdf` is not being truncated.
pub const LOG_RATIO_ALERT: f64 = 1.0;

/// True for tempdb's own files, which are rebuilt at every restart.
pub fn is_tempdb(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    s == "templog" || s.starts_with("tempdb")
}

/// Role for a SQL Server file, by extension. `None` for anything else.
pub fn role_for(name: &str) -> Option<Role> {
    let (stem, ext) = name.rsplit_once('.')?;
    match ext.to_ascii_lowercase().as_str() {
        "mdf" | "ndf" => Some(if is_tempdb(stem) {
            Role::TempSpill
        } else {
            Role::Data
        }),
        "ldf" => Some(if is_tempdb(stem) {
            Role::TempSpill
        } else {
            Role::Wal
        }),
        _ => None,
    }
}

/// `.bak` and `.trn` are claimed only when a real data file sits beside
/// them: `.bak` is far too common a suffix to take on its own.
pub fn backup_extension(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(ext.to_ascii_lowercase().as_str(), "bak" | "trn")
}

/// Data file a transaction log belongs to: `orders_log.ldf` pairs with
/// `orders.mdf`. Only the `_log` convention is stripped — taking a bare
/// `log` suffix would turn `catalog.ldf` into `cata`.
pub fn log_base(stem: &str) -> &str {
    if stem.len() > 4 && stem.to_ascii_lowercase().ends_with("_log") {
        // The suffix is four ASCII bytes, so this is always a char boundary.
        &stem[..stem.len() - 4]
    } else {
        stem
    }
}

/// What a scan found in one directory of SQL Server files.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Instance {
    pub data_bytes: u64,
    pub data_files: u64,
    pub log_bytes: u64,
    pub log_files: u64,
    pub temp_bytes: u64,
    pub backup_bytes: u64,
}

impl Instance {
    /// Only what a documented action definitely returns.
    ///
    /// Backups move off the volume, so they count. A runaway `.ldf` does
    /// not: the correct size for a transaction log is a property of the
    /// workload, and guessing it would put a number on screen that rings
    /// cannot stand behind. tempdb is excluded for the same reason — its
    /// initial size is configured, often deliberately large. Both are still
    /// surfaced as their own rows, with the ratio that makes the case.
    pub fn reclaimable_bytes(&self) -> u64 {
        self.backup_bytes
    }

    pub fn total_bytes(&self) -> u64 {
        self.data_bytes
            .saturating_add(self.log_bytes)
            .saturating_add(self.temp_bytes)
            .saturating_add(self.backup_bytes)
    }
}

/// Note for a log file that has outgrown the data it protects.
pub fn log_ratio_note(log_bytes: u64, data_bytes: u64) -> Option<String> {
    if data_bytes == 0 || log_bytes == 0 {
        return None;
    }
    let ratio = log_bytes as f64 / data_bytes as f64;
    if ratio < LOG_RATIO_ALERT {
        return None;
    }
    Some(format!(
        "{ratio:.1}× its data file ({}) — the log is not being truncated",
        human_bytes(data_bytes)
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_decide_the_role() {
        assert_eq!(role_for("orders.mdf"), Some(Role::Data));
        assert_eq!(role_for("orders_secondary.ndf"), Some(Role::Data));
        assert_eq!(role_for("orders_log.ldf"), Some(Role::Wal));
        assert_eq!(role_for("ORDERS.MDF"), Some(Role::Data), "case-insensitive");
        assert_eq!(role_for("notes.txt"), None);
        assert_eq!(role_for("noextension"), None);
    }

    #[test]
    fn tempdb_is_its_own_role() {
        assert_eq!(role_for("tempdb.mdf"), Some(Role::TempSpill));
        assert_eq!(role_for("tempdb_mssql_2.ndf"), Some(Role::TempSpill));
        assert_eq!(role_for("templog.ldf"), Some(Role::TempSpill));
        // A user database that merely starts similarly is not tempdb.
        assert!(!is_tempdb("temp_orders"));
    }

    #[test]
    fn pairs_a_log_with_its_data_file() {
        assert_eq!(log_base("orders_log"), "orders");
        assert_eq!(log_base("Orders_Log"), "Orders");
        assert_eq!(log_base("orders"), "orders");
        // The trap the `_` guard exists for.
        assert_eq!(log_base("catalog"), "catalog");
        assert_eq!(log_base("_log"), "_log", "nothing left to pair with");
    }

    #[test]
    fn backup_extensions_are_recognised() {
        assert!(backup_extension("orders_full.bak"));
        assert!(backup_extension("orders_2026_09_01.TRN"));
        assert!(!backup_extension("orders.mdf"));
        assert!(!backup_extension("notes"));
    }

    #[test]
    fn a_runaway_log_is_called_out_with_its_ratio() {
        let note = log_ratio_note(40 << 30, 8 << 30).expect("5x log");
        assert!(note.contains("5.0×"), "{note}");
        assert!(note.contains("not being truncated"), "{note}");

        // A log smaller than its data file is unremarkable.
        assert_eq!(log_ratio_note(1 << 30, 8 << 30), None);
        assert_eq!(log_ratio_note(0, 8 << 30), None);
        assert_eq!(log_ratio_note(1 << 30, 0), None, "no data file to compare");
    }

    #[test]
    fn reclaimable_counts_backups_only() {
        let i = Instance {
            data_bytes: 8 << 30,
            data_files: 1,
            log_bytes: 40 << 30,
            log_files: 1,
            temp_bytes: 4 << 30,
            backup_bytes: 12 << 30,
        };
        assert_eq!(
            i.reclaimable_bytes(),
            12 << 30,
            "rings will not put a made-up number on a transaction log"
        );
        assert_eq!(i.total_bytes(), (8 << 30) + (40 << 30) + (4 << 30) + (12 << 30));
    }

    #[test]
    fn data_and_log_files_are_never_deletable() {
        for role in [Role::Data, Role::Wal, Role::TempSpill] {
            assert!(guidance(role).protected, "{role:?}");
        }
        assert!(!guidance(Role::Backup).protected, "backups are markable");
    }
}
