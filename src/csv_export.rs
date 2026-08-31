//! Atomic CSV export of the analyzed tree.

use std::collections::BTreeSet;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::dto::{finding_rows, FindingRow};
use crate::scan::Tree;

pub const CSV_HEADER: &str =
    "path,type,size_bytes,size_human,category,in_delete_collector";

/// Write findings for `subtree` to `dest`. Uses a sibling temp file, then rename.
pub fn write_csv(
    dest: &Path,
    tree: &Tree,
    subtree: usize,
    collector: &BTreeSet<PathBuf>,
) -> Result<usize, String> {
    let rows = finding_rows(tree, subtree, collector);
    let body = render_csv(&rows);
    atomic_write(dest, body.as_bytes())?;
    Ok(rows.len())
}

pub fn render_csv(rows: &[FindingRow]) -> String {
    let mut out = String::from(CSV_HEADER);
    out.push('\n');
    for row in rows {
        out.push_str(&csv_quote(&row.path));
        out.push(',');
        out.push_str(row.kind);
        out.push(',');
        out.push_str(&row.size_bytes.to_string());
        out.push(',');
        out.push_str(&csv_quote(&row.size_human));
        out.push(',');
        out.push_str(row.category.as_str());
        out.push(',');
        out.push_str(if row.in_delete_collector {
            "true"
        } else {
            "false"
        });
        out.push('\n');
    }
    out
}

/// RFC 4180 quoting: wrap and double internal quotes when needed.
pub fn csv_quote(field: &str) -> String {
    let must_quote = field.contains(',')
        || field.contains('"')
        || field.contains('\n')
        || field.contains('\r');
    if !must_quote {
        return field.to_string();
    }
    let mut out = String::from("\"");
    for ch in field.chars() {
        if ch == '"' {
            out.push('"');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

fn atomic_write(dest: &Path, bytes: &[u8]) -> Result<(), String> {
    let parent = dest.parent().filter(|p| !p.as_os_str().is_empty());
    let dir = match parent {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir().map_err(|e| e.to_string())?,
    };
    let name = dest
        .file_name()
        .ok_or_else(|| "CSV path has no file name".to_string())?;
    let tmp = dir.join(format!(".{}.rings.tmp", name.to_string_lossy()));
    {
        let mut f = fs::File::create(&tmp).map_err(|e| {
            format!("cannot create temp CSV {}: {e}", tmp.display())
        })?;
        f.write_all(bytes).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
    }
    fs::rename(&tmp, dest).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("cannot replace {}: {e}", dest.display())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Category;
    use crate::dto::FindingRow;
    use crate::scan::{scan, WalkOptions};
    use std::collections::BTreeSet;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn header_and_columns_match_spec() {
        let header = CSV_HEADER.split(',').collect::<Vec<_>>();
        assert_eq!(
            header,
            [
                "path",
                "type",
                "size_bytes",
                "size_human",
                "category",
                "in_delete_collector"
            ]
        );
        let rows = [FindingRow {
            path: "/tmp/foo".into(),
            kind: "dir",
            size_bytes: 100,
            size_human: "100 B".into(),
            category: Category::Temp,
            in_delete_collector: false,
        }];
        let text = render_csv(&rows);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[0], CSV_HEADER);
        assert_eq!(lines[1], "/tmp/foo,dir,100,100 B,temp,false");
    }

    #[test]
    fn quotes_commas_quotes_and_newlines() {
        assert_eq!(csv_quote("plain"), "plain");
        assert_eq!(csv_quote("a,b"), "\"a,b\"");
        assert_eq!(csv_quote("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_quote("line\nbreak"), "\"line\nbreak\"");
        assert_eq!(csv_quote("win\r\n"), "\"win\r\n\"");
    }

    #[test]
    fn write_csv_atomic_and_includes_dirs() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("tiny"), b"abc").unwrap();
        let tree = scan(root, WalkOptions::default()).unwrap();
        let dest = tmp.path().join("out.csv");
        let n = write_csv(&dest, &tree, tree.root, &BTreeSet::new()).unwrap();
        assert!(n >= 2, "root dir + sub dir");
        let text = fs::read_to_string(&dest).unwrap();
        assert!(text.starts_with(CSV_HEADER));
        assert!(text.contains(",dir,"));
        assert!(!dest.with_file_name(".out.csv.rings.tmp").exists());
    }
}
