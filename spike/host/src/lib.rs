//! The spike's host: one reader run natively, as a component in wasmtime (Cranelift or
//! Pulley), as a core module in wasmi, or as a separate program (`proc`).

pub mod proc;
#[cfg(feature = "wasmi")]
pub mod wasmi_rt;
#[cfg(feature = "wasmtime")]
pub mod wasmtime_rt;

pub use spike_games::{Job, Row};

/// One instance of a reader. A plugin instance has its own memory; one per thread.
pub trait Inst: Send {
    fn split(&mut self, job: Job, files: &[&[u8]], parts: u32) -> Vec<(u64, u64)>;
    /// `files` whole; PGN jobs cut `span` out of `files[0]` (a plugin is handed only that).
    fn work(&mut self, job: Job, files: &[&[u8]], span: (u64, u64)) -> Vec<Row>;
    fn load(&mut self, job: Job, files: &[&[u8]]) -> u32;
    fn page(&mut self, start: u32, count: u32) -> Vec<Row>;
    fn positions(&mut self, pgn: &[u8]) -> Vec<String>;
}

pub trait Runtime: Sync {
    fn instance(&self) -> Box<dyn Inst>;
}

pub fn is_pgn(job: Job) -> bool {
    matches!(job, Job::PgnScan | Job::PgnParse)
}

/// What a plugin is handed for one chunk: a PGN chunk's own bytes, or the SCID files whole.
pub fn handed<'a>(job: Job, files: &[&'a [u8]], span: (u64, u64)) -> (Vec<&'a [u8]>, (u64, u64)) {
    if is_pgn(job) {
        let c = &files[0][span.0 as usize..span.1 as usize];
        (vec![c], (0, c.len() as u64))
    } else {
        (files.to_vec(), span)
    }
}

pub struct Native;

#[derive(Default)]
pub struct NativeInst {
    rows: Vec<Row>,
}

impl Runtime for Native {
    fn instance(&self) -> Box<dyn Inst> {
        Box::new(NativeInst::default())
    }
}

impl Inst for NativeInst {
    fn split(&mut self, job: Job, files: &[&[u8]], parts: u32) -> Vec<(u64, u64)> {
        spike_games::split(job, files, parts)
    }
    fn work(&mut self, job: Job, files: &[&[u8]], span: (u64, u64)) -> Vec<Row> {
        spike_games::work(job, files, span)
    }
    fn load(&mut self, job: Job, files: &[&[u8]]) -> u32 {
        self.rows = spike_games::all(job, files);
        self.rows.len() as u32
    }
    fn page(&mut self, start: u32, count: u32) -> Vec<Row> {
        let s = (start as usize).min(self.rows.len());
        self.rows[s..(s + count as usize).min(self.rows.len())].to_vec()
    }
    fn positions(&mut self, pgn: &[u8]) -> Vec<String> {
        spike_games::positions(pgn)
    }
}

/// Rows' count, plies and a hash of every string, to prove two runtimes read the same.
pub fn checksum(rows: &[Row]) -> (usize, u64, u64) {
    let mut h: u64 = 0xcbf29ce484222325;
    let mut plies = 0u64;
    for r in rows {
        for s in [&r.white, &r.black, &r.event, &r.site, &r.date, &r.result, &r.eco] {
            for b in s.bytes().chain([0]) {
                h = (h ^ b as u64).wrapping_mul(0x100000001b3);
            }
        }
        plies += r.plies as u64;
    }
    (rows.len(), plies, h)
}

/// Split, one instance per thread on each chunk, merge (#863 decision 9). The merge keeps the
/// chunks' order. Returns the rows and the time spent making instances.
pub fn run(rt: &dyn Runtime, job: Job, files: &[&[u8]], threads: u32) -> (Vec<Row>, std::time::Duration) {
    let mut first = rt.instance();
    let spans = if threads <= 1 {
        vec![(0, if is_pgn(job) { files[0].len() as u64 } else { spike_games::chess::scid::count_of(files[0]) as u64 })]
    } else {
        first.split(job, files, threads)
    };
    let inst_time = std::sync::Mutex::new(std::time::Duration::ZERO);
    let parts: Vec<Vec<Row>> = std::thread::scope(|s| {
        let hs: Vec<_> = spans
            .iter()
            .enumerate()
            .map(|(i, &span)| {
                let inst_time = &inst_time;
                let reuse = (i == 0).then(|| std::mem::replace(&mut first, Box::new(NativeInst::default())));
                s.spawn(move || {
                    let t = std::time::Instant::now();
                    let mut inst = reuse.unwrap_or_else(|| rt.instance());
                    *inst_time.lock().unwrap() += t.elapsed();
                    inst.work(job, files, span)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    (parts.concat(), inst_time.into_inner().unwrap())
}
