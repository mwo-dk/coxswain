//! Container images saved as files: a `docker save` / `podman save` tar (a `manifest.json`
//! and its layers) and an OCI image layout (`oci-layout`, `index.json`, `blobs/sha256/…`), as
//! a tar or as a folder, whose `index.json` is what opens. They open like a folder, read-only
//! (`archive`), as the one file system their layers make: applied in order, gzip, zstd or
//! plain, whiteouts (`.wh.<name>`) and opaque folders (`.wh..wh..opq`) honoured.
//!
//! An image is hostile input. Paths are made relative to the image's top and refused with
//! `..` or a NUL; links are resolved inside the merged tree only (a link to a file shows as
//! that file, others are left out); hard links name a file already in the tree; the files,
//! the entries read, the layers, the JSON and what a layer may unpack to are all capped.
//! Nothing is ever written into an image, and the live stores of Docker and Podman are not
//! read.
//!
//! The merged tree is made once, every layer read through, and kept in the cache
//! (`tar_index`, by the image's identity and its layers'), with where each file's bytes start
//! in its layer and, for a gzip or zstd layer, the places it can be unpacked from again: a
//! file is then read from the last such place before it, a plain layer's where it is.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::t;

/// The most bytes of a JSON document of an image (manifest, index, config).
const JSON_MAX: u64 = 4 << 20;
/// The most entries of an image's own tar looked at to find its manifest: an image tar holds
/// a few per layer.
const OUTER_MAX: usize = 4096;
/// The most layers; Docker itself stops at 127.
const LAYERS_MAX: usize = 256;
/// How deep an index may name other indexes, and how many index documents are read in all,
/// each blob once.
const NESTED_MAX: usize = 4;
const INDEXES_MAX: usize = 256;
/// The most manifests an image's indexes name together.
const MANIFESTS_MAX: usize = 1024;
/// The most files and folders of the merged tree.
pub(crate) const FILES_MAX: usize = 500_000;
/// The most tar entries read from all layers together.
const READ_MAX: usize = 2_000_000;
/// What one layer may unpack to: 200 times its packed size, at least 1 GiB, at most 64 GiB.
const UNPACK_FLOOR: u64 = 1 << 30;
const UNPACK_RATIO: u64 = 200;
const UNPACK_MAX: u64 = 64 << 30;
/// The most links followed resolving one path, as a kernel's ELOOP; the most path parts one
/// resolving walks through, and all of an image's together.
const HOPS: usize = 40;
const PARTS_MAX: usize = 4096;
const LINK_WORK_MAX: usize = 4_000_000;

type Stamp = (u64, Option<std::time::SystemTime>);

fn stamp(path: &Path) -> io::Result<Stamp> {
    let m = std::fs::metadata(path)?;
    Ok((m.len(), m.modified().ok()))
}

fn bad(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, t!("container_image.broken", "why" => why.into()))
}

/// Where an image's blobs are: entries of its tar (name → where the bytes start, how many),
/// or the files of an OCI layout folder.
enum Store {
    Tar(PathBuf, HashMap<String, (u64, u64)>),
    Dir(PathBuf),
}

impl Store {
    fn size(&self, name: &str) -> Option<u64> {
        match self {
            Store::Tar(_, m) => m.get(name).map(|&(_, n)| n),
            Store::Dir(d) => std::fs::symlink_metadata(d.join(name))
                .ok()
                .filter(|m| m.is_file())
                .map(|m| m.len()),
        }
    }

    /// The bytes of `name`: an entry of the tar, or a file of the folder (never a link).
    fn open(&self, name: &str) -> io::Result<Box<dyn Read>> {
        self.open_at(name, 0)
    }

    /// The bytes of `name` from `skip` on.
    fn open_at(&self, name: &str, skip: u64) -> io::Result<Box<dyn Read>> {
        let missing = || bad(format!("{name} is missing"));
        match self {
            Store::Tar(p, m) => {
                let &(at, n) = m.get(name).ok_or_else(missing)?;
                let skip = skip.min(n);
                let mut f = File::open(p)?;
                f.seek(SeekFrom::Start(at + skip))?;
                Ok(Box::new(BufReader::new(f.take(n - skip))))
            }
            Store::Dir(d) => {
                let p = d.join(name);
                if !std::fs::symlink_metadata(&p).is_ok_and(|m| m.is_file()) {
                    return Err(missing());
                }
                let mut f = File::open(p)?;
                f.seek(SeekFrom::Start(skip))?;
                Ok(Box::new(BufReader::new(f)))
            }
        }
    }

    /// What tells a blob changed, past the image file's own identity: in a layout folder the
    /// blob is a file of its own (its size and time); in a tar the tar's identity says it all.
    fn blob_stamp(&self, name: &str) -> String {
        match self {
            Store::Tar(..) => String::new(),
            Store::Dir(d) => match stamp(&d.join(name)) {
                Ok((n, t)) => format!("{n}:{:?}", t.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())),
                Err(_) => "missing".into(),
            },
        }
    }

    fn json(&self, name: &str) -> io::Result<Value> {
        let mut buf = vec![];
        self.open(name)?.take(JSON_MAX + 1).read_to_end(&mut buf)?;
        if buf.len() as u64 > JSON_MAX {
            return Err(bad(format!("{name} is too large")));
        }
        serde_json::from_slice(&buf).map_err(|e| bad(format!("{name}: {e}")))
    }
}

/// `sha256:<hex>` as the path of its blob in an OCI layout; `None` for anything else.
fn blob(digest: &str) -> Option<String> {
    let (alg, hex) = digest.split_once(':')?;
    let len = match alg {
        "sha256" => 64,
        "sha512" => 128,
        _ => return None,
    };
    (hex.len() == len && hex.bytes().all(|b| b.is_ascii_hexdigit()))
        .then(|| format!("blobs/{alg}/{hex}"))
}

/// An OCI layout's `index.json`: what opens of a layout folder.
pub fn layout_index(path: &Path) -> bool {
    path.file_name().is_some_and(|n| n == "index.json")
        && path.parent().is_some_and(|d| d.join("oci-layout").is_file())
}

#[derive(Clone, Debug, Default)]
pub struct Image {
    /// `docker-archive` or `OCI`.
    pub format: &'static str,
    pub tags: Vec<String>,
    /// Every platform an index names (a multi-arch image), as `os/arch[/variant]`.
    pub platforms: Vec<String>,
    /// The layers, bottom first: their blob and packed size.
    pub layers: Vec<(String, u64)>,
    pub config: Value,
}

struct Opened {
    store: Store,
    image: Image,
}

/// The images last looked at, and the merged trees of the last few: browsing lists one per
/// folder, and a tree not kept means unpacking every layer.
type Seen = (PathBuf, Stamp, Option<Arc<Opened>>);
static SEEN: Mutex<Vec<Seen>> = Mutex::new(Vec::new());
const SEEN_KEPT: usize = 32;
static TREES: Mutex<Vec<(PathBuf, Stamp, Arc<Tree>)>> = Mutex::new(Vec::new());
const TREES_KEPT: usize = 2;

/// `path` as an image: `None` for a tar that is none (a plain tar).
fn opened(path: &Path) -> io::Result<Option<Arc<Opened>>> {
    let st = stamp(path)?;
    if let Some((.., o)) = SEEN.lock().unwrap().iter().find(|(p, s, _)| p == path && *s == st) {
        return Ok(o.clone());
    }
    let o = read(path)?.map(Arc::new);
    let mut seen = SEEN.lock().unwrap();
    seen.retain(|(p, ..)| p != path);
    if seen.len() >= SEEN_KEPT {
        seen.remove(0);
    }
    seen.push((path.to_path_buf(), st, o.clone()));
    Ok(o)
}

/// Whether the tar (or `index.json`) at `path` is a container image. Read once per change.
pub fn is_image(path: &Path) -> bool {
    opened(path).is_ok_and(|o| o.is_some())
}

/// The image at `path` (its tags, platforms, layers, config), if it is one.
pub fn image(path: &Path) -> Option<Image> {
    opened(path).ok().flatten().map(|o| o.image.clone())
}

fn read(path: &Path) -> io::Result<Option<Opened>> {
    let store = if layout_index(path) {
        Store::Dir(path.parent().unwrap_or(path).to_path_buf())
    } else {
        // A plain tar's headers, read by seeking past what lies between them.
        let mut names = HashMap::new();
        let mut a = tar::Archive::new(BufReader::new(File::open(path)?));
        let (mut n, mut links) = (0, vec![]);
        let r = crate::archive::tar_named(a.entries_with_seek()?, |name, link, e| {
            n += 1;
            let ty = e.header().entry_type();
            if ty.is_file() {
                names.insert(name, (e.raw_file_position(), e.size()));
            } else if let (true, Some(to)) = (ty.is_symlink() || ty.is_hard_link(), link) {
                links.push((name, to));
            }
            Ok(n < OUTER_MAX)
        });
        // A layer named by a link (`<id>/layer.tar -> ../<digest>.tar`): its file, one step.
        for (name, to) in links {
            let dir = name.rsplit_once('/').map_or("", |(d, _)| d);
            let mut parts: Vec<&str> = dir.split('/').filter(|p| !p.is_empty()).collect();
            for p in to.split('/') {
                match p {
                    "" | "." => {}
                    ".." => drop(parts.pop()),
                    p => parts.push(p),
                }
            }
            if let Some(&at) = names.get(&parts.join("/")) {
                names.entry(name).or_insert(at);
            }
        }
        // A tar that cannot be read is no image here: it fails as a tar.
        if r.is_err() || !(names.contains_key("manifest.json") || names.contains_key("oci-layout")) {
            return Ok(None);
        }
        Store::Tar(path.to_path_buf(), names)
    };
    let image = match &store {
        Store::Tar(_, m) if m.contains_key("manifest.json") => docker(&store)?,
        _ => oci(&store)?,
    };
    if image.layers.len() > LAYERS_MAX {
        return Err(bad(format!("more than {LAYERS_MAX} layers")));
    }
    Ok(Some(Opened { store, image }))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|s| s.as_str().map(String::from))
        .collect()
}

/// A `docker save` tar: its `manifest.json` lists images; the first is opened, the tags are
/// all of them.
fn docker(store: &Store) -> io::Result<Image> {
    let m = store.json("manifest.json")?;
    let all = m.as_array().ok_or_else(|| bad("manifest.json"))?;
    let first = all.first().ok_or_else(|| bad("manifest.json names no image"))?;
    let mut tags = vec![];
    for i in all.iter().take(64) {
        tags.extend(strings(&i["RepoTags"]));
    }
    let layers = strings(&first["Layers"])
        .into_iter()
        .map(|l| {
            let size = store.size(&l).ok_or_else(|| bad(format!("{l} is missing")))?;
            Ok((l, size))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let config = match first["Config"].as_str() {
        Some(c) => store.json(c)?,
        None => Value::Null,
    };
    Ok(Image { format: "docker-archive", tags, platforms: vec![], layers, config })
}

fn platform(p: &Value) -> Option<String> {
    let (os, arch) = (p["os"].as_str()?, p["architecture"].as_str()?);
    if os == "unknown" || arch == "unknown" {
        return None;
    }
    Some(match p["variant"].as_str() {
        Some(v) => format!("{os}/{arch}/{v}"),
        None => format!("{os}/{arch}"),
    })
}

/// This machine's platform as images name it.
fn here() -> String {
    let arch = match std::env::consts::ARCH {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        "x86" => "386",
        "powerpc64" => "ppc64le",
        a => a,
    };
    format!("linux/{arch}")
}

/// An OCI layout: `index.json` names image manifests, or indexes of them (one per platform).
fn oci(store: &Store) -> io::Result<Image> {
    let index = store.json("index.json")?;
    let (mut tags, mut found) = (vec![], vec![]);
    let mut todo = vec![(index, 0)];
    let (mut seen, mut read, mut named) = (std::collections::HashSet::new(), 1, 0);
    while let Some((ix, depth)) = todo.pop() {
        for d in ix["manifests"].as_array().into_iter().flatten() {
            named += 1;
            if named > MANIFESTS_MAX {
                break;
            }
            let a = &d["annotations"];
            for key in ["io.containerd.image.name", "org.opencontainers.image.ref.name"] {
                if let Some(n) = a[key].as_str()
                    && !tags.iter().any(|t: &String| t == n || t.ends_with(&format!(":{n}")))
                {
                    tags.push(n.to_string());
                }
            }
            let Some(b) = d["digest"].as_str().and_then(blob) else { continue };
            let media = d["mediaType"].as_str().unwrap_or_default();
            if media.ends_with("image.index.v1+json") || media.ends_with("manifest.list.v2+json") {
                // Each index once (one naming itself, or another twice, is read once), and few.
                if depth < NESTED_MAX && read < INDEXES_MAX && seen.insert(b.clone()) {
                    read += 1;
                    todo.push((store.json(&b)?, depth + 1));
                }
            } else {
                found.push((platform(&d["platform"]), b));
            }
        }
    }
    let platforms: Vec<String> = found.iter().filter_map(|(p, _)| p.clone()).collect();
    let here = here();
    let pick = found
        .iter()
        .find(|(p, _)| p.as_deref().is_some_and(|p| p == here || p.starts_with(&format!("{here}/"))))
        .or_else(|| found.iter().find(|(p, _)| p.is_some()))
        .or(found.first())
        .ok_or_else(|| bad("index.json names no image"))?;
    let m = store.json(&pick.1)?;
    let layers = m["layers"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|l| {
            let b = l["digest"].as_str().and_then(blob).ok_or_else(|| bad("a layer's digest"))?;
            let size = store.size(&b).ok_or_else(|| bad(format!("{b} is missing")))?;
            Ok((b, size))
        })
        .collect::<io::Result<Vec<_>>>()?;
    let config = match m["config"]["digest"].as_str().and_then(blob) {
        Some(c) => store.json(&c)?,
        None => Value::Null,
    };
    Ok(Image {
        format: "OCI",
        tags,
        platforms: if platforms.len() > 1 { platforms } else { vec![] },
        layers,
        config,
    })
}

/// What the panel's title shows inside the image: its first tag, else its format.
pub fn title(path: &Path) -> Option<String> {
    let o = opened(path).ok().flatten()?;
    Some(o.image.tags.first().cloned().unwrap_or_else(|| o.image.format.to_string()))
}

/// At most `n` characters of `s`, an ellipsis after a cut.
fn cut(s: String, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((at, _)) => format!("{}…", &s[..at]),
        None => s,
    }
}

/// The facts of the image at `path` as (label, value) lines: format, tags, platform (and
/// every platform of a multi-arch image), created, layers with their sizes, total size,
/// entrypoint, command, environment, exposed ports and labels.
pub fn facts(path: &Path) -> Option<Vec<(String, String)>> {
    let i = image(path)?;
    let c = &i.config;
    let mut out = vec![(t!("facts.format"), format!("{} {}", i.format, t!("container_image.kind")))];
    let join = |v: Vec<String>, sep: &str| cut(v.into_iter().take(64).collect::<Vec<_>>().join(sep), 2000);
    if !i.tags.is_empty() {
        out.push((t!("container_image.tags"), join(i.tags.clone(), ", ")));
    }
    if let Some(p) = platform(c) {
        out.push((t!("container_image.platform"), p));
    }
    if !i.platforms.is_empty() {
        out.push((t!("container_image.platforms"), join(i.platforms.clone(), ", ")));
    }
    if let Some(d) = c["created"].as_str().and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok()) {
        out.push((t!("container_image.created"), d.to_utc().format("%Y-%m-%d %H:%M UTC").to_string()));
    }
    let sizes: Vec<String> = i.layers.iter().map(|(_, n)| crate::settings::human(*n)).collect();
    out.push((t!("facts.layers"), format!("{}: {}", i.layers.len(), join(sizes, ", "))));
    out.push((t!("preview.total_size"), crate::settings::human(i.layers.iter().map(|(_, n)| n).sum())));
    let cfg = &c["config"];
    for (key, label) in [("Entrypoint", "container_image.entrypoint"), ("Cmd", "container_image.cmd")] {
        let v = strings(&cfg[key]);
        if !v.is_empty() {
            out.push((t!(label), join(v, " ")));
        }
    }
    let env = strings(&cfg["Env"]);
    if !env.is_empty() {
        out.push((t!("container_image.env"), join(env, ", ")));
    }
    let keys = |v: &Value| -> Vec<String> {
        v.as_object().into_iter().flatten().map(|(k, v)| match v.as_str() {
            Some(s) => format!("{k}={}", cut(s.to_string(), 200)),
            None => k.clone(),
        }).collect()
    };
    let ports = keys(&cfg["ExposedPorts"]);
    if !ports.is_empty() {
        out.push((t!("container_image.ports"), join(ports, ", ")));
    }
    let labels = keys(&cfg["Labels"]);
    if !labels.is_empty() {
        out.push((t!("container_image.labels"), join(labels, ", ")));
    }
    Some(out)
}

// ---------------------------------------------------------------- the merged tree

/// Where a file's bytes are: its layer, the entry's number in the layer's tar, and, for a
/// plain file, where its bytes start in the unpacked tar and how many (a sparse file's are
/// read through its entry: `at` is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub(crate) struct Src {
    pub layer: usize,
    pub entry: usize,
    pub at: Option<u64>,
    pub size: u64,
}

/// A file or folder of the merged tree. `src`: where its bytes are, for a file.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Node {
    pub size: u64,
    pub dir: bool,
    pub modified: u64,
    pub mode: u32,
    pub src: Option<Src>,
    /// A symbolic link's target until links are resolved.
    #[serde(skip)]
    link: Option<String>,
    /// A symbolic link resolved to its file, or a file in a folder a link copies: shown as
    /// that file, not counted again in folder sizes.
    #[serde(default)]
    pub linked: bool,
    /// The layer that made it: a whiteout takes away only what lower layers made.
    #[serde(skip)]
    layer: usize,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Tree {
    pub files: BTreeMap<String, Node>,
}

/// How a layer is packed, told by its first bytes.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Pack {
    Plain,
    Gz,
    Zst,
}

fn pack_of(head: &[u8]) -> Pack {
    if head.starts_with(&[0x1f, 0x8b]) {
        Pack::Gz
    } else if head.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        Pack::Zst
    } else {
        Pack::Plain
    }
}

fn cap(packed: u64) -> u64 {
    packed.saturating_mul(UNPACK_RATIO).clamp(UNPACK_FLOOR, UNPACK_MAX)
}

/// A layer's tar from its start, its compression (gzip, zstd, none) found from its first
/// bytes, and what it unpacks to capped. With `note`, the places it can be unpacked from again
/// (`tar_index`) are noted on the way.
fn layer(store: &Store, name: &str, packed: u64, note: Option<crate::tar_index::Points>) -> io::Result<(Box<dyn Read>, Pack)> {
    let mut r = BufReader::new(store.open(name)?);
    let pack = pack_of(r.fill_buf()?);
    let un: Box<dyn Read> = match pack {
        Pack::Gz => Box::new(crate::tar_index::Gz::new(r, note)),
        Pack::Zst => Box::new(crate::tar_index::Zst::new(r, note)),
        Pack::Plain => Box::new(r),
    };
    Ok((Box::new(Capped { inner: un, left: cap(packed) }), pack))
}

/// A layer's tar from a noted place on (`p.out` bytes into it).
fn layer_from(store: &Store, name: &str, packed: u64, pack: Pack, p: &crate::tar_index::Point) -> io::Result<Box<dyn Read>> {
    let src = BufReader::new(store.open_at(name, p.file)?);
    let un: Box<dyn Read> = match pack {
        Pack::Gz => Box::new(crate::tar_index::Gz::from(src, p)),
        Pack::Zst => Box::new(crate::tar_index::Zst::from(src, p)),
        Pack::Plain => Box::new(src),
    };
    Ok(Box::new(Capped { inner: un, left: cap(packed) }))
}

/// Where the places a layer of the image at `path` can be unpacked from are kept.
fn points_file(path: &Path, store: &Store, name: &str) -> Option<PathBuf> {
    let part = format!("image-layer-1\0{name}\0{}", store.blob_stamp(name));
    Some(crate::tar_index::kept_file(path, &part)?.with_extension("seek"))
}

/// Where the merged tree of the image at `path` is kept: it changes with any of its layers.
fn tree_file(path: &Path, o: &Opened) -> Option<PathBuf> {
    // 2: links marked, so folder sizes count each file once.
    let mut part = String::from("image-tree-2");
    for (name, packed) in &o.image.layers {
        part.push_str(&format!("\0{name}\0{packed}\0{}", o.store.blob_stamp(name)));
    }
    crate::tar_index::kept_file(path, &part)
}

/// A reader of exactly `left` bytes: one that ends before is a damaged layer, told.
struct Exact<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Exact<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 || buf.is_empty() {
            return Ok(0);
        }
        let n = buf.len().min(self.left.min(usize::MAX as u64) as usize);
        let got = self.inner.read(&mut buf[..n])?;
        if got == 0 {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        self.left -= got as u64;
        Ok(got)
    }
}

/// `n` bytes of `r` passed over; fewer is a damaged layer.
fn pass(r: &mut dyn Read, n: u64) -> io::Result<()> {
    if io::copy(&mut r.take(n), &mut io::sink())? != n {
        return Err(io::ErrorKind::UnexpectedEof.into());
    }
    Ok(())
}

/// A reader that fails past `left` bytes: a layer that unpacks to far more than it could hold
/// (a bomb) is stopped, not read to its end.
struct Capped<R> {
    inner: R,
    left: u64,
}

impl<R: Read> Read for Capped<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.left == 0 {
            // One byte more says whether it ends here.
            return match self.inner.read(&mut [0])? {
                0 => Ok(0),
                _ => Err(io::Error::new(io::ErrorKind::FileTooLarge, t!("container_image.unpacks_too_much"))),
            };
        }
        let n = buf.len().min(self.left.min(usize::MAX as u64) as usize);
        let got = self.inner.read(&mut buf[..n])?;
        self.left -= got as u64;
        Ok(got)
    }
}

/// A path of a layer as a path from the image's top: `./`, `/` and empty parts dropped;
/// `None` with `..` or a NUL, or too long or deep.
fn clean(name: &str) -> Option<String> {
    if name.contains('\0') || name.len() > 4096 {
        return None;
    }
    let parts: Vec<&str> = name.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
    (!parts.is_empty() && parts.len() <= 64 && !parts.contains(&"..")).then(|| parts.join("/"))
}

/// What is below `dir` (not `dir` itself), from layers under `layer`, taken away.
fn remove_below(files: &mut BTreeMap<String, Node>, dir: &str, layer: usize) {
    let prefix = if dir.is_empty() { String::new() } else { format!("{dir}/") };
    let gone: Vec<String> = files
        .range(prefix.clone()..)
        .take_while(|(k, _)| k.starts_with(&prefix))
        .filter(|(_, n)| n.layer < layer)
        .map(|(k, _)| k.clone())
        .collect();
    for k in gone {
        files.remove(&k);
    }
}

/// The image's layers applied in order into one tree.
pub(crate) fn tree(path: &Path) -> io::Result<Arc<Tree>> {
    let st = stamp(path)?;
    if let Some((.., t)) = TREES.lock().unwrap().iter().find(|(p, s, _)| p == path && *s == st) {
        return Ok(t.clone());
    }
    let o = opened(path)?.ok_or_else(|| bad("no manifest"))?;
    let file = tree_file(path, &o);
    let t = match file.as_deref().and_then(|f| kept_tree(f, &o)) {
        Some(t) => t,
        None => {
            let t = merge(&o, path)?;
            // Kept between runs, unless its names alone are too many to keep (`tar_index`).
            if let Some(f) = &file
                && t.files.keys().map(String::len).sum::<usize>() <= crate::tar_index::KEPT_NAMES
            {
                crate::tar_index::save_json(f, &t);
            }
            t
        }
    };
    let t = Arc::new(t);
    let mut trees = TREES.lock().unwrap();
    trees.retain(|(p, ..)| p != path);
    if trees.len() >= TREES_KEPT {
        trees.remove(0);
    }
    trees.push((path.to_path_buf(), st, t.clone()));
    Ok(t)
}

/// The tree kept at `file` by an earlier run, when it fits the image as it is now.
fn kept_tree(file: &Path, o: &Opened) -> Option<Tree> {
    let t: Tree = crate::tar_index::load_json(file)?;
    let fits = t.files.len() <= FILES_MAX
        && t.files.values().all(|n| n.src.is_none_or(|s| s.layer < o.image.layers.len() && s.size == n.size));
    fits.then_some(t)
}

/// Whether the merged tree of the image at `path` is known without unpacking its layers: in
/// memory, or kept by an earlier run.
pub(crate) fn tree_known(path: &Path) -> bool {
    let Ok(st) = stamp(path) else { return false };
    if TREES.lock().unwrap().iter().any(|(p, s, _)| p == path && *s == st) {
        return true;
    }
    let Ok(Some(o)) = opened(path) else { return false };
    tree_file(path, &o).is_some_and(|f| f.is_file())
}

fn merge(o: &Opened, path: &Path) -> io::Result<Tree> {
    let mut files: BTreeMap<String, Node> = BTreeMap::new();
    let mut read = 0usize;
    for (l, (name, packed)) in o.image.layers.iter().enumerate() {
        let mut ordinal = 0usize;
        let points = crate::tar_index::Points::default();
        let (r, pack) = layer(&o.store, name, *packed, Some(points.clone()))?;
        // What this layer's whiteouts take away from lower layers: a path and all below it
        // (`.wh.<name>`), or only what is below (an opaque folder). Applied when the layer is
        // read, once each.
        let mut gone: std::collections::HashMap<String, bool> = std::collections::HashMap::new();
        crate::archive::tar_named(tar::Archive::new(r).entries()?, |raw, link, e| {
            let at = ordinal;
            ordinal += 1;
            read += 1;
            if read > READ_MAX || files.len() + gone.len() > FILES_MAX {
                return Err(io::Error::new(
                    io::ErrorKind::FileTooLarge,
                    t!("container_image.too_many", "n" => FILES_MAX.to_string()),
                ));
            }
            let Some(p) = clean(&raw) else { return Ok(true) };
            let (parent, base) = p.rsplit_once('/').unwrap_or(("", &p));
            // Whiteouts: what lower layers have there goes.
            if base == ".wh..wh..opq" {
                gone.entry(parent.to_string()).or_insert(false);
                return Ok(true);
            }
            if let Some(name) = base.strip_prefix(".wh.") {
                let path = if parent.is_empty() { name.to_string() } else { format!("{parent}/{name}") };
                gone.insert(path, true);
                return Ok(true);
            }
            let h = e.header();
            let ty = h.entry_type();
            let mut node = Node {
                size: 0,
                dir: ty.is_dir(),
                modified: h.mtime().unwrap_or(0),
                mode: h.mode().unwrap_or(0o644),
                src: None,
                link: None,
                linked: false,
                layer: l,
            };
            if ty.is_file() || ty.is_gnu_sparse() {
                let start = (!ty.is_gnu_sparse()).then(|| e.raw_file_position());
                node.size = e.size();
                node.src = Some(Src { layer: l, entry: at, at: start, size: node.size });
            } else if ty.is_hard_link() {
                // Only a file already in the merged tree.
                let Some(target) = link.as_deref().and_then(clean) else { return Ok(true) };
                let Some(t) = files.get(&target).filter(|t| !t.dir) else { return Ok(true) };
                node = Node { layer: l, ..t.clone() };
            } else if ty.is_symlink() {
                let Some(target) = link.filter(|t| !t.contains('\0') && t.len() <= 4096) else { return Ok(true) };
                node.link = Some(target);
            } else if !node.dir {
                // A device or a pipe: not a file to copy out.
                return Ok(true);
            }
            // A file (or link) in a folder's place takes the folder's contents away; a lower
            // file in the place of a folder this one is in becomes a folder.
            if !node.dir {
                remove_below(&mut files, &p, usize::MAX);
            }
            let mut up = parent;
            while !up.is_empty() {
                match files.get_mut(up) {
                    Some(n) if n.dir => break,
                    Some(n) => *n = Node { dir: true, size: 0, src: None, link: None, ..n.clone() },
                    None => {}
                }
                up = up.rsplit_once('/').map_or("", |(a, _)| a);
            }
            files.insert(p, node);
            Ok(true)
        })?;
        // A whiteout inside a folder another one takes away already does nothing more: left
        // out, so every entry is looked at once per layer however many there are.
        let covered = |path: &str| {
            if !path.is_empty() && gone.contains_key("") {
                return true;
            }
            let mut up = path;
            while let Some((a, _)) = up.rsplit_once('/') {
                if gone.contains_key(a) {
                    return true;
                }
                up = a;
            }
            false
        };
        // Where it can be unpacked from again, for the files read out of it later.
        if pack != Pack::Plain
            && let Some(f) = points_file(path, &o.store, name)
        {
            crate::tar_index::save_points_to(&f, &points.borrow());
        }
        let roots: Vec<(String, bool)> =
            gone.iter().filter(|(p, _)| !covered(p)).map(|(p, s)| (p.clone(), *s)).collect();
        for (path, with_itself) in roots {
            if with_itself && files.get(&path).is_some_and(|n| n.layer < l) {
                files.remove(&path);
            }
            remove_below(&mut files, &path, l);
        }
    }
    resolve_links(&mut files);
    Ok(Tree { files })
}

/// Each symbolic link as what it leads to inside the tree (an absolute target from the
/// image's top, `..` never above it): a file as that file, a folder as a copy of what is in it
/// (`/bin -> usr/bin`), so it can be opened and copied out like the folder. Links to nothing,
/// round in circles, or to a folder they are in go.
fn resolve_links(files: &mut BTreeMap<String, Node>) {
    let links: Vec<String> = files.iter().filter(|(_, n)| n.link.is_some()).map(|(k, _)| k.clone()).collect();
    // Past the image's budget, the links left go.
    let mut work = LINK_WORK_MAX;
    let targets: Vec<Option<String>> = links.iter().map(|at| resolve(files, at, &mut work)).collect();
    let mut folders = vec![];
    for (at, to) in links.iter().zip(targets) {
        let link = files.remove(at).expect("listed");
        let Some(to) = to else { continue };
        match files.get(&to) {
            Some(n) if !n.dir && n.link.is_none() => {
                let n = Node { layer: link.layer, modified: link.modified, linked: true, ..n.clone() };
                files.insert(at.clone(), n);
            }
            Some(n) if n.link.is_some() => {}
            // A folder, also one only named in the paths below it.
            Some(_) => folders.push((at.clone(), to, link)),
            None if files.range(format!("{to}/")..).next().is_some_and(|(k, _)| k.starts_with(&format!("{to}/"))) => {
                folders.push((at.clone(), to, link))
            }
            None => {}
        }
    }
    // Folders last, from what is there before any is copied: a link inside one copied is
    // already its file (or gone), so nothing is copied twice over.
    let mut copies = vec![];
    for (at, to, link) in folders {
        let prefix = format!("{to}/");
        if at == to || at.starts_with(&prefix) {
            continue;
        }
        copies.push((at.clone(), Node { dir: true, size: 0, src: None, link: None, ..link }));
        for (k, n) in files.range(prefix.clone()..).take_while(|(k, _)| k.starts_with(&prefix)) {
            if n.link.is_none() {
                copies.push((format!("{at}/{}", &k[prefix.len()..]), Node { linked: true, ..n.clone() }));
            }
            if files.len() + copies.len() > FILES_MAX {
                return;
            }
        }
    }
    for (k, n) in copies {
        files.entry(k).or_insert(n);
    }
}

/// Where the link at `at` leads, following the links on the way: `HOPS` at most, and
/// `PARTS_MAX` path parts walked, each counted from `work` too.
fn resolve(files: &BTreeMap<String, Node>, at: &str, work: &mut usize) -> Option<String> {
    let start = files.get(at)?.link.as_deref()?;
    // The path so far as one string, and where each of its parts starts.
    let (mut path, mut starts) = (String::new(), Vec::<usize>::new());
    let push = |path: &mut String, starts: &mut Vec<usize>, part: &str| {
        starts.push(path.len());
        if !path.is_empty() {
            path.push('/');
        }
        path.push_str(part);
    };
    if !start.starts_with('/')
        && let Some((dir, _)) = at.rsplit_once('/')
    {
        for p in dir.split('/') {
            push(&mut path, &mut starts, p);
        }
    }
    let mut todo: VecDeque<&str> = start.split('/').collect();
    let (mut hops, mut walked) = (0, 0);
    while let Some(c) = todo.pop_front() {
        walked += 1;
        if walked > PARTS_MAX || *work == 0 {
            return None;
        }
        *work -= 1;
        match c {
            "" | "." => {}
            ".." => {
                if let Some(at) = starts.pop() {
                    path.truncate(at);
                }
            }
            _ => {
                push(&mut path, &mut starts, c);
                if let Some(t) = files.get(&path).and_then(|n| n.link.as_deref()) {
                    hops += 1;
                    if hops > HOPS {
                        return None;
                    }
                    path.truncate(starts.pop().unwrap_or(0));
                    if t.starts_with('/') {
                        (path, starts) = (String::new(), vec![]);
                    }
                    for p in t.split('/').rev() {
                        todo.push_front(p);
                    }
                }
            }
        }
    }
    (!path.is_empty()).then_some(path)
}

/// The files `wanted` (a source in the tree, where it goes, its modified time) out of the
/// image's layers: `write` makes each file from its bytes.
pub(crate) fn read_out(
    path: &Path,
    wanted: &[(Src, PathBuf, u64)],
    write: &dyn Fn(&Path, &mut dyn Read, u64) -> io::Result<()>,
) -> io::Result<()> {
    let picked = wanted.iter().map(|(src, to, secs)| (*src, (to.as_path(), *secs))).collect();
    each(path, picked, &mut |to: &[(&Path, u64)], r| {
        write(to[0].0, r, to[0].1)?;
        // The same bytes under other names (hard links, links to it): from the first.
        for (more, secs) in &to[1..] {
            write(more, &mut File::open(to[0].0)?, *secs)?;
        }
        Ok(true)
    })
}

/// The files of the image at `path` that `wanted` picks by name and size (folders never),
/// for search: `read` gets each with its names, and says whether to go on.
pub(crate) fn search_read(
    path: &Path,
    wanted: &dyn Fn(&str, u64) -> bool,
    read: &mut dyn FnMut(&[&str], &mut dyn Read) -> bool,
) -> io::Result<()> {
    let t = tree(path)?;
    let picked = t
        .files
        .iter()
        .filter_map(|(name, n)| Some((n.src?, name.as_str())))
        .filter(|(s, name)| wanted(name, s.size))
        .collect();
    // The same bytes under several names (links) are read once, for all of them.
    each(path, picked, &mut |names: &[&str], r| Ok(read(names, r)))
}

/// What is done with each file's bytes, given what it was wanted with: whether to go on.
type Take<'a, T> = dyn FnMut(&[T], &mut dyn Read) -> io::Result<bool> + 'a;

/// Each of `wanted` (a source, and what `f` is given with its bytes) out of the image's
/// layers, in the order they are in them. A file whose place in its layer is known is reached
/// from the nearest place before it the layer can be unpacked from (`tar_index`'s points, kept
/// when the tree was made; a plain layer: at once); others are found by reading their layer
/// through, once. `f` says whether to go on.
fn each<T>(
    path: &Path,
    wanted: Vec<(Src, T)>,
    f: &mut Take<'_, T>,
) -> io::Result<()> {
    let o = opened(path)?.ok_or_else(|| bad("no manifest"))?;
    let mut by_layer: BTreeMap<usize, BTreeMap<Src, Vec<T>>> = BTreeMap::new();
    for (src, t) in wanted {
        by_layer.entry(src.layer).or_default().entry(src).or_default().push(t);
    }
    for (l, want) in by_layer {
        let (name, packed) = o.image.layers.get(l).ok_or_else(|| bad("layer"))?;
        let go = if want.keys().all(|s| s.at.is_some()) {
            by_place(path, &o, name, *packed, want, f)?
        } else {
            by_entry(&o, name, *packed, want, f)?
        };
        if !go {
            break;
        }
    }
    Ok(())
}

/// `each` in one layer whose files' places are known.
fn by_place<T>(
    path: &Path,
    o: &Opened,
    name: &str,
    packed: u64,
    want: BTreeMap<Src, Vec<T>>,
    f: &mut Take<'_, T>,
) -> io::Result<bool> {
    let pack = pack_of(BufReader::new(o.store.open(name)?).fill_buf()?);
    let points = match pack {
        Pack::Plain => vec![],
        _ => points_file(path, &o.store, name)
            .and_then(|p| crate::tar_index::load_points_from(&p))
            .unwrap_or_default(),
    };
    // The layer unpacked so far, and how far.
    let mut cur: Option<(Box<dyn Read>, u64)> = None;
    for (s, ts) in want {
        let at = s.at.expect("known places only");
        if pack == Pack::Plain {
            // A plain layer is read where the file is.
            cur = Some((Box::new(o.store.open_at(name, at)?), at));
        } else {
            let p = points.iter().rev().find(|p| p.out > 0 && p.out <= at);
            let behind = cur.as_ref().is_none_or(|(_, pos)| p.is_some_and(|p| p.out > *pos));
            if behind {
                cur = Some(match p {
                    Some(p) => (layer_from(&o.store, name, packed, pack, p)?, p.out),
                    None => (layer(&o.store, name, packed, None)?.0, 0),
                });
            }
        }
        let (r, pos) = cur.as_mut().expect("opened above");
        pass(r, at - *pos)?;
        let mut e = Exact { inner: &mut *r, left: s.size };
        let go = f(&ts, &mut e)?;
        // What `f` left of it, so the next file is where it is said to be.
        io::copy(&mut e, &mut io::sink())?;
        *pos = at + s.size;
        if !go {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `each` in one layer, read through from its start once, its files found by their entry.
fn by_entry<T>(
    o: &Opened,
    name: &str,
    packed: u64,
    mut want: BTreeMap<Src, Vec<T>>,
    f: &mut Take<'_, T>,
) -> io::Result<bool> {
    let mut by_entry: HashMap<usize, Src> = want.keys().map(|s| (s.entry, *s)).collect();
    let mut ordinal = 0usize;
    let mut go = true;
    crate::archive::tar_named(tar::Archive::new(layer(&o.store, name, packed, None)?.0).entries()?, |_, _, e| {
        let at = ordinal;
        ordinal += 1;
        if let Some(s) = by_entry.remove(&at)
            && let Some(ts) = want.remove(&s)
        {
            go = f(&ts, e)?;
        }
        Ok(go && !want.is_empty())
    })?;
    Ok(go)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A tar of `(name, bytes)` files; a name ending in `/` is a folder, `name -> target` a
    /// link, `name => target` a hard link.
    pub(crate) fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(vec![]);
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_mode(0o755);
            h.set_mtime(1_700_000_000);
            if let Some((n, t)) = name.split_once(" -> ") {
                h.set_entry_type(tar::EntryType::Symlink);
                h.set_size(0);
                b.append_link(&mut h, n, t).unwrap();
            } else if let Some((n, t)) = name.split_once(" => ") {
                h.set_entry_type(tar::EntryType::Link);
                h.set_size(0);
                b.append_link(&mut h, n, t).unwrap();
            } else if name.ends_with('/') {
                h.set_entry_type(tar::EntryType::Directory);
                h.set_size(0);
                b.append_data(&mut h, name, &[][..]).unwrap();
            } else {
                h.set_size(data.len() as u64);
                if name.len() > 100 {
                    b.append_data(&mut h, name, *data).unwrap();
                    continue;
                }
                // Raw names, so a test can give `..` or `/` that the builder would refuse.
                h.as_gnu_mut().unwrap().name[..name.len()].copy_from_slice(name.as_bytes());
                h.set_cksum();
                b.append(&h, *data).unwrap();
            }
        }
        b.into_inner().unwrap()
    }

    fn gz(b: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut w = flate2::write::GzEncoder::new(vec![], flate2::Compression::fast());
        w.write_all(b).unwrap();
        w.finish().unwrap()
    }

    fn digest(b: &[u8]) -> String {
        use sha2::Digest;
        sha2::Sha256::digest(b).iter().map(|x| format!("{x:02x}")).collect()
    }

    const CONFIG: &str = r#"{"architecture":"amd64","os":"linux","created":"2026-10-01T12:00:00Z",
        "config":{"Entrypoint":["/bin/sh"],"Cmd":["-c","echo hi"],"Env":["PATH=/usr/bin"],
        "ExposedPorts":{"80/tcp":{}},"Labels":{"org.opencontainers.image.title":"demo"}}}"#;

    fn zst(b: &[u8]) -> Vec<u8> {
        let mut out = vec![];
        ruzstd::encoding::compress(b, &mut out, ruzstd::encoding::CompressionLevel::Fastest);
        out
    }

    /// A `docker save` tar of `layers` (plain, gzip, zstd in turn), tagged demo:1.
    pub(crate) fn docker_tar(layers: &[Vec<u8>]) -> Vec<u8> {
        let packed: Vec<Vec<u8>> = layers
            .iter()
            .enumerate()
            .map(|(i, l)| match i % 3 {
                1 => gz(l),
                2 => zst(l),
                _ => l.clone(),
            })
            .collect();
        docker_tar_of(&packed)
    }

    /// A `docker save` tar of `layers` as they are given, packed or not.
    pub(crate) fn docker_tar_of(layers: &[Vec<u8>]) -> Vec<u8> {
        let mut files: Vec<(String, Vec<u8>)> = vec![];
        let mut names = vec![];
        for l in layers {
            let l = l.clone();
            let n = format!("blobs/sha256/{}", digest(&l));
            names.push(n.clone());
            files.push((n, l));
        }
        files.push(("config.json".into(), CONFIG.as_bytes().to_vec()));
        let m = serde_json::json!([{"Config": "config.json", "RepoTags": ["demo:1"], "Layers": names}]);
        files.push(("manifest.json".into(), m.to_string().into_bytes()));
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        tar_of(&refs)
    }

    /// An OCI layout folder at `dir` of `layers`, an index of two platforms.
    pub(crate) fn oci_dir(dir: &Path, layers: &[Vec<u8>]) {
        let put = |b: &[u8]| {
            let d = digest(b);
            std::fs::create_dir_all(dir.join("blobs/sha256")).unwrap();
            std::fs::write(dir.join("blobs/sha256").join(&d), b).unwrap();
            (format!("sha256:{d}"), b.len())
        };
        let ls: Vec<Value> = layers.iter().map(|l| {
            let (d, n) = put(l);
            serde_json::json!({"mediaType": "application/vnd.oci.image.layer.v1.tar", "digest": d, "size": n})
        }).collect();
        let (c, cn) = put(CONFIG.as_bytes());
        let m = serde_json::json!({"schemaVersion": 2, "config": {"digest": c, "size": cn}, "layers": ls}).to_string();
        let (md, mn) = put(m.as_bytes());
        let ix = serde_json::json!({"schemaVersion": 2, "manifests": [
            {"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": md, "size": mn, "platform": {"os": "linux", "architecture": "amd64"}},
            {"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": md, "size": mn, "platform": {"os": "linux", "architecture": "arm64", "variant": "v8"}},
            {"mediaType": "application/vnd.oci.image.manifest.v1+json", "digest": md, "size": mn, "platform": {"os": "unknown", "architecture": "unknown"}}
        ], "annotations": {}}).to_string();
        let mut ix: Value = serde_json::from_str(&ix).unwrap();
        ix["manifests"][0]["annotations"] = serde_json::json!({"org.opencontainers.image.ref.name": "demo:2"});
        std::fs::write(dir.join("index.json"), ix.to_string()).unwrap();
        std::fs::write(dir.join("oci-layout"), r#"{"imageLayoutVersion":"1.0.0"}"#).unwrap();
    }

    fn names(t: &Tree) -> Vec<&str> {
        t.files.keys().map(String::as_str).collect()
    }

    #[test]
    fn layers_merge_with_whiteouts_and_links() {
        let dir = crate::pictures::tests::folder("image-merge");
        let low = tar_of(&[
            ("etc/", b""), ("etc/os", b"alpine"), ("etc/gone", b"x"), ("var/", b""),
            ("var/cache/", b""), ("var/cache/a", b"aa"), ("bin/", b""), ("bin/sh", b"#!sh"),
            ("old/", b""), ("old/f", b"f"),
        ]);
        let high = tar_of(&[
            ("etc/.wh.gone", b""), ("var/cache/.wh..wh..opq", b""), ("var/cache/b", b"bb"),
            ("usr/", b""), ("usr/sh -> /bin/sh", b""), ("usr/up -> ../../../../etc/os", b""),
            ("usr/dir -> /etc", b""), ("etc/me -> ../etc", b""), ("usr/loop -> loop", b""), ("usr/hard => bin/sh", b""),
            ("old", b"now a file"),
        ]);
        let img = dir.join("demo.tar");
        std::fs::write(&img, docker_tar(&[low, high, tar_of(&[("zst.txt", b"z")])])).unwrap();
        assert!(is_image(&img));
        let t = tree(&img).unwrap();
        assert_eq!(
            names(&t),
            ["bin", "bin/sh", "etc", "etc/os", "old", "usr", "usr/dir", "usr/dir/os", "usr/hard", "usr/sh", "usr/up", "var", "var/cache", "var/cache/b", "zst.txt"]
        );
        // Links lead inside the image only, `..` stopping at its top.
        assert_eq!(t.files["usr/up"].src, t.files["etc/os"].src);
        assert_eq!(t.files["usr/sh"].size, 4);
        assert_eq!(t.files["usr/hard"].src, t.files["bin/sh"].src);
        assert!(!t.files["old"].dir);
        // Copied out through the archive machinery, the links' bytes too.
        let out = dir.join("out");
        crate::archive::copy_out(&img, "usr", &out, None, false).unwrap();
        assert_eq!(std::fs::read(out.join("sh")).unwrap(), b"#!sh");
        assert_eq!(std::fs::read(out.join("up")).unwrap(), b"alpine");
        let f = facts(&img).unwrap();
        let v = |k: &str| f.iter().find(|(l, _)| l == &t!(k)).map(|(_, v)| v.clone());
        assert_eq!(v("container_image.tags").as_deref(), Some("demo:1"));
        assert_eq!(v("container_image.platform").as_deref(), Some("linux/amd64"));
        assert_eq!(v("container_image.ports").as_deref(), Some("80/tcp"));
        assert_eq!(title(&img).as_deref(), Some("demo:1"));
        assert!(crate::archive::read_only(&img));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Links show as their files, but a folder's size counts each file once: alpine's bin/ of
    /// busybox applets is busybox's size, not busybox's times the links to it.
    #[test]
    fn folder_sizes_count_a_linked_file_once() {
        let dir = crate::pictures::tests::folder("image-link-sizes");
        let layer = tar_of(&[
            ("bin/", b""), ("bin/busybox", b"0123456789"), ("bin/ls -> busybox", b""), ("bin/cat -> /bin/busybox", b""),
            ("usr/", b""), ("usr/bin -> /bin", b""), ("etc/", b""), ("etc/os", b"alpine"),
        ]);
        let img = dir.join("alpine.tar");
        std::fs::write(&img, docker_tar(&[layer])).unwrap();
        let size = |inner: &str, name: &str| {
            crate::archive::list_in(&img, inner).unwrap().into_iter().find(|e| e.name == name).map(|e| e.size)
        };
        assert_eq!(size("bin", "ls"), Some(10), "a link shows as its file");
        assert_eq!(size("", "bin"), Some(10));
        assert_eq!(size("", "usr"), Some(0), "a link to a folder adds nothing");
        assert_eq!(size("", "etc"), Some(6));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hostile_paths_stay_inside() {
        let dir = crate::pictures::tests::folder("image-hostile");
        let evil = tar_of(&[
            ("../../escape", b"x"), ("/abs", b"abs"), ("a/../../b", b"x"), ("ok/./f", b"f"),
            ("ok/out -> ../../../../etc/passwd", b""), ("ok/hl => ../../etc/passwd", b""),
            ("ok/self -> self", b""), ("ok/a -> b", b""), ("ok/b -> a", b""),
        ]);
        let img = dir.join("evil.tar");
        std::fs::write(&img, docker_tar(&[evil])).unwrap();
        let t = tree(&img).unwrap();
        assert_eq!(names(&t), ["abs", "ok/f"]);
        let out = dir.join("out");
        std::fs::create_dir(&out).unwrap();
        crate::archive::extract(&img, &out).unwrap();
        assert!(!dir.join("escape").exists() && !dir.join("b").exists());
        assert_eq!(std::fs::read(out.join("evil/abs")).unwrap(), b"abs");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spoiled_images_fail_without_panic() {
        let dir = crate::pictures::tests::folder("image-spoiled");
        let good = docker_tar(&[tar_of(&[("a/b.txt", b"b")]), tar_of(&[("a/.wh.b.txt", b""), ("c -> a", b"")])]);
        let n = std::cell::Cell::new(0);
        crate::pictures::tests::hostile(&good, |b| {
            n.set(n.get() + 1);
            let p = dir.join(format!("{}.tar", n.get()));
            std::fs::write(&p, b).unwrap();
            let _ = crate::archive::list(&p, 100);
            let _ = facts(&p);
            let _ = std::fs::remove_file(&p);
        });
        // A manifest naming a layer outside the tar is no image: the tar opens as the tar it is.
        let m = serde_json::json!([{"Config": "../../etc/passwd", "Layers": ["../../etc/passwd", "/etc/passwd"]}]);
        let p = dir.join("bad.tar");
        std::fs::write(&p, tar_of(&[("manifest.json", m.to_string().as_bytes())])).unwrap();
        assert!(!is_image(&p));
        let (l, _) = crate::archive::list(&p, 10).unwrap();
        assert_eq!(l.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["manifest.json"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `f` within `secs` seconds, else the test fails.
    fn quick(secs: u64, f: impl FnOnce()) {
        let t = std::time::Instant::now();
        f();
        // A debug build on a loaded runner is many times slower: `test_limit` allows for it.
        let limit = crate::test_limit(std::time::Duration::from_secs(secs));
        assert!(t.elapsed() < limit, "took {:?}", t.elapsed());
    }

    #[test]
    fn an_index_naming_itself_is_read_once() {
        let dir = crate::pictures::tests::folder("image-index-loop");
        oci_dir(&dir, &[tar_of(&[("a.txt", b"a")])]);
        // A nested index that names itself a thousand times, and the real manifest.
        let ix: Value = serde_json::from_str(&std::fs::read_to_string(dir.join("index.json")).unwrap()).unwrap();
        let real = ix["manifests"][0].clone();
        let me = "sha256:".to_string() + &"0".repeat(64);
        let mut loopy = serde_json::json!({"schemaVersion": 2, "manifests": [real]});
        for _ in 0..1000 {
            loopy["manifests"].as_array_mut().unwrap().push(serde_json::json!({"mediaType": "application/vnd.oci.image.index.v1+json", "digest": me}));
        }
        std::fs::write(dir.join("blobs/sha256").join("0".repeat(64)), loopy.to_string()).unwrap();
        let top = serde_json::json!({"schemaVersion": 2, "manifests": [{"mediaType": "application/vnd.oci.image.index.v1+json", "digest": me}]});
        std::fs::write(dir.join("index.json"), top.to_string()).unwrap();
        let ix = dir.join("index.json");
        quick(5, || {
            let (entries, _) = crate::archive::listing(&ix, "").unwrap();
            assert!(entries.iter().any(|e| e.name == "a.txt"));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn many_opaque_folders_and_long_link_chains_stay_quick() {
        let dir = crate::pictures::tests::folder("image-opaque");
        // A lower layer of 20,000 files in a deep tree; above it 100,000 opaque markers, one in
        // each folder down that tree, and a chain of links each through the one before.
        let deep: Vec<String> = (0..60).map(|i| format!("d{i}")).collect();
        let mut low: Vec<(String, Vec<u8>)> = vec![];
        for i in 0..20_000 {
            let depth = i % 60;
            low.push((format!("{}/f{i}", deep[..=depth].join("/")), b"x".to_vec()));
        }
        let mut high: Vec<(String, Vec<u8>)> = vec![];
        for i in 0..100_000 {
            // Half in folders of their own, half (again and again) in the lower tree's.
            let depth = 1 + i % 59;
            let at = if i % 2 == 0 { format!("{}/o{i}", deep[..=depth].join("/")) } else { deep[..=depth].join("/") };
            high.push((format!("{at}/.wh..wh..opq"), vec![]));
        }
        high.push(("l0 -> d0/f0".into(), vec![]));
        for i in 1..2000 {
            high.push((format!("l{i} -> l{}/../l{}", i - 1, i - 1), vec![]));
        }
        let refs = |v: &[(String, Vec<u8>)]| -> Vec<u8> {
            let r: Vec<(&str, &[u8])> = v.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
            tar_of(&r)
        };
        let img = dir.join("opaque.tar");
        std::fs::write(&img, docker_tar(&[refs(&low), refs(&high)])).unwrap();
        quick(20, || {
            let t = tree(&img).unwrap();
            assert!(t.files.contains_key("d0/f0") && t.files.contains_key("l0"));
            assert!(!t.files.contains_key("d0/d1/f1"), "an opaque folder takes away what was below");
            // Each hop adds parts: a chain past the caps is left out, not followed for ever.
            assert!(!t.files.contains_key("l1999"));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_layer_bomb_is_stopped() {
        // 3 GiB of zeros packs to a few MB: far past 200 times that.
        let mut c = Capped { inner: io::repeat(0).take(3 << 30), left: UNPACK_FLOOR };
        let e = io::copy(&mut c, &mut io::sink()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::FileTooLarge);
        let mut ok = Capped { inner: io::repeat(0).take(10), left: 10 };
        assert_eq!(io::copy(&mut ok, &mut io::sink()).unwrap(), 10);
    }

    #[test]
    fn oci_layout_folder_and_platforms() {
        let dir = crate::pictures::tests::folder("image-oci");
        let l = tar_of(&[("hello.txt", b"hi")]);
        oci_dir(&dir, &[l]);
        let ix = dir.join("index.json");
        assert!(layout_index(&ix) && crate::archive::opens(&ix));
        let f = facts(&ix).unwrap();
        let v = |k: &str| f.iter().find(|(l, _)| l == &t!(k)).map(|(_, v)| v.clone());
        assert_eq!(v("container_image.platforms").as_deref(), Some("linux/amd64, linux/arm64/v8"));
        assert_eq!(v("container_image.tags").as_deref(), Some("demo:2"));
        let (entries, _) = crate::archive::listing(&ix, "").unwrap();
        assert!(entries.iter().any(|e| e.name == "hello.txt" && e.size == 2));
        // A digest that is no digest is never a path.
        assert_eq!(blob("sha256:../../etc/passwd"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Text that packs as text does, different for each `seed`.
    fn text(n: usize, seed: u32) -> Vec<u8> {
        let mut x = seed.wrapping_mul(2_654_435_761) | 1;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if i % 7 < 4 { b"the quick brown fox "[i % 20] } else { (x % 40) as u8 + b'0' }
            })
            .collect()
    }

    fn forget(img: &Path) {
        TREES.lock().unwrap().retain(|(p, ..)| p != img);
    }

    #[test]
    fn files_are_read_from_a_place_near_them_and_the_tree_is_kept() {
        let dir = crate::pictures::tests::folder("image-seek");
        // 12 MB of files in a gzip layer and in a zstd layer of 1 MB frames: far apart enough
        // for places to unpack from between them.
        let files: Vec<(String, Vec<u8>)> = (0..8).map(|i| (format!("big/f{i}.txt"), text(1_500_000, i))).collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, b)| (n.as_str(), b.as_slice())).collect();
        let plain = tar_of(&refs);
        let frames: Vec<u8> = plain.chunks(1 << 20).flat_map(zst).collect();
        let low = tar_of(&[("etc/", b""), ("etc/os", b"alpine")]);
        let img = dir.join("seek.tar");
        let mut packed = flate2::write::GzEncoder::new(vec![], flate2::Compression::default());
        io::Write::write_all(&mut packed, &plain).unwrap();
        std::fs::write(&img, docker_tar_of(&[low, packed.finish().unwrap(), frames])).unwrap();
        let t = tree(&img).unwrap();
        let o = opened(&img).unwrap().unwrap();
        assert!(tree_file(&img, &o).unwrap().is_file(), "the tree is kept");
        for (name, _) in &o.image.layers[1..] {
            let p = crate::tar_index::load_points_from(&points_file(&img, &o.store, name).unwrap()).expect("points kept");
            assert!(p.iter().filter(|p| p.out > 0).count() >= 2, "{name}: {}", p.len());
        }
        assert!(t.files["big/f7.txt"].src.unwrap().at.is_some());
        // Each file comes out whole, from the place before it, in either layer.
        let check = |what: &str| {
            for (i, (name, body)) in files.iter().enumerate() {
                let out = dir.join(format!("out-{what}-{i}"));
                std::fs::create_dir_all(&out).unwrap();
                let got = crate::archive::copy_out(&img, name, &out, None, false).unwrap();
                assert!(std::fs::read(got).unwrap() == *body, "{what}: {name}");
            }
        };
        check("points");
        // All of them in one go, the lower layer's too: each layer read on from where it is.
        let all = dir.join("all");
        std::fs::create_dir(&all).unwrap();
        crate::archive::extract(&img, &all).unwrap();
        assert_eq!(std::fs::read(all.join("seek/etc/os")).unwrap(), b"alpine");
        assert!(std::fs::read(all.join("seek/big/f5.txt")).unwrap() == files[5].1);
        // Damaged points are not followed: the layer is unpacked from its start.
        for (name, _) in &o.image.layers[1..] {
            std::fs::write(points_file(&img, &o.store, name).unwrap(), b"CXSEEK1\nrubbish").unwrap();
        }
        check("damaged");
        // A later run reads the kept tree, not the layers.
        let kept = tree_file(&img, &o).unwrap();
        let mut saved: Tree = crate::tar_index::load_json(&kept).unwrap();
        let node = saved.files["etc/os"].clone();
        saved.files.insert("etc/kept".into(), node);
        crate::tar_index::save_json(&kept, &saved);
        forget(&img);
        assert!(tree_known(&img));
        assert!(tree(&img).unwrap().files.contains_key("etc/kept"));
        // One naming a layer the image does not have is no tree of it.
        let mut wrong = saved;
        wrong.files.get_mut("etc/kept").unwrap().src.as_mut().unwrap().layer = 9;
        crate::tar_index::save_json(&kept, &wrong);
        forget(&img);
        assert!(!tree(&img).unwrap().files.contains_key("etc/kept"));
        // Search: the merged files by name, and their bytes.
        let size = std::fs::metadata(&img).unwrap().len();
        let names: Vec<String> = crate::archive::search_entries(&img, size).unwrap().into_iter().map(|e| e.name).collect();
        assert!(names.contains(&"big/f3.txt".to_string()) && names.contains(&"etc/os".to_string()));
        assert!(!names.iter().any(|n| n.starts_with("blobs/")), "never the blobs");
        let mut got = vec![];
        crate::archive::search_read(&img, size, &|n, _| n.ends_with("f2.txt") || n == "etc/os", &|_| false, &mut |n, r| {
            let mut b = vec![];
            r.read_to_end(&mut b).unwrap();
            got.push((n[0].to_string(), b));
            true
        })
        .unwrap();
        assert_eq!(got.len(), 2);
        assert!(got.iter().any(|(n, b)| n == "big/f2.txt" && *b == files[2].1));
        assert!(got.iter().any(|(n, b)| n == "etc/os" && b == b"alpine"));
        // Changed: neither its tree nor its points are taken for the new one.
        std::fs::write(&img, docker_tar_of(&[tar_of(&[("new.txt", b"new")])])).unwrap();
        assert!(!tree_known(&img));
        assert_eq!(names_of(&tree(&img).unwrap()), ["new.txt"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn names_of(t: &Tree) -> Vec<String> {
        t.files.keys().cloned().collect()
    }

    #[test]
    fn a_plain_tar_stays_a_tar() {
        let dir = crate::pictures::tests::folder("image-plain");
        let p = dir.join("plain.tar");
        std::fs::write(&p, tar_of(&[("a.txt", b"a")])).unwrap();
        assert!(!is_image(&p) && !crate::archive::read_only(&p));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
