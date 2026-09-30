//! What both apps' BOM views need beyond the model: a BOM read, rated and laid out as a tree in
//! one go, names to show, filters and the sunburst's geometry. The desktop app filters and draws
//! in JavaScript (gui/src/bom.js, the same rules); the terminal app uses these.

use std::collections::HashSet;
use std::path::Path;

use super::assess::{self, Assessed, Context};
use super::model::*;
use super::policy::{policy, Family, Resolution};
use super::status::Status;
use super::tree::{self, Tree, TreeMode};
use super::Error;

/// A BOM read, resolved, rated and laid out.
pub struct Loaded {
    pub bom: Bom,
    pub tree: Tree,
    pub mode: TreeMode,
    pub resolutions: Vec<Option<Resolution>>,
    /// From resolving: names nothing knows, and outvoted OIDs.
    pub issues: Vec<Issue>,
    pub assessed: Assessed,
    pub ctx: Context,
    /// Each BOM node's outgoing edges.
    pub out: Vec<Vec<(EdgeKind, u32)>>,
}

impl Loaded {
    /// Reads `path` and lays it out in `mode`, or in its best mode when it has not got that one.
    pub fn open(path: &Path, mode: Option<TreeMode>) -> Result<Loaded, Error> {
        Ok(Loaded::new(super::load(path)?, mode))
    }

    pub fn new(bom: Bom, mode: Option<TreeMode>) -> Loaded {
        let modes = tree::modes(&bom);
        let mode = mode.filter(|m| modes.contains(m)).unwrap_or(modes[0]);
        let tree = tree::build(&bom, mode);
        let p = policy();
        let (resolutions, issues) = assess::resolve(&bom, p);
        let ctx = Context::today(p);
        let assessed = assess::assess(&bom, &tree, p, &resolutions, &ctx);
        let mut out = vec![vec![]; bom.nodes.len()];
        for e in &bom.edges {
            out[e.from as usize].push((e.kind, e.to));
        }
        Loaded { bom, tree, mode, resolutions, issues, assessed, ctx, out }
    }

    /// This BOM as one side of a compare.
    pub fn side(&self) -> super::diff::Side<'_> {
        super::diff::Side { bom: &self.bom, tree: &self.tree, resolutions: &self.resolutions, status: &self.assessed.status }
    }

    pub fn node(&self, i: u32) -> &Node {
        self.tree.node(&self.bom, i)
    }

    pub fn status(&self, i: u32) -> Status {
        self.assessed.status[i as usize]
    }

    /// The kinds of cryptography a node is: an algorithm's own, from the catalog, and those of
    /// the algorithms a key, certificate or protocol refers to, two steps deep.
    pub fn families(&self, i: u32) -> Vec<Family> {
        let mut found = std::collections::BTreeSet::new();
        if i as usize >= self.bom.nodes.len() {
            return vec![];
        }
        let mut todo = vec![(i, 0)];
        while let Some((n, depth)) = todo.pop() {
            match self.bom.nodes[n as usize].kind {
                NodeKind::Algorithm => {
                    if let Some(Resolution::Rated { parts, .. }) = &self.resolutions[n as usize] {
                        found.extend(parts.iter().filter_map(|p| policy().algorithm(&p.algorithm_id)).map(|a| a.family));
                    }
                }
                NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol if depth < 2 => {
                    todo.extend(self.out[n as usize].iter().filter(|(k, _)| *k != EdgeKind::Contains).map(|&(_, to)| (to, depth + 1)));
                }
                _ => {}
            }
        }
        found.into_iter().collect()
    }

    /// A key a scanner named "secret-key@<uuid>": its type and its algorithm's label, to show instead.
    pub fn key_of(&self, i: u32) -> Option<(String, String)> {
        let node = self.bom.nodes.get(i as usize)?;
        let (prefix, rest) = node.label.split_once('@')?;
        let uuid = rest.len() == 36 && rest.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-');
        if node.kind != NodeKind::Material || !uuid {
            return None;
        }
        let kind = match &node.crypto {
            Some(Crypto::Material(m)) => m.kind.clone(),
            _ => None,
        }
        .unwrap_or_else(|| prefix.to_string());
        let &(_, alg) = self.out[i as usize].iter().find(|(_, to)| self.bom.nodes[*to as usize].kind == NodeKind::Algorithm)?;
        Some((kind, self.bom.nodes[alg as usize].label.clone()))
    }

    /// For a group by kind: the kind, which names it in the user's language.
    pub fn group_kind(&self, i: u32) -> Option<NodeKind> {
        let group = self.node(i).group.as_ref()?;
        if group.kind != GroupKind::Kind {
            return None;
        }
        self.tree.children[i as usize].first().map(|&c| self.node(c).kind)
    }

    /// What a search looks in, lower case: label, OID and source paths.
    pub fn search_text(&self, i: u32) -> String {
        let node = self.node(i);
        let mut q = node.label.to_lowercase();
        if let Some(Crypto::Algorithm(a)) = &node.crypto
            && let Some(oid) = &a.oid
        {
            q.push('\n');
            q.push_str(oid);
        }
        for o in &node.occurrences {
            q.push('\n');
            q.push_str(&o.location.to_lowercase());
        }
        q
    }
}

/// A path in the BOM, relative to the scanned repository, as a file next to the BOM, when it is
/// there. Never a path that climbs out of the BOM's folder.
pub fn on_disk(bom_path: &Path, location: &str) -> Option<std::path::PathBuf> {
    let base = bom_path.parent()?;
    let rel = location.trim_start_matches(['/', '\\']);
    if rel.is_empty() || rel.split(['/', '\\']).any(|s| s == "..") {
        return None;
    }
    let p = base.join(rel);
    p.exists().then_some(p)
}

pub fn is_crypto(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Algorithm | NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol)
}

/// What to show. Empty sets let everything through.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub status: HashSet<Status>,
    pub kind: HashSet<NodeKind>,
    pub family: HashSet<Family>,
    /// Lower case.
    pub query: String,
}

impl Filter {
    pub fn is_active(&self) -> bool {
        !self.status.is_empty() || !self.kind.is_empty() || !self.family.is_empty() || !self.query.trim().is_empty()
    }
}

/// Which tree nodes match (`.0`), and which have a match at or below them (`.1`), so that what a
/// match sits in stays visible. Status and family filters look at crypto assets only; the search
/// also finds groups and components by name. `text` is what the search looks in, per node.
pub fn mask(l: &Loaded, f: &Filter, text: &[String]) -> (Vec<bool>, Vec<bool>) {
    let n = l.tree.len();
    let q = f.query.trim().to_lowercase();
    let only_assets = !f.status.is_empty() || !f.family.is_empty();
    let matches: Vec<bool> = (0..n as u32)
        .map(|i| {
            let node = l.node(i);
            (!only_assets || is_crypto(node.kind))
                && (f.status.is_empty() || f.status.contains(&l.status(i)))
                && (f.kind.is_empty() || f.kind.contains(&node.kind))
                && (f.family.is_empty() || l.families(i).iter().any(|x| f.family.contains(x)))
                && (q.is_empty() || text[i as usize].contains(&q))
        })
        .collect();
    let mut keep = matches.clone();
    for &i in l.tree.order.iter().rev() {
        if keep[i as usize]
            && let Some(p) = l.tree.parent[i as usize]
        {
            keep[p as usize] = true;
        }
    }
    (matches, keep)
}

/// Leaves beneath each tree node (a leaf counts itself); with `keep`, nodes outside it count nothing.
pub fn leaf_counts(tree: &Tree, keep: Option<&[bool]>) -> Vec<f64> {
    let mut leaves = vec![0.0; tree.len()];
    for &i in tree.order.iter().rev() {
        if keep.is_some_and(|k| !k[i as usize]) {
            continue;
        }
        let kids = &tree.children[i as usize];
        leaves[i as usize] = if kids.is_empty() { 1.0 } else { kids.iter().map(|&c| leaves[c as usize]).sum() };
    }
    leaves
}

/// One arc of a sunburst, in degrees clockwise from the top. `node` is `None` for thin siblings
/// merged into one arc under `parent`, with their worst status.
#[derive(Clone, Debug, PartialEq)]
pub struct Arc {
    pub node: Option<u32>,
    pub parent: u32,
    /// How many nodes it stands for: 1, or the thin siblings merged.
    pub count: usize,
    pub status: Status,
    /// 1 is the first ring around the middle.
    pub depth: usize,
    pub a0: f64,
    pub a1: f64,
}

pub const RINGS: usize = 6;
pub const MIN_ANGLE: f64 = 0.5;

/// The arcs below `root`, at most `rings` deep, thin ones merged (see gui/src/bom.js).
pub fn sunburst(l: &Loaded, leaves: &[f64], root: u32, rings: usize, min_angle: f64) -> Vec<Arc> {
    let mut arcs = vec![];
    let mut stack = vec![(root, 0usize, 0.0, 360.0)];
    while let Some((p, depth, a0, a1)) = stack.pop() {
        if depth >= rings || leaves[p as usize] == 0.0 {
            continue;
        }
        let scale = (a1 - a0) / leaves[p as usize];
        let mut at = a0;
        let mut merged: Option<Arc> = None;
        for &c in &l.tree.children[p as usize] {
            let span = leaves[c as usize] * scale;
            if span == 0.0 {
                continue;
            }
            if span < min_angle {
                let m = merged.get_or_insert(Arc {
                    node: None,
                    parent: p,
                    count: 0,
                    status: Status::NotRated,
                    depth: depth + 1,
                    a0: at,
                    a1: at,
                });
                m.count += 1;
                m.a1 += span;
                m.status = m.status.worst(l.status(c));
            } else {
                arcs.push(Arc { node: Some(c), parent: p, count: 1, status: l.status(c), depth: depth + 1, a0: at, a1: at + span });
                stack.push((c, depth + 1, at, at + span));
            }
            at += span;
        }
        arcs.extend(merged);
    }
    arcs
}
