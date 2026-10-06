//! File flags: chflags(1) on FreeBSD and macOS, shown by the names `ls -lo` uses; on Linux the
//! attributes `lsattr` shows that matter here (immutable, append-only, no dump), read only. The
//! user flags (uchg, uappnd, nodump, hidden) can be set by the file's owner; the system flags
//! (schg, sappnd …) only by root, and not at all at securelevel 1 and above.

use serde::Serialize;
use std::io;
use std::path::Path;

/// A flag: its bit, its name as `ls -lo` prints it, and whether the owner may set it.
type Flag = (u32, &'static str, bool);

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const MACOS: &[Flag] = &[
    (0x1, "nodump", true),
    (0x2, "uchg", true),
    (0x4, "uappnd", true),
    (0x8, "opaque", false),
    (0x20, "compressed", false),
    (0x40, "tracked", false),
    (0x8000, "hidden", true),
    (0x10000, "arch", false),
    (0x20000, "schg", false),
    (0x40000, "sappnd", false),
    (0x80000, "restricted", false),
    (0x100000, "sunlnk", false),
];

#[cfg_attr(not(target_os = "freebsd"), allow(dead_code))]
const FREEBSD: &[Flag] = &[
    (0x1, "nodump", true),
    (0x2, "uchg", true),
    (0x4, "uappnd", true),
    (0x8, "opaque", false),
    (0x10, "uunlnk", false),
    (0x80, "system", false),
    (0x100, "sparse", false),
    (0x200, "offline", false),
    (0x400, "reparse", false),
    (0x800, "uarch", false),
    (0x1000, "rdonly", false),
    (0x8000, "hidden", true),
    (0x10000, "arch", false),
    (0x20000, "schg", false),
    (0x40000, "sappnd", false),
    (0x100000, "sunlnk", false),
    (0x200000, "snapshot", false),
];

/// The system flags start here: only root sets them.
const SYSTEM: u32 = 0x10000;

/// A file's flags: those set, the ones its owner may change (with whether each is set), and
/// whether you may change them (you own it, on a system that has them).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Flags {
    pub set: Vec<&'static str>,
    pub user: Vec<(&'static str, bool)>,
    pub can_set: bool,
    /// A system flag is set: say what that means.
    pub system: bool,
}

#[cfg_attr(not(any(target_os = "freebsd", target_os = "macos")), allow(dead_code))]
fn table() -> &'static [Flag] {
    if cfg!(target_os = "macos") { MACOS } else { FREEBSD }
}

/// The flags in `bits`, by `table`'s names.
pub fn from_bits(bits: u32, table: &[Flag], owner: bool) -> Flags {
    Flags {
        set: table.iter().filter(|(b, ..)| bits & b != 0).map(|(_, n, _)| *n).collect(),
        user: table.iter().filter(|(.., u)| *u).map(|(b, n, _)| (*n, bits & b != 0)).collect(),
        can_set: owner,
        system: bits >= SYSTEM,
    }
}

/// `bits` with the user flags named in `on` set and the other user flags cleared; the rest
/// (system flags, uarch …) as they were.
pub fn with_user(bits: u32, on: &[&str], table: &[Flag]) -> io::Result<u32> {
    if let Some(bad) = on.iter().find(|n| !table.iter().any(|(_, name, u)| *u && name == *n)) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, crate::t!("flags.unknown", "name" => bad)));
    }
    let user: u32 = table.iter().filter(|(.., u)| *u).map(|(b, ..)| b).sum();
    let chosen: u32 = table.iter().filter(|(_, n, _)| on.contains(n)).map(|(b, ..)| b).sum();
    Ok(bits & !user | chosen)
}

/// The attributes of one `lsattr -d` line that are flags here: `i` immutable, `a` append-only,
/// `d` no dump.
pub fn parse_lsattr(line: &str) -> Vec<&'static str> {
    let attrs = line.split_whitespace().next().unwrap_or_default();
    [('i', "immutable"), ('a', "append-only"), ('d', "nodump")].into_iter().filter(|(c, _)| attrs.contains(*c)).map(|(_, n)| n).collect()
}

/// The flags of `path` (not followed when it is a link); `None` where the system has none.
pub fn read(path: &Path) -> Option<Flags> {
    #[cfg(any(target_os = "freebsd", target_os = "macos"))]
    {
        let m = std::fs::symlink_metadata(path).ok()?;
        #[cfg(target_os = "freebsd")]
        let bits = std::os::freebsd::fs::MetadataExt::st_flags(&m);
        #[cfg(target_os = "macos")]
        let bits = std::os::macos::fs::MetadataExt::st_flags(&m);
        // SAFETY: geteuid has no preconditions.
        let me = unsafe { libc::geteuid() };
        let owner = std::os::unix::fs::MetadataExt::uid(&m) == me || me == 0;
        let mut f = from_bits(bits, table(), owner);
        // ZFS on FreeBSD keeps no uchg or uappnd (only root's schg and sappnd): not offered.
        if cfg!(target_os = "freebsd") && crate::zfs::dataset_of(path).is_some() {
            f.user.retain(|(n, _)| !matches!(*n, "uchg" | "uappnd"));
        }
        Some(f)
    }
    #[cfg(target_os = "linux")]
    {
        // lsattr knows the file systems that have these (ext4, XFS, Btrfs …); on others it fails.
        let lsattr = crate::tools::which("lsattr").or_else(|| ["/usr/bin/lsattr", "/bin/lsattr", "/sbin/lsattr"].iter().map(std::path::PathBuf::from).find(|p| p.is_file()))?;
        let out = crate::tools::output(&lsattr, &["-d".as_ref(), path.as_os_str()], std::time::Duration::from_secs(2), 64 << 10)?;
        Some(Flags { set: parse_lsattr(&String::from_utf8_lossy(&out)), ..Flags::default() })
    }
    #[cfg(not(any(target_os = "freebsd", target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        None
    }
}

/// Set the user flags named in `on` on `path` and clear the others; system flags stay. As
/// chflags(1) does, a link itself is not followed.
pub fn set_user(path: &Path, on: &[&str]) -> io::Result<()> {
    #[cfg(any(target_os = "freebsd", target_os = "macos"))]
    {
        let m = std::fs::symlink_metadata(path)?;
        #[cfg(target_os = "freebsd")]
        let bits = std::os::freebsd::fs::MetadataExt::st_flags(&m);
        #[cfg(target_os = "macos")]
        let bits = std::os::macos::fs::MetadataExt::st_flags(&m);
        let new = with_user(bits, on, table())?;
        let c = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).map_err(io::Error::other)?;
        // SAFETY: `c` is a NUL-terminated path.
        if unsafe { libc::lchflags(c.as_ptr(), new as _) } != 0 {
            let e = io::Error::last_os_error();
            // ZFS on FreeBSD keeps nodump and hidden for users, but no uchg or uappnd.
            if e.raw_os_error() == Some(libc::EOPNOTSUPP) {
                return Err(io::Error::new(io::ErrorKind::Unsupported, crate::t!("flags.unsupported")));
            }
            return Err(e);
        }
        Ok(())
    }
    #[cfg(not(any(target_os = "freebsd", target_os = "macos")))]
    {
        let _ = (path, on);
        Err(io::Error::new(io::ErrorKind::Unsupported, crate::t!("flags.not_here")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_are_named_as_ls_names_them() {
        // `stat -f %Xf` of a file with `chflags uchg,nodump`, on FreeBSD 14.5: 3.
        let f = from_bits(0x3, FREEBSD, true);
        assert_eq!(f.set, ["nodump", "uchg"]);
        assert_eq!(f.user, [("nodump", true), ("uchg", true), ("uappnd", false), ("hidden", false)]);
        assert!(!f.system);
        // /var/empty: schg. A file on ZFS: uarch.
        let f = from_bits(0x20000, FREEBSD, false);
        assert_eq!((f.set.as_slice(), f.system, f.can_set), (["schg"].as_slice(), true, false));
        assert_eq!(from_bits(0x800, FREEBSD, true).set, ["uarch"]);
        assert_eq!(from_bits(0x8000, MACOS, true).set, ["hidden"]);
    }

    #[test]
    fn flags_set_only_the_user_ones() {
        // schg and uarch stay; nodump goes, uappnd comes.
        assert_eq!(with_user(0x20000 | 0x800 | 0x1, &["uappnd"], FREEBSD).unwrap(), 0x20000 | 0x800 | 0x4);
        assert_eq!(with_user(0x2, &[], FREEBSD).unwrap(), 0);
        // A system flag cannot be asked for.
        assert!(with_user(0, &["schg"], FREEBSD).is_err());
        assert!(with_user(0, &["uhidden"], FREEBSD).is_err());
        assert_eq!(with_user(0, &["hidden"], MACOS).unwrap(), 0x8000);
    }

    #[test]
    fn flags_from_lsattr() {
        assert_eq!(parse_lsattr("----i---------e------- /etc/resolv.conf"), ["immutable"]);
        assert_eq!(parse_lsattr("-----a-d------e------- log"), ["append-only", "nodump"]);
        assert!(parse_lsattr("--------------e------- x").is_empty());
    }
}
