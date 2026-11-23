//! The component in wasmtime: compiled once, one store and instance per thread.
use crate::{Inst, Job, Row, Runtime, handed};
use wasmtime::component::{Component, Linker};
use wasmtime::{Config, Engine, Store};

wasmtime::component::bindgen!({ world: "plugin", path: "../wit" });

use exports::coxswain::spike::grid as wgrid;
use exports::coxswain::spike::reader as wr;

pub struct Wasmtime {
    engine: Engine,
    pre: PluginPre<()>,
}

impl Wasmtime {
    /// `pulley`: Wasmtime's interpreter instead of the machine's code.
    pub fn new(wasm: &[u8], pulley: bool) -> wasmtime::Result<Wasmtime> {
        #[allow(unused_mut)]
        let mut c = Config::new();
        if pulley {
            #[cfg(feature = "pulley")]
            {
                c.target(if cfg!(target_pointer_width = "64") { "pulley64" } else { "pulley32" })?;
            }
            #[cfg(not(feature = "pulley"))]
            wasmtime::bail!("built without the pulley feature");
        }
        let engine = Engine::new(&c)?;
        let component = Component::new(&engine, wasm)?;
        let linker = Linker::new(&engine);
        let pre = PluginPre::new(linker.instantiate_pre(&component)?)?;
        Ok(Wasmtime { engine, pre })
    }

    pub fn instance_typed(&self) -> WInst {
        let mut store = Store::new(&self.engine, ());
        let p = self.pre.instantiate(&mut store).expect("instantiate");
        WInst { store, p }
    }
}

pub struct WInst {
    store: Store<()>,
    p: Plugin,
}

fn job(j: Job) -> wr::Job {
    match j {
        Job::PgnScan => wr::Job::PgnScan,
        Job::PgnParse => wr::Job::PgnParse,
        Job::ScidTable => wr::Job::ScidTable,
        Job::ScidDecode => wr::Job::ScidDecode,
    }
}

fn row(r: wr::Row) -> Row {
    Row { white: r.white, black: r.black, event: r.event, site: r.site, date: r.date, result: r.result, eco: r.eco, plies: r.plies }
}

impl Runtime for Wasmtime {
    fn instance(&self) -> Box<dyn Inst> {
        Box::new(self.instance_typed())
    }
}

impl Inst for WInst {
    fn split(&mut self, j: Job, files: &[&[u8]], parts: u32) -> Vec<(u64, u64)> {
        let owned: Vec<Vec<u8>> = files.iter().map(|f| f.to_vec()).collect();
        let s = self.p.coxswain_spike_reader().call_split(&mut self.store, job(j), &owned, parts).expect("split");
        s.into_iter().map(|s| (s.start, s.end)).collect()
    }
    fn work(&mut self, j: Job, files: &[&[u8]], span: (u64, u64)) -> Vec<Row> {
        let (files, span) = handed(j, files, span);
        let owned: Vec<Vec<u8>> = files.iter().map(|f| f.to_vec()).collect();
        let r = self.p.coxswain_spike_reader().call_work(&mut self.store, job(j), &owned, wr::Span { start: span.0, end: span.1 }).expect("work");
        r.into_iter().map(row).collect()
    }
    fn load(&mut self, j: Job, files: &[&[u8]]) -> u32 {
        let owned: Vec<Vec<u8>> = files.iter().map(|f| f.to_vec()).collect();
        self.p.coxswain_spike_reader().call_load(&mut self.store, job(j), &owned).expect("load")
    }
    fn page(&mut self, start: u32, count: u32) -> Vec<Row> {
        self.p.coxswain_spike_reader().call_page(&mut self.store, start, count).expect("page").into_iter().map(row).collect()
    }
    fn positions(&mut self, pgn: &[u8]) -> Vec<String> {
        self.p.coxswain_spike_reader().call_positions(&mut self.store, pgn).expect("positions")
    }
}

pub use wgrid::{Cell, Colour};

impl WInst {
    pub fn grid_open(&mut self, pgn: &[u8]) -> u32 {
        self.p.coxswain_spike_grid().call_open(&mut self.store, pgn).expect("open")
    }
    pub fn grid_draw(&mut self, w: u16, h: u16) -> Vec<Cell> {
        self.p.coxswain_spike_grid().call_draw(&mut self.store, w, h).expect("draw")
    }
    pub fn grid_key(&mut self, k: &str) -> bool {
        self.p.coxswain_spike_grid().call_key(&mut self.store, k).expect("key")
    }
}
