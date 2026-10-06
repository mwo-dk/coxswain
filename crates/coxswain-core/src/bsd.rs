//! FreeBSD's own: the package a file belongs to (`pkg which`) and the files of a package as a
//! folder (`<file>/@package`), the boot environments (`bectl list`) and the jails (`jls`).
//! Read only: nothing here installs, activates or starts anything. Off FreeBSD, nothing is found.

use serde::Serialize;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::fs::Entry;

/// The path segment after a file that leads to the files of the package it belongs to.
pub const PACKAGE: &str = "@package";

/// What `program` with `args` prints, given a few seconds; `None` when it is missing or fails.
fn run(program: &str, args: &[&str]) -> Option<String> {
    let p = crate::tools::which(program).or_else(|| ["/usr/sbin", "/sbin", "/usr/bin"].iter().map(|d| Path::new(d).join(program)).find(|p| p.is_file()))?;
    let args: Vec<&std::ffi::OsStr> = args.iter().map(std::ffi::OsStr::new).collect();
    let mut c = crate::tools::command(&p);
    c.args(&args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null());
    let mut child = c.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut v = String::new();
        let _ = io::Read::read_to_string(&mut out, &mut v);
        v
    });
    let ok = crate::tools::wait(&mut child, Duration::from_secs(4)).ok()?.success();
    let text = reader.join().ok()?;
    ok.then_some(text)
}

// ---------------------------------------------------------------- packages

/// The installed package `path` belongs to, as `name-version`; `None` for a file of the base
/// system or of no package, and off FreeBSD.
pub fn package_of(path: &Path) -> Option<String> {
    if !cfg!(target_os = "freebsd") {
        return None;
    }
    let p = path.to_str()?;
    let name = run("pkg", &["which", "-q", p])?.trim().to_string();
    (!name.is_empty() && !name.contains(char::is_whitespace)).then_some(name)
}

/// `pkg info -q -l`'s text: the package's files.
pub fn parse_files(text: &str) -> Vec<PathBuf> {
    text.lines().map(str::trim).filter(|l| l.starts_with('/')).map(PathBuf::from).collect()
}

/// The file whose package `path` lists, when it is `<file>/@package` (and not a real entry).
pub fn split(path: &Path) -> Option<PathBuf> {
    if path.file_name()? != PACKAGE || std::fs::symlink_metadata(path).is_ok() {
        return None;
    }
    let file = path.parent()?;
    std::fs::symlink_metadata(file).is_ok().then(|| file.to_path_buf())
}

/// The path of the list of files of `file`'s package.
pub fn path(file: &Path) -> PathBuf {
    file.join(PACKAGE)
}

/// The files of the package `file` belongs to, as `fs::list` gives a folder: `..` back to the
/// file's folder, then each file by its full path, as it is on disk now.
pub fn list(file: &Path) -> io::Result<Vec<Entry>> {
    let pkg = package_of(file).ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, crate::t!("pkg.none", "name" => file.display())))?;
    let text = run("pkg", &["info", "-q", "-l", &pkg]).ok_or_else(|| io::Error::other(crate::t!("pkg.none", "name" => file.display())))?;
    let up = Entry { name: "..".into(), path: file.parent().unwrap_or(file).to_path_buf(), is_dir: true, ..Entry::default() };
    Ok(std::iter::once(up)
        .chain(parse_files(&text).into_iter().map(|p| {
            let m = std::fs::symlink_metadata(&p).ok();
            let secs = |t: io::Result<std::time::SystemTime>| t.ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            Entry {
                name: p.to_string_lossy().into_owned(),
                is_symlink: m.as_ref().is_some_and(|m| m.file_type().is_symlink()),
                size: m.as_ref().map_or(0, |m| m.len()),
                modified: m.as_ref().map_or(0, |m| secs(m.modified())),
                path: p,
                ..Entry::default()
            }
        }))
        .collect())
}

// ---------------------------------------------------------------- boot environments and jails

/// A boot environment: its name, whether it is the one running and the one booted next, and
/// where it is mounted (the running one at `/`), if it is.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct BootEnv {
    pub name: String,
    pub active: bool,
    pub on_reboot: bool,
    pub mountpoint: Option<PathBuf>,
}

/// `bectl list -H`'s text: name, active (`N` now, `R` on reboot, `T` once), mountpoint (`-`
/// when not mounted), space, created; separated by tabs.
pub fn parse_bectl(text: &str) -> Vec<BootEnv> {
    text.lines()
        .filter_map(|l| {
            let mut f = l.split('\t');
            let (name, active, mount) = (f.next()?.trim(), f.next()?.trim(), f.next()?.trim());
            (!name.is_empty()).then(|| BootEnv {
                name: name.to_string(),
                active: active.contains('N'),
                on_reboot: active.contains('R') || active.contains('T'),
                mountpoint: (mount != "-" && mount.starts_with('/')).then(|| PathBuf::from(mount)),
            })
        })
        .collect()
}

/// The boot environments, on FreeBSD with a ZFS root; empty otherwise.
pub fn boot_environments() -> Vec<BootEnv> {
    if !cfg!(target_os = "freebsd") {
        return vec![];
    }
    run("bectl", &["list", "-H"]).map(|t| parse_bectl(&t)).unwrap_or_default()
}

/// A running jail: its id, name, root folder and host name.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Jail {
    pub jid: u32,
    pub name: String,
    pub path: PathBuf,
    pub hostname: String,
}

/// `jls -h jid name path host.hostname`'s text: a heading line, then one jail a line. The path
/// is everything between the name and the host name, so a space in it stays.
pub fn parse_jls(text: &str) -> Vec<Jail> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let (jid, rest) = l.trim().split_once(' ')?;
            let (name, rest) = rest.split_once(' ')?;
            let (path, hostname) = rest.rsplit_once(' ')?;
            Some(Jail { jid: jid.parse().ok()?, name: name.to_string(), path: PathBuf::from(path), hostname: hostname.to_string() })
        })
        .collect()
}

/// The running jails, on FreeBSD; empty otherwise.
pub fn jails() -> Vec<Jail> {
    if !cfg!(target_os = "freebsd") {
        return vec![];
    }
    run("jls", &["-h", "jid", "name", "path", "host.hostname"]).map(|t| parse_jls(&t)).unwrap_or_default()
}

/// A place to go to in the sidebar and the terminal app's go-to list: a boot environment or a
/// jail, the folder it opens (none: not mounted, or not readable by you) and a line about it.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Place {
    /// `boot` or `jail`.
    pub kind: &'static str,
    pub name: String,
    pub path: Option<PathBuf>,
    pub note: String,
}

/// The boot environments and the jails, as places.
pub fn places() -> Vec<Place> {
    let mut v: Vec<Place> = boot_environments()
        .into_iter()
        .map(|b| {
            let note = match (b.active, b.on_reboot) {
                (true, true) => crate::t!("bsd.be_now_and_next"),
                (true, false) => crate::t!("bsd.be_now"),
                (false, true) => crate::t!("bsd.be_next"),
                _ if b.mountpoint.is_none() => crate::t!("bsd.be_unmounted"),
                _ => String::new(),
            };
            Place { kind: "boot", name: b.name, path: b.mountpoint.filter(|p| p.is_dir()), note }
        })
        .collect();
    v.extend(jails().into_iter().map(|j| Place {
        kind: "jail",
        note: crate::t!("bsd.jail_note", "jid" => j.jid, "host" => j.hostname),
        name: j.name,
        // A jail's root that you cannot read is listed, not opened.
        path: std::fs::read_dir(&j.path).is_ok().then_some(j.path),
    }));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bsd_package_files_are_parsed() {
        // `pkg info -q -l git`, recorded on FreeBSD 14.5.
        let v = parse_files("\t/usr/local/bin/git\n\t/usr/local/bin/git-shell\n\n/usr/local/share/man/man1/git.1.gz\n");
        assert_eq!(v, [Path::new("/usr/local/bin/git"), Path::new("/usr/local/bin/git-shell"), Path::new("/usr/local/share/man/man1/git.1.gz")]);
        assert!(parse_files("git-2.56.0:\n").is_empty());
    }

    #[test]
    fn bsd_package_path_is_virtual() {
        let f = std::env::temp_dir().join(format!("coxswain-pkg-{}", std::process::id()));
        std::fs::write(&f, "x").unwrap();
        assert_eq!(split(&path(&f)), Some(f.clone()));
        assert_eq!(split(&f.with_extension("gone").join(PACKAGE)), None);
        // Nothing goes into it.
        assert_eq!(crate::fs::mkdir(&path(&f).join("x")).unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        std::fs::remove_file(&f).unwrap();
    }

    #[test]
    fn bsd_boot_environments_are_parsed() {
        // `bectl list -H` on a FreeBSD 14 machine with three boot environments.
        let text = "default\tNR\t/\t2.1G\t2024-05-01 10:12\n14.1-backup\t-\t-\t312M\t2024-06-02 09:00\nupgrade\t-\t/tmp/be_mount.Xa1\t1.1G\t2024-07-01 12:00\nnext\tT\t-\t8K\t2024-07-02 12:00\n";
        let v = parse_bectl(text);
        assert_eq!(v.len(), 4);
        assert_eq!(v[0], BootEnv { name: "default".into(), active: true, on_reboot: true, mountpoint: Some("/".into()) });
        assert_eq!((v[1].active, v[1].on_reboot, v[1].mountpoint.clone()), (false, false, None));
        assert_eq!(v[2].mountpoint.as_deref(), Some(Path::new("/tmp/be_mount.Xa1")));
        assert!(v[3].on_reboot && !v[3].active);
    }

    #[test]
    fn bsd_jails_are_parsed() {
        // `jls -h jid name path host.hostname`, recorded on FreeBSD 14.5.
        let text = "jid name path host.hostname\n1 web /jails/web web.example.org\n2 db / db\n3 odd /jails/with space odd.example\n";
        let v = parse_jls(text);
        assert_eq!(v.len(), 3);
        assert_eq!(v[0], Jail { jid: 1, name: "web".into(), path: "/jails/web".into(), hostname: "web.example.org".into() });
        assert_eq!(v[2].path, Path::new("/jails/with space"));
        assert!(parse_jls("jid name path host.hostname\n").is_empty());
    }
}
