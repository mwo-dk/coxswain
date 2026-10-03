//! Everything-style file name index.
//!
//! Every name lives in one `\0`-separated byte buffer (plus a lowercased twin), and each
//! node is 12 bytes: parent id, name offset, name length, flags. A query is one SIMD
//! `memmem` pass over the buffer, split across all cores; paths are rebuilt only for hits.

use memchr::memmem::Finder;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use crate::config::SearchConfig;

const NONE: u32 = u32::MAX;
const DIR: u8 = 1;
const GONE: u8 = 2;
/// An archive whose entries are its children.
const ARC: u8 = 4;
const MAGIC: &[u8; 8] = b"COXSWIX1";

#[derive(Clone, Copy, Debug)]
struct Node {
    parent: u32,
    off: u32,
    len: u16,
    flags: u8,
}

#[derive(Default)]
pub struct Index {
    roots: Vec<PathBuf>,
    exclude: Vec<String>,
    names: Vec<u8>,
    lower: Vec<u8>,
    nodes: Vec<Node>,
    /// Chained path hash -> node id, directories only. Used to apply change events.
    dirs: HashMap<u64, u32>,
    root_ids: Vec<u32>,
    gone: usize,
    /// Look inside the archives in the folders whose text is read (`store::roots`, less the
    /// folders `store::left_out`): these settings, and those folders. `None`: never.
    archives: Option<(SearchConfig, Vec<PathBuf>)>,
    /// Each archive looked into: its size and modified time (nanoseconds) then.
    stamps: HashMap<u32, Stamp>,
}

type Stamp = (u64, u64);

/// Whether the node can have children: a folder, or an archive looked into.
fn holds(flags: u8) -> bool {
    flags & (DIR | ARC) != 0
}

fn stamp_of(path: &Path) -> Option<Stamp> {
    let m = fs::metadata(path).ok()?;
    Some((m.len(), m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos() as u64))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hit {
    pub path: PathBuf,
    pub is_dir: bool,
    /// A search in the files' text: the passage that matched, its words between
    /// `store::MARK`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub snippet: Option<String>,
    /// Found by meaning, not by its words: how close (1 the same). The snippet is then the
    /// start of the passage that was close.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub similar: Option<f32>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Results {
    pub hits: Vec<Hit>,
    /// All matches, including those past `max`.
    pub total: usize,
    pub micros: u64,
}

fn chain(h: u64, name: &[u8]) -> u64 {
    // FNV-1a seeded with the parent's hash; collisions are checked on lookup.
    let mut x = h ^ 0xcbf2_9ce4_8422_2325;
    for &b in name {
        x = (x ^ b as u64).wrapping_mul(0x0100_0000_01b3);
    }
    x.rotate_left(5) ^ name.len() as u64
}

fn lowercase(name: &str) -> Vec<u8> {
    // Offsets are shared with `names`, so only keep folds that preserve byte length.
    let l = name.to_lowercase();
    if l.len() == name.len() { l.into_bytes() } else { name.to_ascii_lowercase().into_bytes() }
}

/// A directory scanned on disk, not yet placed in the index.
struct Scanned {
    name: String,
    is_dir: bool,
    children: Vec<Scanned>,
    /// An archive looked into, as it was then: its children are its entries, and they take
    /// the place of what the index had of it.
    stamp: Option<Box<Stamp>>,
}

/// Entries of an archive (`a/b/c.txt`) as a tree, with the folders only named in their paths.
fn tree(entries: Vec<crate::archive::ArchiveEntry>) -> Vec<Scanned> {
    // Flat first, each name once in its folder: (folder, name, is a folder).
    let mut flat: Vec<(usize, String, bool)> = vec![(0, String::new(), true)];
    let mut at: HashMap<(usize, String), usize> = HashMap::new();
    for e in entries {
        let mut parent = 0;
        let mut parts = e.name.split('/').peekable();
        while let Some(part) = parts.next() {
            let dir = e.is_dir || parts.peek().is_some();
            let id = *at.entry((parent, part.to_string())).or_insert_with(|| {
                flat.push((parent, part.to_string(), dir));
                flat.len() - 1
            });
            flat[id].2 |= dir;
            parent = id;
        }
    }
    let mut kids: Vec<Vec<usize>> = vec![vec![]; flat.len()];
    for (i, f) in flat.iter().enumerate().skip(1) {
        kids[f.0].push(i);
    }
    fn nest(i: usize, flat: &mut [(usize, String, bool)], kids: &[Vec<usize>]) -> Scanned {
        let children = kids[i].iter().map(|&k| nest(k, flat, kids)).collect();
        Scanned { name: std::mem::take(&mut flat[i].1), is_dir: flat[i].2, children, stamp: None }
    }
    kids[0].iter().map(|&k| nest(k, &mut flat, &kids)).collect()
}

/// What `Index::look` read from the disk for `Index::apply`: each dirty directory's node, its
/// hash, whether it still is one, and what it holds now; and the children the index knows.
#[derive(Default)]
pub struct Looked {
    targets: Vec<(u32, u64, bool, Vec<Scanned>)>,
    known: HashMap<u32, HashMap<Vec<u8>, u32>>,
}

impl Index {
    pub fn default_roots() -> Vec<PathBuf> {
        if cfg!(windows) {
            (b'A'..=b'Z').map(|d| PathBuf::from(format!("{}:\\", d as char))).filter(|p| p.exists()).collect()
        } else {
            vec![PathBuf::from("/")]
        }
    }

    pub fn cache_path() -> Option<PathBuf> {
        Some(dirs::cache_dir()?.join("coxswain").join("index.bin"))
    }

    pub fn len(&self) -> usize {
        self.nodes.len() - self.gone
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn excluded(&self, path: &Path, name: &str) -> bool {
        self.exclude.iter().any(|e| if e.contains(['/', '\\']) { path.starts_with(e) } else { e == name })
    }

    /// Walk `roots` in parallel and index every name, with `archives` the entries of the
    /// archives in the folders whose text it reads too: those `before` knew as they are now are
    /// taken from it, the rest are read.
    pub fn build(roots: &[PathBuf], exclude: &[String], archives: Option<&SearchConfig>, before: Option<&Index>) -> Index {
        let archives = archives.map(|cfg| (cfg.clone(), crate::store::roots(cfg)));
        let mut ix = Index { roots: roots.to_vec(), exclude: exclude.to_vec(), archives, ..Default::default() };
        for root in roots {
            let children = ix.scan(root, before, ix.looks_in(root));
            let name = root.to_string_lossy().into_owned();
            ix.push_tree(NONE, Scanned { name, is_dir: true, children, stamp: None });
        }
        ix.rehash();
        ix
    }

    /// `dir` and below, `look` when the archives in it are looked into.
    fn scan(&self, dir: &Path, before: Option<&Index>, look: bool) -> Vec<Scanned> {
        let mut out = self.scan_one(dir);
        out.par_iter_mut().filter(|s| s.is_dir || look && crate::archive::is_archive(Path::new(&s.name))).for_each(|s| {
            let path = dir.join(&s.name);
            if s.is_dir {
                let look = self.looks_below(&path, look);
                s.children = self.scan(&path, before, look);
            } else {
                Index::open_up(&path, s, before);
            }
        });
        out
    }

    /// Whether the archives in `dir`, a folder in one where they are (`parent`) or not, are
    /// looked into.
    fn looks_below(&self, dir: &Path, parent: bool) -> bool {
        let Some((cfg, read)) = &self.archives else { return false };
        read.iter().any(|r| r == dir) || parent && !crate::store::left_out(dir, cfg)
    }

    /// Whether the archives in `dir` are looked into, from its path alone.
    fn looks_in(&self, dir: &Path) -> bool {
        let Some((cfg, read)) = &self.archives else { return false };
        read.iter().any(|r| dir.starts_with(r) && !dir.ancestors().take_while(|a| a != r).any(|a| crate::store::left_out(a, cfg)))
    }

    /// An archive's entries as its children: from `before` when it knew the archive as it is,
    /// else read from it.
    fn open_up(path: &Path, s: &mut Scanned, before: Option<&Index>) {
        let Some(stamp) = stamp_of(path) else { return };
        s.stamp = Some(Box::new(stamp));
        let known = before.and_then(|b| b.find_dir(path).filter(|(id, _)| b.stamps.get(id) == Some(&stamp)).map(|(id, _)| (b, id)));
        s.children = match known {
            Some((b, id)) => b.subtree(id),
            None => crate::archive::search_entries(path, stamp.0).map(tree).unwrap_or_default(),
        };
    }

    /// What is below `id` (an archive), as scanned: its nodes follow it, each after its parent.
    fn subtree(&self, id: u32) -> Vec<Scanned> {
        // A stack of the nodes on the way down to the one being read.
        let mut stack: Vec<(u32, Scanned)> = vec![(id, Scanned { name: String::new(), is_dir: true, children: vec![], stamp: None })];
        for i in id + 1..self.nodes.len() as u32 {
            let n = self.nodes[i as usize];
            if n.parent < id || n.parent >= i {
                break;
            }
            while stack.last().is_some_and(|(top, _)| *top != n.parent) {
                let (_, done) = stack.pop().unwrap();
                stack.last_mut().unwrap().1.children.push(done);
            }
            let name = String::from_utf8_lossy(self.name(i)).into_owned();
            stack.push((i, Scanned { name, is_dir: n.flags & DIR != 0, children: vec![], stamp: None }));
        }
        while stack.len() > 1 {
            let (_, done) = stack.pop().unwrap();
            stack.last_mut().unwrap().1.children.push(done);
        }
        stack.pop().unwrap().1.children
    }

    fn push(&mut self, parent: u32, name: &str, is_dir: bool) -> u32 {
        let id = self.nodes.len() as u32;
        let len = name.len().min(u16::MAX as usize);
        self.nodes.push(Node { parent, off: self.names.len() as u32, len: len as u16, flags: if is_dir { DIR } else { 0 } });
        self.names.extend_from_slice(&name.as_bytes()[..len]);
        self.names.push(0);
        self.lower.extend_from_slice(&lowercase(name)[..len]);
        self.lower.push(0);
        id
    }

    fn push_tree(&mut self, parent: u32, s: Scanned) -> u32 {
        let id = self.push(parent, &s.name, s.is_dir);
        if let Some(stamp) = s.stamp {
            self.nodes[id as usize].flags |= ARC;
            self.stamps.insert(id, *stamp);
        }
        for c in s.children {
            self.push_tree(id, c);
        }
        id
    }

    fn name(&self, id: u32) -> &[u8] {
        let n = self.nodes[id as usize];
        &self.names[n.off as usize..n.off as usize + n.len as usize]
    }

    fn hash_of(&self, id: u32, parent_hash: u64) -> u64 {
        chain(parent_hash, self.name(id))
    }

    /// Recompute the directory lookup table. Parents always precede children.
    fn rehash(&mut self) {
        let mut h = vec![0u64; self.nodes.len()];
        self.dirs.clear();
        self.root_ids = (0..self.nodes.len() as u32).filter(|&i| self.nodes[i as usize].parent == NONE).collect();
        for i in 0..self.nodes.len() {
            let n = self.nodes[i];
            let ph = if n.parent == NONE { 0 } else { h[n.parent as usize] };
            h[i] = self.hash_of(i as u32, ph);
            if holds(n.flags) && n.flags & GONE == 0 {
                self.dirs.insert(h[i], i as u32);
            }
        }
    }

    /// Full path, or `None` if the node or an ancestor was deleted.
    pub fn path(&self, id: u32) -> Option<PathBuf> {
        let mut parts = vec![];
        let mut cur = id;
        while cur != NONE {
            let n = self.nodes[cur as usize];
            if n.flags & GONE != 0 {
                return None;
            }
            parts.push(cur);
            cur = n.parent;
        }
        let mut p = PathBuf::new();
        for &i in parts.iter().rev() {
            p.push(String::from_utf8_lossy(self.name(i)).as_ref());
        }
        Some(p)
    }

    fn find_dir(&self, path: &Path) -> Option<(u32, u64)> {
        let (ri, root) = self.roots.iter().enumerate().filter(|(_, r)| path.starts_with(r)).max_by_key(|(_, r)| r.as_os_str().len())?;
        let rid = *self.root_ids.get(ri)?;
        let mut h = self.hash_of(rid, 0);
        let mut id = rid;
        for c in path.strip_prefix(root).ok()?.components() {
            h = chain(h, c.as_os_str().to_string_lossy().as_bytes());
            id = *self.dirs.get(&h)?;
        }
        (self.path(id).as_deref() == Some(path)).then_some((id, h))
    }

    /// Re-read these directories (one level) and apply what changed. New subdirectories
    /// are scanned recursively.
    pub fn refresh(&mut self, dirs: &HashSet<PathBuf>) {
        let looked = self.look(dirs);
        self.apply(looked);
    }

    /// The disk side of `refresh`: what the dirty directories hold now, new subdirectories
    /// scanned. Needs no write lock, so searches go on while a moved-in tree is read.
    pub fn look(&self, dirs: &HashSet<PathBuf>) -> Looked {
        let targets: HashMap<u32, (PathBuf, u64)> =
            dirs.iter().filter_map(|d| self.find_dir(d).filter(|(id, _)| self.nodes[*id as usize].flags & DIR != 0).map(|(id, h)| (id, (d.clone(), h)))).collect();
        if targets.is_empty() {
            return Looked::default();
        }
        // One parallel pass finds the current children of every dirty directory.
        let kids: Vec<(u32, u32)> = self
            .nodes
            .par_iter()
            .enumerate()
            .filter(|(_, n)| n.flags & GONE == 0 && targets.contains_key(&n.parent))
            .map(|(i, n)| (n.parent, i as u32))
            .collect();
        let mut known: HashMap<u32, HashMap<Vec<u8>, u32>> = HashMap::new();
        for (p, c) in kids {
            known.entry(p).or_default().insert(self.name(c).to_vec(), c);
        }
        let targets = targets
            .into_iter()
            .map(|(id, (dir, h))| {
                let is_dir = dir.is_dir();
                let mut on_disk = if is_dir { self.scan_one(&dir) } else { vec![] };
                let look = is_dir && self.looks_in(&dir);
                let same = |s: &Scanned| known.get(&id).and_then(|k| k.get(s.name.as_bytes())).is_some_and(|&c| (self.nodes[c as usize].flags & DIR != 0) == s.is_dir);
                for s in on_disk.iter_mut() {
                    if s.is_dir && !same(s) {
                        let path = dir.join(&s.name);
                        let below = self.looks_below(&path, look);
                        s.children = self.scan(&path, None, below);
                    } else if !s.is_dir && look && crate::archive::is_archive(Path::new(&s.name)) {
                        // An archive that is new, or changed since it was looked into, is read again.
                        let path = dir.join(&s.name);
                        let had = known.get(&id).and_then(|k| k.get(s.name.as_bytes())).and_then(|c| self.stamps.get(c));
                        if had.is_none() || had.copied() != stamp_of(&path) {
                            Index::open_up(&path, s, None);
                        }
                    }
                }
                (id, h, is_dir, on_disk)
            })
            .collect();
        Looked { targets, known }
    }

    /// The index side of `refresh`: what `look` found goes in.
    pub fn apply(&mut self, mut looked: Looked) {
        for (id, h, is_dir, on_disk) in looked.targets {
            let mut known = looked.known.remove(&id).unwrap_or_default();
            if !is_dir {
                self.remove(id, h);
            }
            for s in on_disk {
                match known.remove(s.name.as_bytes()) {
                    // Same name, same kind: unchanged.
                    Some(c) if (self.nodes[c as usize].flags & DIR != 0) == s.is_dir && s.stamp.is_none() => {}
                    old => {
                        if let Some(c) = old {
                            self.remove(c, chain(h, self.name(c)));
                        }
                        let holder = s.is_dir || s.stamp.is_some();
                        let start = self.nodes.len();
                        self.push_tree(id, s);
                        if holder {
                            self.hash_new(start);
                        }
                    }
                }
            }
            for (name, c) in known {
                self.remove(c, chain(h, &name));
            }
        }
        // ponytail: tombstones are never compacted in place; a rebuild reclaims them (`stale`).
    }

    /// Whether a quarter of the nodes are gone: every search still walks them, so a rebuild
    /// is due before the hourly one.
    pub fn stale(&self) -> bool {
        self.gone * 4 > self.nodes.len()
    }

    /// One directory level, without excluded directories.
    fn scan_one(&self, dir: &Path) -> Vec<Scanned> {
        let Ok(rd) = fs::read_dir(dir) else { return vec![] };
        rd.flatten()
            .map(|de| Scanned {
                name: de.file_name().to_string_lossy().into_owned(),
                // d_type from readdir: no stat, and symlinks are not followed.
                is_dir: de.file_type().is_ok_and(|t| t.is_dir()),
                children: vec![],
                stamp: None,
            })
            .filter(|s| !(s.is_dir && self.excluded(&dir.join(&s.name), &s.name)))
            .collect()
    }

    /// Add directory hashes for nodes appended from `start` on.
    fn hash_new(&mut self, start: usize) {
        let mut h: HashMap<u32, u64> = HashMap::new();
        for i in start..self.nodes.len() {
            let n = self.nodes[i];
            let ph = match h.get(&n.parent) {
                Some(&ph) => ph,
                None => self.path(n.parent).and_then(|p| self.find_dir(&p)).map_or(0, |x| x.1),
            };
            let hi = self.hash_of(i as u32, ph);
            if holds(n.flags) {
                h.insert(i as u32, hi);
                self.dirs.insert(hi, i as u32);
            }
        }
    }

    fn remove(&mut self, id: u32, hash: u64) {
        let n = &mut self.nodes[id as usize];
        if n.flags & GONE == 0 {
            n.flags |= GONE;
            self.gone += 1;
            if holds(n.flags) {
                self.dirs.remove(&hash);
            }
            self.stamps.remove(&id);
        }
    }

    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        let t = Instant::now();
        let q = Query::parse(query);
        if q.terms.is_empty() {
            return Results::default();
        }
        let scope = match scope {
            Some(p) => match self.find_dir(p) {
                Some((id, _)) => Some(id),
                None => return Results::default(),
            },
            None => None,
        };
        let hay = if q.case { &self.names } else { &self.lower };
        let check = |id: u32| self.matches(id, &q, scope);

        let mut ids: Vec<u32> = match q.anchor() {
            // Scan the whole name buffer once, in parallel chunks cut at name boundaries.
            Some(finder) => {
                let chunks = rayon::current_num_threads() * 8;
                let step = hay.len() / chunks + 1;
                let mut cuts = vec![0];
                while *cuts.last().unwrap() < hay.len() {
                    let from = (cuts.last().unwrap() + step).min(hay.len());
                    cuts.push(memchr::memchr(0, &hay[from..]).map_or(hay.len(), |i| from + i + 1));
                }
                cuts.par_windows(2)
                    .flat_map_iter(|w| {
                        let mut last = NONE;
                        finder.find_iter(&hay[w[0]..w[1]]).filter_map(move |pos| {
                            let pos = (w[0] + pos) as u32;
                            let id = self.nodes.partition_point(|n| n.off <= pos) as u32 - 1;
                            (id != last).then(|| {
                                last = id;
                                id
                            })
                        })
                    })
                    .filter(|&id| check(id))
                    .collect()
            }
            None => (0..self.nodes.len() as u32).into_par_iter().filter(|&id| check(id)).collect(),
        };
        let total = ids.len();
        ids.truncate(max);
        let hits = ids
            .into_iter()
            .filter_map(|id| Some(Hit { path: self.path(id)?, is_dir: self.nodes[id as usize].flags & DIR != 0, snippet: None, similar: None }))
            .collect();
        Results { hits, total, micros: t.elapsed().as_micros() as u64 }
    }

    fn matches(&self, id: u32, q: &Query, scope: Option<u32>) -> bool {
        let n = self.nodes[id as usize];
        if n.flags & GONE != 0 || n.parent == NONE {
            return false;
        }
        let buf = if q.case { &self.names } else { &self.lower };
        let name = &buf[n.off as usize..n.off as usize + n.len as usize];
        let mut path: Option<Vec<u8>> = None;
        let is_dir = n.flags & DIR != 0;
        for term in &q.terms {
            let hit = term.alts.iter().any(|pat| match pat {
                Pat::File => !is_dir,
                Pat::Folder => is_dir,
                Pat::Ext(exts) => !is_dir && exts.iter().any(|e| name.len() > e.len() && name.ends_with(e) && name[name.len() - e.len() - 1] == b'.'),
                Pat::Sub(f, false) => f.find(name).is_some(),
                Pat::Glob(g, false) => glob(g, name),
                Pat::Sub(_, true) | Pat::Glob(_, true) => {
                    let p = path.get_or_insert_with(|| {
                        let s = self.path(id).map(|p| p.to_string_lossy().replace(std::path::MAIN_SEPARATOR, "/")).unwrap_or_default();
                        if q.case { s.into_bytes() } else { lowercase(&s) }
                    });
                    match pat {
                        Pat::Sub(f, _) => f.find(p).is_some(),
                        Pat::Glob(g, _) => glob(g, p),
                        _ => unreachable!(),
                    }
                }
            });
            if hit == term.neg {
                return false;
            }
        }
        if scope.is_none() && self.gone == 0 {
            return true;
        }
        // Reject children of deleted directories, and anything outside `scope`.
        let (mut cur, mut inside) = (n.parent, scope.is_none());
        while cur != NONE {
            let p = self.nodes[cur as usize];
            if p.flags & GONE != 0 {
                return false;
            }
            inside |= Some(cur) == scope;
            cur = p.parent;
        }
        inside
    }

    // ------------------------------------------------------------ persistence

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let tmp = path.with_extension("tmp");
        crate::fs::private(path.parent().unwrap_or(Path::new(".")), None)?;
        let mut w = io::BufWriter::new(fs::File::create(&tmp)?);
        w.write_all(MAGIC)?;
        let roots = self.roots.iter().map(|r| r.to_string_lossy()).collect::<Vec<_>>().join("\0");
        let excl = self.exclude.join("\0");
        for s in [&roots, &excl] {
            w.write_all(&(s.len() as u32).to_le_bytes())?;
            w.write_all(s.as_bytes())?;
        }
        // Tombstones are kept so ids stay dense; the next rebuild clears them.
        w.write_all(&(self.nodes.len() as u32).to_le_bytes())?;
        w.write_all(&(self.names.len() as u32).to_le_bytes())?;
        for n in &self.nodes {
            w.write_all(&n.parent.to_le_bytes())?;
            w.write_all(&n.off.to_le_bytes())?;
            w.write_all(&n.len.to_le_bytes())?;
            w.write_all(&[n.flags, 0])?;
        }
        w.write_all(&self.names)?;
        // Then the archives looked into, as they were: a rebuild takes their entries from here.
        w.write_all(&(self.stamps.len() as u32).to_le_bytes())?;
        for (id, (size, modified)) in &self.stamps {
            w.write_all(&id.to_le_bytes())?;
            w.write_all(&size.to_le_bytes())?;
            w.write_all(&modified.to_le_bytes())?;
        }
        w.into_inner()?.sync_all()?;
        crate::fs::private(path.parent().unwrap_or(Path::new(".")), Some(&tmp))?;
        fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> io::Result<Index> {
        let mut buf = vec![];
        fs::File::open(path)?.read_to_end(&mut buf)?;
        let mut r = Reader { buf: &buf, at: 0 };
        if r.take(8)? != MAGIC {
            return Err(r.bad());
        }
        let mut strs = vec![];
        for _ in 0..2 {
            let n = r.u32()? as usize;
            let s = String::from_utf8_lossy(r.take(n)?);
            strs.push(s.split('\0').filter(|x| !x.is_empty()).map(String::from).collect::<Vec<_>>());
        }
        let (n_nodes, n_names) = (r.u32()? as usize, r.u32()? as usize);
        let nodes: Vec<Node> = r
            .take(n_nodes * 12)?
            .chunks_exact(12)
            .map(|c| Node {
                parent: u32::from_le_bytes(c[0..4].try_into().unwrap()),
                off: u32::from_le_bytes(c[4..8].try_into().unwrap()),
                len: u16::from_le_bytes(c[8..10].try_into().unwrap()),
                flags: c[10],
            })
            .collect();
        let names = r.take(n_names)?.to_vec();
        // An index from before archives were looked into ends here.
        let mut stamps = HashMap::new();
        if let Ok(n) = r.u32() {
            for c in r.take(n as usize * 20)?.chunks_exact(20) {
                let id = u32::from_le_bytes(c[0..4].try_into().unwrap());
                if (id as usize) < nodes.len() {
                    stamps.insert(id, (u64::from_le_bytes(c[4..12].try_into().unwrap()), u64::from_le_bytes(c[12..20].try_into().unwrap())));
                }
            }
        }
        // A damaged file must not take the helper down at every start: every name inside the
        // buffer, every parent an earlier node (so no cycles), the names in order.
        let sound = |(i, n): (usize, &Node)| (n.off as usize + n.len as usize) < names.len() && (n.parent == NONE || (n.parent as usize) < i) && (i == 0 || n.off > nodes[i - 1].off);
        if !nodes.iter().enumerate().all(sound) {
            return Err(r.bad());
        }
        let mut lower = Vec::with_capacity(names.len());
        for n in &nodes {
            let name = String::from_utf8_lossy(&names[n.off as usize..n.off as usize + n.len as usize]).into_owned();
            lower.resize(n.off as usize, 0);
            lower.extend_from_slice(&lowercase(&name)[..n.len as usize]);
            lower.push(0);
        }
        let exclude = strs.pop().unwrap();
        let mut ix = Index {
            roots: strs.pop().unwrap().into_iter().map(PathBuf::from).collect(),
            exclude,
            gone: nodes.iter().filter(|n| n.flags & GONE != 0).count(),
            stamps,
            names,
            lower,
            nodes,
            ..Default::default()
        };
        ix.rehash();
        Ok(ix)
    }
}

struct Reader<'a> {
    buf: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bad(&self) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidData, "bad index file")
    }
    fn take(&mut self, n: usize) -> io::Result<&'a [u8]> {
        let s = self.buf.get(self.at..self.at + n).ok_or_else(|| self.bad())?;
        self.at += n;
        Ok(s)
    }
    fn u32(&mut self) -> io::Result<u32> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
}

// ---------------------------------------------------------------- query

enum Pat {
    /// Substring; `true` = match against the full path.
    Sub(Finder<'static>, bool),
    Glob(Vec<u8>, bool),
    Ext(Vec<Vec<u8>>),
    File,
    Folder,
}

struct Term {
    neg: bool,
    alts: Vec<Pat>,
}

struct Query {
    case: bool,
    terms: Vec<Term>,
}

impl Query {
    /// Everything syntax subset: `a b` AND, `a|b` OR, `!a` NOT, `*`/`?` wildcards (whole
    /// name), `ext:rs;toml`, `file:`, `folder:`, `case:`, a term with a path separator
    /// matches the full path, `"quoted phrase"` keeps spaces.
    fn parse(q: &str) -> Query {
        let mut case = false;
        let mut words = vec![];
        let mut cur = String::new();
        let mut quoted = false;
        for c in q.chars() {
            match c {
                '"' => quoted = !quoted,
                c if c.is_whitespace() && !quoted => words.push(std::mem::take(&mut cur)),
                c => cur.push(c),
            }
        }
        words.push(cur);
        let mut terms = vec![];
        for w in words.into_iter().filter(|w| !w.is_empty()) {
            let (neg, w) = match w.strip_prefix('!') {
                Some(r) => (true, r.to_string()),
                None => (false, w),
            };
            if w.eq_ignore_ascii_case("case:") {
                case = true;
                continue;
            }
            let alts = w.split('|').filter(|a| !a.is_empty()).map(|a| (a, a.to_ascii_lowercase())).map(|(a, l)| {
                if l == "file:" {
                    Pat::File
                } else if l == "folder:" {
                    Pat::Folder
                } else if let Some(e) = l.strip_prefix("ext:") {
                    Pat::Ext(e.split(';').filter(|x| !x.is_empty()).map(|x| x.as_bytes().to_vec()).collect())
                } else {
                    // Paths are matched with `/` everywhere, so `src/ui` works on Windows too.
                    let a = &a.replace(std::path::MAIN_SEPARATOR, "/");
                    let path = a.contains('/');
                    // Case folding happens at compile time of the finder; see Query::fold.
                    if a.contains(['*', '?']) {
                        Pat::Glob(a.as_bytes().to_vec(), path)
                    } else {
                        Pat::Sub(Finder::new(a.as_bytes()).into_owned(), path)
                    }
                }
            });
            let alts: Vec<Pat> = alts.collect();
            if !alts.is_empty() {
                terms.push(Term { neg, alts });
            }
        }
        let mut q = Query { case, terms };
        if !q.case {
            q.fold();
        }
        q
    }

    fn fold(&mut self) {
        for t in &mut self.terms {
            for p in &mut t.alts {
                match p {
                    Pat::Sub(f, path) => *p = Pat::Sub(Finder::new(&lowercase(&String::from_utf8_lossy(f.needle()))).into_owned(), *path),
                    Pat::Glob(g, _) => *g = lowercase(&String::from_utf8_lossy(g)),
                    Pat::Ext(es) => es.iter_mut().for_each(|e| *e = lowercase(&String::from_utf8_lossy(e))),
                    _ => {}
                }
            }
        }
    }

    /// The first positive, single, name-substring term: it drives the buffer scan.
    fn anchor(&self) -> Option<&Finder<'static>> {
        self.terms.iter().find_map(|t| match (t.neg, t.alts.as_slice()) {
            (false, [Pat::Sub(f, false)]) => Some(f),
            _ => None,
        })
    }
}

/// `*` any run, `?` any one item (a byte, or a character when given chars); anchored at both
/// ends.
pub fn glob<T: PartialEq + Copy + From<u8>>(p: &[T], s: &[T]) -> bool {
    let (any, run) = (T::from(b'?'), T::from(b'*'));
    let (mut pi, mut si, mut star, mut mark) = (0, 0, None, 0);
    while si < s.len() {
        if pi < p.len() && (p[pi] == any || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == run {
            star = Some(pi);
            mark = si;
            pi += 1;
        } else if let Some(sp) = star {
            pi = sp + 1;
            mark += 1;
            si = mark;
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == run)
}

// ---------------------------------------------------------------- service

/// Shared, live index for a UI: loads the cache, rebuilds in the background, follows
/// file system changes, and saves itself.
pub struct Service {
    index: RwLock<Index>,
    state: AtomicU8,
    /// Who else follows the watcher: each burst of changed paths is sent to them.
    listeners: Mutex<Vec<std::sync::mpsc::Sender<Vec<PathBuf>>>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Serving a cached index while a fresh one is built.
    Stale,
    Building,
    Ready,
}

impl Service {
    pub fn start(cfg: &SearchConfig) -> Arc<Service> {
        let roots = if cfg.roots.is_empty() { Index::default_roots() } else { cfg.roots.clone() };
        let cache = Index::cache_path();
        let cached = cache.as_deref().and_then(|p| Index::load(p).ok()).filter(|ix| ix.roots == roots && ix.exclude == cfg.exclude).map(|ix| Index { archives: cfg.archives.then(|| (cfg.clone(), crate::store::roots(cfg))), ..ix });
        let state = if cached.is_some() { State::Stale } else { State::Building };
        let svc = Arc::new(Service { index: RwLock::new(cached.unwrap_or_default()), state: AtomicU8::new(state as u8), listeners: Mutex::default() });
        let (s, exclude, watch, archives) = (svc.clone(), cfg.exclude.clone(), cfg.watch, cfg.archives.then(|| cfg.clone()));
        std::thread::spawn(move || s.run(roots, exclude, cache, watch, archives));
        svc
    }

    pub fn state(&self) -> State {
        match self.state.load(Ordering::Relaxed) {
            0 => State::Stale,
            1 => State::Building,
            _ => State::Ready,
        }
    }

    pub fn len(&self) -> usize {
        self.index.read().unwrap().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn search(&self, query: &str, scope: Option<&Path>, max: usize) -> Results {
        self.index.read().unwrap().search(query, scope, max)
    }

    /// The paths the watcher sees change, a burst at a time.
    pub fn changes(&self) -> std::sync::mpsc::Receiver<Vec<PathBuf>> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.listeners.lock().unwrap().push(tx);
        rx
    }

    fn run(&self, roots: Vec<PathBuf>, exclude: Vec<String>, cache: Option<PathBuf>, watch: bool, archives: Option<SearchConfig>) {
        use notify::{RecursiveMode, Watcher};
        let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
        let mut watcher: Option<notify::RecommendedWatcher> = None;
        let mut rebuild_at = Instant::now();
        loop {
            if rebuild_at <= Instant::now() {
                // Archives that have not changed since are not read again.
                let fresh = Index::build(&roots, &exclude, archives.as_ref(), Some(&self.index.read().unwrap()));
                *self.index.write().unwrap() = fresh;
                self.state.store(State::Ready as u8, Ordering::Relaxed);
                if let Some(c) = &cache {
                    let _ = self.index.read().unwrap().save(c);
                }
                rebuild_at = Instant::now() + Duration::from_secs(3600);
                if watch && watcher.is_none() {
                    watcher = notify::RecommendedWatcher::new(tx.clone(), notify::Config::default().with_follow_symlinks(false)).ok();
                    for root in &roots {
                        let Some(w) = watcher.as_mut() else { break };
                        // Watch top-level children one by one so excluded trees (/proc) are skipped.
                        // ponytail: inotify needs one watch per directory; past max_user_watches
                        // the rest go unwatched until the hourly rebuild. fanotify is the upgrade.
                        let kids = fs::read_dir(root).map(|rd| rd.flatten().map(|d| d.path()).collect::<Vec<_>>()).unwrap_or_default();
                        for k in kids.iter().filter(|k| k.is_dir() && !k.is_symlink() && !exclude.iter().any(|e| k.starts_with(e))) {
                            let _ = w.watch(k, RecursiveMode::Recursive);
                        }
                        let _ = w.watch(root, RecursiveMode::NonRecursive);
                    }
                }
            }
            // Gather a burst of events, then apply them in one pass.
            let (mut dirty, mut paths) = (HashSet::new(), vec![]);
            let Ok(first) = rx.recv_timeout(Duration::from_secs(60)) else { continue };
            let deadline = Instant::now() + Duration::from_millis(500);
            let mut ev = Some(first);
            while let Some(e) = ev {
                if let Ok(e) = e {
                    dirty.extend(e.paths.iter().filter_map(|p| p.parent().map(Path::to_path_buf)));
                    paths.extend(e.paths);
                }
                ev = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).ok();
            }
            if !dirty.is_empty() {
                // The disk is read under a read lock: searches go on meanwhile.
                let looked = self.index.read().unwrap().look(&dirty);
                self.index.write().unwrap().apply(looked);
                self.listeners.lock().unwrap().retain(|l| l.send(paths.clone()).is_ok());
                // Archives edited over and over leave their old entries behind: compacted.
                if self.index.read().unwrap().stale() {
                    rebuild_at = Instant::now();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name_of(p: &Path) -> String {
        p.file_name().unwrap().to_string_lossy().into_owned()
    }

    fn tree(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-ix-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        for f in ["src/main.rs", "src/lib.rs", "src/ui/App.svelte", "docs/README.md", "Cargo.toml", "skip/secret.rs", "Ærø.txt"] {
            let p = d.join(f);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, "").unwrap();
        }
        d
    }

    fn names(ix: &Index, q: &str) -> Vec<String> {
        let mut v: Vec<_> = ix.search(q, None, 100).hits.into_iter().map(|h| name_of(&h.path)).collect();
        v.sort();
        v
    }

    #[test]
    fn index_query_syntax() {
        let d = tree("q");
        let ix = Index::build(&[d.clone()], &["skip".into()], None, None);
        assert_eq!(names(&ix, "main"), ["main.rs"]);
        assert_eq!(names(&ix, "MAIN"), ["main.rs"]);
        assert_eq!(names(&ix, "case: MAIN"), Vec::<String>::new());
        assert_eq!(names(&ix, "ext:rs"), ["lib.rs", "main.rs"]);
        assert_eq!(names(&ix, "ext:rs;toml"), ["Cargo.toml", "lib.rs", "main.rs"]);
        assert_eq!(names(&ix, "*.rs"), ["lib.rs", "main.rs"]);
        assert_eq!(names(&ix, "s !ui folder:"), ["docs", "src"]);
        assert_eq!(names(&ix, "main|readme"), ["README.md", "main.rs"]);
        assert_eq!(names(&ix, "src/ui app"), ["App.svelte"]);
        assert_eq!(names(&ix, "ærø"), ["Ærø.txt"]);
        assert_eq!(names(&ix, "secret"), Vec::<String>::new());
        let scoped: Vec<_> = ix.search("rs", Some(&d.join("src")), 10).hits.into_iter().map(|h| name_of(&h.path)).collect();
        assert_eq!(scoped.len(), 2);
        assert_eq!(ix.search("rs", None, 1).total, 2);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn index_refresh_and_persist() {
        let d = tree("r");
        let mut ix = Index::build(&[d.clone()], &[], None, None);
        fs::remove_file(d.join("src/lib.rs")).unwrap();
        fs::remove_dir_all(d.join("docs")).unwrap();
        fs::create_dir_all(d.join("src/new/deep")).unwrap();
        fs::write(d.join("src/new/deep/fresh.rs"), "").unwrap();
        ix.refresh(&[d.join("src"), d.clone()].into_iter().collect());
        assert_eq!(names(&ix, "ext:rs"), ["fresh.rs", "main.rs", "secret.rs"]);
        assert_eq!(names(&ix, "readme"), Vec::<String>::new());
        assert!(!ix.stale());
        // Written again and again (an archive edited): the entries left behind add up.
        for _ in 0..3 {
            fs::write(d.join("src/new/deep/fresh.rs"), "").unwrap();
            fs::remove_file(d.join("src/new/deep/fresh.rs")).unwrap();
            ix.refresh(&[d.join("src/new/deep")].into_iter().collect());
            fs::write(d.join("src/new/deep/fresh.rs"), "").unwrap();
            ix.refresh(&[d.join("src/new/deep")].into_iter().collect());
        }
        assert!(ix.stale(), "{} of {} gone", ix.gone, ix.nodes.len());
        // The new directory is findable for later events.
        fs::write(d.join("src/new/deep/later.rs"), "").unwrap();
        ix.refresh(&[d.join("src/new/deep")].into_iter().collect());
        assert_eq!(names(&ix, "later"), ["later.rs"]);

        let file = d.join("ix.bin");
        ix.save(&file).unwrap();
        let back = Index::load(&file).unwrap();
        assert_eq!(names(&back, "ext:rs"), names(&ix, "ext:rs"));
        assert_eq!(back.len(), ix.len());
        // A damaged cache is an error, never a crash: a node whose parent comes after it (a
        // parent past the end), and one whose name lies outside the buffer.
        let good = fs::read(&file).unwrap();
        let at = 8 + 4 + (good[8..12].iter().rev().fold(0usize, |a, &b| a << 8 | b as usize)) + 4;
        let excl = u32::from_le_bytes(good[at - 4..at].try_into().unwrap()) as usize;
        let nodes_at = at + excl + 8 + 12;
        for (off, bytes) in [(nodes_at, (u32::MAX - 1).to_le_bytes()), (nodes_at + 4, u32::MAX.to_le_bytes())] {
            let mut bad = good.clone();
            bad[off..off + 4].copy_from_slice(&bytes);
            fs::write(&file, &bad).unwrap();
            assert!(Index::load(&file).is_err());
        }
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn index_looks_inside_archives_and_follows_their_changes() {
        let d = std::env::temp_dir().join(format!("coxswain-ix-arc-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        let home = d.join("home");
        for (f, text) in [("src/main.rs", "fn main() {}"), ("src/ui/App.svelte", "<p/>"), ("docs/notes.txt", "notes"), ("launch.rs", ""), ("secret-plan.txt", "x")] {
            fs::create_dir_all(d.join(f).parent().unwrap()).unwrap();
            fs::write(d.join(f), text).unwrap();
        }
        fs::create_dir_all(&home).unwrap();
        let (tgz, zip, seven) = (home.join("website.tar.gz"), home.join("docs.zip"), home.join("locked.7z"));
        crate::archive::create(&tgz, &[d.join("src")]).unwrap();
        crate::archive::create(&zip, &[d.join("docs")]).unwrap();
        // A 7z whose names are locked too. Its password, given in this run, is not used.
        let mut w = sevenz_rust2::ArchiveWriter::create(&seven).unwrap();
        w.set_content_methods(vec![sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from("hunter2")).into(), sevenz_rust2::EncoderMethod::LZMA2.into()]);
        w.push_archive_entry(sevenz_rust2::ArchiveEntry::from_path(d.join("secret-plan.txt"), "secret-plan.txt".into()), Some(fs::File::open(d.join("secret-plan.txt")).unwrap())).unwrap();
        w.finish().unwrap();
        crate::archive::remember(&seven, "hunter2");

        let paths = |ix: &Index, q: &str, scope: Option<&Path>| {
            let mut v: Vec<PathBuf> = ix.search(q, scope, 100).hits.into_iter().map(|h| h.path).collect();
            v.sort();
            v
        };
        // Hidden folders, like the others whose text is not read, are not looked into.
        fs::create_dir_all(home.join(".cache")).unwrap();
        crate::archive::create(&home.join(".cache/old.zip"), &[d.join("launch.rs")]).unwrap();
        let cfg = SearchConfig { text_roots: vec![home.clone()], ..SearchConfig::default() };
        let mut ix = Index::build(std::slice::from_ref(&home), &[], Some(&cfg), None);
        assert_eq!(paths(&ix, "launch", None), Vec::<PathBuf>::new());
        assert_eq!(paths(&ix, "main.rs", None), [tgz.join("src").join("main.rs")]);
        assert!(ix.search("ui", None, 10).hits.iter().any(|h| h.is_dir && h.path == tgz.join("src").join("ui")), "a folder only named in paths is one");
        assert_eq!(paths(&ix, "secret", None), Vec::<PathBuf>::new(), "a locked 7z keeps its names");
        assert_eq!(paths(&ix, "website", None), std::slice::from_ref(&tgz), "an archive is still a file");
        // In this folder: the folder an archive is in, and a folder inside one.
        assert_eq!(paths(&ix, "svelte", Some(&home)), [tgz.join("src").join("ui").join("App.svelte")]);
        assert_eq!(paths(&ix, "svelte", Some(&tgz.join("src"))).len(), 1);
        assert_eq!(paths(&ix, "notes", Some(&tgz)), Vec::<PathBuf>::new());
        assert_eq!(paths(&Index::build(std::slice::from_ref(&home), &[], None, None), "main", None), Vec::<PathBuf>::new(), "switched off");
        let elsewhere = SearchConfig { text_roots: vec![d.join("src")], ..SearchConfig::default() };
        assert_eq!(paths(&Index::build(std::slice::from_ref(&home), &[], Some(&elsewhere), None), "main", None), Vec::<PathBuf>::new(), "not a folder read");

        // Changed by Coxswain: a file in, a folder out, one renamed. The watcher's folder is read again.
        crate::archive::add(&tgz, &[("src/launch.rs".into(), d.join("launch.rs"))], None).unwrap();
        crate::archive::remove(&tgz, &["src/ui".into()], None).unwrap();
        crate::archive::rename_in(&zip, "docs/notes.txt", "docs/minutes.txt", None).unwrap();
        ix.refresh(&[home.clone()].into_iter().collect());
        assert_eq!(paths(&ix, "ext:rs", None), [tgz.join("src").join("launch.rs"), tgz.join("src").join("main.rs")]);
        assert_eq!((paths(&ix, "svelte", None), paths(&ix, "notes", None)), (vec![], vec![]));
        assert_eq!(paths(&ix, "minutes", Some(&zip.join("docs"))), [zip.join("docs").join("minutes.txt")]);
        // Changed by anything else: written anew.
        fs::remove_file(&zip).unwrap();
        crate::archive::create(&zip, &[d.join("launch.rs")]).unwrap();
        ix.refresh(&[home.clone()].into_iter().collect());
        assert_eq!(paths(&ix, "minutes", None), Vec::<PathBuf>::new());
        assert_eq!(paths(&ix, "launch", Some(&zip)), [zip.join("launch.rs")]);

        // Saved and loaded with what it knows of each archive: a rebuild takes the entries of an
        // archive that has not changed from it, without reading the archive.
        let file = d.join("ix.bin");
        ix.save(&file).unwrap();
        let back = Index::load(&file).unwrap();
        assert_eq!(paths(&back, "ext:rs", None), paths(&ix, "ext:rs", None));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&tgz, fs::Permissions::from_mode(0o000)).unwrap();
            if fs::File::open(&tgz).is_err() {
                assert_eq!(paths(&Index::build(std::slice::from_ref(&home), &[], Some(&cfg), Some(&back)), "main", None), [tgz.join("src").join("main.rs")]);
                assert_eq!(paths(&Index::build(std::slice::from_ref(&home), &[], Some(&cfg), None), "main", None), Vec::<PathBuf>::new());
            }
            fs::set_permissions(&tgz, fs::Permissions::from_mode(0o644)).unwrap();
        }
        crate::archive::forget(&seven);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn index_glob() {
        assert!(glob(b"*.rs", b"main.rs"));
        assert!(glob(b"m?in*", b"main.rs"));
        assert!(!glob(b"*.rs", b"main.rsx"));
        assert!(glob(b"*a*b*", b"xxaxxbxx"));
        // On chars, `?` is one character, whatever its length in bytes.
        let chars = |s: &str| s.chars().collect::<Vec<_>>();
        assert!(glob(&chars("?.txt"), &chars("é.txt")));
        assert!(!glob(b"?.txt", "é.txt".as_bytes()));
    }
}
