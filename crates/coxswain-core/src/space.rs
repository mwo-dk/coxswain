//! Space: where the bytes under a folder go, for both apps. A walk keeps every file's size
//! (`Tree::scan`, on a thread, with progress and a stop), and `Tree::layout` lays the files out
//! as a treemap: every file a rectangle sized by its bytes, inside its folder's rectangle,
//! coloured by its kind (`Family`). The apps only draw the rectangles. Beside it: the bytes by
//! extension (`Tree::types`) and the largest files (`Tree::largest`).
//!
//! The walk counts what the folder sizes count (`sizes::walk`): files by their length, symbolic
//! links as links, never followed; unreadable folders, other apps' data on a Mac, `/proc`,
//! `/sys`, `/dev` and `/run` skipped. Two differences, as this is about the disk: a file only in
//! the cloud counts 0 bytes (that is what it takes here; it is never read, so never
//! downloaded), and a cloud mount whose files the settings do not read is not walked.

use rayon::prelude::*;
use serde::Serialize;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// What the walk has measured so far, and the stop. Updated from the walk's threads.
#[derive(Default, Debug)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub stop: AtomicBool,
}

impl Progress {
    /// Files and bytes measured so far.
    pub fn now(&self) -> (u64, u64) {
        (
            self.files.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
        )
    }
}

/// A folder worth showing in Space: 1 GB or more as measured (`size`), else a drive's top
/// folder (the top of a file system; on Windows a drive like `C:\`). Never `/proc` and the
/// like, nor folders of memory or boot files. For the hint that names Space; the last answer is
/// kept, so the cursor moving does not stat the same folder again and again.
pub fn worth_it(dir: &Path, size: Option<u64>) -> bool {
    const NOT: [&str; 4] = ["/tmp", "/boot", "/var/tmp", "/efi"];
    if crate::sizes::not_on_disk(dir) || (cfg!(unix) && NOT.iter().any(|n| dir.starts_with(n))) {
        return false;
    }
    if size.is_some_and(|s| s >= 1 << 30) {
        return true;
    }
    let Some(up) = dir.parent() else {
        return true;
    };
    static LAST: std::sync::Mutex<Option<(PathBuf, bool)>> = std::sync::Mutex::new(None);
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((p, top)) = &*last
        && p == dir
    {
        return *top;
    }
    #[cfg(unix)]
    let top = {
        use std::os::unix::fs::MetadataExt;
        match (fs::symlink_metadata(dir), fs::symlink_metadata(up)) {
            (Ok(a), Ok(b)) => a.is_dir() && a.dev() != b.dev(),
            _ => false,
        }
    };
    #[cfg(not(unix))]
    let top = {
        let _ = up;
        false
    };
    *last = Some((dir.to_path_buf(), top));
    top
}

/// A file or folder of the tree. A folder's size and files are its contents', all levels down.
#[derive(Debug)]
pub struct Node {
    /// As the file system has it (not always valid UTF-8): paths are built from it. `label()`
    /// for showing it.
    pub name: Box<OsStr>,
    pub parent: u32,
    pub size: u64,
    pub files: u32,
    pub dir: bool,
    /// Another disk (file system) mounted here: not walked, shown as a block of its own.
    pub mount: bool,
    /// How many such disks are at or below it.
    mounts: u32,
    /// A folder's contents, the largest first.
    kids: Vec<u32>,
}

impl Node {
    /// Its name for showing: not valid UTF-8 shows with a replacement character.
    pub fn label(&self) -> std::borrow::Cow<'_, str> {
        self.name.to_string_lossy()
    }
}

/// The tree's top: the folder Space was opened on.
pub const ROOT: u32 = 0;

#[derive(Debug)]
pub struct Tree {
    pub root: PathBuf,
    nodes: Vec<Node>,
    /// Files only in the cloud: counted, at 0 bytes.
    pub online: u64,
}

/// A file or folder as the walk finds it, before it goes into the tree.
struct Found {
    name: Box<OsStr>,
    size: u64,
    files: u32,
    online: u64,
    mount: bool,
    mounts: u32,
    /// `None` for a file or a mount.
    kids: Option<Vec<Found>>,
}

impl Tree {
    /// Every file below `root`, with its size. `None` when `p.stop` was set meanwhile.
    pub fn scan(root: &Path, p: &Progress) -> Option<Tree> {
        let name: Box<OsStr> = root.as_os_str().into();
        // The folder itself followed, when it is a link (~/Music to another disk, /tmp on a Mac):
        // what it holds is what was asked for.
        let mut top = match fs::metadata(root) {
            Ok(m) if m.is_dir() => {
                let w = Walk {
                    p,
                    #[cfg(unix)]
                    dev: std::os::unix::fs::MetadataExt::dev(&m),
                    // On Linux every file of a cloud mount is only in the cloud: decided once.
                    #[cfg(target_os = "linux")]
                    cloud: crate::cloud::mount_of(root).is_some(),
                };
                // Threads of its own: the shared pool may be busy with long work elsewhere (the
                // search helper, tests), and a walk the user waits for must not queue behind it.
                pool().install(|| w.dir(root, name))?
            }
            _ => Found { name, size: 0, files: 0, online: 0, mount: false, mounts: 0, kids: Some(vec![]) },
        };
        if top.kids.is_none() {
            top.kids = Some(vec![]);
        }
        let mut tree = Tree {
            root: root.to_path_buf(),
            nodes: vec![],
            online: top.online,
        };
        tree.add(top, ROOT);
        Some(tree)
    }

    /// `top` and what is in it, as the tree's first node. Ids go depth first, each folder's
    /// biggest first; a list of what is still to add, not a frame per level, so thousands of
    /// nested folders fit any thread's stack.
    fn add(&mut self, top: Found, parent: u32) {
        let mut todo = vec![(top, parent)];
        while let Some((f, parent)) = todo.pop() {
            let id = self.nodes.len() as u32;
            if id != ROOT {
                self.nodes[parent as usize].kids.push(id);
            }
            self.nodes.push(Node {
                name: f.name,
                parent,
                size: f.size,
                files: f.files,
                dir: f.kids.is_some(),
                mount: f.mount,
                mounts: f.mounts,
                kids: vec![],
            });
            if let Some(mut kids) = f.kids {
                kids.sort_by(|a, b| b.size.cmp(&a.size).then_with(|| a.name.cmp(&b.name)));
                todo.extend(kids.into_iter().rev().map(|k| (k, id)));
            }
        }
    }

    /// `id`'s name for showing: a model store's blob or folder by the model it is
    /// (`local_models::names`), anything else by its own name.
    pub fn label(&self, id: u32) -> std::borrow::Cow<'_, str> {
        let n = self.node(id);
        if id != ROOT
            && let Some(m) = crate::local_models::name_in(&self.path(n.parent), &n.label())
        {
            return m.into();
        }
        n.label()
    }

    pub fn node(&self, id: u32) -> &Node {
        &self.nodes[id as usize]
    }

    pub fn kids(&self, id: u32) -> &[u32] {
        &self.nodes[id as usize].kids
    }

    /// The full path of `id`.
    pub fn path(&self, id: u32) -> PathBuf {
        let mut names = vec![];
        let mut at = id;
        while at != ROOT {
            names.push(&*self.nodes[at as usize].name);
            at = self.nodes[at as usize].parent;
        }
        let mut p = self.root.clone();
        p.extend(names.into_iter().rev());
        p
    }

    /// Whether `id` is in the tree: not removed, and not past its end.
    pub fn alive(&self, id: u32) -> bool {
        (id as usize) < self.nodes.len() && self.find(&self.path(id)) == Some(id)
    }

    /// The node at `path`, when it is in the tree.
    pub fn find(&self, path: &Path) -> Option<u32> {
        let rest = path.strip_prefix(&self.root).ok()?;
        rest.components().try_fold(ROOT, |at, c| {
            self.kids(at)
                .iter()
                .copied()
                .find(|&k| *self.nodes[k as usize].name == *c.as_os_str())
        })
    }

    /// `id` and every folder above it, up to the top.
    fn chain(&self, id: u32) -> impl Iterator<Item = u32> + '_ {
        std::iter::successors(Some(id), move |&at| {
            (at != ROOT).then(|| self.nodes[at as usize].parent)
        })
    }

    /// The child of `under` on the way down to `id`: what a zoom into `id` from `under` shows.
    pub fn toward(&self, under: u32, id: u32) -> Option<u32> {
        let mut last = None;
        for at in self.chain(id) {
            if at == under {
                return last;
            }
            last = Some(at);
        }
        None
    }

    /// Whether `id` is `under` or inside it.
    pub fn within(&self, id: u32, under: u32) -> bool {
        self.chain(id).any(|a| a == under)
    }

    /// `path` is gone (deleted): out of the tree, its bytes and files off every folder above it.
    /// Its id is not used again; whatever pointed at it should forget it. False when it was not
    /// in the tree.
    pub fn remove(&mut self, path: &Path) -> bool {
        let Some(id) = self.find(path).filter(|&id| id != ROOT) else {
            return false;
        };
        let (size, files, mounts, parent) = {
            let n = &self.nodes[id as usize];
            (n.size, n.files, n.mounts, n.parent)
        };
        self.nodes[parent as usize].kids.retain(|&k| k != id);
        for at in self.chain(parent).collect::<Vec<_>>() {
            let n = &mut self.nodes[at as usize];
            n.size -= size;
            n.files -= files;
            n.mounts -= mounts;
        }
        true
    }

    /// The lower-case extension of `id` ("" when none, and for folders).
    pub fn ext(&self, id: u32) -> String {
        let n = &self.nodes[id as usize];
        if n.dir {
            return String::new();
        }
        ext_of(&n.name.to_string_lossy())
    }

    /// The family of `id`, by its extension.
    pub fn family(&self, id: u32) -> Family {
        family(&self.ext(id))
    }

    /// The bytes under `under` by extension, the most first.
    pub fn types(&self, under: u32) -> Vec<TypeRow> {
        let mut by: std::collections::HashMap<String, (u64, u64)> = Default::default();
        let mut stack = vec![under];
        while let Some(at) = stack.pop() {
            let n = &self.nodes[at as usize];
            if n.dir {
                stack.extend(&n.kids);
            } else if !n.mount {
                let e = by.entry(ext_of(&n.name.to_string_lossy())).or_default();
                e.0 += n.size;
                e.1 += 1;
            }
        }
        let total = self.nodes[under as usize].size.max(1) as f64;
        let mut rows: Vec<TypeRow> = by
            .into_iter()
            .map(|(ext, (bytes, files))| TypeRow {
                family: family(&ext),
                share: bytes as f64 / total,
                ext,
                bytes,
                files,
            })
            .collect();
        rows.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.ext.cmp(&b.ext)));
        rows
    }

    /// The `n` largest files under `under`, the largest first.
    pub fn largest(&self, under: u32, n: usize) -> Vec<u32> {
        let mut all = vec![];
        let mut stack = vec![under];
        while let Some(at) = stack.pop() {
            let node = &self.nodes[at as usize];
            if node.dir {
                stack.extend(&node.kids);
            } else if node.size > 0 {
                all.push(at);
            }
        }
        let by_size = |a: &u32, b: &u32| self.nodes[*b as usize].size.cmp(&self.nodes[*a as usize].size);
        if all.len() > n {
            all.select_nth_unstable_by(n, by_size);
            all.truncate(n);
        }
        all.sort_by(by_size);
        all
    }

    /// The treemap of `under` in `area`: a rectangle for every file and folder big enough to
    /// see, parents before what is in them, so drawing them in order paints the deepest last.
    pub fn layout(&self, under: u32, area: Rect, opts: &Opts) -> Vec<Tile> {
        let mut out = vec![];
        if self.nodes[under as usize].size > 0 && area.w > 0.0 && area.h > 0.0 {
            self.place(under, area, 0, opts, &mut out);
        }
        out
    }

    fn place(&self, id: u32, r: Rect, depth: u16, opts: &Opts, out: &mut Vec<Tile>) {
        let n = &self.nodes[id as usize];
        if n.mount {
            out.push(Tile::new(r, id, Kind::Mount, depth, Family::Other, 0));
            return;
        }
        if !n.dir {
            out.push(Tile::new(r, id, Kind::File, depth, family(&ext_of(&n.name.to_string_lossy())), 1));
            return;
        }
        let mut inner = r;
        if depth > 0 {
            out.push(Tile::new(r, id, Kind::Folder, depth, Family::Other, n.files));
            let head = opts.head > 0.0 && r.w >= opts.head * 4.0 && r.h >= opts.head * 3.0;
            inner = Rect {
                x: r.x + opts.pad,
                y: r.y + opts.pad + if head { opts.head } else { 0.0 },
                w: r.w - 2.0 * opts.pad,
                h: r.h - 2.0 * opts.pad - if head { opts.head } else { 0.0 },
            };
        }
        if inner.w <= 0.0 || inner.h <= 0.0 || (n.size == 0 && n.mounts == 0) {
            return;
        }
        // Other disks are not to scale (they hold none of these bytes): each gets a small share,
        // the bytes the rest.
        let all = inner.w as f64 * inner.h as f64;
        let share = if n.mounts == 0 { 0.0 } else if n.size == 0 { all / n.mounts as f64 } else { all * (0.04f64).min(0.2 / n.mounts as f64) };
        let scale = if n.size == 0 { 0.0 } else { (all - share * n.mounts as f64) / n.size as f64 };
        let area = |k: u32| {
            let c = &self.nodes[k as usize];
            c.size as f64 * scale + c.mounts as f64 * share
        };
        let mut kids: Vec<u32> = n.kids.iter().copied().filter(|&k| area(k) > 0.0).collect();
        if n.mounts > 0 {
            kids.sort_by(|a, b| area(*b).total_cmp(&area(*a)));
        }
        // The largest first, so the small ones are the tail: merged into one block. A folder
        // that leads to another disk is never merged away.
        let small_one = |k: u32| self.nodes[k as usize].mounts == 0 && area(k) < opts.min_area as f64;
        let big: Vec<u32> = kids.iter().copied().filter(|&k| !small_one(k)).collect();
        let small: f64 = kids.iter().copied().filter(|&k| small_one(k)).map(area).sum();
        let small_files: u32 = kids.iter().copied().filter(|&k| small_one(k)).map(|k| self.nodes[k as usize].files).sum();
        let mut areas: Vec<f64> = big.iter().map(|&k| area(k)).collect();
        if small > 0.0 {
            areas.push(small);
        }
        let rects = squarify(&areas, inner);
        for (i, rect) in rects.into_iter().enumerate() {
            match big.get(i) {
                Some(&k) => self.place(k, rect, depth + 1, opts, out),
                None => out.push(Tile::new(rect, id, Kind::Small, depth + 1, Family::Other, small_files)),
            }
        }
    }
}

/// The lower-case extension of a file name: after its last dot, not a leading one.
fn ext_of(name: &str) -> String {
    match name.rfind('.') {
        Some(i) if i > 0 && i + 1 < name.len() => name[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// The threads Space walks on: one per core, made once.
fn pool() -> &'static rayon::ThreadPool {
    static POOL: std::sync::OnceLock<rayon::ThreadPool> = std::sync::OnceLock::new();
    POOL.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(2, |n| n.get()).max(2);
        // A frame per folder level: 64 MiB holds tens of thousands of nested folders, deeper
        // than any path a system opens; 2 MiB, the default, overflowed at about 1,900.
        rayon::ThreadPoolBuilder::new()
            .num_threads(n)
            .stack_size(64 << 20)
            .thread_name(|i| format!("coxswain-space-{i}"))
            .build()
            .expect("a thread pool")
    })
}

/// One walk: its progress and stop, and what holds for every folder in it.
struct Walk<'a> {
    p: &'a Progress,
    /// The disk it stays on: a folder on another one is a mount, not walked.
    #[cfg(unix)]
    dev: u64,
    #[cfg(target_os = "linux")]
    cloud: bool,
}

impl Walk<'_> {
    /// The folder `path` and what is below it, staying on one disk. Symbolic links are counted,
    /// not followed. Each entry is looked at through the folder's listing (`DirEntry`), which on
    /// Windows opens nothing, so a file only in the cloud is never downloaded.
    fn dir(&self, path: &Path, name: Box<OsStr>) -> Option<Found> {
        if self.p.stop.load(Ordering::Relaxed) {
            return None;
        }
        let entries: Vec<_> = crate::fs::read_dir(path).map(|rd| rd.flatten().collect()).unwrap_or_default();
        let kids = entries.par_iter().map(|de| self.entry(de)).collect::<Option<Vec<_>>>()?;
        Some(Found {
            size: kids.iter().map(|k| k.size).sum(),
            files: kids.iter().map(|k| k.files).sum(),
            online: kids.iter().map(|k| k.online).sum(),
            mounts: kids.iter().map(|k| k.mounts).sum(),
            mount: false,
            name,
            kids: Some(kids),
        })
    }

    fn entry(&self, de: &fs::DirEntry) -> Option<Found> {
        let name: Box<OsStr> = de.file_name().into_boxed_os_str();
        let found = |size: u64, files: u32, online: bool, mount: bool| Found { name: name.clone(), size, files, online: online as u64, mount, mounts: mount as u32, kids: None };
        let meta = de.metadata();
        if de.file_type().is_ok_and(|t| t.is_dir()) {
            #[cfg(unix)]
            if meta.as_ref().is_ok_and(|m| std::os::unix::fs::MetadataExt::dev(m) != self.dev) {
                return Some(found(0, 0, false, true));
            }
            // Below the folder asked for: what is never walked is an empty folder here.
            let path = de.path();
            if crate::sizes::not_on_disk(&path) || crate::fs::protected_for_space(&path) || crate::cloud::unread_mount(&path) {
                return Some(Found { name, size: 0, files: 0, online: 0, mount: false, mounts: 0, kids: Some(vec![]) });
            }
            return self.dir(&path, name);
        }
        let Ok(meta) = meta else {
            self.p.files.fetch_add(1, Ordering::Relaxed);
            return Some(found(0, 1, false, false));
        };
        #[cfg(target_os = "linux")]
        let online = self.cloud;
        #[cfg(not(target_os = "linux"))]
        let online = crate::cloud::online_meta(&meta, &de.path());
        let size = if online { 0 } else { meta.len() };
        self.p.files.fetch_add(1, Ordering::Relaxed);
        self.p.bytes.fetch_add(size, Ordering::Relaxed);
        Some(found(size, 1, online, false))
    }
}

/// The bytes of one extension.
#[derive(Clone, Debug, Serialize)]
pub struct TypeRow {
    /// Lower case, "" for files without one.
    pub ext: String,
    pub family: Family,
    pub bytes: u64,
    pub files: u64,
    /// Of the bytes under the folder shown, 0 to 1.
    pub share: f64,
}

/// A kind of file, which gives its rectangles their colour.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    Image,
    Video,
    Audio,
    Archive,
    Document,
    Code,
    Data,
    Program,
    Disk,
    Other,
}

impl Family {
    pub const ALL: [Family; 10] = [
        Family::Image,
        Family::Video,
        Family::Audio,
        Family::Archive,
        Family::Document,
        Family::Code,
        Family::Data,
        Family::Program,
        Family::Disk,
        Family::Other,
    ];

    /// Its colour, `#rrggbb`: bright enough on a dark theme and on Classic blue, and far apart
    /// from each other.
    pub fn colour(self) -> &'static str {
        match self {
            Family::Image => "#b07cff",
            Family::Video => "#ff5f6d",
            Family::Audio => "#ff9f40",
            Family::Archive => "#e8d44d",
            Family::Document => "#4ea8ff",
            Family::Code => "#3fd47a",
            Family::Data => "#26d0c4",
            Family::Program => "#ff6fd2",
            Family::Disk => "#c98a5a",
            Family::Other => "#8d99a6",
        }
    }

    /// Its name, in the current language.
    pub fn label(self) -> String {
        crate::t!(&format!("space.family_{}", self.id()))
    }

    pub fn id(self) -> &'static str {
        match self {
            Family::Image => "image",
            Family::Video => "video",
            Family::Audio => "audio",
            Family::Archive => "archive",
            Family::Document => "document",
            Family::Code => "code",
            Family::Data => "data",
            Family::Program => "program",
            Family::Disk => "disk",
            Family::Other => "other",
        }
    }
}

/// The family of an extension (lower case).
pub fn family(ext: &str) -> Family {
    const KINDS: &[(Family, &[&str])] = &[
        (
            Family::Image,
            &[
                "png", "jpg", "jpeg", "gif", "webp", "bmp", "tif", "tiff", "heic", "heif", "avif",
                "raw", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "psd", "xcf", "svg", "ico",
                "jxl", "kra",
            ],
        ),
        (
            Family::Video,
            &[
                "mp4", "mkv", "mov", "webm", "avi", "m4v", "wmv", "flv", "mpg", "mpeg", "ts", "mts",
                "m2ts", "3gp", "ogv", "vob",
            ],
        ),
        (
            Family::Audio,
            &[
                "mp3", "flac", "wav", "ogg", "oga", "opus", "m4a", "aac", "wma", "aiff", "aif",
                "alac", "mid", "midi",
            ],
        ),
        (
            Family::Archive,
            &[
                "zip", "gz", "tgz", "xz", "txz", "zst", "7z", "rar", "tar", "bz2", "tbz", "lz",
                "lz4", "lzma", "cab", "jar", "war", "apk", "whl", "crate", "nupkg", "tbz2", "tzst",
                "vsix",
            ],
        ),
        (
            Family::Document,
            &[
                "pdf", "doc", "docx", "odt", "rtf", "txt", "md", "epub", "xls", "xlsx", "ods",
                "ppt", "pptx", "odp", "pages", "numbers", "key", "tex", "rst", "adoc", "eml",
                "mbox", "djvu", "mobi", "azw3", "html", "htm", "msg", "ps", "eps", "xps", "oxps",
                "typ", "chm", "cbz", "cbr", "cb7", "cbt", "pgn", "epd", "fen", "sgf", "wmf", "emf", "docm", "dotx", "ott", "ppsx",
                "potx", "otp", "fodt", "fods", "fodp", "fodg", "cdr", "pmd",
            ],
        ),
        (
            Family::Code,
            &[
                "rs", "c", "h", "cpp", "hpp", "cc", "cs", "go", "java", "kt", "swift", "py", "rb",
                "php", "js", "mjs", "cjs", "ts", "tsx", "jsx", "svelte", "vue", "css", "scss",
                "lua", "sh", "bash", "zsh", "fish", "ps1", "pl", "hs", "ml", "ex", "exs", "erl",
                "clj", "scala", "dart", "zig", "nim", "r", "jl", "m", "mm", "toml", "yaml", "yml",
                "ipynb", "map", "pyc", "class", "rlib", "rmeta", "d", "o", "obj", "pdb", "lock",
                // Diagrams, drawn from their source.
                "d2", "ditaa", "bob", "svgbob", "pikchr", "nomnoml", "noml", "wavedrom", "vg", "vega",
                "vl", "vegalite", "dbml", "er", "erd", "bytefield", "puml", "plantuml", "pu", "iuml",
                "wsd", "mmd", "drawio", "dio",
            ],
        ),
        (
            Family::Data,
            &[
                "db", "sqlite", "sqlite3", "json", "jsonl", "ndjson", "csv", "tsv", "parquet",
                "xml", "duckdb", "arrow", "feather", "h5", "hdf5", "npy", "npz", "pkl", "bin",
                "dat", "log", "gguf", "safetensors", "onnx", "pt", "ckpt", "idx", "pack", "dbf", "ddb",
            ],
        ),
        (
            Family::Program,
            &[
                "exe", "msi", "dll", "so", "dylib", "a", "lib", "appimage", "deb", "rpm", "pkg",
                "apk", "app", "flatpak", "snap", "wasm", "sys", "ko", "efi",
            ],
        ),
        (
            Family::Disk,
            &[
                "iso", "img", "qcow2", "vmdk", "vdi", "vhd", "vhdx", "dmg", "raw", "ova", "wim",
                "sparseimage", "qcow",
            ],
        ),
    ];
    if let Some((f, _)) = KINDS.iter().find(|(_, exts)| exts.contains(&ext)) {
        return *f;
    }
    // The rest Coxswain knows by their readers' own lists, so a new one lands in its family.
    let name = format!("x.{ext}");
    let path = std::path::Path::new(&name);
    if crate::pictures::named(path) {
        Family::Image
    } else if crate::music::named(path) {
        Family::Audio
    } else if crate::extract::installed::OFFICE.contains(&ext) {
        Family::Document
    } else if crate::archive::is_archive(path) {
        Family::Archive
    } else if crate::disk_image::named(path) {
        Family::Disk
    } else if crate::code::is_code(&name) {
        Family::Code
    } else {
        Family::Other
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
}

/// How the treemap is laid out, in the units of the area (pixels, or terminal half-cells).
#[derive(Clone, Copy, Debug)]
pub struct Opts {
    /// The frame inside each folder's rectangle.
    pub pad: f32,
    /// A folder's title strip at its top, for folders big enough to carry one; 0 for none.
    pub head: f32,
    /// Smaller rectangles are merged into one "small files" block per folder.
    pub min_area: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Folder,
    /// Another disk mounted here: not walked; `id` is its folder. Opening Space there shows it.
    Mount,
    /// A folder's files and folders too small to show, as one block; `id` is the folder.
    Small,
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Tile {
    #[serde(flatten)]
    pub rect: Rect,
    pub id: u32,
    pub kind: Kind,
    /// 1 for what is directly in the folder shown.
    pub depth: u16,
    pub family: Family,
    /// The files it stands for: a folder's all levels down, a small block's merged ones.
    pub files: u32,
}

impl Tile {
    fn new(rect: Rect, id: u32, kind: Kind, depth: u16, family: Family, files: u32) -> Tile {
        Tile {
            rect,
            id,
            kind,
            depth,
            family,
            files,
        }
    }
}

/// Rectangles with `areas` (the largest first, summing to `r`'s area), filling `r`, each as
/// near a square as the order allows: the squarified treemap (Bruls, Huizing and van Wijk).
pub fn squarify(areas: &[f64], r: Rect) -> Vec<Rect> {
    let mut out = Vec::with_capacity(areas.len());
    let (mut x, mut y, mut w, mut h) = (r.x as f64, r.y as f64, r.w as f64, r.h as f64);
    // The worst aspect ratio of a row of `sum` along `side`, its largest `big`, smallest `small`.
    let worst = |big: f64, small: f64, sum: f64, side: f64| {
        let s2 = side * side;
        (s2 * big / (sum * sum)).max(sum * sum / (s2 * small))
    };
    let mut i = 0;
    while i < areas.len() {
        let side = w.min(h).max(f64::MIN_POSITIVE);
        let mut j = i + 1;
        let mut sum = areas[i];
        let mut best = worst(areas[i], areas[i], sum, side);
        while j < areas.len() {
            let next = worst(areas[i], areas[j], sum + areas[j], side);
            if next > best {
                break;
            }
            best = next;
            sum += areas[j];
            j += 1;
        }
        // The last row takes what is left, so rounding leaves no gap.
        let last = j == areas.len();
        if w >= h {
            let thick = if last { w } else { (sum / h).min(w) };
            let mut at = y;
            for (k, a) in areas[i..j].iter().enumerate() {
                let len = if k == j - i - 1 { y + h - at } else { a / thick };
                out.push(Rect { x: x as f32, y: at as f32, w: thick as f32, h: len as f32 });
                at += len;
            }
            x += thick;
            w -= thick;
        } else {
            let thick = if last { h } else { (sum / w).min(h) };
            let mut at = x;
            for (k, a) in areas[i..j].iter().enumerate() {
                let len = if k == j - i - 1 { x + w - at } else { a / thick };
                out.push(Rect { x: at as f32, y: y as f32, w: len as f32, h: thick as f32 });
                at += len;
            }
            y += thick;
            h -= thick;
        }
        i = j;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 1,900 folders, each in the one before, overflowed the walks' 2 MiB stacks. Linux only:
    /// the path is near 4 KB, more than other systems open.
    #[cfg(target_os = "linux")]
    #[test]
    fn thousands_of_nested_folders_are_measured() {
        let d = std::env::temp_dir().join(format!("coxswain-space-deep-{}", std::process::id()));
        let deep = (0..1900).fold(d.clone(), |p, _| p.join("d"));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&deep).unwrap();
        fs::write(deep.join("f"), [0u8; 7]).unwrap();
        let tree = Tree::scan(&d, &Progress::default()).unwrap();
        assert_eq!((tree.node(ROOT).size, tree.node(ROOT).files), (7, 1));
        assert_eq!(tree.path((tree.nodes.len() - 1) as u32), deep.join("f"));
        assert_eq!(crate::sizes::walk(&d, &AtomicBool::new(false)), Some((7, 1)));
        drop(tree);
        fs::remove_dir_all(&d).unwrap();
    }

    /// Every ending Coxswain reads lands in a family, the old formats too: none is "Other".
    #[test]
    fn known_endings_have_a_family() {
        let want = [
            ("iff", Family::Image), ("tiff", Family::Image), ("tga", Family::Image), ("pcx", Family::Image),
            ("xpm", Family::Image), ("xbm", Family::Image), ("psd", Family::Image), ("jxl", Family::Image),
            ("exr", Family::Image), ("hdr", Family::Image), ("dds", Family::Image), ("qoi", Family::Image),
            ("pbm", Family::Image), ("cbz", Family::Document), ("cbr", Family::Document), ("mbox", Family::Document),
            ("msg", Family::Document), ("dbf", Family::Data), ("mod", Family::Audio), ("xm", Family::Audio),
            ("s3m", Family::Audio), ("it", Family::Audio), ("iso", Family::Disk), ("dmg", Family::Disk),
            ("vhd", Family::Disk), ("vhdx", Family::Disk), ("vmdk", Family::Disk), ("qcow2", Family::Disk),
            ("vdi", Family::Disk), ("wim", Family::Disk), ("pgn", Family::Document), ("fen", Family::Document), ("sgf", Family::Document),
            ("typ", Family::Document), ("eps", Family::Document), ("wpd", Family::Document), ("d2", Family::Code),
            ("cob", Family::Code), ("tbz2", Family::Archive),
        ];
        for (ext, f) in want {
            assert_eq!(family(ext), f, "{ext}");
        }
        let lists = [
            crate::pictures::endings(),
            crate::disk_image::endings(),
            crate::archive::endings().0,
            crate::extract::installed::OFFICE.to_vec(),
        ];
        for e in lists.concat() {
            let e = e.rsplit('.').next().unwrap();
            assert_ne!(family(e), Family::Other, "{e}");
        }
    }

    fn area(r: &Rect) -> f64 {
        r.w as f64 * r.h as f64
    }

    #[test]
    fn squarify_fills_the_rectangle_with_the_areas_and_near_squares() {
        let r = Rect { x: 10.0, y: 5.0, w: 600.0, h: 400.0 };
        let sizes = [6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let total: f64 = sizes.iter().sum();
        let areas: Vec<f64> = sizes.iter().map(|s| s / total * 240_000.0).collect();
        let out = squarify(&areas, r);
        assert_eq!(out.len(), areas.len());
        for (a, rect) in areas.iter().zip(&out) {
            assert!((area(rect) - a).abs() < 1.0, "{rect:?} has {a}");
            assert!(rect.x >= 10.0 - 1e-3 && rect.y >= 5.0 - 1e-3);
            assert!(rect.x + rect.w <= 610.0 + 1e-3 && rect.y + rect.h <= 405.0 + 1e-3);
            assert!(rect.w.max(rect.h) / rect.w.min(rect.h) < 3.0, "near a square: {rect:?}");
        }
        // No two overlap.
        for (i, a) in out.iter().enumerate() {
            for b in &out[i + 1..] {
                let ox = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
                let oy = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
                assert!(ox <= 1e-3 || oy <= 1e-3, "{a:?} and {b:?} overlap");
            }
        }
        assert!(squarify(&[], r).is_empty());
        assert_eq!(squarify(&[240_000.0], r), vec![r]);
    }

    fn tree(dir: &Path) {
        let _ = fs::remove_dir_all(dir);
        fs::create_dir_all(dir.join("photos/old")).unwrap();
        fs::create_dir_all(dir.join("code")).unwrap();
        fs::write(dir.join("photos/a.jpg"), vec![0u8; 6000]).unwrap();
        fs::write(dir.join("photos/old/b.JPG"), vec![0u8; 3000]).unwrap();
        fs::write(dir.join("code/main.rs"), vec![0u8; 900]).unwrap();
        fs::write(dir.join("code/x.rs"), vec![0u8; 100]).unwrap();
        fs::write(dir.join("film.mkv"), vec![0u8; 10_000]).unwrap();
        fs::write(dir.join("empty"), b"").unwrap();
    }

    #[test]
    fn a_scan_keeps_every_file_and_a_delete_takes_it_off_every_folder_above() {
        let d = std::env::temp_dir().join(format!("coxswain-space-{}", std::process::id()));
        tree(&d);
        let p = Progress::default();
        let mut t = Tree::scan(&d, &p).unwrap();
        assert_eq!(p.now(), (6, 20_000));
        assert_eq!((t.node(ROOT).size, t.node(ROOT).files), (20_000, 6));
        let names: Vec<String> = t.kids(ROOT).iter().map(|&k| t.node(k).label().into_owned()).collect();
        assert_eq!(names, ["film.mkv", "photos", "code", "empty"], "the largest first");
        let b = t.find(&d.join("photos/old/b.JPG")).unwrap();
        assert_eq!(t.path(b), d.join("photos/old/b.JPG"));
        assert_eq!(t.ext(b), "jpg");
        let photos = t.find(&d.join("photos")).unwrap();
        assert_eq!(t.toward(ROOT, b), Some(photos));
        assert!(t.within(b, photos) && !t.within(photos, b));

        let types = t.types(ROOT);
        assert_eq!(
            types.iter().map(|r| (r.ext.as_str(), r.bytes, r.files)).collect::<Vec<_>>(),
            [("mkv", 10_000, 1), ("jpg", 9000, 2), ("rs", 1000, 2), ("", 0, 1)]
        );
        assert_eq!((types[0].family, types[1].family, types[2].family), (Family::Video, Family::Image, Family::Code));
        assert!((types[0].share - 0.5).abs() < 1e-9);
        let largest: Vec<PathBuf> = t.largest(ROOT, 2).into_iter().map(|i| t.path(i)).collect();
        assert_eq!(largest, [d.join("film.mkv"), d.join("photos/a.jpg")]);

        assert!(t.remove(&d.join("photos/old/b.JPG")));
        assert!(!t.remove(&d.join("photos/old/b.JPG")), "once");
        assert_eq!((t.node(photos).size, t.node(photos).files), (6000, 1));
        assert_eq!((t.node(ROOT).size, t.node(ROOT).files), (17_000, 5));
        assert_eq!(t.find(&d.join("photos/old/b.JPG")), None);
        assert!(!t.alive(b) && t.alive(photos) && !t.alive(9999));

        let stopped = Progress::default();
        stopped.stop.store(true, Ordering::Relaxed);
        assert!(Tree::scan(&d, &stopped).is_none());
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn the_layout_nests_files_in_folders_and_merges_the_tiny_ones() {
        let d = std::env::temp_dir().join(format!("coxswain-space-layout-{}", std::process::id()));
        tree(&d);
        // Many tiny files beside the large ones.
        for i in 0..50 {
            fs::write(d.join(format!("code/t{i}.txt")), [0u8; 2]).unwrap();
        }
        let t = Tree::scan(&d, &Progress::default()).unwrap();
        let whole = Rect { x: 0.0, y: 0.0, w: 200.0, h: 100.0 };
        let opts = Opts { pad: 1.0, head: 0.0, min_area: 4.0 };
        let tiles = t.layout(ROOT, whole, &opts);
        let files: Vec<&Tile> = tiles.iter().filter(|x| x.kind == Kind::File).collect();
        // Each file's area follows its bytes; folders are framed, so a little less there.
        let film = files.iter().find(|x| t.path(x.id) == d.join("film.mkv")).unwrap();
        let share = area(&film.rect) / 20_000.0;
        assert!((share - 10_000.0 / 20_100.0).abs() < 0.01, "{share}");
        assert_eq!(film.family, Family::Video);
        // Every file lies inside its folder's rectangle, drawn after it.
        for (i, f) in tiles.iter().enumerate() {
            let parent = t.node(f.id).parent;
            if f.kind == Kind::File && parent != ROOT {
                let at = tiles[..i].iter().position(|x| x.kind == Kind::Folder && x.id == parent).unwrap();
                let p = tiles[at].rect;
                assert!(f.rect.x >= p.x && f.rect.y >= p.y && f.rect.x + f.rect.w <= p.x + p.w + 1e-3);
                assert_eq!(f.depth, tiles[at].depth + 1);
            }
        }
        // The fifty tiny ones are one block in code, not fifty rectangles.
        let code = t.find(&d.join("code")).unwrap();
        let small: Vec<&Tile> = tiles.iter().filter(|x| x.kind == Kind::Small).collect();
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].id, code);
        assert!(small[0].files >= 50, "the block counts the files it stands for: {}", small[0].files);
        assert!(!files.iter().any(|x| t.node(x.id).label().starts_with("t")));
        // A title strip on the folders big enough for one, and a zoom shows one folder alone.
        let headed = t.layout(ROOT, whole, &Opts { head: 10.0, ..opts });
        let photos = t.find(&d.join("photos")).unwrap();
        let p = headed.iter().find(|x| x.id == photos && x.kind == Kind::Folder).unwrap().rect;
        let a = headed.iter().find(|x| t.path(x.id) == d.join("photos/a.jpg")).unwrap().rect;
        assert!(a.y >= p.y + 10.0, "{a:?} below the strip of {p:?}");
        let zoomed = t.layout(photos, whole, &opts);
        assert!(zoomed.iter().all(|x| x.kind != Kind::Folder || t.within(x.id, photos)));
        assert_eq!(zoomed.iter().filter(|x| x.kind == Kind::File).count(), 2);
        fs::remove_dir_all(d).unwrap();
    }

    /// A name that is not UTF-8 (Linux and the BSDs allow any bytes) is kept as it is: the path
    /// leads to that very file, not to one whose name has a replacement character.
    #[cfg(unix)]
    #[test]
    fn a_name_that_is_not_utf8_leads_to_that_file() {
        use std::os::unix::ffi::OsStrExt;
        let d = std::env::temp_dir().join(format!("coxswain-space-bytes-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        let odd = d.join(OsStr::from_bytes(b"caf\xe9.txt"));
        // Where the lossy name would lead: a different file.
        let lossy = d.join("caf\u{fffd}.txt");
        if fs::write(&odd, [0u8; 300]).is_err() {
            return; // a file system that wants UTF-8 names (some macOS ones)
        }
        fs::write(&lossy, [0u8; 5]).unwrap();
        let mut t = Tree::scan(&d, &Progress::default()).unwrap();
        let big = t.largest(ROOT, 1)[0];
        assert_eq!(t.path(big), odd);
        assert_eq!(t.find(&odd), Some(big));
        assert_eq!(t.node(big).label(), "caf\u{fffd}.txt");
        // A delete of it takes that file off the tree, and the other stays.
        fs::remove_file(t.path(big)).unwrap();
        assert!(lossy.exists());
        assert!(t.remove(&odd));
        assert_eq!(t.node(ROOT).size, 5);
        fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn another_disk_is_a_block_of_its_own_not_to_scale() {
        let f = |name: &str, size: u64| Found { name: OsStr::new(name).into(), size, files: 1, online: 0, mount: false, mounts: 0, kids: None };
        let disk = Found { name: OsStr::new("usb").into(), size: 0, files: 0, online: 0, mount: true, mounts: 1, kids: None };
        let media = Found { name: OsStr::new("media").into(), size: 0, files: 0, online: 0, mount: false, mounts: 1, kids: Some(vec![disk]) };
        let top = Found { name: OsStr::new("/").into(), size: 1000, files: 2, online: 0, mount: false, mounts: 1, kids: Some(vec![f("a.iso", 900), f("b.txt", 100), media]) };
        let mut t = Tree { root: "/".into(), nodes: vec![], online: 0 };
        t.add(top, ROOT);
        let tiles = t.layout(ROOT, Rect { x: 0.0, y: 0.0, w: 100.0, h: 100.0 }, &Opts { pad: 0.0, head: 0.0, min_area: 1.0 });
        let mount = tiles.iter().find(|x| x.kind == Kind::Mount).expect("a block for the disk");
        assert_eq!(t.path(mount.id), Path::new("/media/usb"));
        let a = mount.rect.w * mount.rect.h;
        assert!(a > 100.0 && a < 1000.0, "a small share: {a}");
        let iso = tiles.iter().find(|x| x.kind == Kind::File && t.node(x.id).size == 900).unwrap();
        assert!((iso.rect.w * iso.rect.h - 0.96 * 0.9 * 10_000.0).abs() < 50.0);
        assert!(t.types(ROOT).iter().all(|r| !r.ext.is_empty()), "a mount is no file of no type");
        assert_eq!(t.largest(ROOT, 10).len(), 2);
    }

    #[test]
    fn space_walks_the_mac_library_but_not_its_private_vaults() {
        use crate::fs::{protected_for_space_in as space, protected_in as sizes};
        let h = Path::new("/Users/ada");
        let l = |p: &str| h.join("Library").join(p);
        for p in ["Caches", "Developer/Xcode", "Application Support/Code", ""] {
            assert!(!space(h, &l(p)), "{p} counts in Space");
            assert!(sizes(h, &l(p)) || p.is_empty(), "{p} stays out of the background sizes");
        }
        for p in ["Containers", "Group Containers/x", "Mail", "Messages", "Safari", "Application Support/AddressBook"] {
            assert!(space(h, &l(p)), "{p} asks for permission");
        }
        assert!(space(h, &h.join(".Trash")) && space(h, &h.join("Pictures/Photos Library.photoslibrary")));
        assert!(!space(h, &h.join("Documents")));
    }

    #[test]
    fn families_by_extension() {
        assert_eq!(family("jpg"), Family::Image);
        assert_eq!(family("mkv"), Family::Video);
        assert_eq!(family("flac"), Family::Audio);
        assert_eq!(family("zst"), Family::Archive);
        assert_eq!(family("pdf"), Family::Document);
        assert_eq!(family("rs"), Family::Code);
        assert_eq!(family("sqlite"), Family::Data);
        assert_eq!(family("so"), Family::Program);
        assert_eq!(family("iso"), Family::Disk);
        assert_eq!(family(""), Family::Other);
        assert_eq!(ext_of(".bashrc"), "");
        assert_eq!(ext_of("A.TAR.GZ"), "gz");
        let colours: std::collections::HashSet<_> = Family::ALL.iter().map(|f| f.colour()).collect();
        assert_eq!(colours.len(), Family::ALL.len(), "a colour each");
    }
}
