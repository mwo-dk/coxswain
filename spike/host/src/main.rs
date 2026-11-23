//! spike-host bench <native|cranelift|pulley|wasmi> <job> <threads> <reps> <files…>
//! spike-host batch <pgn file> [runtimes…]     rows paged out in batches, per runtime
//! spike-host grid <pgn file>                  the terminal grid (feature tui)
//! SPIKE_WASM: the component (plugin.wasm); SPIKE_CORE: its core module (plugin-core.wasm).
use serde_json::json;
use spike_host::*;
use std::time::{Duration, Instant};

fn wasm() -> Vec<u8> {
    std::fs::read(std::env::var("SPIKE_WASM").unwrap_or("plugin.wasm".into())).expect("plugin.wasm")
}

#[allow(unused_variables)]
fn runtime(name: &str) -> (Box<dyn Runtime>, Duration) {
    let t = Instant::now();
    let rt: Box<dyn Runtime> = match name {
        "native" => Box::new(Native),
        #[cfg(feature = "wasmtime")]
        "cranelift" => Box::new(wasmtime_rt::Wasmtime::new(&wasm(), false).expect("wasmtime")),
        #[cfg(feature = "wasmtime")]
        "pulley" => Box::new(wasmtime_rt::Wasmtime::new(&wasm(), true).expect("pulley")),
        #[cfg(feature = "wasmi")]
        "wasmi" => Box::new(wasmi_rt::Wasmi::new(&std::fs::read(std::env::var("SPIKE_CORE").unwrap_or("plugin-core.wasm".into())).unwrap())),
        n => panic!("runtime {n} not built in"),
    };
    (rt, t.elapsed())
}

fn job(s: &str) -> Job {
    match s {
        "pgn-scan" => Job::PgnScan,
        "pgn-parse" => Job::PgnParse,
        "scid-table" => Job::ScidTable,
        "scid-decode" => Job::ScidDecode,
        _ => panic!("job"),
    }
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1e6).round() / 1e3
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a.get(1).map(|s| s.as_str()) {
        Some("bench") => {
            let (rt, compile) = runtime(&a[2]);
            let j = job(&a[3]);
            let threads: u32 = a[4].parse().unwrap();
            let reps: u32 = a[5].parse().unwrap();
            let data: Vec<Vec<u8>> = a[6..].iter().map(|p| std::fs::read(p).unwrap()).collect();
            let files: Vec<&[u8]> = data.iter().map(|d| d.as_slice()).collect();
            let (mut best, mut inst, mut sum) = (Duration::MAX, Duration::ZERO, (0, 0, 0));
            for _ in 0..reps {
                let t = Instant::now();
                let (rows, i) = run(rt.as_ref(), j, &files, threads);
                let e = t.elapsed();
                if e < best {
                    (best, inst) = (e, i);
                }
                sum = checksum(&rows);
            }
            println!(
                "{}",
                json!({"runtime": a[2], "job": a[3], "threads": threads, "games": sum.0, "plies": sum.1, "hash": format!("{:016x}", sum.2),
                       "compile_ms": ms(compile), "instances_ms": ms(inst), "best_ms": ms(best),
                       "mb": data.iter().map(|d| d.len()).sum::<usize>() as f64 / 1e6})
            );
        }
        Some("batch") => batch(&a[2], &a[3..]),
        #[cfg(feature = "tui")]
        Some("grid") => grid::run(&a[2]),
        _ => eprintln!("see the top of host/src/main.rs"),
    }
}

/// 100,000 rows paged out of a loaded table in batches of each size, and a call that carries
/// nothing (`page(0, 0)`), per kind: in the process, a component, a program over stdin/stdout,
/// one over HTTP.
fn batch(pgn: &str, kinds: &[String]) {
    let data = std::fs::read(pgn).unwrap();
    let files = [data.as_slice()];
    let kinds: Vec<String> = if kinds.is_empty() { ["native", "cranelift", "stdio", "http"].map(String::from).to_vec() } else { kinds.to_vec() };
    for k in &kinds {
        let rt;
        let mut inst: Box<dyn Inst> = match k.as_str() {
            "stdio" => Box::new(proc::Stdio_::spawn()),
            "http" => Box::new(proc::Http::spawn()),
            n => {
                rt = runtime(n).0;
                rt.instance()
            }
        };
        let t = Instant::now();
        let n = inst.load(Job::PgnScan, &files);
        let load = t.elapsed();
        assert!(n >= 100_000, "the file needs 100,000 games");
        let empty_calls = 20_000;
        let t = Instant::now();
        for _ in 0..empty_calls {
            inst.page(0, 0);
        }
        let per_call = t.elapsed() / empty_calls;
        let mut sizes = vec![];
        for b in [1u32, 10, 100, 1_000, 10_000, 100_000] {
            let t = Instant::now();
            let mut got = 0u32;
            let mut sum = 0u64;
            while got < 100_000 {
                let r = inst.page(got, b.min(100_000 - got));
                sum += r.iter().map(|r| r.plies as u64).sum::<u64>();
                got += r.len() as u32;
            }
            let e = t.elapsed();
            sizes.push(json!({"batch": b, "ms_100k": ms(e), "us_per_call": (e.as_secs_f64() * 1e6 / (100_000 / b) as f64 * 10.0).round() / 10.0, "plies": sum}));
        }
        println!("{}", json!({"kind": k, "rows": n, "load_ms": ms(load), "empty_call_us": (per_call.as_secs_f64() * 1e7).round() / 10.0, "batches": sizes}));
    }
}

#[cfg(feature = "tui")]
mod grid {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind};
    use ratatui::buffer::Buffer;
    use ratatui::style::Color;
    use spike_host::wasmtime_rt::{Colour, Wasmtime};
    use std::time::{Duration, Instant};

    /// The named colours of the contract, as a theme gives them (here: Classic blue).
    fn theme(c: Colour) -> Color {
        match c {
            Colour::Text => Color::Rgb(0xff, 0xff, 0x55),
            Colour::Dim => Color::Rgb(0x55, 0xff, 0xff),
            Colour::Accent => Color::Rgb(0xff, 0xff, 0xff),
            Colour::LightSquare => Color::Rgb(0xc8, 0xb0, 0x8a),
            Colour::DarkSquare => Color::Rgb(0x8a, 0x6a, 0x45),
            Colour::WhitePiece => Color::Rgb(0xff, 0xff, 0xff),
            Colour::BlackPiece => Color::Rgb(0x00, 0x00, 0x00),
            Colour::Highlight => Color::Rgb(0x00, 0xaa, 0xaa),
            Colour::Background => Color::Rgb(0x00, 0x00, 0xaa),
        }
    }

    pub fn run(pgn: &str) {
        let data = std::fs::read(pgn).unwrap();
        // The first game of the file.
        let end = memchr_next_game(&data);
        let rt = Wasmtime::new(&super::wasm(), false).unwrap();
        let mut inst = rt.instance_typed();
        let plies = inst.grid_open(&data[..end]);
        let mut term = ratatui::init();
        let (mut draws, mut draw_time, mut keys, mut key_time) = (0u32, Duration::ZERO, 0u32, Duration::ZERO);
        loop {
            term.draw(|f| {
                let area = f.area();
                let t = Instant::now();
                let cells = inst.grid_draw(area.width, area.height);
                draw_time += t.elapsed();
                draws += 1;
                let buf: &mut Buffer = f.buffer_mut();
                for (i, c) in cells.iter().enumerate() {
                    let (x, y) = ((i % area.width as usize) as u16, (i / area.width as usize) as u16);
                    if let Some(cell) = buf.cell_mut((x, y)) {
                        cell.set_char(c.ch).set_fg(theme(c.fg)).set_bg(theme(c.bg));
                    }
                }
            })
            .unwrap();
            let Event::Key(k) = event::read().unwrap() else { continue };
            if k.kind != KeyEventKind::Press {
                continue;
            }
            let name = match k.code {
                KeyCode::Char('q') => break,
                KeyCode::Left => "left",
                KeyCode::Right => "right",
                KeyCode::Home => "home",
                KeyCode::End => "end",
                KeyCode::Char('f') => "f",
                _ => continue,
            };
            let t = Instant::now();
            inst.grid_key(name);
            key_time += t.elapsed();
            keys += 1;
        }
        ratatui::restore();
        let report = format!(
            "{{\"plies\": {plies}, \"draws\": {draws}, \"draw_us\": {:.1}, \"keys\": {keys}, \"key_us\": {:.1}}}\n",
            draw_time.as_secs_f64() * 1e6 / draws.max(1) as f64,
            key_time.as_secs_f64() * 1e6 / keys.max(1) as f64
        );
        std::fs::write(std::env::var("SPIKE_GRID_REPORT").unwrap_or("grid-report.json".into()), report).unwrap();
    }

    fn memchr_next_game(d: &[u8]) -> usize {
        d.windows(8).skip(1).position(|w| w == b"\n[Event ").map_or(d.len(), |i| i + 2)
    }
}
