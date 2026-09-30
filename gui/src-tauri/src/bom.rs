//! The BOM view (docs/design/bom-viewer.md): a CycloneDX BOM as a rated tree, and the details
//! of one node. Parsing and rating are coxswain-core's `bom`; this shapes them for BomView.svelte.
//!
//! The last BOM read is kept, by path, modification time and tree mode, so selecting nodes
//! and switching between the preview and its window never reads the file again.

use coxswain_core::bom::assess::{self, Assessed, Context, Reason, Unresolved};
use coxswain_core::bom::policy::{policy, Family, Param, Resolution};
use coxswain_core::bom::{self, tree, Bom, Crypto, EdgeKind, GroupKind, Issue, NodeKind, Status, Tree, TreeMode};
use coxswain_core::t;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type Res<T> = Result<T, String>;

struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    mode: TreeMode,
    bom: Bom,
    tree: Tree,
    resolutions: Vec<Option<Resolution>>,
    issues: Vec<Issue>,
    assessed: Assessed,
    ctx: Context,
}

static LAST: Mutex<Option<Arc<Loaded>>> = Mutex::new(None);

fn mode_of(name: Option<&str>) -> Option<TreeMode> {
    match name? {
        "dependencies" => Some(TreeMode::Dependencies),
        "files" => Some(TreeMode::Files),
        "flat" => Some(TreeMode::Flat),
        _ => None,
    }
}

fn error_text(e: bom::Error) -> String {
    match e {
        bom::Error::NotCycloneDx => t!("err.bom.not_cyclonedx"),
        bom::Error::InvalidJson { line, message } => t!("err.bom.invalid_json", "line" => line, "message" => message),
        bom::Error::InvalidXml { line, message } => t!("err.bom.invalid_xml", "line" => line, "message" => message),
        bom::Error::UnsafeXml => t!("err.bom.unsafe_xml"),
        bom::Error::TooLarge(_) => t!("err.bom.too_large"),
        bom::Error::Io(e) => e,
    }
}

/// The BOM at `path`, from the cache or read, parsed, resolved and rated anew.
fn loaded(path: &Path, mode: Option<&str>) -> Res<Arc<Loaded>> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let want = mode_of(mode);
    if let Some(l) = LAST.lock().unwrap().as_ref()
        && l.path == path
        && l.modified == modified
        && want.is_none_or(|m| m == l.mode)
    {
        return Ok(l.clone());
    }
    let bom = bom::load(path).map_err(error_text)?;
    let modes = tree::modes(&bom);
    let mode = want.filter(|m| modes.contains(m)).unwrap_or(modes[0]);
    let tree = tree::build(&bom, mode);
    let p = policy();
    let (resolutions, issues) = assess::resolve(&bom, p);
    let ctx = Context::today(p);
    let assessed = assess::assess(&bom, &tree, p, &resolutions, &ctx);
    let l = Arc::new(Loaded { path: path.to_path_buf(), modified, mode, bom, tree, resolutions, issues, assessed, ctx });
    *LAST.lock().unwrap() = Some(l.clone());
    Ok(l)
}

/// One row of the tree.
#[derive(Serialize)]
pub struct Row {
    /// Label, kind, status, parent (-1 for the root).
    l: String,
    k: NodeKind,
    s: Status,
    p: i64,
    /// Where it is first found, as "File.java:88".
    #[serde(skip_serializing_if = "Option::is_none")]
    o: Option<String>,
    /// For a group: directory, file or kind.
    #[serde(skip_serializing_if = "Option::is_none")]
    g: Option<GroupKind>,
    /// For a group by kind: the kind, which names it in the user's language.
    #[serde(skip_serializing_if = "Option::is_none")]
    gk: Option<NodeKind>,
    /// The kinds of cryptography it is, for the family filter.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    f: Vec<Family>,
    /// For a key a scanner named by a UUID: its type and its algorithm, to show instead.
    #[serde(skip_serializing_if = "Option::is_none")]
    key: Option<(String, String)>,
    /// What the search box looks in, lower case: label, OID and source paths.
    q: String,
}

#[derive(Serialize)]
pub struct BomView {
    format: bom::Format,
    spec_version: String,
    modes: Vec<TreeMode>,
    mode: TreeMode,
    rows: Vec<Row>,
    /// Pre-order: every parent before its children, siblings in order.
    order: Vec<u32>,
    issues: Vec<Issue>,
    /// Crypto assets only, not the groups above them.
    by_status: BTreeMap<Status, usize>,
    by_kind: BTreeMap<String, usize>,
    by_family: BTreeMap<Family, usize>,
    /// The profile's name and year the ratings are for, and whether the catalog was reviewed.
    profile: String,
    year: i32,
    reviewed: bool,
}

fn is_crypto(kind: NodeKind) -> bool {
    matches!(kind, NodeKind::Algorithm | NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol)
}

fn kind_name(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Application => "application",
        NodeKind::Group => "group",
        NodeKind::Component => "component",
        NodeKind::Algorithm => "algorithm",
        NodeKind::Certificate => "certificate",
        NodeKind::Protocol => "protocol",
        NodeKind::Material => "material",
    }
}

/// The families of an algorithm, from the catalog entries it resolved to.
fn algorithm_families(l: &Loaded, i: u32) -> Vec<Family> {
    match &l.resolutions[i as usize] {
        Some(Resolution::Rated { parts, .. }) => parts.iter().filter_map(|p| policy().algorithm(&p.algorithm_id)).map(|a| a.family).collect(),
        _ => vec![],
    }
}

/// The families of any node: an algorithm's own, and those of the algorithms a key,
/// certificate or protocol refers to, two steps deep (a certificate's key's algorithm).
fn families(l: &Loaded, i: u32, out_edges: &[Vec<(EdgeKind, u32)>]) -> Vec<Family> {
    let mut found = BTreeSet::new();
    let mut todo = vec![(i, 0)];
    while let Some((n, depth)) = todo.pop() {
        match l.bom.nodes[n as usize].kind {
            NodeKind::Algorithm => found.extend(algorithm_families(l, n)),
            NodeKind::Material | NodeKind::Certificate | NodeKind::Protocol if depth < 2 => {
                todo.extend(out_edges[n as usize].iter().filter(|(k, _)| *k != EdgeKind::Contains).map(|&(_, to)| (to, depth + 1)));
            }
            _ => {}
        }
    }
    found.into_iter().collect()
}

/// A key CBOMkit named "secret-key@<uuid>": its type and its algorithm's label.
fn uuid_key(l: &Loaded, i: u32, out_edges: &[Vec<(EdgeKind, u32)>]) -> Option<(String, String)> {
    let node = &l.bom.nodes[i as usize];
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
    let alg = out_edges[i as usize].iter().find(|(_, to)| l.bom.nodes[*to as usize].kind == NodeKind::Algorithm)?;
    Some((kind, l.bom.nodes[alg.1 as usize].label.clone()))
}

fn short_place(o: &bom::Occurrence) -> String {
    let file = o.location.rsplit('/').next().unwrap_or(&o.location);
    match o.line {
        Some(line) => format!("{file}:{line}"),
        None => file.to_string(),
    }
}

/// A BOM as a rated tree. `mode` is the grouping: dependencies, files or flat (default: the best).
#[tauri::command]
pub async fn bom_info(path: PathBuf, mode: Option<String>) -> Res<BomView> {
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path, mode.as_deref())?;
        let (bom, tree) = (&l.bom, &l.tree);
        let mut out_edges = vec![vec![]; bom.nodes.len()];
        for e in &bom.edges {
            out_edges[e.from as usize].push((e.kind, e.to));
        }

        let mut by_status = BTreeMap::new();
        let mut by_kind = BTreeMap::new();
        let mut by_family = BTreeMap::new();
        let rows = (0..tree.len() as u32)
            .map(|i| {
                let node = tree.node(bom, i);
                let status = l.assessed.status[i as usize];
                let own = (i as usize) < bom.nodes.len();
                let f = if own { families(&l, i, &out_edges) } else { vec![] };
                if is_crypto(node.kind) {
                    *by_status.entry(status).or_insert(0) += 1;
                    for x in &f {
                        *by_family.entry(*x).or_insert(0) += 1;
                    }
                }
                if node.kind != NodeKind::Group && i != 0 {
                    *by_kind.entry(kind_name(node.kind).to_string()).or_insert(0) += 1;
                }
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
                Row {
                    l: node.label.clone(),
                    k: node.kind,
                    s: status,
                    p: tree.parent[i as usize].map_or(-1, i64::from),
                    o: node.occurrences.first().map(short_place),
                    g: node.group.as_ref().map(|g| g.kind),
                    gk: (node.group.as_ref().map(|g| g.kind) == Some(GroupKind::Kind))
                        .then(|| tree.children[i as usize].first().map(|&c| tree.node(bom, c).kind))
                        .flatten(),
                    f,
                    key: if own { uuid_key(&l, i, &out_edges) } else { None },
                    q,
                }
            })
            .collect();

        let catalog = policy().catalog();
        Ok(BomView {
            format: bom.source.format,
            spec_version: bom.source.spec_version.clone(),
            modes: tree::modes(bom),
            mode: l.mode,
            rows,
            order: tree.order.clone(),
            issues: bom.issues.iter().chain(&l.issues).cloned().collect(),
            by_status,
            by_kind,
            by_family,
            profile: catalog.profiles.get(&l.ctx.profile).map_or_else(|| l.ctx.profile.clone(), |p| p.name.clone()),
            year: l.ctx.year,
            reviewed: catalog.last_reviewed.is_some(),
        })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// One reason, flat, for the details box to word in the user's language.
#[derive(Serialize, Default)]
pub struct Why {
    /// rule, not-rated, unresolved, inherited, reference, lifecycle or rollup.
    kind: &'static str,
    status: Option<Status>,
    /// The algorithm's name in the catalog ("SHA-2"), and the parameters found for it.
    algorithm: Option<String>,
    key_bits: Option<u64>,
    security_bits: Option<u64>,
    param_set: Option<String>,
    /// What a rule needed and the asset lacks.
    missing: Vec<Param>,
    source: Option<String>,
    note: Option<String>,
    /// The node this reason is about: a referenced asset, or the one that sets a roll-up.
    node: Option<u32>,
    node_label: Option<String>,
    edge: Option<EdgeKind>,
    days_left: Option<i64>,
    primitive: Option<String>,
    unresolved: Option<Unresolved>,
}

#[derive(Serialize)]
pub struct Place {
    location: String,
    line: Option<u64>,
    context: Option<String>,
    /// The file on disk, when it is where the BOM says, next to the BOM.
    path: Option<String>,
}

#[derive(Serialize)]
pub struct NodeDetails {
    label: String,
    kind: NodeKind,
    status: Status,
    why: Vec<Why>,
    /// The catalog's advice for what is not green.
    advice: Vec<String>,
    found_in: Vec<Place>,
    /// A group's source file or directory on disk, when it is there.
    group_path: Option<String>,
    /// The node as the file has it, as JSON.
    raw: Option<String>,
}

/// A path in the BOM, relative to the scanned repository, as a file next to the BOM.
fn on_disk(bom_path: &Path, location: &str) -> Option<String> {
    let base = bom_path.parent()?;
    let rel = location.trim_start_matches(['/', '\\']);
    if rel.is_empty() || rel.split(['/', '\\']).any(|s| s == "..") {
        return None;
    }
    let p = base.join(rel);
    p.exists().then(|| p.to_string_lossy().into_owned())
}

const RAW_LIMIT: usize = 64 * 1024;

/// The details of tree node `index`: why it has its status, where it is found, and its JSON.
#[tauri::command]
pub async fn bom_node(path: PathBuf, mode: Option<String>, index: u32) -> Res<NodeDetails> {
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path, mode.as_deref())?;
        let (bom, tree) = (&l.bom, &l.tree);
        if index as usize >= tree.len() {
            return Err(t!("err.bom.no_node"));
        }
        let node = tree.node(bom, index);
        let label = |i: u32| tree.node(bom, i).label.clone();
        let p = policy();
        let assessment = l.assessed.explain(index);
        let mut advice = vec![];
        let why = assessment
            .reasons
            .iter()
            .map(|r| match r {
                Reason::Rule { params, evaluation } => {
                    if !matches!(evaluation.status, Status::Safe | Status::Acceptable) {
                        advice.extend(p.remediation(params, &l.ctx.profile).into_iter().map(|a| a.summary.clone()));
                    }
                    Why {
                        kind: "rule",
                        status: Some(evaluation.status),
                        algorithm: Some(p.algorithm(&params.algorithm_id).map_or_else(|| params.algorithm_id.clone(), |a| a.name.clone())),
                        key_bits: params.key_bits,
                        security_bits: params.security_bits,
                        param_set: params.param_set.clone(),
                        missing: evaluation.missing.clone(),
                        source: evaluation.source.as_ref().map(|s| s.text.clone()),
                        note: evaluation.note.clone(),
                        ..Default::default()
                    }
                }
                Reason::NotRated { primitive } => Why { kind: "not-rated", primitive: primitive.clone(), ..Default::default() },
                Reason::Unresolved { why } => Why { kind: "unresolved", unresolved: Some(*why), ..Default::default() },
                Reason::Inherited { node, status } => {
                    Why { kind: "inherited", status: Some(*status), node: Some(*node), node_label: Some(label(*node)), ..Default::default() }
                }
                Reason::Reference { edge, node, status } => Why {
                    kind: "reference",
                    status: Some(*status),
                    node: Some(*node),
                    node_label: Some(label(*node)),
                    edge: Some(*edge),
                    ..Default::default()
                },
                Reason::Lifecycle { days_left, status, .. } => {
                    Why { kind: "lifecycle", status: Some(*status), days_left: Some(*days_left), ..Default::default() }
                }
                Reason::Rollup { node, status } => {
                    Why { kind: "rollup", status: Some(*status), node: Some(*node), node_label: Some(label(*node)), ..Default::default() }
                }
            })
            .collect();
        advice.dedup();

        let found_in = node
            .occurrences
            .iter()
            .map(|o| Place { location: o.location.clone(), line: o.line, context: o.context.clone(), path: on_disk(&path, &o.location) })
            .collect();
        let group_path = node.group.as_ref().and_then(|g| g.path.as_deref()).and_then(|p| on_disk(&path, p));
        let raw = (!node.raw.is_null()).then(|| {
            let mut s = serde_json::to_string_pretty(&node.raw).unwrap_or_default();
            if s.len() > RAW_LIMIT {
                let mut cut = RAW_LIMIT;
                while !s.is_char_boundary(cut) {
                    cut -= 1;
                }
                s.truncate(cut);
                s.push_str("\n…");
            }
            s
        });
        Ok(NodeDetails { label: node.label.clone(), kind: node.kind, status: assessment.status, why, advice, found_in, group_path, raw })
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<T>(f: impl std::future::Future<Output = T>) -> T {
        tauri::async_runtime::block_on(f)
    }

    #[test]
    fn bom_view_of_a_scan_next_to_its_sources() {
        // A repository with its CBOM at the top, as CBOMkit leaves it.
        let dir = std::env::temp_dir().join(format!("coxswain-test-bom-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let src = dir.join("core/src/main/java/org/keycloak/jose/jwk");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("JWKParser.java"), "class JWKParser {}\n").unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/coxswain-core/src/bom/testdata/cbomkit/keycloak.cdx.json");
        let bom_path = dir.join("cbom.json");
        std::fs::copy(fixture, &bom_path).unwrap();

        let view = run(bom_info(bom_path.clone(), None)).unwrap();
        assert_eq!(view.mode, TreeMode::Files);
        assert_eq!(view.rows[0].p, -1);
        assert_eq!(view.rows.len(), view.order.len());
        assert_eq!(view.by_status[&Status::Disallowed], 3);
        assert!(view.by_family.contains_key(&Family::Signature));
        // CBOMkit's "key@<uuid>" names are replaced by what the key is
        let key = view.rows.iter().find(|r| r.l == "secret-key@ad2ff456-2f18-4c34-938b-54964e020aeb").unwrap();
        assert_eq!(key.key, Some(("secret-key".into(), "HMAC-SHA256".into())));

        let rsa = view.rows.iter().position(|r| r.l == "RSA-2048").unwrap() as u32;
        let details = run(bom_node(bom_path.clone(), None, rsa)).unwrap();
        assert_eq!(details.status, Status::Acceptable);
        assert_eq!(details.why[0].kind, "rule");
        assert_eq!(details.why[0].key_bits, Some(2048));
        assert_eq!(details.found_in.len(), 3);
        assert_eq!(details.found_in[0].path.as_deref(), Some(src.join("JWKParser.java").to_str().unwrap()));
        assert_eq!(details.found_in[1].path, None); // not in this checkout

        let root = run(bom_node(bom_path.clone(), None, 0)).unwrap();
        assert_eq!((root.why[0].kind, root.why[0].node_label.as_deref()), ("rollup", Some("DSA")));

        // another grouping, from the same parse
        let flat = run(bom_info(bom_path.clone(), Some("flat".into()))).unwrap();
        assert_eq!(flat.mode, TreeMode::Flat);
        assert!(flat.rows.iter().any(|r| r.gk == Some(NodeKind::Algorithm)));

        assert!(run(bom_node(bom_path, None, 999_999)).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn locations_never_leave_the_folder_of_the_bom() {
        let bom = std::env::temp_dir().join("x/cbom.json");
        assert_eq!(on_disk(&bom, "../etc/passwd"), None);
        assert_eq!(on_disk(&bom, ""), None);
    }
}
