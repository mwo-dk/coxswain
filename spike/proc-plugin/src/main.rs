//! The native-program kind: spike-games behind JSON messages, over stdin/stdout or, with
//! `--http`, as an HTTP service on 127.0.0.1 (a free port, said on the first line of stdout).
use serde_json::{Value, json};
use spike_games::{Job, Row};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};

#[derive(Default)]
struct State {
    rows: Vec<Row>,
}

impl State {
    fn handle(&mut self, msg: &Value, payload: &[Vec<u8>]) -> Value {
        match msg["op"].as_str().unwrap_or("") {
            "load" => {
                let job: Job = serde_json::from_value(msg["job"].clone()).unwrap();
                let files: Vec<&[u8]> = payload.iter().map(|f| f.as_slice()).collect();
                self.rows = spike_games::all(job, &files);
                json!({"count": self.rows.len()})
            }
            "page" => {
                let s = (msg["start"].as_u64().unwrap() as usize).min(self.rows.len());
                let e = (s + msg["count"].as_u64().unwrap() as usize).min(self.rows.len());
                json!({"rows": &self.rows[s..e]})
            }
            "positions" => json!({"fens": spike_games::positions(&payload[0])}),
            _ => json!({"error": "unknown op"}),
        }
    }
}

fn payload(r: &mut impl Read, msg: &Value) -> Vec<Vec<u8>> {
    let sizes: Vec<usize> = msg.get("sizes").map(|s| serde_json::from_value(s.clone()).unwrap()).unwrap_or_default();
    sizes
        .into_iter()
        .map(|n| {
            let mut b = vec![0; n];
            r.read_exact(&mut b).unwrap();
            b
        })
        .collect()
}

fn main() {
    let mut st = State::default();
    if std::env::args().any(|a| a == "--http") {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        println!("{}", l.local_addr().unwrap());
        std::io::stdout().flush().unwrap();
        for s in l.incoming() {
            let s = s.unwrap();
            s.set_nodelay(true).unwrap();
            let mut r = BufReader::new(s.try_clone().unwrap());
            let mut w = BufWriter::new(s);
            loop {
                let mut n = 0usize;
                let mut first = true;
                loop {
                    let mut h = String::new();
                    if r.read_line(&mut h).unwrap_or(0) == 0 {
                        return;
                    }
                    let h = h.trim_end();
                    if h.is_empty() && !first {
                        break;
                    }
                    first = false;
                    if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                        n = v.trim().parse().unwrap();
                    }
                }
                let mut body = vec![0; n];
                r.read_exact(&mut body).unwrap();
                let nl = body.iter().position(|&b| b == b'\n').unwrap_or(body.len());
                let msg: Value = serde_json::from_slice(&body[..nl]).unwrap();
                let pay = payload(&mut &body[(nl + 1).min(body.len())..], &msg);
                let out = serde_json::to_vec(&st.handle(&msg, &pay)).unwrap();
                write!(w, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n", out.len()).unwrap();
                w.write_all(&out).unwrap();
                w.flush().unwrap();
            }
        }
        return;
    }
    let mut r = BufReader::new(std::io::stdin().lock());
    let mut w = BufWriter::new(std::io::stdout().lock());
    let mut line = String::new();
    while { line.clear(); r.read_line(&mut line).unwrap_or(0) > 0 } {
        let msg: Value = serde_json::from_str(&line).unwrap();
        let pay = payload(&mut r, &msg);
        serde_json::to_writer(&mut w, &st.handle(&msg, &pay)).unwrap();
        w.write_all(b"\n").unwrap();
        w.flush().unwrap();
    }
}
