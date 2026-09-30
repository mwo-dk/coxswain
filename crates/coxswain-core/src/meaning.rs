//! Search by meaning: a small multilingual language model (multilingual-e5-small, 384
//! numbers per passage) turns passages of text into vectors, and a question into one too;
//! passages whose vectors point the same way are about the same thing, in any language and
//! whatever the words. It runs on the CPU with candle, in pure Rust.
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
use sha2::{Digest, Sha256};

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
/// Words per passage, and passages per file: the start of a long document says what it is about.
const WORDS: usize = 120;
const PASSAGES: usize = 8;

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
    let dir = folder().ok_or_else(|| io::Error::other("no cache folder"))?;
    std::fs::create_dir_all(&dir)?;
    p.total.store(size(), Ordering::Relaxed);
    p.done.store(0, Ordering::Relaxed);
    // The system's certificate store, as for the update check: a proxy that inspects TLS is trusted.
    let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(20))).tls_config(tls).build().into();
    for (name, sha, len) in FILES {
        let file = dir.join(name);
        if std::fs::metadata(&file).is_ok_and(|m| m.len() == *len) {
            p.done.fetch_add(*len, Ordering::Relaxed);
            continue;
        }
        let url = format!("https://huggingface.co/{REPO}/resolve/{REVISION}/{name}");
        let mut body = agent.get(&url).header("User-Agent", concat!("coxswain/", env!("CARGO_PKG_VERSION"))).call().map_err(io::Error::other)?.into_body();
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
        if got != *len || hex != *sha {
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

/// The model, loaded, with its tokenizer. Works on two threads, so the machine stays the user's.
pub struct Embedder {
    model: BertModel,
    tokenizer: tokenizers::Tokenizer,
    pool: rayon::ThreadPool,
}

impl Embedder {
    pub fn load() -> Option<Embedder> {
        let dir = folder().filter(|_| installed())?;
        let config: Config = serde_json::from_str(&std::fs::read_to_string(dir.join("config.json")).ok()?).ok()?;
        // Memory-mapped: the pages are the file's, shared, and dropped by the system when short.
        let weights = unsafe { VarBuilder::from_mmaped_safetensors(&[dir.join("model.safetensors")], DType::F32, &Device::Cpu).ok()? };
        let model = BertModel::load(weights, &config).ok()?;
        let mut tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json")).ok()?;
        tokenizer.with_truncation(Some(tokenizers::TruncationParams { max_length: 512, ..Default::default() })).ok()?;
        let pool = rayon::ThreadPoolBuilder::new().num_threads(2).thread_name(|i| format!("coxswain-meaning-{i}")).build().ok()?;
        Some(Embedder { model, tokenizer, pool })
    }

    /// The vector of a question.
    pub fn query(&self, text: &str) -> Option<Vec<f32>> {
        self.embed(&format!("query: {text}"))
    }

    /// The vector of a passage of a document.
    pub fn passage(&self, text: &str) -> Option<Vec<f32>> {
        self.embed(&format!("passage: {text}"))
    }

    /// The mean of the model's last layer over the words, of length one.
    fn embed(&self, text: &str) -> Option<Vec<f32>> {
        self.pool.install(|| {
            let enc = self.tokenizer.encode(text, true).ok()?;
            let ids = Tensor::new(enc.get_ids(), &Device::Cpu).ok()?.unsqueeze(0).ok()?;
            let types = ids.zeros_like().ok()?;
            let mask = Tensor::new(enc.get_attention_mask(), &Device::Cpu).ok()?.unsqueeze(0).ok()?;
            let out = self.model.forward(&ids, &types, Some(&mask)).ok()?;
            let v: Vec<f32> = out.mean(1).ok()?.squeeze(0).ok()?.to_vec1().ok()?;
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            (n > 0.0).then(|| v.iter().map(|x| x / n).collect())
        })
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

impl Engine {
    /// The engine the settings ask for; `None` for the built-in one before it is downloaded.
    pub fn from_config(cfg: &crate::config::SearchConfig) -> Option<Engine> {
        match cfg.meaning_engine.as_str() {
            "ollama" | "openai" => Some(Engine::Server(Server::new(cfg))),
            _ => Embedder::load().map(Engine::Builtin),
        }
    }

    /// Which model made a vector: vectors of two models cannot be compared.
    pub fn id(&self) -> String {
        match self {
            Engine::Builtin(_) => format!("builtin:{MODEL}@{}", &REVISION[..8]),
            Engine::Server(s) => format!("{}:{}", if s.openai { "openai" } else { "ollama" }, s.model),
        }
    }

    /// The vector of a question.
    pub fn query(&self, text: &str) -> Result<Vec<f32>, String> {
        match self {
            Engine::Builtin(e) => e.query(text).ok_or_else(|| "the model gave no vector".into()),
            Engine::Server(s) => s.embed(&[format!("{}{text}", s.prefix().0)]).map(|mut v| v.remove(0)),
        }
    }

    /// The vectors of a file's passages, all at once. An error: the server did not answer, and
    /// the file waits for the next round.
    pub fn passages(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        match self {
            Engine::Builtin(e) => Ok(texts.iter().filter_map(|t| e.passage(t)).collect()),
            Engine::Server(s) => s.embed(&texts.iter().map(|t| format!("{}{t}", s.prefix().1)).collect::<Vec<_>>()),
        }
    }

    /// The least score of a passage worth showing, by what this model's scores look like:
    /// e5 puts unrelated text near 0.75, most others near 0.3.
    // ponytail: two numbers from trying e5 and bge-m3; a model with odd scores wants its own.
    pub fn floor(&self) -> f32 {
        match self {
            Engine::Builtin(_) => 0.77,
            Engine::Server(s) if s.model.contains("e5") => 0.77,
            Engine::Server(_) => 0.5,
        }
    }
}

impl Server {
    fn new(cfg: &crate::config::SearchConfig) -> Server {
        let openai = cfg.meaning_engine == "openai";
        let url = if cfg.meaning_url.is_empty() && !openai { OLLAMA.to_string() } else { cfg.meaning_url.trim_end_matches('/').to_string() };
        let model = if cfg.meaning_model.is_empty() && !openai { "bge-m3".to_string() } else { cfg.meaning_model.clone() };
        let key = (!cfg.meaning_key_env.is_empty()).then(|| std::env::var(&cfg.meaning_key_env).ok()).flatten();
        let tls = ureq::tls::TlsConfig::builder().root_certs(ureq::tls::RootCerts::PlatformVerifier).build();
        let agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).tls_config(tls).build().into();
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
    fn embed(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, String> {
        let (path, body) = if self.openai { ("/embeddings", serde_json::json!({ "model": self.model, "input": texts })) } else { ("/api/embed", serde_json::json!({ "model": self.model, "input": texts })) };
        let mut req = self.agent.post(&format!("{}{path}", self.url)).header("Content-Type", "application/json");
        if let Some(key) = &self.key {
            req = req.header("Authorization", &format!("Bearer {key}"));
        }
        let text = req.send(body.to_string()).map_err(|e| format!("{}: {e}", self.url))?.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        let rows: Vec<&serde_json::Value> = if self.openai { v["data"].as_array().map(|d| d.iter().map(|x| &x["embedding"]).collect()).unwrap_or_default() } else { v["embeddings"].as_array().map(|d| d.iter().collect()).unwrap_or_default() };
        if rows.len() != texts.len() {
            return Err(v["error"].as_str().or_else(|| v["error"]["message"].as_str()).unwrap_or("no vectors in the answer").to_string());
        }
        rows.into_iter()
            .map(|r| {
                let v: Vec<f32> = r.as_array().ok_or("not a vector")?.iter().filter_map(|x| x.as_f64()).map(|x| x as f32).collect();
                let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
                if n > 0.0 { Ok(v.iter().map(|x| x / n).collect()) } else { Err("an empty vector".to_string()) }
            })
            .collect()
    }
}

/// The embedding models a server has: Ollama's pulled models, or an OpenAI server's list.
pub fn server_models(openai: bool, url: &str) -> Result<Vec<String>, String> {
    let url = if url.is_empty() { OLLAMA } else { url.trim_end_matches('/') };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(5))).build().into();
    let path = if openai { "/models" } else { "/api/tags" };
    let text = agent.get(&format!("{url}{path}")).call().map_err(|e| format!("{url}: {e}"))?.body_mut().read_to_string().map_err(|e| e.to_string())?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let names = if openai { v["data"].as_array().map(|d| d.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect()) } else { v["models"].as_array().map(|d| d.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect()) };
    Ok(names.unwrap_or_default())
}

/// Whether Ollama answers on this machine, asked quickly.
pub fn ollama_here() -> bool {
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_millis(400))).build().into();
    agent.get(&format!("{OLLAMA}/api/version")).call().is_ok()
}

/// Ask Ollama to pull a model, with its progress in `p`.
pub fn ollama_pull(url: &str, model: &str, p: &Progress) -> io::Result<()> {
    let url = if url.is_empty() { OLLAMA } else { url.trim_end_matches('/') };
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_connect(Some(Duration::from_secs(10))).build().into();
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

/// The passages of a text that get a vector: about `WORDS` words each, `PASSAGES` at most.
pub fn passages(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.chunks(WORDS).take(PASSAGES).map(|c| c.join(" ")).filter(|p| p.chars().filter(|c| c.is_alphabetic()).count() >= 20).collect()
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

        let text = "word ".repeat(1000);
        assert_eq!(passages(&text).len(), PASSAGES);
        assert_eq!(passages("too short to mean much").len(), 0);
        assert!(size() > 400_000_000);
    }

    /// With the model downloaded: a question finds the passage about it, across languages.
    #[test]
    fn meaning_finds_passages_by_meaning() {
        let Some(e) = Embedder::load() else { return };
        let q = e.query("how much fuel does the rocket need").unwrap();
        let score = |p: &str| score(&pack(&e.passage(p).unwrap()), &q);
        let (fuel, danish, apples) = (score("The fuel budget for flight seven is the largest cost of the launch."), score("Brændstofbudgettet for flyvning syv er den største udgift ved opsendelsen."), score("Opskrift på æblekage med kanel og vaniljesauce."));
        assert!(fuel > apples && danish > apples, "{fuel} {danish} {apples}");
    }
}
