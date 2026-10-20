//! The user's folders: home, config, data and cache. Everything that finds them goes through
//! here, never through `dirs` or `std::env::home_dir` itself, so that tests (this crate's, and
//! the apps' with the `test-home` feature their dev-dependency turns on) get a home of their
//! own in the temporary folder and never read or write the developer's own config and data.

#[cfg(not(any(test, feature = "test-home")))]
pub use ::dirs::{cache_dir, config_dir, data_dir, data_local_dir, preference_dir};
#[cfg(not(any(test, feature = "test-home")))]
pub use std::env::home_dir;

#[cfg(any(test, feature = "test-home"))]
mod fake {
    use std::path::PathBuf;

    /// `<temp>/coxswain-test-home/<test binary>`: one per test binary, so runs leave one folder
    /// each, shared by the processes a test starts of its own binary (their locks meet there).
    pub fn home_dir() -> Option<PathBuf> {
        static HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        let home = HOME.get_or_init(|| {
            let exe = std::env::current_exe().ok();
            let name = exe.as_deref().and_then(|e| e.file_stem()).map_or_else(
                || format!("pid-{}", std::process::id()),
                |s| s.to_string_lossy().into_owned(),
            );
            let home = std::env::temp_dir().join("coxswain-test-home").join(name);
            let _ = std::fs::create_dir_all(&home);
            home
        });
        Some(home.clone())
    }

    // ponytail: one layout (XDG's) on every system; give each its own when a test needs it.
    fn under(rel: &str) -> Option<PathBuf> {
        Some(home_dir()?.join(rel))
    }
    pub fn config_dir() -> Option<PathBuf> {
        under(".config")
    }
    pub fn preference_dir() -> Option<PathBuf> {
        under(".config")
    }
    pub fn data_dir() -> Option<PathBuf> {
        under(".local/share")
    }
    pub fn data_local_dir() -> Option<PathBuf> {
        under(".local/share")
    }
    pub fn cache_dir() -> Option<PathBuf> {
        under(".cache")
    }
}
#[cfg(any(test, feature = "test-home"))]
pub use fake::*;

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    /// Every place Coxswain keeps or reads the user's things lies in the temporary folder in
    /// tests, none under the developer's own home.
    #[test]
    fn dirs_tests_never_reach_the_real_home() {
        let tmp = std::env::temp_dir();
        let mut all: Vec<(&str, Option<PathBuf>)> = vec![
            ("home", super::home_dir()),
            ("config dir", super::config_dir()),
            ("data dir", super::data_dir()),
            ("data local dir", super::data_local_dir()),
            ("cache dir", super::cache_dir()),
            ("preference dir", super::preference_dir()),
        ];
        // The search store (Find duplicates opens it too), the index, models and previews.
        all.extend(crate::config::Config::paths());
        for sub in ["peek", "provenance"] {
            all.push((sub, crate::archive::run_folder(sub, false).ok()));
        }
        #[cfg(unix)]
        all.push(("trash", crate::xdg_trash::dir()));
        for (what, path) in all {
            let path = path.unwrap_or_else(|| panic!("{what}: none"));
            assert!(path.starts_with(&tmp), "{what}: {}", path.display());
        }
        let profile = crate::extract::installed::libreoffice_profile("test-home-profile");
        let tmp_url = crate::extract::installed::file_url(&tmp.to_string_lossy());
        assert!(
            profile.as_ref().is_some_and(|p| p.starts_with(&tmp_url)),
            "{profile:?}"
        );
    }
}
