//! The guided setup of search by meaning and Ask, for both apps: which model servers answer on
//! this machine and what each of their models can do (make vectors, answer), what the machine
//! has to run them on, what suits it best, and whether a server really uses the graphics card.
//! Nothing is downloaded here unless `pull` is called, and nothing leaves the machine but the
//! requests to the servers asked about.

use crate::meaning::{ollama_can, said, shown, tls};
use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// A kind of model server; they are equals here, each with its own way of saying what it has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Ollama,
    Lemonade,
    LmStudio,
    LlamaCpp,
    Jan,
    LocalAi,
    Other,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Ollama => "Ollama",
            Kind::Lemonade => "Lemonade",
            Kind::LmStudio => "LM Studio",
            Kind::LlamaCpp => "llama.cpp",
            Kind::Jan => "Jan",
            Kind::LocalAi => "LocalAI",
            Kind::Other => "OpenAI API",
        }
    }
}

/// A model on a server, and what it can do.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    /// It makes vectors (an embedding model).
    pub embed: bool,
    /// It answers (a chat model).
    pub chat: bool,
    /// Billions of weights, as the server says (Ollama); else `size` reads them from the name.
    #[serde(default)]
    pub params: Option<f64>,
}

/// A server that answered.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Found {
    pub kind: Kind,
    /// The address as Settings takes it: Ollama's own, or the OpenAI API's base.
    pub url: String,
    /// `meaning_engine` for it: "ollama" or "openai".
    pub engine: String,
    pub models: Vec<Model>,
}

/// Where the servers listen unless told otherwise. Lemonade moved from 8000 to 13305; llama.cpp's
/// server and LocalAI share 8080.
const PLACES: [(Kind, &str); 7] = [
    (Kind::Ollama, "http://localhost:11434"),
    (Kind::Lemonade, "http://localhost:13305/api/v1"),
    (Kind::Lemonade, "http://localhost:8000/api/v1"),
    (Kind::LmStudio, "http://localhost:1234/v1"),
    (Kind::Jan, "http://localhost:1337/v1"),
    (Kind::LlamaCpp, "http://localhost:8080/v1"),
    (Kind::Other, "http://localhost:8000/v1"),
];

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder().tls_config(tls()).timeout_global(Some(timeout)).http_status_as_error(false).build().into()
}

fn get(agent: &ureq::Agent, url: &str, key: Option<&str>) -> Option<serde_json::Value> {
    let mut req = agent.get(url);
    if let Some(key) = key {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    let mut res = req.call().ok()?;
    if res.status().as_u16() >= 400 {
        return None;
    }
    serde_json::from_str(&res.body_mut().read_to_string().ok()?).ok()
}

/// The servers on this machine, asked all at once and quickly.
pub fn probe() -> Vec<Found> {
    probe_at(&PLACES.map(|(k, u)| (k, u.to_string())), None)
}

/// The servers at `places`, asked at once; one address is listed once, by the first kind that
/// answers there.
pub fn probe_at(places: &[(Kind, String)], key: Option<&str>) -> Vec<Found> {
    let found: Vec<Option<Found>> = std::thread::scope(|s| {
        let asked: Vec<_> = places.iter().map(|(kind, url)| s.spawn(move || ask_server(*kind, url, key))).collect();
        asked.into_iter().map(|h| h.join().ok().flatten()).collect()
    });
    let mut out: Vec<Found> = vec![];
    for f in found.into_iter().flatten() {
        let host = |u: &str| u.split('/').take(3).collect::<Vec<_>>().join("/");
        if !out.iter().any(|o| host(&o.url) == host(&f.url)) {
            out.push(f);
        }
    }
    out
}

/// A server someone named (one on another machine): what answers there, of any kind.
pub fn probe_url(url: &str, key: Option<&str>) -> Option<Found> {
    let url = url.trim().trim_end_matches('/');
    let base = url.trim_end_matches("/v1").trim_end_matches("/api");
    let places = [(Kind::Ollama, base.to_string()), (Kind::Lemonade, format!("{base}/api/v1")), (Kind::LmStudio, format!("{base}/v1")), (Kind::Other, url.to_string())];
    probe_at(&places, key).into_iter().next()
}

/// What answers at `url` as a server of `kind`, with its models and what they can do.
fn ask_server(kind: Kind, url: &str, key: Option<&str>) -> Option<Found> {
    let quick = agent(Duration::from_millis(800));
    let slow = agent(Duration::from_secs(30));
    let url = url.trim_end_matches('/');
    let openai = |kind, models| Some(Found { kind, url: url.to_string(), engine: "openai".into(), models });
    match kind {
        Kind::Ollama => {
            let tags = get(&quick, &format!("{url}/api/tags"), key)?;
            let models = tags["models"].as_array()?.iter().filter_map(|m| {
                let name = m["name"].as_str()?.to_string();
                // Newer Ollamas say it in the list; older ones in `/api/show`.
                let can: Option<Vec<String>> = m["capabilities"].as_array().map(|c| c.iter().filter_map(|x| x.as_str().map(String::from)).collect()).or_else(|| ollama_can(&slow, url, &name).ok().flatten());
                let params = billions(m["details"]["parameter_size"].as_str().unwrap_or(""));
                Some(Model { params, ..match can {
                    Some(c) => Model { embed: c.iter().any(|x| x == "embedding"), chat: c.iter().any(|x| x == "completion"), name, params: None },
                    None => guess(name),
                } })
            });
            Some(Found { kind, url: url.to_string(), engine: "ollama".into(), models: models.collect() })
        }
        Kind::Lemonade => {
            let v = get(&quick, &format!("{url}/models"), key)?;
            // Only Lemonade's list has `recipe`; another server on the port is not Lemonade.
            let data = v["data"].as_array()?;
            if !data.iter().any(|m| m.get("recipe").is_some()) && !data.is_empty() {
                return None;
            }
            let models = data.iter().filter(|m| m["downloaded"].as_bool() != Some(false)).filter_map(|m| {
                let labels: Vec<&str> = m["labels"].as_array().map(|l| l.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                let embed = labels.contains(&"embeddings");
                Some(Model { name: m["id"].as_str()?.to_string(), embed, chat: !embed && !labels.contains(&"reranking"), params: None })
            });
            openai(kind, models.collect())
        }
        Kind::LmStudio => {
            // LM Studio's own API says what each model is: `llm`, `vlm` or `embeddings`.
            let base = url.trim_end_matches("/v1");
            let v = get(&quick, &format!("{base}/api/v0/models"), key)?;
            let models = v["data"].as_array()?.iter().filter_map(|m| {
                let t = m["type"].as_str().unwrap_or("");
                Some(Model { name: m["id"].as_str()?.to_string(), embed: t == "embeddings", chat: t == "llm" || t == "vlm", params: None })
            });
            openai(kind, models.collect())
        }
        Kind::LlamaCpp | Kind::Jan | Kind::LocalAi | Kind::Other => {
            let v = get(&quick, &format!("{url}/models"), key)?;
            let names: Vec<String> = v["data"].as_array()?.iter().filter_map(|m| m["id"].as_str().map(String::from)).collect();
            // llama.cpp's server has `/props`; on 8080 without it, it is LocalAI.
            let base = url.trim_end_matches("/v1");
            let kind = if kind == Kind::LlamaCpp && get(&quick, &format!("{base}/props"), key).is_none() { Kind::LocalAi } else { kind };
            // These do not say what a model can do: a word to each, both ways, at once.
            let models = std::thread::scope(|s| {
                let tried: Vec<_> = names.iter().map(|n| { let slow = &slow; s.spawn(move || Model { name: n.clone(), embed: tries(slow, url, key, "/embeddings", serde_json::json!({ "model": n, "input": "test" })), chat: tries(slow, url, key, "/chat/completions", serde_json::json!({ "model": n, "messages": [{ "role": "user", "content": "Hi" }], "max_tokens": 1 })), params: None }) }).collect();
                tried.into_iter().filter_map(|h| h.join().ok()).collect()
            });
            openai(kind, models)
        }
    }
}

/// Whether the server takes `body` at `path` for the model.
fn tries(agent: &ureq::Agent, url: &str, key: Option<&str>, path: &str, body: serde_json::Value) -> bool {
    let mut req = agent.post(&format!("{url}{path}")).header("Content-Type", "application/json");
    if let Some(key) = key {
        req = req.header("Authorization", &format!("Bearer {key}"));
    }
    req.send(body.to_string()).is_ok_and(|r| r.status().as_u16() < 400)
}

/// What a model can do by its name, when the server does not say.
fn guess(name: String) -> Model {
    let n = name.to_lowercase();
    let embed = ["embed", "bge", "e5", "minilm", "gte", "arctic-embed"].iter().any(|w| n.contains(w));
    Model { embed, chat: !embed, name, params: None }
}

/// Billions of weights in "8.2B", "567M" (Ollama's `parameter_size`) or a name's "14b".
fn billions(s: &str) -> Option<f64> {
    let s = s.trim().to_lowercase();
    let (n, scale) = s.strip_suffix('b').map(|n| (n, 1.0)).or_else(|| s.strip_suffix('m').map(|n| (n, 1e-3)))?;
    n.parse::<f64>().ok().filter(|n| *n > 0.0).map(|n| n * scale)
}

/// A chat model's size from its name: its billions of weights, and of those each word goes
/// through when it is a mixture of experts ("qwen3:30b-a3b", "Qwen3-30B-A3B-MLX-4bit").
fn size(name: &str) -> (Option<f64>, Option<f64>) {
    let n = name.to_lowercase();
    let words: Vec<&str> = n.split(|c: char| !c.is_ascii_alphanumeric() && c != '.').collect();
    let total = words.iter().find(|w| w.ends_with('b') && w.starts_with(|c: char| c.is_ascii_digit())).and_then(|w| billions(w));
    let active = words.iter().find_map(|w| w.strip_prefix('a').filter(|w| w.starts_with(|c: char| c.is_ascii_digit())).and_then(billions));
    (total, active)
}

// ---------------------------------------------------------------- the machine

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Vendor {
    Nvidia,
    Amd,
    Apple,
}

/// What the machine has to run models on.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Machine {
    /// The graphics card (or Apple's GPU), its name, and its own memory in GB when it has some.
    pub gpu: Option<(Vendor, String, Option<u64>)>,
    /// An AMD Ryzen AI NPU.
    pub npu: bool,
    /// Memory in GB.
    pub ram: u64,
}

/// The machine as it is; `found` lets Lemonade, which knows AMD's hardware best, say more.
pub fn machine(found: &[Found]) -> Machine {
    let mut m = Machine { ram: ram_gb(), ..Machine::default() };
    if let Ok(out) = crate::tools::command("nvidia-smi").args(["--query-gpu=name,memory.total", "--format=csv,noheader,nounits"]).output() {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some((name, mb)) = text.lines().next().and_then(|l| l.split_once(',')) {
            m.gpu = Some((Vendor::Nvidia, name.trim().to_string(), mb.trim().parse::<u64>().ok().map(|mb| mb.div_ceil(1024))));
        }
    }
    if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
        m.gpu = Some((Vendor::Apple, "Apple silicon".into(), None));
    }
    #[cfg(target_os = "linux")]
    if m.gpu.is_none() {
        m.gpu = amd_on_linux();
        m.npu = std::path::Path::new("/dev/accel/accel0").exists();
    }
    // Lemonade's own report of AMD's GPU and NPU, on any system.
    if let Some(l) = found.iter().find(|f| f.kind == Kind::Lemonade)
        && let Some(d) = get(&agent(Duration::from_secs(3)), &format!("{}/system-info", l.url), None).map(|v| v["devices"].clone())
    {
        m.npu |= d["amd_npu"]["available"].as_bool() == Some(true);
        if m.gpu.is_none()
            && let Some(g) = d["amd_gpu"].as_array().and_then(|g| g.iter().find(|g| g["available"].as_bool() == Some(true)))
        {
            m.gpu = Some((Vendor::Amd, g["name"].as_str().unwrap_or("AMD Radeon").to_string(), g["vram_gb"].as_f64().map(|v| v.round() as u64)));
        }
    }
    m
}

/// An AMD graphics card or the Radeon in a Ryzen, from the kernel's view of it.
#[cfg(target_os = "linux")]
fn amd_on_linux() -> Option<(Vendor, String, Option<u64>)> {
    let cards = std::fs::read_dir("/sys/class/drm").ok()?;
    let dev = cards.flatten().map(|e| e.path().join("device")).find(|d| std::fs::read_to_string(d.join("vendor")).is_ok_and(|v| v.trim() == "0x1002"))?;
    let vram = std::fs::read_to_string(dev.join("mem_info_vram_total")).ok().and_then(|b| b.trim().parse::<u64>().ok()).map(|b| b.div_ceil(1 << 30));
    let name = std::fs::read_to_string(dev.join("product_name")).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).unwrap_or_else(|| "AMD Radeon".into());
    Some((Vendor::Amd, name, vram))
}

pub fn ram_gb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        info.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok()).map_or(0, |kb| kb.div_ceil(1 << 20))
    }
    #[cfg(any(target_os = "macos", target_os = "freebsd"))]
    {
        let key = if cfg!(target_os = "macos") { "hw.memsize" } else { "hw.physmem" };
        crate::tools::command("sysctl").args(["-n", key]).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok()).map_or(0, |b| b.div_ceil(1 << 30))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut s) } != 0 { s.ullTotalPhys.div_ceil(1 << 30) } else { 0 }
    }
    // NetBSD, OpenBSD and illumos: pages times their size.
    #[cfg(all(unix, not(any(target_os = "linux", target_os = "macos", target_os = "freebsd"))))]
    {
        // SAFETY: sysconf only reads.
        let (pages, size) = unsafe { (libc::sysconf(libc::_SC_PHYS_PAGES), libc::sysconf(libc::_SC_PAGESIZE)) };
        if pages > 0 && size > 0 { (pages as u64 * size as u64).div_ceil(1 << 30) } else { 0 }
    }
}

// ---------------------------------------------------------------- advice

/// What suits the machine: a server, the model for the vectors (empty: the built-in one), the
/// chat model for Ask, and why, in one line.
#[derive(Debug, Clone, Serialize)]
pub struct Advice {
    pub server: Option<Kind>,
    pub embed: String,
    pub chat: String,
    pub why: String,
}

/// The chat model whose size fits `gb` of memory for the model.
fn chat_for(gb: u64) -> &'static str {
    match gb {
        0..=7 => "qwen3:4b",
        8..=15 => "qwen3:8b",
        _ => "qwen3:14b",
    }
}

/// What suits `m`, with the servers `found` on it. A Mac with no server is told the built-in
/// model, which runs on its GPU: nothing to install, nothing leaves the machine.
pub fn advise(m: &Machine, found: &[Found]) -> Advice {
    let ram = m.ram.to_string();
    match &m.gpu {
        Some((Vendor::Apple, _, _)) if found.is_empty() => {
            Advice { server: None, embed: String::new(), chat: chat_for(m.ram / 2).into(), why: crate::t!("setup.why_apple_builtin", "ram" => ram) }
        }
        Some((Vendor::Nvidia, name, vram)) => {
            let chat = chat_for(vram.unwrap_or(0));
            let gb = vram.map(|v| v.to_string()).unwrap_or_else(|| "?".into());
            Advice { server: Some(Kind::Ollama), embed: "bge-m3".into(), chat: chat.into(), why: crate::t!("setup.why_nvidia", "gpu" => name, "vram" => gb, "chat" => chat) }
        }
        Some((Vendor::Amd, name, _)) => {
            // An APU shares the system's memory; Lemonade gives models up to about half of it.
            let chat = if m.ram >= 32 || m.npu { "Qwen3-8B-GGUF" } else { "Qwen3-4B-GGUF" };
            let what = if m.npu { format!("{name} + NPU") } else { name.clone() };
            Advice { server: Some(Kind::Lemonade), embed: "nomic-embed-text-v1-GGUF".into(), chat: chat.into(), why: crate::t!("setup.why_amd", "gpu" => what, "chat" => chat) }
        }
        Some((Vendor::Apple, _, _)) => {
            let chat = chat_for(m.ram / 2);
            Advice { server: Some(Kind::Ollama), embed: "bge-m3".into(), chat: chat.into(), why: crate::t!("setup.why_apple", "ram" => ram, "chat" => chat) }
        }
        None => Advice { server: None, embed: String::new(), chat: "qwen3:4b".into(), why: crate::t!("setup.why_cpu", "ram" => ram) },
    }
}

/// Where the built-in model runs, in words: where it runs now (`now`, while it makes the
/// vectors), else where it will: a Mac's GPU when it can, the CPU elsewhere.
pub fn builtin_runs(cfg: &crate::config::SearchConfig, now: Option<&crate::meaning::Runs>) -> String {
    match now {
        Some(runs) => runs.text(),
        None if cfg!(target_os = "macos") && cfg.meaning_device != "cpu" => crate::t!("meaning.on_metal_if"),
        None => crate::t!("meaning.on_cpu"),
    }
}

/// The server of `found` to recommend: the one that suits the machine, else the one with the
/// most models that can do both jobs.
pub fn best<'a>(found: &'a [Found], advice: &Advice) -> Option<&'a Found> {
    let both = |f: &Found| f.models.iter().filter(|m| m.embed).count().min(1) + f.models.iter().filter(|m| m.chat).count().min(1);
    found.iter().find(|f| Some(f.kind) == advice.server).or_else(|| found.iter().max_by_key(|f| both(f)))
}

// ---------------------------------------------------------------- the speed check

/// Whether the server runs its models on the processor while the machine has a graphics card
/// or an NPU, and how to change that; None when all is well or it cannot be told. `model` is
/// the chat model, loaded by the test question.
pub fn speed_problem(f: &Found, m: &Machine, model: &str) -> Option<String> {
    let gpu = match &m.gpu {
        Some((_, name, _)) => name.clone(),
        None if m.npu => "NPU".into(),
        None => return None,
    };
    let quick = agent(Duration::from_secs(3));
    match f.kind {
        Kind::Ollama => {
            let ps = get(&quick, &format!("{}/api/ps", f.url), None)?;
            let on_cpu = ps["models"].as_array()?.iter().find(|x| x["size_vram"].as_u64() == Some(0))?;
            let model = on_cpu["name"].as_str().unwrap_or(model);
            let package = match m.gpu {
                Some((Vendor::Nvidia, ..)) => "ollama-cuda",
                _ => "ollama-rocm",
            };
            Some(if cfg!(target_os = "linux") && std::path::Path::new("/etc/arch-release").exists() {
                crate::t!("setup.cpu_ollama_arch", "model" => model, "gpu" => gpu, "package" => package)
            } else {
                crate::t!("setup.cpu_ollama", "model" => model, "gpu" => gpu, "url" => "https://docs.ollama.com/gpu")
            })
        }
        Kind::Lemonade => {
            let health = get(&quick, &format!("{}/health", f.url), None)?;
            let loaded = health["all_models_loaded"].as_array()?;
            let on_cpu = loaded.iter().find(|x| x["device"].as_str().is_some_and(|d| d.eq_ignore_ascii_case("cpu")))?;
            Some(crate::t!("setup.cpu_lemonade", "model" => on_cpu["model_name"].as_str().unwrap_or(model), "gpu" => gpu))
        }
        Kind::LmStudio => Some(crate::t!("setup.cpu_lmstudio", "gpu" => gpu)),
        _ => None,
    }
}

// ---------------------------------------------------------------- the better choice

/// Whether `f` runs its models on a graphics card or an NPU: `Some(true)` when a model it has
/// loaded is there, `Some(false)` when one is on the processor or the machine has neither,
/// None when it has nothing loaded to tell by.
pub fn on_gpu(f: &Found, m: &Machine) -> Option<bool> {
    if m.gpu.is_none() && !m.npu {
        return Some(false);
    }
    let quick = agent(Duration::from_secs(2));
    let (loaded, on): (serde_json::Value, fn(&serde_json::Value) -> bool) = match f.kind {
        Kind::Ollama => (get(&quick, &format!("{}/api/ps", f.url), None)?["models"].clone(), |x: &serde_json::Value| x["size_vram"].as_u64().is_some_and(|v| v > 0)),
        Kind::Lemonade => (get(&quick, &format!("{}/health", f.url), None)?["all_models_loaded"].clone(), |x: &serde_json::Value| x["device"].as_str().is_some_and(|d| !d.eq_ignore_ascii_case("cpu"))),
        _ => return None,
    };
    let loaded = loaded.as_array()?;
    (!loaded.is_empty()).then(|| loaded.iter().any(on))
}

/// The servers on this machine, the machine, and where each server runs its models (`on_gpu`).
#[derive(Clone, Debug, Default)]
pub struct Look {
    pub found: Vec<Found>,
    pub machine: Machine,
    pub gpu: Vec<Option<bool>>,
}

impl Look {
    /// The servers whose models run on a graphics card, or can (a graphics card and nothing
    /// loaded to tell by), with whether that is known; the known ones first.
    pub fn gpu_servers(&self) -> Vec<(&Found, bool)> {
        let card = self.machine.gpu.is_some() || self.machine.npu;
        let mut v: Vec<(&Found, bool)> = self.found.iter().zip(&self.gpu).filter(|(_, g)| **g == Some(true) || (g.is_none() && card)).map(|(f, g)| (f, *g == Some(true))).collect();
        v.sort_by_key(|(_, sure)| !sure);
        v
    }

    /// The graphics card's name, or the NPU.
    fn card(&self) -> String {
        self.machine.gpu.as_ref().map_or_else(|| "NPU".into(), |(_, name, _)| name.clone())
    }
}

/// How long a look is kept: servers come and go, but asking takes a second or more.
const FRESH: Duration = Duration::from_secs(120);

/// What answers on this machine and where it runs its models, asked at most every `FRESH`.
/// `wait` false never waits: it asks in the background and gives what was kept, None the
/// first time. Nothing is asked but the servers on this machine.
pub fn look(wait: bool) -> Option<std::sync::Arc<Look>> {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    static KEPT: Mutex<Option<(Instant, Arc<Look>)>> = Mutex::new(None);
    static ASKING: AtomicBool = AtomicBool::new(false);
    let fresh = || {
        let found = probe();
        let machine = machine(&found);
        let gpu = found.iter().map(|f| on_gpu(f, &machine)).collect();
        // The processor's speed for the built-in models, measured once.
        let _ = crate::chat::estimate(&crate::chat::MODELS[0], true, true);
        let l = Arc::new(Look { found, machine, gpu });
        *KEPT.lock().unwrap_or_else(|e| e.into_inner()) = Some((Instant::now(), l.clone()));
        l
    };
    let kept = KEPT.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let again = LOOK_AGAIN.swap(false, Ordering::SeqCst);
    if let Some((at, l)) = &kept
        && at.elapsed() < FRESH
        && !again
    {
        return Some(l.clone());
    }
    if wait {
        return Some(fresh());
    }
    if !ASKING.swap(true, Ordering::SeqCst) {
        let _ = std::thread::Builder::new().name("coxswain-look".into()).spawn(move || {
            fresh();
            ASKING.store(false, Ordering::SeqCst);
        });
    }
    kept.map(|(_, l)| l)
}

/// Whether two addresses are the same server: the same host and port, `localhost` and
/// `127.0.0.1` alike.
fn same_server(a: &str, b: &str) -> bool {
    use crate::settings::{host, is_local};
    let port = |u: &str| host(u).rsplit_once(':').map(|(_, p)| p.to_string()).unwrap_or_default();
    host(a) == host(b) || (is_local(a) && is_local(b) && port(a) == port(b))
}

/// The servers of `look` on a graphics card that Ask can use as `cfg` stands: Ollama here with
/// the built-in model for the vectors, else the server that makes them.
pub fn ask_servers<'a>(cfg: &crate::config::SearchConfig, look: &'a Look) -> Vec<(&'a Found, bool)> {
    let url = crate::settings::server_url(cfg);
    look.gpu_servers().into_iter().filter(|(f, _)| same_server(&f.url, &url) && (cfg.meaning_engine != "builtin" || f.kind == Kind::Ollama)).collect()
}

/// The servers of `look` on a graphics card that can make the vectors: any, unless Ask uses a
/// server's model, which has to stay on the same server.
fn meaning_servers<'a>(cfg: &crate::config::SearchConfig, look: &'a Look) -> Vec<(&'a Found, bool)> {
    let server_ask = !cfg.ask_model.is_empty() && crate::chat::of(&cfg.ask_model).is_none();
    let url = crate::settings::server_url(cfg);
    look.gpu_servers().into_iter().filter(|(f, _)| !server_ask || same_server(&f.url, &url)).collect()
}

/// A model of `f` that answers (`chat`) or makes vectors: the one named like `hint` (the size
/// that suits the graphics card), else a Qwen or one of the known embedding models, else the first.
fn pick(f: &Found, chat: bool, hint: &str) -> Option<String> {
    let can: Vec<&str> = f.models.iter().filter(|m| if chat { m.chat } else { m.embed }).map(|m| m.name.as_str()).collect();
    let hint = hint.to_lowercase();
    let known: &[&str] = if chat { &["qwen"] } else { &["bge-m3", "nomic"] };
    let like = |w: &str| can.iter().find(|n| n.to_lowercase().starts_with(w)).or_else(|| can.iter().find(|n| n.to_lowercase().contains(w)));
    (!hint.is_empty()).then(|| like(&hint)).flatten().or_else(|| known.iter().find_map(|w| like(w))).or(can.first()).map(|n| n.to_string())
}

/// What beats a built-in model that was chosen.
pub enum Better<'a> {
    /// A server's model on the graphics card; `sure` when a model it has loaded is there.
    Server { found: &'a Found, model: String, sure: bool },
    /// A smaller built-in model that answers within `chat::QUICK` here, if there is one.
    Smaller(Option<&'static crate::chat::Model>),
    /// On a Mac's GPU: a server's model there too, larger than the built-in one, and `quicker`
    /// when it is a mixture of experts whose words go through fewer weights than the built-in one's.
    Metal { found: &'a Found, model: String, quicker: bool },
}

/// What beats the built-in chat model `m` on this machine, or None when it is a good choice.
/// A server's model on a graphics card, of `servers` (those Ask can use, the known ones
/// first); on a Mac's GPU (`metal`) only one larger than `m`. Else, off Metal, when `m` takes
/// `chat::QUICK` or longer to its first word (`first`, the probe's estimate), a smaller one
/// that does not.
pub fn better_ask<'a>(m: &crate::chat::Model, metal: bool, first: impl Fn(&crate::chat::Model) -> Option<f64>, servers: &[(&'a Found, bool)], hint: &str) -> Option<Better<'a>> {
    if metal {
        return servers.iter().find_map(|(f, _)| larger(f, m, hint).map(|(model, quicker)| Better::Metal { found: f, model, quicker }));
    }
    if let Some((found, model, sure)) = servers.iter().find_map(|(f, sure)| pick(f, true, hint).map(|m| (*f, m, *sure))) {
        return Some(Better::Server { found, model, sure });
    }
    if first(m).is_none_or(|s| s < crate::chat::QUICK) {
        return None;
    }
    Some(Better::Smaller(crate::chat::MODELS.iter().rev().find(|x| x.ram < m.ram && first(x).is_some_and(|s| s < crate::chat::QUICK))))
}

/// The chat model of `f` larger than the built-in `m`, and whether it is quicker too: a mixture
/// of experts that is, else the smallest larger one. Its size as the server says, else as its
/// name says, else, for the model the hardware advice names (`hint`), the advice's.
fn larger(f: &Found, m: &crate::chat::Model, hint: &str) -> Option<(String, bool)> {
    let mine = size(m.id).0?;
    let hint = hint.to_lowercase();
    f.models
        .iter()
        .filter(|x| x.chat)
        .filter_map(|x| {
            let (named, active) = size(&x.name);
            let total = x.params.or(named).or_else(|| (!hint.is_empty() && x.name.to_lowercase().starts_with(&hint)).then(|| size(&hint).0).flatten())?;
            (total > mine).then(|| (x.name.clone(), active.is_some_and(|a| a < mine), total))
        })
        .min_by(|a, b| b.1.cmp(&a.1).then(a.2.total_cmp(&b.2)))
        .map(|(name, quicker, _)| (name, quicker))
}

/// Embedding models that find by meaning better than the built-in multilingual-e5-small.
const STRONGER: [&str; 3] = ["bge-m3", "qwen3-embedding", "snowflake-arctic-embed2"];

/// The server's embedding model on a graphics card that beats the built-in model, of
/// `servers`: the server, the model, and whether that it is on the card is known. Off Metal
/// any beats it on the processor; on a Mac's GPU (`metal`) only a stronger one (`STRONGER`).
pub fn better_meaning<'a>(metal: bool, servers: &[(&'a Found, bool)]) -> Option<(&'a Found, String, bool)> {
    if metal {
        return servers.iter().find_map(|(f, sure)| f.models.iter().find(|m| m.embed && STRONGER.iter().any(|w| m.name.to_lowercase().contains(w))).map(|m| (*f, m.name.clone(), *sure)));
    }
    servers.iter().find_map(|(f, sure)| pick(f, false, "").map(|m| (*f, m, *sure)))
}

/// A choice of model that is poor on this machine: what to say, and the better one.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Poor {
    /// About Ask's chat model; else about the model for the vectors.
    pub ask: bool,
    /// What is slow here, and what is quicker.
    pub text: String,
    /// One short line, for Find's Ask row.
    pub short: String,
    /// The button that takes the better one ("Use qwen3:8b"), and the options it saves; none
    /// when the better one is to be downloaded or set up first (the text says which).
    pub button: Option<String>,
    pub changes: serde_json::Map<String, serde_json::Value>,
    /// Not poor, only better at hand: a tip, shown plain, not as a warning (a Mac's GPU).
    #[serde(default)]
    pub tip: bool,
}

/// The choices of `cfg` that are poor here: a built-in model on the processor while a server
/// on the graphics card answers, or a built-in chat model too large for the processor; on a
/// Mac's GPU, a tip when a server there has a larger or stronger model. Empty until the
/// servers have been asked (`look`): `wait` false never waits for that.
pub fn poor(cfg: &crate::config::SearchConfig, wait: bool) -> Vec<Poor> {
    let builtin_meaning = cfg.meaning && cfg.meaning_engine == "builtin";
    if crate::chat::of(&cfg.ask_model).is_none() && !builtin_meaning {
        return vec![];
    }
    look(wait).map(|l| poor_in(cfg, &l)).unwrap_or_default()
}

/// `poor` for what `look` found.
pub fn poor_in(cfg: &crate::config::SearchConfig, look: &Look) -> Vec<Poor> {
    poor_at(cfg, look, crate::chat::metal(cfg.meaning_device == "cpu"))
}

/// `poor_in`, on a Mac's GPU or not (`metal`).
fn poor_at(cfg: &crate::config::SearchConfig, look: &Look, metal: bool) -> Vec<Poor> {
    use crate::t;
    let cpu_only = cfg.meaning_device == "cpu";
    let first = |m: &crate::chat::Model| crate::chat::estimate(m, cpu_only, false).map(|e| e.first);
    let seconds = |s: f64| format!("{:.0}", s.max(1.0));
    let mut out = vec![];
    if let Some(m) = crate::chat::of(&cfg.ask_model) {
        let hint = advise(&look.machine, &look.found).chat;
        let slow = match first(m) {
            Some(s) => t!("ask.poor_cpu", "model" => m.name, "seconds" => seconds(s)),
            None => t!("ask.poor_cpu_unknown", "model" => m.name),
        };
        let short = t!("find.ask_slow", "model" => m.name);
        let mut changes = serde_json::Map::new();
        match better_ask(m, metal, first, &ask_servers(cfg, look), &hint) {
            None => {}
            Some(Better::Server { found, model, sure }) => {
                let key = if sure { "ask.poor_server_sure" } else { "ask.poor_server_maybe" };
                let better = t!(key, "model" => model.as_str(), "server" => found.kind.name(), "gpu" => look.card());
                changes.insert("ask_model".into(), model.clone().into());
                out.push(Poor { ask: true, text: format!("{slow} {better}"), short, button: Some(t!("settings.use_model", "model" => model)), changes, tip: false });
            }
            Some(Better::Metal { found, model, quicker }) => {
                let key = if quicker { "ask.tip_metal_quicker" } else { "ask.tip_metal" };
                let text = t!(key, "model" => model.as_str(), "server" => found.kind.name(), "builtin" => m.name);
                changes.insert("ask_model".into(), model.clone().into());
                out.push(Poor { ask: true, short: text.clone(), text, button: Some(t!("settings.use_model", "model" => model)), changes, tip: true });
            }
            Some(Better::Smaller(smaller)) => {
                let better = match smaller {
                    Some(x) => t!("ask.poor_smaller", "model" => x.name, "seconds" => first(x).map(seconds).unwrap_or_default()),
                    None => t!("ask.poor_none"),
                };
                let button = smaller.filter(|x| x.installed()).map(|x| {
                    changes.insert("ask_model".into(), x.key().into());
                    t!("settings.use_model", "model" => x.name)
                });
                out.push(Poor { ask: true, text: format!("{slow} {better}"), short, button, changes, tip: false });
            }
        }
    }
    if cfg.meaning && cfg.meaning_engine == "builtin"
        && let Some((found, model, sure)) = better_meaning(metal, &meaning_servers(cfg, look))
    {
        let key = if sure { "meaning.poor_server_sure" } else { "meaning.poor_server_maybe" };
        let text = if metal { t!("meaning.tip_metal", "model" => model.as_str(), "server" => found.kind.name()) } else { format!("{} {}", t!("meaning.poor_cpu"), t!(key, "model" => model.as_str(), "server" => found.kind.name(), "gpu" => look.card())) };
        let mut changes = serde_json::Map::new();
        changes.insert("meaning_engine".into(), found.engine.clone().into());
        changes.insert("meaning_url".into(), found.url.clone().into());
        changes.insert("meaning_model".into(), model.clone().into());
        out.push(Poor { ask: false, short: text.clone(), text, button: Some(t!("settings.use_model", "model" => model)), changes, tip: metal });
    }
    out
}

/// The chat model to recommend and preselect for Ask: a server's model on the graphics card
/// when one answers that Ask can use (on a Mac's GPU the one larger than the built-in one,
/// as `better_ask` says, if it has one), else the built-in one for this machine. `look`
/// None: the servers have not been asked yet.
pub fn recommend_ask(cfg: &crate::config::SearchConfig, look: Option<&Look>) -> String {
    let cpu_only = cfg.meaning_device == "cpu";
    let builtin = crate::chat::preselect(ram_gb(), cpu_only);
    if let Some(l) = look {
        let hint = advise(&l.machine, &l.found).chat;
        let metal = crate::chat::metal(cpu_only);
        if let Some(m) = ask_servers(cfg, l).iter().find_map(|(f, _)| metal.then(|| larger(f, builtin, &hint)).flatten().map(|(m, _)| m).or_else(|| pick(f, true, &hint))) {
            return m;
        }
    }
    builtin.key()
}

/// A chat model Ask can take, as Settings and the guide list it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AskChoice {
    /// `ask_model` for it.
    pub value: String,
    /// How it is shown: its name, and for a built-in one its size and speed here.
    pub label: String,
    /// Downloaded (a server's always is).
    pub installed: bool,
    pub recommended: bool,
    /// A poor choice here: slow on the processor while something quicker is at hand.
    pub slow: bool,
}

/// The chat models of one place: the server Ask uses, or the built-in ones.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct AskGroup {
    /// "On Ollama at localhost:11434 · on the graphics card (RTX 4070)", "Built in · on the CPU".
    pub label: String,
    pub models: Vec<AskChoice>,
    /// Why a server's group has none (it does not answer, or is still being asked).
    pub missing: Option<String>,
    /// The servers are still being asked.
    pub loading: bool,
    pub server: bool,
}

/// Every chat model Ask can take as `cfg` stands, in one list: the server's (Ollama here with
/// the built-in vectors, else the server that makes them) and the built-in ones, so one pick
/// sets `ask_model`. `look` None: the servers are still being asked.
pub fn ask_choices(cfg: &crate::config::SearchConfig, look: Option<&Look>) -> Vec<AskGroup> {
    use crate::t;
    let url = crate::settings::server_url(cfg);
    let host = crate::settings::host(&url).to_string();
    let recommended = recommend_ask(cfg, look);
    let at = look.and_then(|l| l.found.iter().position(|f| same_server(&f.url, &url)).map(|i| (l, i)));
    let server = at.map(|(l, i)| &l.found[i]);
    let kind = server.map_or(if cfg.meaning_engine == "openai" { Kind::Other } else { Kind::Ollama }, |f| f.kind);
    let mut label = t!("settings.ask_group_server", "server" => kind.name(), "host" => host.as_str());
    let card = at.is_some_and(|(l, i)| l.gpu[i] == Some(true) || (l.gpu[i].is_none() && (l.machine.gpu.is_some() || l.machine.npu)));
    if let Some((l, _)) = at.filter(|_| crate::settings::is_local(&url)) {
        label = format!("{label} · {}", if card { t!("settings.ask_on_card", "gpu" => l.card()) } else { t!("meaning.on_cpu") });
    }
    let models: Vec<AskChoice> = server.map(|f| f.models.iter().filter(|m| m.chat).map(|m| AskChoice { value: m.name.clone(), label: m.name.clone(), installed: true, recommended: m.name == recommended, slow: false }).collect()).unwrap_or_default();
    let missing = match (look, server) {
        (None, _) => Some(t!("common.loading")),
        (_, None) => Some(t!("settings.ask_no_server", "server" => kind.name(), "host" => host.as_str())),
        (_, Some(_)) if models.is_empty() => Some(t!("settings.ask_no_chat_model", "server" => kind.name())),
        _ => None,
    };
    let quicker = card && !models.is_empty();
    let cpu_only = cfg.meaning_device == "cpu";
    let builtin = crate::chat::fitting(ram_gb())
        .map(|m| {
            let first = crate::chat::estimate(m, cpu_only, false).map(|e| e.first);
            let mut label = format!("{} · {}", m.name, crate::settings::human(m.size()));
            if let Some(s) = first {
                label = format!("{label} · {}", t!("settings.ask_first_word", "seconds" => format!("{:.0}", s.max(1.0))));
            }
            if !m.installed() {
                label = format!("{label} · {}", t!("settings.ask_downloads_first"));
            }
            let slow = !crate::chat::metal(cpu_only) && (quicker || first.is_some_and(|s| s >= crate::chat::QUICK));
            // Never both: a model slow here is not recommended, even as the one preselected.
            AskChoice { value: m.key(), label, installed: m.installed(), recommended: m.key() == recommended && !slow, slow }
        })
        .collect();
    vec![
        AskGroup { label, models, missing, loading: look.is_none(), server: true },
        AskGroup { label: format!("{} · {}", t!("settings.ask_group_builtin"), builtin_runs(cfg, None)), models: builtin, missing: None, loading: false, server: false },
    ]
}

/// Ask the servers again at the next `look`: Settings opened, a model pulled.
pub fn forget_look() {
    LOOK_AGAIN.store(true, std::sync::atomic::Ordering::SeqCst);
}

static LOOK_AGAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// ---------------------------------------------------------------- pulling and trying

/// Download `model` onto the server, with its progress in `p` where the server tells it.
/// LM Studio and the others download in their own windows: the error says how.
pub fn pull(f: &Found, model: &str, p: &crate::meaning::Progress) -> Result<(), String> {
    match f.kind {
        Kind::Ollama => crate::meaning::ollama_pull(&f.url, model, p).map_err(|e| e.to_string()),
        Kind::Lemonade => {
            // Lemonade answers when the download is done.
            let a: ureq::Agent = ureq::Agent::config_builder().tls_config(tls()).timeout_connect(Some(Duration::from_secs(10))).http_status_as_error(false).build().into();
            let mut res = a.post(&format!("{}/pull", f.url)).header("Content-Type", "application/json").send(serde_json::json!({ "model_name": model }).to_string()).map_err(|e| format!("{}: {e}", shown(&f.url)))?;
            if res.status().as_u16() >= 400 {
                return Err(said(&res.body_mut().read_to_string().unwrap_or_default()));
            }
            Ok(())
        }
        _ => Err(crate::t!("setup.pull_elsewhere", "server" => f.kind.name(), "model" => model)),
    }
}

/// Ask the chat model of `cfg` a question about one made-up source: the time to its first word.
pub fn try_ask(cfg: &crate::config::SearchConfig) -> Result<Duration, String> {
    if let Some(why) = crate::meaning::chat_problem(cfg, false) {
        return Err(why);
    }
    let start = Instant::now();
    let mut first = None;
    let sources = [(std::path::PathBuf::from("rocket.md"), "The rocket is named Tern and flies in May.".to_string())];
    let question = "What is the rocket called?";
    let on = |piece: &str| {
        if !piece.trim().is_empty() {
            first = Some(start.elapsed());
        }
        first.is_none()
    };
    // The built-in model is tried where Ask runs it, in the search helper, so it stays loaded.
    if crate::chat::of(&cfg.ask_model).is_some() {
        crate::helper::Client::start(cfg).answer(cfg, &[], question, &sources, on)?;
    } else {
        crate::meaning::ask(cfg, &[], question, &sources, on)?;
    }
    first.ok_or_else(|| crate::t!("setup.no_answer"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A server answering by path: the first route whose key the request line contains.
    fn fake(routes: Vec<(&'static str, &'static str)>) -> String {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for c in listener.incoming() {
                let mut c = c.unwrap();
                let mut got = Vec::new();
                let mut buf = [0; 4096];
                loop {
                    let n = c.read(&mut buf).unwrap_or(0);
                    got.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&got);
                    let Some((head, body)) = text.split_once("\r\n\r\n") else {
                        if n == 0 { break } else { continue }
                    };
                    let len = head.lines().find_map(|l| l.split_once(':').filter(|(k, _)| k.eq_ignore_ascii_case("content-length")).and_then(|(_, n)| n.trim().parse::<usize>().ok())).unwrap_or(0);
                    if body.len() >= len || n == 0 {
                        break;
                    }
                }
                let text = String::from_utf8_lossy(&got).into_owned();
                let (status, body) = routes.iter().find(|(k, _)| text.contains(k)).map_or(("404 Not Found", "{\"error\":\"no\"}"), |(_, b)| ("200 OK", *b));
                let _ = write!(c, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
                let _ = c.shutdown(std::net::Shutdown::Write);
                let _ = c.read_to_end(&mut Vec::new());
            }
        });
        url
    }

    /// Each kind of server says in its own way which models make vectors and which answer, and
    /// each is listed with only what its models can do.
    #[test]
    fn servers_are_found_with_what_their_models_can_do() {
        let ollama = fake(vec![
            ("GET /api/tags", r#"{"models":[{"name":"qwen3:8b","details":{"parameter_size":"8.2B"}},{"name":"bge-m3:latest","capabilities":["embedding"]}]}"#),
            (r#""model":"qwen3:8b""#, r#"{"capabilities":["completion","tools"]}"#),
        ]);
        let lemonade = fake(vec![("GET /api/v1/models", r#"{"data":[{"id":"Qwen3-8B-GGUF","recipe":"llamacpp","labels":["reasoning"],"downloaded":true},{"id":"nomic-embed-text-v1-GGUF","recipe":"llamacpp","labels":["embeddings"],"downloaded":true},{"id":"bge-reranker","recipe":"llamacpp","labels":["reranking"]},{"id":"Gemma-3-4b","recipe":"oga-hybrid","downloaded":false}]}"#)]);
        let lmstudio = fake(vec![("GET /api/v0/models", r#"{"data":[{"id":"text-embedding-nomic","type":"embeddings"},{"id":"qwen2.5-7b","type":"llm"}]}"#)]);
        let llamacpp = fake(vec![("GET /v1/models", r#"{"data":[{"id":"only-vectors"}]}"#), ("GET /props", "{}"), ("POST /v1/embeddings", r#"{"data":[]}"#)]);
        let nobody = "http://127.0.0.1:9".to_string();
        let found = probe_at(&[(Kind::Ollama, ollama.clone()), (Kind::Lemonade, format!("{lemonade}/api/v1")), (Kind::LmStudio, format!("{lmstudio}/v1")), (Kind::LlamaCpp, format!("{llamacpp}/v1")), (Kind::Jan, nobody), (Kind::Lemonade, format!("{lmstudio}/api/v1"))], None);
        let kinds: Vec<Kind> = found.iter().map(|f| f.kind).collect();
        assert_eq!(kinds, [Kind::Ollama, Kind::Lemonade, Kind::LmStudio, Kind::LlamaCpp], "nobody on 9; LM Studio is not taken for Lemonade");
        let can = |f: &Found| f.models.iter().map(|m| (m.name.clone(), m.embed, m.chat)).collect::<Vec<_>>();
        let all = |v: &[(&str, bool, bool)]| v.iter().map(|(n, e, c)| (n.to_string(), *e, *c)).collect::<Vec<_>>();
        assert_eq!(can(&found[0]), all(&[("qwen3:8b", false, true), ("bge-m3:latest", true, false)]));
        assert_eq!(found[0].engine, "ollama");
        assert_eq!(found[0].models[0].params, Some(8.2), "as Ollama says");
        assert_eq!(can(&found[1]), all(&[("Qwen3-8B-GGUF", false, true), ("nomic-embed-text-v1-GGUF", true, false), ("bge-reranker", false, false)]), "not downloaded: not listed");
        assert_eq!(found[1].url, format!("{lemonade}/api/v1"));
        assert_eq!(can(&found[2]), all(&[("text-embedding-nomic", true, false), ("qwen2.5-7b", false, true)]));
        assert_eq!(can(&found[3]), all(&[("only-vectors", true, false)]), "tried both ways: it makes vectors only");
        assert_eq!(probe_url(&format!("{lmstudio}/v1/"), None).map(|f| f.kind), Some(Kind::LmStudio));
    }

    /// With no server on the machine (nothing listens, or something listens and never answers)
    /// the look ends within a second or two, with nothing found: the built-in model is the way.
    #[test]
    fn no_server_is_found_quickly() {
        let mute = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let silent = format!("http://{}", mute.local_addr().unwrap());
        let start = Instant::now();
        let found = probe_at(&[(Kind::Ollama, "http://127.0.0.1:9".into()), (Kind::LmStudio, format!("{silent}/v1")), (Kind::Lemonade, format!("{silent}/api/v1"))], None);
        assert!(found.is_empty(), "{found:?}");
        // Under the 30 seconds of the slow agent, with room for a busy machine.
        assert!(start.elapsed() < Duration::from_secs(20), "{:?}", start.elapsed());
        assert!(probe_url(&silent, None).is_none());
        drop(mute);
    }

    #[test]
    fn advice_follows_the_hardware() {
        let nvidia = Machine { gpu: Some((Vendor::Nvidia, "RTX 4070".into(), Some(12))), npu: false, ram: 32 };
        let some = [Found { kind: Kind::Ollama, url: "y".into(), engine: "ollama".into(), models: vec![] }];
        let a = advise(&nvidia, &some);
        assert_eq!((a.server, a.embed.as_str(), a.chat.as_str()), (Some(Kind::Ollama), "bge-m3", "qwen3:8b"));
        assert!(a.why.contains("RTX 4070"), "{}", a.why);
        let ryzen = Machine { gpu: Some((Vendor::Amd, "Radeon 890M".into(), None)), npu: true, ram: 64 };
        assert_eq!(advise(&ryzen, &some).server, Some(Kind::Lemonade));
        let mac = Machine { gpu: Some((Vendor::Apple, "Apple silicon".into(), None)), npu: false, ram: 36 };
        assert_eq!((advise(&mac, &some).server, advise(&mac, &some).chat.as_str()), (Some(Kind::Ollama), "qwen3:14b"));
        // A Mac with no server (no Ollama, no LM Studio): the built-in model, on its GPU.
        let alone = advise(&mac, &[]);
        assert_eq!((alone.server, alone.embed.as_str()), (None, ""), "the built-in model");
        assert!(alone.why.contains("Metal") && alone.why.contains("36"), "{}", alone.why);
        assert_eq!(best(&[], &alone).map(|f| f.kind), None);
        let cpu = advise(&Machine { ram: 16, ..Machine::default() }, &[]);
        assert_eq!((cpu.server, cpu.embed.as_str(), cpu.chat.as_str()), (None, "", "qwen3:4b"), "the built-in model");
        let found = [Found { kind: Kind::LmStudio, url: "x".into(), engine: "openai".into(), models: vec![] }, Found { kind: Kind::Ollama, url: "y".into(), engine: "ollama".into(), models: vec![] }];
        assert_eq!(best(&found, &a).map(|f| f.kind), Some(Kind::Ollama));
    }

    /// Ollama that keeps a model off the graphics card is told so, with the fix.
    #[test]
    fn a_model_on_the_cpu_is_named() {
        let url = fake(vec![("GET /api/ps", r#"{"models":[{"name":"qwen3:8b","size_vram":0}]}"#)]);
        let f = Found { kind: Kind::Ollama, url, engine: "ollama".into(), models: vec![] };
        let gpu = Machine { gpu: Some((Vendor::Nvidia, "RTX 4070".into(), Some(12))), npu: false, ram: 32 };
        let why = speed_problem(&f, &gpu, "qwen3:8b").unwrap();
        assert!(why.contains("qwen3:8b") && why.contains("RTX 4070"), "{why}");
        assert_eq!(speed_problem(&f, &Machine::default(), "qwen3:8b"), None, "no GPU, nothing to say");
        let ok = fake(vec![("GET /api/ps", r#"{"models":[{"name":"qwen3:8b","size_vram":5000000000}]}"#)]);
        assert_eq!(speed_problem(&Found { url: ok, ..f }, &gpu, "qwen3:8b"), None);
    }

    fn ollama() -> Found {
        let m = |name: &str, embed| Model { name: name.into(), embed, chat: !embed, params: None };
        Found { kind: Kind::Ollama, url: crate::meaning::OLLAMA.into(), engine: "ollama".into(), models: vec![m("bge-m3:latest", true), m("llama3.2:3b", false), m("qwen3:8b", false)] }
    }

    /// The built-in chat model against what else answers: a server on the graphics card beats
    /// any built-in model on the processor; on a Mac's GPU the built-in one is a good choice;
    /// with no server, one too slow for the processor is told the smaller one that is quick.
    #[test]
    fn the_better_choice_for_ask() {
        use crate::chat::MODELS;
        let ollama = ollama();
        let (small, large) = (&MODELS[0], &MODELS[2]);
        let first = |m: &crate::chat::Model| Some(match m.id { "qwen3-14b" => 110.0, "qwen3-4b" => 30.0, _ => 8.0 });
        match better_ask(large, false, first, &[(&ollama, true)], "qwen3:8b") {
            Some(Better::Server { found, model, sure }) => assert_eq!((found.kind, model.as_str(), sure), (Kind::Ollama, "qwen3:8b", true)),
            _ => panic!("the server's model"),
        }
        assert!(matches!(better_ask(small, false, first, &[(&ollama, false)], ""), Some(Better::Server { model, .. }) if model == "qwen3:8b"), "a Qwen without a hint; even the quick small one loses");
        assert!(better_ask(large, true, first, &[(&ollama, true)], "").is_none(), "a Mac's GPU: the server's 8B is no larger");
        assert!(matches!(better_ask(large, false, first, &[], ""), Some(Better::Smaller(Some(m))) if m.id == "qwen3-1.7b"));
        assert!(better_ask(small, false, first, &[], "").is_none(), "quick enough");
        assert!(better_ask(large, false, |_| None, &[], "").is_none(), "nothing said before the estimate");
        assert!(matches!(better_ask(large, false, |_| Some(60.0), &[], ""), Some(Better::Smaller(None))), "none is quick: a server, said in words");
        assert_eq!(better_meaning(false, &[(&ollama, true)]).map(|(_, m, _)| m).as_deref(), Some("bge-m3:latest"));
        assert!(better_meaning(true, &[(&ollama, true)]).is_some(), "bge-m3 beats e5 on Metal too");
        let nomic = Found { models: vec![Model { name: "nomic-embed-text:latest".into(), embed: true, ..Default::default() }], ..ollama.clone() };
        assert!(better_meaning(true, &[(&nomic, true)]).is_none(), "on Metal, only a stronger one");
    }

    /// A server whose models are on the processor does not count, nor any without a graphics
    /// card; one with nothing loaded counts when there is a card. What is said, and the button.
    #[test]
    fn a_poor_choice_is_said_with_the_better_one() {
        let lemonade = Found { kind: Kind::Lemonade, url: "http://localhost:13305/api/v1".into(), engine: "openai".into(), models: vec![Model { name: "Qwen3-8B-GGUF".into(), embed: false, chat: true, params: None }] };
        let card = Machine { gpu: Some((Vendor::Nvidia, "RTX 4070".into(), Some(8))), npu: false, ram: 32 };
        let look = Look { found: vec![ollama(), lemonade.clone()], machine: card.clone(), gpu: vec![Some(false), None] };
        assert_eq!(look.gpu_servers().iter().map(|(f, sure)| (f.kind, *sure)).collect::<Vec<_>>(), [(Kind::Lemonade, false)]);
        assert!(Look { found: vec![ollama()], machine: Machine::default(), gpu: vec![None] }.gpu_servers().is_empty(), "no card");
        let cfg = crate::config::SearchConfig { ask_model: "builtin:qwen3-14b".into(), meaning: true, ..Default::default() };
        // Lemonade is no server for Ask with the built-in vectors: Ollama here is.
        assert!(ask_servers(&cfg, &look).is_empty());
        let look = Look { gpu: vec![Some(true), None], ..look };
        let poor = poor_at(&cfg, &look, false);
        let ask = poor.iter().find(|p| p.ask).unwrap();
        assert!(ask.text.contains("Qwen3 14B") && ask.text.contains("qwen3:8b") && ask.text.contains("Ollama") && ask.text.contains("RTX 4070"), "{}", ask.text);
        assert_eq!((ask.button.as_deref(), ask.changes["ask_model"].as_str()), (Some(crate::t!("settings.use_model", "model" => "qwen3:8b").as_str()), Some("qwen3:8b")));
        let meaning = poor.iter().find(|p| !p.ask).unwrap();
        assert_eq!((meaning.changes["meaning_engine"].as_str(), meaning.changes["meaning_model"].as_str()), (Some("ollama"), Some("bge-m3:latest")));
        if !cfg!(target_os = "macos") {
            assert_eq!(recommend_ask(&cfg, Some(&look)), "qwen3:8b");
        }
        assert!(recommend_ask(&cfg, None).starts_with(crate::chat::PREFIX), "before the servers are asked: built in");
        // A server's model for Ask: nothing to say about it.
        let cfg = crate::config::SearchConfig { ask_model: "qwen3:8b".into(), ..cfg };
        assert!(poor_at(&cfg, &look, false).iter().all(|p| !p.ask));
    }

    /// Sizes from names: the weights, and those each word goes through in a mixture of experts.
    #[test]
    fn sizes_are_read_from_names() {
        assert_eq!(size("qwen3:30b-a3b"), (Some(30.0), Some(3.0)));
        assert_eq!(size("mlx-community/Qwen3-30B-A3B-4bit"), (Some(30.0), Some(3.0)));
        assert_eq!(size("qwen3-1.7b"), (Some(1.7), None));
        assert_eq!(size("llama3.2:latest"), (None, None));
        assert_eq!(billions("567.75M"), Some(0.56775));
    }

    /// On a Mac's GPU the built-in models are a good choice: a server there only says a tip,
    /// when it has a larger chat model or a stronger embedding model; one with smaller models,
    /// or none, says nothing.
    #[test]
    fn on_metal_a_larger_server_model_is_a_tip() {
        let mac = Machine { gpu: Some((Vendor::Apple, "Apple silicon".into(), None)), npu: false, ram: 36 };
        let m = |name: &str, embed, params| Model { name: name.into(), embed, chat: !embed, params };
        let cfg = crate::config::SearchConfig { ask_model: "builtin:qwen3-14b".into(), meaning: true, ..Default::default() };
        // No server: the built-in models, nothing to say.
        assert!(poor_at(&cfg, &Look { found: vec![], machine: mac.clone(), gpu: vec![] }, true).is_empty());
        // Ollama on Metal with a mixture of experts and bge-m3.
        let ollama = Found { models: vec![m("qwen3:8b", false, Some(8.2)), m("qwen3:30b-a3b", false, Some(30.5)), m("bge-m3:latest", true, Some(0.567))], ..ollama() };
        let poor = poor_at(&cfg, &Look { found: vec![ollama.clone()], machine: mac.clone(), gpu: vec![Some(true)] }, true);
        let ask = poor.iter().find(|p| p.ask).unwrap();
        assert!(ask.tip && ask.text.contains("qwen3:30b-a3b") && ask.text.contains("Ollama") && ask.text.contains("Metal"), "{ask:?}");
        assert_eq!(ask.text, crate::t!("ask.tip_metal_quicker", "model" => "qwen3:30b-a3b", "server" => "Ollama", "builtin" => "Qwen3 14B"));
        assert_eq!(ask.changes["ask_model"].as_str(), Some("qwen3:30b-a3b"));
        let meaning = poor.iter().find(|p| !p.ask).unwrap();
        assert!(meaning.tip && meaning.changes["meaning_model"] == "bge-m3:latest", "{meaning:?}");
        // A larger dense model is larger, not quicker.
        let dense = Found { models: vec![m("qwen3:32b", false, None)], ..ollama.clone() };
        assert!(matches!(better_ask(&crate::chat::MODELS[2], true, |_| None, &[(&dense, true)], ""), Some(Better::Metal { quicker: false, .. })));
        // LM Studio with MLX models: Ask uses it once it makes the vectors.
        let lms = Found { kind: Kind::LmStudio, url: "http://localhost:1234/v1".into(), engine: "openai".into(), models: vec![m("qwen3-30b-a3b-mlx", false, None), m("text-embedding-qwen3-embedding-0.6b", true, None)] };
        let look = Look { found: vec![lms], machine: mac.clone(), gpu: vec![None] };
        let poor = poor_at(&cfg, &look, true);
        assert_eq!(poor.iter().map(|p| (p.ask, p.tip)).collect::<Vec<_>>(), [(false, true)], "built-in vectors: Ask stays with Ollama here");
        assert_eq!(poor[0].changes["meaning_model"], "text-embedding-qwen3-embedding-0.6b");
        let on_lms = crate::config::SearchConfig { meaning_engine: "openai".into(), meaning_url: "http://localhost:1234/v1".into(), ..cfg.clone() };
        let ask = poor_at(&on_lms, &look, true).into_iter().find(|p| p.ask).unwrap();
        assert!(ask.tip && ask.changes["ask_model"] == "qwen3-30b-a3b-mlx" && ask.text.contains("LM Studio"), "{ask:?}");
        // A server whose models are smaller and weaker than the built-in ones: nothing.
        let small = Found { models: vec![m("qwen3:8b", false, Some(8.2)), m("nomic-embed-text:latest", true, Some(0.137))], ..ollama };
        assert!(poor_at(&cfg, &Look { found: vec![small], machine: mac, gpu: vec![Some(true)] }, true).is_empty());
    }

    /// What this machine has: `cargo test -p coxswain-core setup -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn probe_this_machine() {
        let found = probe();
        let m = machine(&found);
        println!("{m:?}\n{:?}", advise(&m, &found));
        for f in &found {
            println!("{} {} {:?}", f.kind.name(), f.url, f.models);
        }
    }
}
