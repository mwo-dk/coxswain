//! The built-in editor (docs/commands/editor.md): a file read for editing and saved again as
//! it was, in its encoding (UTF-8, with or without its byte order mark, UTF-16 by its mark, or
//! Windows-1252), with its line endings and permissions, through a file beside it so a failed
//! save never leaves half a file; and the text being edited (`Buffer`), with its cursor, its
//! selection and undo, for the terminal app. The desktop app edits in a text field of its own
//! and opens and saves through here.
//!
//! A file that changed on the disk since it was opened is not saved over without asking: its
//! `Stamp` (length and time of change) is taken on opening and compared before saving.

use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Files bigger than this are not opened for editing: the viewer reads them.
pub const MOST: u64 = 16 << 20;

/// What a file is written in, kept from opening to saving.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Encoding {
    Utf8,
    /// UTF-8 with its byte order mark first.
    Utf8Bom,
    Utf16Le,
    Utf16Be,
    Windows1252,
}

impl Encoding {
    /// Its name, as the title shows it.
    pub fn name(self) -> &'static str {
        match self {
            Encoding::Utf8 => "UTF-8",
            Encoding::Utf8Bom => "UTF-8 BOM",
            Encoding::Utf16Le => "UTF-16 LE",
            Encoding::Utf16Be => "UTF-16 BE",
            Encoding::Windows1252 => "Windows-1252",
        }
    }
}

/// How a file was when it was read: changed since when any of it differs. The time of its last
/// change in seconds since 1970 and nanoseconds, each a number JavaScript holds exactly; 0 when
/// the system does not say.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Stamp {
    pub len: u64,
    pub secs: u64,
    pub nanos: u32,
}

impl Stamp {
    pub(crate) fn of(m: &std::fs::Metadata) -> Stamp {
        let d = m.modified().ok().and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok()).unwrap_or_default();
        Stamp { len: m.len(), secs: d.as_secs(), nanos: d.subsec_nanos() }
    }
}

/// A file read for editing.
#[derive(Clone, Debug, Serialize)]
pub struct Doc {
    /// Its text, lines ended by `\n` whatever the file used.
    pub text: String,
    pub encoding: Encoding,
    /// Its lines end in `\r\n` (most of them did): saved so again.
    pub crlf: bool,
    pub stamp: Stamp,
}

/// Why a file could not be saved.
#[derive(Debug, PartialEq, Eq)]
pub enum SaveError {
    /// It changed on the disk since it was opened: saving would lose that.
    Changed,
    /// The text holds a character its encoding (Windows-1252) has no byte for.
    Unencodable(char),
    Io(String),
}

impl std::fmt::Display for SaveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SaveError::Changed => f.write_str(&crate::t!("editor.changed")),
            SaveError::Unencodable(c) => f.write_str(&crate::t!("editor.unencodable", "char" => c)),
            SaveError::Io(e) => f.write_str(e),
        }
    }
}

/// Read `path` for editing: a regular file of at most `MOST` bytes, and text (no zero byte
/// outside UTF-16). UTF-8 only when all of it is, so a save never changes a byte it did not
/// touch: else it is Windows-1252, which every byte is.
pub fn open(path: &Path) -> Result<Doc, String> {
    let m = std::fs::metadata(path).map_err(|e| e.to_string())?;
    if !m.is_file() {
        return Err(crate::t!("editor.not_a_file"));
    }
    if m.len() > MOST {
        return Err(crate::t!("editor.too_big", "size" => crate::settings::human(m.len())));
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let (text, encoding) = decode(&bytes).ok_or_else(|| crate::t!("editor.binary"))?;
    let (crlf, lone) = (text.matches("\r\n").count(), text.matches('\n').count());
    let crlf = crlf > 0 && crlf * 2 >= lone;
    let text = if text.contains("\r\n") { text.replace("\r\n", "\n") } else { text };
    Ok(Doc { text, encoding, crlf, stamp: Stamp::of(&m) })
}

/// Bytes as text in the encoding they are in; `None` when they are no text.
pub(crate) fn decode(bytes: &[u8]) -> Option<(String, Encoding)> {
    let utf16 = |rest: &[u8], unit: fn([u8; 2]) -> u16| {
        if rest.len() % 2 == 1 {
            return None;
        }
        let units = rest.chunks_exact(2).map(|c| unit([c[0], c[1]]));
        char::decode_utf16(units).collect::<Result<String, _>>().ok()
    };
    if let Some(rest) = bytes.strip_prefix(&[0xff, 0xfe]) {
        return utf16(rest, u16::from_le_bytes).map(|t| (t, Encoding::Utf16Le));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xfe, 0xff]) {
        return utf16(rest, u16::from_be_bytes).map(|t| (t, Encoding::Utf16Be));
    }
    if bytes.contains(&0) {
        return None;
    }
    if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        return std::str::from_utf8(rest).ok().map(|t| (t.to_owned(), Encoding::Utf8Bom));
    }
    match std::str::from_utf8(bytes) {
        Ok(t) => Some((t.to_owned(), Encoding::Utf8)),
        Err(_) => Some((crate::view::decode_line(bytes, false), Encoding::Windows1252)),
    }
}

/// `text` as the bytes of a file in `encoding`, its lines ended in `\r\n` when `crlf`; the first
/// character Windows-1252 has no byte for, when it has none.
pub fn encode(text: &str, encoding: Encoding, crlf: bool) -> Result<Vec<u8>, char> {
    let text = if crlf { std::borrow::Cow::Owned(text.replace('\n', "\r\n")) } else { std::borrow::Cow::Borrowed(text) };
    Ok(match encoding {
        Encoding::Utf8 => text.as_bytes().to_vec(),
        Encoding::Utf8Bom => [&[0xef, 0xbb, 0xbf][..], text.as_bytes()].concat(),
        Encoding::Utf16Le => [0xff, 0xfe].into_iter().chain(text.encode_utf16().flat_map(u16::to_le_bytes)).collect(),
        Encoding::Utf16Be => [0xfe, 0xff].into_iter().chain(text.encode_utf16().flat_map(u16::to_be_bytes)).collect(),
        Encoding::Windows1252 => text.chars().map(cp1252_byte).collect::<Option<Vec<u8>>>().ok_or_else(|| {
            text.chars().find(|&c| cp1252_byte(c).is_none()).unwrap_or('?')
        })?,
    })
}

/// The Windows-1252 byte of `c`, as `view::decode_line` reads it back.
fn cp1252_byte(c: char) -> Option<u8> {
    let n = c as u32;
    if n < 0x100 && !(0x80..0xa0).contains(&n) {
        return Some(n as u8);
    }
    (0x80u8..0xa0).find(|&b| crate::view::decode_line(&[b], false).starts_with(c))
}

/// Whether `path` changed on the disk since `stamp` (or is gone).
pub fn changed(path: &Path, stamp: &Stamp) -> bool {
    std::fs::metadata(path).map_or(true, |m| Stamp::of(&m) != *stamp)
}

/// Save `text` to `path` as `doc` read it (its encoding, line endings), unless the file changed
/// since `doc.stamp` and `force` is not given. A link is saved through to its file; the file
/// keeps its permissions. A file this user may not write (read-only, another's) is refused, as
/// it would be in place: a rename never gets round that. Written beside the file and renamed
/// over it, so a failed save leaves the file as it was; written in place instead, so it stays
/// the very file with all it carries, when it has other hard links, is another user's, has an
/// ACL or extended attributes (or that cannot be told), or its folder takes no new file. The
/// new stamp.
pub fn save(path: &Path, text: &str, encoding: Encoding, crlf: bool, stamp: Option<&Stamp>) -> Result<Stamp, SaveError> {
    let bytes = encode(text, encoding, crlf).map_err(SaveError::Unencodable)?;
    let io = |e: std::io::Error| SaveError::Io(e.to_string());
    if stamp.is_some_and(|s| changed(path, s)) {
        return Err(SaveError::Changed);
    }
    let target: PathBuf = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    // Opened for writing first (not emptied): what this user may not write fails here.
    let mut o = std::fs::OpenOptions::new();
    o.write(true);
    // A named pipe is never waited on.
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::custom_flags(&mut o, libc::O_NONBLOCK);
    let existing = match o.open(&target) {
        Ok(f) => Some(f),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(io(e)),
    };
    let old = existing.as_ref().map(|f| f.metadata()).transpose().map_err(io)?;
    let in_place = match (&existing, &old) {
        (Some(f), Some(m)) => !renamed_over_safely(f, m),
        _ => false,
    };
    if !in_place {
        match beside(&target, &bytes, old.as_ref()) {
            Ok(()) => return std::fs::metadata(&target).map(|m| Stamp::of(&m)).map_err(io),
            // No new file there, or its group cannot be given to one: in place, below.
            Err((_, true)) => {}
            // Anything else (a full disk, a quota) is said, the file left as it was.
            Err((e, false)) => return Err(io(e)),
        }
    }
    let mut f = match existing {
        Some(f) => f,
        None => std::fs::OpenOptions::new().write(true).create(true).truncate(true).open(&target).map_err(io)?,
    };
    // Written over, then cut to length: never emptied first.
    f.write_all(&bytes).map_err(io)?;
    f.set_len(bytes.len() as u64).map_err(io)?;
    f.sync_all().map_err(io)?;
    std::fs::metadata(&target).map(|m| Stamp::of(&m)).map_err(io)
}

/// Whether a new file renamed over `f` would be the same to its user: no other hard links,
/// this user's own, no ACL and no extended attributes.
#[cfg(unix)]
fn renamed_over_safely(f: &std::fs::File, m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt;
    // SAFETY: geteuid has no preconditions.
    m.nlink() == 1 && m.uid() == unsafe { libc::geteuid() } && !extra_permissions(f)
}

// ponytail: the same test as find and replace's (pinned.rs in #377); one copy once both are in.
/// The file has permissions beyond its bits (an ACL) or extended attributes: written anew it
/// would lose them. It fails closed: an error, or a system where this cannot be told, counts
/// as extra. SELinux's own label is no reason; a file system that cannot hold extended
/// attributes at all has none to lose.
#[cfg(unix)]
fn extra_permissions(f: &std::fs::File) -> bool {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    {
        // ACLs are the xattrs `system.posix_acl_*` here: listed with the rest.
        xattrs_listed(f)
    }
    #[cfg(target_os = "macos")]
    {
        unsafe extern "C" {
            fn acl_get_fd(fd: libc::c_int) -> *mut libc::c_void;
            fn acl_free(obj: *mut libc::c_void) -> libc::c_int;
        }
        // SAFETY: an open file; the ACL got is freed at once.
        let acl = unsafe { acl_get_fd(std::os::fd::AsRawFd::as_raw_fd(f)) };
        if !acl.is_null() {
            // SAFETY: as above.
            unsafe { acl_free(acl) };
            return true;
        }
        // No extended ACL is ENOENT; anything else is not knowing.
        std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOENT) || xattrs_listed(f)
    }
    #[cfg(any(target_os = "freebsd", target_os = "netbsd"))]
    {
        unsafe extern "C" {
            fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
            fn acl_is_trivial_np(acl: *const libc::c_void, trivial: *mut libc::c_int) -> libc::c_int;
            fn acl_free(obj: *mut libc::c_void) -> libc::c_int;
        }
        // ACL_TYPE_NFS4 (ZFS), else ACL_TYPE_ACCESS (POSIX.1e, UFS): whichever it has.
        let mut acl = std::ptr::null_mut();
        for kind in [4, 2] {
            // SAFETY: an open file and an ACL type.
            acl = unsafe { acl_get_fd_np(std::os::fd::AsRawFd::as_raw_fd(f), kind) };
            if !acl.is_null() {
                break;
            }
        }
        if acl.is_null() {
            // No ACL support on this file system at all is no ACL; anything else is not knowing.
            if std::io::Error::last_os_error().raw_os_error() != Some(libc::EOPNOTSUPP) {
                return true;
            }
        } else {
            let mut trivial = 0;
            // SAFETY: the ACL got above, freed at once.
            let ok = unsafe { acl_is_trivial_np(acl, &mut trivial) } == 0;
            // SAFETY: as above.
            unsafe { acl_free(acl) };
            if !ok || trivial != 1 {
                return true;
            }
        }
        // SAFETY: an open file; no buffer, only the length is asked for.
        let n = unsafe { libc::extattr_list_fd(std::os::fd::AsRawFd::as_raw_fd(f), libc::EXTATTR_NAMESPACE_USER, std::ptr::null_mut(), 0) };
        n > 0 || n < 0 && std::io::Error::last_os_error().raw_os_error() != Some(libc::EOPNOTSUPP)
    }
    // OpenBSD has neither ACLs nor extended attributes: nothing to lose.
    #[cfg(target_os = "openbsd")]
    {
        let _ = f;
        false
    }
    #[cfg(any(target_os = "illumos", target_os = "solaris"))]
    {
        #[link(name = "sec")]
        unsafe extern "C" {
            fn facl_get(fd: libc::c_int, flag: libc::c_int, aclp: *mut *mut libc::c_void) -> libc::c_int;
            fn acl_free(acl: *mut libc::c_void);
        }
        /// `facl_get` gives no ACL (a null one) when the file's is trivial: its mode bits alone.
        const ACL_NO_TRIVIAL: libc::c_int = 0x2;
        let mut acl = std::ptr::null_mut();
        // SAFETY: an open file and a place for the ACL got, freed at once.
        if unsafe { facl_get(std::os::fd::AsRawFd::as_raw_fd(f), ACL_NO_TRIVIAL, &mut acl) } != 0 {
            return true;
        }
        if !acl.is_null() {
            // SAFETY: as above.
            unsafe { acl_free(acl) };
            return true;
        }
        // Extended attributes other than the system's own: 1 when there are, -1 not knowing.
        // SAFETY: an open file.
        unsafe { libc::fpathconf(std::os::fd::AsRawFd::as_raw_fd(f), libc::_PC_XATTR_EXISTS) != 0 }
    }
    // Elsewhere it cannot be told here: left as it is.
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd", target_os = "illumos", target_os = "solaris")))]
    {
        let _ = f;
        true
    }
}

/// Extended attributes on the file, SELinux's label aside; an error but "none possible here"
/// counts as some.
#[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
fn xattrs_listed(f: &std::fs::File) -> bool {
    let mut names = vec![0u8; 64 * 1024];
    // SAFETY: an open file and a buffer of the length given.
    #[cfg(target_os = "macos")]
    let n = unsafe { libc::flistxattr(std::os::fd::AsRawFd::as_raw_fd(f), names.as_mut_ptr().cast(), names.len(), 0) };
    // SAFETY: as above.
    #[cfg(not(target_os = "macos"))]
    let n = unsafe { libc::flistxattr(std::os::fd::AsRawFd::as_raw_fd(f), names.as_mut_ptr().cast(), names.len()) };
    if n < 0 {
        return std::io::Error::last_os_error().raw_os_error() != Some(libc::ENOTSUP);
    }
    names[..n as usize].split(|&b| b == 0).any(|name| !name.is_empty() && name != b"security.selinux")
}

#[cfg(not(unix))]
fn renamed_over_safely(_: &std::fs::File, _: &std::fs::Metadata) -> bool {
    true
}

/// `bytes` written to a file beside `target`, for this user alone until it has the old file's
/// group and permissions (set before a byte is written), then renamed over it. The error, and
/// whether to write in place instead: no new file can be made there, or the group not given.
fn beside(target: &Path, bytes: &[u8], old: Option<&std::fs::Metadata>) -> Result<(), (std::io::Error, bool)> {
    let dir = target.parent().unwrap_or(Path::new("."));
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let tmp = dir.join(format!(".{name}.coxswain-{}.tmp", std::process::id()));
    let mut o = std::fs::OpenOptions::new();
    o.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut o, 0o600);
    let mut f = o.open(&tmp).map_err(|e| {
        let no_room = matches!(e.kind(), std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::ReadOnlyFilesystem);
        (e, no_room)
    })?;
    let done = (|| {
        #[cfg(unix)]
        if let Some(m) = old {
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            if f.metadata().map_err(|e| (e, false))?.gid() != m.gid() {
                std::os::unix::fs::fchown(&f, None, Some(m.gid())).map_err(|e| (e, true))?;
            }
            // Its permission bits, not its set-user or set-group ones.
            f.set_permissions(std::fs::Permissions::from_mode(m.mode() & 0o777)).map_err(|e| (e, false))?;
        }
        #[cfg(not(unix))]
        let _ = old;
        f.write_all(bytes).and_then(|_| f.sync_all()).map_err(|e| (e, false))?;
        drop(f);
        std::fs::rename(&tmp, target).map_err(|e| (e, false))
    })();
    if done.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    done
}

// ---------------------------------------------------------------- the text being edited

/// A place in the text: a line, and a byte in it (always where a character starts).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

/// One change, as undo takes it back: `text` put in at `at`, or taken out from there.
#[derive(Clone, Debug)]
struct Change {
    at: Pos,
    text: String,
    put: bool,
    /// Where the cursor stood before it.
    before: Pos,
}

/// The text being edited, as lines, with the cursor, the selection and undo.
pub struct Buffer {
    pub lines: Vec<String>,
    pub cursor: Pos,
    /// Where the selection started: it runs from here to the cursor.
    pub anchor: Option<Pos>,
    /// The column the cursor keeps going up and down (in characters).
    want: usize,
    /// What one Tab puts in: a tab when the file indents with tabs, else four spaces.
    pub indent: String,
    undo: Vec<Vec<Change>>,
    redo: Vec<Vec<Change>>,
    /// Typing goes into the last undo step until something else is done.
    typing: bool,
    /// Changes made since the last save (undo counts back).
    changes: isize,
    /// The first line changed since `take_touched` was asked.
    touched: Option<usize>,
}

impl Buffer {
    pub fn new(text: &str) -> Buffer {
        let lines: Vec<String> = text.split('\n').map(str::to_owned).collect();
        let tabs = lines.iter().filter(|l| l.starts_with('\t')).count();
        let spaces = lines.iter().filter(|l| l.starts_with("  ")).count();
        let indent = if tabs > spaces { "\t".into() } else { "    ".into() };
        Buffer { lines, cursor: Pos::default(), anchor: None, want: 0, indent, undo: vec![], redo: vec![], typing: false, changes: 0, touched: None }
    }

    /// The text, lines joined by `\n`.
    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    /// Changed since it was opened or saved.
    pub fn dirty(&self) -> bool {
        self.changes != 0
    }

    /// Saved now: what it holds counts as unchanged.
    pub fn saved(&mut self) {
        self.changes = 0;
        self.typing = false;
    }

    /// The selection, its start first; `None` when nothing is selected.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| if a < self.cursor { (a, self.cursor) } else { (self.cursor, a) })
    }

    /// The text from `a` to `b`.
    pub fn slice(&self, a: Pos, b: Pos) -> String {
        if a.line == b.line {
            return self.lines[a.line][a.col..b.col].to_owned();
        }
        let mut s = self.lines[a.line][a.col..].to_owned();
        for l in &self.lines[a.line + 1..b.line] {
            s.push('\n');
            s.push_str(l);
        }
        s.push('\n');
        s.push_str(&self.lines[b.line][..b.col]);
        s
    }

    /// The selected text, if any.
    pub fn selected(&self) -> Option<String> {
        self.selection().map(|(a, b)| self.slice(a, b))
    }

    /// The first line changed since this was last asked, if any (what a highlighter must
    /// read again from).
    pub fn take_touched(&mut self) -> Option<usize> {
        self.touched.take()
    }

    fn touch(&mut self, line: usize) {
        self.touched = Some(self.touched.map_or(line, |t| t.min(line)));
    }

    /// `text` put in at `at`; where it ends.
    fn put_raw(&mut self, at: Pos, text: &str) -> Pos {
        self.touch(at.line);
        let rest = self.lines[at.line].split_off(at.col);
        let mut parts = text.split('\n');
        self.lines[at.line].push_str(parts.next().unwrap_or(""));
        let mut end = Pos { line: at.line, col: self.lines[at.line].len() };
        let new: Vec<String> = parts.map(str::to_owned).collect();
        if !new.is_empty() {
            end = Pos { line: at.line + new.len(), col: new.last().map_or(0, String::len) };
            self.lines.splice(at.line + 1..at.line + 1, new);
        }
        self.lines[end.line].push_str(&rest);
        end
    }

    /// The text from `a` to `b` taken out; what it was.
    fn take_raw(&mut self, a: Pos, b: Pos) -> String {
        self.touch(a.line);
        let s = self.slice(a, b);
        let tail = self.lines[b.line][b.col..].to_owned();
        self.lines[a.line].truncate(a.col);
        self.lines[a.line].push_str(&tail);
        self.lines.drain(a.line + 1..=b.line);
        s
    }

    /// One change, kept for undo: into the last step with `merge`, else a step of its own.
    fn record(&mut self, c: Change, merge: bool) {
        self.redo.clear();
        self.changes += 1;
        match self.undo.last_mut() {
            Some(step) if merge => step.push(c),
            _ => self.undo.push(vec![c]),
        }
    }

    /// The selection taken out, kept for undo as part of the step that follows (`merge`).
    fn take_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else { return false };
        let before = self.cursor;
        let text = self.take_raw(a, b);
        self.record(Change { at: a, text, put: false, before }, false);
        self.typing = false;
        self.cursor = a;
        self.anchor = None;
        true
    }

    /// `text` typed or pasted at the cursor, over the selection. One character typed after
    /// another is undone as one step; a space or a new line starts the next.
    pub fn insert(&mut self, text: &str) {
        let replaced = self.take_selection();
        let before = self.cursor;
        let at = self.cursor;
        self.cursor = self.put_raw(at, text);
        let word = text.chars().count() == 1 && !text.contains([' ', '\n', '\t']);
        // In one step with the selection it replaced, or with the letters typed before it.
        self.record(Change { at, text: text.to_owned(), put: true, before }, replaced || word && self.typing);
        self.typing = word;
        self.want = self.col_chars();
    }

    /// A new line, indented as the line it was started from.
    pub fn newline(&mut self) {
        let line = &self.lines[self.cursor.line];
        let indent: String = line.chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let indent = &indent[..indent.len().min(self.cursor.col)];
        self.insert(&format!("\n{indent}"));
    }

    /// Backspace: the selection, else the character before the cursor (or the line break).
    pub fn backspace(&mut self) {
        if self.take_selection() {
            return;
        }
        let b = self.cursor;
        let a = self.before(b);
        self.delete_span(a, b);
    }

    /// Delete: the selection, else the character under the cursor (or the line break).
    pub fn delete(&mut self) {
        if self.take_selection() {
            return;
        }
        let a = self.cursor;
        let b = self.after(a);
        self.delete_span(a, b);
    }

    fn delete_span(&mut self, a: Pos, b: Pos) {
        if a == b {
            return;
        }
        let before = self.cursor;
        let text = self.take_raw(a, b);
        self.record(Change { at: a, text, put: false, before }, false);
        self.typing = false;
        self.cursor = a;
        self.want = self.col_chars();
    }

    /// The selection taken out and given back (cut).
    pub fn cut(&mut self) -> Option<String> {
        let s = self.selected()?;
        self.take_selection();
        Some(s)
    }

    /// Take back the last step; false when there is none.
    pub fn undo(&mut self) -> bool {
        let Some(step) = self.undo.pop() else { return false };
        for c in step.iter().rev() {
            if c.put {
                let end = self.end_of(c.at, &c.text);
                self.take_raw(c.at, end);
            } else {
                self.put_raw(c.at, &c.text);
            }
            self.cursor = c.before;
        }
        self.changes -= step.len() as isize;
        self.redo.push(step);
        self.after_jump();
        true
    }

    /// Do again what undo took back; false when there is nothing.
    pub fn redo(&mut self) -> bool {
        let Some(step) = self.redo.pop() else { return false };
        for c in &step {
            if c.put {
                self.cursor = self.put_raw(c.at, &c.text);
            } else {
                let end = self.end_of(c.at, &c.text);
                self.take_raw(c.at, end);
                self.cursor = c.at;
            }
        }
        self.changes += step.len() as isize;
        self.undo.push(step);
        self.after_jump();
        true
    }

    fn after_jump(&mut self) {
        self.anchor = None;
        self.typing = false;
        self.want = self.col_chars();
    }

    /// Where `text` put in at `at` ends.
    fn end_of(&self, at: Pos, text: &str) -> Pos {
        match text.rfind('\n') {
            Some(i) => Pos { line: at.line + text.matches('\n').count(), col: text.len() - i - 1 },
            None => Pos { line: at.line, col: at.col + text.len() },
        }
    }

    /// The place one character before `p` (the end of the line before at a line's start).
    fn before(&self, p: Pos) -> Pos {
        if p.col > 0 {
            let l = &self.lines[p.line];
            let col = l[..p.col].char_indices().last().map_or(0, |(i, _)| i);
            Pos { line: p.line, col }
        } else if p.line > 0 {
            Pos { line: p.line - 1, col: self.lines[p.line - 1].len() }
        } else {
            p
        }
    }

    /// The place one character after `p`.
    fn after(&self, p: Pos) -> Pos {
        let l = &self.lines[p.line];
        if p.col < l.len() {
            Pos { line: p.line, col: p.col + l[p.col..].chars().next().map_or(1, char::len_utf8) }
        } else if p.line + 1 < self.lines.len() {
            Pos { line: p.line + 1, col: 0 }
        } else {
            p
        }
    }

    /// The cursor's column in characters.
    fn col_chars(&self) -> usize {
        self.lines[self.cursor.line][..self.cursor.col].chars().count()
    }

    /// The byte of line `line` at character column `chars` (its end when shorter).
    fn byte_at(&self, line: usize, chars: usize) -> usize {
        let l = &self.lines[line];
        l.char_indices().nth(chars).map_or(l.len(), |(i, _)| i)
    }

    /// Start or end a selection as the cursor moves: kept with Shift, dropped without.
    fn select(&mut self, keep: bool) {
        self.typing = false;
        if keep {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
    }

    pub fn left(&mut self, select: bool) {
        self.select(select);
        self.cursor = self.before(self.cursor);
        self.want = self.col_chars();
    }

    pub fn right(&mut self, select: bool) {
        self.select(select);
        self.cursor = self.after(self.cursor);
        self.want = self.col_chars();
    }

    /// `by` lines up (negative) or down, keeping the column it had.
    pub fn vertical(&mut self, by: isize, select: bool) {
        self.select(select);
        let line = (self.cursor.line as isize + by).clamp(0, self.lines.len() as isize - 1) as usize;
        self.cursor = Pos { line, col: self.byte_at(line, self.want) };
    }

    /// Home: the line's first character that is no space, then its very start.
    pub fn home(&mut self, select: bool) {
        self.select(select);
        let l = &self.lines[self.cursor.line];
        let text_at = l.len() - l.trim_start_matches([' ', '\t']).len();
        self.cursor.col = if self.cursor.col == text_at { 0 } else { text_at };
        self.want = self.col_chars();
    }

    pub fn end(&mut self, select: bool) {
        self.select(select);
        self.cursor.col = self.lines[self.cursor.line].len();
        self.want = self.col_chars();
    }

    /// The start of the text, or (`end`) its end.
    pub fn edge(&mut self, end: bool, select: bool) {
        self.select(select);
        self.cursor = if end { Pos { line: self.lines.len() - 1, col: self.lines.last().map_or(0, String::len) } } else { Pos::default() };
        self.want = self.col_chars();
    }

    /// A word to the left or right (Ctrl+arrows): over spaces, then over the word.
    pub fn word(&mut self, right: bool, select: bool) {
        self.select(select);
        let at = self.cursor;
        let step = |b: &Buffer, p: Pos| if right { b.after(p) } else { b.before(p) };
        let ch = |b: &Buffer, p: Pos| {
            let q = if right { p } else { b.before(p) };
            b.lines[q.line][q.col..].chars().next()
        };
        let mut p = at;
        while ch(self, p).is_some_and(|c| !c.is_alphanumeric() && c != '_') && step(self, p) != p {
            p = step(self, p);
            if p.line != at.line {
                break;
            }
        }
        while ch(self, p).is_some_and(|c| c.is_alphanumeric() || c == '_') && step(self, p) != p {
            p = step(self, p);
        }
        self.cursor = if p == at { step(self, p) } else { p };
        self.want = self.col_chars();
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos::default());
        self.edge(true, true);
    }

    /// Go to line `n` (from 1), at its start.
    pub fn go_to(&mut self, n: usize) {
        self.anchor = None;
        self.typing = false;
        self.cursor = Pos { line: n.saturating_sub(1).min(self.lines.len() - 1), col: 0 };
        self.want = 0;
    }

    /// Find `q` (case aside) after the cursor, or (`back`) before the selection, around the end
    /// of the text; the hit is selected. False when it is nowhere.
    pub fn find(&mut self, q: &str, back: bool) -> bool {
        if q.is_empty() {
            return false;
        }
        let q = fold(q);
        let n = self.lines.len();
        let from = self.selection().map_or(self.cursor, |(a, b)| if back { a } else { b });
        // Line by line from the cursor's, around the end, back to it.
        for k in 0..=n {
            let line = if back { (from.line + n * 2 - k) % n } else { (from.line + k) % n };
            let hits = folded_hits(&self.lines[line], &q);
            let hit = if back {
                hits.into_iter().rev().find(|&(i, _)| k > 0 || i < from.col)
            } else {
                hits.into_iter().find(|&(i, _)| k > 0 && k < n || k == 0 && i >= from.col || k == n && i < from.col)
            };
            if let Some((i, end)) = hit {
                self.anchor = Some(Pos { line, col: i });
                self.cursor = Pos { line, col: end };
                self.typing = false;
                self.want = self.col_chars();
                return true;
            }
        }
        false
    }

    /// Replace every `q` (case aside) with `with`, in one undo step; how many.
    pub fn replace_all(&mut self, q: &str, with: &str) -> usize {
        if q.is_empty() {
            return 0;
        }
        let ql = fold(q);
        let mut hits = vec![];
        for (line, l) in self.lines.iter().enumerate() {
            hits.extend(folded_hits(l, &ql).into_iter().map(|(a, b)| (Pos { line, col: a }, b)));
        }
        if hits.is_empty() {
            return 0;
        }
        let before = self.cursor;
        let mut step = vec![];
        // From the end, so the places before stay where they are.
        for (at, end) in hits.iter().rev() {
            let end = Pos { line: at.line, col: *end };
            let text = self.take_raw(*at, end);
            step.push(Change { at: *at, text, put: false, before });
            self.put_raw(*at, with);
            step.push(Change { at: *at, text: with.to_owned(), put: true, before });
        }
        self.redo.clear();
        self.changes += step.len() as isize;
        self.undo.push(step);
        self.cursor = Pos { line: self.cursor.line.min(self.lines.len() - 1), col: 0 };
        self.after_jump();
        hits.len()
    }
}

/// `s` with its case set aside, a character at a time, as `folded_hits` compares.
fn fold(s: &str) -> String {
    s.chars().flat_map(char::to_lowercase).collect()
}

/// Where `ql` (folded) is in `line`, case aside: the start and end of each hit in `line`'s own
/// bytes. Folding can change a character's length (ẞ, the Kelvin sign, İ), so places in the
/// folded line are taken back to the characters they came from; a hit starting or ending
/// inside one character's folding is none.
fn folded_hits(line: &str, ql: &str) -> Vec<(usize, usize)> {
    let mut folded = String::with_capacity(line.len());
    // Where each character starts in the folded line, and in `line`.
    let mut at = Vec::with_capacity(line.len() + 1);
    for (i, c) in line.char_indices() {
        at.push((folded.len(), i));
        folded.extend(c.to_lowercase());
    }
    at.push((folded.len(), line.len()));
    let back = |o: usize| at.binary_search_by_key(&o, |&(f, _)| f).ok().map(|k| at[k].1);
    folded.match_indices(ql).filter_map(|(s, _)| Some((back(s)?, back(s + ql.len())?))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-edit-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_file_is_saved_as_it_was_read() {
        let d = dir("round");
        for (name, bytes, enc, crlf) in [
            ("plain.txt", "smørrebrød\nline two\n".as_bytes().to_vec(), Encoding::Utf8, false),
            ("bom.txt", b"\xef\xbb\xbfhi\r\nthere\r\n".to_vec(), Encoding::Utf8Bom, true),
            ("old.txt", b"\x93Caf\xe9\x94 \x81\n".to_vec(), Encoding::Windows1252, false),
            ("wide.txt", [0xff, 0xfe].into_iter().chain("a\r\nb".encode_utf16().flat_map(u16::to_le_bytes)).collect(), Encoding::Utf16Le, true),
            ("big.txt", [0xfe, 0xff].into_iter().chain("é".encode_utf16().flat_map(u16::to_be_bytes)).collect(), Encoding::Utf16Be, false),
        ] {
            let p = d.join(name);
            std::fs::write(&p, &bytes).unwrap();
            let doc = open(&p).unwrap();
            assert_eq!((doc.encoding, doc.crlf), (enc, crlf), "{name}");
            assert!(!doc.text.contains('\r'), "{name}");
            save(&p, &doc.text, doc.encoding, doc.crlf, Some(&doc.stamp)).unwrap();
            assert_eq!(std::fs::read(&p).unwrap(), bytes, "{name} unchanged");
        }
        // A stray byte in otherwise UTF-8 text makes it Windows-1252, so it is kept as it is.
        std::fs::write(d.join("stray.txt"), b"caf\xc3\xa9 \xff").unwrap();
        assert_eq!(open(&d.join("stray.txt")).unwrap().encoding, Encoding::Windows1252);
        assert_eq!(encode("€ ✓", Encoding::Windows1252, false), Err('✓'));
        // Binary and too big are refused.
        std::fs::write(d.join("a.bin"), b"MZ\0\x01").unwrap();
        assert!(open(&d.join("a.bin")).is_err());
        assert!(open(&d).is_err(), "a folder");
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn a_file_changed_since_is_not_saved_over_and_keeps_its_permissions() {
        let d = dir("changed");
        let p = d.join("notes.md");
        std::fs::write(&p, "one\n").unwrap();
        let doc = open(&p).unwrap();
        std::fs::write(&p, "one\ntwo, written elsewhere\n").unwrap();
        assert_eq!(save(&p, "mine\n", doc.encoding, doc.crlf, Some(&doc.stamp)), Err(SaveError::Changed));
        let stamp = save(&p, "mine\n", doc.encoding, doc.crlf, None).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), "mine\n");
        assert!(!changed(&p, &stamp));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let s = d.join("run.sh");
            std::fs::write(&s, "#!/bin/sh\n").unwrap();
            std::fs::set_permissions(&s, std::fs::Permissions::from_mode(0o750)).unwrap();
            // Through a link, to the file it names; a hard link keeps seeing it.
            std::os::unix::fs::symlink(&s, d.join("link.sh")).unwrap();
            std::fs::hard_link(&s, d.join("hard.sh")).unwrap();
            save(&d.join("link.sh"), "#!/bin/sh\necho hi\n", Encoding::Utf8, false, None).unwrap();
            assert!(std::fs::symlink_metadata(d.join("link.sh")).unwrap().file_type().is_symlink());
            assert_eq!(std::fs::metadata(&s).unwrap().permissions().mode() & 0o777, 0o750);
            assert_eq!(std::fs::read_to_string(d.join("hard.sh")).unwrap(), "#!/bin/sh\necho hi\n");
            std::fs::remove_file(d.join("hard.sh")).unwrap();
            save(&s, "#!/bin/sh\n", Encoding::Utf8, false, None).unwrap();
            assert_eq!(std::fs::metadata(&s).unwrap().permissions().mode() & 0o777, 0o750, "renamed over it");
        }
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn typing_moving_and_undo() {
        let mut b = Buffer::new("fn main() {\n    let x = 1;\n}");
        assert_eq!(b.indent, "    ");
        b.vertical(1, false);
        b.end(false);
        b.newline();
        assert_eq!(b.cursor, Pos { line: 2, col: 4 }, "indented as the line above");
        for c in "let y".chars() {
            b.insert(&c.to_string());
        }
        assert_eq!(b.lines[2], "    let y");
        assert!(b.dirty());
        // "y" is one step, "let" another, the space and the new line others.
        b.undo();
        assert_eq!(b.lines[2], "    let ");
        b.undo();
        b.undo();
        assert_eq!(b.lines[2], "    ");
        b.undo();
        assert_eq!(b.text(), "fn main() {\n    let x = 1;\n}");
        assert!(!b.dirty());
        b.redo();
        b.redo();
        assert_eq!(b.lines[2], "    let");
        // Backspace joins lines; a selection is replaced by what is typed.
        let mut b = Buffer::new("ab\ncd");
        b.vertical(1, false);
        b.backspace();
        assert_eq!(b.text(), "abcd");
        b.home(false);
        b.right(true);
        b.right(true);
        assert_eq!(b.selected().as_deref(), Some("ab"));
        b.insert("X");
        assert_eq!(b.text(), "Xcd");
        b.undo();
        assert_eq!(b.text(), "abcd", "the replacement is one step");
        // Multi-byte characters and moving up and down keep the column.
        let mut b = Buffer::new("ææææ\nx\nøøøø");
        b.end(false);
        b.vertical(1, false);
        assert_eq!(b.cursor, Pos { line: 1, col: 1 });
        b.vertical(1, false);
        assert_eq!(b.cursor, Pos { line: 2, col: 8 });
        b.left(false);
        b.delete();
        assert_eq!(b.lines[2], "øøø");
        b.select_all();
        assert_eq!(b.cut().as_deref(), Some("ææææ\nx\nøøø"));
        assert_eq!(b.text(), "");
        b.insert("one\ntwo");
        assert_eq!(b.cursor, Pos { line: 1, col: 3 });
    }

    #[test]
    fn case_aside_finds_and_replaces_whole_characters() {
        // ẞ folds to ß (3 bytes to 2), the Kelvin sign to k (3 to 1), İ to i and a dot (2 to 3).
        let mut b = Buffer::new("Maße STRAẞE\n\u{212A}=273 kelvin\nİstanbul istanbul");
        assert_eq!(b.replace_all("ẞ", "ss"), 2);
        assert_eq!(b.lines[0], "Masse STRAssE");
        assert_eq!(b.replace_all("k", "K"), 2);
        assert_eq!(b.lines[1], "K=273 Kelvin");
        // An i is not half of an İ.
        assert_eq!(b.replace_all("ist", "IST"), 1);
        assert_eq!(b.lines[2], "İstanbul ISTanbul");
        assert!(b.find("İSTANBUL", false));
        assert_eq!(b.selection(), Some((Pos { line: 2, col: 0 }, Pos { line: 2, col: "İstanbul".len() })));
        b.undo();
        b.undo();
        b.undo();
        assert_eq!(b.text(), "Maße STRAẞE\n\u{212A}=273 kelvin\nİstanbul istanbul");
        assert!(b.find("\u{212A}ELVIN", false));
        assert_eq!(b.selection(), Some((Pos { line: 1, col: 8 }, Pos { line: 1, col: 14 })));
    }

    #[test]
    fn a_failed_save_leaves_the_file_and_what_it_carries() {
        let d = dir("carries");
        let p = d.join("notes.txt");
        std::fs::write(&p, "as it was\n").unwrap();
        #[cfg(unix)]
        {
            // Something in the way of the file beside it that is no lack of room: said, not
            // written in place. (Windows calls a folder in the way "access denied".)
            let tmp = d.join(format!(".notes.txt.coxswain-{}.tmp", std::process::id()));
            std::fs::create_dir(&tmp).unwrap();
            assert!(matches!(save(&p, "new\n", Encoding::Utf8, false, None), Err(SaveError::Io(_))));
            assert_eq!(std::fs::read_to_string(&p).unwrap(), "as it was\n");
            std::fs::remove_dir(&tmp).unwrap();
            use std::os::unix::fs::{MetadataExt, PermissionsExt};
            let ino = |p: &Path| std::fs::metadata(p).unwrap().ino();
            // A private file stays private, its group kept; renamed over.
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            let (was, gid) = (ino(&p), std::fs::metadata(&p).unwrap().gid());
            save(&p, "new\n", Encoding::Utf8, false, None).unwrap();
            let m = std::fs::metadata(&p).unwrap();
            assert_eq!((m.mode() & 0o777, m.gid()), (0o600, gid));
            assert_ne!(m.ino(), was, "renamed over");
            // SAFETY: geteuid has no preconditions.
            if unsafe { libc::geteuid() } != 0 {
                // Read-only: refused, not got round by a rename.
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o400)).unwrap();
                assert!(save(&p, "newer\n", Encoding::Utf8, false, None).is_err());
                assert_eq!(std::fs::read_to_string(&p).unwrap(), "new\n");
                std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            // Extended attributes, where the file system takes them: written in place, kept.
            let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes()).unwrap();
            #[cfg(any(target_os = "linux", target_os = "android"))]
            // SAFETY: a path and a value of the length given.
            let set = unsafe { libc::setxattr(c.as_ptr(), c"user.coxswain-test".as_ptr(), b"x".as_ptr().cast(), 1, 0) } == 0;
            #[cfg(not(any(target_os = "linux", target_os = "android")))]
            let set = { let _ = c; false };
            if set {
                let was = ino(&p);
                save(&p, "with attributes\n", Encoding::Utf8, false, None).unwrap();
                assert_eq!(ino(&p), was, "in place");
                assert_eq!(std::fs::read_to_string(&p).unwrap(), "with attributes\n");
                save(&p, "short\n", Encoding::Utf8, false, None).unwrap();
                assert_eq!(std::fs::read_to_string(&p).unwrap(), "short\n", "cut to length");
            }
        }
        assert!(std::fs::read_dir(&d).unwrap().all(|e| !e.unwrap().file_name().to_string_lossy().ends_with(".tmp")));
        std::fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn find_goes_around_and_replace_all_is_one_step() {
        let mut b = Buffer::new("alpha\nBeta alpha\ngamma");
        assert!(b.find("ALPHA", false));
        assert_eq!(b.selection(), Some((Pos { line: 0, col: 0 }, Pos { line: 0, col: 5 })));
        assert!(b.find("alpha", false));
        assert_eq!(b.cursor, Pos { line: 1, col: 10 });
        assert!(b.find("alpha", false), "around the end");
        assert_eq!(b.cursor, Pos { line: 0, col: 5 });
        assert!(b.find("alpha", true), "back, around the start");
        assert_eq!(b.cursor, Pos { line: 1, col: 10 });
        assert!(!b.find("delta", false));
        assert_eq!(b.replace_all("alpha", "omega"), 2);
        assert_eq!(b.text(), "omega\nBeta omega\ngamma");
        b.undo();
        assert_eq!(b.text(), "alpha\nBeta alpha\ngamma");
        assert!(!b.dirty());
        // Ctrl+arrows go by words.
        let mut b = Buffer::new("let fuel_left = 42;");
        b.word(true, false);
        assert_eq!(b.cursor.col, 3);
        b.word(true, false);
        assert_eq!(b.cursor.col, 13);
        b.word(false, false);
        assert_eq!(b.cursor.col, 4);
        let tabs = Buffer::new("a\n\tb\n\tc");
        assert_eq!(tabs.indent, "\t");
    }
}
