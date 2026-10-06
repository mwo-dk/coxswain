//! The provenance view (docs/design/provenance-viewer.md): inputs → build → outputs, the signer,
//! and the checks against the disk. Reading and checking are coxswain-core's `provenance`; this
//! shapes them for ProvenanceView.svelte. Nothing is verified, and nothing leaves the machine.
//!
//! The last two files read are kept, by path and modification time, so moving between
//! statements, checking subjects and comparing never read a file again.

use coxswain_core::provenance::check::{self, Source, Subject};
use coxswain_core::provenance::diff::{self, Area, Diff};
use coxswain_core::provenance::view::{self, Fact};
use coxswain_core::provenance::{self, Attestations, Entry};
use coxswain_core::t;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

type Res<T> = Result<T, String>;

struct Loaded {
    path: PathBuf,
    modified: Option<SystemTime>,
    attestations: Attestations,
}

static LAST: Mutex<Vec<Arc<Loaded>>> = Mutex::new(Vec::new());
const KEPT: usize = 2;

fn error_text(e: provenance::Error) -> String {
    match e {
        provenance::Error::NotAttestation => t!("err.provenance.not_attestation"),
        provenance::Error::InvalidJson { line, message } => t!("err.bom.invalid_json", "line" => line, "message" => message),
        provenance::Error::TooLarge(_) => t!("err.provenance.too_large"),
        provenance::Error::Io(e) => e,
    }
}

fn loaded(path: &Path) -> Res<Arc<Loaded>> {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    {
        let mut last = LAST.lock().unwrap();
        if let Some(at) = last.iter().position(|l| l.path == path && l.modified == modified) {
            let l = last.remove(at);
            last.insert(0, l.clone());
            return Ok(l);
        }
    }
    let attestations = provenance::load(path).map_err(error_text)?;
    let l = Arc::new(Loaded { path: path.to_path_buf(), modified, attestations });
    let mut last = LAST.lock().unwrap();
    last.retain(|x| x.path != l.path);
    last.insert(0, l.clone());
    last.truncate(KEPT);
    Ok(l)
}

fn entry_at(l: &Loaded, index: usize) -> Res<&Entry> {
    l.attestations.entries.get(index).ok_or_else(|| t!("err.provenance.no_statement"))
}

/// An entry as the view needs it: the model, the build's facts, and what can be checked.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryView {
    #[serde(flatten)]
    entry: Entry,
    facts: Vec<Fact>,
    /// Per subject: a container image, which no file here can match.
    images: Vec<bool>,
    /// Per dependency: a git source, whose commit can be looked for in a checkout.
    git: Vec<bool>,
    builder_name: Option<&'static str>,
}

#[derive(Serialize)]
pub struct ProvenanceView {
    entries: Vec<EntryView>,
    issues: Vec<provenance::Issue>,
}

#[tauri::command]
pub async fn provenance_info(path: PathBuf) -> Res<ProvenanceView> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path)?;
        let entries = l
            .attestations
            .entries
            .iter()
            .map(|e| {
                let (git, builder_name) = match &e.statement.predicate {
                    provenance::Predicate::Provenance(p) => {
                        (p.dependencies.iter().map(|d| check::git_source(d).is_some()).collect(), view::builder_name(&p.builder.id))
                    }
                    _ => (vec![], None),
                };
                EntryView {
                    entry: e.clone(),
                    facts: view::facts(e),
                    images: e.statement.subjects.iter().map(check::is_image).collect(),
                    git,
                    builder_name,
                }
            })
            .collect();
        Ok(ProvenanceView { entries, issues: l.attestations.issues.clone() })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The decoded statements, for the Statement view: one, or a list when the file holds several.
#[tauri::command]
pub async fn provenance_statements(path: PathBuf) -> Res<serde_json::Value> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path)?;
        let mut all: Vec<serde_json::Value> = l.attestations.entries.iter().map(|e| e.statement.raw.clone()).collect();
        Ok(if all.len() == 1 { all.remove(0) } else { serde_json::Value::Array(all) })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// The checks under way: a new provenance (or `provenance_cancel`) stops those of the last one.
static CHECKING: Mutex<Option<(PathBuf, Arc<AtomicBool>)>> = Mutex::new(None);

fn cancel_for(path: &Path) -> Arc<AtomicBool> {
    let mut c = CHECKING.lock().unwrap();
    match &*c {
        Some((p, flag)) if p == path => flag.clone(),
        _ => {
            if let Some((_, old)) = c.take() {
                old.store(true, Ordering::Relaxed);
            }
            let flag = Arc::new(AtomicBool::new(false));
            *c = Some((path.to_path_buf(), flag.clone()));
            flag
        }
    }
}

#[tauri::command]
pub fn provenance_cancel() {
    if let Some((_, flag)) = CHECKING.lock().unwrap().take() {
        flag.store(true, Ordering::Relaxed);
    }
}

/// One subject against the files here. `all`: also a large one (the user asked).
#[tauri::command]
pub async fn provenance_subject(path: PathBuf, entry: usize, index: usize, other: Option<PathBuf>, all: bool) -> Res<Subject> {
    crate::here(&path)?;
    let cancel = cancel_for(&path);
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path)?;
        let s = entry_at(&l, entry)?.statement.subjects.get(index).ok_or_else(|| t!("err.provenance.no_statement"))?;
        let limit = if all { None } else { Some(check::AUTO_LIMIT) };
        Ok(check::subject(&path, other.as_deref(), s, limit, &cancel))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Per dependency, where its commit is, for the git sources (none for the rest).
#[tauri::command]
pub async fn provenance_sources(path: PathBuf, entry: usize, other: Option<PathBuf>) -> Res<Vec<Option<Source>>> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path)?;
        let provenance::Predicate::Provenance(p) = &entry_at(&l, entry)?.statement.predicate else { return Ok(vec![]) };
        Ok(p.dependencies.iter().map(|d| check::git_source(d).map(|g| check::source(&path, other.as_deref(), &g))).collect())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
pub struct ProvenanceDiff {
    #[serde(flatten)]
    diff: Diff,
    counts: Counts,
}

#[derive(Serialize)]
struct Counts {
    builder: usize,
    parameters: usize,
    dependencies: usize,
    subjects: usize,
    signer: usize,
}

/// `old` (the other pane's file) against `path`: what differs between the two builds.
#[tauri::command]
pub async fn provenance_diff(old: PathBuf, path: PathBuf) -> Res<ProvenanceDiff> {
    crate::here(&path)?;
    crate::here(&old)?;
    tauri::async_runtime::spawn_blocking(move || {
        let (before, after) = (loaded(&old)?, loaded(&path)?);
        let d = diff::diff(&before.attestations, &after.attestations);
        let counts = Counts {
            builder: d.count(Area::Builder),
            parameters: d.count(Area::Parameter),
            dependencies: d.count(Area::Dependency),
            subjects: d.count(Area::Subject),
            signer: d.count(Area::Signer),
        };
        Ok(ProvenanceDiff { diff: d, counts })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// A CycloneDX predicate as a file, for the BOM viewer.
#[tauri::command]
pub async fn provenance_bom(path: PathBuf, entry: usize) -> Res<String> {
    crate::here(&path)?;
    tauri::async_runtime::spawn_blocking(move || {
        let l = loaded(&path)?;
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        view::bom_file(entry_at(&l, entry)?, &name).map(|p| p.to_string_lossy().into_owned()).map_err(|e| e.to_string())
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

    fn fixture(p: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../crates/coxswain-core/src/provenance/testdata").join(p)
    }

    #[test]
    fn a_bundle_for_the_view() {
        let v = run(provenance_info(fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl"))).unwrap();
        let json = serde_json::to_value(&v).unwrap();
        let e = &json["entries"][0];
        assert_eq!(e["wrapping"]["form"], "bundle");
        assert_eq!(e["signer"]["identity"], "https://github.com/bazel-contrib/publish-to-bcr/.github/workflows/publish.yaml@refs/tags/v0.0.1");
        assert_eq!(e["statement"]["predicate"]["kind"], "provenance");
        assert_eq!(e["git"], serde_json::json!([true]));
        assert!(e["statement"].get("raw").is_none(), "the statement comes on its own");
        assert!(e["facts"].as_array().unwrap().iter().any(|f| f["key"] == "commit"));
        let st = run(provenance_statements(fixture("slsa-verifier/bundle-v0.3-github-v1.intoto.jsonl"))).unwrap();
        assert_eq!(st["_type"], "https://in-toto.io/Statement/v1");
    }

    #[test]
    fn subjects_are_checked_and_cancelled() {
        let dir = std::env::temp_dir().join(format!("coxswain-gui-provenance-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("dist")).unwrap();
        let p = dir.join("rocket.provenance.json");
        std::fs::copy(fixture("made/statement-v1.json"), &p).unwrap();
        std::fs::write(dir.join("dist/rocket-1.4.0.tar.gz"), "foo").unwrap();
        assert!(matches!(run(provenance_subject(p.clone(), 0, 0, None, false)).unwrap(), Subject::Matches { .. }));
        assert!(matches!(run(provenance_subject(p.clone(), 0, 1, None, false)).unwrap(), Subject::Missing));
        assert!(run(provenance_subject(p.clone(), 0, 9, None, false)).is_err());
        assert!(run(provenance_subject(p.clone(), 3, 0, None, false)).is_err());
        provenance_cancel();
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn two_builds() {
        let d = run(provenance_diff(fixture("made/statement-v1.json"), fixture("made/statement-v1.json"))).unwrap();
        assert_eq!(d.counts.parameters + d.counts.dependencies + d.counts.subjects + d.counts.builder + d.counts.signer, 0);
        assert!(run(provenance_diff(fixture("made/statement-v1.json"), PathBuf::from("/nonexistent"))).is_err());
    }
}
