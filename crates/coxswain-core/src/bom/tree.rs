//! The tree the views show. A BOM is a graph, and scanner CBOMs often have no component
//! structure at all, only crypto assets and their source files. A node is placed, in order:
//!
//! 1. by the mode: the dependency tree from the root, or the source file it is found in;
//! 2. under a placed node that refers to it (a certificate's signature algorithm, a key's
//!    algorithm, …), until nothing changes;
//! 3. in a group per kind of asset under the root, for what is left.
//!
//! Every edge of the BOM that is not a tree edge is kept as a cross edge.

use serde::Serialize;
use std::collections::{HashMap, HashSet};

use super::model::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TreeMode {
    /// Components and what they contain, depend on and provide.
    Dependencies,
    /// Directories and source files, from each asset's first occurrence.
    Files,
    /// One group per kind of asset.
    Flat,
}

#[derive(Clone, Debug)]
pub struct Tree {
    pub mode: TreeMode,
    /// The groups the tree made. Node `i` is `bom.nodes[i]` below `bom.nodes.len()`, and a
    /// group above it: see [`Tree::node`].
    pub groups: Vec<Node>,
    /// The parent of each node; `None` for the root.
    pub parent: Vec<Option<u32>>,
    pub children: Vec<Vec<u32>>,
    pub depth: Vec<u32>,
    /// Pre-order from the root: every parent comes before its children.
    pub order: Vec<u32>,
    /// The BOM's edges that are not tree edges, and `OccursIn` edges for further occurrences.
    pub cross_edges: Vec<Edge>,
}

impl Tree {
    pub fn node<'a>(&'a self, bom: &'a Bom, i: u32) -> &'a Node {
        let i = i as usize;
        bom.nodes.get(i).unwrap_or_else(|| &self.groups[i - bom.nodes.len()])
    }

    pub fn len(&self) -> usize {
        self.parent.len()
    }

    pub fn is_empty(&self) -> bool {
        self.parent.is_empty()
    }
}

fn structural(kind: EdgeKind) -> bool {
    matches!(kind, EdgeKind::Contains | EdgeKind::DependsOn | EdgeKind::Provides)
}

/// The modes this BOM can be shown in, best first. The view offers a switch when there is more than one.
pub fn modes(bom: &Bom) -> Vec<TreeMode> {
    let mut modes = vec![];
    let component_tree = bom.edges.iter().any(|e| {
        structural(e.kind) && matches!(bom.nodes[e.from as usize].kind, NodeKind::Application | NodeKind::Component)
    });
    if component_tree {
        modes.push(TreeMode::Dependencies);
    }
    if bom.nodes.iter().any(|n| !n.occurrences.is_empty()) {
        modes.push(TreeMode::Files);
    }
    modes.push(TreeMode::Flat);
    modes
}

/// While building: a node's parent, or that it has none yet.
const UNPLACED: u32 = u32::MAX;
const ROOT: u32 = u32::MAX - 1;

struct Builder {
    first_group: usize,
    groups: Vec<Node>,
    parent: Vec<u32>,
    tree_edges: HashSet<Edge>,
}

impl Builder {
    fn add_group(&mut self, label: String, kind: GroupKind, path: Option<String>, parent: u32) -> u32 {
        let idx = (self.first_group + self.groups.len()) as u32;
        let tag = match kind {
            GroupKind::Directory => "directory",
            GroupKind::File => "file",
            GroupKind::Kind => "kind",
        };
        let key = format!("#group:{tag}:{}", path.as_deref().unwrap_or(&label));
        let mut node = Node::synthetic(key, NodeKind::Group, label);
        node.group = Some(GroupInfo { kind, path });
        self.groups.push(node);
        self.parent.push(parent);
        idx
    }

    fn place(&mut self, node: u32, parent: u32, via: Option<Edge>) {
        self.parent[node as usize] = parent;
        if let Some(edge) = via {
            self.tree_edges.insert(edge);
        }
    }

    fn placed(&self, node: u32) -> bool {
        self.parent[node as usize] != UNPLACED
    }

    /// Hangs unplaced nodes under a placed node that refers to them, until nothing changes.
    fn attach(&mut self, bom: &Bom) {
        let mut changed = true;
        while changed {
            changed = false;
            for &e in &bom.edges {
                if self.placed(e.from) && !self.placed(e.to) {
                    self.place(e.to, e.from, Some(e));
                    changed = true;
                }
            }
        }
    }
}

pub fn build(bom: &Bom, mode: TreeMode) -> Tree {
    let n = bom.nodes.len();
    let mut b = Builder { first_group: n, groups: vec![], parent: vec![UNPLACED; n], tree_edges: HashSet::new() };
    b.parent[0] = ROOT;
    let mut extra_edges = vec![];

    let mut out: Vec<Vec<Edge>> = vec![vec![]; n];
    for &e in &bom.edges {
        out[e.from as usize].push(e);
    }

    // 1. by the mode
    match mode {
        TreeMode::Dependencies => {
            let mut queue = vec![0u32];
            let mut q = 0;
            while q < queue.len() {
                let u = queue[q];
                q += 1;
                for &e in &out[u as usize] {
                    if structural(e.kind) && !b.placed(e.to) {
                        b.place(e.to, u, Some(e));
                        queue.push(e.to);
                    }
                }
            }
        }
        TreeMode::Files => place_by_source_file(bom, &mut b, &mut extra_edges),
        TreeMode::Flat => {}
    }

    // 2. under whatever refers to them
    b.attach(bom);

    // 3. what is left, sources first (nodes no unplaced node points at), so that what they
    // point at can still attach below them
    let mut kind_groups: HashMap<NodeKind, u32> = HashMap::new();
    loop {
        let unplaced: Vec<u32> = (0..n as u32).filter(|&i| !b.placed(i)).collect();
        if unplaced.is_empty() {
            break;
        }
        let pointed_at: HashSet<u32> = bom.edges.iter().filter(|e| !b.placed(e.from)).map(|e| e.to).collect();
        let sources: Vec<u32> = unplaced.iter().copied().filter(|i| !pointed_at.contains(i)).collect();
        let now = if sources.is_empty() { &unplaced[..1] } else { &sources[..] };
        for &i in now {
            let kind = bom.nodes[i as usize].kind;
            let group = match kind_groups.get(&kind) {
                Some(&g) => g,
                None => {
                    let g = b.add_group(kind_label(kind).into(), GroupKind::Kind, None, 0);
                    kind_groups.insert(kind, g);
                    g
                }
            };
            b.place(i, group, None);
        }
        b.attach(bom);
    }

    // The tree's arrays
    let total = b.parent.len();
    let parent: Vec<Option<u32>> = b.parent.iter().map(|&p| (p != ROOT).then_some(p)).collect();
    let mut children = vec![vec![]; total];
    for (i, p) in parent.iter().enumerate().skip(1) {
        children[p.expect("everything is placed") as usize].push(i as u32);
    }
    let mut depth = vec![0; total];
    let mut order = Vec::with_capacity(total);
    let mut stack = vec![0u32];
    while let Some(v) = stack.pop() {
        order.push(v);
        for &c in children[v as usize].iter().rev() {
            depth[c as usize] = depth[v as usize] + 1;
            stack.push(c);
        }
    }

    let mut cross_edges: Vec<Edge> = bom.edges.iter().copied().filter(|e| !b.tree_edges.contains(e)).collect();
    cross_edges.extend(extra_edges);
    Tree { mode, groups: b.groups, parent, children, depth, order, cross_edges }
}

/// The label of a group per kind. The views translate it by the group's kind; this is the fallback.
fn kind_label(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Application => "Applications",
        NodeKind::Group => "Groups",
        NodeKind::Component => "Components",
        NodeKind::Algorithm => "Algorithms",
        NodeKind::Certificate => "Certificates",
        NodeKind::Protocol => "Protocols",
        NodeKind::Material => "Keys & secrets",
    }
}

#[derive(Default)]
struct Dir {
    dirs: Vec<(String, usize)>,
    dir_index: HashMap<String, usize>,
    /// File name and the assets whose first occurrence it is.
    files: Vec<(String, Vec<u32>)>,
    file_index: HashMap<String, usize>,
}

/// The directory and file entry of a source path, made on first sight.
fn file_slot(dirs: &mut Vec<Dir>, location: &str) -> (usize, usize) {
    let segments: Vec<&str> = location.split('/').filter(|s| !s.is_empty()).collect();
    let mut d = 0;
    for &s in segments.iter().take(segments.len().saturating_sub(1)) {
        d = match dirs[d].dir_index.get(s) {
            Some(&next) => next,
            None => {
                let next = dirs.len();
                dirs.push(Dir::default());
                dirs[d].dirs.push((s.to_string(), next));
                dirs[d].dir_index.insert(s.to_string(), next);
                next
            }
        };
    }
    let file = segments.last().copied().unwrap_or(location);
    let f = match dirs[d].file_index.get(file) {
        Some(&f) => f,
        None => {
            let f = dirs[d].files.len();
            dirs[d].files.push((file.to_string(), vec![]));
            dirs[d].file_index.insert(file.to_string(), f);
            f
        }
    };
    (d, f)
}

/// Groups assets by source path: root, directories, file, asset. A chain of directories with
/// one child each becomes one group ("services/src/main/java/org/keycloak"). Each asset sits
/// under the file of its first occurrence; the other files get an `OccursIn` edge to it when
/// they have a group of their own. A file where assets only occur again gets none: it would be
/// an empty row in the tree (cipherscape keeps it, for its graph).
fn place_by_source_file(bom: &Bom, b: &mut Builder, extra_edges: &mut Vec<Edge>) {
    let mut dirs = vec![Dir::default()];
    let mut further: Vec<(String, u32)> = vec![];
    for (i, node) in bom.nodes.iter().enumerate() {
        let Some(first) = node.occurrences.first().map(|o| o.location.as_str()) else { continue };
        let (d, f) = file_slot(&mut dirs, first);
        dirs[d].files[f].1.push(i as u32);
        let mut seen = HashSet::new();
        for o in &node.occurrences[1..] {
            if o.location != first && seen.insert(o.location.as_str()) {
                further.push((o.location.clone(), i as u32));
            }
        }
    }

    // Depth first without recursion: a path of a hundred thousand segments costs no stack.
    enum Work {
        /// A subdirectory: make its group (after folding single-child chains), then fill it.
        Dir { name: String, dir: usize, path: String, parent: u32 },
        /// A directory's files, after all its subdirectories.
        Files { dir: usize, path: String, parent: u32 },
    }
    let join = |path: &str, name: &str| if path.is_empty() { name.to_string() } else { format!("{path}/{name}") };
    let mut file_group: HashMap<String, u32> = HashMap::new();
    let mut work = vec![Work::Files { dir: 0, path: String::new(), parent: 0 }];
    work.extend(dirs[0].dirs.iter().rev().map(|(name, d)| Work::Dir { name: name.clone(), dir: *d, path: String::new(), parent: 0 }));
    while let Some(item) = work.pop() {
        match item {
            Work::Dir { name, mut dir, path, parent } => {
                let mut full = join(&path, &name);
                let mut label = name;
                while dirs[dir].files.is_empty() && dirs[dir].dirs.len() == 1 {
                    let (child_name, child) = &dirs[dir].dirs[0];
                    full = format!("{full}/{child_name}");
                    label = format!("{label}/{child_name}");
                    dir = *child;
                }
                let g = b.add_group(label, GroupKind::Directory, Some(full.clone()), parent);
                work.push(Work::Files { dir, path: full.clone(), parent: g });
                work.extend(
                    dirs[dir].dirs.iter().rev().map(|(n, d)| Work::Dir { name: n.clone(), dir: *d, path: full.clone(), parent: g }),
                );
            }
            Work::Files { dir, path, parent } => {
                for (name, assets) in &dirs[dir].files {
                    let full = join(&path, name);
                    let g = b.add_group(name.clone(), GroupKind::File, Some(full.clone()), parent);
                    file_group.insert(full, g);
                    for &a in assets {
                        b.place(a, g, None);
                    }
                }
            }
        }
    }

    for (path, asset) in further {
        if let Some(&g) = file_group.get(&path) {
            extra_edges.push(Edge { from: g, to: asset, kind: EdgeKind::OccursIn });
        }
    }
}
