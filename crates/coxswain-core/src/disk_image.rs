//! Disk images: .dmg, .vhd, .vhdx, .vmdk, .qcow2, .vdi, .iso, .wim and raw .img files; and,
//! read the same way, CHM help files (.chm) and CBR comics (.cbr, a RAR).
//!
//! Their facts (format, disk size, size on disk, fixed or dynamic, the partition table, a
//! volume's label) are read here from their documented headers, in pure Rust, always. With
//! *Open disk images like folders* on (`[archive] disk_images`, off by default) they also open
//! like an archive, read-only: an installed 7-Zip lists them and copies single files or folders
//! out. Nothing is ever mounted, and nothing is ever written into an image. Search does not
//! read inside them.

use serde::Serialize;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

// ---------------------------------------------------------------- the option

/// `[archive] disk_images`, as the app runs with it: set when the config is read or saved.
static OPEN: AtomicBool = AtomicBool::new(false);

#[cfg(test)]
thread_local! {
    /// A test's own switch: the global one is set by other tests' configs meanwhile.
    pub(crate) static FORCE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Use `[archive] disk_images` from now on.
pub fn set_open(on: bool) {
    OPEN.store(on, Ordering::Relaxed);
}

/// Whether disk images open like folders.
pub fn open_on() -> bool {
    #[cfg(test)]
    if FORCE.with(|f| f.get()) {
        return true;
    }
    OPEN.load(Ordering::Relaxed)
}

// ---------------------------------------------------------------- what is one

/// Endings that are disk images by their name alone.
const CLEAR: &[&str] = &[
    ".dmg", ".vhdx", ".vmdk", ".qcow2", ".qcow", ".vdi", ".iso", ".wim", ".chm", ".cbr",
];
/// Endings shared with other files (`.vhd` is VHDL source too, `.img` anything): a disk image
/// only when its content says so.
const SHARED: &[&str] = &[".vhd", ".img"];

fn ends(path: &Path, list: &[&str]) -> bool {
    let Some(n) = path.file_name().map(|n| n.as_encoded_bytes()) else {
        return false;
    };
    list.iter()
        .any(|e| n.len() > e.len() && n[n.len() - e.len()..].eq_ignore_ascii_case(e.as_bytes()))
}

/// Every ending a disk image may have, for the desktop app (the shared ones are checked with
/// `is_disk_image` before they are called one).
pub fn endings() -> Vec<&'static str> {
    CLEAR.iter().chain(SHARED).copied().collect()
}

/// A disk image (or CHM, CBR) by its name alone: not `.vhd` or `.img`.
pub fn sure(path: &Path) -> bool {
    ends(path, CLEAR)
}

/// A name a disk image may have; no file is read.
pub fn named(path: &Path) -> bool {
    ends(path, CLEAR) || ends(path, SHARED)
}

/// `path` is a disk image: by its ending, and for `.vhd` and `.img` by its content (a VHD's
/// footer; a partition table, a file system or an ISO volume).
pub fn is_disk_image(path: &Path) -> bool {
    if ends(path, CLEAR) {
        return true;
    }
    ends(path, SHARED) && facts(path).is_some()
}

/// `path` is a disk image that opens like a folder now: the option is on.
pub fn opens(path: &Path) -> bool {
    open_on() && is_disk_image(path) && path.is_file()
}

// ---------------------------------------------------------------- facts

/// What a disk image says of itself.
#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Facts {
    /// `VHD`, `VHDX`, `VMDK`, `QCOW2`, `DMG`, `VDI`, `WIM`, `ISO 9660`, `UDF`, `raw`, or `chm` and
    /// `cbr` (no disk images, but read the same way).
    pub format: &'static str,
    /// The size of the disk it holds.
    pub virtual_size: Option<u64>,
    /// What the file takes on the disk (less than its length when it is sparse).
    pub on_disk: u64,
    /// `fixed`, `dynamic` or `differencing` (it holds only changes to a parent image).
    pub variant: Option<&'static str>,
    /// The volume's name: an ISO's, a FAT or ext file system's.
    pub label: Option<String>,
    /// The file system right at its start, when it has no partition table.
    pub file_system: Option<&'static str>,
    /// `MBR` or `GPT`, and its partitions.
    pub table: Option<&'static str>,
    pub partitions: Vec<Partition>,
}

#[derive(Clone, Debug, Default, Serialize, PartialEq)]
pub struct Partition {
    /// What the partition's type says: `EFI`, `NTFS/exFAT`, `Linux`, `FreeBSD ZFS`, … or its
    /// number or GUID when it is not one known here.
    pub kind: String,
    pub start: u64,
    pub size: u64,
    /// A GPT partition's own name.
    pub name: String,
}

/// The first bytes read: a partition table, the first file system's start, an ISO's volume
/// descriptors.
const HEAD: usize = 1 << 20;

fn read_at(f: &mut File, at: u64, len: usize) -> Vec<u8> {
    let mut v = Vec::with_capacity(len.min(HEAD));
    if f.seek(SeekFrom::Start(at)).is_ok() {
        let _ = f.take(len as u64).read_to_end(&mut v);
    }
    v
}

/// `n` bytes of `b` from `at`, if it has them: every read of a header goes through here, so a
/// short or crafted file never reads past its end.
fn part(b: &[u8], at: usize, n: usize) -> Option<&[u8]> {
    b.get(at..at.checked_add(n)?)
}
fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(part(b, at, 4)?.try_into().ok()?))
}
fn be64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(part(b, at, 8)?.try_into().ok()?))
}
fn le16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(part(b, at, 2)?.try_into().ok()?))
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(part(b, at, 4)?.try_into().ok()?))
}
fn le64(b: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(part(b, at, 8)?.try_into().ok()?))
}
fn has(b: &[u8], at: usize, what: &[u8]) -> bool {
    part(b, at, what.len()) == Some(what)
}

/// A GUID as written on disk (its first three parts little-endian), in its usual text.
fn guid(b: &[u8]) -> String {
    format!(
        "{:08X}-{:04X}-{:04X}-{}-{}",
        le32(b, 0).unwrap_or(0),
        le16(b, 4).unwrap_or(0),
        le16(b, 6).unwrap_or(0),
        hex(part(b, 8, 2).unwrap_or(&[])),
        hex(part(b, 10, 6).unwrap_or(&[]))
    )
}
fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect()
}

/// Text in a header: up to its first zero, without the spaces that pad it.
fn text(b: &[u8]) -> Option<String> {
    let b = b.split(|&c| c == 0).next().unwrap_or(b);
    let s = String::from_utf8_lossy(b).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// What the file takes on the disk.
fn on_disk(m: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        m.blocks().saturating_mul(512).min(m.len())
    }
    #[cfg(not(unix))]
    m.len()
}

/// The facts read last, by path, length and modified time: the panels ask at every cursor move
/// and redraw (`opens`, the hint, Properties), the file is read once.
type Known = (PathBuf, u64, Option<std::time::SystemTime>, Option<Facts>);
static KNOWN: std::sync::Mutex<Vec<Known>> = std::sync::Mutex::new(Vec::new());
const KNOWN_KEPT: usize = 64;

/// The facts of the disk image at `path`, from its headers. `None` when it is none of the
/// formats known here (VHDL source named `.vhd`, an `.img` that is a picture), or a file only
/// in the cloud (reading it would download it).
pub fn facts(path: &Path) -> Option<Facts> {
    if crate::cloud::keep_out(path) {
        return None;
    }
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let (len, when) = (meta.len(), meta.modified().ok());
    let mut known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(k) = known.iter().find(|k| k.0 == path && k.1 == len && k.2 == when) {
        return k.3.clone();
    }
    drop(known);
    let out = read_facts(path, &meta);
    known = KNOWN.lock().unwrap_or_else(|e| e.into_inner());
    known.retain(|k| k.0 != path);
    if known.len() >= KNOWN_KEPT {
        known.remove(0);
    }
    known.push((path.to_path_buf(), len, when, out.clone()));
    out
}

fn read_facts(path: &Path, meta: &std::fs::Metadata) -> Option<Facts> {
    let mut f = File::open(path).ok()?;
    let len = meta.len();
    let head = read_at(&mut f, 0, HEAD);
    let tail = if len >= 512 {
        read_at(&mut f, len - 512, 512)
    } else {
        vec![]
    };
    let mut out = read(&mut f, &head, &tail, len)?;
    out.on_disk = on_disk(meta);
    Some(out)
}

fn read(f: &mut File, head: &[u8], tail: &[u8], len: u64) -> Option<Facts> {
    let mut x = Facts::default();
    if has(head, 0, b"QFI\xfb") {
        x.format = "QCOW2";
        x.virtual_size = be64(head, 24);
        x.variant = Some(if be64(head, 8).unwrap_or(0) != 0 {
            "differencing"
        } else {
            "dynamic"
        });
    } else if has(head, 0, b"vhdxfile") {
        x.format = "VHDX";
        vhdx(f, &mut x);
    } else if has(head, 0, b"KDMV") {
        x.format = "VMDK";
        x.virtual_size = le64(head, 12).map(|s| s.saturating_mul(512));
        // The descriptor inside it says how it was made.
        let at = le64(head, 28).unwrap_or(0).saturating_mul(512) as usize;
        let n = le64(head, 36).unwrap_or(0).saturating_mul(512) as usize;
        let desc = head.get(at..at.saturating_add(n).min(head.len())).unwrap_or(&[]);
        x.variant = vmdk_variant(&String::from_utf8_lossy(desc)).or(Some("dynamic"));
    } else if has(head, 0, b"# Disk DescriptorFile") {
        x.format = "VMDK";
        let d = String::from_utf8_lossy(&head[..head.len().min(64 << 10)]).into_owned();
        // Each extent line: `RW 41943040 SPARSE "disk-s001.vmdk"`, its size in sectors.
        let sectors: u64 = d
            .lines()
            .filter_map(|l| {
                let mut w = l.split_whitespace();
                matches!(w.next(), Some("RW" | "RDONLY" | "NOACCESS"))
                    .then(|| w.next()?.parse::<u64>().ok())?
            })
            .sum();
        x.virtual_size = (sectors > 0).then(|| sectors.saturating_mul(512));
        x.variant = vmdk_variant(&d);
    } else if has(head, 64, &0xBEDA_107Fu32.to_le_bytes()) {
        x.format = "VDI";
        x.virtual_size = le64(head, 368);
        x.variant = match le32(head, 76) {
            Some(2) => Some("fixed"),
            Some(4) => Some("differencing"),
            Some(_) => Some("dynamic"),
            None => None,
        };
    } else if has(head, 0, b"MSWIM\0\0\0") {
        x.format = "WIM";
    } else if has(head, 0, b"ITSF") {
        x.format = "chm";
    } else if has(head, 0, b"Rar!\x1a\x07") {
        x.format = "cbr";
    } else if has(tail, 0, b"conectix") || has(head, 0, b"conectix") {
        // The footer, at the end; a dynamic VHD has a copy of it at the start too.
        let foot = if has(tail, 0, b"conectix") {
            tail
        } else {
            head
        };
        x.format = "VHD";
        x.virtual_size = be64(foot, 48);
        x.variant = match be32(foot, 60) {
            Some(2) => Some("fixed"),
            Some(3) => Some("dynamic"),
            Some(4) => Some("differencing"),
            _ => None,
        };
        // A fixed VHD is the disk as it is, then the footer.
        if x.variant == Some("fixed") {
            raw(head, &mut x);
        }
    } else if has(tail, 0, b"koly") {
        x.format = "DMG";
        x.virtual_size = be64(tail, 492).map(|s| s.saturating_mul(512));
    } else {
        x.format = "raw";
        x.virtual_size = Some(len);
        if !raw(head, &mut x) {
            return None;
        }
    }
    Some(x)
}

fn vmdk_variant(desc: &str) -> Option<&'static str> {
    let field = |name: &str| {
        desc.lines()
            .find_map(|l| l.trim().strip_prefix(name)?.trim().strip_prefix('='))
            .map(|v| v.trim().trim_matches('"').to_ascii_lowercase())
    };
    if field("parentCID").is_some_and(|p| p != "ffffffff") {
        return Some("differencing");
    }
    let t = field("createType")?;
    Some(if t.contains("flat") || t.contains("raw") {
        "fixed"
    } else {
        "dynamic"
    })
}

/// A VHDX's disk size and kind, from its metadata region (found through its region table).
fn vhdx(f: &mut File, x: &mut Facts) {
    const METADATA: &str = "8B7CA206-4790-4B9A-B8FE-575F050F886E";
    const SIZE: &str = "2FA54224-CD1B-4876-B211-5DBED83BF4B8";
    const PARAMS: &str = "CAA16737-FA36-4D43-B3B6-33F0AA44E76B";
    let regions = read_at(f, 192 << 10, 64 << 10);
    if !has(&regions, 0, b"regi") {
        return;
    }
    let n = le32(&regions, 8).unwrap_or(0).min(2047) as usize;
    let Some(meta_at) = (0..n).find_map(|i| {
        let e = regions.get(16 + i * 32..16 + i * 32 + 32)?;
        (guid(e) == METADATA).then(|| le64(e, 16))?
    }) else {
        return;
    };
    let table = read_at(f, meta_at, 64 << 10);
    if !has(&table, 0, b"metadata") {
        return;
    }
    let n = le16(&table, 10).unwrap_or(0).min(2047) as usize;
    for i in 0..n {
        let Some(e) = table.get(32 + i * 32..64 + i * 32) else {
            break;
        };
        let at = meta_at.saturating_add(u64::from(le32(e, 16).unwrap_or(0)));
        match guid(e).as_str() {
            SIZE => x.virtual_size = le64(&read_at(f, at, 8), 0),
            PARAMS => {
                let flags = le32(&read_at(f, at, 8), 4).unwrap_or(0);
                x.variant = Some(if flags & 2 != 0 {
                    "differencing"
                } else if flags & 1 != 0 {
                    "fixed"
                } else {
                    "dynamic"
                });
            }
            _ => {}
        }
    }
}

/// A raw disk's start: its partition table, or the file system or ISO volume right at it.
/// False when none is recognised.
fn raw(head: &[u8], x: &mut Facts) -> bool {
    let mut known = false;
    if has(head, 32769, b"CD001") && head.get(32768) == Some(&1) {
        x.format = if x.format == "raw" {
            "ISO 9660"
        } else {
            x.format
        };
        x.label = part(head, 32808, 32).and_then(text);
        let blocks = le32(head, 32768 + 80).unwrap_or(0);
        let size = le16(head, 32768 + 128).unwrap_or(2048);
        if x.format == "ISO 9660" {
            x.virtual_size = Some(u64::from(blocks) * u64::from(size));
        }
        known = true;
    } else if has(head, 32769, b"BEA01") || has(head, 32769, b"NSR0") {
        x.format = if x.format == "raw" { "UDF" } else { x.format };
        known = true;
    }
    if has(head, 512, b"EFI PART") {
        x.table = Some("GPT");
        x.partitions = gpt(head);
        return true;
    }
    let boot = has(head, 510, &[0x55, 0xAA]);
    if let Some((fs, label)) = file_system(head) {
        x.file_system = Some(fs);
        x.label = x.label.take().or(label);
        return true;
    }
    if boot && let Some(parts) = mbr(head) {
        x.table = Some("MBR");
        x.partitions = parts;
        return true;
    }
    known
}

/// The file system that starts at the very beginning, and its label.
fn file_system(b: &[u8]) -> Option<(&'static str, Option<String>)> {
    let boot = has(b, 510, &[0x55, 0xAA]);
    if boot && has(b, 3, b"NTFS    ") {
        return Some(("NTFS", None));
    }
    if boot && has(b, 3, b"EXFAT   ") {
        return Some(("exFAT", None));
    }
    if boot && has(b, 82, b"FAT32") {
        return Some(("FAT32", b.get(71..82).and_then(fat_label)));
    }
    if boot && has(b, 54, b"FAT") {
        let fs = if has(b, 54, b"FAT12") { "FAT12" } else { "FAT16" };
        return Some((fs, b.get(43..54).and_then(fat_label)));
    }
    if has(b, 1080, &[0x53, 0xEF]) {
        return Some(("ext2/3/4", b.get(1144..1160).and_then(text)));
    }
    if has(b, 1024, b"H+") || has(b, 1024, b"HX") {
        return Some(("HFS+", None));
    }
    if has(b, 32, b"NXSB") {
        return Some(("APFS", None));
    }
    None
}

fn fat_label(b: &[u8]) -> Option<String> {
    text(b).filter(|l| l != "NO NAME")
}

/// The four partitions of an MBR, when every entry is sound (a boot flag of 0 or 0x80, room
/// after the table) and one is used. An extended partition's logical ones are not followed.
fn mbr(b: &[u8]) -> Option<Vec<Partition>> {
    let mut out = vec![];
    for i in 0..4 {
        let e = b.get(446 + i * 16..462 + i * 16)?;
        if e[0] != 0 && e[0] != 0x80 {
            return None;
        }
        let (kind, start, n) = (e[4], le32(e, 8)?, le32(e, 12)?);
        if kind == 0 {
            continue;
        }
        if start == 0 || n == 0 {
            return None;
        }
        out.push(Partition {
            kind: mbr_kind(kind),
            start: u64::from(start) * 512,
            size: u64::from(n) * 512,
            name: String::new(),
        });
    }
    (!out.is_empty()).then_some(out)
}

fn mbr_kind(t: u8) -> String {
    match t {
        0x01 => "FAT12",
        0x04 | 0x06 | 0x0e => "FAT16",
        0x0b | 0x0c => "FAT32",
        0x07 => "NTFS/exFAT",
        0x05 | 0x0f | 0x85 => "extended",
        0x82 => "Linux swap",
        0x83 => "Linux",
        0x8e => "Linux LVM",
        0xa5 => "FreeBSD",
        0xa6 => "OpenBSD",
        0xa9 => "NetBSD",
        0xaf => "HFS+",
        0xbf => "Solaris",
        0xee => "GPT",
        0xef => "EFI",
        _ => return format!("0x{t:02x}"),
    }
    .to_string()
}

/// A GPT's partitions, as far as the first MiB holds its entries (512-byte sectors).
fn gpt(b: &[u8]) -> Vec<Partition> {
    let Some(h) = b.get(512..) else {
        return vec![];
    };
    let at = le64(h, 72).unwrap_or(2).saturating_mul(512) as usize;
    let n = le32(h, 80).unwrap_or(0).min(1024) as usize;
    let size = le32(h, 84).unwrap_or(128).max(128) as usize;
    let mut out = vec![];
    for i in 0..n {
        let Some(e) = at
            .checked_add(i.saturating_mul(size))
            .and_then(|s| b.get(s..s.checked_add(128)?))
        else {
            break;
        };
        if e[..16].iter().all(|&c| c == 0) {
            continue;
        }
        let (first, last) = (le64(e, 32).unwrap_or(0), le64(e, 40).unwrap_or(0));
        let name: Vec<u16> = e[56..128]
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        out.push(Partition {
            kind: gpt_kind(&guid(&e[..16])),
            start: first.saturating_mul(512),
            size: last
                .saturating_sub(first)
                .saturating_add(1)
                .saturating_mul(512),
            name: String::from_utf16_lossy(&name),
        });
    }
    out
}

fn gpt_kind(g: &str) -> String {
    match g {
        "C12A7328-F81F-11D2-BA4B-00A0C93EC93B" => "EFI",
        "21686148-6449-6E6F-744E-656564454649" => "BIOS boot",
        "EBD0A0A2-B9E5-4433-87C0-68B6B72699C7" => "Microsoft basic data",
        "E3C9E316-0B5C-4DB8-817D-F92DF00215AE" => "Microsoft reserved",
        "DE94BBA4-06D1-4D40-A16A-BFD50179D6AC" => "Windows recovery",
        "0FC63DAF-8483-4772-8E79-3D69D8477DE4" => "Linux",
        "0657FD6D-A4AB-43C4-84E5-0933C84B4F4F" => "Linux swap",
        "E6D6D379-F507-44C2-A23C-238F2A3DF928" => "Linux LVM",
        "4F68BCE3-E8CD-4DB1-96E7-FBCAF984B709" => "Linux root (x86-64)",
        "83BD6B9D-7F41-11DC-BE0B-001560B84F0F" => "FreeBSD boot",
        "516E7CB4-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD",
        "516E7CB5-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD swap",
        "516E7CB6-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD UFS",
        "516E7CBA-6ECF-11D6-8FF8-00022D09712B" => "FreeBSD ZFS",
        "824CC7A0-36A8-11E3-890A-952519AD3F61" => "OpenBSD",
        "49F48D5A-B10E-11DC-B99B-0019D1879648" => "NetBSD FFS",
        "6A898CC3-1DD2-11B2-99A6-080020736631" => "illumos ZFS",
        "48465300-0000-11AA-AA11-00306543ECAC" => "HFS+",
        "7C3457EF-0000-11AA-AA11-00306543ECAC" => "APFS",
        _ => return g.to_string(),
    }
    .to_string()
}

/// The facts as lines of (label, value) in the user's language, for the preview and
/// Properties in both apps.
pub fn lines(x: &Facts) -> Vec<(String, String)> {
    use crate::settings::human;
    let mut v = vec![];
    let format = match x.format {
        "raw" | "chm" | "cbr" => crate::i18n::tr(&format!("disk_image.{}", x.format), &[]),
        f => f.to_string(),
    };
    let format = match x.variant {
        Some(k) => format!(
            "{format} ({})",
            crate::i18n::tr(&format!("disk_image.{k}"), &[])
        ),
        None => format,
    };
    v.push((crate::t!("disk_image.format"), format));
    if let Some(n) = x.virtual_size {
        v.push((crate::t!("disk_image.virtual_size"), human(n)));
    }
    v.push((crate::t!("disk_image.on_disk"), human(x.on_disk)));
    if let Some(l) = &x.label {
        v.push((crate::t!("disk_image.label"), l.clone()));
    }
    if let Some(fs) = x.file_system {
        v.push((crate::t!("disk_image.file_system"), fs.to_string()));
    }
    if let Some(t) = x.table {
        v.push((
            crate::t!("disk_image.table"),
            crate::t!("disk_image.table_value", "table" => t, "n" => x.partitions.len()),
        ));
    }
    for (i, p) in x.partitions.iter().enumerate() {
        let mut s = crate::t!("disk_image.partition_value", "kind" => p.kind, "size" => human(p.size), "start" => human(p.start));
        if !p.name.is_empty() {
            s = format!("{s} · {}", p.name);
        }
        v.push((
            crate::t!("disk_image.partition", "n" => i + 1),
            s,
        ));
    }
    v
}

// ---------------------------------------------------------------- what more is possible

/// What the user can do next with a disk image, said under its facts in both apps.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "state", content = "line")]
pub enum Next {
    /// The option is off: Settings turns it on.
    TurnOn,
    /// On, but 7-Zip is not installed: the line that installs it here, if one is known.
    Install(Option<String>),
    /// On, and 7-Zip is there: Enter opens it.
    Open,
}

pub fn next() -> Next {
    if !open_on() {
        Next::TurnOn
    } else if seven_zip().is_none() {
        Next::Install(crate::tools::install("7z"))
    } else {
        Next::Open
    }
}

/// `next` as one line of the terminal app: `open` is the key that opens, `settings` the one
/// that opens Settings, `menu` the action menu's.
pub fn next_line(open: &str, settings: &str, menu: &str) -> String {
    match next() {
        Next::TurnOn => crate::t!("disk_image.next_off_tui", "key" => settings, "menu" => menu),
        Next::Install(Some(line)) => crate::t!("disk_image.next_install", "command" => line),
        Next::Install(None) => crate::t!("disk_image.next_install_get"),
        Next::Open => crate::t!("disk_image.next_open", "key" => open),
    }
}

// ---------------------------------------------------------------- 7-Zip

/// The installed 7-Zip that reads disk images: `7zz` (7-Zip itself, as FreeBSD's 7-zip and
/// Homebrew's sevenzip install it), else `7z` (p7zip, or 7-Zip's 7z.exe on Windows). Not
/// `7za`, which reads none of them.
pub fn seven_zip() -> Option<PathBuf> {
    crate::tools::which("7zz").or_else(|| crate::tools::which("7z"))
}

fn no_seven_zip() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        match crate::tools::install("7z") {
            Some(line) => crate::t!("disk_image.next_install", "command" => line),
            None => crate::t!("disk_image.next_install_get"),
        },
    )
}

/// 7-Zip with the switches every call takes: no questions (no password is ever given, so a
/// locked image fails instead of waiting), names in UTF-8, no wildcards in them.
fn seven(op: &str) -> io::Result<std::process::Command> {
    let mut c = crate::tools::command(seven_zip().ok_or_else(no_seven_zip)?);
    c.args([op, "-y", "-bd", "-spd", "-sccUTF-8", "-scsUTF-8", "-p-"]);
    c.stdin(std::process::Stdio::null());
    Ok(c)
}

/// What 7-Zip said when it failed: its error lines.
fn failed(err: &[u8]) -> io::Error {
    let s = String::from_utf8_lossy(err);
    let said: Vec<&str> = s
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("7-Zip") && !l.starts_with("64-bit"))
        .collect();
    io::Error::other(format!("7-Zip: {}", said.join(" ")))
}

/// An entry of a disk image as 7-Zip lists it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Entry {
    pub name: String,
    pub size: u64,
    pub dir: bool,
    pub modified: u64,
    /// A symbolic link: listed, never made when copied out.
    pub link: bool,
}

/// The most 7-Zip prints for one listing.
const LIST_MAX: u64 = 64 << 20;

/// Images whose listing was longer than `LIST_MAX`: only their first entries are known, and
/// the preview says there are more.
static CUT: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());

/// Whether `image`'s last listing was cut at `LIST_MAX`.
pub(crate) fn was_cut(image: &Path) -> bool {
    CUT.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == image)
}

/// The entries of `image`, all of them, as far as `LIST_MAX` of 7-Zip's listing holds
/// (`was_cut` says when it did not).
pub(crate) fn list(image: &Path) -> io::Result<Vec<Entry>> {
    let mut c = seven("l")?;
    c.args(["-slt", "-ba", "--"]).arg(image);
    let out = crate::tools::capture(c, std::time::Duration::from_secs(300), LIST_MAX)?;
    if !out.status.success() {
        return Err(failed(&out.stderr));
    }
    let cut = out.stdout.len() as u64 >= LIST_MAX;
    let mut cuts = CUT.lock().unwrap_or_else(|e| e.into_inner());
    cuts.retain(|p| p != image);
    if cut {
        cuts.push(image.to_path_buf());
    }
    drop(cuts);
    let text = String::from_utf8_lossy(&out.stdout);
    // The block the cut went through is left out, not taken for a whole entry.
    let whole = if cut { text.rsplit_once("\n\n").map_or("", |(w, _)| w) } else { &text };
    Ok(parse_list(whole, usize::MAX))
}

/// `7z l -slt -ba`: a block of `Key = value` lines per entry, a blank line between.
fn parse_list(s: &str, max: usize) -> Vec<Entry> {
    let mut out = vec![];
    for block in s.replace("\r\n", "\n").split("\n\n") {
        let get = |k: &str| {
            block
                .lines()
                .find_map(|l| l.strip_prefix(k)?.strip_prefix(" = ").or(l.strip_prefix(k)?.strip_prefix(" =")))
        };
        let Some(path) = get("Path").filter(|p| !p.is_empty()) else {
            continue;
        };
        let modified = get("Modified")
            .and_then(|m| chrono::NaiveDateTime::parse_from_str(m.get(..19)?, "%Y-%m-%d %H:%M:%S").ok())
            .and_then(|t| {
                use chrono::TimeZone;
                chrono::Local.from_local_datetime(&t).earliest()
            })
            .map_or(0, |t| t.timestamp().max(0) as u64);
        out.push(Entry {
            // 7-Zip writes `\` between folders on Windows; elsewhere it is a letter of a name.
            name: if cfg!(windows) { path.replace('\\', "/") } else { path.to_string() }.trim_end_matches('/').to_string(),
            size: get("Size").and_then(|s| s.parse().ok()).unwrap_or(0),
            dir: get("Folder") == Some("+") || get("Attributes").is_some_and(|a| a.starts_with('D')),
            modified,
            link: get("Symbolic Link").is_some_and(|l| !l.is_empty()),
        });
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Reads through, counting.
struct Counted<R> {
    r: R,
    n: u64,
}

impl<R: Read> Read for Counted<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = self.r.read(buf)?;
        self.n += n as u64;
        Ok(n)
    }
}

/// The file `inner` of `image`, `size` bytes as listed, as 7-Zip writes it out, given to
/// `write`. Fewer or more bytes than listed is an error: 7-Zip says nothing, and succeeds, when
/// a name matches no file.
pub(crate) fn read_file(
    image: &Path,
    inner: &str,
    size: u64,
    write: &mut dyn FnMut(&mut dyn Read) -> io::Result<()>,
) -> io::Result<()> {
    let mut c = seven("e")?;
    c.arg("-so").arg("--").arg(image).arg(inner);
    c.stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = c.spawn()?;
    let mut err = child.stderr.take();
    let said = std::thread::spawn(move || {
        let mut v = vec![];
        if let Some(e) = err.as_mut() {
            let _ = e.take(64 << 10).read_to_end(&mut v);
            let _ = io::copy(e, &mut io::sink());
        }
        v
    });
    let mut out = Counted {
        r: child.stdout.take().expect("piped"),
        n: 0,
    };
    let wrote = write(&mut out);
    let got = out.n;
    drop(out);
    if wrote.is_err() {
        let _ = child.kill();
    }
    let status = child.wait()?;
    let said = said.join().unwrap_or_default();
    wrote?;
    if !status.success() {
        return Err(failed(&said));
    }
    if got != size {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            crate::t!("err.not_in_the_archive", "name" => inner),
        ));
    }
    Ok(())
}

/// The 7-Zip found is p7zip, which makes the links an image holds even when told not to:
/// folders then come out file by file.
fn p7zip() -> bool {
    static BULK: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    !*BULK.get_or_init(|| {
        seven_zip().is_some_and(|p| {
            crate::tools::output(&p, &[], std::time::Duration::from_secs(10), 4096)
                .or_else(|| {
                    let c = crate::tools::command(&p);
                    crate::tools::capture(c, std::time::Duration::from_secs(10), 4096)
                        .ok()
                        .map(|o| o.stdout)
                })
                .is_some_and(|o| bulk_safe(&String::from_utf8_lossy(&o)))
        })
    })
}

/// Whether 7-Zip's banner names a 7-Zip that honours `-snl-` (21 and later, not p7zip). Only a
/// positive answer allows the folder-at-once extraction; anything unclear takes the file-by-file
/// way, which makes no links whatever the program does.
fn bulk_safe(banner: &str) -> bool {
    let Some(line) = banner.lines().find(|l| l.trim_start().starts_with("7-Zip")) else {
        return false;
    };
    if banner.contains("p7zip") {
        return false;
    }
    line.split_whitespace()
        .find_map(|w| w.split('.').next()?.parse::<u32>().ok().filter(|_| w.contains('.')))
        .is_some_and(|major| major >= 21)
}

/// `inner` of `image` ("" for all of it) copied out to `to`, which must not exist yet: a
/// folder with all below it. Through a folder of its own next to `to`, which then takes its
/// place, so a failure leaves nothing behind. No link is made (`-snl-`), so nothing can be
/// written through one; 7-Zip keeps every name inside the folder. With p7zip, which makes links
/// anyway, each file of `entries` (the image's listing) comes out on its own instead.
pub(crate) fn copy_tree(image: &Path, inner: &str, entries: &[Entry], to: &Path) -> io::Result<()> {
    let parent = to.parent().unwrap_or(Path::new("."));
    let stage = parent.join(format!(
        ".{}.{}.coxswain-tmp",
        to.file_name().unwrap_or_default().to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&stage);
    std::fs::create_dir(&stage)?;
    let r = (|| -> io::Result<()> {
        if p7zip() {
            one_by_one(image, inner, entries, &stage)?;
        } else {
            let mut c = seven("x")?;
            let mut o = std::ffi::OsString::from("-o");
            o.push(&stage);
            c.arg(o).arg("-snl-").arg("--").arg(image);
            if !inner.is_empty() {
                c.arg(inner);
            }
            let out = crate::tools::capture(c, std::time::Duration::from_secs(24 * 3600), 1 << 20)?;
            if !out.status.success() {
                return Err(failed(&out.stderr));
            }
        }
        // Belt and braces: a link that came out anyway goes.
        unlink(&stage)?;
        let from = inner
            .split('/')
            .filter(|p| !p.is_empty())
            .fold(stage.clone(), |p, part| p.join(part));
        if std::fs::symlink_metadata(&from).is_err() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                crate::t!("err.not_in_the_archive", "name" => inner),
            ));
        }
        std::fs::rename(&from, to)
    })();
    let _ = std::fs::remove_dir_all(&stage);
    r
}

/// The entries at and below `inner` into `stage` one file at a time, at their paths inside it
/// (a `..` or an empty part is refused), links left out.
fn one_by_one(image: &Path, inner: &str, entries: &[Entry], stage: &Path) -> io::Result<()> {
    let prefix = format!("{inner}/");
    for e in entries.iter().filter(|e| !e.link && (inner.is_empty() || e.name == inner || e.name.starts_with(&prefix))) {
        let mut at = stage.to_path_buf();
        for part in e.name.split('/') {
            if part.is_empty() || part == "." || part == ".." || (cfg!(windows) && part.contains([':', '\\'])) {
                return Err(io::Error::new(io::ErrorKind::InvalidData, crate::t!("err.not_in_the_archive", "name" => e.name)));
            }
            at.push(part);
        }
        if e.dir {
            std::fs::create_dir_all(&at)?;
            continue;
        }
        if let Some(p) = at.parent() {
            std::fs::create_dir_all(p)?;
        }
        read_file(image, &e.name, e.size, &mut |r| io::copy(r, &mut File::create(&at)?).map(drop))?;
    }
    Ok(())
}

/// Every symbolic link below `dir` taken away (never followed).
fn unlink(dir: &Path) -> io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        let t = e.file_type()?;
        if t.is_symlink() {
            std::fs::remove_file(e.path())?;
        } else if t.is_dir() {
            unlink(&e.path())?;
        }
    }
    Ok(())
}

/// What an error says when something would be written into a disk image.
pub fn read_only() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        crate::t!("disk_image.read_only"),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn folders_come_out_at_once_only_with_a_7zip_known_to_make_no_links() {
        assert!(bulk_safe("\n7-Zip (z) 26.04 (x64) : Copyright (c) 1999-2026 Igor Pavlov : 2026-04-04\n"));
        assert!(bulk_safe("7-Zip 21.07 (x64) : Copyright"));
        assert!(!bulk_safe("7-Zip [64] 16.02 : Copyright (c) 1999-2016 Igor Pavlov : 2016-05-21\np7zip Version 16.02"));
        assert!(!bulk_safe("p7zip Version 16.02 (locale=C,Utf16=off)\n7-Zip [64] 16.02"));
        assert!(!bulk_safe("7-Zip 19.00 (x64)"));
        assert!(!bulk_safe(""));
        assert!(!bulk_safe("something else 30.1"));
    }

    pub(crate) fn folder(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("coxswain-disk-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A VHD footer for a disk of `size` bytes of kind `kind` (2 fixed, 3 dynamic).
    fn footer(size: u64, kind: u32) -> Vec<u8> {
        let mut f = vec![0u8; 512];
        f[..8].copy_from_slice(b"conectix");
        f[48..56].copy_from_slice(&size.to_be_bytes());
        f[60..64].copy_from_slice(&kind.to_be_bytes());
        f
    }

    /// A raw disk of 2 MiB with an MBR: a FAT32 partition from 1 MiB.
    fn mbr_disk() -> Vec<u8> {
        let mut d = vec![0u8; 2 << 20];
        let e = &mut d[446..462];
        e[4] = 0x0c;
        e[8..12].copy_from_slice(&2048u32.to_le_bytes());
        e[12..16].copy_from_slice(&2048u32.to_le_bytes());
        d[510] = 0x55;
        d[511] = 0xAA;
        d
    }

    /// A minimal ISO 9660 image (2048-byte blocks) with the volume label `label` and one file,
    /// README.TXT, holding `body`.
    pub(crate) fn iso(label: &str, body: &[u8]) -> Vec<u8> {
        const B: usize = 2048;
        let mut d = vec![0u8; 23 * B];
        let both32 = |d: &mut [u8], at: usize, v: u32| {
            d[at..at + 4].copy_from_slice(&v.to_le_bytes());
            d[at + 4..at + 8].copy_from_slice(&v.to_be_bytes());
        };
        let both16 = |d: &mut [u8], at: usize, v: u16| {
            d[at..at + 2].copy_from_slice(&v.to_le_bytes());
            d[at + 2..at + 4].copy_from_slice(&v.to_be_bytes());
        };
        let record = |name: &[u8], extent: u32, len: u32, dir: bool| {
            let n = 33 + name.len() + (name.len() + 1) % 2;
            let mut r = vec![0u8; n];
            r[0] = n as u8;
            r[2..6].copy_from_slice(&extent.to_le_bytes());
            r[6..10].copy_from_slice(&extent.to_be_bytes());
            r[10..14].copy_from_slice(&len.to_le_bytes());
            r[14..18].copy_from_slice(&len.to_be_bytes());
            r[18..25].copy_from_slice(&[126, 10, 19, 12, 0, 0, 0]);
            r[25] = if dir { 2 } else { 0 };
            r[28..30].copy_from_slice(&1u16.to_le_bytes());
            r[30..32].copy_from_slice(&1u16.to_be_bytes());
            r[32] = name.len() as u8;
            r[33..33 + name.len()].copy_from_slice(name);
            r
        };
        let p = 16 * B;
        d[p] = 1;
        d[p + 1..p + 6].copy_from_slice(b"CD001");
        d[p + 6] = 1;
        d[p + 8..p + 72].fill(b' ');
        d[p + 40..p + 40 + label.len()].copy_from_slice(label.as_bytes());
        both32(&mut d, p + 80, 23);
        both16(&mut d, p + 120, 1);
        both16(&mut d, p + 124, 1);
        both16(&mut d, p + 128, B as u16);
        both32(&mut d, p + 132, 10);
        d[p + 140..p + 144].copy_from_slice(&18u32.to_le_bytes());
        d[p + 148..p + 152].copy_from_slice(&19u32.to_be_bytes());
        let root = record(&[0], 20, B as u32, true);
        d[p + 156..p + 156 + 34].copy_from_slice(&root);
        d[p + 881] = 1;
        let t = 17 * B;
        d[t] = 255;
        d[t + 1..t + 6].copy_from_slice(b"CD001");
        d[t + 6] = 1;
        for (at, le) in [(18 * B, true), (19 * B, false)] {
            d[at] = 1;
            let ext = if le { 20u32.to_le_bytes() } else { 20u32.to_be_bytes() };
            d[at + 2..at + 6].copy_from_slice(&ext);
            d[at + 6..at + 8].copy_from_slice(&1u16.to_le_bytes());
        }
        let mut dir = vec![];
        dir.extend(record(&[0], 20, B as u32, true));
        dir.extend(record(&[1], 20, B as u32, true));
        dir.extend(record(b"README.TXT;1", 21, body.len() as u32, false));
        d[20 * B..20 * B + dir.len()].copy_from_slice(&dir);
        d[21 * B..21 * B + body.len()].copy_from_slice(body);
        d
    }

    fn facts_of(name: &str, bytes: &[u8]) -> Option<Facts> {
        let d = folder(&format!("facts-{name}"));
        let p = d.join(name);
        std::fs::write(&p, bytes).unwrap();
        let f = facts(&p);
        let _ = std::fs::remove_dir_all(&d);
        f
    }

    #[test]
    fn vhd_footers_say_size_and_kind_and_a_fixed_ones_partitions() {
        let mut fixed = mbr_disk();
        fixed.extend(footer(2 << 20, 2));
        let f = facts_of("a.vhd", &fixed).unwrap();
        assert_eq!((f.format, f.virtual_size, f.variant), ("VHD", Some(2 << 20), Some("fixed")));
        assert_eq!(f.table, Some("MBR"));
        assert_eq!(
            f.partitions,
            vec![Partition { kind: "FAT32".into(), start: 1 << 20, size: 1 << 20, name: String::new() }]
        );
        // A dynamic one: the footer's copy at the start, then the rest.
        let mut dynamic = footer(40 << 30, 3);
        dynamic.extend(vec![0u8; 1024]);
        dynamic.extend(footer(40 << 30, 3));
        let f = facts_of("b.vhd", &dynamic).unwrap();
        assert_eq!((f.virtual_size, f.variant), (Some(40 << 30), Some("dynamic")));
        assert!(f.partitions.is_empty());
        let mut diff = vec![0u8; 512];
        diff.extend(footer(1 << 30, 4));
        assert_eq!(facts_of("c.vhd", &diff).unwrap().variant, Some("differencing"));
    }

    #[test]
    fn vhdl_source_named_vhd_is_no_disk_image() {
        let src = b"library ieee;\nuse ieee.std_logic_1164.all;\n\nentity blink is\n  port (clk : in std_logic);\nend entity;\n";
        let d = folder("vhdl");
        let p = d.join("blink.vhd");
        std::fs::write(&p, src).unwrap();
        assert!(facts(&p).is_none());
        assert!(!is_disk_image(&p));
        assert!(crate::code::is_code(&p.to_string_lossy()), "still code");
        let mut img = mbr_disk();
        img.extend(footer(2 << 20, 2));
        let v = d.join("disk.vhd");
        std::fs::write(&v, &img).unwrap();
        assert!(is_disk_image(&v));
        // Search never reads the image as source: its start has zero bytes.
        assert_eq!(crate::extract::text_of(&v, img.len() as u64, u64::MAX), None);
        assert!(crate::extract::text_of(&p, src.len() as u64, u64::MAX).is_some());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn qcow2_vmdk_vdi_dmg_and_wim_headers() {
        let mut q = vec![0u8; 512];
        q[..4].copy_from_slice(b"QFI\xfb");
        q[4..8].copy_from_slice(&3u32.to_be_bytes());
        q[24..32].copy_from_slice(&(10u64 << 30).to_be_bytes());
        let f = facts_of("a.qcow2", &q).unwrap();
        assert_eq!((f.format, f.virtual_size, f.variant), ("QCOW2", Some(10 << 30), Some("dynamic")));

        let mut k = vec![0u8; 2048];
        k[..4].copy_from_slice(b"KDMV");
        k[12..20].copy_from_slice(&2_097_152u64.to_le_bytes());
        k[28..36].copy_from_slice(&1u64.to_le_bytes());
        k[36..44].copy_from_slice(&1u64.to_le_bytes());
        let desc = b"# Disk DescriptorFile\ncreateType=\"streamOptimized\"\n";
        k[512..512 + desc.len()].copy_from_slice(desc);
        let f = facts_of("a.vmdk", &k).unwrap();
        assert_eq!((f.format, f.virtual_size, f.variant), ("VMDK", Some(1 << 30), Some("dynamic")));

        let text = "# Disk DescriptorFile\nversion=1\nparentCID=ffffffff\ncreateType=\"monolithicFlat\"\n\nRW 2048 FLAT \"a-flat.vmdk\" 0\nRW 2048 FLAT \"b-flat.vmdk\" 0\n";
        let f = facts_of("d.vmdk", text.as_bytes()).unwrap();
        assert_eq!((f.virtual_size, f.variant), (Some(2 << 20), Some("fixed")));

        let mut v = vec![0u8; 512];
        v[64..68].copy_from_slice(&0xBEDA_107Fu32.to_le_bytes());
        v[76..80].copy_from_slice(&2u32.to_le_bytes());
        v[368..376].copy_from_slice(&(8u64 << 30).to_le_bytes());
        let f = facts_of("a.vdi", &v).unwrap();
        assert_eq!((f.format, f.virtual_size, f.variant), ("VDI", Some(8 << 30), Some("fixed")));

        let mut dmg = vec![0u8; 1024];
        let t = dmg.len() - 512;
        dmg[t..t + 4].copy_from_slice(b"koly");
        dmg[t + 492..t + 500].copy_from_slice(&4096u64.to_be_bytes());
        let f = facts_of("a.dmg", &dmg).unwrap();
        assert_eq!((f.format, f.virtual_size), ("DMG", Some(2 << 20)));

        let mut w = vec![0u8; 512];
        w[..8].copy_from_slice(b"MSWIM\0\0\0");
        assert_eq!(facts_of("a.wim", &w).unwrap().format, "WIM");
        let f = facts_of("help.chm", b"ITSF\x03\0\0\0").unwrap();
        assert_eq!((f.format, f.virtual_size), ("chm", None));
        assert_eq!(facts_of("issue.cbr", b"Rar!\x1a\x07\x01\0").unwrap().format, "cbr");
        // By their names alone they open; search still never reads them.
        assert!(named(Path::new("a.CHM")) && is_disk_image(Path::new("x.cbr")));
        assert!(!crate::archive::is_archive(Path::new("x.cbr")));
    }

    #[test]
    fn vhdx_size_and_kind_come_from_its_metadata() {
        let mut d = vec![0u8; 1 << 20];
        d[..8].copy_from_slice(b"vhdxfile");
        let guid_bytes = |g: &str| {
            let h: Vec<u8> = g
                .replace('-', "")
                .as_bytes()
                .chunks(2)
                .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
                .collect();
            let mut b = h.clone();
            b[..4].reverse();
            b[4..6].reverse();
            b[6..8].reverse();
            b
        };
        let r = 192 << 10;
        d[r..r + 4].copy_from_slice(b"regi");
        d[r + 8..r + 12].copy_from_slice(&1u32.to_le_bytes());
        d[r + 16..r + 32].copy_from_slice(&guid_bytes("8B7CA206-4790-4B9A-B8FE-575F050F886E"));
        let m = 512 << 10;
        d[r + 32..r + 40].copy_from_slice(&(m as u64).to_le_bytes());
        d[m..m + 8].copy_from_slice(b"metadata");
        d[m + 10..m + 12].copy_from_slice(&2u16.to_le_bytes());
        for (i, (g, off)) in [
            ("2FA54224-CD1B-4876-B211-5DBED83BF4B8", 65536u32),
            ("CAA16737-FA36-4D43-B3B6-33F0AA44E76B", 65544),
        ]
        .into_iter()
        .enumerate()
        {
            let e = m + 32 + i * 32;
            d[e..e + 16].copy_from_slice(&guid_bytes(g));
            d[e + 16..e + 20].copy_from_slice(&off.to_le_bytes());
        }
        d[m + 65536..m + 65544].copy_from_slice(&(127u64 << 30).to_le_bytes());
        d[m + 65548..m + 65552].copy_from_slice(&2u32.to_le_bytes());
        let f = facts_of("a.vhdx", &d).unwrap();
        assert_eq!((f.format, f.virtual_size, f.variant), ("VHDX", Some(127 << 30), Some("differencing")));
    }

    #[test]
    fn raw_images_need_a_table_a_file_system_or_a_volume() {
        let f = facts_of("disk.img", &mbr_disk()).unwrap();
        assert_eq!((f.format, f.table), ("raw", Some("MBR")));

        let mut g = vec![0u8; 1 << 20];
        g[510] = 0x55;
        g[511] = 0xAA;
        g[446 + 4] = 0xee;
        g[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
        g[446 + 12..446 + 16].copy_from_slice(&2047u32.to_le_bytes());
        g[512..520].copy_from_slice(b"EFI PART");
        g[512 + 72..512 + 80].copy_from_slice(&2u64.to_le_bytes());
        g[512 + 80..512 + 84].copy_from_slice(&128u32.to_le_bytes());
        g[512 + 84..512 + 88].copy_from_slice(&128u32.to_le_bytes());
        let e = 1024;
        // EFI system partition, as written on disk.
        g[e..e + 16].copy_from_slice(&[
            0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E,
            0xC9, 0x3B,
        ]);
        g[e + 32..e + 40].copy_from_slice(&2048u64.to_le_bytes());
        g[e + 40..e + 48].copy_from_slice(&4095u64.to_le_bytes());
        for (i, c) in "boot".encode_utf16().enumerate() {
            g[e + 56 + i * 2..e + 58 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        let f = facts_of("gpt.img", &g).unwrap();
        assert_eq!(f.table, Some("GPT"));
        assert_eq!(
            f.partitions,
            vec![Partition { kind: "EFI".into(), start: 1 << 20, size: 1 << 20, name: "boot".into() }]
        );

        let mut fat = vec![0u8; 4096];
        fat[510] = 0x55;
        fat[511] = 0xAA;
        fat[54..59].copy_from_slice(b"FAT16");
        fat[43..54].copy_from_slice(b"DEMODISK   ");
        let f = facts_of("fat.img", &fat).unwrap();
        assert_eq!((f.file_system, f.label.as_deref()), (Some("FAT16"), Some("DEMODISK")));

        let iso = iso("DEMO_ISO", b"hello\n");
        let f = facts_of("demo.iso", &iso).unwrap();
        assert_eq!((f.format, f.label.as_deref(), f.virtual_size), ("ISO 9660", Some("DEMO_ISO"), Some(23 * 2048)));

        // A picture, a text, nothing at all.
        assert!(facts_of("photo.img", b"\x89PNG\r\n\x1a\n").is_none());
        assert!(facts_of("notes.img", &vec![b'a'; 4096]).is_none());
        assert!(facts_of("empty.img", b"").is_none());
        let mut bad = mbr_disk();
        bad[446] = 0x12;
        assert!(facts_of("bad.img", &bad).is_none());
    }

    #[test]
    fn seven_zip_listings_are_read() {
        let s = "Path = docs\nFolder = +\nSize = \nModified = 2026-10-19 15:28:42\n\nPath = docs/notes.md\nFolder = -\nSize = 5\nModified = 2026-10-19 15:28:42.1234567\n\nPath = link\nFolder = -\nSize = 4\nSymbolic Link = /etc/passwd\n\nPath = 0.fat\nSize = 2097152\nFile System = Windows BDP\n\n";
        let e = parse_list(s, 100);
        assert_eq!(e.len(), 4);
        assert!(e[0].dir && e[0].modified > 0);
        assert_eq!((e[1].name.as_str(), e[1].size, e[1].dir), ("docs/notes.md", 5, false));
        assert!(e[2].link && !e[3].dir);
        assert_eq!(parse_list(s, 2).len(), 2);
        // A CHM, as 7-Zip lists one (7-Zip cannot write CHM or RAR, so no real one is made here).
        let chm = "Path = #SYSTEM\nSize = 4232\nPacked Size = \nMethod = \nBlock = \n\nPath = index.html\nSize = 1520\nPacked Size = \nMethod = LZX:16\nBlock = 0\n\nPath = images/logo.png\nSize = 9312\nPacked Size = \nMethod = LZX:16\nBlock = 0\n\n";
        let e = parse_list(chm, 100);
        assert_eq!(e.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["#SYSTEM", "index.html", "images/logo.png"]);
        assert!(e.iter().all(|e| !e.dir));
        // A CBR (RAR 5), as 7-Zip lists one.
        let cbr = "Path = Issue 1\nFolder = +\nSize = 0\nPacked Size = 0\nModified = 2024-03-01 10:00:00.1234567\nAttributes = D\n\nPath = Issue 1/001.jpg\nFolder = -\nSize = 245760\nPacked Size = 240000\nModified = 2024-03-01 10:00:00.1234567\nAttributes = A\nEncrypted = -\nSolid = -\n\n";
        let e = parse_list(cbr, 100);
        assert!(e[0].dir);
        assert_eq!((e[1].name.as_str(), e[1].size), ("Issue 1/001.jpg", 245760));
    }

    /// Like a user: an ISO opened like a folder with the 7-Zip installed here, a file listed,
    /// copied out and unpacked with the rest, and nothing written into it. Skipped without 7-Zip.
    #[test]
    fn an_iso_opens_like_a_folder_read_only_with_seven_zip() {
        if seven_zip().is_none() {
            eprintln!("no 7-Zip here: skipped");
            return;
        }
        let d = folder("open");
        let image = d.join("demo.iso");
        std::fs::write(&image, iso("DEMO", b"hello from the iso\n")).unwrap();
        assert!(!crate::archive::opens(&image), "off by default");
        FORCE.with(|f| f.set(true));
        assert!(crate::archive::opens(&image));
        assert!(!crate::archive::is_archive(&image), "search never looks inside");
        let inside = image.join("README.TXT");
        assert_eq!(crate::archive::split(&inside), Some((image.clone(), "README.TXT".into())));
        let (entries, locked) = crate::archive::listing(&image, "").unwrap();
        assert!(!locked);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["..", "README.TXT"]);
        assert_eq!(entries[1].size, 19);
        let out = d.join("out");
        std::fs::create_dir(&out).unwrap();
        let got = crate::fs::copy(&inside, &out).unwrap();
        assert_eq!(std::fs::read(&got).unwrap(), b"hello from the iso\n");
        // A name 7-Zip would take as a wildcard or a switch is not one that is there.
        assert!(crate::archive::copy_out(&image, "*", &out, None, false).is_err());
        assert!(crate::archive::copy_out(&image, "-ao", &out, None, false).is_err());
        let all = crate::archive::extract(&image, &out).unwrap();
        assert_eq!(std::fs::read(all.join("README.TXT")).unwrap(), b"hello from the iso\n");
        // Nothing goes in, nothing is changed or taken out.
        let before = std::fs::read(&image).unwrap();
        let note = d.join("note.txt");
        std::fs::write(&note, "x").unwrap();
        for err in [
            crate::fs::copy(&note, &image).unwrap_err(),
            crate::fs::delete(&inside).unwrap_err(),
            crate::fs::rename(&inside, &out.join("moved")).unwrap_err(),
            crate::archive::mkdir(&image, "new", None).unwrap_err(),
        ] {
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
        }
        assert_eq!(std::fs::read(&image).unwrap(), before);
        assert!(!out.join("moved").exists());
        // Search does not read it.
        assert!(crate::archive::search_entries(&image, before.len() as u64).is_none());
        FORCE.with(|f| f.set(false));
        let _ = std::fs::remove_dir_all(&d);
    }

    /// A name 7-Zip matches to nothing gives no bytes and success: that is an error here, not
    /// an empty file. And no link comes out of an image, so nothing is written through one.
    #[test]
    fn missing_names_fail_and_links_never_come_out() {
        if seven_zip().is_none() {
            eprintln!("no 7-Zip here: skipped");
            return;
        }
        let d = folder("links");
        let image = d.join("demo.iso");
        std::fs::write(&image, iso("DEMO", b"hello\n")).unwrap();
        let mut sink = vec![];
        assert!(read_file(&image, "NOT-THERE.TXT", 6, &mut |r| io::copy(r, &mut sink).map(drop)).is_err());
        assert!(read_file(&image, "README.TXT", 6, &mut |r| io::copy(r, &mut sink).map(drop)).is_ok());
        assert_eq!(sink, b"hello\n");
        // A tar (7-Zip reads it the same way) with a link to outside, then a file through it.
        let outside = d.join("outside");
        std::fs::create_dir(&outside).unwrap();
        let tar = d.join("evil.tar");
        {
            let mut b = tar::Builder::new(File::create(&tar).unwrap());
            let mut h = tar::Header::new_gnu();
            h.set_entry_type(tar::EntryType::Symlink);
            h.set_size(0);
            h.set_mode(0o777);
            b.append_link(&mut h, "lnk", &outside).unwrap();
            let mut h = tar::Header::new_gnu();
            h.set_size(4);
            h.set_mode(0o644);
            b.append_data(&mut h, "lnk/evil.txt", &b"evil"[..]).unwrap();
            b.finish().unwrap();
        }
        let entries = list(&tar).unwrap();
        let to = d.join("out");
        let _ = copy_tree(&tar, "", &entries, &to);
        assert!(!outside.join("evil.txt").exists(), "written through a link");
        if to.exists() {
            for e in std::fs::read_dir(&to).unwrap() {
                assert!(!e.unwrap().file_type().unwrap().is_symlink());
            }
        }
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_backslash_is_part_of_a_name_on_unix() {
        let e = parse_list("Path = a\\b.txt\nSize = 1\n\n", 10);
        assert_eq!(e[0].name, if cfg!(windows) { "a/b.txt" } else { "a\\b.txt" });
    }

    /// Every format's header cut short at every length near what is read, and with each of its
    /// fields made huge: nothing panics, whatever the file says (a short `.iso` crashed the
    /// terminal app on a cursor move).
    #[test]
    fn short_and_crafted_headers_never_panic() {
        let mut samples: Vec<Vec<u8>> = vec![];
        let mut v = mbr_disk();
        v.extend(footer(2 << 20, 2));
        samples.push(v);
        let mut v = footer(1 << 30, 3);
        v.extend(vec![0u8; 1024]);
        samples.push(v);
        let mut q = vec![0u8; 512];
        q[..4].copy_from_slice(b"QFI\xfb");
        samples.push(q);
        let mut k = vec![0u8; 2048];
        k[..4].copy_from_slice(b"KDMV");
        k[28..36].copy_from_slice(&1u64.to_le_bytes());
        k[36..44].copy_from_slice(&u64::MAX.to_le_bytes());
        samples.push(k);
        let mut vdi = vec![0u8; 512];
        vdi[64..68].copy_from_slice(&0xBEDA_107Fu32.to_le_bytes());
        samples.push(vdi);
        let mut dmg = vec![0u8; 1024];
        dmg[512..516].copy_from_slice(b"koly");
        samples.push(dmg);
        let mut x = vec![0u8; 300 << 10];
        x[..8].copy_from_slice(b"vhdxfile");
        x[192 << 10..(192 << 10) + 4].copy_from_slice(b"regi");
        samples.push(x);
        samples.push(iso("SHORT", b"x"));
        let mut g = mbr_disk();
        g[512..520].copy_from_slice(b"EFI PART");
        samples.push(g);
        let mut fat = vec![0u8; 4096];
        fat[510] = 0x55;
        fat[511] = 0xAA;
        fat[82..87].copy_from_slice(b"FAT32");
        samples.push(fat);
        let d = folder("fuzz");
        let empty = d.join("empty");
        std::fs::write(&empty, b"").unwrap();
        let mut f = File::open(&empty).unwrap();
        let mut feed = |b: &[u8]| {
            let head = &b[..b.len().min(HEAD)];
            let tail = &b[b.len().saturating_sub(512)..];
            let _ = read(&mut f, head, tail, b.len() as u64);
        };
        let near = |n: usize| (0..=2048).chain(32700..32900).chain(n.saturating_sub(600)..=n);
        for s in &samples {
            for len in near(s.len()).filter(|&l| l <= s.len()) {
                feed(&s[..len]);
            }
            // Each field huge, in turn.
            for at in (0..s.len().min(4096)).step_by(4).chain((32768..32968).step_by(4)) {
                if at + 8 <= s.len() {
                    let mut c = s.clone();
                    c[at..at + 8].fill(0xFF);
                    feed(&c);
                }
            }
        }
        // An ISO cut right after its volume descriptor's name: the crash the review found.
        let mut short = vec![0u8; 32800];
        short[32768..32774].copy_from_slice(b"\x01CD001");
        assert_eq!(facts_of("short.iso", &short).map(|f| f.format), Some("ISO 9660"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn facts_are_read_once_per_change_of_the_file() {
        let d = folder("memo");
        let p = d.join("disk.img");
        std::fs::write(&p, mbr_disk()).unwrap();
        assert!(facts(&p).is_some());
        // Read again only when it changed: the cached facts come back without opening it.
        std::fs::write(&p, mbr_disk()).unwrap();
        let first = facts(&p);
        let mut perm = std::fs::metadata(&p).unwrap().permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            perm.set_mode(0o000);
            std::fs::set_permissions(&p, perm.clone()).unwrap();
            if File::open(&p).is_err() {
                assert_eq!(facts(&p), first, "from memory, not the file");
            }
            perm.set_mode(0o644);
            std::fs::set_permissions(&p, perm).unwrap();
        }
        #[cfg(not(unix))]
        let _ = (perm, first);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn lines_say_the_facts() {
        crate::i18n::set_language("en-GB");
        let f = Facts {
            format: "VHD",
            virtual_size: Some(2 << 20),
            on_disk: 1 << 20,
            variant: Some("fixed"),
            table: Some("MBR"),
            partitions: vec![Partition { kind: "FAT32".into(), start: 1 << 20, size: 1 << 20, name: String::new() }],
            ..Facts::default()
        };
        let l = lines(&f);
        assert_eq!(l[0].1, "VHD (fixed size)");
        assert!(l.iter().any(|(_, v)| v.contains("FAT32")), "{l:?}");
    }
}
