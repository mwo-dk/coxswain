//! Local models by name, for both apps. Ollama keeps its models as blobs named `sha256-…`; its
//! manifests (`manifests/<host>/<namespace>/<model>/<tag>`) say which blob is which model, so a
//! panel in `blobs/` (and Space) shows `llama3.2:3b` beside each. The Hugging Face cache (used by
//! Lemonade, llama.cpp, vLLM) keeps `models--org--name` folders, shown as `org/name`, with hashed
//! `blobs/` that its snapshots link to by their real names. LM Studio's names are plain already.
//!
//! A model file's facts (`facts`): a GGUF's or a safetensors file's header (architecture,
//! parameters, quantisation, context, chat template, licence, tensors by layer), the memory it
//! takes to load and whether that fits this machine. Only headers are read, every length and
//! count checked and capped: a model file is input like any other. A PyTorch pickle is named
//! and never opened, as reading one can run code.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use crate::t;

/// File name → the model it is, for the entries of `dir`.
pub type Names = Arc<HashMap<String, String>>;

/// The names of the entries of `dir` when it is a model store's folder: Ollama's `blobs/`, a
/// Hugging Face repository's `blobs/`, or the Hugging Face cache itself. `None` for any other
/// folder, decided from its name before anything is read. Kept a few seconds, as Space asks
/// for it at every frame.
pub fn names(dir: &Path) -> Option<Names> {
    let leaf = dir.file_name()?.to_str()?;
    let read: fn(&Path) -> HashMap<String, String> = if leaf == "blobs" {
        let up = dir.parent()?;
        if up.join("manifests").is_dir() {
            ollama
        } else if up.join("snapshots").is_dir() {
            hf_blobs
        } else {
            return None;
        }
    } else if is_hub(dir) {
        hf_hub
    } else {
        return None;
    };
    type Cache = HashMap<PathBuf, (Instant, Names)>;
    static CACHE: Mutex<Option<Cache>> = Mutex::new(None);
    let mut c = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let c = c.get_or_insert_with(HashMap::new);
    if let Some((at, n)) = c.get(dir)
        && at.elapsed() < Duration::from_secs(5)
    {
        return Some(n.clone());
    }
    let n: Names = Arc::new(read(dir));
    if c.len() > 64 {
        c.clear();
    }
    c.insert(dir.to_path_buf(), (Instant::now(), n.clone()));
    Some(n)
}

/// The model `name` in `dir` is, when `dir` is a store's folder that knows it.
pub fn name_in(dir: &Path, name: &str) -> Option<String> {
    names(dir)?.get(name).cloned()
}

/// The Hugging Face cache: a folder named `hub` (its default, `~/.cache/huggingface/hub`), or
/// where `HF_HUB_CACHE` or `HF_HOME` puts it.
fn is_hub(dir: &Path) -> bool {
    if dir.file_name().is_some_and(|n| n == "hub") {
        return true;
    }
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    env("HF_HUB_CACHE").is_some_and(|p| p == dir) || env("HF_HOME").is_some_and(|p| p.join("hub") == dir)
}

/// At most this many entries are looked at in a store's folder.
const MAX_ENTRIES: usize = 100_000;

/// `models--org--name` as `org/name` (`datasets--`, `spaces--` too).
fn hf_hub(dir: &Path) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten().take(MAX_ENTRIES) {
        let Ok(name) = e.file_name().into_string() else { continue };
        if let Some(n) = hf_repo(&name) {
            out.insert(name, n);
        }
    }
    out
}

/// A cache folder's repository: `models--Qwen--Qwen3-0.6B` → `Qwen/Qwen3-0.6B`.
fn hf_repo(folder: &str) -> Option<String> {
    let rest = ["models--", "datasets--", "spaces--"].iter().find_map(|p| folder.strip_prefix(p))?;
    (!rest.is_empty()).then(|| rest.replacen("--", "/", 1))
}

/// A repository's blobs, by the files of its snapshots that link to them: `model.safetensors`,
/// with the repository's name.
fn hf_blobs(blobs: &Path) -> HashMap<String, String> {
    let mut out: HashMap<String, String> = HashMap::new();
    let Some(repo) = blobs.parent() else { return out };
    let named = repo.file_name().and_then(|n| n.to_str()).and_then(hf_repo);
    let mut todo = vec![(repo.join("snapshots"), 0)];
    let mut seen = 0;
    while let Some((d, depth)) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                return out;
            }
            let p = e.path();
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && depth < 6 {
                todo.push((p, depth + 1));
            } else if ft.is_symlink()
                && let Ok(to) = std::fs::read_link(&p)
                && let Some(blob) = to.file_name().and_then(|n| n.to_str())
                // Past the snapshot's own folder: `snapshots/<rev>/a/b.gguf` → `a/b.gguf`.
                && let Some(rel) = p.strip_prefix(repo.join("snapshots")).ok().map(|r| r.components().skip(1).collect::<PathBuf>())
            {
                let file = rel.to_string_lossy().into_owned();
                let label = match &named {
                    Some(r) => format!("{r} · {file}"),
                    None => file,
                };
                out.entry(blob.to_string()).or_insert(label);
            }
        }
    }
    out
}

/// A manifest is read only when it is at most this big.
const MAX_MANIFEST: u64 = 1 << 20;

/// Ollama's blobs by its manifests: `sha256-…` → `llama3.2:3b`, or `llama3.2:3b · template`
/// for a blob that is not the model's weights. A blob several models share names them all.
fn ollama(blobs: &Path) -> HashMap<String, String> {
    let Some(root) = blobs.parent() else { return HashMap::new() };
    // blob → part → models
    let mut by: HashMap<String, BTreeMap<String, BTreeSet<String>>> = HashMap::new();
    let manifests = root.join("manifests");
    let mut seen = 0;
    // manifests/<host>/<namespace>/<model>/<tag>: four levels, each read once.
    let mut todo: Vec<(PathBuf, Vec<String>)> = vec![(manifests, vec![])];
    while let Some((d, parts)) = todo.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > MAX_ENTRIES {
                break;
            }
            let Ok(n) = e.file_name().into_string() else { continue };
            let mut parts = parts.clone();
            parts.push(n);
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() && parts.len() < 4 {
                todo.push((e.path(), parts));
            } else if ft.is_file() && parts.len() == 4 {
                let model = ollama_name(&parts[0], &parts[1], &parts[2], &parts[3]);
                for (digest, part) in manifest_layers(&e.path()) {
                    by.entry(digest).or_default().entry(part).or_default().insert(model.clone());
                }
            }
        }
    }
    by.into_iter()
        .map(|(blob, parts)| {
            let label = parts
                .iter()
                .map(|(part, models)| {
                    let mut v: Vec<&str> = models.iter().map(String::as_str).take(3).collect();
                    let more = models.len().saturating_sub(3);
                    let more = if more > 0 { format!(" +{more}") } else { String::new() };
                    v.sort();
                    let names = format!("{}{more}", v.join(", "));
                    if part.is_empty() { names } else { format!("{names} · {part}") }
                })
                .collect::<Vec<_>>()
                .join("; ");
            (blob, label)
        })
        .collect()
}

/// The name Ollama shows: `llama3.2:3b` from its own library, `user/model:tag` from another
/// namespace, `hf.co/org/repo:tag` from another registry.
fn ollama_name(host: &str, ns: &str, model: &str, tag: &str) -> String {
    match (host, ns) {
        ("registry.ollama.ai", "library") => format!("{model}:{tag}"),
        ("registry.ollama.ai", _) => format!("{ns}/{model}:{tag}"),
        _ => format!("{host}/{ns}/{model}:{tag}"),
    }
}

/// A manifest's blobs, as file names (`sha256-…`), each with its part: empty for the
/// weights, else `template`, `licence` … in words.
fn manifest_layers(path: &Path) -> Vec<(String, String)> {
    let Ok(f) = File::open(path) else { return vec![] };
    let mut text = String::new();
    if f.take(MAX_MANIFEST).read_to_string(&mut text).is_err() {
        return vec![];
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else { return vec![] };
    let blob = |l: &serde_json::Value| l["digest"].as_str().map(|d| d.replacen(':', "-", 1)).filter(|d| d.starts_with("sha256-") && !d.contains(['/', '\\']));
    let mut out = vec![];
    if let Some(d) = blob(&v["config"]) {
        out.push((d, t!("local_models.part.config")));
    }
    for l in v["layers"].as_array().into_iter().flatten().take(1000) {
        let Some(d) = blob(l) else { continue };
        let kind = l["mediaType"].as_str().unwrap_or_default().rsplit('.').next().unwrap_or_default();
        let part = match kind {
            "model" => String::new(),
            "template" | "license" | "params" | "system" | "projector" | "adapter" | "messages" => t!(&format!("local_models.part.{kind}")),
            other => other.chars().take(40).collect(),
        };
        out.push((d, part));
    }
    out
}

// ---------------------------------------------------------------- facts

/// Whether `path` is a model file whose facts `facts` gives (or a pickle it names): by its
/// ending, or as a blob of a model store. For the hint; reads nothing but a folder's name.
pub fn is_model(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    matches!(ext.as_str(), "gguf" | "safetensors" | "pt" | "pth" | "ckpt")
        || path.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.ends_with(".safetensors.index.json") || pickle_bin(path, n))
        || path.parent().is_some_and(|d| d.file_name().is_some_and(|n| n == "blobs") && d.parent().is_some_and(|r| r.join("manifests").is_dir()))
}

/// A `.bin` that is PyTorch's: `pytorch_model*.bin`, or one in a Hugging Face snapshot.
fn pickle_bin(path: &Path, name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".bin")
        && (name.starts_with("pytorch_model") || path.components().any(|c| c.as_os_str() == "snapshots"))
}

/// The facts of a model file as (label, value) lines; `None` for a file that is none. A
/// continued list (the tensors) has an empty label on its later lines.
pub fn facts(path: &Path) -> Option<Vec<(String, String)>> {
    let name = path.file_name()?.to_str()?;
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if matches!(ext.as_str(), "pt" | "pth" | "ckpt") || pickle_bin(path, name) {
        return Some(vec![(t!("facts.format"), t!("local_models.pickle"))]);
    }
    let known = path.parent().and_then(|d| name_in(d, name));
    // A Hugging Face blob of a pickle: named by the file that links to it, never opened.
    if known.as_deref().is_some_and(|k| {
        let k = k.to_ascii_lowercase();
        [".pt", ".pth", ".ckpt", ".bin"].iter().any(|e| k.ends_with(e))
    }) {
        return Some(vec![(t!("facts.format"), t!("local_models.pickle"))]);
    }
    let m = if name.ends_with(".safetensors.index.json") {
        sharded(path)?
    } else if ext.is_empty() || ext == "gguf" || ext == "safetensors" {
        let mut f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        let mut head = [0u8; 9];
        f.read_exact(&mut head).ok()?;
        drop(f);
        if &head[..4] == b"GGUF" {
            gguf(path, len).ok()?
        } else if head[8] == b'{' || ext == "safetensors" {
            safetensors(path, len).ok()?
        } else {
            return None;
        }
    } else {
        return None;
    };
    let mut out = vec![];
    if let Some(k) = known {
        out.push((t!("local_models.model"), k));
    }
    out.extend(m.lines());
    Some(out)
}

/// What a header said, whichever format.
#[derive(Default, Debug)]
struct Model {
    format: String,
    name: Option<String>,
    architecture: Option<String>,
    /// `general.size_label`: "3B".
    size_label: Option<String>,
    quant: Option<String>,
    context: Option<u64>,
    template: Option<String>,
    licence: Option<String>,
    tensors: Vec<Tensor>,
    /// The weights' bytes when the tensors' do not add up (an unknown dtype): the file's.
    file_bytes: u64,
    /// Bytes of the cache of keys and values per token, for the memory to load.
    kv_per_token: Option<u64>,
    shards: Option<(usize, usize)>,
}

/// A tensor: name, parameters, dtype, bytes (`None` where the dtype's size is not known).
type Tensor = (String, u64, String, Option<u64>);

/// Tokens of context the memory to load counts: what Ollama and llama.cpp start with.
const CONTEXT: u64 = 4096;

impl Model {
    fn lines(&self) -> Vec<(String, String)> {
        let mut v = vec![(t!("facts.format"), self.format.clone())];
        let mut add = |k: &str, val: Option<String>| {
            if let Some(val) = val.filter(|s| !s.is_empty()) {
                v.push((t!(k), val));
            }
        };
        add("local_models.name", self.name.clone());
        add("local_models.architecture", self.architecture.clone());
        if let Some((have, n)) = self.shards {
            add("local_models.shards", Some(if have == n { n.to_string() } else { t!("local_models.shards_here", "have" => have, "n" => n) }));
        }
        let params = self.tensors.iter().try_fold(0u64, |a, t| a.checked_add(t.1));
        let params = params.filter(|&p| p > 0).map(count).or_else(|| self.size_label.clone());
        add("local_models.parameters", params);
        add("local_models.quantisation", self.quant.clone());
        add("local_models.context", self.context.map(|c| t!("local_models.tokens", "n" => c)));
        add("local_models.licence", self.licence.clone());
        add("local_models.chat_template", self.template.as_deref().map(fold));
        if !self.tensors.is_empty() {
            add("local_models.tensors", Some(self.tensors.len().to_string()));
            // Bytes by dtype, the largest first.
            let mut by: BTreeMap<&str, u64> = BTreeMap::new();
            for (_, _, ty, b) in &self.tensors {
                *by.entry(ty).or_default() = by.get(ty.as_str()).copied().unwrap_or(0).saturating_add(b.unwrap_or(0));
            }
            let mut by: Vec<_> = by.into_iter().collect();
            by.sort_by_key(|b| std::cmp::Reverse(b.1));
            add("local_models.types", Some(by.iter().take(6).map(|(t, b)| if *b > 0 { format!("{t} {}", crate::settings::human(*b)) } else { t.to_string() }).collect::<Vec<_>>().join(", ")));
            for (i, line) in groups(self.tensors.iter().map(|t| (t.0.as_str(), t.1))).into_iter().enumerate() {
                v.push((if i == 0 { t!("local_models.layers") } else { String::new() }, line));
            }
        }
        // The weights, and the cache of keys and values for the context.
        let tensor_bytes = self.tensors.iter().try_fold(0u64, |a, t| a.checked_add(t.3?));
        let weights = tensor_bytes.filter(|&b| b > 0).unwrap_or(self.file_bytes);
        if weights > 0 {
            let tokens = self.context.unwrap_or(CONTEXT).min(CONTEXT);
            let kv = self.kv_per_token.map(|k| k.saturating_mul(tokens));
            let need = weights.saturating_add(kv.unwrap_or(0));
            let val = match kv {
                Some(_) => t!("local_models.memory_context", "size" => crate::settings::human(need), "tokens" => tokens),
                None => t!("local_models.memory_value", "size" => crate::settings::human(need)),
            };
            v.push((t!("local_models.memory"), val));
            v.push((t!("local_models.here"), fits(need, machine())));
        }
        v
    }
}

/// A count of parameters in words: "3.2 billion", "494 million".
fn count(n: u64) -> String {
    crate::i18n::digits(count_latin(n))
}

fn count_latin(n: u64) -> String {
    if n >= 1_000_000_000 {
        t!("local_models.billion", "n" => format!("{:.1}", n as f64 / 1e9))
    } else if n >= 1_000_000 {
        t!("local_models.million", "n" => format!("{:.0}", n as f64 / 1e6))
    } else {
        n.to_string()
    }
}

/// A chat template folded into one line: how long it is and how it starts.
fn fold(s: &str) -> String {
    let lines = s.lines().count().max(1);
    let first: String = s.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(40).collect();
    t!("local_models.template_value", "lines" => lines, "start" => format!("{first}…"))
}

/// The tensors grouped: numbered ones by layer ("28 × blk: 3.0 billion"), the rest by name
/// ("token_embd: 394 million"), the largest first, at most eight lines.
fn groups<'a>(tensors: impl Iterator<Item = (&'a str, u64)>) -> Vec<String> {
    let mut layers: BTreeMap<String, (BTreeSet<u64>, u64)> = BTreeMap::new();
    let mut rest: BTreeMap<String, u64> = BTreeMap::new();
    for (name, n) in tensors {
        let seg: Vec<&str> = name.split('.').collect();
        match seg.iter().position(|s| !s.is_empty() && s.len() <= 6 && s.bytes().all(|b| b.is_ascii_digit())) {
            Some(i) if i > 0 => {
                let e = layers.entry(seg[..i].join(".")).or_default();
                if e.0.len() < 100_000 {
                    e.0.insert(seg[i].parse().unwrap_or(0));
                }
                e.1 = e.1.saturating_add(n);
            }
            _ => {
                let key = if seg.len() > 1 { seg[..seg.len() - 1].join(".") } else { name.to_string() };
                let e = rest.entry(key).or_default();
                *e = e.saturating_add(n);
            }
        }
    }
    let mut layers: Vec<_> = layers.into_iter().collect();
    layers.sort_by_key(|l| std::cmp::Reverse(l.1.1));
    let mut rest: Vec<_> = rest.into_iter().collect();
    rest.sort_by_key(|r| std::cmp::Reverse(r.1));
    let mut out: Vec<String> = layers.iter().map(|(k, (ids, n))| format!("{} × {k}: {}", ids.len(), count(*n))).collect();
    let room = 8usize.saturating_sub(out.len()).max(1);
    let shown = if rest.len() > room { room - 1 } else { rest.len() };
    out.extend(rest[..shown].iter().map(|(k, n)| format!("{k}: {}", count(*n))));
    if rest.len() > shown {
        let left: u64 = rest[shown..].iter().fold(0u64, |a, (_, n)| a.saturating_add(*n));
        out.push(t!("local_models.more_tensors", "n" => rest.len() - shown, "params" => count(left)));
    }
    out
}

/// This machine's graphics card and memory, found once (`setup::machine`).
fn machine() -> &'static crate::setup::Machine {
    static M: OnceLock<crate::setup::Machine> = OnceLock::new();
    M.get_or_init(|| crate::setup::machine(&[]))
}

/// Whether `need` bytes fit this machine: on its graphics card, in its memory, or neither.
fn fits(need: u64, m: &crate::setup::Machine) -> String {
    let gb = |n: u64| n << 30;
    match &m.gpu {
        Some((_, name, Some(vram))) if need <= gb(*vram) => t!("local_models.fits_gpu", "gpu" => name, "gb" => vram),
        Some((_, name, Some(vram))) if m.ram == 0 || need <= gb(m.ram) => t!("local_models.fits_ram_not_gpu", "ram" => m.ram, "gpu" => name, "gb" => vram),
        _ if m.ram == 0 => t!("local_models.fits_unknown"),
        _ if need <= gb(m.ram) => t!("local_models.fits_ram", "ram" => m.ram),
        _ => t!("local_models.too_big", "ram" => m.ram),
    }
}

// ---------------------------------------------------------------- GGUF

/// Caps on what a header may claim: past one, the file is not read further.
const MAX_HEADER: u64 = 256 << 20;
const MAX_STRING: u64 = 16 << 20;
const MAX_KEY: u64 = 64 << 10;
const MAX_KVS: u64 = 1 << 20;
const MAX_TENSORS: u64 = 1 << 20;
const MAX_ARRAY: u64 = 1 << 26;
const MAX_DIMS: u32 = 8;

type Res<T> = Result<T, &'static str>;

/// A file read from the start, no further than its length and `MAX_HEADER`.
struct Reader {
    r: BufReader<File>,
    at: u64,
    end: u64,
}

impl Reader {
    fn take(&mut self, n: u64) -> Res<()> {
        let to = self.at.checked_add(n).ok_or("overflow")?;
        if to > self.end {
            return Err("past the end");
        }
        self.at = to;
        Ok(())
    }
    fn bytes(&mut self, n: u64) -> Res<Vec<u8>> {
        self.take(n)?;
        let mut b = vec![0; n as usize];
        self.r.read_exact(&mut b).map_err(|_| "short")?;
        Ok(b)
    }
    fn skip(&mut self, n: u64) -> Res<()> {
        self.take(n)?;
        self.r.seek_relative(n as i64).map_err(|_| "short")
    }
    fn u32(&mut self) -> Res<u32> {
        Ok(u32::from_le_bytes(self.bytes(4)?.try_into().unwrap_or_default()))
    }
    fn u64(&mut self) -> Res<u64> {
        Ok(u64::from_le_bytes(self.bytes(8)?.try_into().unwrap_or_default()))
    }
    fn string(&mut self, max: u64) -> Res<String> {
        let n = self.u64()?;
        if n > max {
            return Err("string too long");
        }
        Ok(String::from_utf8_lossy(&self.bytes(n)?).into_owned())
    }
}

/// A value of the header kept: a number or a text; arrays are skipped.
enum Val {
    Int(u64),
    Text(String),
    Other,
}

/// Bytes of a GGUF value type of fixed size.
fn fixed(ty: u32) -> Option<u64> {
    Some(match ty {
        0 | 1 | 7 => 1,
        2 | 3 => 2,
        4..=6 => 4,
        10..=12 => 8,
        _ => return None,
    })
}

fn value(r: &mut Reader, ty: u32, keep: bool, depth: u32) -> Res<Val> {
    if let Some(n) = fixed(ty) {
        let b = r.bytes(n)?;
        let mut w = [0u8; 8];
        w[..b.len()].copy_from_slice(&b);
        return Ok(match ty {
            0 | 2 | 4 | 10 => Val::Int(u64::from_le_bytes(w)),
            // Signed: kept only when not negative.
            1 | 3 | 5 | 11 => {
                let bits = n * 8;
                let neg = bits < 64 && (u64::from_le_bytes(w) >> (bits - 1)) & 1 == 1 || bits == 64 && (w[7] & 0x80) != 0;
                if neg { Val::Other } else { Val::Int(u64::from_le_bytes(w)) }
            }
            _ => Val::Other,
        });
    }
    match ty {
        8 if keep => Ok(Val::Text(r.string(MAX_STRING)?)),
        8 => {
            let n = r.u64()?;
            if n > MAX_STRING {
                return Err("string too long");
            }
            r.skip(n).map(|_| Val::Other)
        }
        9 if depth < 2 => {
            let inner = r.u32()?;
            let n = r.u64()?;
            if n > MAX_ARRAY {
                return Err("array too long");
            }
            match fixed(inner) {
                Some(size) => r.skip(n.checked_mul(size).ok_or("overflow")?)?,
                None => {
                    for _ in 0..n {
                        value(r, inner, false, depth + 1)?;
                    }
                }
            }
            Ok(Val::Other)
        }
        _ => Err("unknown value type"),
    }
}

/// A ggml tensor type's name, and its block: elements and bytes.
fn ggml(ty: u32) -> (String, Option<(u64, u64)>) {
    let (n, b) = match ty {
        0 => ("F32", (1, 4)),
        1 => ("F16", (1, 2)),
        2 => ("Q4_0", (32, 18)),
        3 => ("Q4_1", (32, 20)),
        6 => ("Q5_0", (32, 22)),
        7 => ("Q5_1", (32, 24)),
        8 => ("Q8_0", (32, 34)),
        9 => ("Q8_1", (32, 36)),
        10 => ("Q2_K", (256, 84)),
        11 => ("Q3_K", (256, 110)),
        12 => ("Q4_K", (256, 144)),
        13 => ("Q5_K", (256, 176)),
        14 => ("Q6_K", (256, 210)),
        15 => ("Q8_K", (256, 292)),
        16 => ("IQ2_XXS", (256, 66)),
        17 => ("IQ2_XS", (256, 74)),
        18 => ("IQ3_XXS", (256, 98)),
        19 => ("IQ1_S", (256, 50)),
        20 => ("IQ4_NL", (32, 18)),
        21 => ("IQ3_S", (256, 110)),
        22 => ("IQ2_S", (256, 82)),
        23 => ("IQ4_XS", (256, 136)),
        24 => ("I8", (1, 1)),
        25 => ("I16", (1, 2)),
        26 => ("I32", (1, 4)),
        27 => ("I64", (1, 8)),
        28 => ("F64", (1, 8)),
        29 => ("IQ1_M", (256, 56)),
        30 => ("BF16", (1, 2)),
        34 => ("TQ1_0", (256, 54)),
        35 => ("TQ2_0", (256, 66)),
        39 => ("MXFP4", (32, 17)),
        _ => return (format!("type {ty}"), None),
    };
    (n.to_string(), Some(b))
}

/// `general.file_type`: how the file as a whole is quantised.
fn file_type(n: u64) -> Option<&'static str> {
    Some(match n {
        0 => "F32",
        1 => "F16",
        2 => "Q4_0",
        3 => "Q4_1",
        7 => "Q8_0",
        8 => "Q5_0",
        9 => "Q5_1",
        10 => "Q2_K",
        11 => "Q3_K_S",
        12 => "Q3_K_M",
        13 => "Q3_K_L",
        14 => "Q4_K_S",
        15 => "Q4_K_M",
        16 => "Q5_K_S",
        17 => "Q5_K_M",
        18 => "Q6_K",
        19 => "IQ2_XXS",
        20 => "IQ2_XS",
        21 => "Q2_K_S",
        22 => "IQ3_XS",
        23 => "IQ3_XXS",
        24 => "IQ1_S",
        25 => "IQ4_NL",
        26 => "IQ3_S",
        27 => "IQ3_M",
        28 => "IQ2_S",
        29 => "IQ2_M",
        30 => "IQ4_XS",
        31 => "IQ1_M",
        32 => "BF16",
        36 => "TQ1_0",
        37 => "TQ2_0",
        38 => "MXFP4_MOE",
        _ => return None,
    })
}

/// The keys of the header kept: the rest are skipped.
fn wanted(key: &str) -> bool {
    key.starts_with("general.") || key == "tokenizer.chat_template" || key.split_once('.').is_some_and(|(_, k)| matches!(k, "context_length" | "block_count" | "embedding_length" | "attention.head_count" | "attention.head_count_kv"))
}

fn gguf(path: &Path, len: u64) -> Res<Model> {
    let f = File::open(path).map_err(|_| "open")?;
    let mut r = Reader { r: BufReader::with_capacity(1 << 16, f), at: 0, end: len.min(MAX_HEADER) };
    if r.bytes(4)? != b"GGUF" {
        return Err("not GGUF");
    }
    let version = r.u32()?;
    if !(2..=3).contains(&version) {
        return Err("version");
    }
    let tensors = r.u64()?;
    let kvs = r.u64()?;
    if tensors > MAX_TENSORS || kvs > MAX_KVS {
        return Err("too many");
    }
    let mut kv: HashMap<String, Val> = HashMap::new();
    for _ in 0..kvs {
        let key = r.string(MAX_KEY)?;
        let ty = r.u32()?;
        let keep = wanted(&key);
        let v = value(&mut r, ty, keep, 0)?;
        if keep {
            kv.insert(key, v);
        }
    }
    let text = |k: &str| match kv.get(k) {
        Some(Val::Text(s)) => Some(s.chars().take(200).collect::<String>()),
        _ => None,
    };
    let int = |k: &str| match kv.get(k) {
        Some(Val::Int(n)) => Some(*n),
        _ => None,
    };
    let arch = text("general.architecture");
    let a = |k: &str| arch.as_ref().and_then(|a| int(&format!("{a}.{k}")));
    let mut m = Model {
        format: format!("GGUF v{version}"),
        name: text("general.name"),
        size_label: text("general.size_label"),
        quant: int("general.file_type").and_then(file_type).map(str::to_string),
        context: a("context_length"),
        template: match kv.get("tokenizer.chat_template") {
            Some(Val::Text(s)) => Some(s.clone()),
            _ => None,
        },
        licence: text("general.license"),
        file_bytes: len,
        ..Model::default()
    };
    // Keys and values, f16, per token: 2 × layers × the width of the keys' heads.
    if let (Some(l), Some(e), Some(h)) = (a("block_count"), a("embedding_length"), a("attention.head_count").filter(|&h| h > 0)) {
        let kvh = a("attention.head_count_kv").unwrap_or(h);
        m.kv_per_token = l.checked_mul(e).and_then(|x| x.checked_mul(kvh)).map(|x| x / h).and_then(|x| x.checked_mul(4));
    }
    m.architecture = arch;
    // The tensors: name, dimensions, type, offset. Cut short (a header past the cap) keeps
    // those read.
    for _ in 0..tensors {
        let Ok(t) = (|| -> Res<_> {
            let name = r.string(MAX_KEY)?;
            let dims = r.u32()?;
            if dims > MAX_DIMS {
                return Err("dimensions");
            }
            let mut n: u64 = 1;
            for _ in 0..dims {
                n = n.checked_mul(r.u64()?).ok_or("overflow")?;
            }
            let ty = r.u32()?;
            r.u64()?;
            Ok((name, n, ty))
        })() else {
            break;
        };
        let (tn, block) = ggml(t.2);
        let bytes = block.and_then(|(e, b)| t.1.div_ceil(e).checked_mul(b));
        m.tensors.push((t.0, t.1, tn, bytes));
    }
    Ok(m)
}

// ---------------------------------------------------------------- safetensors

/// The safetensors header's own cap.
const MAX_ST_HEADER: u64 = 100_000_000;

/// The header of a safetensors file, as JSON.
fn st_header(path: &Path, len: u64) -> Res<(serde_json::Map<String, serde_json::Value>, u64)> {
    let mut f = File::open(path).map_err(|_| "open")?;
    let mut b = [0u8; 8];
    f.read_exact(&mut b).map_err(|_| "short")?;
    let n = u64::from_le_bytes(b);
    let data = n.checked_add(8).ok_or("overflow")?;
    if n > MAX_ST_HEADER || data > len {
        return Err("header length");
    }
    let mut h = vec![0; n as usize];
    f.read_exact(&mut h).map_err(|_| "short")?;
    match serde_json::from_slice(&h) {
        Ok(serde_json::Value::Object(o)) => Ok((o, len - data)),
        _ => Err("not JSON"),
    }
}

/// Bytes of an element of a safetensors dtype.
fn st_size(dtype: &str) -> Option<u64> {
    Some(match dtype {
        "BOOL" | "U8" | "I8" | "F8_E4M3" | "F8_E5M2" | "F8_E8M0" => 1,
        "U16" | "I16" | "F16" | "BF16" => 2,
        "U32" | "I32" | "F32" => 4,
        "U64" | "I64" | "F64" => 8,
        _ => return None,
    })
}

/// The tensors of a header, each checked against the data it has.
fn st_tensors(h: &serde_json::Map<String, serde_json::Value>, data: u64) -> Res<Vec<Tensor>> {
    if h.len() as u64 > MAX_TENSORS {
        return Err("too many");
    }
    let mut out = vec![];
    for (name, v) in h {
        if name == "__metadata__" {
            continue;
        }
        let dtype = v["dtype"].as_str().ok_or("dtype")?;
        let shape = v["shape"].as_array().ok_or("shape")?;
        if shape.len() > MAX_DIMS as usize {
            return Err("dimensions");
        }
        let n = shape.iter().try_fold(1u64, |a, d| a.checked_mul(d.as_u64()?)).ok_or("shape")?;
        let off = v["data_offsets"].as_array().ok_or("offsets")?;
        let (a, b) = match off.as_slice() {
            [a, b] => (a.as_u64().ok_or("offsets")?, b.as_u64().ok_or("offsets")?),
            _ => return Err("offsets"),
        };
        if a > b || b > data {
            return Err("offsets");
        }
        if st_size(dtype).and_then(|s| s.checked_mul(n)).is_some_and(|want| want != b - a) {
            return Err("size");
        }
        out.push((name.chars().take(200).collect(), n, dtype.chars().take(16).collect(), Some(b - a)));
    }
    Ok(out)
}

fn safetensors(path: &Path, len: u64) -> Res<Model> {
    let (h, data) = st_header(path, len)?;
    let mut m = Model { format: "safetensors".into(), file_bytes: data, tensors: st_tensors(&h, data)?, ..Model::default() };
    if let Some(meta) = h.get("__metadata__") {
        m.name = meta["name"].as_str().map(|s| s.chars().take(200).collect());
    }
    Ok(m)
}

/// A sharded model's `model.safetensors.index.json`: its shards, and their headers when all
/// are here.
fn sharded(path: &Path) -> Option<Model> {
    let f = File::open(path).ok()?;
    let mut text = String::new();
    f.take(16 << 20).read_to_string(&mut text).ok()?;
    let v: serde_json::Value = serde_json::from_str(&text).ok()?;
    let map = v["weight_map"].as_object()?;
    let files: BTreeSet<&str> = map.values().filter_map(|f| f.as_str()).filter(|f| !f.contains(['/', '\\']) && *f != ".." && !f.is_empty()).take(10_000).collect();
    let dir = path.parent()?;
    let here: Vec<PathBuf> = files.iter().map(|f| dir.join(f)).filter(|p| p.is_file()).collect();
    let mut m = Model {
        format: t!("local_models.sharded"),
        file_bytes: v["metadata"]["total_size"].as_u64().unwrap_or(0),
        shards: Some((here.len(), files.len())),
        ..Model::default()
    };
    if here.len() == files.len() && here.len() <= 256 {
        for p in &here {
            let len = std::fs::metadata(p).ok()?.len();
            let (h, data) = st_header(p, len).ok()?;
            m.tensors.extend(st_tensors(&h, data).ok()?);
            if m.tensors.len() as u64 > MAX_TENSORS {
                return None;
            }
        }
    }
    Some(m)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-models-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn s(b: &mut Vec<u8>, t: &str) {
        b.extend((t.len() as u64).to_le_bytes());
        b.extend(t.as_bytes());
    }

    /// A small GGUF: a llama with `layers` blocks of two tensors, embeddings and output.
    pub fn gguf_bytes(layers: u64) -> Vec<u8> {
        let mut tensors: Vec<(String, Vec<u64>, u32)> = vec![("token_embd.weight".into(), vec![64, 100], 1), ("output.weight".into(), vec![64, 100], 1)];
        for i in 0..layers {
            tensors.push((format!("blk.{i}.attn_q.weight"), vec![64, 64], 8));
            tensors.push((format!("blk.{i}.ffn_up.weight"), vec![64, 128], 8));
        }
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend((tensors.len() as u64).to_le_bytes());
        b.extend(9u64.to_le_bytes());
        let kv_s = |b: &mut Vec<u8>, k: &str, v: &str| {
            s(b, k);
            b.extend(8u32.to_le_bytes());
            s(b, v);
        };
        let kv_u = |b: &mut Vec<u8>, k: &str, v: u32| {
            s(b, k);
            b.extend(4u32.to_le_bytes());
            b.extend(v.to_le_bytes());
        };
        kv_s(&mut b, "general.architecture", "llama");
        kv_s(&mut b, "general.name", "Demo 3B");
        kv_s(&mut b, "general.license", "llama3.2");
        kv_u(&mut b, "general.file_type", 15);
        kv_u(&mut b, "llama.context_length", 131072);
        kv_u(&mut b, "llama.block_count", layers as u32);
        kv_u(&mut b, "llama.embedding_length", 64);
        kv_s(&mut b, "tokenizer.chat_template", "{% for m in messages %}\n{{ m.content }}\n{% endfor %}");
        // An array of strings, skipped.
        s(&mut b, "tokenizer.ggml.tokens");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend(2u64.to_le_bytes());
        s(&mut b, "a");
        s(&mut b, "b");
        for (name, dims, ty) in &tensors {
            s(&mut b, name);
            b.extend((dims.len() as u32).to_le_bytes());
            for d in dims {
                b.extend(d.to_le_bytes());
            }
            b.extend(ty.to_le_bytes());
            b.extend(0u64.to_le_bytes());
        }
        b
    }

    /// A safetensors file of two F16 tensors.
    pub fn safetensors_bytes() -> Vec<u8> {
        let h = br#"{"__metadata__":{"format":"pt"},"model.layers.0.mlp.weight":{"dtype":"F16","shape":[2,3],"data_offsets":[0,12]},"model.embed_tokens.weight":{"dtype":"F16","shape":[4],"data_offsets":[12,20]}}"#;
        let mut b = (h.len() as u64).to_le_bytes().to_vec();
        b.extend(h);
        b.extend([0u8; 20]);
        b
    }

    fn val<'a>(f: &'a [(String, String)], k: &str) -> Option<&'a str> {
        f.iter().find(|(l, _)| l == &t!(k)).map(|(_, v)| v.as_str())
    }

    #[test]
    fn gguf_facts() {
        let d = folder("gguf");
        let p = d.join("demo.gguf");
        std::fs::write(&p, gguf_bytes(28)).unwrap();
        let f = facts(&p).unwrap();
        assert_eq!(val(&f, "facts.format"), Some("GGUF v3"));
        assert_eq!(val(&f, "local_models.architecture"), Some("llama"));
        assert_eq!(val(&f, "local_models.quantisation"), Some("Q4_K_M"));
        assert_eq!(val(&f, "local_models.licence"), Some("llama3.2"));
        assert_eq!(val(&f, "local_models.tensors"), Some("58"));
        assert!(val(&f, "local_models.chat_template").unwrap().contains('3'), "{f:?}");
        let layers = val(&f, "local_models.layers").unwrap();
        assert!(layers.starts_with("28 × blk:"), "{layers}");
        assert!(f.iter().any(|(l, v)| l.is_empty() && v.starts_with("token_embd")), "{f:?}");
        assert!(val(&f, "local_models.memory").is_some());
        // Cut anywhere, it is read no further: no panic, no facts or fewer.
        let all = gguf_bytes(4);
        for cut in 0..all.len() {
            std::fs::write(&p, &all[..cut]).unwrap();
            let _ = facts(&p);
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn hostile_headers_are_refused() {
        let d = folder("hostile");
        let p = d.join("x.gguf");
        // A string claiming 2^63 bytes, a count of 2^64-1 tensors, an array of 2^62 strings.
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend((1u64 << 63).to_le_bytes());
        std::fs::write(&p, &b).unwrap();
        assert!(gguf(&p, b.len() as u64).is_err());
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        std::fs::write(&p, &b).unwrap();
        assert!(gguf(&p, b.len() as u64).is_err());
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        s(&mut b, "tokenizer.ggml.tokens");
        b.extend(9u32.to_le_bytes());
        b.extend(8u32.to_le_bytes());
        b.extend((1u64 << 62).to_le_bytes());
        std::fs::write(&p, &b).unwrap();
        assert!(gguf(&p, b.len() as u64).is_err());
        // Dimensions whose product overflows: that tensor is not counted.
        let mut b = b"GGUF".to_vec();
        b.extend(3u32.to_le_bytes());
        b.extend(1u64.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        s(&mut b, "w");
        b.extend(2u32.to_le_bytes());
        b.extend(u64::MAX.to_le_bytes());
        b.extend(3u64.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(0u64.to_le_bytes());
        std::fs::write(&p, &b).unwrap();
        assert!(gguf(&p, b.len() as u64).unwrap().tensors.is_empty());
        // Safetensors: a header longer than the file, offsets past the data, a size that lies.
        let st = d.join("x.safetensors");
        std::fs::write(&st, [0xff; 16]).unwrap();
        assert!(facts(&st).is_none());
        for h in [
            r#"{"a":{"dtype":"F16","shape":[2],"data_offsets":[0,999]}}"#,
            r#"{"a":{"dtype":"F16","shape":[2],"data_offsets":[4,0]}}"#,
            r#"{"a":{"dtype":"F16","shape":[3],"data_offsets":[0,4]}}"#,
            r#"{"a":{"dtype":"F16","shape":[18446744073709551615,2],"data_offsets":[0,4]}}"#,
        ] {
            let mut b = (h.len() as u64).to_le_bytes().to_vec();
            b.extend(h.as_bytes());
            b.extend([0u8; 4]);
            std::fs::write(&st, &b).unwrap();
            assert!(facts(&st).is_none(), "{h}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn safetensors_and_shards() {
        let d = folder("st");
        let p = d.join("model-00001-of-00002.safetensors");
        std::fs::write(&p, safetensors_bytes()).unwrap();
        let f = facts(&p).unwrap();
        assert_eq!(val(&f, "local_models.tensors"), Some("2"));
        assert_eq!(val(&f, "local_models.parameters"), Some("10"));
        assert!(val(&f, "local_models.types").unwrap().starts_with("F16"));
        let idx = d.join("model.safetensors.index.json");
        std::fs::write(&idx, r#"{"metadata":{"total_size":40},"weight_map":{"a":"model-00001-of-00002.safetensors","b":"model-00002-of-00002.safetensors","c":"../../etc/passwd"}}"#).unwrap();
        let f = facts(&idx).unwrap();
        assert_eq!(val(&f, "local_models.shards"), Some(t!("local_models.shards_here", "have" => 1, "n" => 2).as_str()));
        std::fs::copy(&p, d.join("model-00002-of-00002.safetensors")).unwrap();
        let f = facts(&idx).unwrap();
        assert_eq!(val(&f, "local_models.tensors"), Some("4"));
        // A pickle is named, never opened.
        let pt = d.join("pytorch_model.bin");
        std::fs::write(&pt, b"\x80\x02c__builtin__\neval\n").unwrap();
        assert_eq!(facts(&pt).unwrap()[0].1, t!("local_models.pickle"));
        assert!(facts(&d.join("missing.bin")).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn ollama_and_hugging_face_names() {
        let d = folder("stores");
        let root = d.join("models");
        let blobs = root.join("blobs");
        let man = root.join("manifests/registry.ollama.ai/library/llama3.2");
        std::fs::create_dir_all(&blobs).unwrap();
        std::fs::create_dir_all(&man).unwrap();
        std::fs::write(blobs.join("sha256-aaa"), gguf_bytes(2)).unwrap();
        std::fs::write(man.join("3b"), r#"{"config":{"digest":"sha256:ccc"},"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:aaa"},{"mediaType":"application/vnd.ollama.image.template","digest":"sha256:bbb"},{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:../../x"}]}"#).unwrap();
        let other = root.join("manifests/hf.co/org/repo");
        std::fs::create_dir_all(&other).unwrap();
        std::fs::write(other.join("Q4_K_M"), r#"{"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:aaa"}]}"#).unwrap();
        let n = names(&blobs).unwrap();
        assert_eq!(n["sha256-aaa"], "hf.co/org/repo:Q4_K_M, llama3.2:3b");
        assert_eq!(n["sha256-bbb"], format!("llama3.2:3b · {}", t!("local_models.part.template")));
        assert_eq!(n.len(), 3);
        assert!(is_model(&blobs.join("sha256-aaa")));
        // Space names the blob by its model too.
        let tree = crate::space::Tree::scan(&d, &crate::space::Progress::default()).unwrap();
        assert_eq!(tree.label(tree.find(&blobs.join("sha256-aaa")).unwrap()), n["sha256-aaa"].as_str());
        assert_eq!(val(&facts(&blobs.join("sha256-aaa")).unwrap(), "local_models.model"), Some("hf.co/org/repo:Q4_K_M, llama3.2:3b"));
        // The Hugging Face cache.
        let hub = d.join("hub");
        let repo = hub.join("models--Qwen--Qwen3-0.6B");
        std::fs::create_dir_all(repo.join("blobs")).unwrap();
        std::fs::create_dir_all(repo.join("snapshots/abc123")).unwrap();
        std::fs::write(repo.join("blobs/f00d"), safetensors_bytes()).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("../../blobs/f00d", repo.join("snapshots/abc123/model.safetensors")).unwrap();
        std::fs::write(repo.join("blobs/beef"), b"\x80\x02c__builtin__\neval\n").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink("../../blobs/beef", repo.join("snapshots/abc123/pytorch_model.bin")).unwrap();
        assert_eq!(names(&hub).unwrap()["models--Qwen--Qwen3-0.6B"], "Qwen/Qwen3-0.6B");
        #[cfg(unix)]
        assert_eq!(names(&repo.join("blobs")).unwrap()["f00d"], "Qwen/Qwen3-0.6B · model.safetensors");
        // A pickle's blob, named by its link, is never opened.
        #[cfg(unix)]
        assert_eq!(facts(&repo.join("blobs/beef")).unwrap()[0].1, t!("local_models.pickle"));
        // A blob without an ending is still read by its header.
        assert_eq!(val(&facts(&repo.join("blobs/f00d")).unwrap(), "local_models.tensors"), Some("2"));
        assert!(names(&d).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn fits_this_machine() {
        use crate::setup::{Machine, Vendor};
        let m = Machine { gpu: Some((Vendor::Nvidia, "RTX".into(), Some(8))), npu: false, ram: 32 };
        assert_eq!(fits(4 << 30, &m), t!("local_models.fits_gpu", "gpu" => "RTX", "gb" => 8));
        assert_eq!(fits(16 << 30, &m), t!("local_models.fits_ram_not_gpu", "ram" => 32, "gpu" => "RTX", "gb" => 8));
        assert_eq!(fits(64 << 30, &m), t!("local_models.too_big", "ram" => 32));
        let cpu = Machine { ram: 16, ..Machine::default() };
        assert_eq!(fits(4 << 30, &cpu), t!("local_models.fits_ram", "ram" => 16));
    }
}
