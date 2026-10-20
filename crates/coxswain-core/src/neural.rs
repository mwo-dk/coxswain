//! Neural network models (`.onnx`, `.tflite`, `.mlmodel`, `.mlpackage`, `.keras`, Keras `.h5`):
//! what a model is, for both apps' facts. Its format and the program that made it, its
//! operator set or format version, its inputs and outputs with their shapes, how many nodes
//! and parameters it has. Read from the file's own structure (protocol buffers, a flat
//! buffer, JSON), never run: no pickle is read, no code of the model's is executed. Every
//! count and every length is capped, so a broken or hostile file cannot make it read forever.
//! The desktop app also draws the graph (Netron, in `gui/public/vendor/netron`).

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use crate::t;

/// The most protocol buffer fields read of one model: past it the model counts as unreadable.
const MAX_FIELDS: u64 = 8_000_000;
/// The largest message read into memory whole (an input's type, a node); larger ones are skipped.
const MAX_MESSAGE: u64 = 4 << 20;
/// The most inputs or outputs kept (all are counted).
const MAX_PORTS: usize = 64;
/// The most kinds of operator counted.
const MAX_OPS: usize = 4096;
/// The most bytes of a Keras HDF5 file looked through for its model's configuration.
const H5_SCAN: u64 = 16 << 20;
/// The largest JSON file read of a Keras archive.
const MAX_JSON: u64 = 32 << 20;

/// What a model is.
#[derive(Debug, Default, PartialEq)]
pub struct Model {
    /// "ONNX", "TensorFlow Lite", "Core ML", "Keras".
    pub format: String,
    /// The format's own version: ONNX's IR version, TensorFlow Lite's schema, Core ML's
    /// specification, the Keras version.
    pub version: String,
    /// What made it: "pytorch 2.5.1", "MLIR Converted.", "coremltools 8.1 (torch==2.5.1)".
    pub producer: String,
    /// ONNX's operator sets: "ai.onnx 17".
    pub opsets: Vec<String>,
    /// What kind of model: Core ML's ("neural network", "ML program"), Keras's class.
    pub kind: String,
    pub inputs: Vec<Port>,
    pub outputs: Vec<Port>,
    /// How many inputs and outputs there are, when more than `MAX_PORTS`.
    pub input_count: usize,
    pub output_count: usize,
    /// Nodes (ONNX), operators (TensorFlow Lite, ML program), layers (Core ML, Keras).
    pub nodes: Option<u64>,
    /// The number of weights stored in it, where the format says so.
    pub parameters: Option<u64>,
    /// The operators most used, most first.
    pub ops: Vec<(String, u64)>,
}

/// An input or an output: its name and type, e.g. `float32[1,3,224,224]`.
#[derive(Debug, Default, PartialEq, Clone)]
pub struct Port {
    pub name: String,
    pub kind: String,
}

impl std::fmt::Display for Port {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.kind.is_empty() { write!(f, "{}", self.name) } else { write!(f, "{} {}", self.name, self.kind) }
    }
}

fn ending(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase()
}

/// A model by its name: one of the endings, and a `.mlpackage` is a folder. Nothing is read.
pub fn named(path: &Path) -> bool {
    match ending(path).as_str() {
        "onnx" | "tflite" | "mlmodel" | "keras" | "h5" => !path.is_dir(),
        "mlpackage" => path.is_dir(),
        _ => false,
    }
}

/// The model at `path`; `None` for a file that is none (an HDF5 file of no Keras model), one
/// that cannot be read, and one only in the cloud (reading it would download it).
pub fn read(path: &Path) -> Option<Model> {
    if !named(path) || crate::cloud::unasked(path) {
        return None;
    }
    let mut m = match ending(path).as_str() {
        "onnx" => onnx(&mut Pb::open(path)?)?,
        "tflite" => tflite(&mut Fb::open(path)?)?,
        "mlmodel" => coreml(&mut Pb::open(path)?)?,
        "mlpackage" => {
            let mut m = coreml(&mut Pb::open(&package_model(path)?)?)?;
            m.format = t!("neural.coreml_package");
            m
        }
        "keras" => keras_archive(path)?,
        "h5" => keras_h5(path)?,
        _ => return None,
    };
    m.input_count = m.input_count.max(m.inputs.len());
    m.output_count = m.output_count.max(m.outputs.len());
    m.inputs.truncate(MAX_PORTS);
    m.outputs.truncate(MAX_PORTS);
    Some(m)
}

/// A `.mlpackage`'s model file, as its `Manifest.json` names it (inside the package only).
pub fn package_model(dir: &Path) -> Option<PathBuf> {
    let manifest: serde_json::Value = serde_json::from_slice(&read_capped(&dir.join("Manifest.json"), 1 << 20)?).ok()?;
    let root = manifest.get("rootModelIdentifier")?.as_str()?;
    let rel = manifest.get("itemInfoEntries")?.get(root)?.get("path")?.as_str()?;
    let rel = Path::new(rel);
    if !rel.components().all(|c| matches!(c, std::path::Component::Normal(_))) {
        return None;
    }
    Some(dir.join("Data").join(rel))
}

fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let mut b = vec![];
    File::open(path).ok()?.take(max + 1).read_to_end(&mut b).ok()?;
    (b.len() as u64 <= max).then_some(b)
}

/// "25.6 M (25557032)": big numbers short, and exact.
fn count(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1} k ({n})", n as f64 / 1e3),
        1_000_000..1_000_000_000 => format!("{:.1} M ({n})", n as f64 / 1e6),
        _ => format!("{:.2} G ({n})", n as f64 / 1e9),
    }
}

/// The ports as one line: the first four, and how many more.
fn ports(list: &[Port], total: usize) -> String {
    let mut s = list.iter().take(4).map(Port::to_string).collect::<Vec<_>>().join(", ");
    if total > 4 {
        s += &format!(", {}", t!("neural.more", "n" => total - 4));
    }
    s
}

/// The model's facts as (label, value) lines, each a single line.
pub fn facts(m: &Model) -> Vec<(String, String)> {
    let mut out = vec![(t!("facts.format"), if m.version.is_empty() { m.format.clone() } else { format!("{} {}", m.format, m.version) })];
    let mut add = |k: String, v: String| {
        if !v.is_empty() {
            out.push((k, v));
        }
    };
    add(t!("facts.made_with"), m.producer.clone());
    add(t!("neural.opsets"), m.opsets.join(", "));
    add(t!("facts.kind"), m.kind.clone());
    add(t!("neural.inputs"), ports(&m.inputs, m.input_count));
    add(t!("neural.outputs"), ports(&m.outputs, m.output_count));
    add(t!("neural.nodes"), m.nodes.map(count).unwrap_or_default());
    add(t!("neural.parameters"), m.parameters.map(count).unwrap_or_default());
    add(t!("neural.operators"), m.ops.iter().take(5).map(|(o, n)| format!("{o} × {n}")).collect::<Vec<_>>().join(", "));
    out
}

/// The model as text, for the terminal app's viewer (F3): its facts, then every input and
/// output on a line of its own.
pub fn text(name: &str, m: &Model) -> String {
    let f = facts(m);
    let w = f.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let mut out = format!("{name}\n\n");
    for (k, v) in &f {
        out += &format!("{k}{}  {v}\n", " ".repeat(w - k.chars().count()));
    }
    for (label, list, total) in [(t!("neural.inputs"), &m.inputs, m.input_count), (t!("neural.outputs"), &m.outputs, m.output_count)] {
        if list.is_empty() {
            continue;
        }
        out += &format!("\n{label}\n");
        let w = list.iter().map(|p| p.name.chars().count()).max().unwrap_or(0);
        for p in list.iter() {
            out += &format!("  {}{}  {}\n", p.name, " ".repeat(w - p.name.chars().count()), p.kind);
        }
        if total > list.len() {
            out += &format!("  {}\n", t!("neural.more", "n" => total - list.len()));
        }
    }
    if !m.ops.is_empty() {
        out += &format!("\n{}\n", t!("neural.operators"));
        for (o, n) in m.ops.iter().take(40) {
            out += &format!("  {n:>6}  {o}\n");
        }
    }
    out + &format!("\n{}\n", t!("neural.tui"))
}

/// Counts operators by name, up to `MAX_OPS` kinds; `sorted` gives them most used first.
#[derive(Default)]
struct Ops(HashMap<String, u64>);

impl Ops {
    fn add(&mut self, name: &str) {
        if let Some(n) = self.0.get_mut(name) {
            *n += 1;
        } else if self.0.len() < MAX_OPS {
            self.0.insert(name.to_string(), 1);
        }
    }
    fn sorted(self) -> Vec<(String, u64)> {
        let mut v: Vec<_> = self.0.into_iter().collect();
        v.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        v
    }
}

fn shape(dims: &[String]) -> String {
    format!("[{}]", dims.join(","))
}

// ---------------------------------------------------------------- protocol buffers

/// A protocol buffer field's value. A length-delimited field's bytes are not read yet: the
/// caller takes them (`bytes`), skips them, or reads the message inside up to `pos + len`.
enum W {
    Int(u64),
    Len(u64),
    Fixed,
}

/// Reads protocol buffers field by field from a file or from bytes, skipping what is not
/// wanted without reading it.
struct Pb<R> {
    r: R,
    pos: u64,
    len: u64,
    budget: u64,
}

impl Pb<BufReader<File>> {
    fn open(path: &Path) -> Option<Self> {
        let f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        Some(Pb { r: BufReader::with_capacity(64 << 10, f), pos: 0, len, budget: MAX_FIELDS })
    }
}

impl<'a> Pb<std::io::Cursor<&'a [u8]>> {
    fn of(b: &'a [u8]) -> Self {
        Pb { r: std::io::Cursor::new(b), pos: 0, len: b.len() as u64, budget: MAX_FIELDS }
    }
}

impl<R: BufRead + Seek> Pb<R> {
    fn byte(&mut self) -> Option<u8> {
        let mut b = [0];
        self.r.read_exact(&mut b).ok()?;
        self.pos += 1;
        Some(b[0])
    }
    fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for i in 0..10 {
            let b = self.byte()?;
            v |= u64::from(b & 0x7f) << (7 * i);
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    fn skip(&mut self, n: u64) -> Option<()> {
        if self.pos.checked_add(n)? > self.len {
            return None;
        }
        let buffered = self.r.fill_buf().ok()?.len() as u64;
        if n <= buffered {
            self.r.consume(n as usize);
        } else {
            self.r.seek(SeekFrom::Current(n as i64)).ok()?;
        }
        self.pos += n;
        Some(())
    }
    fn bytes(&mut self, n: u64) -> Option<Vec<u8>> {
        if n > MAX_MESSAGE || self.pos + n > self.len {
            return None;
        }
        let mut b = vec![0; n as usize];
        self.r.read_exact(&mut b).ok()?;
        self.pos += n;
        Some(b)
    }
    fn string(&mut self, n: u64) -> Option<String> {
        Some(String::from_utf8_lossy(&self.bytes(n)?).into_owned())
    }
    /// The next field before `end`: its number and value; `None` at `end` and on broken bytes.
    fn field(&mut self, end: u64) -> Option<(u64, W)> {
        if self.pos >= end || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        let key = self.varint()?;
        let w = match key & 7 {
            0 => W::Int(self.varint()?),
            1 => {
                self.skip(8)?;
                W::Fixed
            }
            5 => {
                self.skip(4)?;
                W::Fixed
            }
            2 => {
                let n = self.varint()?;
                if self.pos.checked_add(n)? > end.min(self.len) {
                    return None;
                }
                W::Len(n)
            }
            _ => return None,
        };
        Some((key >> 3, w))
    }
    /// A message's fields until `end`, each given to `f`, which reads or skips a `Len` itself
    /// (or returns `false` to have it skipped). `None` when the bytes are broken.
    fn each(&mut self, end: u64, mut f: impl FnMut(&mut Self, u64, &W) -> Option<bool>) -> Option<()> {
        while self.pos < end {
            let (n, w) = self.field(end)?;
            let at = self.pos;
            let used = f(self, n, &w)?;
            if let W::Len(len) = w {
                if !used {
                    self.skip(len)?;
                } else if self.pos != at + len {
                    return None;
                }
            }
        }
        (self.pos == end).then_some(())
    }
    /// Varints of a repeated field, packed or not.
    fn ints(&mut self, w: &W, out: &mut Vec<i64>) -> Option<()> {
        match *w {
            W::Int(v) => out.push(v as i64),
            W::Len(n) => {
                let end = self.pos + n;
                while self.pos < end {
                    if out.len() >= 64 {
                        return self.skip(end - self.pos);
                    }
                    out.push(self.varint()? as i64);
                }
            }
            W::Fixed => {}
        }
        Some(())
    }
}

// ---------------------------------------------------------------- ONNX

fn onnx_type(t: i64) -> &'static str {
    const NAMES: [&str; 24] = [
        "undefined", "float32", "uint8", "int8", "uint16", "int16", "int32", "int64", "string", "bool", "float16", "float64",
        "uint32", "uint64", "complex64", "complex128", "bfloat16", "float8e4m3fn", "float8e4m3fnuz", "float8e5m2",
        "float8e5m2fnuz", "uint4", "int4", "float4e2m1",
    ];
    NAMES.get(t as usize).copied().unwrap_or("?")
}

/// An ONNX `TypeProto` as `float32[1,3,224,224]`.
fn onnx_value_type<R: BufRead + Seek>(p: &mut Pb<R>, end: u64) -> Option<String> {
    let mut out = String::new();
    p.each(end, |p, n, w| {
        let W::Len(len) = *w else { return Some(false) };
        let end = p.pos + len;
        match n {
            // A tensor, or a sparse one.
            1 | 8 => {
                let (mut elem, mut dims) = (0, None::<Vec<String>>);
                p.each(end, |p, n, w| match (n, w) {
                    (1, W::Int(v)) => {
                        elem = *v as i64;
                        Some(true)
                    }
                    (2, W::Len(len)) => {
                        let mut d = vec![];
                        let end = p.pos + len;
                        p.each(end, |p, n, w| {
                            let (1, W::Len(len)) = (n, w) else { return Some(false) };
                            let mut dim = "?".to_string();
                            p.each(p.pos + len, |p, n, w| match (n, w) {
                                (1, W::Int(v)) => {
                                    dim = (*v as i64).to_string();
                                    Some(true)
                                }
                                (2, W::Len(len)) => {
                                    dim = p.string(*len)?;
                                    Some(true)
                                }
                                _ => Some(false),
                            })?;
                            if d.len() < 16 {
                                d.push(dim);
                            }
                            Some(true)
                        })?;
                        dims = Some(d);
                        Some(true)
                    }
                    _ => Some(false),
                })?;
                if n == 8 {
                    out += "sparse ";
                }
                out += onnx_type(elem);
                if let Some(d) = dims {
                    out += &shape(&d);
                }
                Some(true)
            }
            4 => {
                out = "sequence".into();
                Some(false)
            }
            5 => {
                out = "map".into();
                Some(false)
            }
            9 => {
                out = "optional".into();
                Some(false)
            }
            _ => Some(false),
        }
    })?;
    Some(out)
}

/// An ONNX `ValueInfoProto`, from its bytes.
fn onnx_port(b: &[u8]) -> Option<Port> {
    let mut p = Pb::of(b);
    let mut port = Port::default();
    p.each(b.len() as u64, |p, n, w| match (n, w) {
        (1, W::Len(len)) => {
            port.name = p.string(*len)?;
            Some(true)
        }
        (2, W::Len(len)) => {
            port.kind = onnx_value_type(p, p.pos + len)?;
            Some(true)
        }
        _ => Some(false),
    })?;
    Some(port)
}

fn onnx<R: BufRead + Seek>(p: &mut Pb<R>) -> Option<Model> {
    let mut m = Model { format: "ONNX".into(), ..Default::default() };
    let (mut ir, mut name, mut version, mut graph_seen) = (None, String::new(), String::new(), false);
    let mut inputs = vec![];
    let mut initializers = std::collections::HashSet::new();
    let (mut nodes, mut params) = (0u64, 0u64);
    let mut ops = Ops::default();
    let end = p.len;
    p.each(end, |p, n, w| match (n, w) {
        (1, W::Int(v)) => {
            ir = Some(*v);
            Some(true)
        }
        (2, W::Len(len)) => {
            name = p.string(*len)?;
            Some(true)
        }
        (3, W::Len(len)) => {
            version = p.string(*len)?;
            Some(true)
        }
        (8, W::Len(len)) => {
            let (mut domain, mut v) = (String::new(), 0);
            p.each(p.pos + len, |p, n, w| match (n, w) {
                (1, W::Len(len)) => {
                    domain = p.string(*len)?;
                    Some(true)
                }
                (2, W::Int(x)) => {
                    v = *x;
                    Some(true)
                }
                _ => Some(false),
            })?;
            if m.opsets.len() < 16 {
                m.opsets.push(format!("{} {v}", if domain.is_empty() { "ai.onnx" } else { &domain }));
            }
            Some(true)
        }
        (7, W::Len(len)) => {
            graph_seen = true;
            p.each(p.pos + len, |p, n, w| match (n, w) {
                (1, W::Len(len)) => {
                    nodes += 1;
                    let (mut op, mut domain) = (String::new(), String::new());
                    p.each(p.pos + len, |p, n, w| match (n, w) {
                        (4, W::Len(len)) => {
                            op = p.string(*len)?;
                            Some(true)
                        }
                        (7, W::Len(len)) => {
                            domain = p.string(*len)?;
                            Some(true)
                        }
                        _ => Some(false),
                    })?;
                    ops.add(&if domain.is_empty() || domain == "ai.onnx" { op } else { format!("{domain}.{op}") });
                    Some(true)
                }
                // Initializers: their dimensions and names; the data is skipped.
                (5, W::Len(len)) => {
                    let mut dims = vec![];
                    p.each(p.pos + len, |p, n, w| match (n, w) {
                        (1, _) => {
                            p.ints(w, &mut dims)?;
                            Some(true)
                        }
                        (8, W::Len(len)) => {
                            if initializers.len() < 1_000_000 {
                                initializers.insert(p.string(*len)?);
                                return Some(true);
                            }
                            Some(false)
                        }
                        _ => Some(false),
                    })?;
                    params = params.saturating_add(dims.iter().map(|&d| d.max(0) as u64).fold(1u64, u64::saturating_mul));
                    Some(true)
                }
                (11 | 12, W::Len(len)) => {
                    let list = if n == 11 { &mut inputs } else { &mut m.outputs };
                    if *len > MAX_MESSAGE {
                        return Some(false);
                    }
                    let b = p.bytes(*len)?;
                    if list.len() < 100_000 {
                        list.push(onnx_port(&b)?);
                    }
                    Some(true)
                }
                _ => Some(false),
            })?;
            Some(true)
        }
        _ => Some(false),
    })?;
    // A protocol buffer with no ir_version and no graph is not ONNX.
    if ir.is_none() && !graph_seen {
        return None;
    }
    m.version = ir.map(|v| t!("neural.ir", "n" => v)).unwrap_or_default();
    m.producer = format!("{name} {version}").trim().to_string();
    // Older models list their weights among the inputs too.
    m.inputs = inputs.into_iter().filter(|i| !initializers.contains(&i.name)).collect();
    m.nodes = Some(nodes);
    m.parameters = Some(params);
    m.ops = ops.sorted();
    Some(m)
}

// ---------------------------------------------------------------- Core ML

fn coreml_feature(b: &[u8]) -> Option<Port> {
    let mut p = Pb::of(b);
    let mut port = Port::default();
    p.each(b.len() as u64, |p, n, w| match (n, w) {
        (1, W::Len(len)) => {
            port.name = p.string(*len)?;
            Some(true)
        }
        (3, W::Len(len)) => {
            p.each(p.pos + len, |p, n, w| {
                let simple = match n {
                    1 => "int64",
                    2 => "double",
                    3 => "string",
                    6 => "dictionary",
                    7 => "sequence",
                    8 => "state",
                    _ => "",
                };
                if !simple.is_empty() {
                    port.kind = simple.into();
                    return Some(false);
                }
                let W::Len(len) = *w else { return Some(false) };
                let end = p.pos + len;
                match n {
                    // An image: width, height and colours.
                    4 => {
                        let (mut wd, mut ht, mut cs) = (0, 0, 0);
                        p.each(end, |_, n, w| {
                            if let W::Int(v) = w {
                                match n {
                                    1 => wd = *v,
                                    2 => ht = *v,
                                    3 => cs = *v,
                                    _ => {}
                                }
                            }
                            Some(false)
                        })?;
                        let colours = match cs {
                            10 => " grayscale",
                            20 => " RGB",
                            30 => " BGR",
                            40 => " grayscale float16",
                            _ => "",
                        };
                        port.kind = format!("image {wd}×{ht}{colours}");
                        Some(true)
                    }
                    // A multi-array: element type and shape.
                    5 => {
                        let (mut dims, mut dt) = (vec![], 0);
                        p.each(end, |p, n, w| match n {
                            1 => {
                                p.ints(w, &mut dims)?;
                                Some(true)
                            }
                            2 => {
                                if let W::Int(v) = w {
                                    dt = *v;
                                }
                                Some(false)
                            }
                            _ => Some(false),
                        })?;
                        let ty = match dt {
                            65568 => "float32",
                            65600 => "float64",
                            131104 => "int32",
                            131080 => "int8",
                            65552 => "float16",
                            _ => "array",
                        };
                        port.kind = format!("{ty}{}", shape(&dims.iter().map(i64::to_string).collect::<Vec<_>>()));
                        Some(true)
                    }
                    _ => Some(false),
                }
            })?;
            Some(true)
        }
        _ => Some(false),
    })?;
    Some(port)
}

/// Counts an ML program's operations (in its first function's block) by type.
fn mil<R: BufRead + Seek>(p: &mut Pb<R>, end: u64, ops: &mut Ops, n_ops: &mut u64) -> Option<()> {
    let mut done = false;
    // Program.functions: map entries (key, Function).
    p.each(end, |p, n, w| {
        let (2, W::Len(len)) = (n, w) else { return Some(false) };
        if done {
            return Some(false);
        }
        done = true;
        p.each(p.pos + len, |p, n, w| {
            let (2, W::Len(len)) = (n, w) else { return Some(false) };
            // Function.block_specializations: entries (opset, Block).
            let mut block_done = false;
            p.each(p.pos + len, |p, n, w| {
                let (3, W::Len(len)) = (n, w) else { return Some(false) };
                if block_done {
                    return Some(false);
                }
                block_done = true;
                p.each(p.pos + len, |p, n, w| {
                    let (2, W::Len(len)) = (n, w) else { return Some(false) };
                    p.each(p.pos + len, |p, n, w| {
                        let (3, W::Len(len)) = (n, w) else { return Some(false) };
                        *n_ops += 1;
                        p.each(p.pos + len, |p, n, w| match (n, w) {
                            (1, W::Len(len)) => {
                                ops.add(&p.string(*len)?);
                                Some(true)
                            }
                            _ => Some(false),
                        })?;
                        Some(true)
                    })?;
                    Some(true)
                })?;
                Some(true)
            })?;
            Some(true)
        })?;
        Some(true)
    })
}

fn coreml<R: BufRead + Seek>(p: &mut Pb<R>) -> Option<Model> {
    let mut m = Model { format: "Core ML".into(), ..Default::default() };
    let (mut spec, mut tools, mut source, mut layers) = (None, String::new(), String::new(), None::<u64>);
    let mut ops = Ops::default();
    let mut n_ops = 0;
    let end = p.len;
    p.each(end, |p, n, w| match (n, w) {
        (1, W::Int(v)) => {
            spec = Some(*v);
            Some(true)
        }
        (2, W::Len(len)) => {
            p.each(p.pos + len, |p, n, w| match (n, w) {
                (1 | 10, W::Len(len)) => {
                    let b = p.bytes(*len)?;
                    let list = if n == 1 { &mut m.inputs } else { &mut m.outputs };
                    if list.len() < 100_000 {
                        list.push(coreml_feature(&b)?);
                    }
                    Some(true)
                }
                // Metadata: coremltools' version and what the model was converted from.
                (100, W::Len(len)) => {
                    p.each(p.pos + len, |p, n, w| {
                        let (100, W::Len(len)) = (n, w) else { return Some(false) };
                        let (mut k, mut v) = (String::new(), String::new());
                        p.each(p.pos + len, |p, n, w| match (n, w) {
                            (1, W::Len(len)) => {
                                k = p.string(*len)?;
                                Some(true)
                            }
                            (2, W::Len(len)) => {
                                v = p.string(*len)?;
                                Some(true)
                            }
                            _ => Some(false),
                        })?;
                        match k.as_str() {
                            "com.github.apple.coremltools.version" => tools = v,
                            "com.github.apple.coremltools.source" => source = v,
                            _ => {}
                        }
                        Some(true)
                    })?;
                    Some(true)
                }
                _ => Some(false),
            })?;
            Some(true)
        }
        (_, W::Len(len)) if n >= 200 => {
            m.kind = coreml_kind(n).into();
            match n {
                // Neural networks: their layers counted, each skipped.
                303 | 403 | 500 => {
                    let mut c = 0;
                    p.each(p.pos + len, |_, n, _| {
                        if n == 1 {
                            c += 1;
                        }
                        Some(false)
                    })?;
                    layers = Some(c);
                    Some(true)
                }
                502 => {
                    mil(p, p.pos + len, &mut ops, &mut n_ops)?;
                    Some(true)
                }
                // Pipelines: how many models.
                200..=202 => {
                    let mut c = 0;
                    p.each(p.pos + len, |p, n, w| {
                        let (1, W::Len(len)) = (n, w) else { return Some(false) };
                        p.each(p.pos + len, |_, n, _| {
                            if n == 1 {
                                c += 1;
                            }
                            Some(false)
                        })?;
                        Some(true)
                    })?;
                    m.kind = t!("neural.pipeline", "n" => c);
                    Some(true)
                }
                _ => Some(false),
            }
        }
        _ => Some(false),
    })?;
    let spec = spec?;
    m.version = t!("neural.specification", "n" => spec);
    m.producer = match (tools.is_empty(), source.is_empty()) {
        (false, false) => format!("coremltools {tools} ({source})"),
        (false, true) => format!("coremltools {tools}"),
        (true, false) => source,
        (true, true) => String::new(),
    };
    m.nodes = layers.or((n_ops > 0).then_some(n_ops));
    m.ops = ops.sorted();
    Some(m)
}

fn coreml_kind(n: u64) -> &'static str {
    match n {
        200 => "pipeline classifier",
        201 => "pipeline regressor",
        202 => "pipeline",
        300 => "GLM regressor",
        301 => "support vector regressor",
        302 => "tree ensemble regressor",
        303 => "neural network regressor",
        304 => "Bayesian probit regressor",
        400 => "GLM classifier",
        401 => "support vector classifier",
        402 => "tree ensemble classifier",
        403 => "neural network classifier",
        404 => "k-nearest neighbours classifier",
        500 => "neural network",
        501 => "item similarity recommender",
        502 => "ML program",
        555 => "custom model",
        556 => "linked model",
        560 => "class converter",
        600 => "one-hot encoder",
        601 => "imputer",
        602 => "feature vectorizer",
        603 => "dictionary vectorizer",
        604 => "scaler",
        606 => "categorical mapping",
        607 => "normalizer",
        609 => "array feature extractor",
        610 => "non-maximum suppression",
        900 => "identity",
        2000 => "text classifier",
        2001 => "word tagger",
        2002 => "vision feature print",
        2003 => "sound analysis preprocessing",
        2004 => "gazetteer",
        2005 => "word embedding",
        2006 => "audio feature print",
        3000 => "serialized model",
        _ => "",
    }
}

// ---------------------------------------------------------------- TensorFlow Lite

/// Reads a flat buffer from a file at any offset, through a few cached 64 KiB pages.
struct Fb {
    f: File,
    len: u64,
    pages: HashMap<u64, Vec<u8>>,
    budget: u64,
}

const PAGE: u64 = 64 << 10;

impl Fb {
    fn open(path: &Path) -> Option<Self> {
        let f = File::open(path).ok()?;
        let len = f.metadata().ok()?.len();
        Some(Fb { f, len, pages: HashMap::new(), budget: MAX_FIELDS })
    }
    fn bytes<const N: usize>(&mut self, at: u64) -> Option<[u8; N]> {
        if at.checked_add(N as u64)? > self.len || self.budget == 0 {
            return None;
        }
        self.budget -= 1;
        let mut out = [0; N];
        for (i, o) in out.iter_mut().enumerate() {
            let a = at + i as u64;
            let page = a / PAGE;
            if !self.pages.contains_key(&page) {
                if self.pages.len() >= 256 {
                    self.pages.clear();
                }
                let mut b = vec![0; PAGE.min(self.len - page * PAGE) as usize];
                self.f.seek(SeekFrom::Start(page * PAGE)).ok()?;
                self.f.read_exact(&mut b).ok()?;
                self.pages.insert(page, b);
            }
            *o = self.pages[&page][(a % PAGE) as usize];
        }
        Some(out)
    }
    fn u8(&mut self, at: u64) -> Option<u8> {
        Some(self.bytes::<1>(at)?[0])
    }
    fn u16(&mut self, at: u64) -> Option<u16> {
        Some(u16::from_le_bytes(self.bytes(at)?))
    }
    fn u32(&mut self, at: u64) -> Option<u32> {
        Some(u32::from_le_bytes(self.bytes(at)?))
    }
    fn u64(&mut self, at: u64) -> Option<u64> {
        Some(u64::from_le_bytes(self.bytes(at)?))
    }
    /// Where field `i` of the table at `t` is, if it is there.
    fn field(&mut self, t: u64, i: u64) -> Option<Option<u64>> {
        let vt = (t as i64).checked_sub(i64::from(self.u32(t)? as i32))?;
        let vt = u64::try_from(vt).ok()?;
        let size = u64::from(self.u16(vt)?);
        if 4 + 2 * i + 2 > size {
            return Some(None);
        }
        let off = u64::from(self.u16(vt + 4 + 2 * i)?);
        Some((off != 0).then_some(t + off))
    }
    /// Where an offset at `at` points.
    fn deref(&mut self, at: u64) -> Option<u64> {
        at.checked_add(u64::from(self.u32(at)?))
    }
    /// A vector's start and length, of the field `i` of the table `t`.
    fn vector(&mut self, t: u64, i: u64) -> Option<(u64, u64)> {
        let Some(at) = self.field(t, i)? else { return Some((0, 0)) };
        let v = self.deref(at)?;
        Some((v + 4, u64::from(self.u32(v)?)))
    }
    fn string(&mut self, t: u64, i: u64) -> Option<String> {
        let (start, len) = self.vector(t, i)?;
        let len = len.min(4096);
        let mut s = Vec::with_capacity(len as usize);
        for k in 0..len {
            s.push(self.u8(start + k)?);
        }
        Some(String::from_utf8_lossy(&s).into_owned())
    }
    /// The `k`th table of a vector of tables starting at `start`.
    fn table_at(&mut self, start: u64, k: u64) -> Option<u64> {
        self.deref(start + 4 * k)
    }
    fn ints(&mut self, t: u64, i: u64) -> Option<Vec<i32>> {
        let (start, len) = self.vector(t, i)?;
        (0..len.min(64)).map(|k| self.u32(start + 4 * k).map(|v| v as i32)).collect()
    }
    fn scalar_u32(&mut self, t: u64, i: u64) -> Option<u32> {
        match self.field(t, i)? {
            Some(at) => self.u32(at),
            None => Some(0),
        }
    }
}

fn tflite_type(t: u8) -> &'static str {
    const NAMES: [&str; 19] = [
        "float32", "float16", "int32", "uint8", "int64", "string", "bool", "int16", "complex64", "int8", "float64",
        "complex128", "uint64", "resource", "variant", "uint32", "uint16", "int4", "bfloat16",
    ];
    NAMES.get(t as usize).copied().unwrap_or("?")
}

fn tflite(b: &mut Fb) -> Option<Model> {
    if b.bytes::<4>(4)? != *b"TFL3" {
        return None;
    }
    let root = b.deref(0)?;
    let mut m = Model { format: "TensorFlow Lite".into(), ..Default::default() };
    m.version = t!("neural.schema", "n" => b.scalar_u32(root, 0)?);
    m.producer = b.string(root, 3)?.trim().to_string();
    // Operator names, by index.
    let (codes, n_codes) = b.vector(root, 1)?;
    let mut names = Vec::with_capacity(n_codes.min(4096) as usize);
    for k in 0..n_codes.min(4096) {
        let c = b.table_at(codes, k)?;
        let old = match b.field(c, 0)? {
            Some(at) => i32::from(b.u8(at)? as i8),
            None => 0,
        };
        let new = match b.field(c, 3)? {
            Some(at) => b.u32(at)? as i32,
            None => 0,
        };
        let code = old.max(new);
        names.push(if code == 32 {
            b.string(c, 1)?
        } else {
            TFLITE_OPS.get(code as usize).map_or_else(|| format!("op {code}"), |s| s.to_string())
        });
    }
    // Which buffers hold data: the tensors that use them are weights.
    let (bufs, n_bufs) = b.vector(root, 4)?;
    let mut filled = vec![false; n_bufs.min(10_000_000) as usize];
    for (k, f) in filled.iter_mut().enumerate() {
        let t = b.table_at(bufs, k as u64)?;
        let data = b.vector(t, 0)?.1;
        let size = match b.field(t, 2)? {
            Some(at) => b.u64(at)?,
            None => 0,
        };
        *f = data > 0 || size > 0;
    }
    let (subs, n_subs) = b.vector(root, 2)?;
    let (mut nodes, mut params) = (0u64, 0u64);
    let mut ops = Ops::default();
    for s in 0..n_subs.min(1024) {
        let g = b.table_at(subs, s)?;
        let (tensors, n_tensors) = b.vector(g, 0)?;
        let port = |b: &mut Fb, k: i32| -> Option<Port> {
            let t = b.table_at(tensors, u64::try_from(k).ok().filter(|&k| k < n_tensors)?)?;
            // The shape with -1 where it may change, if the file says.
            let sig = b.ints(t, 7)?;
            let dims = if sig.is_empty() { b.ints(t, 0)? } else { sig };
            let ty = match b.field(t, 1)? {
                Some(at) => b.u8(at)?,
                None => 0,
            };
            let dims: Vec<String> = dims.iter().map(|&d| if d < 0 { "?".into() } else { d.to_string() }).collect();
            Some(Port { name: b.string(t, 3)?, kind: format!("{}{}", tflite_type(ty), shape(&dims)) })
        };
        // The first subgraph is the model; the others are the bodies of its loops and conditions.
        if s == 0 {
            for k in b.ints(g, 1)? {
                m.inputs.push(port(b, k)?);
            }
            for k in b.ints(g, 2)? {
                m.outputs.push(port(b, k)?);
            }
            m.input_count = b.vector(g, 1)?.1 as usize;
            m.output_count = b.vector(g, 2)?.1 as usize;
        }
        for k in 0..n_tensors.min(10_000_000) {
            let t = b.table_at(tensors, k)?;
            let buffer = b.scalar_u32(t, 2)? as usize;
            if buffer > 0 && filled.get(buffer).copied().unwrap_or(false) {
                let dims = b.ints(t, 0)?;
                params = params.saturating_add(dims.iter().map(|&d| d.max(0) as u64).fold(1u64, u64::saturating_mul));
            }
        }
        let (opv, n_op) = b.vector(g, 3)?;
        nodes += n_op;
        for k in 0..n_op.min(10_000_000) {
            let o = b.table_at(opv, k)?;
            let idx = b.scalar_u32(o, 0)? as usize;
            ops.add(names.get(idx).map_or("?", String::as_str));
        }
    }
    m.nodes = Some(nodes);
    m.parameters = Some(params);
    m.ops = ops.sorted();
    Some(m)
}

// ---------------------------------------------------------------- Keras

/// A Keras model's configuration (`config.json`, or an HDF5 file's `model_config`).
fn keras_config(m: &mut Model, c: &serde_json::Value) -> Option<()> {
    m.kind = c.get("class_name")?.as_str()?.to_string();
    let layers = c.get("config")?.get("layers")?.as_array()?;
    m.nodes = Some(layers.len() as u64);
    let dims = |v: &serde_json::Value| -> Option<String> {
        let d: Vec<String> = v.as_array()?.iter().take(16).map(|x| x.as_i64().map_or("?".into(), |n| n.to_string())).collect();
        Some(shape(&d))
    };
    for l in layers {
        let cfg = l.get("config");
        let name = cfg.and_then(|c| c.get("name")).and_then(|n| n.as_str()).unwrap_or_default();
        // An input: its own layer (Keras 3 batch_shape, Keras 2 batch_input_shape), or a
        // Sequential model's first layer with batch_input_shape.
        let batch = cfg.and_then(|c| c.get("batch_shape").or_else(|| c.get("batch_input_shape")));
        if let Some(b) = batch.filter(|_| l.get("class_name").and_then(|n| n.as_str()) == Some("InputLayer") || m.inputs.is_empty())
            && m.inputs.len() < MAX_PORTS
        {
            let dtype = cfg.and_then(|c| c.get("dtype")).and_then(|d| d.as_str().or_else(|| d.get("config")?.get("name")?.as_str())).unwrap_or("float32");
            m.inputs.push(Port { name: name.into(), kind: format!("{dtype}{}", dims(b).unwrap_or_default()) });
        }
    }
    if m.inputs.is_empty()
        && let Some(s) = c.get("build_config").and_then(|b| b.get("input_shape")).and_then(dims)
    {
        m.inputs.push(Port { name: "input".into(), kind: s });
    }
    Some(())
}

fn keras_archive(path: &Path) -> Option<Model> {
    let mut z = zip::ZipArchive::new(BufReader::new(File::open(path).ok()?)).ok()?;
    let mut json = |name: &str| -> Option<serde_json::Value> {
        let f = z.by_name(name).ok()?;
        let mut b = vec![];
        f.take(MAX_JSON).read_to_end(&mut b).ok()?;
        serde_json::from_slice(&b).ok()
    };
    let config = json("config.json")?;
    let meta = json("metadata.json");
    let mut m = Model { format: "Keras".into(), ..Default::default() };
    m.version = meta.as_ref().and_then(|v| v.get("keras_version")?.as_str().map(str::to_string)).unwrap_or_default();
    keras_config(&mut m, &config)?;
    Some(m)
}

/// A Keras HDF5 file: its `model_config` attribute is JSON stored as it is, found in the
/// file's first bytes. An HDF5 file without one is no Keras model.
fn keras_h5(path: &Path) -> Option<Model> {
    let mut b = vec![];
    File::open(path).ok()?.take(H5_SCAN).read_to_end(&mut b).ok()?;
    if !b.starts_with(b"\x89HDF\r\n\x1a\n") {
        return None;
    }
    let mut m = Model { format: t!("neural.keras_h5"), ..Default::default() };
    let mut at = 0;
    while let Some(i) = find(&b[at..], b"{\"class_name\"") {
        let start = at + i;
        if let Some(Ok(v)) = serde_json::Deserializer::from_slice(&b[start..]).into_iter::<serde_json::Value>().next()
            && v.get("config").and_then(|c| c.get("layers")).is_some()
            && keras_config(&mut m, &v).is_some()
        {
            // keras_version is a short string attribute next to it.
            if let Some(i) = find(&b, b"keras_version") {
                let tail = &b[i + 13..(i + 160).min(b.len())];
                if let Some(v) = tail.windows(1).position(|w| w[0].is_ascii_digit()).map(|p| &tail[p..])
                    && let Some(end) = v.iter().position(|c| !(c.is_ascii_alphanumeric() || *c == b'.'))
                    && v[..end].contains(&b'.')
                {
                    m.version = String::from_utf8_lossy(&v[..end]).into_owned();
                }
            }
            return Some(m);
        }
        at = start + 1;
    }
    None
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// TensorFlow Lite's built-in operators, by code (schema.fbs `BuiltinOperator`).
const TFLITE_OPS: &[&str] = &[
    "ADD", "AVERAGE_POOL_2D", "CONCATENATION", "CONV_2D", "DEPTHWISE_CONV_2D", "DEPTH_TO_SPACE", "DEQUANTIZE",
    "EMBEDDING_LOOKUP", "FLOOR", "FULLY_CONNECTED", "HASHTABLE_LOOKUP", "L2_NORMALIZATION", "L2_POOL_2D",
    "LOCAL_RESPONSE_NORMALIZATION", "LOGISTIC", "LSH_PROJECTION", "LSTM", "MAX_POOL_2D", "MUL", "RELU",
    "RELU_N1_TO_1", "RELU6", "RESHAPE", "RESIZE_BILINEAR", "RNN", "SOFTMAX", "SPACE_TO_DEPTH", "SVDF", "TANH",
    "CONCAT_EMBEDDINGS", "SKIP_GRAM", "CALL", "CUSTOM", "EMBEDDING_LOOKUP_SPARSE", "PAD",
    "UNIDIRECTIONAL_SEQUENCE_RNN", "GATHER", "BATCH_TO_SPACE_ND", "SPACE_TO_BATCH_ND", "TRANSPOSE", "MEAN", "SUB",
    "DIV", "SQUEEZE", "UNIDIRECTIONAL_SEQUENCE_LSTM", "STRIDED_SLICE", "BIDIRECTIONAL_SEQUENCE_RNN", "EXP",
    "TOPK_V2", "SPLIT", "LOG_SOFTMAX", "DELEGATE", "BIDIRECTIONAL_SEQUENCE_LSTM", "CAST", "PRELU", "MAXIMUM",
    "ARG_MAX", "MINIMUM", "LESS", "NEG", "PADV2", "GREATER", "GREATER_EQUAL", "LESS_EQUAL", "SELECT", "SLICE",
    "SIN", "TRANSPOSE_CONV", "SPARSE_TO_DENSE", "TILE", "EXPAND_DIMS", "EQUAL", "NOT_EQUAL", "LOG", "SUM", "SQRT",
    "RSQRT", "SHAPE", "POW", "ARG_MIN", "FAKE_QUANT", "REDUCE_PROD", "REDUCE_MAX", "PACK", "LOGICAL_OR", "ONE_HOT",
    "LOGICAL_AND", "LOGICAL_NOT", "UNPACK", "REDUCE_MIN", "FLOOR_DIV", "REDUCE_ANY", "SQUARE", "ZEROS_LIKE", "FILL",
    "FLOOR_MOD", "RANGE", "RESIZE_NEAREST_NEIGHBOR", "LEAKY_RELU", "SQUARED_DIFFERENCE", "MIRROR_PAD", "ABS",
    "SPLIT_V", "UNIQUE", "CEIL", "REVERSE_V2", "ADD_N", "GATHER_ND", "COS", "WHERE", "RANK", "ELU",
    "REVERSE_SEQUENCE", "MATRIX_DIAG", "QUANTIZE", "MATRIX_SET_DIAG", "ROUND", "HARD_SWISH", "IF", "WHILE",
    "NON_MAX_SUPPRESSION_V4", "NON_MAX_SUPPRESSION_V5", "SCATTER_ND", "SELECT_V2", "DENSIFY", "SEGMENT_SUM",
    "BATCH_MATMUL", "PLACEHOLDER_FOR_GREATER_OP_CODES", "CUMSUM", "CALL_ONCE", "BROADCAST_TO", "RFFT2D", "CONV_3D",
    "IMAG", "REAL", "COMPLEX_ABS", "HASHTABLE", "HASHTABLE_FIND", "HASHTABLE_IMPORT", "HASHTABLE_SIZE",
    "REDUCE_ALL", "CONV_3D_TRANSPOSE", "VAR_HANDLE", "READ_VARIABLE", "ASSIGN_VARIABLE", "BROADCAST_ARGS",
    "RANDOM_STANDARD_NORMAL", "BUCKETIZE", "RANDOM_UNIFORM", "MULTINOMIAL", "GELU", "DYNAMIC_UPDATE_SLICE",
    "RELU_0_TO_1", "UNSORTED_SEGMENT_PROD", "UNSORTED_SEGMENT_MAX", "UNSORTED_SEGMENT_SUM", "ATAN2",
    "UNSORTED_SEGMENT_MIN", "SIGN", "BITCAST", "BITWISE_XOR", "RIGHT_SHIFT", "STABLEHLO_LOGISTIC", "STABLEHLO_ADD",
    "STABLEHLO_DIVIDE", "STABLEHLO_MULTIPLY", "STABLEHLO_MAXIMUM", "STABLEHLO_RESHAPE", "STABLEHLO_CLAMP",
    "STABLEHLO_CONCATENATE", "STABLEHLO_BROADCAST_IN_DIM", "STABLEHLO_CONVOLUTION", "STABLEHLO_SLICE",
    "STABLEHLO_CUSTOM_CALL", "STABLEHLO_REDUCE", "STABLEHLO_ABS", "STABLEHLO_AND", "STABLEHLO_COSINE",
    "STABLEHLO_EXPONENTIAL", "STABLEHLO_FLOOR", "STABLEHLO_LOG", "STABLEHLO_MINIMUM", "STABLEHLO_NEGATE",
    "STABLEHLO_OR", "STABLEHLO_POWER", "STABLEHLO_REMAINDER", "STABLEHLO_RSQRT", "STABLEHLO_SELECT",
    "STABLEHLO_SUBTRACT", "STABLEHLO_TANH", "STABLEHLO_SCATTER", "STABLEHLO_COMPARE", "STABLEHLO_CONVERT",
    "STABLEHLO_DYNAMIC_SLICE", "STABLEHLO_DYNAMIC_UPDATE_SLICE", "STABLEHLO_PAD", "STABLEHLO_IOTA",
    "STABLEHLO_DOT_GENERAL", "STABLEHLO_REDUCE_WINDOW", "STABLEHLO_SORT", "STABLEHLO_WHILE", "STABLEHLO_GATHER",
    "STABLEHLO_TRANSPOSE", "DILATE", "STABLEHLO_RNG_BIT_GENERATOR", "REDUCE_WINDOW", "STABLEHLO_COMPOSITE",
    "STABLEHLO_SHIFT_LEFT", "STABLEHLO_CBRT", "STABLEHLO_CASE",
];

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    // A small protocol buffer writer for the tests.
    pub fn varint(mut n: u64, out: &mut Vec<u8>) {
        loop {
            let b = (n & 0x7f) as u8;
            n >>= 7;
            if n == 0 {
                out.push(b);
                return;
            }
            out.push(b | 0x80);
        }
    }
    pub fn int(f: u64, v: u64) -> Vec<u8> {
        let mut o = vec![];
        varint(f << 3, &mut o);
        varint(v, &mut o);
        o
    }
    pub fn len(f: u64, b: impl AsRef<[u8]>) -> Vec<u8> {
        let b = b.as_ref();
        let mut o = vec![];
        varint(f << 3 | 2, &mut o);
        varint(b.len() as u64, &mut o);
        o.extend_from_slice(b);
        o
    }
    fn cat(parts: &[Vec<u8>]) -> Vec<u8> {
        parts.concat()
    }

    fn onnx_port(name: &str, dims: &[&str]) -> Vec<u8> {
        let shape: Vec<Vec<u8>> = dims
            .iter()
            .map(|d| len(1, d.parse::<u64>().map_or_else(|_| len(2, d), |n| int(1, n))))
            .collect();
        cat(&[len(1, name), len(2, len(1, cat(&[int(1, 1), len(2, shape.concat())])))])
    }

    /// A small ONNX model: Conv and Relu, two initializers (one also listed as an input, as
    /// older exporters do).
    pub fn onnx_bytes() -> Vec<u8> {
        let node = |op: &str| len(1, cat(&[len(1, "x"), len(2, "y"), len(4, op)]));
        let init = |name: &str, dims: &[u64], packed: bool| {
            let d = if packed {
                let mut p = vec![];
                for &x in dims {
                    varint(x, &mut p);
                }
                len(1, p)
            } else {
                dims.iter().map(|&x| int(1, x)).collect::<Vec<_>>().concat()
            };
            len(5, cat(&[d, int(2, 1), len(8, name), len(9, vec![0u8; 64])]))
        };
        let graph = cat(&[
            node("Conv"),
            node("Relu"),
            node("Relu"),
            len(2, "net"),
            init("w", &[8, 3, 3, 3], true),
            init("b", &[8], false),
            len(11, onnx_port("image", &["batch", "3", "32", "32"])),
            len(11, onnx_port("w", &["8", "3", "3", "3"])),
            len(12, onnx_port("scores", &["batch", "8"])),
        ]);
        cat(&[int(1, 8), len(2, "pytorch"), len(3, "2.5.1"), len(8, cat(&[len(1, ""), int(2, 17)])), len(7, graph)])
    }

    pub fn coreml_bytes() -> Vec<u8> {
        let array = |name: &str, dims: &[u64]| {
            let mut p = vec![];
            for &x in dims {
                varint(x, &mut p);
            }
            cat(&[len(1, name), len(3, len(5, cat(&[len(1, p), int(2, 65568)])))])
        };
        let entry = |k: &str, v: &str| len(100, cat(&[len(1, k), len(2, v)]));
        let desc = cat(&[
            len(1, array("telemetry", &[1, 16])),
            len(10, array("thrust", &[1, 4])),
            len(100, cat(&[entry("com.github.apple.coremltools.version", "8.1"), entry("com.github.apple.coremltools.source", "torch==2.5.1")])),
        ]);
        let layer = |name: &str| len(1, cat(&[len(1, name), len(130, len(10, b""))]));
        cat(&[int(1, 4), len(2, desc), len(500, cat(&[layer("dense"), layer("relu")]))])
    }

    /// A flat buffer written front to back: each table's vtable just before it, children after.
    pub struct Fbw(pub Vec<u8>);
    pub enum F {
        U8(u8),
        U32(u32),
        Str(String),
        Ints(Vec<i32>),
        Bytes(Vec<u8>),
        Tables(Vec<Vec<(u16, F)>>),
    }
    impl Fbw {
        fn align(&mut self, n: usize) {
            while !self.0.len().is_multiple_of(n) {
                self.0.push(0);
            }
        }
        fn put32(&mut self, at: usize, v: u32) {
            self.0[at..at + 4].copy_from_slice(&v.to_le_bytes());
        }
        pub fn table(&mut self, fields: &[(u16, F)]) -> usize {
            let n = fields.iter().map(|(i, _)| *i as usize + 1).max().unwrap_or(0);
            self.align(2);
            let vt = self.0.len();
            self.0.extend(std::iter::repeat_n(0, 4 + 2 * n));
            self.align(4);
            let t = self.0.len();
            let soff = (t - vt) as u32;
            self.0.extend(soff.to_le_bytes());
            let mut children = vec![];
            for (i, f) in fields {
                let at = self.0.len();
                match f {
                    F::U8(v) => self.0.push(*v),
                    F::U32(v) => self.0.extend(v.to_le_bytes()),
                    _ => {
                        self.0.extend([0; 4]);
                        children.push((at, f));
                    }
                }
                let off = (at - t) as u16;
                self.0[vt + 4 + 2 * *i as usize..vt + 6 + 2 * *i as usize].copy_from_slice(&off.to_le_bytes());
                self.align(4);
            }
            let size = (self.0.len() - t) as u16;
            self.0[vt..vt + 2].copy_from_slice(&((4 + 2 * n) as u16).to_le_bytes());
            self.0[vt + 2..vt + 4].copy_from_slice(&size.to_le_bytes());
            for (at, f) in children {
                self.align(4);
                let c = self.0.len();
                match f {
                    F::Str(s) => {
                        self.0.extend((s.len() as u32).to_le_bytes());
                        self.0.extend(s.as_bytes());
                        self.0.push(0);
                    }
                    F::Ints(v) => {
                        self.0.extend((v.len() as u32).to_le_bytes());
                        v.iter().for_each(|x| self.0.extend(x.to_le_bytes()));
                    }
                    F::Bytes(v) => {
                        self.0.extend((v.len() as u32).to_le_bytes());
                        self.0.extend(v);
                    }
                    F::Tables(list) => {
                        self.0.extend((list.len() as u32).to_le_bytes());
                        let slots = self.0.len();
                        self.0.extend(std::iter::repeat_n(0, 4 * list.len()));
                        for (k, tb) in list.iter().enumerate() {
                            let p = self.table(tb);
                            self.put32(slots + 4 * k, (p - (slots + 4 * k)) as u32);
                        }
                    }
                    _ => unreachable!(),
                }
                self.put32(at, (c - at) as u32);
            }
            t
        }
    }

    pub fn tflite_bytes() -> Vec<u8> {
        let tensor = |name: &str, shape: Vec<i32>, buffer: u32| vec![(0, F::Ints(shape)), (1, F::U8(0)), (2, F::U32(buffer)), (3, F::Str(name.into()))];
        let model = vec![
            (0, F::U32(3)),
            (1, F::Tables(vec![vec![(0, F::U8(9)), (3, F::U32(9))], vec![(0, F::U8(25)), (3, F::U32(25))]])),
            (
                2,
                F::Tables(vec![vec![
                    (
                        0,
                        F::Tables(vec![
                            tensor("serving_default_x:0", vec![1, 8], 0),
                            tensor("dense/MatMul", vec![4, 8], 1),
                            tensor("dense/BiasAdd", vec![4], 2),
                            tensor("dense", vec![1, 4], 0),
                            tensor("Identity", vec![1, 4], 0),
                        ]),
                    ),
                    (1, F::Ints(vec![0])),
                    (2, F::Ints(vec![4])),
                    (3, F::Tables(vec![vec![(0, F::U32(0)), (1, F::Ints(vec![0, 1, 2])), (2, F::Ints(vec![3]))], vec![(0, F::U32(1)), (1, F::Ints(vec![3])), (2, F::Ints(vec![4]))]])),
                    (4, F::Str("main".into())),
                ]]),
            ),
            (3, F::Str("MLIR Converted.".into())),
            (4, F::Tables(vec![vec![], vec![(0, F::Bytes(vec![0; 128]))], vec![(0, F::Bytes(vec![0; 16]))]])),
        ];
        let mut w = Fbw(vec![0; 8]);
        w.0[4..8].copy_from_slice(b"TFL3");
        let root = w.table(&model);
        w.put32(0, root as u32);
        w.0
    }

    fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-neural-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn onnx_facts() {
        let d = folder("onnx");
        let p = d.join("net.onnx");
        std::fs::write(&p, onnx_bytes()).unwrap();
        let m = read(&p).unwrap();
        assert_eq!(m.producer, "pytorch 2.5.1");
        assert_eq!(m.opsets, ["ai.onnx 17"]);
        // The weight listed as an input is no input.
        assert_eq!(m.inputs, [Port { name: "image".into(), kind: "float32[batch,3,32,32]".into() }]);
        assert_eq!(m.outputs[0].kind, "float32[batch,8]");
        assert_eq!(m.nodes, Some(3));
        assert_eq!(m.parameters, Some(8 * 27 + 8));
        assert_eq!(m.ops[0], ("Relu".into(), 2));
        let f = facts(&m);
        assert!(f.iter().any(|(_, v)| v == "image float32[batch,3,32,32]"), "{f:?}");
        assert!(text("net.onnx", &m).contains("scores  float32[batch,8]"));
        // Cut short anywhere, or random bytes: no facts, no panic.
        let b = onnx_bytes();
        for n in [1, 5, 20, b.len() / 2, b.len() - 1] {
            std::fs::write(&p, &b[..n]).unwrap();
            let _ = read(&p);
        }
        std::fs::write(&p, b"not a model at all").unwrap();
        assert!(read(&p).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn tflite_facts() {
        let d = folder("tflite");
        let p = d.join("lander.tflite");
        std::fs::write(&p, tflite_bytes()).unwrap();
        let m = read(&p).unwrap();
        assert_eq!(m.producer, "MLIR Converted.");
        assert_eq!(m.inputs, [Port { name: "serving_default_x:0".into(), kind: "float32[1,8]".into() }]);
        assert_eq!(m.outputs[0].name, "Identity");
        assert_eq!(m.nodes, Some(2));
        assert_eq!(m.parameters, Some(36));
        assert!(m.ops.iter().any(|(o, _)| o == "FULLY_CONNECTED") && m.ops.iter().any(|(o, _)| o == "SOFTMAX"), "{:?}", m.ops);
        let b = tflite_bytes();
        for n in [8, 12, b.len() / 2, b.len() - 1] {
            std::fs::write(&p, &b[..n]).unwrap();
            let _ = read(&p);
        }
        // A vtable offset pointing far away: refused, not followed.
        let mut bad = b.clone();
        let root = u32::from_le_bytes(bad[0..4].try_into().unwrap()) as usize;
        bad[root..root + 4].copy_from_slice(&i32::MIN.to_le_bytes());
        std::fs::write(&p, &bad).unwrap();
        assert!(read(&p).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn coreml_and_package_facts() {
        let d = folder("coreml");
        let p = d.join("Thrust.mlmodel");
        std::fs::write(&p, coreml_bytes()).unwrap();
        let m = read(&p).unwrap();
        assert_eq!(m.producer, "coremltools 8.1 (torch==2.5.1)");
        assert_eq!(m.kind, "neural network");
        assert_eq!(m.nodes, Some(2));
        assert_eq!(m.inputs[0].to_string(), "telemetry float32[1,16]");
        // A package: its manifest names the model; a path out of it is refused.
        let pkg = d.join("Thrust.mlpackage");
        std::fs::create_dir_all(pkg.join("Data/com.apple.CoreML")).unwrap();
        std::fs::write(pkg.join("Data/com.apple.CoreML/model.mlmodel"), coreml_bytes()).unwrap();
        let manifest = |path: &str| format!(r#"{{"rootModelIdentifier":"A","itemInfoEntries":{{"A":{{"path":"{path}"}}}}}}"#);
        std::fs::write(pkg.join("Manifest.json"), manifest("com.apple.CoreML/model.mlmodel")).unwrap();
        assert!(named(&pkg));
        assert_eq!(read(&pkg).unwrap().outputs[0].name, "thrust");
        std::fs::write(pkg.join("Manifest.json"), manifest("../Thrust.mlmodel")).unwrap();
        assert!(read(&pkg).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn keras_facts() {
        let d = folder("keras");
        let config = r#"{"module":"keras","class_name":"Sequential","config":{"name":"lander","layers":[
            {"class_name":"InputLayer","config":{"name":"telemetry","batch_shape":[null,16],"dtype":"float32"}},
            {"class_name":"Dense","config":{"name":"dense","units":4}}]}}"#;
        let p = d.join("lander.keras");
        let mut z = zip::ZipWriter::new(File::create(&p).unwrap());
        z.start_file("config.json", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, config.as_bytes()).unwrap();
        z.start_file("metadata.json", zip::write::SimpleFileOptions::default()).unwrap();
        std::io::Write::write_all(&mut z, br#"{"keras_version":"3.6.0"}"#).unwrap();
        z.finish().unwrap();
        let m = read(&p).unwrap();
        assert_eq!((m.version.as_str(), m.kind.as_str(), m.nodes), ("3.6.0", "Sequential", Some(2)));
        assert_eq!(m.inputs[0].to_string(), "telemetry float32[?,16]");
        // HDF5: the configuration found in the file's bytes; an HDF5 file of no model has none.
        let h5 = d.join("lander.h5");
        let mut b = b"\x89HDF\r\n\x1a\n".to_vec();
        b.extend(vec![0; 100]);
        b.extend(b"keras_version\0\0\x052.15.0\0");
        // As Keras 2 writes it: json.dumps, class_name first.
        b.extend(config.replace(r#""module":"keras","#, "").replace(r#"{"class_name":"Sequential""#, r#"{"class_name": "Sequential""#).as_bytes());
        std::fs::write(&h5, &b).unwrap();
        let m = read(&h5).unwrap();
        assert_eq!((m.version.as_str(), m.nodes), ("2.15.0", Some(2)));
        std::fs::write(&h5, &b[..200]).unwrap();
        assert!(read(&h5).is_none());
        std::fs::remove_dir_all(d).unwrap();
    }
}
