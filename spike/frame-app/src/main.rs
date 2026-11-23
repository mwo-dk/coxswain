//! Plugin spike (#1024): a plugin's web bundle in a sealed, cross-origin frame. The bundle is
//! served by the `plugin` scheme with a strict CSP and the theme's colours; its only channel is
//! postMessage to the host page, which calls the plugin (a component in wasmtime) for it.
//! SPIKE_FRAME_SELFTEST=1: probe the seal, time the frame, print one JSON report and exit.
use spike_host::{Inst, Job, NativeInst, Row, Runtime, wasmtime_rt::Wasmtime};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::http::Response;

static BREACH: AtomicUsize = AtomicUsize::new(0);
static CONNECTIONS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static PORT: OnceLock<u16> = OnceLock::new();
static STASH: Mutex<String> = Mutex::new(String::new());

struct Plugin(Mutex<Box<dyn Inst>>);
struct NativeTable(Mutex<NativeInst>);
struct Game(Vec<u8>);

fn pgn_path() -> String {
    std::env::var("SPIKE_PGN").unwrap_or("../../../spike-plugins-scratch/lichess-2013-01.pgn".into())
}

/// The plugin bundle's page, made per load: a fresh nonce for its script and style, the
/// theme's colours as CSS variables.
fn frame_page() -> (String, String) {
    let nonce = format!("{:016x}{:016x}", std::hash::BuildHasher::hash_one(&std::collections::hash_map::RandomState::new(), 1), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos() as u64);
    let host = if cfg!(windows) { "http://tauri.localhost" } else { "tauri://localhost" };
    let port = PORT.get().copied().unwrap_or(0);
    let file = if cfg!(windows) { "file:///C:/Windows/win.ini" } else { "file:///etc/hostname" };
    let script = include_str!("../frame.js").replace("__PORT__", &port.to_string()).replace("__HOST__", host).replace("__FILE__", file);
    let html = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>plugin</title>\
<style nonce=\"{nonce}\">:root{{--bg:#0b0f1a;--fg:#e0f7ff;--light:#c8b08a;--dark:#8a6a45;--accent:#ff2fa0}}\
body{{background:var(--bg);color:var(--fg);font:12px sans-serif;margin:4px}}\
.b{{display:grid;grid-template-columns:repeat(8,22px)}}.b div{{width:22px;height:22px;text-align:center;line-height:22px;font-size:18px}}\
.l{{background:var(--light)}}.d{{background:var(--dark)}}td{{padding:0 4px;white-space:nowrap}}</style></head>\
<body><div id=\"b\" class=\"b\"></div><table><tbody id=\"t\"></tbody></table><script nonce=\"{nonce}\">{script}</script></body></html>"
    );
    let csp = format!("default-src 'none'; script-src 'nonce-{nonce}'; style-src 'nonce-{nonce}'; img-src data:; base-uri 'none'; form-action 'none'");
    (html, csp)
}

/// The probe target: counts every connection a frame manages to make.
fn listen() {
    let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    PORT.set(l.local_addr().unwrap().port()).unwrap();
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let mut s = s;
            let mut b = [0u8; 2048];
            let n = s.read(&mut b).unwrap_or(0);
            let first = String::from_utf8_lossy(&b[..n]).lines().next().unwrap_or("").to_string();
            CONNECTIONS.lock().unwrap().push(first);
            let _ = s.write_all(b"HTTP/1.1 200 OK\r\nAccess-Control-Allow-Origin: *\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok");
        }
    });
}

#[tauri::command]
fn selftest() -> bool {
    std::env::var("SPIKE_FRAME_SELFTEST").is_ok_and(|v| v == "1")
}

/// What no frame may reach: counted when anything calls it.
#[tauri::command]
fn breach() {
    BREACH.fetch_add(1, Ordering::SeqCst);
}

#[tauri::command]
fn counts() -> serde_json::Value {
    serde_json::json!({"breach_calls": BREACH.load(Ordering::SeqCst), "listener_connections": *CONNECTIONS.lock().unwrap()})
}

#[tauri::command]
fn stash(json: Option<String>) -> String {
    let mut s = STASH.lock().unwrap();
    if let Some(j) = json {
        *s = j;
    }
    s.clone()
}

#[tauri::command]
fn native_positions(game: tauri::State<Game>) -> Vec<String> {
    spike_games::positions(&game.0)
}

#[tauri::command]
fn plugin_positions(game: tauri::State<Game>, p: tauri::State<Plugin>) -> Vec<String> {
    p.0.lock().unwrap().positions(&game.0)
}

#[tauri::command]
fn plugin_page(start: u32, count: u32, p: tauri::State<Plugin>) -> Vec<Row> {
    p.0.lock().unwrap().page(start, count)
}

#[tauri::command]
fn native_page(start: u32, count: u32, n: tauri::State<NativeTable>) -> Vec<Row> {
    n.0.lock().unwrap().page(start, count)
}

/// Test only: hands the invoke key to the host page, which passes it to the frame, to see what a
/// frame could do if the key ever leaked to it (Tauri's docs: never expose it to an iframe).
#[tauri::command]
fn leak_key(app: tauri::AppHandle) -> String {
    app.invoke_key().to_string()
}

#[tauri::command]
fn report(json: String, app: tauri::AppHandle) {
    println!("{json}");
    std::io::stdout().flush().unwrap();
    app.exit(0);
}

fn main() {
    listen();
    let wasm = std::fs::read(std::env::var("SPIKE_WASM").unwrap_or("../plugin.wasm".into())).expect("plugin.wasm");
    let rt = Wasmtime::new(&wasm, false).expect("wasmtime");
    let pgn = std::fs::read(pgn_path()).expect("SPIKE_PGN");
    let first = pgn.windows(8).skip(1).position(|w| w == b"\n[Event ").map_or(pgn.len(), |i| i + 2);
    let game = pgn[..first].to_vec();
    // The table: 100,000+ games, kept in the plugin and natively.
    let mut inst = rt.instance();
    let t = std::time::Instant::now();
    let n = inst.load(Job::PgnScan, &[&pgn]);
    let wasm_load = t.elapsed();
    let mut native = NativeInst::default();
    let t = std::time::Instant::now();
    native.load(Job::PgnScan, &[&pgn]);
    eprintln!("table: {n} games; load in the plugin {wasm_load:?}, natively {:?}", t.elapsed());
    if selftest() {
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_secs(120));
            println!("{{\"error\": \"self-test timed out\", \"stash\": {:?}}}", STASH.lock().unwrap());
            std::process::exit(2);
        });
    }
    tauri::Builder::default()
        .manage(Plugin(Mutex::new(inst)))
        .manage(NativeTable(Mutex::new(native)))
        .manage(Game(game))
        .register_uri_scheme_protocol("plugin", |_ctx, req| {
            if req.uri().path() != "/frame.html" {
                return Response::builder().status(404).body(Vec::new()).unwrap();
            }
            let (html, csp) = frame_page();
            Response::builder()
                .header("Content-Type", "text/html; charset=utf-8")
                .header("Content-Security-Policy", csp)
                .header("X-Content-Type-Options", "nosniff")
                .body(html.into_bytes())
                .unwrap()
        })
        .invoke_handler(tauri::generate_handler![selftest, breach, counts, stash, native_positions, plugin_positions, plugin_page, native_page, leak_key, report])
        .run(tauri::generate_context!())
        .expect("tauri");
}
