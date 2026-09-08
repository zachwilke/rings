//! Scan-wide fuzzy finder. `std` only — subsequence scoring, no crates.

use crate::classify::Category;
use crate::scan::{Node, Tree};
use crate::tui::app::{scroll_to_show, LIST_PAGE};

/// Hard cap so a million-file home stays snappy.
pub const RESULT_CAP: usize = 200;

/// One ranked hit. `name_marks` are character indices into `Node::name`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FinderHit {
    pub node: usize,
    pub score: i32,
    pub size: u64,
    pub name_marks: Vec<usize>,
}

/// Live finder state. Query changes rescore; Esc drops the view, not this
/// buffer, so opening `/` again keeps what you were typing if we reset on open.
#[derive(Clone, Debug, Default)]
pub struct Finder {
    pub query: String,
    pub selected: usize,
    pub offset: usize,
    /// `true` (default): whole scan. `false`: current drill-in directory.
    pub whole_scan: bool,
    pub results: Vec<FinderHit>,
}

impl Finder {
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.offset = 0;
        self.whole_scan = true;
        self.results.clear();
    }

    pub fn rescore(&mut self, tree: &Tree, scope: usize, apparent: bool) {
        self.results = search(tree, scope, &self.query, apparent, RESULT_CAP);
        if self.results.is_empty() {
            self.selected = 0;
            self.offset = 0;
            return;
        }
        self.selected = self.selected.min(self.results.len() - 1);
        self.offset = scroll_to_show(self.selected, self.offset, LIST_PAGE);
    }

    pub fn move_sel(&mut self, delta: isize) {
        self.selected = crate::tui::app::step(self.selected, delta, self.results.len());
        self.offset = scroll_to_show(self.selected, self.offset, LIST_PAGE);
    }

    pub fn select_row(&mut self, i: usize) {
        if self.results.is_empty() {
            self.selected = 0;
            return;
        }
        self.selected = i.min(self.results.len() - 1);
        self.offset = scroll_to_show(self.selected, self.offset, LIST_PAGE);
    }

    pub fn selected_node(&self) -> Option<usize> {
        self.results.get(self.selected).map(|h| h.node)
    }

    pub fn type_char(&mut self, ch: char) {
        if !ch.is_control() {
            self.query.push(ch);
        }
    }

    pub fn backspace(&mut self) {
        self.query.pop();
    }

    pub fn toggle_scope(&mut self) {
        self.whole_scan = !self.whole_scan;
        self.selected = 0;
        self.offset = 0;
    }
}

/// Tight subsequence score. `None` if `query` is not a subsequence of `text`.
/// Positions are character indices into `text`. Higher is better.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }
    let t: Vec<char> = text.chars().collect();
    let q: Vec<char> = query.chars().collect();
    if q.len() > t.len() {
        return None;
    }

    // Forward pass: prove a match exists (leftmost).
    let mut pos = Vec::with_capacity(q.len());
    let mut start = 0;
    for &qc in &q {
        let want = fold(qc);
        let found = t[start..].iter().position(|&c| fold(c) == want)?;
        let i = start + found;
        pos.push(i);
        start = i + 1;
    }

    // Backward pass: pull the match as tight as possible (fzf-style).
    let mut end = t.len();
    for qi in (0..q.len()).rev() {
        let want = fold(q[qi]);
        let floor = if qi == 0 { 0 } else { pos[qi - 1] + 1 };
        let found = t[floor..end].iter().rposition(|&c| fold(c) == want)?;
        pos[qi] = floor + found;
        end = pos[qi];
    }

    Some((score_positions(&t, &pos), pos))
}

fn fold(c: char) -> char {
    if c.is_ascii() {
        c.to_ascii_lowercase()
    } else {
        c
    }
}

fn is_boundary(c: char) -> bool {
    matches!(c, '/' | '\\' | '.' | '-' | '_' | ' ' | ':' | '+' | '@')
}

fn score_positions(text: &[char], pos: &[usize]) -> i32 {
    if pos.is_empty() {
        return 0;
    }
    let mut score = 16;
    score += (32i32.saturating_sub(pos[0] as i32 * 2)).max(0);
    let span = (pos[pos.len() - 1] - pos[0] + 1) as i32;
    score += (64 - span).max(0);
    let mut run = pos.len() > 1;
    for w in pos.windows(2) {
        if w[1] == w[0] + 1 {
            score += 24;
        } else {
            run = false;
            score -= (w[1] - w[0] - 1) as i32;
        }
    }
    if run {
        score += 20;
    }
    for &i in pos {
        if i == 0 || is_boundary(text[i - 1]) {
            score += 12;
        }
    }
    score
}

fn eq_ci(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.chars().zip(b.chars()).all(|(x, y)| fold(x) == fold(y))
}

fn starts_with_ci(text: &str, prefix: &str) -> bool {
    let mut ti = text.chars();
    for pc in prefix.chars() {
        match ti.next() {
            Some(tc) if fold(tc) == fold(pc) => {}
            _ => return false,
        }
    }
    true
}

/// Extension without the dot. Leading-dot names (`.gitignore`) have none
/// unless there is a later dot (`.tar.gz` → `gz`).
pub fn file_ext(name: &str) -> &str {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => &name[i + 1..],
        _ => "",
    }
}

fn ext_marks(name: &str, ext: &str) -> Vec<usize> {
    let chars: Vec<char> = name.chars().collect();
    let ext_chars: Vec<char> = ext.chars().collect();
    if ext_chars.is_empty() || chars.len() < ext_chars.len() {
        return Vec::new();
    }
    let start = chars.len() - ext_chars.len();
    if chars[start..]
        .iter()
        .zip(ext_chars.iter())
        .all(|(&a, &b)| fold(a) == fold(b))
    {
        return (start..chars.len()).collect();
    }
    Vec::new()
}

/// Category words people actually type. Kept small so the classify hot
/// path is never involved — this is query-time only.
pub fn category_query(q: &str) -> Option<Category> {
    let q = q.strip_prefix('.').unwrap_or(q);
    if eq_ci(q, "temp") || eq_ci(q, "tmp") {
        Some(Category::Temp)
    } else if eq_ci(q, "cache") || eq_ci(q, "caches") {
        Some(Category::Cache)
    } else if eq_ci(q, "log") || eq_ci(q, "logs") {
        Some(Category::Log)
    } else if eq_ci(q, "journal") {
        Some(Category::Journal)
    } else if eq_ci(q, "crash") || eq_ci(q, "dump") || eq_ci(q, "coredump") {
        Some(Category::Crash)
    } else {
        None
    }
}

/// Score one node against the query. Empty query is a match with score 0
/// (caller sorts those by size). `path` is a display string, usually the
/// relative path from the scan root.
pub fn score_node(query: &str, node: &Node, path: &str) -> Option<(i32, Vec<usize>)> {
    let q = query.trim();
    if q.is_empty() {
        return Some((0, Vec::new()));
    }

    let mut best: Option<(i32, Vec<usize>)> = None;
    let consider = |best: &mut Option<(i32, Vec<usize>)>, score: i32, marks: Vec<usize>| {
        if best.as_ref().map_or(true, |(s, _)| score > *s) {
            *best = Some((score, marks));
        }
    };

    if let Some((s, pos)) = fuzzy(q, &node.name) {
        let mut score = s + 30;
        if eq_ci(&node.name, q) {
            score += 40;
        } else if starts_with_ci(&node.name, q) {
            score += 20;
        }
        consider(&mut best, score, pos);
    }

    let ext = file_ext(&node.name);
    if !ext.is_empty() {
        let q_ext = q.strip_prefix('.').unwrap_or(q);
        if eq_ci(ext, q_ext) {
            consider(&mut best, 90, ext_marks(&node.name, ext));
        } else if let Some((s, _)) = fuzzy(q_ext, ext) {
            consider(&mut best, s + 50, ext_marks(&node.name, ext));
        }
    }

    if let Some(cat) = category_query(q) {
        if node.category == cat {
            consider(&mut best, 85, Vec::new());
        }
    } else if node.category.is_waste() {
        if let Some((s, _)) = fuzzy(q, node.category.as_str()) {
            consider(&mut best, s + 40, Vec::new());
        }
        if let Some((s, _)) = fuzzy(q, node.category.label()) {
            consider(&mut best, s + 40, Vec::new());
        }
    }

    if let Some((s, _)) = fuzzy(q, path) {
        // Path is a weaker field; keep name marks if we already have them.
        let marks = best.as_ref().map(|(_, m)| m.clone()).unwrap_or_default();
        consider(&mut best, s + 10, marks);
    }

    best
}

pub fn relative_path(tree: &Tree, id: usize) -> String {
    let root = &tree.root_node().path;
    let path = &tree.get(id).path;
    path.strip_prefix(root)
        .map(|p| {
            let s = p.to_string_lossy();
            if s.is_empty() {
                tree.get(id).name.clone()
            } else {
                s.replace('\\', "/")
            }
        })
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn better(a: &FinderHit, b: &FinderHit) -> bool {
    a.score > b.score
        || (a.score == b.score && a.size > b.size)
        || (a.score == b.score && a.size == b.size && a.node < b.node)
}

/// Keep the best `cap` hits without collecting every match.
fn push_top(hits: &mut Vec<FinderHit>, hit: FinderHit, cap: usize) {
    if hits.len() < cap {
        hits.push(hit);
        return;
    }
    let mut worst = 0;
    for i in 1..hits.len() {
        if better(&hits[worst], &hits[i]) {
            worst = i;
        }
    }
    if better(&hit, &hits[worst]) {
        hits[worst] = hit;
    }
}

/// Walk `scope` (exclusive — the directory itself is not a hit) and rank
/// every descendant. Empty query: largest `cap` nodes, size descending.
pub fn search(
    tree: &Tree,
    scope: usize,
    query: &str,
    apparent: bool,
    cap: usize,
) -> Vec<FinderHit> {
    if cap == 0 || scope >= tree.nodes.len() {
        return Vec::new();
    }
    let mut hits = Vec::with_capacity(cap.min(64));
    let mut stack = tree.get(scope).children.clone();
    while let Some(id) = stack.pop() {
        let node = tree.get(id);
        stack.extend_from_slice(&node.children);
        let path = relative_path(tree, id);
        if let Some((score, name_marks)) = score_node(query, node, &path) {
            push_top(
                &mut hits,
                FinderHit {
                    node: id,
                    score,
                    size: node.display_size(apparent),
                    name_marks,
                },
                cap,
            );
        }
    }
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.size.cmp(&a.size))
            .then_with(|| a.node.cmp(&b.node))
    });
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scan::{Node, ScanStats, Tree};
    use std::path::PathBuf;

    fn file(name: &str, path: &str, parent: Option<usize>, used: u64, cat: Category) -> Node {
        Node {
            name: name.into(),
            path: PathBuf::from(path),
            parent,
            children: vec![],
            is_dir: false,
            own_used: used,
            own_apparent: used,
            used,
            apparent: used,
            category: cat,
            nlink: 1,
            app: None,
            guard: None,
        }
    }

    fn dir(name: &str, path: &str, parent: Option<usize>, children: Vec<usize>) -> Node {
        Node {
            name: name.into(),
            path: PathBuf::from(path),
            parent,
            children,
            is_dir: true,
            own_used: 0,
            own_apparent: 0,
            used: 0,
            apparent: 0,
            category: Category::Normal,
            nlink: 2,
            app: None,
            guard: None,
        }
    }

    /// root / movie.mp4(5000) / notes.txt(100) / cache/thumb(8000) / deep/iso.iso(3000)
    fn sample_tree() -> Tree {
        let mut tree = Tree {
            nodes: vec![
                dir("root", "/root", None, vec![1, 2, 3, 5]),
                file(
                    "movie.mp4",
                    "/root/movie.mp4",
                    Some(0),
                    5000,
                    Category::Normal,
                ),
                file(
                    "notes.txt",
                    "/root/notes.txt",
                    Some(0),
                    100,
                    Category::Normal,
                ),
                dir("cache", "/root/cache", Some(0), vec![4]),
                file("thumb", "/root/cache/thumb", Some(3), 8000, Category::Cache),
                dir("deep", "/root/deep", Some(0), vec![6]),
                file(
                    "iso.iso",
                    "/root/deep/iso.iso",
                    Some(5),
                    3000,
                    Category::Normal,
                ),
            ],
            root: 0,
            stats: ScanStats::default(),
            probes: Vec::new(),
        };
        tree.recompute();
        tree
    }

    #[test]
    fn empty_query_is_a_match() {
        assert_eq!(fuzzy("", "anything"), Some((0, vec![])));
    }

    #[test]
    fn missing_letter_is_not_a_match() {
        assert_eq!(fuzzy("xyz", "movie.mp4"), None);
    }

    #[test]
    fn tightens_to_the_extension() {
        let (score, pos) = fuzzy("mp4", "movie.mp4").unwrap();
        assert_eq!(pos, vec![6, 7, 8], "prefer the consecutive tail");
        let loose = fuzzy("mp4", "m-extra-p-extra-4").unwrap();
        assert!(
            score > loose.0,
            "consecutive extension ({score}) beats a gapped subsequence ({})",
            loose.0
        );
    }

    #[test]
    fn consecutive_beats_gapped() {
        let tight = fuzzy("abc", "xxabcxx").unwrap();
        let gapped = fuzzy("abc", "a_b_c").unwrap();
        assert!(tight.0 > gapped.0, "{} vs {}", tight.0, gapped.0);
    }

    #[test]
    fn word_boundary_is_rewarded() {
        let boundary = fuzzy("log", "var/log/syslog").unwrap();
        let buried = fuzzy("log", "cataloguing").unwrap();
        assert!(
            boundary.0 > buried.0,
            "slash-prefixed ({}) should beat buried ({})",
            boundary.0,
            buried.0
        );
    }

    #[test]
    fn case_insensitive_ascii() {
        let (s, pos) = fuzzy("Rings", "rings").unwrap();
        assert_eq!(pos, vec![0, 1, 2, 3, 4]);
        assert!(s > 0);
    }

    #[test]
    fn file_ext_skips_leading_dot() {
        assert_eq!(file_ext("movie.mp4"), "mp4");
        assert_eq!(file_ext(".gitignore"), "");
        assert_eq!(file_ext("archive.tar.gz"), "gz");
        assert_eq!(file_ext("README"), "");
    }

    #[test]
    fn category_words() {
        assert_eq!(category_query("cache"), Some(Category::Cache));
        assert_eq!(category_query("CACHE"), Some(Category::Cache));
        assert_eq!(category_query("tmp"), Some(Category::Temp));
        assert_eq!(category_query("logs"), Some(Category::Log));
        assert_eq!(category_query("crash"), Some(Category::Crash));
        assert_eq!(category_query("movie"), None);
    }

    #[test]
    fn empty_query_lists_largest_first() {
        let tree = sample_tree();
        let hits = search(&tree, 0, "", false, 10);
        assert!(!hits.is_empty());
        assert!(
            hits.windows(2).all(|w| w[0].size >= w[1].size),
            "empty query is size-desc: {:?}",
            hits.iter().map(|h| h.size).collect::<Vec<_>>()
        );
        assert_eq!(
            tree.get(hits[0].node).name,
            "cache",
            "largest subtree first"
        );
    }

    #[test]
    fn name_fragment_finds_the_file() {
        let tree = sample_tree();
        let hits = search(&tree, 0, "notes", false, 10);
        assert_eq!(hits[0].node, 2);
        assert!(!hits[0].name_marks.is_empty());
    }

    #[test]
    fn extension_query_with_or_without_dot() {
        let tree = sample_tree();
        for q in ["mp4", ".mp4", "MP4"] {
            let hits = search(&tree, 0, q, false, 10);
            assert_eq!(
                tree.get(hits[0].node).name,
                "movie.mp4",
                "query {q:?} should prefer the mp4"
            );
            assert!(
                hits[0].name_marks.windows(3).any(|w| w == [6, 7, 8]),
                "query {q:?} should mark the extension, got {:?}",
                hits[0].name_marks
            );
        }
        let hits = search(&tree, 0, "iso", false, 10);
        assert_eq!(tree.get(hits[0].node).name, "iso.iso");
    }

    #[test]
    fn category_query_surfaces_cache() {
        let tree = sample_tree();
        let hits = search(&tree, 0, "cache", false, 10);
        assert!(
            hits.iter()
                .any(|h| tree.get(h.node).category == Category::Cache),
            "cache query should hit the tagged file: {:?}",
            hits.iter()
                .map(|h| &tree.get(h.node).name)
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn equal_score_prefers_the_larger_file() {
        let mut tree = sample_tree();
        tree.nodes.push(file(
            "other.mp4",
            "/root/other.mp4",
            Some(0),
            50_000,
            Category::Normal,
        ));
        let id = tree.nodes.len() - 1;
        tree.nodes[0].children.push(id);
        tree.recompute();
        let hits = search(&tree, 0, "mp4", false, 10);
        assert_eq!(tree.get(hits[0].node).name, "other.mp4");
        assert!(hits[0].size > hits[1].size);
    }

    #[test]
    fn scope_limits_to_a_subtree() {
        let tree = sample_tree();
        let hits = search(&tree, 5, "", false, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(tree.get(hits[0].node).name, "iso.iso");
        let hits = search(&tree, 5, "mp4", false, 10);
        assert!(hits.is_empty(), "movie.mp4 is outside /deep");
    }

    #[test]
    fn cap_keeps_only_the_best() {
        let tree = sample_tree();
        let hits = search(&tree, 0, "", false, 2);
        assert_eq!(hits.len(), 2);
        assert!(hits[0].size >= hits[1].size);
    }

    #[test]
    fn finder_types_and_rescored_selection_clamps() {
        let tree = sample_tree();
        let mut f = Finder {
            whole_scan: true,
            selected: 99,
            ..Finder::default()
        };
        f.type_char('m');
        f.type_char('p');
        f.type_char('4');
        f.rescore(&tree, 0, false);
        assert_eq!(f.query, "mp4");
        assert_eq!(tree.get(f.selected_node().unwrap()).name, "movie.mp4");
        f.backspace();
        f.backspace();
        f.backspace();
        assert!(f.query.is_empty());
        f.toggle_scope();
        assert!(!f.whole_scan);
    }
}
