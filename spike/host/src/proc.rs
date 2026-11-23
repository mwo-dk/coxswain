//! The native-program kind (#863 decisions 2, 10, 11) as a client: the same calls as JSON
//! messages to `spike-proc`, over its stdin/stdout or over HTTP on localhost. A message is one
//! JSON line; `load` sends the files' raw bytes after it (their sizes in the line), not inside
//! the JSON.
use crate::{Inst, Job, Row};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, BufWriter, Read, Write};
use std::net::TcpStream;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

fn exe() -> std::path::PathBuf {
    let me = std::env::current_exe().unwrap();
    me.with_file_name(if cfg!(windows) { "spike-proc.exe" } else { "spike-proc" })
}

pub struct Stdio_ {
    child: Child,
    w: BufWriter<ChildStdin>,
    r: BufReader<ChildStdout>,
}

impl Stdio_ {
    pub fn spawn() -> Stdio_ {
        let mut child = Command::new(exe()).stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().expect("spike-proc");
        let w = BufWriter::new(child.stdin.take().unwrap());
        let r = BufReader::new(child.stdout.take().unwrap());
        Stdio_ { child, w, r }
    }
    fn call(&mut self, msg: &Value, bytes: &[&[u8]]) -> Value {
        serde_json::to_writer(&mut self.w, msg).unwrap();
        self.w.write_all(b"\n").unwrap();
        for b in bytes {
            self.w.write_all(b).unwrap();
        }
        self.w.flush().unwrap();
        let mut line = String::new();
        self.r.read_line(&mut line).unwrap();
        serde_json::from_str(&line).expect("reply")
    }
}

impl Drop for Stdio_ {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub struct Http {
    child: Child,
    s: BufReader<TcpStream>,
}

impl Http {
    pub fn spawn() -> Http {
        let mut child = Command::new(exe()).arg("--http").stdout(Stdio::piped()).spawn().expect("spike-proc --http");
        // The program says its port on its first line.
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let s = TcpStream::connect(line.trim()).expect("connect");
        s.set_nodelay(true).unwrap();
        Http { child, s: BufReader::new(s) }
    }
    fn call(&mut self, msg: &Value, bytes: &[&[u8]]) -> Value {
        let mut body = serde_json::to_vec(msg).unwrap();
        body.push(b'\n');
        let len = body.len() + bytes.iter().map(|b| b.len()).sum::<usize>();
        let s = self.s.get_mut();
        write!(s, "POST /call HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {len}\r\n\r\n").unwrap();
        s.write_all(&body).unwrap();
        for b in bytes {
            s.write_all(b).unwrap();
        }
        s.flush().unwrap();
        let mut n = 0usize;
        loop {
            let mut h = String::new();
            self.s.read_line(&mut h).unwrap();
            let h = h.trim_end();
            if h.is_empty() {
                break;
            }
            if let Some(v) = h.to_ascii_lowercase().strip_prefix("content-length:") {
                n = v.trim().parse().unwrap();
            }
        }
        let mut b = vec![0; n];
        self.s.read_exact(&mut b).unwrap();
        serde_json::from_slice(&b).expect("reply")
    }
}

impl Drop for Http {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn load_msg(job: Job, files: &[&[u8]]) -> Value {
    json!({"op": "load", "job": job, "sizes": files.iter().map(|f| f.len()).collect::<Vec<_>>()})
}

fn rows(v: Value) -> Vec<Row> {
    serde_json::from_value(v["rows"].clone()).expect("rows")
}

macro_rules! inst {
    ($t:ty) => {
        impl Inst for $t {
            fn split(&mut self, _: Job, _: &[&[u8]], _: u32) -> Vec<(u64, u64)> {
                unimplemented!()
            }
            fn work(&mut self, _: Job, _: &[&[u8]], _: (u64, u64)) -> Vec<Row> {
                unimplemented!()
            }
            fn load(&mut self, job: Job, files: &[&[u8]]) -> u32 {
                self.call(&load_msg(job, files), files)["count"].as_u64().unwrap() as u32
            }
            fn page(&mut self, start: u32, count: u32) -> Vec<Row> {
                rows(self.call(&json!({"op": "page", "start": start, "count": count}), &[]))
            }
            fn positions(&mut self, pgn: &[u8]) -> Vec<String> {
                let v = self.call(&json!({"op": "positions", "sizes": [pgn.len()]}), &[pgn]);
                serde_json::from_value(v["fens"].clone()).unwrap()
            }
        }
    };
}
inst!(Stdio_);
inst!(Http);
