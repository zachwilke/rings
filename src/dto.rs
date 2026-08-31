//! Export DTOs. Kept separate from scan models so the TUI/tree can change
//! without dragging the CSV/JSON shape along.

use std::collections::BTreeSet;
use std::path::PathBuf;

use crate::classify::Category;
use crate::constants::EXPORT_FILE_MIN_BYTES;
use crate::scan::Tree;
use crate::size::human_bytes;

#[derive(Clone, Debug)]
pub struct FindingRow {
    pub path: String,
    pub kind: &'static str,
    pub size_bytes: u64,
    pub size_human: String,
    pub category: Category,
    pub in_delete_collector: bool,
}

/// Include every directory, every waste hit, and files at/above the size floor.
pub fn include_in_export(is_dir: bool, size_bytes: u64, category: Category) -> bool {
    is_dir || category.is_waste() || size_bytes >= EXPORT_FILE_MIN_BYTES
}

/// Rows for CSV, walked iteratively so deep trees cannot blow the stack.
pub fn finding_rows(
    tree: &Tree,
    subtree: usize,
    collector: &BTreeSet<PathBuf>,
) -> Vec<FindingRow> {
    let mut rows = Vec::new();
    let mut stack = vec![subtree];
    while let Some(id) = stack.pop() {
        let n = tree.get(id);
        if include_in_export(n.is_dir, n.used, n.category) {
            rows.push(FindingRow {
                path: n.path.to_string_lossy().into_owned(),
                kind: n.kind_str(),
                size_bytes: n.used,
                size_human: human_bytes(n.used),
                category: n.category,
                in_delete_collector: collector.contains(&n.path),
            });
        }
        // Children are size-sorted; push reversed so rows come out in order.
        for &c in n.children.iter().rev() {
            stack.push(c);
        }
    }
    rows
}

/// Waste hits only, largest first — the Temp & cache view.
pub fn waste_hits(tree: &Tree) -> Vec<usize> {
    let mut ids: Vec<usize> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.category.is_waste())
        .map(|(i, _)| i)
        .collect();
    ids.sort_by(|&a, &b| {
        tree.nodes[b]
            .used
            .cmp(&tree.nodes[a].used)
            .then_with(|| tree.nodes[a].path.cmp(&tree.nodes[b].path))
    });
    ids
}
