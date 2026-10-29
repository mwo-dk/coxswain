//! A folder held open by its handle, for the changes that make, move and remove entries
//! (`compare::sync`), on Windows: every name is opened in its held folder (`NtCreateFile`
//! relative to the folder's handle), never through a link or junction (a reparse point is opened
//! itself), and every entry is checked through its own handle to be exactly what a plan saw
//! (`Id`) before it is moved or removed; a move or removal is made through that very handle, so
//! nothing can be swapped in between. Only on a local NTFS or ReFS disk: a network share's server,
//! or a file system in user mode, opens by path and could follow a junction swapped in there.
//! The same calls as `dir.rs` on Unix.

use super::imp::{DELETE, GENERIC_READ, GENERIC_WRITE, LIST_DIRECTORY, READ_ATTRIBUTES, SHARE_ALL, SYNCHRONIZE, WRITE_ATTRIBUTES, basic, create_in, local_disk, nt, open_in};
use super::{Id, Kind, Stat, changed, other, stat_of};
use crate::t;
use std::ffi::{OsStr, OsString};
use std::io::{Read, Write};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::{FILE_CREATE, FILE_DIRECTORY_FILE, FILE_NON_DIRECTORY_FILE, FILE_OPEN, FILE_SYNCHRONOUS_IO_NONALERT, FileRenameInformation, NtCreateFile, NtSetInformationFile};
use windows_sys::Win32::Foundation::{HANDLE, UNICODE_STRING};
use windows_sys::Win32::Storage::FileSystem::{FILE_BASIC_INFO, FILE_CASE_SENSITIVE_INFO, FILE_DISPOSITION_INFO, FILE_ID_BOTH_DIR_INFO, FILE_RENAME_INFO, FILE_STREAM_INFO, FileBasicInfo, FileCaseSensitiveInfo, FileDispositionInfo, FileIdBothDirectoryInfo, FileIdBothDirectoryRestartInfo, FileStreamInfo, GetFileInformationByHandleEx, GetFinalPathNameByHandleW, SetFileInformationByHandle};
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

/// FILE_TRAVERSE: names in the folder can be opened through it.
const TRAVERSE: u32 = 0x20;
/// FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT: a folder opens, a link is opened
/// itself.
const FLAGS: u32 = 0x0200_0000 | 0x0020_0000;
/// STATUS_OBJECT_NAME_COLLISION: the name is taken.
const COLLISION: i32 = 0xC000_0035_u32 as i32;
/// Read-only and hidden: the attributes `Stat::mode` keeps on Windows.
const READ_ONLY: u32 = 0x1;
const HIDDEN: u32 = 0x2;
const NORMAL: u32 = 0x80;
/// Encrypted by EFS: a copy is made encrypted too, or not at all.
const ENCRYPTED: u32 = 0x4000;

/// A folder held open.
pub struct Dir {
    h: std::fs::File,
}

fn last() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// One name in a folder: never a path, a stream, `.` or `..`.
fn wide(name: &OsStr) -> std::io::Result<Vec<u16>> {
    let w: Vec<u16> = name.encode_wide().collect();
    let plain = !w.is_empty() && w != [b'.' as u16] && w != [b'.' as u16, b'.' as u16] && !w.iter().any(|&c| c == b'\\' as u16 || c == b'/' as u16 || c == b':' as u16 || c == 0);
    if !plain || w.len() > 32_000 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    Ok(w)
}

fn raw(f: &std::fs::File) -> HANDLE {
    f.as_raw_handle() as HANDLE
}

/// A name of its own beside `name`, for a file being written (process, time, a count).
fn temp_name(name: &OsStr) -> Vec<u16> {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stem: String = name.to_string_lossy().chars().filter(|c| !matches!(c, '\\' | '/' | ':' | '\0')).take(40).collect();
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.subsec_nanos()).unwrap_or(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    format!(".{stem}.coxswain-{}-{nanos}-{n}.tmp", crate::spill::run()).encode_utf16().collect()
}

/// A time as Windows keeps it: 100 ns steps since 1601.
fn filetime((s, n): (i64, u32)) -> i64 {
    (s + 11_644_473_600).saturating_mul(10_000_000).saturating_add(i64::from(n / 100))
}

/// The open entry `f` given `attributes` (0: as they are) and, where not 0, these times.
fn set_basic(f: &std::fs::File, attributes: u32, accessed: i64, modified: i64) -> std::io::Result<()> {
    let info = FILE_BASIC_INFO { CreationTime: 0, LastAccessTime: accessed, LastWriteTime: modified, ChangeTime: 0, FileAttributes: attributes };
    // SAFETY: an open handle with write-attributes access, `info` of the size given.
    if unsafe { SetFileInformationByHandle(raw(f), FileBasicInfo, std::ptr::from_ref(&info).cast(), std::mem::size_of::<FILE_BASIC_INFO>() as u32) } == 0 {
        return Err(last());
    }
    Ok(())
}

/// The open entry `f` renamed to `name` in the folder `to`, never over what is there, through
/// its own handle (opened with DELETE): the very entry, whatever happened to its name meanwhile.
fn rename(f: &std::fs::File, to: HANDLE, name: &[u16]) -> std::io::Result<()> {
    let head = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let size = (head + name.len() * 2).max(std::mem::size_of::<FILE_RENAME_INFO>());
    let mut buf = vec![0u64; size.div_ceil(8) + 1];
    // SAFETY: the buffer holds the struct and the name (sized above), aligned for it; zeroes
    // leave ReplaceIfExists false.
    unsafe {
        let r = buf.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        (*r).RootDirectory = to;
        (*r).FileNameLength = (name.len() * 2) as u32;
        std::ptr::copy_nonoverlapping(name.as_ptr(), buf.as_mut_ptr().cast::<u8>().add(head).cast::<u16>(), name.len());
    }
    // SAFETY: zeroes are a valid IO_STATUS_BLOCK.
    let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: an open handle, the rename record of the size given.
    let st = unsafe { NtSetInformationFile(raw(f), &mut io, buf.as_ptr().cast(), size as u32, FileRenameInformation) };
    match st {
        0.. => Ok(()),
        COLLISION => Err(other(t!("sync.taken"))),
        _ => Err(nt(st)),
    }
}

/// The open entry `f` deleted through its own handle: its name goes at once where the file
/// system can (POSIX semantics, read-only or not), else when the last handle closes.
fn delete(f: &std::fs::File) -> std::io::Result<()> {
    // FILE_DISPOSITION_INFORMATION_EX: DELETE | POSIX_SEMANTICS | IGNORE_READONLY_ATTRIBUTE.
    let flags: u32 = 0x1 | 0x2 | 0x10;
    #[cfg(test)]
    if OLD_DELETE.get() {
        return plain_delete(f);
    }
    // SAFETY: zeroes are a valid IO_STATUS_BLOCK.
    let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    // SAFETY: an open handle (with DELETE), four bytes of flags; class 64 is
    // FileDispositionInformationEx.
    let st = unsafe { NtSetInformationFile(raw(f), &mut io, std::ptr::from_ref(&flags).cast(), 4, 64) };
    if st >= 0 {
        return Ok(());
    }
    // STATUS_INVALID_PARAMETER, STATUS_INVALID_INFO_CLASS, STATUS_NOT_SUPPORTED: an older
    // Windows or a file system without it; anything else (a folder not empty) is said.
    if !matches!(st as u32, 0xC000_000D | 0xC000_0003 | 0xC000_00BB) {
        return Err(nt(st));
    }
    plain_delete(f)
}

#[cfg(test)]
thread_local! {
    /// A test's: delete as an older Windows does, without POSIX semantics.
    pub(crate) static OLD_DELETE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The plain delete of an older Windows, which refuses a read-only file: read-only is taken off
/// first, through the same handle (opened with write-attributes), and put back if the delete is
/// still refused.
fn plain_delete(f: &std::fs::File) -> std::io::Result<()> {
    let was = basic(f).map(|b| b.FileAttributes).unwrap_or(0);
    let read_only = was & READ_ONLY != 0;
    if read_only {
        set_basic(f, match was & !READ_ONLY { 0 => NORMAL, a => a }, 0, 0)?;
    }
    let gone = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: an open handle (with DELETE), the plain record of the size given.
    if unsafe { SetFileInformationByHandle(raw(f), FileDispositionInfo, std::ptr::from_ref(&gone).cast(), std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32) } == 0 {
        let e = last();
        if read_only {
            let _ = set_basic(f, was, 0, 0);
        }
        return Err(e);
    }
    Ok(())
}

/// The data stream `name` (`:name:$DATA`) of the open file `f`, opened through the file's own
/// handle, never by a path: to read it (`create` false) or as a new one, to be written and, if
/// it cannot be whole, deleted through its handle.
fn stream(f: &std::fs::File, name: &[u16], create: bool) -> std::io::Result<std::fs::File> {
    if name.first() != Some(&(b':' as u16)) || name.iter().any(|&c| c == b'\\' as u16 || c == b'/' as u16 || c == 0) || name.len() > 600 {
        return Err(std::io::Error::from(std::io::ErrorKind::InvalidInput));
    }
    let us = UNICODE_STRING { Length: (name.len() * 2) as u16, MaximumLength: (name.len() * 2) as u16, Buffer: name.as_ptr() as *mut u16 };
    let oa = OBJECT_ATTRIBUTES { Length: std::mem::size_of::<OBJECT_ATTRIBUTES>() as u32, RootDirectory: raw(f), ObjectName: &us, Attributes: 0x40, SecurityDescriptor: std::ptr::null(), SecurityQualityOfService: std::ptr::null() };
    let mut h: HANDLE = std::ptr::null_mut();
    // SAFETY: zeroes are a valid IO_STATUS_BLOCK, which the call fills in.
    let mut io: IO_STATUS_BLOCK = unsafe { std::mem::zeroed() };
    let (access, disposition, share) = if create { (GENERIC_WRITE | DELETE, FILE_CREATE, 0) } else { (GENERIC_READ, FILE_OPEN, SHARE_ALL) };
    // SAFETY: an open file handle, a stream name that outlives the call, `h` and `io` ours to
    // fill; synchronous, so the handle works as a std File.
    let st = unsafe { NtCreateFile(&mut h, access | SYNCHRONIZE, &oa, &mut io, std::ptr::null(), 0x80, share, disposition, FILE_NON_DIRECTORY_FILE | FILE_SYNCHRONOUS_IO_NONALERT, std::ptr::null(), 0) };
    if st < 0 {
        return Err(nt(st));
    }
    // SAFETY: just opened, owned here.
    Ok(std::fs::File::from(unsafe { std::os::windows::io::OwnedHandle::from_raw_handle(h as _) }))
}

/// The data streams of the open file `f` besides its own (`:name:$DATA`), with their sizes:
/// the mark of a download (`:Zone.Identifier`, the warning Windows shows before a program from
/// the internet runs) and any other. None on a file system without them (FAT).
fn streams(f: &std::fs::File) -> std::io::Result<Vec<(Vec<u16>, u64)>> {
    let mut buf = vec![0u64; 8 * 1024];
    loop {
        // SAFETY: an open file, a buffer of the length given.
        if unsafe { GetFileInformationByHandleEx(raw(f), FileStreamInfo, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32) } != 0 {
            break;
        }
        let e = last();
        match e.raw_os_error() {
            // ERROR_HANDLE_EOF: none at all; and a file system without streams.
            Some(38 | 1 | 50 | 87) => return Ok(vec![]),
            // ERROR_MORE_DATA: a larger buffer, up to 4 MB.
            Some(234) if buf.len() < (512 << 10) => buf.resize(buf.len() * 4, 0),
            _ => return Err(e),
        }
    }
    let (base, end) = (buf.as_ptr().cast::<u8>(), buf.len() * 8);
    let main: Vec<u16> = "::$DATA".encode_utf16().collect();
    let (mut out, mut at) = (vec![], 0usize);
    loop {
        if at + std::mem::size_of::<FILE_STREAM_INFO>() > end {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        // SAFETY: a record the call wrote, inside the buffer (checked above); unaligned reads.
        let e = unsafe { base.add(at).cast::<FILE_STREAM_INFO>() };
        // SAFETY: as above.
        let (next, len, size) = unsafe { (std::ptr::addr_of!((*e).NextEntryOffset).read_unaligned(), std::ptr::addr_of!((*e).StreamNameLength).read_unaligned(), std::ptr::addr_of!((*e).StreamSize).read_unaligned()) };
        let name_at = at + std::mem::offset_of!(FILE_STREAM_INFO, StreamName);
        if name_at + len as usize > end {
            return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
        }
        // SAFETY: the name's bytes, inside the buffer (checked above), copied out.
        let name: Vec<u16> = unsafe { std::slice::from_raw_parts(base.add(name_at), len as usize) }.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
        if name != main {
            out.push((name, u64::try_from(size).unwrap_or(0)));
        }
        if next == 0 {
            return Ok(out);
        }
        at += next as usize;
    }
}

/// The stream `name` of `from` (`size` bytes as listed) written into a new stream of `to`, a
/// megabyte at a time; one cut short (stopped, or changed meanwhile) is deleted again.
fn copy_stream(from: &std::fs::File, to: &std::fs::File, name: &[u16], size: u64, stop: &AtomicBool) -> std::io::Result<()> {
    let mut src = stream(from, name, false)?;
    let mut dst = stream(to, name, true)?;
    let done = (|| {
        let mut buf = vec![0u8; (1 << 20).min(size as usize + 1)];
        let mut left = size + 1;
        while left > 0 {
            if stop.load(Ordering::Relaxed) {
                return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
            }
            let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
            let k = read_full(&mut src, &mut buf[..want])?;
            if k == 0 {
                break;
            }
            dst.write_all(&buf[..k])?;
            left -= k as u64;
        }
        if left != 1 {
            return Err(changed());
        }
        Ok(())
    })();
    if done.is_err() {
        let _ = delete(&dst);
    }
    done
}

/// Where the open `f` is now, as the system names it (its NT path: a volume needs no letter).
fn final_path(f: &std::fs::File) -> Option<PathBuf> {
    let mut buf = vec![0u16; 1024];
    loop {
        // SAFETY: an open handle and a buffer of the length given.
        // VOLUME_NAME_NT.
        let n = unsafe { GetFinalPathNameByHandleW(raw(f), buf.as_mut_ptr(), buf.len() as u32, 2) } as usize;
        if n == 0 {
            return None;
        }
        if n < buf.len() {
            return Some(PathBuf::from(OsString::from_wide(&buf[..n])));
        }
        if buf.len() > 70_000 {
            return None;
        }
        buf.resize(n + 1, 0);
    }
}

impl Dir {
    /// The folder at `path`, as itself: its real path (a link on the way resolved once, here),
    /// opened as a folder without following a link at its end, on a local disk only.
    pub fn root(path: &Path) -> std::io::Result<Dir> {
        let real = std::fs::canonicalize(path)?;
        let h = std::fs::OpenOptions::new().access_mode(LIST_DIRECTORY | TRAVERSE | READ_ATTRIBUTES | SYNCHRONIZE).share_mode(SHARE_ALL).custom_flags(FLAGS).open(real)?;
        let d = Dir { h };
        match d.stat()?.kind {
            Kind::Dir => {}
            Kind::Link => return Err(other(t!("pinned.link"))),
            _ => return Err(std::io::Error::from(std::io::ErrorKind::NotADirectory)),
        }
        if !local_disk(raw(&d.h)) {
            return Err(other(t!("pinned.remote")));
        }
        Ok(d)
    }

    /// The same folder, a handle of its own.
    pub fn try_clone(&self) -> std::io::Result<Dir> {
        Ok(Dir { h: self.h.try_clone()? })
    }

    /// Whether this folder is no longer in the folder `other`, nor anywhere under it (it was
    /// moved elsewhere), by where the system says each handle is now.
    pub fn parent_is_not(&self, other: &Dir) -> bool {
        match (final_path(&self.h), final_path(&other.h)) {
            (Some(me), Some(o)) => !me.to_string_lossy().to_lowercase().starts_with(&format!("{}\\", o.to_string_lossy().to_lowercase().trim_end_matches('\\'))),
            _ => false,
        }
    }

    /// What this folder is now.
    pub fn stat(&self) -> std::io::Result<Stat> {
        stat_of(&self.h)
    }

    /// Whether names here differ by case (the folder's case-sensitive flag, `fsutil file
    /// setCaseSensitiveInfo`, as WSL sets it). Unknown: not.
    pub fn case_sensitive(&self) -> bool {
        let mut i = FILE_CASE_SENSITIVE_INFO { Flags: 0 };
        // SAFETY: an open folder, `i` of the size given.
        let ok = unsafe { GetFileInformationByHandleEx(raw(&self.h), FileCaseSensitiveInfo, std::ptr::from_mut(&mut i).cast(), std::mem::size_of::<FILE_CASE_SENSITIVE_INFO>() as u32) } != 0;
        ok && i.Flags & 1 != 0
    }

    /// The entry `name` here opened with `access`, never through a link: the reparse point itself.
    fn open(&self, name: &OsStr, access: u32, disposition: u32, options: u32, share: u32) -> std::io::Result<std::fs::File> {
        open_in(raw(&self.h), &wide(name)?, access, disposition, options, share)
    }

    /// The entry `name` here, as it is now, never through a link.
    pub fn stat_at(&self, name: &OsStr) -> std::io::Result<Stat> {
        stat_of(&self.open(name, READ_ATTRIBUTES, FILE_OPEN, 0, SHARE_ALL)?)
    }

    /// The folder `name` here, opened here without following a link, and checked through its
    /// handle to be `want` exactly (same folder, same change time); with `same_file`, the same
    /// folder whatever its change time (undo, after the run changed it).
    pub fn sub(&self, name: &OsStr, want: Id, same_file: bool) -> std::io::Result<Dir> {
        let h = self.open(name, LIST_DIRECTORY | TRAVERSE | READ_ATTRIBUTES, FILE_OPEN, FILE_DIRECTORY_FILE, SHARE_ALL)?;
        let s = stat_of(&h)?;
        if s.kind != Kind::Dir || if same_file { !s.id.same_file(&want) } else { s.id != want } {
            return Err(changed());
        }
        Ok(Dir { h })
    }

    /// The names here, `.` and `..` left out, in order.
    pub fn names(&self) -> std::io::Result<Vec<OsString>> {
        let mut names = vec![];
        // 64 KiB, aligned for the records the call writes.
        let mut buf = vec![0u64; 8 * 1024];
        // From the start, whoever read this handle before.
        let mut class = FileIdBothDirectoryRestartInfo;
        loop {
            // SAFETY: an open folder, a buffer of the length given.
            if unsafe { GetFileInformationByHandleEx(raw(&self.h), class, buf.as_mut_ptr().cast(), (buf.len() * 8) as u32) } == 0 {
                let e = last();
                // ERROR_NO_MORE_FILES: the end. Anything else cuts the listing short: said.
                if e.raw_os_error() == Some(18) {
                    break;
                }
                return Err(e);
            }
            class = FileIdBothDirectoryInfo;
            let base = buf.as_ptr().cast::<u8>();
            let mut at = 0usize;
            loop {
                if at + std::mem::size_of::<FILE_ID_BOTH_DIR_INFO>() > buf.len() * 8 {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                // SAFETY: a record the call wrote at `at`, inside the buffer (checked above).
                let e = unsafe { base.add(at).cast::<FILE_ID_BOTH_DIR_INFO>() };
                // SAFETY: as above; unaligned reads, whatever the offsets.
                let (next, len) = unsafe { (std::ptr::addr_of!((*e).NextEntryOffset).read_unaligned(), std::ptr::addr_of!((*e).FileNameLength).read_unaligned()) };
                let name_at = at + std::mem::offset_of!(FILE_ID_BOTH_DIR_INFO, FileName);
                if name_at + len as usize > buf.len() * 8 {
                    return Err(std::io::Error::from(std::io::ErrorKind::InvalidData));
                }
                // SAFETY: the name's bytes, inside the buffer (checked above), copied out.
                let name: Vec<u16> = unsafe { std::slice::from_raw_parts(base.add(name_at), len as usize) }.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
                if name != [b'.' as u16] && name != [b'.' as u16, b'.' as u16] {
                    names.push(OsString::from_wide(&name));
                }
                if next == 0 {
                    break;
                }
                at += next as usize;
            }
        }
        names.sort();
        Ok(names)
    }

    /// The file `name` here opened to read, checked to be `want` exactly and a file: a link or
    /// junction is never opened through.
    pub fn open_file(&self, name: &OsStr, want: Id) -> std::io::Result<(std::fs::File, Stat)> {
        let f = self
            .open(name, GENERIC_READ | READ_ATTRIBUTES, FILE_OPEN, FILE_NON_DIRECTORY_FILE, SHARE_ALL)
            .map_err(|e| if e.kind() == std::io::ErrorKind::PermissionDenied { other(t!("pinned.no_open")) } else { e })?;
        let s = stat_of(&f)?;
        match s.kind {
            Kind::Link => return Err(other(t!("pinned.link"))),
            Kind::Other => return Err(other(t!("pinned.special"))),
            _ => {}
        }
        if s.kind != Kind::File || s.id != want {
            return Err(changed());
        }
        Ok((f, s))
    }

    /// Links and junctions are not copied on Windows (making one needs a right of its own):
    /// left as they are.
    pub fn read_link(&self, _name: &OsStr, _want: Id) -> std::io::Result<OsString> {
        Err(other(t!("pinned.link")))
    }

    /// Two files here and there the same, by their bytes (read through their handles, a
    /// megabyte at a time).
    pub fn same_bytes(&self, name: &OsStr, a: Id, there: &Dir, b: Id, stop: &AtomicBool) -> std::io::Result<bool> {
        let (mut fa, _) = self.open_file(name, a)?;
        let (mut fb, _) = there.open_file(name, b)?;
        let (mut x, mut y) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
        loop {
            if stop.load(Ordering::Relaxed) {
                return Ok(false);
            }
            let (n, m) = (read_full(&mut fa, &mut x)?, read_full(&mut fb, &mut y)?);
            if n != m || x[..n] != y[..m] {
                return Ok(false);
            }
            if n == 0 {
                return Ok(true);
            }
        }
    }

    /// A new file `name` here, with the bytes of `from` (as `was`): written beside under a name
    /// of its own (made new, shared with no one, never through a link), given `was`'s times and
    /// hidden attribute, then renamed to `name` through its own handle, only while nothing is
    /// there, then given its other data streams (the mark of a download among them) and made
    /// read-only after when `was` is. Encrypted (EFS), it is made encrypted or not at all.
    /// Stopped half way, nothing is left. What it is now, and whether it got all `was` carries
    /// (false: in place without a stream or read-only).
    pub fn create_file(&self, name: &OsStr, from: &mut std::fs::File, was: &Stat, stop: &AtomicBool) -> std::io::Result<(Stat, bool)> {
        let n = wide(name)?;
        // An encrypted file (EFS) is made encrypted, for this user, before a byte goes in; where
        // that cannot be (FAT, ReFS, EFS turned off), it is not copied: never as plain data.
        let encrypted = basic(from)?.FileAttributes & ENCRYPTED != 0;
        let access = GENERIC_WRITE | READ_ATTRIBUTES | WRITE_ATTRIBUTES | DELETE;
        let made_new = create_in(raw(&self.h), &temp_name(name), access, FILE_CREATE, FILE_NON_DIRECTORY_FILE, 0, if encrypted { ENCRYPTED } else { NORMAL });
        let mut f = match made_new {
            Err(_) if encrypted => return Err(other(t!("pinned.efs"))),
            r => r?,
        };
        if encrypted && !basic(&f).is_ok_and(|b| b.FileAttributes & ENCRYPTED != 0) {
            let _ = delete(&f);
            return Err(other(t!("pinned.efs")));
        }
        let done = (|| {
            let mut buf = vec![0u8; 1 << 20];
            // At most the size it had, and one byte more to tell it grew: a file still being
            // written is not copied without end, nor torn.
            let mut left = was.size + 1;
            while left > 0 {
                if stop.load(Ordering::Relaxed) {
                    return Err(std::io::Error::from(std::io::ErrorKind::Interrupted));
                }
                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let k = read_full(from, &mut buf[..want])?;
                if k == 0 {
                    break;
                }
                f.write_all(&buf[..k])?;
                left -= k as u64;
            }
            if stat_of(from)?.id != was.id || stat_of(&f)?.size != was.size {
                return Err(changed());
            }
            // Its other streams, as they are now; one that cannot be listed is said, not lost.
            let extra = streams(from).ok();
            set_basic(&f, if was.mode & HIDDEN != 0 { HIDDEN } else { NORMAL }, filetime(was.accessed), filetime(was.modified))?;
            rename(&f, raw(&self.h), &n)?;
            Ok(extra)
        })();
        let extra = match done {
            Ok(z) => z,
            Err(e) => {
                // Deleted through its own handle, never by name.
                let _ = delete(&f);
                return Err(e);
            }
        };
        let made = stat_of(&f)?;
        if extra.as_ref().is_some_and(Vec::is_empty) && was.mode & READ_ONLY == 0 {
            return Ok((made, true));
        }
        // Other streams (the mark of a download among them) and read-only come once it is in
        // place: a stream cannot be added while the file is shared with no one, nor a read-only
        // file renamed. Through a handle of its own, only to the very file put there, unchanged
        // (the same change time, write time and size); written, it stays even should this fail,
        // and that is said.
        drop(f);
        let Ok(g) = open_in(raw(&self.h), &n, READ_ATTRIBUTES | WRITE_ATTRIBUTES, FILE_OPEN, FILE_NON_DIRECTORY_FILE, SHARE_ALL) else { return Ok((made, false)) };
        if !stat_of(&g).is_ok_and(|s| s.id == made.id && (s.size, s.modified) == (made.size, made.modified)) {
            return Ok((made, false));
        }
        // Each stream tried, whatever became of the one before.
        let marked = extra.is_some_and(|all| all.iter().filter(|(name, size)| copy_stream(from, &g, name, *size, stop).is_err()).count() == 0);
        // The times again (a new stream moves them), and read-only.
        let attributes = match was.mode & (READ_ONLY | HIDDEN) {
            0 => NORMAL,
            a => a,
        };
        let set = set_basic(&g, attributes, filetime(was.accessed), filetime(was.modified)).is_ok();
        Ok((stat_of(&g).unwrap_or(made), marked && set))
    }

    /// A new, empty folder `name` here, held open: made here and nothing else (one that is there
    /// is refused, as the call makes it new and opens it at once). Its times come last
    /// (`finish`), once what goes into it is in.
    pub fn make_dir(&self, name: &OsStr) -> std::io::Result<Dir> {
        let h = self.open(name, LIST_DIRECTORY | TRAVERSE | READ_ATTRIBUTES | WRITE_ATTRIBUTES, FILE_CREATE, FILE_DIRECTORY_FILE, SHARE_ALL).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists { other(t!("sync.taken")) } else { e }
        })?;
        Ok(Dir { h })
    }

    /// This folder (one `make_dir` made) given `was`'s times and hidden attribute, now that it
    /// is filled.
    pub fn finish(&self, was: &Stat) -> std::io::Result<()> {
        set_basic(&self.h, if was.mode & HIDDEN != 0 { HIDDEN } else { 0 }, filetime(was.accessed), filetime(was.modified))
    }

    /// Links are not made on Windows: see `read_link`.
    pub fn make_link(&self, _name: &OsStr, _to: &OsStr) -> std::io::Result<Stat> {
        Err(other(t!("pinned.link")))
    }

    /// The entry `name` here moved into the folder `to` as `new` (the same disk, renamed, never
    /// copied), only while it is `want` exactly (any kind: it is opened itself, never what a link
    /// points at) and `new` is free there. The rename goes through the entry's own handle, the
    /// one checked.
    pub fn move_to(&self, name: &OsStr, want: Id, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        self.move_checked(name, want, false, to, new)
    }

    /// As `move_to`, for undo: the same entry, whatever its change time (moving it moved that).
    pub fn move_back(&self, name: &OsStr, want: Id, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        self.move_checked(name, want, true, to, new)
    }

    fn move_checked(&self, name: &OsStr, want: Id, same_file: bool, to: &Dir, new: &OsStr) -> std::io::Result<()> {
        let m = wide(new)?;
        let e = self.open(name, DELETE | READ_ATTRIBUTES, FILE_OPEN, 0, SHARE_ALL)?;
        let s = stat_of(&e)?;
        if if same_file { !s.id.same_file(&want) } else { s.id != want } {
            return Err(changed());
        }
        rename(&e, raw(&to.h), &m)
    }

    /// The entry `name` here removed for good (what a run made, for undo), only while it is the
    /// very one (`same_file`) with the size and time it was left with; a folder only when empty.
    /// Removed through the handle checked.
    pub fn remove(&self, name: &OsStr, want: &Stat) -> std::io::Result<()> {
        // Write-attributes too: an older Windows deletes a read-only file only once read-only
        // is taken off (`plain_delete`). Where the file's rights grant delete but not that, it
        // is opened without it: a current Windows deletes it all the same.
        let e = match self.open(name, DELETE | READ_ATTRIBUTES | WRITE_ATTRIBUTES, FILE_OPEN, 0, SHARE_ALL) {
            Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => self.open(name, DELETE | READ_ATTRIBUTES, FILE_OPEN, 0, SHARE_ALL)?,
            r => r?,
        };
        let s = stat_of(&e)?;
        let same = s.id.same_file(&want.id) && s.kind == want.kind && (s.kind == Kind::Dir || (s.size, s.modified) == (want.size, want.modified));
        if !same {
            return Err(changed());
        }
        delete(&e)
    }
}

fn read_full(f: &mut std::fs::File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match f.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}
