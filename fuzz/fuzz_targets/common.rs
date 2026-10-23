// The fuzz input written to a file of its own with an ending the first byte picks, so the
// readers that take a path see it as that kind.
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

static N: AtomicU64 = AtomicU64::new(0);

pub fn file(endings: &'static [&'static str], data: &[u8]) -> Option<(tempdir::Dir, PathBuf, &'static str)> {
    let (&pick, rest) = data.split_first()?;
    // FUZZ_ENDINGS=psd,pcx: only those, for a run that dwells on one reader.
    static ONLY: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    let only = ONLY.get_or_init(|| {
        let want = std::env::var("FUZZ_ENDINGS").unwrap_or_default();
        endings.iter().copied().filter(|e| want.split(',').any(|w| w == *e)).collect()
    });
    let ending = if only.is_empty() { endings[pick as usize % endings.len()] } else { only[pick as usize % only.len()] };
    let dir = tempdir::Dir::new();
    let path = dir.0.join(format!("f{}.{ending}", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&path, rest).ok()?;
    Some((dir, path, ending))
}

pub mod tempdir {
    use super::*;
    pub struct Dir(pub PathBuf);
    impl Dir {
        pub fn new() -> Dir {
            let base = std::env::var_os("FUZZ_TMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
            let d = base.join(format!("fz{}-{}", std::process::id(), N.fetch_add(1, Ordering::Relaxed)));
            std::fs::create_dir_all(&d).unwrap();
            Dir(d)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
