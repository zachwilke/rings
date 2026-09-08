//! Scan-wide fuzzy finder. `std` only — subsequence scoring, no crates.
//!
//! Hits are a capped, reused `Vec` (no million-entry index). Match positions
//! live in a `u64` bitset so a hit is a few words and never heap-allocates.

use std::borrow::Cow;

use crate::classify::Category;
use crate::scan::{Node, Tree};
use crate::tui::app::{scroll_to_show, LIST_PAGE};

/// Hard cap so a million-file home stays snappy.
pub const RESULT_CAP: usize = 200;

/// Query chars we bother matching. Longer input is truncated — a 32-letter
/// needle is already unique on a disk.
const MAX_Q: usize = 32;

/// Character indices 0..64 in a name. Enough for highlight; scoring still
/// uses the full (capped) query.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Marks(u64);

impl Marks {
    pub const EMPTY: Marks = Marks(0);

    pub fn contains(self, i: usize) -> bool {
        i < 64 && (self.0 >> i) & 1 == 1
    }

    pub fn is_empty(self) -> bool {
        self.0 == 0
    }

    fn set(&mut self, i: usize) {
        if i < 64 {
            self.0 |= 1 << i;
        }
    }
}

/// One ranked hit. Copy, no heap — safe to shuffle while ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FinderHit {
    pub node: usize,
    pub score: i32,
    pub size: u64,
    pub name_marks: Marks,
}

/// Live finder state. Result and walk vecs keep capacity across keystrokes.
#[derive(Clone, Debug)]
pub struct Finder {
    pub query: String,
    pub selected: usize,
    pub offset: usize,
    /// `true` (default): whole scan. `false`: current drill-in directory.
    pub whole_scan: bool,
    pub results: Vec<FinderHit>,
    scratch: Vec<usize>,
}

impl Default for Finder {
    fn default() -> Self {
        Self {
            query: String::new(),
            selected: 0,
            offset: 0,
            whole_scan: true,
            results: Vec::new(),
            scratch: Vec::new(),
        }
    }
}

impl Finder {
    pub fn reset(&mut self) {
        self.query.clear();
        self.selected = 0;
        self.offset = 0;
        self.whole_scan = true;
        self.results.clear();
        self.scratch.clear();
    }

    pub fn rescore(&mut self, tree: &Tree, scope: usize, apparent: bool) {
        search_into(
            &mut self.results,
            &mut self.scratch,
            tree,
            scope,
            &self.query,
            apparent,
            RESULT_CAP,
        );
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

/// Tight subsequence score. `None` if `query` is not a subsequence of `text`.
pub fn fuzzy(query: &str, text: &str) -> Option<(i32, Marks)> {
    if query.is_empty() {
        return Some((0, Marks::EMPTY));
    }
    let mut qch = ['\0'; MAX_Q];
    let mut qn = 0;
    for c in query.chars() {
        if qn == MAX_Q {
            break;
        }
        qch[qn] = fold(c);
        qn += 1;
    }
    if qn == 0 {
        return Some((0, Marks::EMPTY));
    }

    let mut pos = [0u16; MAX_Q];
    let mut start = 0usize;
    for qi in 0..qn {
        let want = qch[qi];
        let mut found = None;
        for (i, c) in text.chars().enumerate().skip(start) {
            if fold(c) == want {
                found = Some(i);
                break;
            }
        }
        let i = found?;
        pos[qi] = i as u16;
        start = i + 1;
    }

    let tlen = text.chars().count();
    let mut end = tlen;
    for qi in (0..qn).rev() {
        let want = qch[qi];
        let floor = if qi == 0 { 0 } else { pos[qi - 1] as usize + 1 };
        let mut found = None;
        for (i, c) in text.chars().enumerate() {
            if i < floor {
                continue;
            }
            if i >= end {
                break;
            }
            if fold(c) == want {
                found = Some(i);
            }
        }
        let i = found?;
        pos[qi] = i as u16;
        end = i;
    }

    Some((score_positions(text, &pos, qn), marks_from(&pos, qn)))
}

fn marks_from(pos: &[u16; MAX_Q], n: usize) -> Marks {
    let mut m = Marks::EMPTY;
    for i in 0..n {
        m.set(pos[i] as usize);
    }
    m
}

fn score_positions(text: &str, pos: &[u16; MAX_Q], n: usize) -> i32 {
    if n == 0 {
        return 0;
    }
    let mut score = 16;
    score += (32i32.saturating_sub(pos[0] as i32 * 2)).max(0);
    let span = (pos[n - 1] - pos[0] + 1) as i32;
    score += (64 - span).max(0);
    let mut run = n > 1;
    for i in 1..n {
        if pos[i] == pos[i - 1] + 1 {
            score += 24;
        } else {
            run = false;
            score -= (pos[i] - pos[i - 1] - 1) as i32;
        }
    }
    if run {
        score += 20;
    }
    for i in 0..n {
        let p = pos[i] as usize;
        if p == 0 || text.chars().nth(p - 1).is_some_and(is_boundary) {
            score += 12;
        }
    }
    score
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

fn ext_marks(name: &str, ext: &str) -> Marks {
    let n = name.chars().count();
    let e = ext.chars().count();
    if e == 0 || n < e {
        return Marks::EMPTY;
    }
    let start = n - e;
    let ok = name
        .chars()
        .skip(start)
        .zip(ext.chars())
        .all(|(a, b)| fold(a) == fold(b));
    if !ok {
        return Marks::EMPTY;
    }
    let mut m = Marks::EMPTY;
    for i in start..n {
        m.set(i);
    }
    m
}

/// Category words people actually type. Query-time only — classify stays cheap.
pub fn category_query(q: &str) -> Option<Category> {
    let q = q.strip_prefix('.').unwrap_or(q);
    if q.eq_ignore_ascii_case("temp") || q.eq_ignore_ascii_case("tmp") {
        Some(Category::Temp)
    } else if q.eq_ignore_ascii_case("cache") || q.eq_ignore_ascii_case("caches") {
        Some(Category::Cache)
    } else if q.eq_ignore_ascii_case("log") || q.eq_ignore_ascii_case("logs") {
        Some(Category::Log)
    } else if q.eq_ignore_ascii_case("journal") {
        Some(Category::Journal)
    } else if q.eq_ignore_ascii_case("crash")
        || q.eq_ignore_ascii_case("dump")
        || q.eq_ignore_ascii_case("coredump")
    {
        Some(Category::Crash)
    } else {
        None
    }
}

/// Score one node against the query. Empty query is a match with score 0
/// (caller sorts those by size). `path` is a display string, usually the
/// node's full path (a suffix of the relative path is enough).
pub fn score_node(query: &str, node: &Node, path: &str) -> Option<(i32, Marks)> {
    let q = query.trim();
    if q.is_empty() {
        return Some((0, Marks::EMPTY));
    }

    let mut best: Option<(i32, Marks)> = None;
    let consider = |best: &mut Option<(i32, Marks)>, score: i32, marks: Marks| {
        if best.as_ref().map_or(true, |(s, _)| score > *s) {
            *best = Some((score, marks));
        }
    };

    if let Some((s, pos)) = fuzzy(q, &node.name) {
        let mut score = s + 30;
        if node.name.eq_ignore_ascii_case(q) {
            score += 40;
        } else if starts_with_ci(&node.name, q) {
            score += 20;
        }
        consider(&mut best, score, pos);
    }

    let ext = file_ext(&node.name);
    if !ext.is_empty() {
        let q_ext = q.strip_prefix('.').unwrap_or(q);
        if ext.eq_ignore_ascii_case(q_ext) {
            consider(&mut best, 90, ext_marks(&node.name, ext));
        } else if let Some((s, _)) = fuzzy(q_ext, ext) {
            consider(&mut best, s + 50, ext_marks(&node.name, ext));
        }
    }

    if let Some(cat) = category_query(q) {
        if node.category == cat {
            consider(&mut best, 85, Marks::EMPTY);
        }
    } else if node.category.is_waste() {
        if let Some((s, _)) = fuzzy(q, node.category.as_str()) {
            consider(&mut best, s + 40, Marks::EMPTY);
        }
        if node.category.label() != node.category.as_str() {
            if let Some((s, _)) = fuzzy(q, node.category.label()) {
                consider(&mut best, s + 40, Marks::EMPTY);
            }
        }
    }

    if let Some((s, _)) = fuzzy(q, path) {
        let marks = best.map(|(_, m)| m).unwrap_or(Marks::EMPTY);
        consider(&mut best, s + 10, marks);
    }

    best
}

/// Path shown in the result list. Only called for visible rows.
pub fn relative_path(tree: &Tree, id: usize) -> Cow<'_, str> {
    let root = &tree.root_node().path;
    let path = &tree.get(id).path;
    match path.strip_prefix(root) {
        Ok(rel) if !rel.as_os_str().is_empty() => {
            let s = rel.to_string_lossy();
            if cfg!(windows) && s.contains('\\') {
                Cow::Owned(s.replace('\\', "/"))
            } else {
                s
            }
        }
        _ => {
            let s = path.to_string_lossy();
            if s.is_empty() {
                Cow::Borrowed(tree.get(id).name.as_str())
            } else if cfg!(windows) && s.contains('\\') {
                Cow::Owned(s.into_owned().replace('\\', "/"))
            } else {
                s
            }
        }
    }
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

fn search_into(
    hits: &mut Vec<FinderHit>,
    stack: &mut Vec<usize>,
    tree: &Tree,
    scope: usize,
    query: &str,
    apparent: bool,
    cap: usize,
) {
    hits.clear();
    stack.clear();
    if cap == 0 || scope >= tree.nodes.len() {
        return;
    }
    if hits.capacity() < cap {
        hits.reserve(cap - hits.capacity());
    }
    stack.extend_from_slice(&tree.get(scope).children);
    while let Some(id) = stack.pop() {
        let node = tree.get(id);
        stack.extend_from_slice(&node.children);
        let path = node.path.to_string_lossy();
        if let Some((score, name_marks)) = score_node(query, node, path.as_ref()) {
            push_top(
                hits,
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
    let mut hits = Vec::new();
    let mut stack = Vec::new();
    search_into(&mut hits, &mut stack, tree, scope, query, apparent, cap);
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

    fn marks_has(m: Marks, idx: &[usize]) -> bool {
        idx.iter().all(|&i| m.contains(i))
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
        assert_eq!(fuzzy("", "anything"), Some((0, Marks::EMPTY)));
    }

    #[test]
    fn missing_letter_is_not_a_match() {
        assert_eq!(fuzzy("xyz", "movie.mp4"), None);
    }

    #[test]
    fn tightens_to_the_extension() {
        let (score, marks) = fuzzy("mp4", "movie.mp4").unwrap();
        assert!(marks_has(marks, &[6, 7, 8]), "prefer the consecutive tail");
        assert!(!marks.contains(5));
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
        let (s, marks) = fuzzy("Rings", "rings").unwrap();
        assert!(marks_has(marks, &[0, 1, 2, 3, 4]));
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
                marks_has(hits[0].name_marks, &[6, 7, 8]),
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

    #[test]
    fn default_scope_is_whole_scan() {
        assert!(Finder::default().whole_scan);
    }
}
