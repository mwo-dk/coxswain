//! The plugin's core module in wasmi, which has no component model: the canonical ABI of the
//! `.wit` functions done by hand here (what wasmtime's bindgen generates).
use crate::{Inst, Job, Row, Runtime, handed};
use wasmi::{Engine, Instance, Linker, Memory, Module, Store, TypedFunc};

const IFACE: &str = "coxswain:spike/reader@0.1.0";

pub struct Wasmi {
    engine: Engine,
    module: Module,
}

impl Wasmi {
    pub fn new(core: &[u8]) -> Wasmi {
        let engine = Engine::default();
        let module = Module::new(&engine, core).expect("module");
        Wasmi { engine, module }
    }
}

pub struct WiInst {
    store: Store<()>,
    inst: Instance,
    mem: Memory,
    realloc: TypedFunc<(i32, i32, i32, i32), i32>,
}

impl Runtime for Wasmi {
    fn instance(&self) -> Box<dyn Inst> {
        let mut store = Store::new(&self.engine, ());
        let linker = Linker::<()>::new(&self.engine);
        let inst = linker.instantiate_and_start(&mut store, &self.module).expect("instantiate");
        let mem = inst.get_memory(&store, "memory").expect("memory");
        let realloc = inst.get_typed_func(&store, "cabi_realloc").expect("cabi_realloc");
        Box::new(WiInst { store, inst, mem, realloc })
    }
}

fn job(j: Job) -> i32 {
    match j {
        Job::PgnScan => 0,
        Job::PgnParse => 1,
        Job::ScidTable => 2,
        Job::ScidDecode => 3,
    }
}

impl WiInst {
    fn alloc(&mut self, align: i32, size: usize) -> i32 {
        self.realloc.call(&mut self.store, (0, 0, align, size as i32)).expect("alloc")
    }
    fn put(&mut self, at: i32, b: &[u8]) {
        self.mem.write(&mut self.store, at as usize, b).expect("write");
    }
    fn u32_at(&self, at: usize) -> u32 {
        let d = self.mem.data(&self.store);
        u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
    }
    fn u64_at(&self, at: usize) -> u64 {
        let d = self.mem.data(&self.store);
        u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
    }
    fn string_at(&self, at: usize) -> String {
        let (p, l) = (self.u32_at(at) as usize, self.u32_at(at + 4) as usize);
        String::from_utf8(self.mem.data(&self.store)[p..p + l].to_vec()).expect("utf-8")
    }
    /// list<list<u8>>: each list's bytes, then the (pointer, length) pairs.
    fn files(&mut self, files: &[&[u8]]) -> (i32, i32) {
        let mut pairs = Vec::with_capacity(files.len() * 8);
        for f in files {
            let p = self.alloc(1, f.len().max(1));
            self.put(p, f);
            pairs.extend_from_slice(&(p as u32).to_le_bytes());
            pairs.extend_from_slice(&(f.len() as u32).to_le_bytes());
        }
        let p = self.alloc(4, pairs.len().max(1));
        self.put(p, &pairs);
        (p, files.len() as i32)
    }
    /// list<row> at the return area `ret`; then the guest's post-return frees it.
    fn rows(&mut self, ret: i32, name: &str) -> Vec<Row> {
        let (p, n) = (self.u32_at(ret as usize) as usize, self.u32_at(ret as usize + 4) as usize);
        let rows = (0..n)
            .map(|i| {
                let a = p + i * 60;
                Row {
                    white: self.string_at(a),
                    black: self.string_at(a + 8),
                    event: self.string_at(a + 16),
                    site: self.string_at(a + 24),
                    date: self.string_at(a + 32),
                    result: self.string_at(a + 40),
                    eco: self.string_at(a + 48),
                    plies: self.u32_at(a + 56),
                }
            })
            .collect();
        self.post(ret, name);
        rows
    }
    fn post(&mut self, ret: i32, name: &str) {
        if let Ok(f) = self.inst.get_typed_func::<i32, ()>(&self.store, &format!("cabi_post_{IFACE}#{name}")) {
            f.call(&mut self.store, ret).expect("post-return");
        }
    }
    fn func<P: wasmi::WasmParams, R: wasmi::WasmResults>(&self, name: &str) -> TypedFunc<P, R> {
        self.inst.get_typed_func(&self.store, &format!("{IFACE}#{name}")).expect(name)
    }
}

impl Inst for WiInst {
    fn split(&mut self, j: Job, files: &[&[u8]], parts: u32) -> Vec<(u64, u64)> {
        let (p, n) = self.files(files);
        let f = self.func::<(i32, i32, i32, i32), i32>("split");
        let ret = f.call(&mut self.store, (job(j), p, n, parts as i32)).expect("split");
        let (sp, sn) = (self.u32_at(ret as usize) as usize, self.u32_at(ret as usize + 4) as usize);
        let v = (0..sn).map(|i| (self.u64_at(sp + 16 * i), self.u64_at(sp + 16 * i + 8))).collect();
        self.post(ret, "split");
        v
    }
    fn work(&mut self, j: Job, files: &[&[u8]], span: (u64, u64)) -> Vec<Row> {
        let (files, span) = handed(j, files, span);
        let (p, n) = self.files(&files);
        let f = self.func::<(i32, i32, i32, i64, i64), i32>("work");
        let ret = f.call(&mut self.store, (job(j), p, n, span.0 as i64, span.1 as i64)).expect("work");
        self.rows(ret, "work")
    }
    fn load(&mut self, j: Job, files: &[&[u8]]) -> u32 {
        let (p, n) = self.files(files);
        let f = self.func::<(i32, i32, i32), i32>("load");
        f.call(&mut self.store, (job(j), p, n)).expect("load") as u32
    }
    fn page(&mut self, start: u32, count: u32) -> Vec<Row> {
        let f = self.func::<(i32, i32), i32>("page");
        let ret = f.call(&mut self.store, (start as i32, count as i32)).expect("page");
        self.rows(ret, "page")
    }
    fn positions(&mut self, _pgn: &[u8]) -> Vec<String> {
        unimplemented!("not measured with wasmi")
    }
}
