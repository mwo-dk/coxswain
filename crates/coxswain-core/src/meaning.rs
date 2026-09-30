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
/// Numbers per vector.
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

/// The passages of a text that get a vector: about `WORDS` words each, `PASSAGES` at most.
pub fn passages(text: &str) -> Vec<String> {
    let words: Vec<&str> = text.split_whitespace().collect();
    words.chunks(WORDS).take(PASSAGES).map(|c| c.join(" ")).filter(|p| p.chars().filter(|c| c.is_alphabetic()).count() >= 20).collect()
}

/// A vector in 388 bytes: its numbers as signed bytes of its largest one, then that one.
pub fn pack(v: &[f32]) -> Vec<u8> {
    let scale = v.iter().fold(0f32, |m, x| m.max(x.abs())).max(f32::MIN_POSITIVE);
    let mut out: Vec<u8> = v.iter().map(|x| (x / scale * 127.0).round().clamp(-127.0, 127.0) as i8 as u8).collect();
    out.extend(scale.to_le_bytes());
    out
}

/// How much a packed vector and a question's vector point the same way (1: the same).
pub fn score(packed: &[u8], q: &[f32]) -> f32 {
    if packed.len() != DIMS + 4 || q.len() != DIMS {
        return 0.0;
    }
    let scale = f32::from_le_bytes([packed[DIMS], packed[DIMS + 1], packed[DIMS + 2], packed[DIMS + 3]]) / 127.0;
    packed[..DIMS].iter().zip(q).map(|(b, x)| (*b as i8) as f32 * x).sum::<f32>() * scale
}

/// The signs of a vector, one bit each: a first sieve over many passages.
pub fn signs(packed: &[u8]) -> [u64; DIMS / 64] {
    let mut out = [0u64; DIMS / 64];
    for (i, b) in packed.iter().take(DIMS).enumerate() {
        if (*b as i8) > 0 {
            out[i / 64] |= 1 << (i % 64);
        }
    }
    out
}

/// Bits two sign sets share: the more, the closer.
pub fn alike(a: &[u64; DIMS / 64], b: &[u64; DIMS / 64]) -> u32 {
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
