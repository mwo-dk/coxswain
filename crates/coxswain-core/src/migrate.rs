//! Coxswain was called Bosum until 2.0. On the first start under the new name, the settings,
//! scripts, tabs, tags and notes kept under the old one are copied over. The old folders are
//! left as they are; caches are not copied, they rebuild.

use std::io;
use std::path::Path;

const OLD: &str = "bosum";
const NEW: &str = "coxswain";

/// Copy what Bosum kept, where Coxswain keeps nothing yet. Failures are reported, not fatal:
/// starting with defaults beats not starting.
pub fn adopt_bosum() {
    for base in [dirs::config_dir(), dirs::data_dir()].into_iter().flatten() {
        if let Err(e) = adopt(&base.join(OLD), &base.join(NEW)) {
            eprintln!("coxswain: could not copy {}: {e}", base.join(OLD).display());
        }
    }
}

/// Copy the folder `old` to `new` unless `new` exists. `true` when it copied.
pub fn adopt(old: &Path, new: &Path) -> io::Result<bool> {
    if new.exists() || !old.is_dir() {
        return Ok(false);
    }
    // Into a folder next to it first, so a copy cut short is never taken for the real thing.
    let part = new.with_extension("part");
    let _ = std::fs::remove_dir_all(&part);
    copy_tree(old, &part)?;
    std::fs::rename(&part, new)?;
    Ok(true)
}

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let (src, dst) = (entry.path(), to.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_copies_once_and_leaves_the_old_folder() {
        let d = std::env::temp_dir().join(format!("coxswain-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("bosum/scripts")).unwrap();
        std::fs::write(d.join("bosum/config.toml"), "theme = \"nc\"\n").unwrap();
        std::fs::write(d.join("bosum/scripts/build.sh"), "cargo build\n").unwrap();

        assert!(!adopt(&d.join("nothing"), &d.join("coxswain")).unwrap(), "nothing to copy");
        assert!(adopt(&d.join("bosum"), &d.join("coxswain")).unwrap());
        assert_eq!(std::fs::read_to_string(d.join("coxswain/config.toml")).unwrap(), "theme = \"nc\"\n");
        assert_eq!(std::fs::read_to_string(d.join("coxswain/scripts/build.sh")).unwrap(), "cargo build\n");
        assert!(d.join("bosum/config.toml").is_file(), "the old folder stays");

        std::fs::write(d.join("coxswain/config.toml"), "theme = \"cyber\"\n").unwrap();
        assert!(!adopt(&d.join("bosum"), &d.join("coxswain")).unwrap(), "never over what is there");
        assert_eq!(std::fs::read_to_string(d.join("coxswain/config.toml")).unwrap(), "theme = \"cyber\"\n");
        std::fs::remove_dir_all(d).unwrap();
    }
}
