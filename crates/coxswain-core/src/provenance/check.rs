//! What a file manager can check against the disk: are the subjects the files here, and is the
//! source commit in a checkout here? Both are local; neither says the provenance is genuine.

use super::model::*;
use serde::Serialize;
use sha2::Digest;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};

/// Subjects up to this size are checked unasked; larger ones when the user asks.
pub const AUTO_LIMIT: u64 = 256 * 1024 * 1024;

/// Where a subject is looked for, in this order: by its name below the provenance's folder, by
/// its last part in that folder, then the same in the other pane's folder.
pub fn locate(provenance: &Path, other: Option<&Path>, s: &Resource) -> Option<PathBuf> {
    let name = s.name.as_deref().or_else(|| s.uri.as_deref().map(url_file)).filter(|n| !n.is_empty())?;
    let last = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let dirs = [provenance.parent(), other];
    dirs.iter().flatten().find_map(|d| crate::fs::beneath(d, name).or_else(|| crate::fs::beneath(d, last))).filter(|p| p.is_file())
}

/// The file a URL names: its last path part, without query or fragment.
fn url_file(u: &str) -> &str {
    let u = u.split(['?', '#']).next().unwrap_or(u);
    if u.contains("://") { u.rsplit('/').next().unwrap_or("") } else { u }
}

/// A subject that names a container image rather than a file: `ghcr.io/owner/app`,
/// `docker.io/library/rust:1`, `pkg:docker/…`, `oci://…`.
pub fn is_image(s: &Resource) -> bool {
    let n = s.name.as_deref().or(s.uri.as_deref()).unwrap_or("");
    if n.starts_with("pkg:docker/") || n.starts_with("pkg:oci/") || n.starts_with("oci://") {
        return true;
    }
    let first = n.split('/').next().unwrap_or("");
    // A registry host: `ghcr.io`, `registry:5000`, `localhost`. Never `.` or `..`.
    let host = !first.starts_with('.') && (first.contains('.') || first.contains(':') || first == "localhost");
    n.contains('/') && !n.contains("://") && host
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Subject {
    /// The file here has the digest the provenance names.
    Matches { path: PathBuf, algorithm: String },
    /// It does not: this is not the file that was built.
    Differs { path: PathBuf, algorithm: String, expected: String, actual: String },
    /// No file of that name here.
    Missing,
    /// Larger than [`AUTO_LIMIT`]: checked when asked.
    Large { path: PathBuf, size: u64 },
    /// A container image, an algorithm we do not hash, or no digest at all.
    CannotCheck { why: CannotCheck },
    /// The file could not be read (in the cloud only, no permission, cancelled).
    Unreadable { path: PathBuf, message: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CannotCheck {
    Image,
    NoDigest,
    /// Only digests we do not compute (the names, e.g. `sha1`, `gitCommit`).
    Algorithms(Vec<String>),
}

/// Checks one subject. `limit` is [`AUTO_LIMIT`] unless the user asked; `cancel` stops the hash.
pub fn subject(provenance: &Path, other: Option<&Path>, s: &Resource, limit: Option<u64>, cancel: &AtomicBool) -> Subject {
    let Some(path) = locate(provenance, other, s) else {
        let why = if is_image(s) {
            CannotCheck::Image
        } else if s.digest.is_empty() {
            CannotCheck::NoDigest
        } else {
            return Subject::Missing;
        };
        return Subject::CannotCheck { why };
    };
    let Some((algorithm, expected)) = ["sha256", "sha512"].iter().find_map(|a| s.digest.get(*a).map(|d| (*a, d.to_lowercase()))) else {
        let why = if s.digest.is_empty() { CannotCheck::NoDigest } else { CannotCheck::Algorithms(s.digest.keys().cloned().collect()) };
        return Subject::CannotCheck { why };
    };
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    if limit.is_some_and(|l| size > l) {
        return Subject::Large { path, size };
    }
    match hash(&path, algorithm, cancel) {
        Ok(actual) if actual == expected => Subject::Matches { path, algorithm: algorithm.into() },
        Ok(actual) => Subject::Differs { path, algorithm: algorithm.into(), expected, actual },
        Err(e) => Subject::Unreadable { path, message: e.to_string() },
    }
}

/// The hex digest of a file, read in 1 MB pieces so `cancel` is seen. A file only in the cloud
/// is not read, unless the settings read those: it would be downloaded.
pub fn hash(path: &Path, algorithm: &str, cancel: &AtomicBool) -> io::Result<String> {
    fn run<D: Digest>(f: &mut std::fs::File, cancel: &AtomicBool) -> io::Result<String> {
        let mut h = D::new();
        let mut buf = vec![0u8; 1 << 20];
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            h.update(&buf[..n]);
        }
        Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
    }
    if crate::cloud::keep_out(path) {
        return Err(crate::cloud::not_here(path));
    }
    let mut f = std::fs::File::open(path)?;
    match algorithm {
        "sha256" => run::<sha2::Sha256>(&mut f, cancel),
        "sha512" => run::<sha2::Sha512>(&mut f, cancel),
        a => Err(io::Error::new(io::ErrorKind::Unsupported, format!("{a} is not computed"))),
    }
}

/// A dependency that is a git source: `git+https://host/owner/repo@ref` with a commit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GitSource {
    /// Host and path, lower case, without `.git`: `github.com/owner/repo`.
    pub repo: String,
    pub reference: Option<String>,
    pub commit: String,
}

pub fn git_source(r: &Resource) -> Option<GitSource> {
    let uri = r.uri.as_deref()?.strip_prefix("git+")?;
    // v1 says gitCommit; v0.2's configSource says sha1.
    let commit = r.digest.get("gitCommit").or_else(|| r.digest.get("sha1"))?.to_lowercase();
    let (scheme_host, path) = uri.split_once("://").map(|(_, rest)| rest.split_once('/').unwrap_or((rest, "")))?;
    let (path, reference) = match path.split_once('@') {
        Some((p, r)) => (p, Some(r.to_string())),
        None => (path, None),
    };
    let host = scheme_host.rsplit('@').next().unwrap_or(scheme_host);
    Some(GitSource { repo: repo_key(host, path), reference, commit })
}

fn repo_key(host: &str, path: &str) -> String {
    let host = host.split(':').next().unwrap_or(host);
    let path = path.trim_matches('/');
    let path = path.strip_suffix(".git").unwrap_or(path);
    format!("{}/{}", host.to_lowercase(), path.to_lowercase())
}

/// A remote's URL as a repo key: `https://…`, `ssh://git@…` and `git@host:owner/repo.git`.
pub fn remote_key(url: &str) -> Option<String> {
    let url = url.trim();
    if let Some((_, rest)) = url.split_once("://") {
        let (host, path) = rest.split_once('/')?;
        return Some(repo_key(host.rsplit('@').next().unwrap_or(host), path));
    }
    // scp-like: [user@]host:path
    let (host, path) = url.split_once(':')?;
    Some(repo_key(host.rsplit('@').next().unwrap_or(host), path))
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum Source {
    /// No checkout of that repository in the provenance's folder, the other pane's, or above.
    NoCheckout,
    /// A checkout, without the commit: it was never fetched, or is gone. Nothing is fetched.
    Missing { checkout: PathBuf },
    /// The commit is HEAD, or `behind` commits before it.
    OnBranch { checkout: PathBuf, behind: u64 },
    /// HEAD is `ahead` commits before the commit.
    Ahead { checkout: PathBuf, ahead: u64 },
    /// In the checkout, but on another line of history.
    Elsewhere { checkout: PathBuf },
}

/// Where the commit of `src` is, in a checkout of its repository at or above `provenance`'s
/// folder or `other`. Runs read-only git; never fetches.
pub fn source(provenance: &Path, other: Option<&Path>, src: &GitSource) -> Source {
    let Some(checkout) = checkouts(provenance.parent(), other).into_iter().find(|c| remotes(c).iter().any(|r| *r == src.repo)) else {
        return Source::NoCheckout;
    };
    let git = |args: &[&str]| crate::git::command(&checkout).args(args).stdout(Stdio::piped()).stderr(Stdio::null()).output().ok();
    let ok = |args: &[&str]| git(args).is_some_and(|o| o.status.success());
    let count = |range: &str| {
        git(&["rev-list", "--count", range]).and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok()).unwrap_or(0)
    };
    // A commit id only: never a ref or an option from the file.
    if src.commit.len() < 7 || !src.commit.bytes().all(|b| b.is_ascii_hexdigit()) || !ok(&["cat-file", "-e", &format!("{}^{{commit}}", src.commit)]) {
        return Source::Missing { checkout };
    }
    if ok(&["merge-base", "--is-ancestor", &src.commit, "HEAD"]) {
        let behind = count(&format!("{}..HEAD", src.commit));
        Source::OnBranch { checkout, behind }
    } else if ok(&["merge-base", "--is-ancestor", "HEAD", &src.commit]) {
        let ahead = count(&format!("HEAD..{}", src.commit));
        Source::Ahead { checkout, ahead }
    } else {
        Source::Elsewhere { checkout }
    }
}

/// The checkouts `dir` and `other` are in, nearest first, each once.
fn checkouts(dir: Option<&Path>, other: Option<&Path>) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for d in [dir, other].into_iter().flatten() {
        let top = crate::git::command(d).args(["rev-parse", "--show-toplevel"]).stdout(Stdio::piped()).stderr(Stdio::null()).output();
        if let Some(o) = top.ok().filter(|o| o.status.success()) {
            let p = PathBuf::from(String::from_utf8_lossy(&o.stdout).trim());
            if !out.contains(&p) {
                out.push(p);
            }
        }
    }
    out
}

/// A checkout's remotes, as repo keys.
fn remotes(checkout: &Path) -> Vec<String> {
    let out = crate::git::command(checkout)
        .args(["config", "--get-regexp", r"^remote\..*\.url$"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output();
    let Ok(o) = out else { return vec![] };
    String::from_utf8_lossy(&o.stdout).lines().filter_map(|l| l.split_once(' ')).filter_map(|(_, url)| remote_key(url)).collect()
}
