//! Transcripts of speech, with Whisper (whisper.cpp): only on request, a file or a folder at a
//! time, in the search helper's background, never on battery, pausable. A transcript is kept
//! in the search store, found by Find like any text and shown in the preview, and made again
//! only when its file changes.
//!
//! `[search] whisper` turns it on; the model (`[search] whisper_model`, else the one this
//! machine runs well) comes from Hugging Face, pinned and checked by its SHA-256, once it is
//! on. An installed `whisper-cli` runs first, else whisper.cpp's container image
//! (`[preview.images] whisper`), with no network, the file's folder read-only and the model
//! folder read-only. A file's name is never read as an option: the container gets it as a
//! script's argument, `ffmpeg` as `file:<path>`, `whisper-cli` as an absolute path after `-f`.
//!
//! Long files are transcribed ten minutes at a time: a pause, the battery or a restart of the
//! helper loses at most the ten minutes under way.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::meaning::{Pinned, Progress};
use crate::store::Store;
use crate::t;

/// Files whose speech can be transcribed: sound, and the sound of a video.
pub const FORMATS: &[&str] = &[
    "mp3", "m4a", "m4b", "aac", "wav", "flac", "ogg", "oga", "opus", "wma", "aiff", "aif", "mp4",
    "m4v", "mov", "mkv", "webm", "avi",
];
/// What `whisper-cli` reads by itself, without ffmpeg.
const PLAIN: &[&str] = &["wav", "mp3", "flac", "ogg", "oga"];
/// Its name under `[preview.images]`.
pub const TOOL: &str = "whisper";
/// Seconds of a file transcribed in one run.
const CHUNK: u64 = 600;

/// Whether the file's speech can be transcribed, by its name.
pub fn is_speech(path: &Path) -> bool {
    ext(path).is_some_and(|e| FORMATS.contains(&e.as_str()))
}

fn ext(path: &Path) -> Option<String> {
    Some(path.extension()?.to_str()?.to_ascii_lowercase())
}

// ---------------------------------------------------------------- models

/// A Whisper model, from whisper.cpp's own repository on Hugging Face.
pub struct Model {
    /// As `whisper_model` names it.
    pub id: &'static str,
    pub name: &'static str,
    file: Pinned,
    /// Work a second of sound takes, against the tiny model's.
    cost: f64,
}

const REPO: &str = "ggerganov/whisper.cpp";
const REVISION: &str = "5359861c739e955e79d9a303bcbc70fb988958b1";

/// The multilingual models, the smallest first: tiny for a slow machine, small for most,
/// large-v3-turbo (quantised) for a Mac's GPU.
pub const MODELS: &[Model] = &[
    Model {
        id: "tiny",
        name: "Whisper tiny",
        file: Pinned { repo: REPO, revision: REVISION, name: "ggml-tiny.bin", sha: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21", len: 77_691_713 },
        cost: 1.0,
    },
    Model {
        id: "base",
        name: "Whisper base",
        file: Pinned { repo: REPO, revision: REVISION, name: "ggml-base.bin", sha: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe", len: 147_951_465 },
        cost: 1.9,
    },
    Model {
        id: "small",
        name: "Whisper small",
        file: Pinned { repo: REPO, revision: REVISION, name: "ggml-small.bin", sha: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b", len: 487_601_967 },
        cost: 6.3,
    },
    Model {
        id: "large-v3-turbo",
        name: "Whisper large-v3-turbo",
        file: Pinned { repo: REPO, revision: REVISION, name: "ggml-large-v3-turbo-q5_0.bin", sha: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2", len: 574_041_195 },
        cost: 16.0,
    },
];

impl Model {
    /// Bytes the download takes.
    pub fn size(&self) -> u64 {
        self.file.len
    }

    pub fn installed(&self) -> bool {
        folder().is_some_and(|d| crate::meaning::complete(&d, std::slice::from_ref(&self.file)))
    }

    fn path(&self) -> Option<PathBuf> {
        Some(folder()?.join(self.file.name))
    }

    /// Seconds of work a second of sound takes here, with `engine`: on a Mac's GPU (an
    /// installed whisper.cpp there uses Metal), else from the processor's speed (`chat::cpu_ns`),
    /// the one measure every model's estimate goes by. None while the processor is not measured.
    pub fn rate(&self, metal: bool, wait: bool) -> Option<f64> {
        if metal {
            return Some(METAL * self.cost);
        }
        crate::chat::cpu_ns(wait).map(|ns| CPU * ns * self.cost)
    }
}

/// Seconds of work a second of sound takes, per nanosecond of the probe, for the tiny model on
/// the processor, half its threads (at most 8) at work. Measured on a Core Ultra 9 185H (the
/// probe 0.0227 ns): 176 s of speech in 5.1 s, container start included.
const CPU: f64 = 1.28;
/// The same on a Mac's GPU, for the tiny model: whisper.cpp's own figures for Apple silicon.
const METAL: f64 = 0.0045;
/// Seconds of work a second of sound may take for a model to be recommended (four times as
/// fast as it plays), and past which it is slow here (not twice as fast).
const QUICK: f64 = 0.25;
const SLOW: f64 = 0.5;

/// The name of the models' folder, among the other built-in models.
pub const FOLDER: &str = "whisper";

/// Where the models are kept: next to the other built-in models.
pub fn folder() -> Option<PathBuf> {
    Some(crate::models::folder()?.join(FOLDER))
}

/// Whether an installed whisper.cpp would run on a Mac's GPU here.
fn metal(local: bool) -> bool {
    local && cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

/// The model to take on this machine: the largest that is quick here; the tiny one when none
/// is, base while the processor is not measured yet.
pub fn recommended(local: bool, wait: bool) -> &'static Model {
    let metal = metal(local);
    let rates: Vec<Option<f64>> = MODELS.iter().map(|m| m.rate(metal, wait)).collect();
    if rates[0].is_none() {
        return &MODELS[1];
    }
    MODELS
        .iter()
        .zip(&rates)
        .rev()
        .find(|(_, r)| r.is_some_and(|r| r <= QUICK))
        .map_or(&MODELS[0], |(m, _)| m)
}

/// The model `whisper_model` names, else the recommended one.
pub fn chosen(cfg: &Config) -> &'static Model {
    let local = matches!(engine(cfg), Ok(Engine::Local { .. }));
    MODELS
        .iter()
        .find(|m| m.id == cfg.search.whisper_model)
        .unwrap_or_else(|| recommended(local, false))
}

/// The choices of `whisper_model`, for both apps: the one chosen by the machine, then each
/// model with its size, marked *recommended* or *slow here*.
pub fn choices(cfg: &Config) -> Vec<(String, String)> {
    let local = matches!(engine(cfg), Ok(Engine::Local { .. }));
    let best = recommended(local, false);
    let mut v = vec![(String::new(), t!("whisper.model_auto", "model" => best.name))];
    for m in MODELS {
        let mut label = format!("{} · {}", m.name, crate::settings::human(m.size()));
        if std::ptr::eq(m, best) {
            label = format!("{label} · {}", t!("setup.recommended"));
        } else if m.rate(metal(local), false).is_some_and(|r| r > SLOW) {
            label = format!("{label} · {}", t!("settings.ask_slow_here"));
        }
        v.push((m.id.to_string(), label));
    }
    v
}

// ---------------------------------------------------------------- how it runs

/// How whisper.cpp runs here.
#[derive(Clone, Debug, PartialEq)]
pub enum Engine {
    /// An installed `whisper-cli`, with `ffmpeg` when it is installed too.
    Local { cli: PathBuf, ffmpeg: Option<PathBuf> },
    Container { rt: &'static str, path: PathBuf, image: String },
}

/// The way it runs here, or why it cannot.
pub fn engine(cfg: &Config) -> Result<Engine, String> {
    let p = &cfg.preview;
    let pref = crate::preview_tools::asked(p, TOOL);
    let pref = pref.as_str();
    let local = crate::tools::which("whisper-cli").map(|cli| Engine::Local { cli, ffmpeg: crate::tools::which("ffmpeg") });
    let image = p.images.get(TOOL).filter(|i| !i.is_empty()).cloned();
    let container = match (crate::tools::container_runtime(p), image) {
        (Some((rt, path)), Some(image)) => Some(Engine::Container { rt, path, image }),
        _ => None,
    };
    match pref {
        "container" => container.or(local),
        "local" => local,
        _ => local.or(container),
    }
    .ok_or_else(|| t!("whisper.no_engine"))
}

/// Threads whisper.cpp takes: half the processor, at most eight.
fn threads() -> usize {
    std::thread::available_parallelism().map_or(2, |n| (n.get() / 2).clamp(1, 8))
}

/// The script the container runs: the part of the sound wanted, as whisper.cpp reads it, then
/// whisper-cli on it, both at the lowest priority. `$1` is the file, `$2` and `$3` the start and
/// length in seconds, `$4` the model, `$5` the threads; none of them is read as an option.
const SCRIPT: &str = r#"nice -n 19 ffmpeg -nostdin -loglevel error -ss "$2" -t "$3" -i "file:$1" -vn -ar 16000 -ac 1 -f wav /tmp/a.wav && nice -n 19 whisper-cli -m "/models/$4" -f /tmp/a.wav -l auto -np -t "$5""#;

/// The command for `secs` seconds of `file` from `start` (`secs` None: to its end).
#[allow(clippy::too_many_arguments)]
fn command(engine: &Engine, model: &Path, file: &Path, start: u64, secs: Option<u64>, scratch: &Path, name: &str) -> Option<Command> {
    let model_name = model.file_name()?.to_str()?;
    let len = secs.unwrap_or(360_000).to_string();
    match engine {
        Engine::Container { rt, path, image } => {
            // The one file, under a name of the container's own: its ending is all it keeps.
            let ending = ext(file).filter(|e| e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric())).unwrap_or_else(|| "bin".into());
            let at = format!("/src/in.{ending}");
            let mut c = crate::images::sealed_file(rt, path, name, file, &at, None);
            c.arg("--pull=never");
            c.arg("--mount").arg(crate::images::bind(model.parent()?, "/models", true));
            c.args(["--entrypoint", "sh", image.as_str(), "-c", SCRIPT, "sh", &at]);
            c.args([start.to_string(), len, model_name.to_string(), threads().to_string()]);
            Some(c)
        }
        Engine::Local { cli, ffmpeg } => {
            let wav = scratch.join("a.wav");
            let mut c = crate::tools::low(cli);
            c.arg("-m").arg(model).arg("-l").arg("auto").arg("-np").arg("-t").arg(threads().to_string()).arg("-f");
            match ffmpeg {
                Some(_) => c.arg(&wav),
                // Without ffmpeg whisper-cli reads WAV, MP3, FLAC and Ogg itself, whole.
                None => c.arg(std::path::absolute(file).ok()?),
            };
            Some(c)
        }
    }
}

/// With an installed ffmpeg, the part of the sound wanted, as a WAV in `scratch`.
fn cut(ffmpeg: &Path, file: &Path, start: u64, secs: Option<u64>, scratch: &Path) -> Result<(), String> {
    let mut c = crate::tools::low(ffmpeg);
    c.args(["-nostdin", "-loglevel", "error", "-y", "-ss", &start.to_string(), "-t", &secs.unwrap_or(360_000).to_string(), "-i"]);
    c.arg(format!("file:{}", std::path::absolute(file).map_err(|e| e.to_string())?.display()));
    c.args(["-vn", "-ar", "16000", "-ac", "1", "-f", "wav"]).arg(scratch.join("a.wav"));
    let out = crate::tools::capture(c, Duration::from_secs(600), 64 << 10).map_err(|e| e.to_string())?;
    if out.status.success() { Ok(()) } else { Err(last_line(&out.stderr)) }
}

fn last_line(b: &[u8]) -> String {
    String::from_utf8_lossy(b).lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// whisper-cli's lines, `[00:01:02.500 --> 00:01:05.000]  text`, as `[1:02] text`, `start`
/// seconds later.
pub fn lines(out: &str, start: u64) -> String {
    out.lines()
        .filter_map(|l| {
            let rest = l.trim().strip_prefix('[')?;
            let (stamp, text) = rest.split_once(']')?;
            let from = stamp.split("-->").next()?.trim();
            let mut parts = from.split(':').rev();
            let s: f64 = parts.next()?.parse().ok()?;
            let m: u64 = parts.next().unwrap_or("0").parse().ok()?;
            let h: u64 = parts.next().unwrap_or("0").parse().ok()?;
            let at = start + h * 3600 + m * 60 + s as u64;
            let text = text.trim();
            (!text.is_empty()).then(|| format!("[{}] {text}", clock(at)))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Seconds as `1:02` or `1:00:02`.
pub fn clock(secs: u64) -> String {
    if secs >= 3600 {
        format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60)
    } else {
        format!("{}:{:02}", secs / 60, secs % 60)
    }
}

/// Transcribe `secs` seconds of `file` from `start`; `stop` ends it at once (None then).
fn run(engine: &Engine, model: &Path, file: &Path, start: u64, secs: Option<u64>, stop: &dyn Fn() -> bool) -> Result<Option<String>, String> {
    let scratch = crate::fs::Scratch::new().map_err(|e| e.to_string())?;
    if let Engine::Local { ffmpeg: Some(ff), .. } = engine {
        cut(ff, file, start, secs, &scratch)?;
    }
    if let Engine::Local { ffmpeg: None, .. } = engine
        && !ext(file).is_some_and(|e| PLAIN.contains(&e.as_str()))
    {
        return Err(t!("whisper.needs_ffmpeg"));
    }
    static N: AtomicU64 = AtomicU64::new(0);
    let name = format!("coxswain-whisper-{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed));
    // Whether the runtime can bound memory and processes is found once, apart, by a probe;
    // never from what this run prints, which a file's name or its sound could make say anything.
    if let Engine::Container { path, image, .. } = engine {
        crate::images::bounds_work(path, image);
    }
    let mut c = command(engine, model, file, start, secs, &scratch, &name).ok_or("no command")?;
    let child = c.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.to_string())?;
    // Long enough for the slowest model on a slow machine: three times as long as the part
    // plays, at least half an hour.
    let most = Duration::from_secs((secs.unwrap_or(CHUNK) * 3).max(1800));
    let Some(out) = watch(child, most, stop, || remove(engine, &name))? else { return Ok(None) };
    if !out.status.success() {
        return Err(last_line(&out.stderr));
    }
    Ok(Some(lines(&String::from_utf8_lossy(&out.stdout), start)))
}

/// Wait for `child` up to `most`, its output kept within bounds; None when `stop` said so
/// (it is killed, and `gone` removes what it left).
fn watch(mut child: std::process::Child, most: Duration, stop: &dyn Fn() -> bool, gone: impl Fn()) -> Result<Option<std::process::Output>, String> {
    let drain = |p: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut v = vec![];
            if let Some(mut p) = p {
                let _ = (&mut p).take(crate::extract::MAX_TEXT as u64).read_to_end(&mut v);
                let _ = std::io::copy(&mut p, &mut std::io::sink());
            }
            v
        })
    };
    let o = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let e = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let began = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().map_err(|e| e.to_string())? {
            break s;
        }
        let late = began.elapsed() > most;
        if stop() || late {
            let _ = child.kill();
            let _ = child.wait();
            gone();
            return if late { Err(t!("whisper.too_long")) } else { Ok(None) };
        }
        std::thread::sleep(Duration::from_millis(200));
    };
    Ok(Some(std::process::Output { status, stdout: o.join().unwrap_or_default(), stderr: e.join().unwrap_or_default() }))
}

fn remove(engine: &Engine, name: &str) {
    if let Engine::Container { path, .. } = engine {
        let _ = crate::tools::command(path).args(["rm", "-f", name]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
}

// ---------------------------------------------------------------- what to transcribe

/// How long the sound of a file is, in seconds, when its header says.
pub fn duration(path: &Path) -> Option<u64> {
    use lofty::prelude::*;
    // The file comes from anywhere and lofty is other people's code: a panic is no length.
    let secs = std::panic::catch_unwind(|| lofty::read_from_path(path).ok().map(|f| f.properties().duration().as_secs()))
        .ok()
        .flatten()?;
    (secs > 0).then_some(secs)
}

/// What a request would transcribe: the files, and the seconds of those whose length is known.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Survey {
    pub files: Vec<PathBuf>,
    pub seconds: u64,
    /// Files whose length is not known.
    pub unknown: usize,
}

/// The files of `paths` with speech, folders gone through, none in the cloud only; at most
/// `MAX_FILES`.
pub fn survey(paths: &[PathBuf]) -> Survey {
    let mut s = Survey::default();
    let mut add = |p: &Path| {
        if s.files.len() >= MAX_FILES || !is_speech(p) || crate::cloud::keep_out(p) {
            return;
        }
        match duration(p) {
            // A header may claim any length: summed without wrapping round.
            Some(d) => s.seconds = s.seconds.saturating_add(d),
            None => s.unknown += 1,
        }
        s.files.push(p.to_path_buf());
    };
    // Folders as they are, links not followed: a link to a folder above would never end.
    let mut todo: Vec<PathBuf> = paths.to_vec();
    while let Some(p) = todo.pop() {
        let Ok(meta) = std::fs::symlink_metadata(&p) else { continue };
        if meta.is_dir() {
            if let Ok(dir) = std::fs::read_dir(&p) {
                let mut inside: Vec<PathBuf> = dir.flatten().map(|e| e.path()).collect();
                inside.sort();
                todo.extend(inside.into_iter().rev());
            }
        } else if meta.is_file() {
            add(&p);
        }
    }
    s
}

/// The most files one request takes.
pub const MAX_FILES: usize = 10_000;

/// Seconds of work `seconds` of sound take here with `cfg`'s model: the speed measured on
/// this machine when there is one (`measured`), else the estimate.
pub fn estimate(cfg: &Config, seconds: u64, measured: Option<f64>) -> Option<u64> {
    let local = matches!(engine(cfg), Ok(Engine::Local { .. }));
    let rate = measured.or_else(|| chosen(cfg).rate(metal(local), true))?;
    Some((seconds as f64 * rate).round() as u64)
}

/// A span of time in words: `3 h 20 min`, `12 min`, `40 s`.
pub fn span(secs: u64) -> String {
    let s = if secs >= 3600 {
        t!("whisper.hours", "h" => secs / 3600, "m" => secs % 3600 / 60)
    } else if secs >= 60 {
        t!("whisper.minutes", "m" => secs.div_ceil(60))
    } else {
        t!("whisper.seconds", "s" => secs)
    };
    crate::i18n::digits(s)
}

/// The question before a request: how much sound, how long it takes here, how it runs, and
/// what it fetches first.
pub fn plan_text(cfg: &Config, s: &Survey, measured: Option<f64>) -> String {
    let mut parts = vec![t!("whisper.plan_files", "n" => crate::settings::count(s.files.len()), "length" => span(s.seconds))];
    if s.unknown > 0 {
        parts.push(t!("whisper.plan_unknown", "n" => crate::settings::count(s.unknown)));
    }
    if let Some(work) = estimate(cfg, s.seconds, measured) {
        parts.push(t!("whisper.plan_time", "time" => span(work.max(1)), "model" => chosen(cfg).name));
    }
    parts.push(t!("whisper.plan_background"));
    let m = chosen(cfg);
    if !m.installed() {
        parts.push(t!("whisper.plan_model", "model" => m.name, "size" => crate::settings::human(m.size())));
    }
    if let Ok(Engine::Container { path, image, .. }) = engine(cfg)
        && crate::tools::image_size(&path, &image).is_none()
    {
        parts.push(t!("whisper.plan_image", "image" => crate::images::short(&image), "size" => crate::images::download(&image).unwrap_or_default()));
    }
    parts.join(" ")
}

/// The transcript of `path` as a text file to view (the terminal app's F3 on a recording):
/// with a line before it while it is not done.
pub fn text_copy(path: &Path, t: &Transcript) -> Result<PathBuf, String> {
    let to = crate::archive::peek_folder()
        .map_err(|e| e.to_string())?
        .join(format!("{}.transcript.txt", path.file_name().unwrap_or_default().to_string_lossy()));
    let head = if t.failed {
        format!("{}\n\n", t!("whisper.failed", "why" => t.text.as_str()))
    } else if !t.finished {
        format!("{}\n\n", t!("whisper.so_far", "time" => clock(t.done)))
    } else {
        String::new()
    };
    let body = if t.failed { "" } else { t.text.as_str() };
    std::fs::write(&to, format!("{head}{body}\n")).map_err(|e| e.to_string())?;
    Ok(to)
}

// ---------------------------------------------------------------- the helper's work

/// How transcribing stands, for the status lines and the apps.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    /// Files waiting, done, and given up.
    pub waiting: usize,
    pub done: usize,
    pub failed: usize,
    /// The file being transcribed, and how far, in percent.
    pub current: Option<(PathBuf, u8)>,
    /// Seconds of work left, as far as the lengths are known.
    pub left: Option<u64>,
    /// Seconds of work a second of sound took here, last measured.
    pub rate: Option<f64>,
    pub state: State,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Idle,
    Working,
    Paused,
    /// Waiting for mains power.
    Battery,
    /// The model downloading: bytes done of all.
    Downloading(u64, u64),
    /// The image pulling: the runtime's last line.
    Pulling(String),
    /// It cannot run here: why.
    Off(String),
}

/// One file's transcript, for the preview.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transcript {
    pub text: String,
    /// Done; else waiting or under way, `done` seconds of it so far.
    pub finished: bool,
    pub failed: bool,
    pub done: u64,
    pub length: Option<u64>,
    /// Its place in the queue (0: the one under way).
    pub place: Option<usize>,
}

static PAUSED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State::Idle);
static CURRENT: Mutex<Option<(PathBuf, u8)>> = Mutex::new(None);
static DOWNLOAD: std::sync::LazyLock<Arc<Progress>> = std::sync::LazyLock::new(Default::default);

/// Pause or go on: the part under way is stopped at once and done again on going on.
pub fn pause(on: bool) {
    PAUSED.store(on, Ordering::Relaxed);
}

/// The settings the helper's worker started with: set while transcribing is on.
static CFG: std::sync::OnceLock<Config> = std::sync::OnceLock::new();

/// How it stands; None when transcribing is off.
pub fn status(store: &Store) -> Option<Status> {
    let cfg = CFG.get()?;
    let (waiting, done, failed, left_secs) = store.transcript_counts().unwrap_or_default();
    let rate = store.transcribe_rate();
    let mut state = STATE.lock().unwrap().clone();
    if PAUSED.load(Ordering::Relaxed) && waiting > 0 {
        state = State::Paused;
    }
    Some(Status {
        waiting,
        done,
        failed,
        current: CURRENT.lock().unwrap().clone(),
        left: estimate(cfg, left_secs, rate),
        rate,
        state,
    })
}

fn set(state: State) {
    *STATE.lock().unwrap() = state;
}

/// The helper's worker: transcribes what was asked for, one part at a time, while `stop` is
/// not set; never on battery, never while paused, nothing while transcribing is off.
pub fn work(store: Arc<Store>, cfg: Config, stop: Arc<AtomicBool>) {
    if !cfg.search.whisper || !cfg.search.text {
        return;
    }
    let cfg = CFG.get_or_init(|| cfg).clone();
    let idle = || std::thread::sleep(Duration::from_secs(2));
    let mut ready: Option<(Engine, PathBuf)> = None;
    while !stop.load(Ordering::Relaxed) {
        let Ok(Some(next)) = store.transcript_next() else {
            set(State::Idle);
            *CURRENT.lock().unwrap() = None;
            idle();
            continue;
        };
        if PAUSED.load(Ordering::Relaxed) {
            idle();
            continue;
        }
        if crate::machine::on_battery() {
            set(State::Battery);
            idle();
            continue;
        }
        if ready.is_none() {
            match prepare(&cfg, &stop) {
                Ok(r) => ready = Some(r),
                Err(why) => {
                    set(State::Off(why));
                    // Tried again in a while: a network that comes back, podman installed.
                    for _ in 0..150 {
                        if stop.load(Ordering::Relaxed) {
                            return;
                        }
                        idle();
                    }
                    continue;
                }
            }
        }
        let (engine, model) = ready.as_ref().unwrap();
        set(State::Working);
        one_part(&store, engine, model, next, &stop);
    }
}

/// The model downloaded and the image pulled, as turning transcribing on asked for.
fn prepare(cfg: &Config, stop: &AtomicBool) -> Result<(Engine, PathBuf), String> {
    let engine = engine(cfg)?;
    let m = chosen(cfg);
    if !m.installed() {
        set(State::Downloading(0, m.size()));
        let p = DOWNLOAD.clone();
        let (p2, done) = (p.clone(), Arc::new(AtomicBool::new(false)));
        let d2 = done.clone();
        std::thread::spawn(move || {
            while !d2.load(Ordering::Relaxed) {
                set(State::Downloading(p2.done.load(Ordering::Relaxed), p2.total.load(Ordering::Relaxed)));
                std::thread::sleep(Duration::from_millis(500));
            }
        });
        let r = crate::meaning::fetch(&folder().ok_or("no cache folder")?, std::slice::from_ref(&m.file), &p);
        done.store(true, Ordering::Relaxed);
        r.map_err(|e| e.to_string())?;
        if stop.load(Ordering::Relaxed) {
            return Err(String::new());
        }
    }
    if let Engine::Container { path, image, .. } = &engine
        && crate::tools::image_size(path, image).is_none()
    {
        set(State::Pulling(String::new()));
        let (path2, image2) = (path.clone(), image.clone());
        let pulled = std::thread::spawn(move || crate::images::pull(&path2, &image2));
        while !pulled.is_finished() {
            if let Some(l) = crate::images::pulling().get(image) {
                set(State::Pulling(l.clone()));
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        pulled.join().map_err(|_| "pull".to_string())??;
    }
    if let Engine::Container { image, .. } = &engine {
        let mut st = crate::state::AppState::load();
        crate::images::used(&mut st, image, crate::fs::now());
        let _ = st.save();
    }
    Ok((engine, m.path().ok_or("no cache folder")?))
}

/// Transcribe the next part of `next`, and keep it.
fn one_part(store: &Store, engine: &Engine, model: &Path, next: crate::store::Waiting, stop: &AtomicBool) {
    let path = PathBuf::from(&next.path);
    let Ok(meta) = std::fs::metadata(&path) else {
        let _ = store.transcript_gone(&next.path);
        return;
    };
    // The length, the first time: chunks only when it is known.
    let length = next.length.or_else(|| duration(&path));
    let whole = matches!(engine, Engine::Local { ffmpeg: None, .. }) || length.is_none();
    let secs = (!whole).then_some(CHUNK);
    let pct = |done: u64| length.map_or(0, |l| ((done * 100) / l.max(1)).min(99) as u8);
    *CURRENT.lock().unwrap() = Some((path.clone(), pct(next.done)));
    let began = Instant::now();
    let stopped = || stop.load(Ordering::Relaxed) || PAUSED.load(Ordering::Relaxed) || crate::machine::on_battery();
    match run(engine, model, &path, next.done, secs, &stopped) {
        Ok(None) => {}
        Ok(Some(text)) => {
            let done = if whole { length.unwrap_or(0) } else { length.map_or(next.done + CHUNK, |l| (next.done + CHUNK).min(l)) };
            let finished = whole || length.is_none_or(|l| done >= l);
            let heard = if whole { length } else { Some(CHUNK.min(length.unwrap_or(CHUNK).saturating_sub(next.done))) };
            if let Some(h) = heard.filter(|h| *h >= 30) {
                let _ = store.transcribe_measured(began.elapsed().as_secs_f64() / h as f64);
            }
            let _ = store.transcript_part(&next.path, crate::store::meta_of(&meta), length, done, &text, finished);
            *CURRENT.lock().unwrap() = (!finished).then(|| (path.clone(), pct(done)));
        }
        Err(why) => {
            crate::helper::note(format_args!("whisper: {}: {why}", path.display()));
            let _ = store.transcript_failed(&next.path, &why);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_sound_file_has_no_length() {
        let d = std::env::temp_dir().join(format!("coxswain-duration-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        for (name, bytes) in [("a.mp3", &b"ID3\x04\0\0\x7f\x7f\x7f\x7fnot a song"[..]), ("b.wav", b"RIFF\xff\xff\xff\xffWAVEfmt "), ("c.m4a", b"\0\0\0\x08ftyp")] {
            std::fs::write(d.join(name), bytes).unwrap();
            assert_eq!(duration(&d.join(name)), None, "{name}");
        }
        let s = survey(std::slice::from_ref(&d));
        assert_eq!((s.files.len(), s.seconds, s.unknown), (3, 0, 3));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn whisper_lines_are_kept_with_their_times() {
        let out = "[00:00:00.000 --> 00:00:10.500]   And so, my fellow Americans,\n\n[00:01:02.000 --> 00:01:05.000]  ask not.\nwhisper_print_timings: x\n[00:00:09.000 --> 00:00:10.000]   \n";
        assert_eq!(lines(out, 0), "[0:00] And so, my fellow Americans,\n[1:02] ask not.");
        assert_eq!(lines(out, 3600), "[1:00:00] And so, my fellow Americans,\n[1:01:02] ask not.");
        assert_eq!(clock(59), "0:59");
    }

    #[test]
    fn whisper_names_are_never_options_and_the_container_is_sealed() {
        let e = Engine::Container { rt: "podman", path: "/nonexistent/podman".into(), image: "ghcr.io/ggml-org/whisper.cpp:main@sha256:b3a5".into() };
        let c = command(&e, Path::new("/m/whisper/ggml-tiny.bin"), Path::new("/music/-rf $(id).mp3"), 600, Some(600), Path::new("/tmp/s"), "n").unwrap();
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        for want in ["--network=none", "--pull=never", "--cap-drop=ALL", "no-new-privileges", "--read-only", "/tmp:rw,size=1g", "--memory=3g", "--pids-limit=256", "type=bind,\"source=/m/whisper\",target=/models,readonly", "type=bind,\"source=/music/-rf $(id).mp3\",target=/src/in.mp3,readonly"] {
            assert!(args.iter().any(|a| a == want), "{want}: {args:?}");
        }
        // The one file is mounted under a name of the container's own; that name comes after
        // the script, as one of its arguments.
        let at = args.iter().position(|a| a == SCRIPT).unwrap();
        assert_eq!(args[at + 1..at + 4], ["sh", "/src/in.mp3", "600"]);
        assert!(!args.iter().any(|a| a == "-v" || a.contains("$(id).mp3:")), "{args:?}");
        assert!(SCRIPT.contains("\"file:$1\""));
        // A name that reads like a runtime's complaint changes nothing: the bounds are the
        // probe's, decided before the run.
        let odd = command(&e, Path::new("/m/whisper/ggml-tiny.bin"), Path::new("/music/cgroup memory.max pids.wav"), 0, Some(600), Path::new("/tmp/s"), "n").unwrap();
        let odd: Vec<String> = odd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert!(odd.iter().any(|a| a == "--memory=3g") && odd.iter().any(|a| a == "--pids-limit=256"), "{odd:?}");
        assert!(!include_str!("transcribe.rs").contains(concat!("String::from_utf8_lossy(&o.", "stderr).contains")), "never decided from a run's output");
        let l = Engine::Local { cli: "/usr/bin/whisper-cli".into(), ffmpeg: None };
        let c = command(&l, Path::new("/m/ggml-tiny.bin"), Path::new("/music/-x.mp3"), 0, None, Path::new("/tmp/s"), "n").unwrap();
        let args: Vec<String> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        let f = args.iter().position(|a| a == "-f").unwrap();
        assert_eq!(std::path::Path::new(&args[f + 1]), std::path::absolute("/music/-x.mp3").unwrap(), "an absolute path after -f");
        assert!(std::path::Path::new(&args[f + 1]).is_absolute());
    }

    #[test]
    fn whisper_model_follows_the_machine() {
        // No machine measured here: base; tiny is the one with the least cost.
        assert!(MODELS.windows(2).all(|w| w[0].cost < w[1].cost && w[0].size() < w[1].size() || w[1].id == "large-v3-turbo"));
        assert!(MODELS[3].rate(true, false).unwrap() <= QUICK, "turbo is quick on a Mac's GPU");
        let pick = |ns: f64| MODELS.iter().rev().find(|m| CPU * ns * m.cost <= QUICK).map_or("tiny", |m| m.id);
        assert_eq!(pick(0.0227), "small", "a Core Ultra 9 185H");
        assert_eq!(pick(0.05), "base", "a processor half as quick");
        assert_eq!(pick(0.3), "tiny", "a slow one");
        assert!(CPU * 0.05 * MODELS[3].cost > SLOW, "turbo is slow on a processor");
    }

    #[test]
    fn whisper_takes_sound_and_video() {
        assert!(is_speech(Path::new("a/Talk.MP3")) && is_speech(Path::new("b.mkv")));
        assert!(!is_speech(Path::new("c.txt")) && !is_speech(Path::new("mp3")));
        crate::i18n::set_language("en-GB");
        assert_eq!(span(3 * 3600 + 20 * 60), "3 h 20 min");
        assert_eq!(span(61), "2 min");
    }

    /// Needs whisper-cli installed or podman with the image, and the tiny model:
    /// COXSWAIN_WHISPER_MODEL=…/ggml-tiny.bin COXSWAIN_WHISPER_AUDIO=…/jfk.m4a
    /// `cargo test -p coxswain-core -- --ignored real_whisper`.
    #[test]
    #[ignore]
    fn real_whisper_transcribes_in_parts() {
        let model = PathBuf::from(std::env::var("COXSWAIN_WHISPER_MODEL").unwrap());
        let audio = PathBuf::from(std::env::var("COXSWAIN_WHISPER_AUDIO").unwrap());
        let engine = engine(&Config::default()).unwrap();
        let text = run(&engine, &model, &audio, 0, Some(CHUNK), &|| false).unwrap().unwrap();
        assert!(text.to_lowercase().contains("country"), "{text}");
        let started = Instant::now();
        let stop_soon = || started.elapsed() > Duration::from_millis(300);
        assert_eq!(run(&engine, &model, &audio, 0, Some(CHUNK), &stop_soon).unwrap(), None, "stopped");
    }
}
