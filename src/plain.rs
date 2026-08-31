//! Stable, parseable table for `--plain` / piped stdout. No color, no TUI.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::dto::finding_rows;
use crate::scan::Tree;

pub const PLAIN_HEADER: &str = "path\ttype\tsize_bytes\tsize_human\tcategory";

/// Same row rules as CSV (dirs + waste hits + files ≥ 1 MiB), largest first.
pub fn render_plain(tree: &Tree, subtree: usize) -> String {
    let empty: BTreeSet<PathBuf> = BTreeSet::new();
    let mut rows = finding_rows(tree, subtree, &empty);
    rows.sort_by(|a, b| {
        b.size_bytes
            .cmp(&a.size_bytes)
            .then_with(|| a.path.cmp(&b.path))
    });
    let mut out = String::from(PLAIN_HEADER);
    out.push('\n');
    for row in rows {
        out.push_str(&sanitize(&row.path));
        out.push('\t');
        out.push_str(row.kind);
        out.push('\t');
        out.push_str(&row.size_bytes.to_string());
        out.push('\t');
        out.push_str(&row.size_human);
        out.push('\t');
        out.push_str(row.category.as_str());
        out.push('\n');
    }
    out
}

fn sanitize(path: &str) -> String {
    path.replace(['\t', '\n', '\r'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scan, WalkOptions};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn plain_has_header_no_ansi_largest_first() {
        let tmp = TempDir::new().unwrap();
        let root = tmp.path();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("big.dat"), vec![b'x'; 2 * 1024 * 1024]).unwrap();
        fs::write(root.join("sub").join("tiny"), b"abc").unwrap();

        let tree = scan(root, WalkOptions::default()).unwrap();
        let text = render_plain(&tree, tree.root);

        assert!(
            text.starts_with(PLAIN_HEADER),
            "header must be the first line:\n{text}"
        );
        assert!(
            !text.as_bytes().contains(&0x1b),
            "plain output must not contain ANSI:\n{text:?}"
        );
        assert!(!text.contains("\x1b["), "no CSI sequences");

        let mut bodies: Vec<&str> = text.lines().skip(1).filter(|l| !l.is_empty()).collect();
        assert!(bodies.len() >= 2, "root dir + large file at least");
        let sizes: Vec<u64> = bodies
            .iter()
            .map(|l| {
                l.split('\t')
                    .nth(2)
                    .unwrap()
                    .parse()
                    .expect("size_bytes column")
            })
            .collect();
        let mut sorted = sizes.clone();
        sorted.sort_by(|a, b| b.cmp(a));
        assert_eq!(sizes, sorted, "rows must be largest-first");

        assert!(bodies.iter().any(|l| l.contains("big.dat") && l.contains("file")));
        assert!(bodies.iter().any(|l| l.split('\t').nth(1) == Some("dir")));
    }
}
