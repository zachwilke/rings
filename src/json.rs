//! Hand-rolled JSON writer for the export tree. Replaces serde + serde_json:
//! one fixed shape does not need a serialization framework.

use crate::dto::include_in_export;
use crate::scan::Tree;
use crate::size::human_bytes;

/// Serialize the subtree at `id` (same filter rule as CSV).
pub fn tree_to_json(tree: &Tree, id: usize) -> String {
    let mut out = String::with_capacity(4096);
    write_node(&mut out, tree, id, 0);
    out.push('\n');
    out
}

fn write_node(out: &mut String, tree: &Tree, id: usize, depth: usize) {
    let n = tree.get(id);
    let pad = "  ".repeat(depth);
    let inner = "  ".repeat(depth + 1);
    out.push_str(&format!("{pad}{{\n"));
    out.push_str(&format!(
        "{inner}\"path\": {},\n",
        quote(&n.path.to_string_lossy())
    ));
    out.push_str(&format!("{inner}\"type\": \"{}\",\n", n.kind_str()));
    out.push_str(&format!("{inner}\"used_bytes\": {},\n", n.used));
    out.push_str(&format!("{inner}\"apparent_bytes\": {},\n", n.apparent));
    out.push_str(&format!(
        "{inner}\"size_human\": {},\n",
        quote(&human_bytes(n.used))
    ));
    out.push_str(&format!("{inner}\"category\": \"{}\",\n", n.category));
    if let Some(tag) = n.app {
        out.push_str(&format!(
            "{inner}\"application\": \"{}\",\n",
            tag.kind.as_str()
        ));
        out.push_str(&format!("{inner}\"role\": \"{}\",\n", tag.role.as_str()));
        out.push_str(&format!(
            "{inner}\"reclaim\": \"{}\",\n",
            tag.reclaim().as_str()
        ));
    }
    let kids: Vec<usize> = n
        .children
        .iter()
        .copied()
        .filter(|&c| {
            let k = tree.get(c);
            include_in_export(k.is_dir, k.used, k.category)
        })
        .collect();
    if kids.is_empty() {
        out.push_str(&format!("{inner}\"children\": []\n"));
    } else {
        out.push_str(&format!("{inner}\"children\": [\n"));
        for (i, c) in kids.iter().enumerate() {
            write_node(out, tree, *c, depth + 2);
            if i + 1 < kids.len() {
                out.push(',');
            }
            out.push('\n');
        }
        out.push_str(&format!("{inner}]\n"));
    }
    out.push_str(&format!("{pad}}}"));
}

/// JSON string escaping per RFC 8259.
pub fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{scan, WalkOptions};
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn escapes_control_and_special_chars() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a\"b"), "\"a\\\"b\"");
        assert_eq!(quote("back\\slash"), "\"back\\\\slash\"");
        assert_eq!(quote("nl\n"), "\"nl\\n\"");
        assert_eq!(quote("\u{1}"), "\"\\u0001\"");
    }

    #[test]
    fn emits_parseable_shape() {
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("d")).unwrap();
        fs::write(tmp.path().join("d").join("f"), vec![0u8; 2048]).unwrap();
        let tree = scan(tmp.path(), WalkOptions::default()).unwrap();
        let json = tree_to_json(&tree, tree.root);
        assert!(json.contains("\"used_bytes\""));
        assert!(json.contains("\"children\": ["));
        // Balanced braces/brackets is a cheap sanity proxy without a parser.
        let opens = json.matches('{').count();
        let closes = json.matches('}').count();
        assert_eq!(opens, closes);
        let bra = json.matches('[').count();
        let ket = json.matches(']').count();
        assert_eq!(bra, ket);
    }
}
