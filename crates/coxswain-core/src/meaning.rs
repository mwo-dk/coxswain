//! Search by meaning: a small multilingual language model (multilingual-e5-small, 384
//! numbers per passage) turns passages of text into vectors, and a question into one too;
//! passages whose vectors point the same way are about the same thing, in any language and
//! whatever the words. It runs with candle, in pure Rust: on the CPU, or on a Mac's GPU through
//! Metal when it has one whose results match the CPU's.
//!
//! The model is not shipped: it is downloaded once the user turns the search on, from Hugging
//! Face at a pinned revision, and every file is checked against its SHA-256 before it is used.

use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::t;

pub const MODEL: &str = "multilingual-e5-small";
const REPO: &str = "intfloat/multilingual-e5-small";
const REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
/// The files of the model: name, SHA-256, bytes.
const FILES: &[(&str, &str, u64)] = &[
    ("config.json", "69137736cab8b8903a07fe8afaafdda25aac55415a12a55d1bffa9f581abf959", 655),
    ("tokenizer.json", "0b44a9d7b51c3c62626640cda0e2c2f70fdacdc25bbbd68038369d14ebdf4c39", 17_082_730),
    ("model.safetensors", "1a55775f53449dac10a2bcbc312469fac40b96d53198c407081a831f81c98477", 470_641_600),
];
/// Numbers per vector of the built-in model. A server's model has its own count.
pub const DIMS: usize = 384;
/// Words per passage, the words two passages cut inside a paragraph share, and passages per
/// file at most: about 25,000 words; of a longer file, the start, the end, the headings and
/// passages evenly between.
const WORDS: usize = 120;
const OVERLAP: usize = 20;
pub const PASSAGES: usize = 256;
/// How a file is cut into passages and what the model is shown of each: vectors made another
/// way are made again (the store keeps it in its meta as `passages`).
pub const SCHEME: &str = "2";

/// Where the model is kept: the cache folder, next to the search store.
pub fn folder() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("models").join(format!("{MODEL}-{}", &REVISION[..8])))
}

/// Bytes the download takes.
pub fn size() -> u64 {
    FILES.iter().map(|f| f.2).sum()
}

/// Whether the model is downloaded (checked when it was).
pub fn installed() -> bool {
    folder().is_some_and(|d| FILES.iter().all(|(name, _, len)| std::fs::metadata(d.join(name)).is_ok_and(|m| m.len() == *len)))
}

/// A download under way: bytes done of all, and a way to stop it.
#[derive(Default)]
pub struct Progress {
    pub done: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
}

/// Download what is missing of the model, each file checked against its SHA-256.
pub fn download(p: &Progress) -> io::Result<()> {
    let files: Vec<Pinned> = FILES.iter().map(|&(name, sha, len)| Pinned { repo: REPO, revision: REVISION, name, sha, len }).collect();
    fetch(&folder().ok_or_else(|| io::Error::other("no cache folder"))?, &files, p)
}

/// A file of a model on Hugging Face, at a pinned revision, with its SHA-256 and bytes.
pub(crate) struct Pinned {
    pub repo: &'static str,
    pub revision: &'static str,
    pub name: &'static str,
    pub sha: &'static str,
    pub len: u64,
}

/// Download what is missing of `files` into `dir`, each checked against its SHA-256.
pub(crate) fn fetch(dir: &std::path::Path, files: &[Pinned], p: &Progress) -> io::Result<()> {
    std::fs::create_dir_all(dir)?;
    p.total.store(files.iter().map(|f| f.len).sum(), Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_connect(Some(Duration::from_secs(20))).build().into();
    for &Pinned { repo, revision, name, sha, len } in files {
        let file = dir.join(name);
        if std::fs::metadata(&file).is_ok_and(|m| m.len() == len) {
            p.done.fetch_add(len, Ordering::Relaxed);
            continue;
        }
        let url = format!("https://huggingface.co/{repo}/resolve/{revision}/{name}");
        let mut body = agent.get(&url).header("User-Agent", concat!("coxswain/", env!("CARGO_PKG_VERSION"))).call().map_err(|e| io::Error::other(format!("huggingface.co: {e}")))?.into_body();
        let mut reader = body.as_reader();
        let part = file.with_extension("part");
        let mut out = std::fs::File::create(&part)?;
        let (mut hash, mut buf, mut got) = (Sha256::new(), vec![0u8; 1 << 20], 0u64);
        loop {
            if p.cancel.load(Ordering::Relaxed) {
                drop(out);
                let _ = std::fs::remove_file(&part);
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hash.update(&buf[..n]);
            out.write_all(&buf[..n])?;
            got += n as u64;
            p.done.fetch_add(n as u64, Ordering::Relaxed);
        }
        drop(out);
        let hex: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
        if got != len || hex != sha {
            let _ = std::fs::remove_file(&part);
            return Err(io::Error::other(format!("{name}: not the file expected (SHA-256 {hex})")));
        }
        std::fs::rename(part, file)?;
    }
    Ok(())
}

/// Delete the model.
pub fn remove() -> io::Result<()> {
    match folder().map(std::fs::remove_dir_all) {
        Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

/// Passages per pass on the GPU: one pass over many is what makes it quick there. On the CPU
/// fewer: a long file's passages still go faster than one by one, with little padding.
const BATCH: usize = 32;
const CPU_BATCH: usize = 8;
/// A sentence both devices turn into a vector when the model loads: the GPU's must be the CPU's.
const PROBE: &str = "passage: The fuel budget for flight seven is the largest cost of the launch, \
    more than the rocket itself, the crew, the launch pad and the weather station together. The team \
    checks every tank twice before the countdown, and the numbers go into the report for the board.";
/// How close the GPU's vector of the probe must come to the CPU's (their cosine).
const AGREE: f32 = 0.999;

/// Why the built-in model runs on the CPU on a Mac, where it could have used the GPU.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "why", content = "detail", rename_all = "snake_case")]
pub enum Fallback {
    /// `meaning_device = "cpu"`.
    Chosen,
    /// No Metal device: an Intel Mac without one, a virtual machine.
    NoMetal(String),
    /// The model did not load on the GPU, or gave no vector there.
    Load(String),
    /// The GPU's vector of the probe was not the CPU's: their cosine.
    Probe(f32),
    /// The GPU was slower than the CPU: how many times as fast it was.
    Slower(f32),
    /// The GPU failed while it worked; the rest of the run is on the CPU.
    Failed(String),
}

/// Where the built-in model runs: on the GPU (and how many times as fast as the CPU it was on
/// the probe), or on the CPU, and why when a Mac could have used its GPU.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Runs {
    pub metal: bool,
    pub faster: f32,
    pub cpu_why: Option<Fallback>,
}

impl Runs {
    /// "on the GPU (Metal)", "on the CPU (why)".
    pub fn text(&self) -> String {
        match &self.cpu_why {
            _ if self.metal => t!("meaning.on_metal"),
            Some(why) => t!("meaning.on_cpu_why", "why" => why.text()),
            None => t!("meaning.on_cpu"),
        }
    }
}

impl Fallback {
    pub fn text(&self) -> String {
        match self {
            Fallback::Chosen => t!("meaning.cpu_chosen"),
            Fallback::NoMetal(_) => t!("meaning.cpu_no_metal"),
            Fallback::Load(why) => t!("meaning.cpu_load", "why" => why),
            Fallback::Probe(c) => t!("meaning.cpu_probe", "score" => format!("{c:.4}")),
            Fallback::Slower(_) => t!("meaning.cpu_slower"),
            Fallback::Failed(why) => t!("meaning.cpu_failed", "why" => why),
        }
    }
}

/// The cosine of two vectors.
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
    dot(a, b) / (dot(a, a).sqrt() * dot(b, b).sqrt()).max(f32::MIN_POSITIVE)
}

/// Where the model runs: on the GPU when it is wanted, `gpu` loads it there, its vector of the
/// probe is the CPU's and it is quicker; on the CPU otherwise, with why. `vectors` gives a
/// model's vectors of some texts.
fn choose<M>(cpu_only: bool, cpu: M, gpu: impl FnOnce() -> Result<M, Fallback>, vectors: impl Fn(&M, &[&str]) -> Result<Vec<Vec<f32>>, String>) -> (M, Runs) {
    let on_cpu = |cpu, why| (cpu, Runs { metal: false, faster: 0.0, cpu_why: Some(why) });
    if cpu_only {
        return on_cpu(cpu, Fallback::Chosen);
    }
    let gpu = match gpu() {
        Ok(g) => g,
        Err(why) => return on_cpu(cpu, why),
    };
    let first = |m: &M, n: usize| vectors(m, &vec![PROBE; n]).and_then(|v| v.into_iter().next().ok_or_else(|| "no vector".to_string()));
    let t = std::time::Instant::now();
    let here = first(&cpu, 1);
    let cpu_time = t.elapsed().as_secs_f32();
    // The first pass on the GPU makes its kernels: the second one is timed, eight at once.
    let there = first(&gpu, 1).and_then(|v| {
        let t = std::time::Instant::now();
        first(&gpu, 8)?;
        Ok((v, t.elapsed().as_secs_f32() / 8.0))
    });
    match (here, there) {
        (Ok(a), Ok((b, gpu_time))) => {
            let (agree, faster) = (cosine(&a, &b), cpu_time / gpu_time.max(1e-6));
            if agree < AGREE {
                on_cpu(cpu, Fallback::Probe(agree))
            } else if faster < 1.0 {
                on_cpu(cpu, Fallback::Slower(faster))
            } else {
                (gpu, Runs { metal: true, faster, cpu_why: None })
            }
        }
        (Err(e), _) | (_, Err(e)) => on_cpu(cpu, Fallback::Load(e)),
    }
}

/// The model on one device.
struct Bert {
    model: BertModel,
    device: Device,
}

impl Bert {
    fn load(dir: &std::path::Path, config: &Config, device: Device) -> Result<Bert, String> {
        // Memory-mapped: the pages are the file's, shared, and dropped by the system when short.
        let weights = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DType::F32, &device) }.map_err(|e| e.to_string())?;
        Ok(Bert { model: BertModel::load(weights, config).map_err(|e| e.to_string())?, device })
    }

    /// The vectors of `texts` in one pass: each the mean of the model's last layer over its
    /// words, of length one; empty for a text the model gives nothing for. The shorter texts
    /// are padded and their padding masked, so each vector is what it would be alone.
    fn vectors(&self, tokenizer: &tokenizers::Tokenizer, pad: u32, texts: &[&str]) -> candle_core::Result<Vec<Vec<f32>>> {
        let enc = tokenizer.encode_batch(texts.to_vec(), true).map_err(|e| candle_core::Error::Msg(e.to_string()))?;
        let len = enc.iter().map(|e| e.len()).max().unwrap_or(0);
        let (mut ids, mut mask) = (Vec::with_capacity(len * enc.len()), Vec::with_capacity(len * enc.len()));
        for e in &enc {
            ids.extend(e.get_ids().iter().copied().chain(std::iter::repeat(pad)).take(len));
            mask.extend(e.get_attention_mask().iter().copied().chain(std::iter::repeat(0)).take(len));
        }
        let ids = Tensor::from_vec(ids, (enc.len(), len), &self.device)?;
        let mask = Tensor::from_vec(mask, (enc.len(), len), &self.device)?;
        let out = self.model.forward(&ids, &ids.zeros_like()?, Some(&mask))?;
        let m = mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let rows: Vec<Vec<f32>> = out.broadcast_mul(&m)?.sum(1)?.broadcast_div(&m.sum(1)?)?.to_vec2()?;
        Ok(rows
            .into_iter()
            .map(|v| {
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if n > 0.0 { v.iter().map(|x| x / n).collect() } else { vec![] }
            })
            .collect())
    }
}

/// The model, loaded, with its tokenizer. On the CPU it works on two threads, so the machine
/// stays the user's; on a Mac it runs on the GPU through Metal when it can.
pub struct Embedder {
    bert: std::sync::RwLock<std::sync::Arc<Bert>>,
    runs: std::sync::Mutex<Runs>,
    dir: PathBuf,
    config: Config,
    pad: u32,
    tokenizer: tokenizers::Tokenizer,
    pool: rayon::ThreadPool,
}

impl Embedder {
    /// The model, on the GPU when it can be and `cpu_only` is not asked.
    pub fn load(cpu_only: bool) -> Option<Embedder> {
        let dir = folder().filter(|_| installed())?;
        let config: Config = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).ok()?).ok()?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).ok()?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: 512, ..Default::default() })).ok()?;
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).thread_name(|i| format!("coxswain-meaning-{i}")).build().ok()?;
        let pad = config.pad_token_id as u32;
        let cpu = Bert::load(&dir, &config, Device::Cpu).ok()?;
        let (bert, runs) = if cfg!(target_os = "macos") {
            let gpu = || {
                let device = Device::new_metal(0).map_err(|e| Fallback::NoMetal(e.to_string()))?;
                Bert::load(&dir, &config, device).map_err(Fallback::Load)
            };
            pool.install(|| choose(cpu_only, cpu, gpu, |b: &Bert, t| b.vectors(&tokenizer, pad, t).map_err(|e| e.to_string())))
        } else {
            (cpu, Runs::default())
        };
        Some(Embedder { bert: std::sync::RwLock::new(std::sync::Arc::new(bert)), runs: std::sync::Mutex::new(runs), dir, config, pad, tokenizer, pool })
    }

    /// Where the model runs now.
    pub fn runs(&self) -> Runs {
        self.runs.lock().unwrap().clone()
    }

    /// The vector of a question.
    pub fn query(&self, text: &str) -> Option<Vec<f32>> {
        self.embed(&[&format!("query: {text}")]).pop().filter(|v| !v.is_empty())
    }

    /// The vectors of passages of a document: zeros for one the model gives none for, which
    /// scores nothing, so the ones after it keep their place.
    pub fn passages(&self, texts: &[String]) -> Vec<Vec<f32>> {
        let texts: Vec<String> = texts.iter().map(|t| format!("passage: {t}")).collect();
        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        self.embed(&texts).into_iter().map(|v| if v.is_empty() { vec![0.0; DIMS] } else { v }).collect()
    }

    /// The vectors of `texts`, empty where there is none, in batches. A GPU that fails moves
    /// the model to the CPU for the rest of the run.
    fn embed(&self, texts: &[&str]) -> Vec<Vec<f32>> {
        self.pool.install(|| {
            let bert = self.bert.read().unwrap().clone();
            let size = if bert.device.is_metal() { BATCH } else { CPU_BATCH };
            let mut out = Vec::with_capacity(texts.len());
            for batch in texts.chunks(size) {
                match bert.vectors(&self.tokenizer, self.pad, batch) {
                    Ok(v) => out.extend(v),
                    Err(e) if self.to_cpu(e.to_string()) => {
                        out.extend(self.embed(&texts[out.len()..]));
                        break;
                    }
                    // On the CPU a batch that fails goes one by one: a passage the model cannot
                    // take costs only itself.
                    Err(_) if !bert.device.is_metal() => out.extend(batch.iter().map(|t| bert.vectors(&self.tokenizer, self.pad, &[t]).ok().and_then(|mut v| v.pop()).unwrap_or_default())),
                    Err(_) => {
                        out.resize(texts.len(), vec![]);
                        break;
                    }
                }
            }
            out
        })
    }

    /// The GPU failed with `why`: the CPU takes over, unless it cannot load the model either.
    fn to_cpu(&self, why: String) -> bool {
        let mut bert = self.bert.write().unwrap();
        if bert.device.is_metal() {
            let Ok(cpu) = Bert::load(&self.dir, &self.config, Device::Cpu) else { return false };
            *bert = std::sync::Arc::new(cpu);
            *self.runs.lock().unwrap() = Runs { metal: false, faster: 0.0, cpu_why: Some(Fallback::Failed(why)) };
        }
        true
    }
}

/// Where the vectors come from: the built-in model, or a server the user runs.
pub enum Engine {
    Builtin(Embedder),
    Server(Server),
}

/// An embedding server: Ollama's own API, or the OpenAI one that most others speak.
pub struct Server {
    openai: bool,
    url: String,
    model: String,
    key: Option<String>,
    agent: ureq::Agent,
}

/// Ollama on this machine, where it listens unless told otherwise.
pub const OLLAMA: &str = "http://localhost:11434";

/// The API key the settings name a variable for, when that variable is set.
pub fn key_of(cfg: &crate::config::SearchConfig) -> Option<String> {
    (!cfg.meaning_key_env.is_empty()).then(|| std::env::var(&cfg.meaning_key_env).ok()).flatten()
}

/// Why a server gave no vectors: it did not answer, and the file waits for the next round; or
/// it answered that it cannot (a model that is not there, an input it does not take), and the
/// file is left until the next scan, so the rest go on.
#[derive(Debug)]
pub enum NoVectors {
    Down(String),
    Refused(String),
}

impl NoVectors {
    pub fn why(&self) -> &str {
        match self {
            NoVectors::Down(s) | NoVectors::Refused(s) => s,
        }
    }
}

impl Engine {
    /// The engine the settings ask for; `None` for the built-in one before it is downloaded.
    pub fn from_config(cfg: &crate::config::SearchConfig) -> Option<Engine> {
        match cfg.meaning_engine.as_str() {
            "ollama" | "openai" => Some(Engine::Server(Server::new(cfg))),
            _ => Embedder::load(cfg.meaning_device == "cpu").map(Engine::Builtin),
        }
    }

    /// Where the built-in model runs; nothing for a server.
    pub fn runs(&self) -> Option<Runs> {
        match self {
            Engine::Builtin(e) => Some(e.runs()),
            Engine::Server(_) => None,
        }
    }

    /// Which model made a vector: vectors of two models cannot be compared.
    pub fn id(&self) -> String {
        match self {
            Engine::Builtin(_) => builtin_id(),
            Engine::Server(s) => server_id(s.openai, &s.model),
        }
    }

    /// The digest of the model's weights, when the server says (Ollama's `/api/tags`): the same
    /// weights under another name keep their vectors.
    pub fn digest(&self) -> Option<String> {
        match self {
            Engine::Server(s) if !s.openai => s.digest(),
            _ => None,
        }
    }

    /// The vector of a question.
    pub fn query(&self, text: &str) -> Result<Vec<f32>, String> {
        match self {
            Engine::Builtin(e) => e.query(text).ok_or_else(|| "the model gave no vector".into()),
            Engine::Server(s) => s.embed(&[format!("{}{text}", s.prefix().0)]).map(|mut v| v.remove(0)).map_err(|e| e.why().to_string()),
        }
    }

    /// The vectors of a file's passages, all at once, one per passage: a passage the built-in
    /// model gives none for gets a vector of zeros, which scores nothing, so the ones after it
    /// keep their place.
    pub fn passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, NoVectors> {
        match self {
            Engine::Builtin(e) => Ok(e.passages(texts)),
            // A long file's passages go 32 at a time: a server on a CPU answers each in time.
            Engine::Server(s) => {
                let mut out = Vec::with_capacity(texts.len());
                for part in texts.chunks(32) {
                    out.extend(s.embed(&part.iter().map(|t| format!("{}{t}", s.prefix().1)).collect::<Vec<_>>())?);
                }
                Ok(out)
            }
        }
    }

    /// The least score of a passage worth showing, by what this model's scores look like:
    /// e5 puts unrelated text at 0.76–0.82 and an answer at 0.81–0.91; bge-m3 unrelated text
    /// at 0.30–0.55 and an answer at 0.48–0.72 (`tests/search_quality.rs`).
    // ponytail: measured for e5 and bge-m3; a model with odd scores wants its own.
    pub fn floor(&self) -> f32 {
        if self.is_e5() { 0.77 } else { 0.45 }
    }

    /// How far below the best passage another is still shown: e5's scores sit close together.
    pub fn window(&self) -> f32 {
        if self.is_e5() { 0.10 } else { 0.15 }
    }

    fn is_e5(&self) -> bool {
        match self {
            Engine::Builtin(_) => true,
            Engine::Server(s) => s.model.contains("e5"),
        }
    }
}

/// A time to wait, in words: "a minute", "40 minutes", "3 hours".
pub fn about(secs: f64) -> String {
    if secs < 60.0 {
        crate::t!("search.meaning_change_moment")
    } else if secs < 5400.0 {
        crate::t!("search.meaning_change_minutes", "n" => (secs / 60.0).ceil() as u64)
    } else {
        crate::t!("search.meaning_change_hours", "n" => (secs / 3600.0).ceil() as u64)
    }
}

fn builtin_id() -> String {
    format!("builtin:{MODEL}@{}", &REVISION[..8])
}

/// A server model's id: Ollama's `bge-m3` is `bge-m3:latest`, as its list names it, so picking it
/// from the list does not count as another model.
fn server_id(openai: bool, model: &str) -> String {
    if openai {
        format!("openai:{model}")
    } else if model.rsplit('/').next().is_some_and(|m| m.contains(':')) {
        format!("ollama:{model}")
    } else {
        format!("ollama:{model}:latest")
    }
}

/// Whether two engine ids name the same model, ids saved before tags were added included.
pub fn same_model(a: &str, b: &str) -> bool {
    let norm = |id: &str| match id.strip_prefix("ollama:") {
        Some(m) => server_id(false, m),
        None => id.to_string(),
    };
    norm(a) == norm(b)
}

/// The id of the model `cfg` makes vectors with, without loading it.
pub fn config_id(cfg: &crate::config::SearchConfig) -> String {
    match cfg.meaning_engine.as_str() {
        "ollama" | "openai" => {
            let s = Server::new(cfg);
            server_id(s.openai, &s.model)
        }
        _ => builtin_id(),
    }
}

/// What a change of the vectors' model costs, for the user to confirm before it is saved:
/// None when the vectors stay (the same model, or the same weights under another name) or
/// there are none; else how many files are read again, and about how long that takes here:
/// the time the new model takes for one passage, times the `passages` the store has.
pub fn change_notice(old: &crate::config::SearchConfig, new: &crate::config::SearchConfig, done: usize, passages: usize) -> Option<String> {
    if done == 0 || same_model(&config_id(old), &config_id(new)) {
        return None;
    }
    let digest = |cfg| Engine::from_config(cfg).and_then(|e| e.digest());
    if matches!((digest(old), digest(new)), (Some(a), Some(b)) if a == b) {
        return None;
    }
    const SAMPLE: usize = 8;
    let sample: Vec<String> = (0..SAMPLE).map(|i| format!("Passage {i} of a long file: the budget, the launch plan and the notes from the meeting, written out in plain words. ").repeat(WORDS / 20)).collect();
    let start = std::time::Instant::now();
    match Engine::from_config(new).map(|e| e.passages(&sample)) {
        Some(Ok(_)) => {
            let secs = start.elapsed().as_secs_f64() / SAMPLE as f64 * passages.max(done) as f64;
            Some(crate::t!("search.meaning_change", "n" => done, "time" => about(secs)))
        }
        _ => Some(crate::t!("search.meaning_change_untimed", "n" => done)),
    }
}

impl Server {
    /// The digest Ollama's list gives the model.
    fn digest(&self) -> Option<String> {
        let mut res = self.agent.get(&format!("{}/api/tags", self.url)).call().ok()?;
        let v: serde_json::Value = serde_json::from_str(&res.body_mut().read_to_string().ok()?).ok()?;
        let id = server_id(false, &self.model);
        v["models"].as_array()?.iter().find(|m| m["name"].as_str().is_some_and(|n| server_id(false, n) == id))?["digest"].as_str().map(String::from)
    }

    fn new(cfg: &crate::config::SearchConfig) -> Server {
        let openai = cfg.meaning_engine == "openai";
        let url = if cfg.meaning_url.is_empty() && !openai { OLLAMA.to_string() } else { cfg.meaning_url.trim_end_matches('/').to_string() };
        let model = if cfg.meaning_model.is_empty() && !openai { "bge-m3".to_string() } else { cfg.meaning_model.clone() };
        let key = key_of(cfg);
        let agent = ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(Duration::from_secs(120))).http_status_as_error(false).build().into();
        Server { openai, url, model, key, agent }
    }

    /// What some models want in front of a question and of a passage.
    fn prefix(&self) -> (&'static str, &'static str) {
        let m = self.model.to_lowercase();
        if m.contains("e5") {
            ("query: ", "passage: ")
        } else if m.contains("nomic-embed") {
            ("search_query: ", "search_document: ")
        } else {
            ("", "")
        }
    }

    /// Vectors of `texts`, of length one, in their order.
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, NoVectors> {
        let (path, body) = if self.openai { ("/embeddings", serde_json::json!({ "model": self.model, "input": texts })) } else { ("/api/embed", serde_json::json!({ "model": self.model, "input": texts })) };
        let mut req = self.agent.post(&format!("{}{path}", self.url)).header("Content-Type", "application/json");
        if let Some(key) = &self.key {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        let mut res = req.send(body.to_string()).map_err(|e| NoVectors::Down(format!("{}: {e}", shown(&self.url))))?;
        let status = res.status();
        let text = res.body_mut().read_to_string().map_err(|e| NoVectors::Down(e.to_string()))?;
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let said = || v["error"].as_str().or_else(|| v["error"]["message"].as_str()).map(String::from);
        if status.as_u16() >= 400 {
            return Err(NoVectors::Refused(format!("{} {}: {}", shown(&self.url), status, said().unwrap_or(text))));
        }
        let rows: Vec<&serde_json::Value> = if self.openai { v["data"].as_array().map(|d| d.iter().map(|x| &x["embedding"]).collect()).unwrap_or_default() } else { v["embeddings"].as_array().map(|d| d.iter().collect()).unwrap_or_default() };
        if rows.len() != texts.len() {
            return Err(NoVectors::Refused(said().unwrap_or_else(|| "no vectors in the answer".into())));
        }
        rows.into_iter()
            .map(|r| {
                let v: Vec<f32> = r.as_array().ok_or(NoVectors::Refused("not a vector".into()))?.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if n > 0.0 { Ok(v.iter().map(|x| x / n).collect()) } else { Err(NoVectors::Refused("an empty vector".into())) }
            })
            .collect()
    }
}

/// A question asked before in the same Find file, and its answer.
pub type Turn = (String, String);

/// What the chat model is told to do with the sources.
pub(crate) const RULES: &str = "You answer questions about the user's own files. Use only the numbered sources below. \
After each statement, cite the sources it comes from as [1] or [2][3]. If the sources do not hold the answer, \
say so plainly and do not guess. Answer in the language of the question, briefly.";

/// Ask: `question` answered by the chat model on the user's server or the built-in one, from `sources` (numbered
/// in their order) and the turns before. Each piece of the answer goes to `piece` as it
/// comes, with empty ones between; `piece` returns false to stop. Nothing is kept.
pub fn ask(cfg: &crate::config::SearchConfig, earlier: &[Turn], question: &str, sources: &[(std::path::PathBuf, String)], mut piece: impl FnMut(&str) -> bool) -> Result<(), String> {
    if let Some(m) = crate::chat::of(&cfg.ask_model) {
        return crate::chat::ask(m, cfg.meaning_device == "cpu", cfg.ask_think, earlier, question, sources, piece);
    }
    if cfg.ask_model.is_empty() {
        return Err("no chat model is set for Ask".into());
    }
    let s = Server::new(cfg);
    // The oldest turns go first when the context cannot hold them all with the sources.
    let room = crate::ask::context(cfg).saturating_sub(crate::chat::answer_room(crate::chat::thinks(cfg)));
    let size = |e: &[Turn]| crate::ask::tokens(RULES) + crate::ask::tokens(question) + sources.iter().map(|(p, t)| crate::ask::tokens(t) + p.as_os_str().len() / 3 + 4).sum::<usize>() + e.iter().map(|(q, a)| crate::ask::tokens(q) + crate::ask::tokens(a) + 8).sum::<usize>();
    let mut earlier = earlier;
    while !earlier.is_empty() && size(earlier) > room {
        earlier = &earlier[1..];
    }
    let context: String = sources.iter().enumerate().map(|(i, (path, text))| format!("[{}] {}\n{text}\n\n", i + 1, path.display())).collect();
    let mut messages = vec![serde_json::json!({ "role": "system", "content": format!("{RULES}\n\nSources:\n\n{context}") })];
    for (q, a) in earlier {
        messages.push(serde_json::json!({ "role": "user", "content": q }));
        messages.push(serde_json::json!({ "role": "assistant", "content": a }));
    }
    // A model that thinks before it answers keeps the user waiting for words never shown:
    // asked not to, unless `ask_think`. Ollama has a switch for it; Qwen3 elsewhere reads a
    // `/no_think` at the end of the question.
    let quiet = !cfg.ask_think && s.openai && no_think_by_word(&cfg.ask_model);
    messages.push(serde_json::json!({ "role": "user", "content": if quiet { format!("{question} /no_think") } else { question.to_string() } }));
    let path = if s.openai { "/chat/completions" } else { "/api/chat" };
    let mut body = serde_json::json!({ "model": cfg.ask_model, "messages": messages, "stream": true });
    if !s.openai {
        // Ollama refuses only `think: true` to a model that cannot think; one before 0.9 ignores it.
        if !cfg.ask_think {
            body["think"] = false.into();
        }
        body["options"] = serde_json::json!({ "num_ctx": crate::ask::context(cfg) });
    }
    // A model that is not loaded yet takes a while to answer at all; after that, pieces come.
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls())
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_recv_response(Some(Duration::from_secs(300)))
        // A server that stalls mid-answer does not hold the thread for ever.
        .timeout_recv_body(Some(Duration::from_secs(900)))
        .http_status_as_error(false)
        .build()
        .into();
    let mut req = agent.post(&format!("{}{path}", s.url)).header("Content-Type", "application/json");
    if let Some(key) = &s.key {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    // The wait for the first byte (a model loading) is on a thread of its own, so Stop is
    // heard meanwhile: `piece` is asked every 100 ms whether to go on.
    let (sent, answer) = std::sync::mpsc::channel();
    let (url, body) = (s.url.clone(), body.to_string());
    std::thread::spawn(move || sent.send(req.send(body).map_err(|e| format!("{}: {e}", shown(&url)))));
    let mut res = loop {
        match answer.recv_timeout(Duration::from_millis(100)) {
            Ok(res) => break res?,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) if piece("") => {}
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => return Ok(()),
            Err(_) => return Err(format!("{}: no answer", shown(&s.url))),
        }
    };
    if res.status().as_u16() >= 400 {
        let text = res.body_mut().read_to_string().unwrap_or_default();
        return Err(format!("{} {}: {}", shown(&s.url), res.status(), said(&text)));
    }
    let mut thinking = false;
    // An answer is text: 8 MB is far more than any, and a line without end stops there.
    for line in io::BufRead::lines(io::BufReader::new(res.into_body().into_with_config().limit(8 << 20).reader())) {
        let line = line.map_err(|e| e.to_string())?;
        // OpenAI servers send `data: {…}` lines and `data: [DONE]`; Ollama one JSON per line.
        let data = if s.openai { line.strip_prefix("data:").unwrap_or("").trim() } else { line.trim() };
        if data.is_empty() {
            continue;
        }
        if data == "[DONE]" {
            break;
        }
        let v: serde_json::Value = serde_json::from_str(data).map_err(|e| e.to_string())?;
        if let Some(e) = v["error"].as_str().or_else(|| v["error"]["message"].as_str()) {
            return Err(e.to_string());
        }
        let text = if s.openai { v["choices"][0]["delta"]["content"].as_str() } else { v["message"]["content"].as_str() };
        // Every line asks whether to go on, so Stop works while the model thinks aloud too:
        // an empty piece is only that question.
        let text = text.map(|t| unthink(t, &mut thinking)).unwrap_or_default();
        if !piece(&text) {
            break;
        }
        if v["done"].as_bool() == Some(true) {
            break;
        }
    }
    Ok(())
}

/// Whether a chat model turns its thinking off when the question ends in `/no_think`: Qwen3's
/// hybrid models, not its coder and instruct ones, which do not think at all.
fn no_think_by_word(model: &str) -> bool {
    let m = model.to_lowercase();
    m.contains("qwen3") && !m.contains("coder") && !m.contains("instruct") && !m.contains("thinking")
}

/// Have Ollama load the chat model now, on a thread, so that it is ready by the time the
/// sources are found: a model not loaded takes seconds to answer at all. Only for Ollama
/// (its own API loads a model on an empty request); other servers load as they see fit.
pub fn warm(cfg: &crate::config::SearchConfig) {
    if cfg.meaning_engine == "openai" || cfg.ask_model.is_empty() || crate::chat::of(&cfg.ask_model).is_some() {
        return;
    }
    let s = Server::new(cfg);
    // With the context Ask asks for, or Ollama loads the model a second time for the question.
    let body = serde_json::json!({ "model": cfg.ask_model, "options": { "num_ctx": crate::ask::context(cfg) } }).to_string();
    std::thread::spawn(move || {
        let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(Duration::from_secs(300))).http_status_as_error(false).build().into();
        let _ = agent.post(&format!("{}/api/generate", s.url)).header("Content-Type", "application/json").send(body);
    });
}

/// A piece of an answer without what a reasoning model thinks aloud between `<think>` and
/// `</think>`; `thinking` carries over from piece to piece.
// ponytail: tags split over two pieces are missed; servers send each tag as one token.
pub(crate) fn unthink(piece: &str, thinking: &mut bool) -> String {
    let mut out = String::new();
    let mut rest = piece;
    loop {
        let tag = if *thinking { "</think>" } else { "<think>" };
        match rest.find(tag) {
            Some(i) => {
                if !*thinking {
                    out.push_str(&rest[..i]);
                }
                *thinking = !*thinking;
                rest = &rest[i + tag.len()..];
            }
            None => {
                if !*thinking {
                    out.push_str(rest);
                }
                return out;
            }
        }
    }
}

/// A server's URL as an error names it: without the `user:password@` a URL may carry, which
/// would otherwise show in the status line and in Settings.
pub(crate) fn shown(url: &str) -> String {
    match url.split_once("://") {
        Some((scheme, rest)) if rest.split(['/', '?', '#']).next().is_some_and(|host| host.contains('@')) => format!("{scheme}://{}", &rest[rest.find('@').unwrap() + 1..]),
        _ => url.to_string(),
    }
}

/// TLS that trusts the system's certificate store, as the update check does, so a proxy that
/// inspects TLS or a server with a company certificate works. In Termux, Termux's store.
pub(crate) fn tls() -> ureq::tls::TlsConfig {
    use ureq::tls::{PemItem, RootCerts};
    let termux: Vec<_> = crate::termux::certificates()
        .iter()
        .flat_map(|pem| ureq::tls::parse_pem(pem).filter_map(|i| if let Ok(PemItem::Certificate(c)) = i { Some(c) } else { None }).collect::<Vec<_>>())
        .collect();
    let roots = if termux.is_empty() { RootCerts::PlatformVerifier } else { RootCerts::new_with_certs(&termux) };
    ureq::tls::TlsConfig::builder().root_certs(roots).build()
}

/// The embedding models a server has: Ollama's pulled models, or an OpenAI server's list,
/// with the API `key` a server may want.
pub fn server_models(openai: bool, url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    let url = if url.is_empty() { OLLAMA } else { url.trim_end_matches('/') };
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(Duration::from_secs(5))).build().into();
    let path = if openai { "/models" } else { "/api/tags" };
    let mut req = agent.get(&format!("{url}{path}"));
    if let Some(key) = key {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    let text = req.call().map_err(|e| format!("{}: {e}", shown(url)))?.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let names = if openai { v["data"].as_array().map(|d| d.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()) } else { v["models"].as_array().map(|d| d.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect()) };
    Ok(names.unwrap_or_default())
}

/// The models on a server that can answer, for Ask: Ollama's pulled models without those that
/// only make vectors (as its `/api/show` says), or every model an OpenAI server lists (it does
/// not say which can chat; the chosen one is tried when saved, by `chat_problem`).
pub fn chat_models(openai: bool, url: &str, key: Option<&str>) -> Result<Vec<String>, String> {
    let names = server_models(openai, url, key)?;
    if openai {
        return Ok(names);
    }
    let url = if url.is_empty() { OLLAMA } else { url.trim_end_matches('/') };
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(Duration::from_secs(5))).http_status_as_error(false).build().into();
    Ok(names.into_iter().filter(|m| !matches!(ollama_can(&agent, url, m), Ok(Some(c)) if !c.iter().any(|x| x == "completion"))).collect())
}

/// What an Ollama model can do, from `/api/show`: its capabilities ("completion", "embedding",
/// …), or None when the server does not say (an Ollama before 0.6.4).
pub(crate) fn ollama_can(agent: &ureq::Agent, url: &str, model: &str) -> Result<Option<Vec<String>>, String> {
    let body = serde_json::json!({ "model": model }).to_string();
    let mut res = agent.post(&format!("{url}/api/show")).header("Content-Type", "application/json").send(body).map_err(|e| format!("{}: {e}", shown(url)))?;
    let text = res.body_mut().read_to_string().map_err(|e| e.to_string())?;
    if res.status().as_u16() >= 400 {
        return Err(format!("{} {}: {}", shown(url), res.status(), said(&text)));
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    Ok(v["capabilities"].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(String::from)).collect()))
}

/// The error a server's answer names (`{"error": "…"}` or `{"error": {"message": "…"}}`), or
/// the answer itself.
pub(crate) fn said(text: &str) -> String {
    let v: serde_json::Value = serde_json::from_str(text).unwrap_or_default();
    v["error"].as_str().or_else(|| v["error"]["message"].as_str()).map_or_else(|| text.to_string(), String::from)
}

/// Why Ask's chat model cannot answer, in words for the user; None when it can, or when that
/// is not known without trying. Ollama is asked what the model can do; an OpenAI server is
/// sent a one-word question only when `try_it` (Settings saving the model), as it does not say.
pub fn chat_problem(cfg: &crate::config::SearchConfig, try_it: bool) -> Option<String> {
    let model = cfg.ask_model.as_str();
    if model.is_empty() {
        return None;
    }
    if let Some(m) = crate::chat::of(model) {
        return (!m.installed()).then(|| crate::t!("ask.builtin_missing", "model" => m.name));
    }
    let s = Server::new(cfg);
    if !s.openai {
        return match ollama_can(&s.agent, &s.url, model) {
            Ok(Some(c)) if !c.iter().any(|x| x == "completion") => Some(crate::t!("search.ask_not_chat", "model" => model.trim_end_matches(":latest"), "example" => "qwen3:8b")),
            Ok(_) => None,
            Err(e) => Some(e),
        };
    }
    if !try_it {
        return None;
    }
    let body = serde_json::json!({ "model": model, "messages": [{ "role": "user", "content": "Hi" }], "max_tokens": 1, "stream": false });
    let mut req = s.agent.post(&format!("{}/chat/completions", s.url)).header("Content-Type", "application/json");
    if let Some(key) = &s.key {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    match req.send(body.to_string()) {
        Err(e) => Some(format!("{}: {e}", shown(&s.url))),
        Ok(mut res) if res.status().as_u16() >= 400 => Some(format!("{} {}: {}", shown(&s.url), res.status(), said(&res.body_mut().read_to_string().unwrap_or_default()))),
        Ok(_) => None,
    }
}

/// Whether Ollama answers on this machine, asked quickly.
pub fn ollama_here() -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(Duration::from_millis(400))).build().into();
    agent.get(&format!("{OLLAMA}/api/version")).call().is_ok()
}

/// Ask Ollama to pull a model, with its progress in `p`.
pub fn ollama_pull(url: &str, model: &str, p: &Progress) -> io::Result<()> {
    let url = if url.is_empty() { OLLAMA } else { url.trim_end_matches('/') };
    let agent: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_connect(Some(Duration::from_secs(10))).build().into();
    let mut body = agent.post(&format!("{url}/api/pull")).send(serde_json::json!({ "model": model, "stream": true }).to_string()).map_err(io::Error::other)?.into_body();
    // One JSON line per step: {"status", "total", "completed"} while layers come, {"error"} if not.
    for line in io::BufRead::lines(io::BufReader::new(body.as_reader())) {
        let v: serde_json::Value = serde_json::from_str(&line?).unwrap_or_default();
        if let Some(e) = v["error"].as_str() {
            return Err(io::Error::other(e.to_string()));
        }
        if let (Some(total), Some(done)) = (v["total"].as_u64(), v["completed"].as_u64()) {
            p.total.store(total, Ordering::Relaxed);
            p.done.store(done, Ordering::Relaxed);
        }
        if p.cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
    }
    Ok(())
}

/// A passage of a file, and the Markdown heading it sits under ("" for none).
#[derive(Debug, Clone, PartialEq)]
pub struct Passage {
    pub text: String,
    pub heading: String,
}

/// The passages of a text that get a vector: about `WORDS` words each, cut at headings
/// (`markdown`) and at the end of a paragraph once half full, otherwise inside it with
/// `OVERLAP` words shared; `PASSAGES` at most. The same text always gives the same passages:
/// a vector is found again by its number.
pub fn passages(text: &str, markdown: bool) -> Vec<Passage> {
    // (passage, whether it starts a section)
    let mut all: Vec<(Passage, bool)> = vec![];
    let (mut cur, mut fresh, mut heading, mut starts, mut code) = (Vec::<&str>::new(), 0, "", true, false);
    let mut flush = |cur: &mut Vec<&str>, fresh: &mut usize, heading: &str, starts: &mut bool, keep: usize| {
        if *fresh > 0 {
            all.push((Passage { text: cur.join(" "), heading: heading.to_string() }, *starts));
            *starts = false;
        }
        cur.drain(..cur.len() - keep.min(cur.len()));
        *fresh = 0;
    };
    for line in text.lines() {
        let line = line.trim();
        let hashes = line.len() - line.trim_start_matches('#').len();
        let mut words = line;
        // A `# comment` in a fenced block of code is no heading.
        code ^= markdown && (line.starts_with("```") || line.starts_with("~~~"));
        if markdown && !code && (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
            flush(&mut cur, &mut fresh, heading, &mut starts, 0);
            heading = line[hashes..].trim();
            words = heading;
            starts = true;
        } else if line.is_empty() && cur.len() >= WORDS / 2 {
            flush(&mut cur, &mut fresh, heading, &mut starts, 0);
        }
        for w in words.split_whitespace() {
            cur.push(w);
            fresh += 1;
            if cur.len() == WORDS {
                flush(&mut cur, &mut fresh, heading, &mut starts, OVERLAP);
            }
        }
    }
    flush(&mut cur, &mut fresh, heading, &mut starts, 0);
    all.retain(|(p, _)| p.text.chars().filter(|c| c.is_alphabetic()).count() >= 20);
    let n = all.len();
    if n <= PASSAGES {
        return all.into_iter().map(|(p, _)| p).collect();
    }
    // Too long: the start and the end, the first passage of each section (up to half), and
    // passages evenly spread over the rest.
    let mut keep = vec![false; n];
    for i in (0..16).chain(n - 4..n) {
        keep[i] = true;
    }
    let heads: Vec<usize> = (0..n).filter(|&i| all[i].1).collect();
    let m = heads.len().min(PASSAGES / 2);
    for k in 0..m {
        keep[heads[k * heads.len() / m]] = true;
    }
    let rest: Vec<usize> = (0..n).filter(|&i| !keep[i]).collect();
    let left = PASSAGES - (n - rest.len());
    for k in 0..left {
        keep[rest[k * rest.len() / left]] = true;
    }
    all.into_iter().zip(keep).filter(|(_, k)| *k).map(|((p, _), _)| p).collect()
}

/// `next`, the passage after `text`, put on its end without the `OVERLAP` words they share
/// when a paragraph was cut between them.
pub fn join(text: &mut String, next: &str) {
    let (a, b): (Vec<&str>, Vec<&str>) = (text.split_whitespace().collect(), next.split_whitespace().collect());
    let shared = (1..=OVERLAP.min(a.len()).min(b.len())).rev().find(|&k| a[a.len() - k..] == b[..k]).unwrap_or(0);
    let rest = b[shared..].join(" ");
    if !rest.is_empty() {
        text.push(' ');
        text.push_str(&rest);
    }
}

/// Whether a file's text is Markdown, whose `#` lines are headings.
pub fn is_markdown(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "md" | "markdown" | "mdx")
}

/// What the model is shown of a passage: a line with the file's name, its folder and the
/// heading, so a question that names them finds it, then the passage.
pub fn shown_to_model(path: &str, p: &Passage) -> String {
    let path = std::path::Path::new(path);
    let name = path.file_name().map(|n| n.to_string_lossy()).unwrap_or_default();
    let folder: Vec<_> = path.parent().into_iter().flat_map(|d| d.iter().rev().take(2)).map(|c| c.to_string_lossy()).collect();
    let folder = folder.into_iter().rev().collect::<Vec<_>>().join("/");
    let heading = if p.heading.is_empty() { String::new() } else { format!(" · {}", p.heading) };
    format!("{name} · {folder}{heading}\n{}", p.text)
}

/// A vector in its length plus 4 bytes: its numbers as signed bytes of its largest one, then
/// that one.
pub fn pack(v: &[f32]) -> Vec<u8> {
    let scale = v.iter().fold(0f32, |m, x| m.max(x.abs())).max(f32::MIN_POSITIVE);
    let mut out: Vec<u8> = v.iter().map(|x| (x / scale * 127.0).round().clamp(-127.0, 127.0) as i8 as u8).collect();
    out.extend(scale.to_le_bytes());
    out
}

/// How much a packed vector and a question's vector point the same way (1: the same).
pub fn score(packed: &[u8], q: &[f32]) -> f32 {
    let n = q.len();
    if packed.len() != n + 4 || n == 0 {
        return 0.0;
    }
    let scale = f32::from_le_bytes([packed[n], packed[n + 1], packed[n + 2], packed[n + 3]]) / 127.0;
    packed[..n].iter().zip(q).map(|(b, x)| (*b as i8) as f32 * x).sum::<f32>() * scale
}

/// The signs of a packed vector, one bit each: a first sieve over many passages.
pub fn signs(packed: &[u8]) -> Box<[u64]> {
    let n = packed.len().saturating_sub(4);
    let mut out = vec![0u64; n.div_ceil(64)];
    for (i, b) in packed.iter().take(n).enumerate() {
        if (*b as i8) > 0 {
            out[i / 64] |= 1 << (i % 64);
        }
    }
    out.into_boxed_slice()
}

/// Bits two sign sets share: the more, the closer.
pub fn alike(a: &[u64], b: &[u64]) -> u32 {
    a.iter().zip(b).map(|(x, y)| (!(x ^ y)).count_ones()).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ask_streams_the_answer_without_the_thinking() {
        use std::io::{Read, Write};
        let mut thinking = false;
        assert_eq!(unthink("a<think>b</think>c", &mut thinking), "ac");
        assert_eq!(unthink("<think>still", &mut thinking), "");
        assert!(thinking);
        assert_eq!(unthink(" more</think>Yes", &mut thinking), "Yes");

        // A server that answers once, the Ollama way, and shows what it was sent.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let mut got = Vec::new();
            let mut buf = [0; 4096];
            while !String::from_utf8_lossy(&got).contains("\"think\":false}") {
                let n = c.read(&mut buf).unwrap();
                got.extend_from_slice(&buf[..n]);
            }
            let lines = "{\"message\":{\"content\":\"<think>hm</think>\"}}\n{\"message\":{\"content\":\"Rocket \"}}\n{\"message\":{\"content\":\"[1]\"},\"done\":true}\n";
            write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{lines}", lines.len()).unwrap();
            let _ = c.shutdown(std::net::Shutdown::Write);
            let _ = c.read_to_end(&mut Vec::new());
            String::from_utf8_lossy(&got).into_owned()
        });
        let cfg = crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: url, ask_model: "chat".into(), ..Default::default() };
        let mut answer = String::new();
        let sources = [(std::path::PathBuf::from("/p/rocket.md"), "The rocket is named Tern.".to_string())];
        ask(&cfg, &[("What is it?".into(), "A rocket [1].".into())], "Its name?", &sources, |p| {
            answer.push_str(p);
            true
        })
        .unwrap();
        assert_eq!(answer, "Rocket [1]");
        let sent = server.join().unwrap();
        assert!(sent.starts_with("POST /api/chat"));
        assert!(sent.contains("[1] /p/rocket.md\\nThe rocket is named Tern."), "{sent}");
        assert!(sent.contains("\"model\":\"chat\""));
        assert!(sent.contains("A rocket [1]."), "the turns before go along");
        assert!(sent.contains("\"think\":false"), "asked not to think first: {sent}");
        assert!(sent.contains("\"num_ctx\":8192"), "the context the model was warmed with: {sent}");

        let off = crate::config::SearchConfig::default();
        assert!(ask(&off, &[], "q", &sources, |_| true).is_err(), "no chat model, no Ask");
    }

    #[test]
    fn ask_asks_qwen3_not_to_think_by_word() {
        assert!(no_think_by_word("Qwen3-8B-GGUF") && no_think_by_word("qwen/qwen3-4b"));
        assert!(!no_think_by_word("qwen3-coder:30b") && !no_think_by_word("Qwen3-4B-Instruct-2507") && !no_think_by_word("llama3.2"));
    }

    /// Stop is heard while the server has not said anything yet (a model loading), and Ollama
    /// is asked to load the chat model ahead of the question.
    #[test]
    fn ask_stops_before_the_first_byte_and_warms_the_model() {
        use std::io::Read;
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = std::thread::spawn(move || {
            let (mut heads, mut open) = (vec![], vec![]);
            for _ in 0..2 {
                let (mut c, _) = listener.accept().unwrap();
                let mut buf = [0; 4096];
                let n = c.read(&mut buf).unwrap_or(0);
                heads.push(String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string());
                open.push(c);
            }
            // Not a byte in answer: the client gives up first.
            std::thread::sleep(Duration::from_secs(1));
            heads
        });
        let cfg = crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: url, ask_model: "chat".into(), ..Default::default() };
        warm(&cfg);
        let start = std::time::Instant::now();
        let mut asked = 0;
        ask(&cfg, &[], "q", &[("/p/a.md".into(), "a".into())], |p| {
            asked += 1;
            assert_eq!(p, "");
            asked < 3
        })
        .unwrap();
        assert!(start.elapsed() < Duration::from_secs(2), "stopped while waiting, not after the timeout");
        assert_eq!(asked, 3);
        let mut heads = server.join().unwrap();
        heads.sort();
        assert_eq!(heads, ["POST /api/chat HTTP/1.1", "POST /api/generate HTTP/1.1"]);
    }

    /// Servers with the OpenAI API (Lemonade, LM Studio, llama.cpp) load a model when asked:
    /// nothing is sent to them ahead of the question.
    #[test]
    fn warm_sends_nothing_to_an_openai_server() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        warm(&crate::config::SearchConfig { meaning_engine: "openai".into(), meaning_url: url, ask_model: "chat".into(), ..Default::default() });
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(listener.accept().map(drop).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
    }

    /// A server named with `user:password@` in Settings: its errors do not repeat the password.
    #[test]
    fn ask_errors_never_show_the_password_in_the_url() {
        assert_eq!(shown("http://me:hunter2@host:11434/v1"), "http://host:11434/v1");
        assert_eq!(shown("http://host/x?u=a@b"), "http://host/x?u=a@b");
        assert_eq!(shown("not a url"), "not a url");
        let (url, server) = one_answer("401 Unauthorized", "{\"error\":\"who?\"}");
        let url = url.replace("http://", "http://me:hunter2@");
        let cfg = crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: url, ask_model: "chat".into(), ..Default::default() };
        let err = ask(&cfg, &[], "q", &[("/p/a.md".into(), "a".into())], |_| true).unwrap_err();
        assert!(err.contains("401") && !err.contains("hunter2"), "{err}");
        let _ = server.join();
        let err = server_models(false, "http://me:hunter2@127.0.0.1:9", None).unwrap_err();
        assert!(!err.contains("hunter2"), "{err}");
    }

    /// Stop is heard while the model still thinks aloud, before any of the answer comes.
    #[test]
    fn ask_stops_while_the_model_thinks() {
        let think = "{\"message\":{\"content\":\"<think>hm\"}}\n".repeat(50);
        let (url, server) = one_answer("200 OK", &format!("{think}{{\"message\":{{\"content\":\"</think>Rocket\"}},\"done\":true}}\n"));
        let cfg = crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: url, ask_model: "chat".into(), ..Default::default() };
        let mut heard = vec![];
        ask(&cfg, &[], "q", &[("/p/a.md".into(), "a".into())], |p| {
            heard.push(p.to_string());
            false
        })
        .unwrap();
        assert_eq!(heard, [""], "asked once, while thinking");
        let _ = server.join();
    }

    /// A server that answers once, with what it was sent.
    fn one_answer(status: &str, body: &str) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let reply = format!("HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        let server = std::thread::spawn(move || {
            let (mut c, _) = listener.accept().unwrap();
            let got = request(&mut c);
            write!(c, "{reply}").unwrap();
            // Closed only once the client has read it all: on Windows, a socket closed with
            // bytes left unread resets the connection, and the client sees an error.
            let _ = c.shutdown(std::net::Shutdown::Write);
            let _ = c.read_to_end(&mut Vec::new());
            got
        });
        (url, server)
    }

    /// One whole request, head and body, as it came.
    fn request(c: &mut std::net::TcpStream) -> String {
        use std::io::Read;
        let mut got = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = c.read(&mut buf).unwrap();
            got.extend_from_slice(&buf[..n]);
            let text = String::from_utf8_lossy(&got);
            let Some((head, body)) = text.split_once("\r\n\r\n") else { continue };
            let len = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, n)| n.trim().parse::<usize>().ok())).unwrap_or(0);
            if body.len() >= len {
                return text.into_owned();
            }
        }
    }

    /// A fake Ollama with a chat model and one that only makes vectors: `/api/tags` lists
    /// them, `/api/show` says what each can do.
    fn fake_ollama() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let mut c = c.unwrap();
                let got = request(&mut c);
                let (status, body) = if got.starts_with("GET /api/tags") {
                    ("200 OK", r#"{"models":[{"name":"qwen3:8b"},{"name":"bge-m3:latest"}]}"#)
                } else if got.contains(r#""model":"qwen3:8b""#) {
                    ("200 OK", r#"{"capabilities":["completion","tools"]}"#)
                } else if got.contains(r#""model":"bge-m3:latest""#) {
                    ("200 OK", r#"{"capabilities":["embedding"]}"#)
                } else {
                    ("404 Not Found", r#"{"error":"model 'nope' not found"}"#)
                };
                write!(c, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                let _ = c.shutdown(std::net::Shutdown::Write);
                let _ = c.read_to_end(&mut Vec::new());
            }
        });
        url
    }

    /// An embedding model is not offered for Ask, and one set by hand is named as such.
    #[test]
    fn ask_tells_a_model_that_only_makes_vectors() {
        let url = fake_ollama();
        assert_eq!(chat_models(false, &url, None).unwrap(), ["qwen3:8b"]);
        let cfg = |m: &str| crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: url.clone(), ask_model: m.into(), ..Default::default() };
        assert_eq!(chat_problem(&cfg("qwen3:8b"), true), None);
        let why = chat_problem(&cfg("bge-m3:latest"), false).unwrap();
        assert!(why.contains("bge-m3") && !why.contains(":latest") && why.contains("qwen3:8b"), "{why}");
        let why = chat_problem(&cfg("nope"), false).unwrap();
        assert!(why.contains("not found"), "{why}");
        assert_eq!(chat_problem(&cfg(""), true), None, "no model, nothing to say");
    }

    /// A change of the vectors' model is said before it is saved, with the files it reads again;
    /// the same model with its tag is no change.
    #[test]
    fn a_change_of_model_says_what_it_costs() {
        let old = crate::config::SearchConfig { meaning_engine: "ollama".into(), meaning_url: "http://127.0.0.1:9".into(), meaning_model: "bge-m3".into(), ..Default::default() };
        let tagged = crate::config::SearchConfig { meaning_model: "bge-m3:latest".into(), ..old.clone() };
        assert_eq!(change_notice(&old, &tagged, 3437, 3437), None);
        let vectors = format!("{{\"data\":[{}]}}", ["{\"embedding\":[0.6,0.8]}"; 8].join(","));
        let (url, server) = one_answer("200 OK", &vectors);
        let other = crate::config::SearchConfig { meaning_engine: "openai".into(), meaning_url: url, meaning_model: "nomic".into(), ..old.clone() };
        assert_eq!(change_notice(&old, &other, 0, 0), None, "no vectors, nothing to lose");
        let said = change_notice(&old, &other, 3437, 3437).unwrap();
        assert!(said.contains("3437") && said.contains("about"), "{said}");
        assert!(server.join().unwrap().starts_with("POST /embeddings"));
    }

    /// An OpenAI server does not say what a model can do: it is tried with a one-word
    /// question when asked to, and not otherwise.
    #[test]
    fn ask_tries_an_openai_model_only_when_saved() {
        let cfg = |url: &str| crate::config::SearchConfig { meaning_engine: "openai".into(), meaning_url: url.into(), ask_model: "embed".into(), ..Default::default() };
        assert_eq!(chat_problem(&cfg("http://127.0.0.1:9"), false), None);
        let (url, server) = one_answer("400 Bad Request", r#"{"error":{"message":"embed does not support chat"}}"#);
        let why = chat_problem(&cfg(&url), true).unwrap();
        assert!(why.contains("400") && why.contains("does not support chat"), "{why}");
        let sent = server.join().unwrap();
        assert!(sent.starts_with("POST /chat/completions") && sent.contains(r#""max_tokens":1"#), "{sent}");
    }

    #[test]
    fn meaning_sends_the_key_and_tells_a_refusal_from_a_server_that_is_down() {
        // A variable every machine has stands in for the key's.
        let (var, secret) = std::env::vars().find(|(_, v)| !v.is_empty() && v.is_ascii() && !v.contains(['\r', '\n'])).unwrap();
        let (url, server) = one_answer("200 OK", "{\"data\":[{\"id\":\"nomic\"}]}");
        assert_eq!(server_models(true, &url, Some(&secret)).unwrap(), ["nomic"]);
        let sent_key = |sent: String| sent.to_lowercase().contains(&format!("authorization: bearer {secret}").to_lowercase());
        assert!(sent_key(server.join().unwrap()), "the models list wants the key too");

        let cfg = crate::config::SearchConfig { meaning_engine: "openai".into(), meaning_url: String::new(), meaning_model: "m".into(), meaning_key_env: var, ..Default::default() };
        assert_eq!(key_of(&cfg).as_deref(), Some(secret.as_str()));
        let (url, server) = one_answer("404 Not Found", "{\"error\":{\"message\":\"no such model\"}}");
        let engine = Engine::from_config(&crate::config::SearchConfig { meaning_url: url, ..cfg.clone() }).unwrap();
        match engine.passages(&["a passage".into()]) {
            Err(NoVectors::Refused(why)) => assert!(why.contains("no such model"), "{why}"),
            other => panic!("{other:?}"),
        }
        assert!(sent_key(server.join().unwrap()));
        let engine = Engine::from_config(&crate::config::SearchConfig { meaning_url: "http://127.0.0.1:9".into(), ..cfg }).unwrap();
        assert!(matches!(engine.passages(&["a passage".into()]), Err(NoVectors::Down(_))));
    }

    #[test]
    fn meaning_packs_vectors_and_scores_them() {
        let unit = |f: &dyn Fn(usize) -> f32| {
            let v: Vec<f32> = (0..DIMS).map(f).collect();
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.into_iter().map(|x| x / n).collect::<Vec<f32>>()
        };
        let a = unit(&|i| (i as f32 * 0.37).sin());
        let b = unit(&|i| (i as f32 * 0.37).sin() + 0.3 * (i as f32 * 1.7).cos());
        let c = unit(&|i| (i as f32 * 2.9).cos());
        let exact = |x: &[f32], y: &[f32]| x.iter().zip(y).map(|(p, q)| p * q).sum::<f32>();
        for (x, y) in [(&a, &a), (&a, &b), (&a, &c)] {
            assert!((score(&pack(x), y) - exact(x, y)).abs() < 0.01, "packing keeps the score");
        }
        assert!(score(&pack(&a), &b) > score(&pack(&a), &c));
        assert!(alike(&signs(&pack(&a)), &signs(&pack(&b))) > alike(&signs(&pack(&a)), &signs(&pack(&c))));
        assert_eq!(alike(&signs(&pack(&a)), &signs(&pack(&a))), DIMS as u32);
        assert_eq!(score(&[1, 2, 3], &a), 0.0, "a broken vector scores nothing");

        assert_eq!(passages("too short to mean much", false).len(), 0);
        assert!(size() > 400_000_000);
    }

    /// Neighbouring passages joined read as the text did: the words a cut shares come once.
    #[test]
    fn join_drops_the_words_two_passages_share() {
        let text: String = (0..300).map(|i| format!("w{i} ")).collect();
        let p = passages(&text, false);
        let mut joined = p[0].text.clone();
        join(&mut joined, &p[1].text);
        join(&mut joined, &p[2].text);
        let want: Vec<String> = (0..p[2].text.split_whitespace().last().unwrap()[1..].parse::<usize>().unwrap() + 1).map(|i| format!("w{i}")).collect();
        assert_eq!(joined, want.join(" "));
        let mut apart = "one two".to_string();
        join(&mut apart, "three four");
        assert_eq!(apart, "one two three four");
    }

    /// A whole document is cut into passages: every word is in one, a cut inside a paragraph
    /// shares words with the passage before, and a Markdown heading starts a passage and is
    /// shown to the model with it. A longer one than the cap keeps its start, end and headings.
    #[test]
    fn passages_cover_the_whole_document() {
        let text: String = (0..30).map(|p| (0..90).map(|w| format!("w{p}x{w}")).collect::<Vec<_>>().join(" ") + "\n\n").collect();
        let ps = passages(&text, false);
        for word in text.split_whitespace() {
            assert!(ps.iter().any(|p| p.text.split(' ').any(|w| w == word)), "{word} is in no passage");
        }
        assert!(ps.iter().all(|p| p.text.split(' ').count() <= WORDS));
        // 90-word paragraphs: a passage each. One paragraph of 300 words: cut with 20 shared.
        assert!(ps[0].text.starts_with("w0x0 ") && ps[0].text.ends_with(" w0x89") && ps[1].text.starts_with("w1x0 "));
        let one = (0..300).map(|w| format!("x{w}")).collect::<Vec<_>>().join(" ");
        let ps = passages(&one, false);
        assert_eq!(ps.iter().map(|p| p.text.split(' ').next().unwrap()).collect::<Vec<_>>(), ["x0", "x100", "x200"]);

        let md = format!("# Fuel\n\n{}\n\n```sh\n# not a heading\n```\n\n## Launch window\n\n{}\n", "tanks fuel budget kerosene ".repeat(20), "the window opens at dawn ".repeat(10));
        let ps = passages(&md, true);
        assert_eq!(ps.iter().map(|p| p.heading.as_str()).collect::<Vec<_>>(), ["Fuel", "Launch window"]);
        assert!(ps[1].text.starts_with("Launch window the window"), "{}", ps[1].text);
        assert_eq!(shown_to_model("/home/u/rocket/notes/plan.md", &ps[1]).lines().next(), Some("plan.md · rocket/notes · Launch window"));
        assert!(passages(&md, false).iter().all(|p| p.heading.is_empty()), "a # in code is no heading");

        // 60,000 words with a heading every 2,000: the cap, the first and the last passage, and
        // every heading.
        let long: String = (0..30).map(|s| format!("# Part {s}\n\n{}\n\n", (0..2000).map(|w| format!("p{s}w{w}")).collect::<Vec<_>>().join(" "))).collect();
        let all = passages(&long, true);
        assert_eq!(all.len(), PASSAGES);
        assert!(all[0].text.contains("p0w0") && all.last().unwrap().text.contains("p29w1999"));
        assert!((0..30).all(|s| all.iter().any(|p| p.text.contains(&format!("p{s}w0 ")))), "every part's start");
        assert_eq!(passages(&long, true), all, "the same text, the same passages");
    }

    /// With the model downloaded: a question finds the passage about it, across languages.
    #[test]
    fn meaning_finds_passages_by_meaning() {
        let Some(e) = Embedder::load(false) else { return };
        let q = e.query("how much fuel does the rocket need").unwrap();
        let score = |p: &str| score(&pack(&e.passages(&[p.to_string()])[0]), &q);
        let (fuel, danish, apples) = (score("The fuel budget for flight seven is the largest cost of the launch."), score("Brændstofbudgettet for flyvning syv er den største udgift ved opsendelsen."), score("Opskrift på æblekage med kanel og vaniljesauce."));
        assert!(fuel > apples && danish > apples, "{fuel} {danish} {apples}");
    }

    /// Passages of different lengths in one pass give the vectors each gives alone: the
    /// padding of the shorter ones is masked.
    #[test]
    fn meaning_batches_give_the_vectors_of_one_by_one() {
        let Some(e) = Embedder::load(true) else { return };
        let bert = e.bert.read().unwrap().clone();
        let texts = ["passage: short", PROBE, "passage: a little longer than the first one is"];
        let together = bert.vectors(&e.tokenizer, e.pad, &texts).unwrap();
        for (t, v) in texts.iter().zip(&together) {
            let alone = bert.vectors(&e.tokenizer, e.pad, &[t]).unwrap().remove(0);
            assert!(cosine(v, &alone) > 0.9999, "{t}: {}", cosine(v, &alone));
        }
    }

    /// The device is chosen with stand-ins for the CPU and the GPU: each way to the CPU says why.
    #[test]
    fn meaning_falls_back_to_the_cpu_and_says_why() {
        let sleep = |ms| std::thread::sleep(Duration::from_millis(ms));
        // A device: its name, the vector it gives, the time it takes per text.
        type Fake = (&'static str, Vec<f32>, u64);
        let vectors = |m: &Fake, t: &[&str]| {
            sleep(m.2 * t.len() as u64);
            if m.1.is_empty() { Err("out of memory".to_string()) } else { Ok(vec![m.1.clone(); t.len()]) }
        };
        let cpu = || ("cpu", vec![1.0, 0.0], 20);
        let gpu = |v: Vec<f32>, ms| move || Ok::<Fake, Fallback>(("gpu", v, ms));

        let (m, runs) = choose(false, cpu(), gpu(vec![1.0, 0.0001], 0), vectors);
        assert_eq!(m.0, "gpu");
        assert!(runs.metal && runs.cpu_why.is_none() && runs.faster > 1.0, "{runs:?}");

        let (m, runs) = choose(true, cpu(), gpu(vec![1.0, 0.0], 0), vectors);
        assert_eq!((m.0, runs.cpu_why), ("cpu", Some(Fallback::Chosen)));
        let (m, runs) = choose(false, cpu(), || Err(Fallback::NoMetal("no device".into())), vectors);
        assert_eq!((m.0, runs.cpu_why), ("cpu", Some(Fallback::NoMetal("no device".into()))));
        let (m, runs) = choose(false, cpu(), gpu(vec![], 0), vectors);
        assert_eq!((m.0, runs.cpu_why), ("cpu", Some(Fallback::Load("out of memory".into()))));
        // The probe: a vector that points elsewhere is not the CPU's.
        let (m, runs) = choose(false, cpu(), gpu(vec![1.0, 0.1], 0), vectors);
        assert!(m.0 == "cpu" && matches!(runs.cpu_why, Some(Fallback::Probe(c)) if c < AGREE && c > 0.99), "{runs:?}");
        let (m, runs) = choose(false, ("cpu", vec![1.0, 0.0], 0), gpu(vec![1.0, 0.0], 20), vectors);
        assert!(m.0 == "cpu" && matches!(runs.cpu_why, Some(Fallback::Slower(_))), "{runs:?}");
        assert!(!runs.text().is_empty() && Runs { metal: true, ..Default::default() }.text() != Runs::default().text());
    }

    /// On a Mac with the model downloaded and a Metal GPU: the model runs there, or says why
    /// not (GitHub's macOS machines are virtual, and their Metal may not be the real thing).
    #[cfg(target_os = "macos")]
    #[test]
    fn meaning_runs_on_metal_when_it_can() {
        if Device::new_metal(0).is_err() {
            return eprintln!("skipped: no Metal device");
        }
        let (Some(gpu), Some(cpu)) = (Embedder::load(false), Embedder::load(true)) else { return eprintln!("skipped: the model is not downloaded") };
        let runs = gpu.runs();
        assert!(runs.metal != runs.cpu_why.is_some(), "{runs:?}");
        if !runs.metal {
            return eprintln!("skipped: on the CPU: {}", runs.text());
        }
        let texts: Vec<String> = (0..40).map(|i| format!("Passage {i} about the fuel budget of flight {i}, {}", "and more words ".repeat(i))).collect();
        for (a, b) in gpu.passages(&texts).iter().zip(cpu.passages(&texts)) {
            assert!(cosine(a, &b) >= AGREE, "{}", cosine(a, &b));
        }
    }
}
