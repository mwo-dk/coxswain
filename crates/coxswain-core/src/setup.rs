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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Model {
    pub name: String,
    /// It makes vectors (an embedding model).
    pub embed: bool,
    /// It answers (a chat model).
    pub chat: bool,
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
                Some(match can {
                    Some(c) => Model { embed: c.iter().any(|x| x == "embedding"), chat: c.iter().any(|x| x == "completion"), name },
                    None => guess(name),
                })
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
                Some(Model { name: m["id"].as_str()?.to_string(), embed, chat: !embed && !labels.contains(&"reranking") })
            });
            openai(kind, models.collect())
        }
        Kind::LmStudio => {
            // LM Studio's own API says what each model is: `llm`, `vlm` or `embeddings`.
            let base = url.trim_end_matches("/v1");
            let v = get(&quick, &format!("{base}/api/v0/models"), key)?;
            let models = v["data"].as_array()?.iter().filter_map(|m| {
                let t = m["type"].as_str().unwrap_or("");
                Some(Model { name: m["id"].as_str()?.to_string(), embed: t == "embeddings", chat: t == "llm" || t == "vlm" })
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
                let tried: Vec<_> = names.iter().map(|n| { let slow = &slow; s.spawn(move || Model { name: n.clone(), embed: tries(slow, url, key, "/embeddings", serde_json::json!({ "model": n, "input": "test" })), chat: tries(slow, url, key, "/chat/completions", serde_json::json!({ "model": n, "messages": [{ "role": "user", "content": "Hi" }], "max_tokens": 1 })) }) }).collect();
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
    Model { embed, chat: !embed, name }
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

fn ram_gb() -> u64 {
    #[cfg(target_os = "linux")]
    {
        let info = std::fs::read_to_string("/proc/meminfo").unwrap_or_default();
        info.lines().find_map(|l| l.strip_prefix("MemTotal:")).and_then(|v| v.trim().trim_end_matches("kB").trim().parse::<u64>().ok()).map_or(0, |kb| kb.div_ceil(1 << 20))
    }
    #[cfg(target_os = "macos")]
    {
        crate::tools::command("sysctl").args(["-n", "hw.memsize"]).output().ok().and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u64>().ok()).map_or(0, |b| b.div_ceil(1 << 30))
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        if unsafe { GlobalMemoryStatusEx(&mut s) } != 0 { s.ullTotalPhys.div_ceil(1 << 30) } else { 0 }
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        0
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

pub fn advise(m: &Machine) -> Advice {
    let ram = m.ram.to_string();
    match &m.gpu {
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
    crate::meaning::ask(cfg, &[], "What is the rocket called?", &sources, |piece| {
        if !piece.trim().is_empty() {
            first = Some(start.elapsed());
        }
        first.is_none()
    })?;
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
            ("GET /api/tags", r#"{"models":[{"name":"qwen3:8b"},{"name":"bge-m3:latest","capabilities":["embedding"]}]}"#),
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
        assert_eq!(can(&found[1]), all(&[("Qwen3-8B-GGUF", false, true), ("nomic-embed-text-v1-GGUF", true, false), ("bge-reranker", false, false)]), "not downloaded: not listed");
        assert_eq!(found[1].url, format!("{lemonade}/api/v1"));
        assert_eq!(can(&found[2]), all(&[("text-embedding-nomic", true, false), ("qwen2.5-7b", false, true)]));
        assert_eq!(can(&found[3]), all(&[("only-vectors", true, false)]), "tried both ways: it makes vectors only");
        assert_eq!(probe_url(&format!("{lmstudio}/v1/"), None).map(|f| f.kind), Some(Kind::LmStudio));
    }

    #[test]
    fn advice_follows_the_hardware() {
        let nvidia = Machine { gpu: Some((Vendor::Nvidia, "RTX 4070".into(), Some(12))), npu: false, ram: 32 };
        let a = advise(&nvidia);
        assert_eq!((a.server, a.embed.as_str(), a.chat.as_str()), (Some(Kind::Ollama), "bge-m3", "qwen3:8b"));
        assert!(a.why.contains("RTX 4070"), "{}", a.why);
        let ryzen = Machine { gpu: Some((Vendor::Amd, "Radeon 890M".into(), None)), npu: true, ram: 64 };
        assert_eq!(advise(&ryzen).server, Some(Kind::Lemonade));
        let mac = Machine { gpu: Some((Vendor::Apple, "Apple silicon".into(), None)), npu: false, ram: 36 };
        assert_eq!(advise(&mac).chat, "qwen3:14b");
        let cpu = advise(&Machine { ram: 16, ..Machine::default() });
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

    /// What this machine has: `cargo test -p coxswain-core setup -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn probe_this_machine() {
        let found = probe();
        let m = machine(&found);
        println!("{m:?}\n{:?}", advise(&m));
        for f in &found {
            println!("{} {} {:?}", f.kind.name(), f.url, f.models);
        }
    }
}
