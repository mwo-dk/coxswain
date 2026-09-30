//! Two versions of a BOM: what is new, gone, better or worse.
//!
//! bom-refs cannot tell what is the same (CBOMkit makes up new UUIDs on every scan), so each
//! node is known by what it is and where it sits:
//!
//! - a crypto asset: its algorithm, key, certificate or protocol, at the nearest node above it
//!   that is not a crypto asset;
//! - a component: its purl without the version (a new version is the same library), else its name;
//! - a group: its source path.
//!
//! These are matched as a multiset: three identical keys in one file match three in the other.

use serde::Serialize;
use std::collections::{HashMap, VecDeque};

use super::model::*;
use super::policy::{normalize, Resolution};
use super::status::Status;
use super::tree::Tree;

/// One version: its BOM, tree, resolutions and statuses.
pub struct Side<'a> {
    pub bom: &'a Bom,
    pub tree: &'a Tree,
    pub resolutions: &'a [Option<Resolution>],
    /// By tree node, from [`super::assess::assess`].
    pub status: &'a [Status],
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Change {
    #[default]
    Unchanged,
    /// New in this version.
    Added,
    /// There before, and rated better now.
    Improved,
    /// There before, and rated worse now.
    Worsened,
}

/// A crypto asset that was there before and is gone.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Removed {
    pub label: String,
    pub kind: NodeKind,
    pub status: Status,
    /// Its first source file, else the node it sat under.
    #[serde(rename = "where")]
    pub place: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub added: usize,
    pub removed: usize,
    pub improved: usize,
    pub worsened: usize,
    /// Risks that went away: removed assets that were not green, and improved ones.
    pub fixed: usize,
    /// Risks that came: added assets that are not green, and worsened ones.
    pub new_risks: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Diff {
    /// By tree node of the newer version.
    pub change: Vec<Change>,
    /// Worst first.
    pub removed: Vec<Removed>,
    pub counts: Counts,
}

fn is_crypto(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Algorithm | NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol)
}

fn green(s: Status) -> bool {
    matches!(s, Status::Safe | Status::Acceptable | Status::NotRated)
}

/// For comparing two ratings: not rated counts as nothing, and unknown sits between green and yellow.
fn severity(s: Status) -> i32 {
    if s == Status::NotRated { 0 } else { s as i32 }
}

fn kind_rank(kind: NodeKind) -> u8 {
    match kind {
        NodeKind::Algorithm => 0,
        NodeKind::Certificate => 1,
        NodeKind::Protocol => 2,
        NodeKind::Material => 3,
        NodeKind::Component => 4,
        NodeKind::Group => 5,
        NodeKind::Application => 6,
    }
}

/// What each tree node is, in a form that is the same across scans.
pub fn identities(side: &Side) -> Vec<String> {
    let (bom, tree) = (side.bom, side.tree);
    let mut algorithm_of: HashMap<u32, u32> = HashMap::new();
    for e in &bom.edges {
        if matches!(e.kind, EdgeKind::UsesAlgorithm | EdgeKind::DependsOn)
            && bom.nodes[e.from as usize].kind == NodeKind::Material
            && bom.nodes[e.to as usize].kind == NodeKind::Algorithm
        {
            algorithm_of.entry(e.from).or_insert(e.to);
        }
    }

    let algorithm = |v: u32| -> String {
        match &side.resolutions[v as usize] {
            Some(Resolution::Rated { parts, .. }) => {
                let mut parts: Vec<String> = parts
                    .iter()
                    .map(|p| {
                        let n = |x: Option<u64>| x.map(|x| x.to_string()).unwrap_or_default();
                        format!("{}:{}:{}:{}", p.algorithm_id, n(p.key_bits), n(p.security_bits), p.param_set.as_deref().unwrap_or(""))
                    })
                    .collect();
                parts.sort();
                parts.join("+")
            }
            _ => format!("?{}", normalize(&bom.nodes[v as usize].label)),
        }
    };

    let what = |v: u32| -> String {
        let node = tree.node(bom, v);
        match (&node.crypto, node.kind) {
            (_, NodeKind::Algorithm) => format!("alg:{}", algorithm(v)),
            (crypto, NodeKind::Material) => {
                let (kind, size) = match crypto {
                    Some(Crypto::Material(m)) => (m.kind.as_deref().unwrap_or(""), m.size.map(|s| s.to_string()).unwrap_or_default()),
                    _ => ("", String::new()),
                };
                let alg = algorithm_of.get(&v).map(|&a| algorithm(a)).unwrap_or_default();
                format!("key:{kind}:{size}:{alg}")
            }
            (crypto, NodeKind::Certificate) => {
                let (subject, issuer) = match crypto {
                    Some(Crypto::Certificate(c)) => (c.subject.as_deref(), c.issuer.as_deref()),
                    _ => (None, None),
                };
                format!("cert:{}|{}", subject.unwrap_or(&node.label), issuer.unwrap_or(""))
            }
            (crypto, NodeKind::Protocol) => {
                let (kind, version) = match crypto {
                    Some(Crypto::Protocol(p)) => (p.kind.as_deref(), p.version.as_deref()),
                    _ => (None, None),
                };
                format!("proto:{}:{}", kind.unwrap_or(&node.label), version.unwrap_or(""))
            }
            (_, NodeKind::Component) => match &node.purl {
                Some(purl) => format!("comp:{}", without_version(purl)),
                None => format!("comp:{}", normalize(&node.label)),
            },
            (_, NodeKind::Group) => {
                let group = node.group.as_ref();
                let kind = match group.map(|g| g.kind) {
                    Some(GroupKind::Directory) => "directory",
                    Some(GroupKind::File) => "file",
                    _ => "kind",
                };
                format!("group:{kind}:{}", group.and_then(|g| g.path.as_deref()).unwrap_or(&node.label))
            }
            (_, NodeKind::Application) => "app".to_string(),
        }
    };

    // Pre-order, so a node's parent has its identity before the node needs it.
    let mut out = vec![String::new(); tree.len()];
    for &v in &tree.order {
        if !is_crypto(tree.node(bom, v).kind) {
            out[v as usize] = what(v);
            continue;
        }
        let mut p = tree.parent[v as usize];
        while let Some(q) = p
            && is_crypto(tree.node(bom, q).kind)
        {
            p = tree.parent[q as usize];
        }
        let at = p.map_or("", |p| out[p as usize].as_str());
        out[v as usize] = format!("{} @ {at}", what(v));
    }
    out
}

/// A purl without its version: "pkg:maven/org.x/lib@1.2" is "pkg:maven/org.x/lib".
fn without_version(purl: &str) -> &str {
    match purl.rfind('@') {
        Some(at) if !purl[at + 1..].contains('/') => &purl[..at],
        _ => purl,
    }
}

/// Compares an older version with a newer one.
pub fn diff(before: &Side, after: &Side) -> Diff {
    let id_before = identities(before);
    let id_after = identities(after);

    let mut pool: HashMap<&str, VecDeque<usize>> = HashMap::new();
    for (i, id) in id_before.iter().enumerate() {
        pool.entry(id.as_str()).or_default().push_back(i);
    }

    let mut change = vec![Change::Unchanged; after.tree.len()];
    let mut counts = Counts::default();
    let mut matched = vec![false; before.tree.len()];

    for (v, id) in id_after.iter().enumerate() {
        let crypto = is_crypto(after.tree.node(after.bom, v as u32).kind);
        let Some(old) = pool.get_mut(id.as_str()).and_then(VecDeque::pop_front) else {
            if crypto {
                // new folders and components are context, not findings
                change[v] = Change::Added;
                counts.added += 1;
                if !green(after.status[v]) {
                    counts.new_risks += 1;
                }
            }
            continue;
        };
        matched[old] = true;
        if !crypto {
            continue;
        }
        match severity(after.status[v]).cmp(&severity(before.status[old])) {
            std::cmp::Ordering::Less => {
                change[v] = Change::Improved;
                counts.improved += 1;
                counts.fixed += 1;
            }
            std::cmp::Ordering::Greater => {
                change[v] = Change::Worsened;
                counts.worsened += 1;
                counts.new_risks += 1;
            }
            std::cmp::Ordering::Equal => {}
        }
    }

    let mut removed = vec![];
    for (i, seen) in matched.iter().enumerate() {
        let node = before.tree.node(before.bom, i as u32);
        if *seen || !is_crypto(node.kind) {
            continue;
        }
        let status = before.status[i];
        let place = match node.occurrences.first() {
            Some(o) => o.location.clone(),
            None => id_before[i].split(" @ ").nth(1).unwrap_or("").to_string(),
        };
        removed.push(Removed { label: node.label.clone(), kind: node.kind, status, place });
        counts.removed += 1;
        if !green(status) {
            counts.fixed += 1;
        }
    }
    // worst first; on a tie, an algorithm before the keys and certificates that inherit from it
    removed.sort_by(|a, b| severity(b.status).cmp(&severity(a.status)).then(kind_rank(a.kind).cmp(&kind_rank(b.kind))));

    Diff { change, removed, counts }
}
