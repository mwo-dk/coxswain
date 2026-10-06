//! What both apps show of an entry: the build's facts, each with where it came from, short names
//! for well-known builders, and a CycloneDX predicate as a file the BOM viewer can open.

use super::check;
use super::model::*;
use super::signer::Claim;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;

/// Product names for well-known builder IDs and build types; the full ID is always in the
/// details. Names only: no trust or SLSA level is claimed for any of them.
const BUILDERS: &[(&str, &str)] = &[
    ("https://github.com/actions/runner", "GitHub Actions"),
    ("generator_generic_slsa3.yml", "SLSA GitHub generator (generic)"),
    ("generator_container_slsa3.yml", "SLSA GitHub generator (container)"),
    ("builder_go_slsa3.yml", "SLSA GitHub builder (Go)"),
    ("builder_nodejs_slsa3.yml", "SLSA GitHub builder (Node.js)"),
    ("builder_gradle_slsa3.yml", "SLSA GitHub builder (Gradle)"),
    ("builder_maven_slsa3.yml", "SLSA GitHub builder (Maven)"),
    ("builder_container-based_slsa3.yml", "SLSA GitHub builder (container-based)"),
    ("delegator_generic_slsa3.yml", "SLSA GitHub delegator"),
    ("delegator_lowperms-generic_slsa3.yml", "SLSA GitHub delegator"),
    ("https://cloudbuild.googleapis.com/", "Google Cloud Build"),
    ("https://gitlab.com/gitlab-org/gitlab-runner", "GitLab Runner"),
    ("https://github.com/npm/cli/gha", "npm on GitHub Actions"),
];

const BUILD_TYPES: &[(&str, &str)] = &[
    ("https://actions.github.io/buildtypes/workflow/", "GitHub Actions workflow"),
    ("https://slsa-framework.github.io/github-actions-buildtypes/workflow/", "GitHub Actions workflow"),
    ("https://github.com/slsa-framework/slsa-github-generator/generic@", "SLSA generator, generic"),
    ("https://github.com/slsa-framework/slsa-github-generator/container@", "SLSA generator, container"),
    ("https://github.com/slsa-framework/slsa-github-generator/go@", "SLSA builder, Go"),
    ("https://github.com/slsa-framework/slsa-github-generator/delegator-generic@", "SLSA delegator"),
    ("https://cloudbuild.googleapis.com/", "Google Cloud Build"),
    ("https://github.com/npm/cli/gha/", "npm on GitHub Actions"),
    ("https://gitlab.com/gitlab-org/gitlab-runner/", "GitLab Runner"),
];

/// A well-known builder's product name, by a prefix of its ID or the workflow file it names.
pub fn builder_name(id: &str) -> Option<&'static str> {
    BUILDERS.iter().find(|(k, _)| if k.starts_with("https://") { id.starts_with(k) } else { id.contains(&format!("/{k}@")) }).map(|(_, n)| *n)
}

/// A builder as one short line: its product name, else a workflow builder as
/// `owner/repo/file@ref`, else its ID without the scheme. The full ID is in the details.
pub fn builder_label(id: &str) -> String {
    if let Some(n) = builder_name(id) {
        return n.to_string();
    }
    let bare = id.split_once("://").map_or(id, |(_, rest)| rest);
    if let Some((repo, file)) = bare.split_once("/.github/workflows/") {
        let repo = repo.split_once('/').map_or(repo, |(_, r)| r);
        let (file, r) = file.split_once('@').map_or((file, None), |(f, r)| (f, Some(r)));
        let r = r.map(|r| r.trim_start_matches("refs/tags/").trim_start_matches("refs/heads/"));
        return r.map_or_else(|| format!("{repo}/{file}"), |r| format!("{repo}/{file}@{r}"));
    }
    bare.to_string()
}

/// An input as one short line: a git source as `repo@commit` (seven digits), else its label.
pub fn input_label(r: &Resource) -> String {
    match check::git_source(r) {
        Some(g) => {
            let name = g.repo.rsplit('/').next().unwrap_or(&g.repo);
            format!("{name}@{}", &g.commit[..g.commit.len().min(7)])
        }
        None => r.label().to_string(),
    }
}

pub fn build_type_name(t: &str) -> Option<&'static str> {
    BUILD_TYPES.iter().find(|(k, _)| t.starts_with(k)).map(|(_, n)| *n)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FactKey {
    Builder,
    BuildType,
    Repository,
    Workflow,
    Ref,
    Commit,
    Trigger,
    Runner,
    Started,
    Finished,
    Invocation,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum From {
    /// The signing certificate's claims (Fulcio).
    Certificate,
    /// The provenance itself.
    Predicate,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Fact {
    pub key: FactKey,
    pub value: String,
    pub from: From,
    /// What the provenance says when the certificate says otherwise.
    pub conflict: Option<String>,
}

/// The build at a glance, in the order shown. The certificate wins where both say something;
/// where they disagree, the provenance's value is kept as a conflict.
pub fn facts(e: &Entry) -> Vec<Fact> {
    let Predicate::Provenance(p) = &e.statement.predicate else { return vec![] };
    let cert = |c: Claim| e.signer.as_ref().and_then(|s| s.get(c)).map(str::to_string);
    let at = |v: &Value, ptr: &str| v.pointer(ptr).and_then(Value::as_str).filter(|s| !s.is_empty()).map(str::to_string);
    let (ext, int) = (&p.external, &p.internal);
    let source = p.dependencies.iter().find_map(|d| check::git_source(d).map(|g| (d, g)));
    let source_uri = source.as_ref().and_then(|(d, _)| d.uri.as_deref()).map(|u| {
        let u = u.strip_prefix("git+").unwrap_or(u);
        u.rsplit_once('@').filter(|(h, _)| h.contains("://")).map_or(u, |(h, _)| h).to_string()
    });
    let github = |k: &str| at(int, &format!("/{k}")).or_else(|| at(int, &format!("/environment/{}", k.to_lowercase())));

    let predicate: Vec<(FactKey, Option<String>)> = vec![
        (FactKey::Builder, Some(builder_name(&p.builder.id).map_or_else(|| p.builder.id.clone(), str::to_string))),
        (FactKey::BuildType, Some(build_type_name(&p.build_type).map_or_else(|| p.build_type.clone(), str::to_string))),
        (
            FactKey::Repository,
            at(ext, "/workflow/repository").or_else(|| github("GITHUB_REPOSITORY").map(|r| format!("https://github.com/{r}"))).or(source_uri),
        ),
        (
            FactKey::Workflow,
            at(ext, "/workflow/path")
                .or_else(|| p.dependencies.iter().find_map(|d| d.entry_point.clone()))
                .or_else(|| github("GITHUB_WORKFLOW_REF")),
        ),
        (
            FactKey::Ref,
            at(ext, "/workflow/ref").or_else(|| github("GITHUB_REF")).or_else(|| source.as_ref().and_then(|(_, g)| g.reference.clone())),
        ),
        (FactKey::Commit, source.as_ref().map(|(_, g)| g.commit.clone()).or_else(|| github("GITHUB_SHA"))),
        (FactKey::Trigger, at(int, "/github/event_name").or_else(|| github("GITHUB_EVENT_NAME"))),
        (FactKey::Runner, at(int, "/github/runner_environment")),
        (FactKey::Started, p.started.clone()),
        (FactKey::Finished, p.finished.clone()),
        (FactKey::Invocation, p.invocation.clone()),
    ];
    let certificate = |k: FactKey| match k {
        FactKey::Repository => cert(Claim::SourceRepositoryUri).or_else(|| cert(Claim::Repository).map(|r| format!("https://github.com/{r}"))),
        FactKey::Workflow => cert(Claim::BuildConfigUri),
        FactKey::Ref => cert(Claim::SourceRepositoryRef),
        FactKey::Commit => cert(Claim::SourceRepositoryDigest),
        FactKey::Trigger => cert(Claim::BuildTrigger),
        FactKey::Runner => cert(Claim::RunnerEnvironment),
        FactKey::Invocation => cert(Claim::RunInvocationUri),
        _ => None,
    };
    predicate
        .into_iter()
        .filter_map(|(key, said)| match (certificate(key), said) {
            (Some(c), said) => {
                let conflict = said.filter(|s| !agrees(key, &c, s));
                Some(Fact { key, value: c, from: From::Certificate, conflict })
            }
            (None, Some(s)) => Some(Fact { key, value: s, from: From::Predicate, conflict: None }),
            (None, None) => None,
        })
        .collect()
}

/// Whether the certificate's value and the provenance's say the same, allowing for their forms:
/// a workflow URI against its path, a repository with or without `.git`, a commit's case.
fn agrees(key: FactKey, cert: &str, said: &str) -> bool {
    let norm = |s: &str| s.trim_end_matches('/').trim_end_matches(".git").to_lowercase();
    match key {
        // `https://github.com/o/r/.github/workflows/x.yml@ref` against `.github/workflows/x.yml`
        // or `o/r/.github/workflows/x.yml@ref`.
        FactKey::Workflow => {
            let path = |s: &str| norm(s.split_once('@').map_or(s, |(p, _)| p));
            path(cert).ends_with(&path(said.trim_start_matches("./")))
        }
        _ => norm(cert) == norm(said),
    }
}

/// A CycloneDX predicate written out as a file the BOM viewer opens, in a folder of this run's
/// own (for this user only; earlier runs' folders go after a day).
pub fn bom_file(e: &Entry, name: &str) -> std::io::Result<PathBuf> {
    let Predicate::Bom { bom } = &e.statement.predicate else {
        return Err(std::io::Error::other("not a CycloneDX predicate"));
    };
    let base = dirs::cache_dir().ok_or_else(|| std::io::Error::other("no cache folder"))?.join("coxswain").join("provenance");
    let day = std::time::Duration::from_secs(24 * 3600);
    for old in std::fs::read_dir(&base).into_iter().flatten().flatten() {
        if old.metadata().and_then(|m| m.modified()).is_ok_and(|t| t.elapsed().unwrap_or_default() > day) {
            let _ = std::fs::remove_dir_all(old.path());
        }
    }
    let run = base.join(std::process::id().to_string());
    crate::fs::private_dir(&run)?;
    let stem: String = name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    let to = run.join(format!("{stem}.cdx.json"));
    std::fs::write(&to, serde_json::to_vec_pretty(bom).map_err(std::io::Error::other)?)?;
    Ok(to)
}
