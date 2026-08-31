//! Scan models: arena tree of directories and files.

use std::path::PathBuf;

use crate::classify::Category;

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub parent: Option<usize>,
    pub children: Vec<usize>,
    pub is_dir: bool,
    /// Allocated bytes of this inode only.
    pub own_used: u64,
    /// `st_size` of this inode only.
    pub own_apparent: u64,
    /// Inclusive allocated bytes (self + descendants).
    pub used: u64,
    /// Inclusive apparent bytes (self + descendants).
    pub apparent: u64,
    pub category: Category,
    pub nlink: u64,
}

impl Node {
    pub fn display_size(&self, apparent: bool) -> u64 {
        if apparent {
            self.apparent
        } else {
            self.used
        }
    }

    pub fn kind_str(&self) -> &'static str {
        if self.is_dir {
            "dir"
        } else {
            "file"
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct ScanStats {
    pub files: u64,
    pub dirs: u64,
    pub errors: u64,
    pub skipped_other_fs: u64,
    pub skipped_special: u64,
    pub hardlinks_deduped: u64,
    pub permission_denied: u64,
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub nodes: Vec<Node>,
    pub root: usize,
    pub stats: ScanStats,
}

impl Tree {
    pub fn root_node(&self) -> &Node {
        &self.nodes[self.root]
    }

    pub fn get(&self, id: usize) -> &Node {
        &self.nodes[id]
    }

    pub fn get_mut(&mut self, id: usize) -> &mut Node {
        &mut self.nodes[id]
    }

    /// Walk `cwd` index chain from the root. Invalid indices stop early.
    pub fn node_at(&self, cwd: &[usize]) -> usize {
        let mut id = self.root;
        for &child_pos in cwd {
            let kids = &self.nodes[id].children;
            if child_pos >= kids.len() {
                break;
            }
            id = kids[child_pos];
        }
        id
    }

    pub fn ancestors(&self, mut id: usize) -> Vec<usize> {
        let mut chain = vec![id];
        while let Some(p) = self.nodes[id].parent {
            chain.push(p);
            id = p;
        }
        chain.reverse();
        chain
    }

    /// Subtract a removed child's inclusive sizes from every ancestor.
    pub fn detach(&mut self, id: usize) {
        let used = self.nodes[id].used;
        let apparent = self.nodes[id].apparent;
        let parent = self.nodes[id].parent;
        if let Some(p) = parent {
            self.nodes[p].children.retain(|&c| c != id);
            let mut climb = Some(p);
            while let Some(i) = climb {
                self.nodes[i].used = self.nodes[i].used.saturating_sub(used);
                self.nodes[i].apparent = self.nodes[i].apparent.saturating_sub(apparent);
                climb = self.nodes[i].parent;
            }
        }
        self.nodes[id].parent = None;
        self.nodes[id].children.clear();
    }

    /// Inclusive totals: each parent is own + sum of children.
    pub fn recompute(&mut self) {
        for i in (0..self.nodes.len()).rev() {
            let mut used = self.nodes[i].own_used;
            let mut apparent = self.nodes[i].own_apparent;
            let mut kids = self.nodes[i].children.clone();
            for &c in &kids {
                used = used.saturating_add(self.nodes[c].used);
                apparent = apparent.saturating_add(self.nodes[c].apparent);
            }
            self.nodes[i].used = used;
            self.nodes[i].apparent = apparent;
            kids.sort_by(|&a, &b| {
                self.nodes[b]
                    .used
                    .cmp(&self.nodes[a].used)
                    .then_with(|| self.nodes[a].name.cmp(&self.nodes[b].name))
            });
            self.nodes[i].children = kids;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classify::Category;
    use std::path::PathBuf;

    fn file(name: &str, parent: Option<usize>, used: u64) -> Node {
        Node {
            name: name.into(),
            path: PathBuf::from(name),
            parent,
            children: vec![],
            is_dir: false,
            own_used: used,
            own_apparent: used,
            used,
            apparent: used,
            category: Category::Normal,
            nlink: 1,
        }
    }

    fn dir(name: &str, parent: Option<usize>, own: u64, children: Vec<usize>) -> Node {
        Node {
            name: name.into(),
            path: PathBuf::from(name),
            parent,
            children,
            is_dir: true,
            own_used: own,
            own_apparent: own,
            used: own,
            apparent: own,
            category: Category::Normal,
            nlink: 2,
        }
    }

    #[test]
    fn recompute_sums_children_and_own() {
        // root(own 100) / a(500) / b(dir own 50 + c 200)
        let mut tree = Tree {
            nodes: vec![
                dir("root", None, 100, vec![1, 2]),
                file("a", Some(0), 500),
                dir("b", Some(0), 50, vec![3]),
                file("c", Some(2), 200),
            ],
            root: 0,
            stats: ScanStats::default(),
        };
        tree.recompute();
        assert_eq!(tree.nodes[3].used, 200);
        assert_eq!(tree.nodes[2].used, 250);
        assert_eq!(tree.nodes[1].used, 500);
        assert_eq!(tree.nodes[0].used, 100 + 500 + 250);
        assert_eq!(tree.nodes[0].children[0], 1, "largest child first");
    }

    #[test]
    fn detach_subtracts_from_ancestors() {
        let mut tree = Tree {
            nodes: vec![dir("root", None, 10, vec![1]), file("a", Some(0), 90)],
            root: 0,
            stats: ScanStats::default(),
        };
        tree.recompute();
        assert_eq!(tree.nodes[0].used, 100);
        tree.detach(1);
        assert_eq!(tree.nodes[0].used, 10);
        assert!(tree.nodes[0].children.is_empty());
    }
}
