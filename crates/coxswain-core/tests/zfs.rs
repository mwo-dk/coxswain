//! ZFS from end to end, on a pool of its own in a file: a dataset, two snapshots, the list of
//! snapshots, a file as a snapshot has it, its diff against now, a copy restored out of it, and
//! the refusals to write there. Needs root and ZFS, so it runs only when COXSWAIN_ZFS_E2E=1
//! (the FreeBSD and illumos jobs in CI set it); otherwise it passes at once.

use coxswain_core::{flags, fs as cfs, zfs};
use std::path::{Path, PathBuf};
use std::process::Command;

fn run(cmd: &str, args: &[&str]) {
    let s = Command::new(cmd).args(args).status().unwrap_or_else(|e| panic!("{cmd}: {e}"));
    assert!(s.success(), "{cmd} {args:?}: {s}");
}

/// The pool, destroyed again however the test ends.
struct Pool {
    name: String,
    file: PathBuf,
}

impl Drop for Pool {
    fn drop(&mut self) {
        let _ = Command::new("zpool").args(["destroy", "-f", &self.name]).status();
        let _ = std::fs::remove_file(&self.file);
    }
}

#[test]
fn zfs_snapshots_end_to_end() {
    if std::env::var("COXSWAIN_ZFS_E2E").as_deref() != Ok("1") {
        return;
    }
    let id = std::process::id();
    let base = std::env::temp_dir().join(format!("coxswain-zfs-e2e-{id}"));
    std::fs::create_dir_all(&base).unwrap();
    let pool = Pool { name: format!("coxswain_e2e_{id}"), file: base.join("pool.img") };
    if cfg!(any(target_os = "illumos", target_os = "solaris")) {
        run("mkfile", &["128m", pool.file.to_str().unwrap()]);
    } else {
        run("truncate", &["-s", "128M", pool.file.to_str().unwrap()]);
    }
    let root = base.join("mnt");
    run("zpool", &["create", "-m", root.to_str().unwrap(), &pool.name, pool.file.to_str().unwrap()]);
    let ds = format!("{}/home", pool.name);
    run("zfs", &["create", "-o", "compression=lz4", "-o", "quota=64M", &ds]);
    let home = root.join("home");
    std::fs::create_dir(home.join("docs")).unwrap();
    std::fs::write(home.join("docs/note.md"), "one\ntwo\n").unwrap();
    run("zfs", &["snapshot", &format!("{ds}@first")]);
    std::thread::sleep(std::time::Duration::from_millis(1100));
    std::fs::write(home.join("docs/note.md"), "one\nthree\n").unwrap();
    run("zfs", &["snapshot", &format!("{ds}@second")]);

    // The dataset of a folder, and its facts.
    let docs = home.join("docs");
    assert_eq!(zfs::dataset_of(&docs).map(|d| d.name), Some(ds.clone()));
    let f = zfs::facts(&docs).unwrap();
    assert_eq!((f.compression.as_str(), f.quota), ("lz4", 64 << 20));
    assert!(f.available > 0 && !f.compressratio.is_empty());

    // The list: newest first, each leading to the folder in that snapshot.
    let list = cfs::list(&zfs::path(&docs), true).unwrap();
    let names: Vec<&str> = list.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, ["..", "second", "first"]);
    assert_eq!(list[0].path, docs);
    let first = &list[2].path;
    assert_eq!(first, &home.join(".zfs/snapshot/first/docs"));
    assert!(list[2].referenced > 0);

    // Inside a snapshot: the file as it was, `..` at the top back to the list.
    let in_first = cfs::list(first, true).unwrap();
    assert!(in_first.iter().any(|e| e.name == "note.md"));
    let top = cfs::list(&home.join(".zfs/snapshot/first"), true).unwrap();
    assert_eq!(top[0].path, zfs::path(&home));
    let old = first.join("note.md");
    assert_eq!(std::fs::read_to_string(&old).unwrap(), "one\ntwo\n");
    let at = zfs::at(first).unwrap();
    assert_eq!((at.dataset.as_str(), at.snapshot.as_deref()), (ds.as_str(), Some("first")));

    // What changed since, and a copy restored.
    let d = zfs::diff(&old).unwrap();
    assert!(d.contains("-two") && d.contains("+three"), "{d}");
    let restore = base.join("restore");
    std::fs::create_dir(&restore).unwrap();
    let got = cfs::copy(&old, &restore).unwrap();
    assert_eq!(std::fs::read_to_string(got).unwrap(), "one\ntwo\n");

    // Read-only: nothing is made, moved or deleted there.
    for e in [cfs::mkdir(&first.join("new")), cfs::delete(&old), cfs::rename(&old, &restore).map(drop), cfs::copy(&restore, first).map(drop)] {
        assert_eq!(e.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    }
    assert!(zfs::is_read_only(&zfs::path(&docs)));

    // File flags, where the system has them.
    if cfg!(any(target_os = "freebsd", target_os = "macos")) {
        let live = home.join("docs/note.md");
        // ZFS keeps nodump and hidden for users; uchg it does not keep.
        flags::set_user(&live, &["nodump", "hidden"]).unwrap();
        let f = flags::read(&live).unwrap();
        assert!(f.set.contains(&"nodump") && f.set.contains(&"hidden"), "{:?}", f.set);
        assert_eq!(flags::set_user(&live, &["uchg"]).unwrap_err().kind(), std::io::ErrorKind::Unsupported);
        flags::set_user(&live, &[]).unwrap();
        assert!(!flags::read(&live).unwrap().set.contains(&"nodump"));
    }
    drop(pool);
    let _ = std::fs::remove_dir_all(Path::new(&base));
}
