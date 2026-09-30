//! The BOM view (docs/design/bom-viewer.md): a CycloneDX BOM as a rated tree, and the details
//! of one node. Parsing and rating are coxswain-core's `bom`; this shapes them for BomView.svelte.
//!
//! The last two BOMs read are kept, by path, modification time and tree mode, so selecting
//! nodes, switching between the preview and its window, and comparing two versions never read
//! a file again.

use coxswain_core::bom::assess::{Reason, Unresolved};
use coxswain_core::bom::policy::{policy, Family, Param};
use coxswain_core::bom::diff::{self, Change, Counts, Removed};
use coxswain_core::bom::{self, tree, view, EdgeKind, GroupKind, Issue, NodeKind, Status, TreeMode};
use coxswain_core::t;
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type Res<T> = Result<T, String>;

/// A BOM read and rated (coxswain-core's `view::Loaded`), with what tells whether it is still current.
struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    view: view::Loaded,
}

impl std::ops::Deref for Loaded {
    type Target = view::Loaded;
    fn deref(&self) -> &view::Loaded {
        &self.view
    }
}

/// Most recent first: the BOM in view, and the one it was compared with.
static LAST: Mutex<Vec<Arc<Loaded>>> = Mutex::new(Vec::new());
const KEPT: usize = 2;

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
    {
        let mut last = LAST.lock().unwrap();
        // A mode this BOM does not have gives its best one, which is what is kept.
        let fits = |l: &Loaded| want.is_none_or(|m| m == l.mode || !tree::modes(&l.bom).contains(&m));
        if let Some(at) = last.iter().position(|l| l.path == path && l.modified == modified && fits(l)) {
            let l = last.remove(at);
            last.insert(0, l.clone());
            return Ok(l);
        }
    }
    let view = view::Loaded::open(path, want).map_err(error_text)?;
    let l = Arc::new(Loaded { path: path.to_path_buf(), modified, view });
    let mut last = LAST.lock().unwrap();
    last.retain(|x| x.path != l.path);
    last.insert(0, l.clone());
    last.truncate(KEPT);
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
        let mut by_status = BTreeMap::new();
        let mut by_kind = BTreeMap::new();
        let mut by_family = BTreeMap::new();
        let rows = (0..tree.len() as u32)
            .map(|i| {
                let node = l.node(i);
                let status = l.status(i);
                let f = l.families(i);
                if view::is_crypto(node.kind) {
                    *by_status.entry(status).or_insert(0) += 1;
                    for x in &f {
                        *by_family.entry(*x).or_insert(0) += 1;
                    }
                }
                if node.kind != NodeKind::Group && i != 0 {
                    *by_kind.entry(kind_name(node.kind).to_string()).or_insert(0) += 1;
                }
                Row {
                    l: node.label.clone(),
                    k: node.kind,
                    s: status,
                    p: tree.parent[i as usize].map_or(-1, i64::from),
                    o: node.occurrences.first().map(short_place),
                    g: node.group.as_ref().map(|g| g.kind),
                    gk: l.group_kind(i),
                    f,
                    key: l.key_of(i),
                    q: l.search_text(i),
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

#[derive(Serialize)]
pub struct BomDiff {
    /// By tree node of the newer BOM, as `bom_info` gave it in the same mode.
    change: Vec<Change>,
    removed: Vec<Removed>,
    counts: Counts,
}


/// Compares `old` (the file in the other pane) with `path` (the BOM in view), in `mode`.
#[tauri::command]
pub async fn bom_diff(old: PathBuf, path: PathBuf, mode: Option<String>) -> Res<BomDiff> {
    tauri::async_runtime::spawn_blocking(move || {
        let before = loaded(&old, mode.as_deref())?;
        let after = loaded(&path, mode.as_deref())?;
        let d = diff::diff(&before.side(), &after.side());
        Ok(BomDiff { change: d.change, removed: d.removed, counts: d.counts })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A path in the BOM as a file next to it (see `view::on_disk`), for the frontend.
fn on_disk(bom_path: &Path, location: &str) -> Option<String> {
    view::on_disk(bom_path, location).map(|p| p.to_string_lossy().into_owned())
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
        let src = ["core", "src", "main", "java", "org", "keycloak", "jose", "jwk"].iter().fold(dir.clone(), |p, s| p.join(s));
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
    fn bom_diff_against_an_older_scan() {
        let dir = std::env::temp_dir().join(format!("coxswain-test-bomdiff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let fixture = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/coxswain-core/src/bom/testdata/cbomkit/keycloak.cdx.json");
        let new = dir.join("new.cdx.json");
        std::fs::copy(&fixture, &new).unwrap();
        // the older scan had no SHA-1, and one more AES
        let mut doc: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(&fixture).unwrap()).unwrap();
        let components = doc["components"].as_array_mut().unwrap();
        components.retain(|c| c["name"] != "SHA1");
        components.push(serde_json::json!({"type": "cryptographic-asset", "name": "AES256-GCM", "bom-ref": "gone",
            "cryptoProperties": {"assetType": "algorithm"}, "evidence": {"occurrences": [{"location": "Old.java"}]}}));
        let old = dir.join("old.cdx.json");
        std::fs::write(&old, doc.to_string()).unwrap();

        let view = run(bom_info(new.clone(), None)).unwrap();
        let d = run(bom_diff(old.clone(), new.clone(), Some("files".into()))).unwrap();
        assert_eq!(d.change.len(), view.rows.len());
        let added: Vec<&str> = (0..d.change.len()).filter(|&i| d.change[i] == Change::Added).map(|i| view.rows[i].l.as_str()).collect();
        assert_eq!(added, ["SHA1"]);
        assert_eq!((d.counts.added, d.counts.removed, d.counts.new_risks, d.counts.fixed), (1, 1, 1, 0));
        assert_eq!((d.removed[0].label.as_str(), d.removed[0].place.as_str()), ("AES256-GCM", "Old.java"));

        // a file that is not a BOM says so
        let text = dir.join("notes.json");
        std::fs::write(&text, "{}").unwrap();
        assert!(run(bom_diff(text, new, None)).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn locations_never_leave_the_folder_of_the_bom() {
        let bom = std::env::temp_dir().join("x/cbom.json");
        assert_eq!(on_disk(&bom, "../etc/passwd"), None);
        assert_eq!(on_disk(&bom, ""), None);
    }
}
