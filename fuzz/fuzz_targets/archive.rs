// Archives: listed, browsed and extracted. Nothing may land outside the folder made, and no
// link may come out.
#![no_main]
use libfuzzer_sys::fuzz_target;
use std::path::Path;
mod common;

const ENDINGS: &[&str] = &["zip", "tar", "tar.gz", "tar.bz2", "tar.xz", "tar.zst", "7z", "cbz", "mbox", "jar"];

fn walk(dir: &Path, root: &Path) {
    for e in std::fs::read_dir(dir).unwrap() {
        let e = e.unwrap();
        let t = e.file_type().unwrap();
        assert!(!t.is_symlink(), "a link came out: {:?}", e.path());
        let canon = e.path().canonicalize().unwrap();
        assert!(canon.starts_with(root), "outside: {canon:?}");
        if t.is_dir() {
            walk(&e.path(), root);
        }
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((dir, path, _)) = common::file(ENDINGS, data) else { return };
    use coxswain_core::archive;
    let _ = archive::list(&path, 1000);
    if let Ok((entries, _)) = archive::listing(&path, "") {
        for e in entries.iter().take(8) {
            let inner = path.join(&e.name);
            let _ = archive::peek(&inner);
        }
    }
    let out = dir.0.join("out");
    std::fs::create_dir(&out).unwrap();
    let root = out.canonicalize().unwrap();
    if let Ok(to) = archive::extract(&path, &out) {
        walk(&to, &root);
    }
    // Only the archive and the out folder are in the temp folder.
    for e in std::fs::read_dir(&dir.0).unwrap() {
        let n = e.unwrap().file_name();
        assert!(n == "out" || Path::new(&n) == path.file_name().unwrap(), "written beside: {n:?}");
    }
});
