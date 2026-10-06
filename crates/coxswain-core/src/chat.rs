//! Ask without a server: a small chat model (Qwen3, Apache 2.0) run with candle, in pure Rust,
//! on a Mac's GPU through Metal or on the CPU. It answers from the passages Ask found, as a
//! server's model does, word by word.
//!
//! Like the built-in embedding model it is not shipped: it is downloaded once the user picks
//! it, from Hugging Face at a pinned revision, every file checked against its SHA-256. It is
//! loaded on the first question and let go after `IDLE` without one, so its memory is freed.

use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use candle_core::quantized::gguf_file;
use candle_core::{DType, Device, Tensor};
use candle_transformers::generation::{LogitsProcessor, Sampling};
use candle_transformers::models::quantized_qwen3::ModelWeights;

use crate::meaning::{Pinned, Progress, Turn};

/// What `ask_model` says for a built-in model: `builtin:` and its id.
pub const PREFIX: &str = "builtin:";

/// A built-in chat model.
pub struct Model {
    /// As `ask_model` names it, after `PREFIX`.
    pub id: &'static str,
    /// As the user sees it.
    pub name: &'static str,
    /// Memory the machine should have for it, in GB.
    pub ram: u64,
    /// A hybrid model that thinks unless its answer starts with an empty `<think>` block.
    hybrid: bool,
    files: &'static [Pinned],
}

/// Qwen3's tokenizer, the same for both models.
const TOKENIZER: Pinned = Pinned { repo: "Qwen/Qwen3-1.7B", revision: "70d244cc86ccca08cf5af4e1e306ecf908b1ad5e", name: "tokenizer.json", sha: "aeb13307a71acd8fe81861d94ad54ab689df773318809eed3cbe794b4492dae4", len: 11_422_654 };

/// The small one for any machine, and a better one for a machine with memory to spare.
pub const MODELS: &[Model] = &[
    Model {
        id: "qwen3-1.7b",
        name: "Qwen3 1.7B",
        ram: 8,
        hybrid: true,
        files: &[Pinned { repo: "unsloth/Qwen3-1.7B-GGUF", revision: "d7f544eead698dbd1f15126ef60b45a1e1933222", name: "Qwen3-1.7B-Q4_K_M.gguf", sha: "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897", len: 1_107_409_472 }, TOKENIZER],
    },
    Model {
        id: "qwen3-4b",
        name: "Qwen3 4B Instruct",
        ram: 16,
        hybrid: false,
        files: &[Pinned { repo: "unsloth/Qwen3-4B-Instruct-2507-GGUF", revision: "a06e946bb6b655725eafa393f4a9745d460374c9", name: "Qwen3-4B-Instruct-2507-Q4_K_M.gguf", sha: "3605803b982cb64aead44f6c1b2ae36e3acdb41d8e46c8a94c6533bc4c67e597", len: 2_497_281_120 }, TOKENIZER],
    },
];

/// The built-in model `ask_model` names, if it names one.
pub fn of(ask_model: &str) -> Option<&'static Model> {
    let id = ask_model.strip_prefix(PREFIX)?;
    MODELS.iter().find(|m| m.id == id)
}

/// The model for a machine with `ram` GB: the larger one on a Mac's GPU with the memory for
/// it; the small one on a CPU, where the larger takes three times as long.
pub fn suggest(ram: u64) -> &'static Model {
    let gpu = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    MODELS.iter().rev().find(|m| ram >= m.ram && (gpu || m.ram <= MODELS[0].ram)).unwrap_or(&MODELS[0])
}

/// How `ask_model` is shown: a built-in model by its name, a server's as it is.
pub fn shown(ask_model: &str) -> String {
    of(ask_model).map_or_else(|| ask_model.to_string(), |m| crate::t!("ask.builtin_name", "model" => m.name))
}

impl Model {
    /// `ask_model` for it.
    pub fn key(&self) -> String {
        format!("{PREFIX}{}", self.id)
    }

    /// Where it is kept: next to the embedding model.
    pub fn folder(&self) -> Option<PathBuf> {
        Some(crate::helper::folder()?.join("models").join(format!("{}-{}", self.id, &self.files[0].revision[..8])))
    }

    /// Bytes the download takes.
    pub fn size(&self) -> u64 {
        self.files.iter().map(|f| f.len).sum()
    }

    pub fn installed(&self) -> bool {
        self.folder().is_some_and(|d| self.files.iter().all(|f| std::fs::metadata(d.join(f.name)).is_ok_and(|m| m.len() == f.len)))
    }

    pub fn download(&self, p: &Progress) -> io::Result<()> {
        crate::meaning::fetch(&self.folder().ok_or_else(|| io::Error::other("no cache folder"))?, self.files, p)
    }

    pub fn remove(&self) -> io::Result<()> {
        unload();
        match self.folder().map(std::fs::remove_dir_all) {
            Some(Err(e)) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }
}

/// Tokens the model sees at most, answer included: on a GPU the rules, ten passages and a few
/// turns. Qwen3 takes 32,768, but each token holds memory. candle's CPU reads a prompt about
/// as fast as it writes, 10 to 40 tokens a second, so there the prompt gets about a thousand
/// tokens: the rules and the closest four or five passages.
// ponytail: fixed sizes; a quicker CPU prefill (a matrix kernel for many rows) would let the CPU have more.
const CONTEXT: usize = 8192;
const CPU_CONTEXT: usize = 2048;
/// Tokens an answer takes at most.
const ANSWER: usize = 1024;
/// Tokens the prompt goes through the model at a time: Stop is heard between them.
const CHUNK: usize = 256;
/// A model not asked for this long is let go.
const IDLE: Duration = Duration::from_secs(300);

/// The prompt in Qwen's chat format, the system turn holding the rules and the sources, then
/// the turns before and the question, as long as it fits `budget` tokens by `count`: the
/// oldest turns go first, then the last sources.
fn prompt(hybrid: bool, earlier: &[Turn], question: &str, sources: &[(PathBuf, String)], budget: usize, count: impl Fn(&str) -> usize) -> String {
    let build = |earlier: &[Turn], sources: &[(PathBuf, String)]| {
        let context: String = sources.iter().enumerate().map(|(i, (path, text))| format!("[{}] {}\n{text}\n\n", i + 1, path.display())).collect();
        let mut p = format!("<|im_start|>system\n{}\n\nSources:\n\n{context}<|im_end|>\n", crate::meaning::RULES);
        for (q, a) in earlier {
            p.push_str(&format!("<|im_start|>user\n{q}<|im_end|>\n<|im_start|>assistant\n{a}<|im_end|>\n"));
        }
        p.push_str(&format!("<|im_start|>user\n{question}<|im_end|>\n<|im_start|>assistant\n"));
        // Qwen3's own template turns thinking off this way.
        if hybrid {
            p.push_str("<think>\n\n</think>\n\n");
        }
        p
    };
    let (mut turns, mut kept) = (earlier, sources.len());
    loop {
        let p = build(turns, &sources[..kept]);
        if count(&p) <= budget || (turns.is_empty() && kept <= 1) {
            return p;
        }
        if turns.is_empty() {
            kept -= 1;
        } else {
            turns = &turns[1..];
        }
    }
}

/// The text of `ids` not yet given out, once it ends in a whole character: a character can
/// take more than one token.
fn fresh(text: &str, given: &mut usize) -> Option<String> {
    if text.len() <= *given || text.ends_with('\u{fffd}') || !text.is_char_boundary(*given) {
        return None;
    }
    let out = text[*given..].to_string();
    *given = text.len();
    Some(out)
}

/// The model, loaded on one device, with its tokenizer.
struct Loaded {
    id: &'static str,
    weights: ModelWeights,
    tokenizer: tokenizers::Tokenizer,
    device: Device,
    used: Instant,
}

static LOADED: Mutex<Option<Loaded>> = Mutex::new(None);

/// The model slot; a question that panicked half-way leaves it empty, to be loaded afresh.
fn slot() -> std::sync::MutexGuard<'static, Option<Loaded>> {
    LOADED.lock().unwrap_or_else(|e| {
        LOADED.clear_poison();
        let mut s = e.into_inner();
        *s = None;
        s
    })
}

fn load(m: &'static Model, device: Device) -> Result<Loaded, String> {
    let dir = m.folder().filter(|_| m.installed()).ok_or_else(|| crate::t!("ask.builtin_missing", "model" => m.name))?;
    let mut file = std::fs::File::open(dir.join(m.files[0].name)).map_err(|e| e.to_string())?;
    let content = gguf_file::Content::read(&mut file).map_err(|e| e.to_string())?;
    let weights = ModelWeights::from_gguf(content, &mut file, &device).map_err(|e| e.to_string())?;
    let tokenizer = tokenizers::Tokenizer::from_file(dir.join(TOKENIZER.name)).map_err(|e| e.to_string())?;
    Ok(Loaded { id: m.id, weights, tokenizer, device, used: Instant::now() })
}

/// The device the model runs on: a Mac's GPU unless `cpu_only`, else the CPU.
fn device(cpu_only: bool) -> Device {
    if cfg!(target_os = "macos") && !cpu_only {
        Device::new_metal(0).unwrap_or(Device::Cpu)
    } else {
        Device::Cpu
    }
}

/// Free the model's memory now.
pub fn unload() {
    *slot() = None;
}

/// Ask the built-in model `m`: `question` answered from `sources` (numbered in their order)
/// and the turns before, each piece of the answer to `piece` as it comes, with empty ones
/// between while the prompt is read; `piece` returns false to stop. One question at a time.
pub fn ask(m: &'static Model, cpu_only: bool, earlier: &[Turn], question: &str, sources: &[(PathBuf, String)], mut piece: impl FnMut(&str) -> bool) -> Result<(), String> {
    let mut slot = slot();
    if slot.as_ref().is_none_or(|l| l.id != m.id) {
        let first = slot.is_none();
        *slot = None;
        if !piece("") {
            return Ok(());
        }
        *slot = Some(load(m, device(cpu_only)).or_else(|e| if cpu_only { Err(e) } else { load(m, Device::Cpu) })?);
        if first {
            let _ = std::thread::Builder::new().name("coxswain-chat-idle".into()).spawn(let_go);
        }
    }
    let loaded = slot.as_mut().unwrap();
    let mut said = false;
    let done = answer(loaded, m.hybrid, earlier, question, sources, &mut |t: &str| {
        said |= !t.is_empty();
        piece(t)
    });
    // A GPU that fails before a word was said: the CPU tries.
    let done = match done {
        Err(_) if !said && !loaded.device.is_cpu() => {
            *loaded = load(m, Device::Cpu)?;
            answer(loaded, m.hybrid, earlier, question, sources, &mut piece)
        }
        d => d,
    };
    loaded.used = Instant::now();
    done
}

/// Lets the model go once it has not been asked for `IDLE`; ends with it.
fn let_go() {
    loop {
        std::thread::sleep(Duration::from_secs(30));
        let mut slot = slot();
        if slot.as_ref().is_none_or(|l| l.used.elapsed() > IDLE) {
            *slot = None;
            return;
        }
    }
}

fn answer(l: &mut Loaded, hybrid: bool, earlier: &[Turn], question: &str, sources: &[(PathBuf, String)], piece: &mut dyn FnMut(&str) -> bool) -> Result<(), String> {
    let err = |e: candle_core::Error| e.to_string();
    let tok = &l.tokenizer;
    let count = |s: &str| tok.encode(s, false).map_or(usize::MAX, |e| e.len());
    let text = prompt(hybrid, earlier, question, sources, if l.device.is_cpu() { CPU_CONTEXT } else { CONTEXT } - ANSWER, count);
    let ids = tok.encode(text, false).map_err(|e| e.to_string())?.get_ids().to_vec();
    let ends: Vec<u32> = ["<|im_end|>", "<|endoftext|>"].iter().filter_map(|t| tok.token_to_id(t)).collect();
    l.weights.clear_kv_cache();
    let mut logits = None;
    for (i, chunk) in ids.chunks(CHUNK).enumerate() {
        if !piece("") {
            return Ok(());
        }
        let input = Tensor::new(chunk, &l.device).and_then(|t| t.unsqueeze(0)).map_err(err)?;
        logits = Some(l.weights.forward(&input, i * CHUNK).map_err(err)?);
    }
    // Qwen's advice for answers without thinking, less random: the sources are the facts.
    let mut sampler = LogitsProcessor::from_sampling(ids.len() as u64, Sampling::TopKThenTopP { k: 20, p: 0.8, temperature: 0.5 });
    let (mut out, mut given, mut thinking) = (Vec::<u32>::new(), 0, false);
    let mut logits = logits.ok_or("an empty question")?;
    for n in 0..ANSWER {
        let last = logits.squeeze(0).and_then(|t| t.to_dtype(DType::F32)).map_err(err)?;
        let from = out.len().saturating_sub(64);
        let last = candle_transformers::utils::apply_repeat_penalty(&last, 1.05, &out[from..]).map_err(err)?;
        let next = sampler.sample(&last).map_err(err)?;
        if ends.contains(&next) {
            break;
        }
        out.push(next);
        let text = tok.decode(&out, true).map_err(|e| e.to_string())?;
        let new = fresh(&text, &mut given).map(|t| crate::meaning::unthink(&t, &mut thinking)).unwrap_or_default();
        if !piece(&new) {
            return Ok(());
        }
        let input = Tensor::new(&[next], &l.device).and_then(|t| t.unsqueeze(0)).map_err(err)?;
        logits = l.weights.forward(&input, ids.len() + n).map_err(err)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_prompt_is_chatml_and_fits() {
        let sources = vec![(PathBuf::from("/a.md"), "one ".repeat(50)), (PathBuf::from("/b.md"), "two ".repeat(50))];
        let earlier = vec![("Old?".to_string(), "Old.".to_string())];
        let words = |s: &str| s.split_whitespace().count();
        let all = prompt(true, &earlier, "New?", &sources, 10_000, words);
        assert!(all.starts_with("<|im_start|>system\n"));
        assert!(all.contains("[1] /a.md\n") && all.contains("[2] /b.md\n"));
        assert!(all.contains("<|im_start|>user\nOld?<|im_end|>\n<|im_start|>assistant\nOld.<|im_end|>\n"));
        assert!(all.ends_with("<|im_start|>user\nNew?<|im_end|>\n<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        assert!(!prompt(false, &earlier, "New?", &sources, 10_000, words).contains("<think>"));
        // Too long: the turn before goes first, then the last source; the question stays.
        let short = prompt(true, &earlier, "New?", &sources, words(&all) - 1, words);
        assert!(!short.contains("Old?") && short.contains("[2] /b.md"));
        let shorter = prompt(true, &earlier, "New?", &sources, words(&all) - 40, words);
        assert!(shorter.contains("[1] /a.md") && !shorter.contains("[2] /b.md") && shorter.contains("New?"));
    }

    #[test]
    fn pieces_are_whole_characters() {
        let mut given = 0;
        assert_eq!(fresh("Hej", &mut given).as_deref(), Some("Hej"));
        assert_eq!(fresh("Hej", &mut given), None);
        assert_eq!(fresh("Hej s\u{fffd}", &mut given), None, "half a character waits");
        assert_eq!(fresh("Hej så", &mut given).as_deref(), Some(" så"));
    }

    #[test]
    fn models_by_name_and_memory() {
        assert_eq!(of("builtin:qwen3-1.7b").map(|m| m.id), Some("qwen3-1.7b"));
        assert!(of("qwen3:8b").is_none() && of("builtin:other").is_none());
        assert_eq!(suggest(8).id, "qwen3-1.7b");
        assert_eq!(suggest(64).id, if cfg!(all(target_os = "macos", target_arch = "aarch64")) { "qwen3-4b" } else { "qwen3-1.7b" });
        assert_eq!(of(&MODELS[1].key()).map(|m| m.id), Some("qwen3-4b"));
    }

    /// Downloads the small model (1.1 GB) and asks it about two files.
    #[test]
    #[ignore]
    fn it_answers_from_the_sources() {
        let m = &MODELS[std::env::var("COXSWAIN_CHAT_MODEL").ok().and_then(|i| i.parse().ok()).unwrap_or(0)];
        m.download(&Progress::default()).unwrap();
        let sources = vec![
            (PathBuf::from("/home/demo/rocket/plan.md"), "# Launch plan\nThe rocket is called Kestrel. It launches from Andøya on 14 March, weather permitting.".to_string()),
            (PathBuf::from("/home/demo/rocket/budget.md"), "# Budget\nFuel costs 40,000 euros per flight, more than the crew and the launch pad together.".to_string()),
        ];
        let (mut answer, mut first, start) = (String::new(), None, Instant::now());
        ask(m, true, &[], "What does the fuel cost, and what is the rocket called?", &sources, |t| {
            if !t.is_empty() && first.is_none() {
                first = Some(start.elapsed());
            }
            answer.push_str(t);
            true
        })
        .unwrap();
        let words = start.elapsed() - first.unwrap();
        let tokens = slot().as_ref().map(|l| l.tokenizer.encode(answer.as_str(), false).unwrap().len()).unwrap();
        eprintln!("{answer}\n-- first word after {:.1?}, then {:.1} tokens/s", first.unwrap(), tokens as f64 / words.as_secs_f64());
        assert!(answer.contains("Kestrel") && answer.contains("40"), "{answer}");
    }
}
