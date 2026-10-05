//! `coxswain --setup-search`: the guided setup of search inside files, by meaning and Ask, as
//! the desktop app's *Set up…* does it, asked step by step on the command line. Every step can
//! be skipped, and nothing is downloaded without a yes.

use coxswain_core::config::{Config, SearchConfig};
use coxswain_core::helper::Client;
use coxswain_core::meaning::{self, Progress};
use coxswain_core::setup::{self, Found, Kind};
use coxswain_core::t;
use std::io::Write;

fn line() -> String {
    let mut s = String::new();
    let _ = std::io::stdin().read_line(&mut s);
    s.trim().to_string()
}

fn yes(question: &str) -> bool {
    print!("{question} [y/N] ");
    let _ = std::io::stdout().flush();
    matches!(line().as_str(), "y" | "Y" | "j" | "J")
}

/// One of `items` by its number; Enter takes `default`, `s` (or nothing to choose) skips.
fn choose(items: &[String], default: usize) -> Option<usize> {
    if items.is_empty() {
        return None;
    }
    for (i, it) in items.iter().enumerate() {
        println!("  {}{} {it}", i + 1, if i == default { "*" } else { " " });
    }
    loop {
        print!("{} ", t!("setup.tui_choose", "default" => default + 1));
        let _ = std::io::stdout().flush();
        match line().as_str() {
            "" => return Some(default),
            "s" | "S" => return None,
            n => match n.parse::<usize>() {
                Ok(n) if (1..=items.len()).contains(&n) => return Some(n - 1),
                _ => {}
            },
        }
    }
}

fn save(key: &str, value: &str) {
    if let Err(e) = Config::save_value(&["search", key], value.into()) {
        eprintln!("coxswain: {e}");
    }
}

fn turn_on(key: &str) {
    if let Err(e) = Config::save_value(&["search", key], true.into()) {
        eprintln!("coxswain: {e}");
    }
}

fn search() -> SearchConfig {
    Config::load().map(|c| c.search).unwrap_or_default()
}

/// Run `work` while a progress line shows how far `p` is.
fn with_progress(p: &std::sync::Arc<Progress>, work: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    use std::sync::atomic::Ordering;
    let q = p.clone();
    let shown = std::thread::spawn(move || {
        while !q.cancel.load(Ordering::Relaxed) {
            let (done, total) = (q.done.load(Ordering::Relaxed), q.total.load(Ordering::Relaxed));
            if let Some(percent) = (done * 100).checked_div(total) {
                eprint!("\r{}", t!("tui.meaning_downloading", "percent" => percent));
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        eprintln!();
    });
    let done = work();
    p.cancel.store(true, Ordering::Relaxed);
    let _ = shown.join();
    done
}

fn heading(text: String) {
    println!("\n\x1b[1m{text}\x1b[0m");
}

pub fn run() {
    heading(t!("setup.title"));
    println!("{}", t!("setup.intro"));

    // 1. Search inside files.
    heading(t!("setup.step_text"));
    println!("{}", t!("setup.text_hint"));
    if search().text {
        println!("{}", t!("setup.on"));
    } else if yes(&t!("setup.turn_on")) {
        turn_on("text");
    }

    // 2. Where the vectors come from.
    heading(t!("setup.step_server"));
    println!("{}\n{}", t!("setup.server_hint"), t!("setup.looking"));
    let mut found = setup::probe();
    let machine = setup::machine(&found);
    let advice = setup::advise(&machine);
    let gpu = machine.gpu.as_ref().map_or_else(|| t!("setup.no_gpu"), |(_, name, vram)| vram.map_or(name.clone(), |v| format!("{name} ({v} GB)")));
    println!("{}", t!("setup.machine", "what" => if machine.npu { format!("{gpu} + NPU") } else { gpu }, "ram" => machine.ram));
    println!("{}", advice.why);
    if found.is_empty() {
        println!("{}", t!("setup.none_found"));
    }
    let best = setup::best(&found, &advice).map(|b| b.url.clone());
    let describe = |f: &Found| {
        let n = |want: fn(&setup::Model) -> bool| f.models.iter().filter(|m| want(m)).count();
        let mut s = t!("setup.found", "server" => f.kind.name(), "url" => f.url.as_str(), "embed" => n(|m| m.embed), "chat" => n(|m| m.chat));
        if Some(&f.url) == best.as_ref() {
            s.push_str(&format!(" [{}]", t!("setup.recommended")));
        }
        s
    };
    let mut items: Vec<String> = found.iter().map(describe).collect();
    let builtin_size = meaning::size();
    let now = search();
    let runs = Client::start(&now).status().meaning_runs.filter(|_| now.meaning && now.meaning_engine == "builtin");
    items.push(t!("setup.builtin", "size" => format!("{} MB", builtin_size >> 20), "where" => setup::builtin_runs(&now, runs.as_ref())) + &if machine.gpu.is_none() { format!(" [{}]", t!("setup.recommended")) } else { String::new() });
    items.push(t!("setup.other"));
    let default = found.iter().position(|f| Some(&f.url) == best.as_ref()).unwrap_or(found.len());
    let mut server: Option<Found> = None;
    let mut builtin = false;
    match choose(&items, default) {
        Some(i) if i < found.len() => server = Some(found[i].clone()),
        Some(i) if i == found.len() => builtin = true,
        Some(_) => {
            println!("{}", t!("setup.other_hint"));
            print!("{} ", t!("setup.tui_url"));
            let _ = std::io::stdout().flush();
            let url = line();
            match setup::probe_url(&url, meaning::key_of(&search()).as_deref()) {
                Some(f) => {
                    println!("{}", describe(&f));
                    println!("{}", t!("settings.meaning_remote", "host" => f.url.as_str()));
                    found.push(f.clone());
                    server = Some(f);
                }
                None => println!("{}", t!("setup.nobody", "url" => url)),
            }
        }
        None => {}
    }

    // 3. The model for the vectors.
    heading(t!("setup.step_embed"));
    let confirm = |change: &dyn Fn(&mut SearchConfig)| {
        let old = search();
        let mut new = old.clone();
        change(&mut new);
        let st = Client::start(&old).status();
        match meaning::change_notice(&old, &new, st.meaning_done, st.meaning_passages) {
            Some(why) => yes(&why),
            None => true,
        }
    };
    if builtin {
        println!("{}", t!("setup.builtin_hint"));
        if confirm(&|c| c.meaning_engine = "builtin".into()) && (meaning::installed() || yes(&t!("setup.download_builtin", "size" => format!("{} MB", builtin_size >> 20)))) {
            let p = std::sync::Arc::new(Progress::default());
            match with_progress(&p, || if meaning::installed() { Ok(()) } else { meaning::download(&p).map_err(|e| e.to_string()) }) {
                Ok(()) => {
                    save("meaning_engine", "builtin");
                    turn_on("meaning");
                }
                Err(e) => eprintln!("coxswain: {e}"),
            }
        }
    } else if let Some(f) = &server {
        let suggest = f.models.iter().find(|m| m.embed && (m.name.contains("bge-m3") || m.name.contains("nomic"))).map(|m| m.name.clone()).unwrap_or_else(|| if f.kind == Kind::Ollama { "bge-m3".into() } else { advice.embed.clone() });
        println!("{}", t!("setup.embed_hint", "model" => suggest.as_str()));
        let mut models: Vec<String> = f.models.iter().filter(|m| m.embed).map(|m| m.name.clone()).collect();
        if !suggest.is_empty() && !models.iter().any(|m| m.starts_with(&suggest)) && yes(&t!("setup.pull", "model" => suggest.as_str())) {
            let p = std::sync::Arc::new(Progress::default());
            match with_progress(&p, || setup::pull(f, &suggest, &p)) {
                Ok(()) => models.push(suggest.clone()),
                Err(e) => eprintln!("coxswain: {e}"),
            }
        }
        let default = models.iter().position(|m| m.starts_with(&suggest)).unwrap_or(0);
        if let Some(i) = choose(&models, default) {
            let model = models[i].clone();
            if confirm(&|c| (c.meaning_engine, c.meaning_url, c.meaning_model) = (f.engine.clone(), f.url.clone(), model.clone())) {
                save("meaning_engine", f.engine.as_str());
                save("meaning_url", f.url.as_str());
                save("meaning_model", model.as_str());
                turn_on("meaning");
                turn_on("text");
            }
        }
    }

    // 4. Ask: the chat model, on that server (Ollama here with the built-in model).
    heading(t!("setup.step_ask"));
    let ask_server = server.clone().or_else(|| found.iter().find(|f| f.kind == Kind::Ollama).cloned());
    let mut asked = false;
    match &ask_server {
        None => println!("{}", t!("setup.ask_needs_server")),
        Some(f) => {
            let suggest = if matches!(f.kind, Kind::Ollama | Kind::Lemonade) { advice.chat.clone() } else { String::new() };
            println!("{}", t!("setup.ask_hint", "model" => if suggest.is_empty() { "qwen3:8b" } else { suggest.as_str() }));
            let mut models: Vec<String> = f.models.iter().filter(|m| m.chat).map(|m| m.name.clone()).collect();
            if !suggest.is_empty() && !models.iter().any(|m| m.starts_with(&suggest)) && yes(&t!("setup.pull", "model" => suggest.as_str())) {
                let p = std::sync::Arc::new(Progress::default());
                match with_progress(&p, || setup::pull(f, &suggest, &p)) {
                    Ok(()) => models.push(suggest.clone()),
                    Err(e) => eprintln!("coxswain: {e}"),
                }
            }
            let default = models.iter().position(|m| !suggest.is_empty() && m.starts_with(&suggest)).unwrap_or(0);
            if let Some(i) = choose(&models, default) {
                save("ask_model", models[i].as_str());
                println!("{}", t!("dialogs.ask_waiting", "model" => models[i].as_str()));
                match setup::try_ask(&search()) {
                    Ok(d) => {
                        println!("{}", t!("setup.try_done", "seconds" => format!("{:.1}", d.as_secs_f64())));
                        asked = true;
                    }
                    Err(e) => eprintln!("coxswain: {e}"),
                }
            }
        }
    }

    // 5. Speed.
    heading(t!("setup.step_speed"));
    if machine.gpu.is_none() && !machine.npu {
        println!("{}", t!("setup.speed_no_gpu"));
    } else if let (true, Some(f)) = (asked, &ask_server) {
        match setup::speed_problem(f, &machine, &search().ask_model) {
            Some(why) => println!("{why}"),
            None => println!("{}", t!("setup.speed_ok")),
        }
    } else {
        println!("{}", t!("setup.speed_hint"));
    }

    // 6. Keep reading in the background.
    heading(t!("setup.step_service"));
    println!("{}", t!("setup.service_hint"));
    if coxswain_core::service::installed() {
        println!("{}", t!("setup.on"));
    } else if yes(&t!("setup.service_on"))
        && let Err(e) = coxswain_core::tools::this_app().and_then(|exe| coxswain_core::service::install(&exe))
    {
        eprintln!("coxswain: {e}");
    }

    // 7. What is set.
    heading(t!("setup.step_done"));
    let s = search();
    let on = |b: bool| if b { t!("setup.on") } else { t!("setup.off") };
    let meaning_now = if !s.meaning { t!("setup.off") } else if s.meaning_engine == "builtin" { t!("setup.builtin_short") } else { format!("{} · {}", s.meaning_model, if s.meaning_url.is_empty() { meaning::OLLAMA } else { &s.meaning_url }) };
    println!("{}", t!("setup.sum_text", "state" => on(s.text)));
    println!("{}", t!("setup.sum_meaning", "state" => meaning_now));
    println!("{}", t!("setup.sum_ask", "state" => if s.ask_model.is_empty() { t!("setup.off") } else { s.ask_model.clone() }));
    println!("{}", t!("setup.sum_service", "state" => on(coxswain_core::service::installed())));
    Client::start(&s).restart();
}
