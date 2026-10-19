//! The container images of `[preview.images]`: what each one brings and costs, keeping the
//! pulled ones up to date (`[preview] image_updates`), and removing those no preview used for
//! 90 days (`[preview] image_prune`). The desktop app does both in the background; the terminal
//! app makes no container previews, so it only shows the settings.
//!
//! Only Coxswain's own images are ever touched: an update removes the copy it replaced by that
//! copy's id, never with `image prune`, which would take the user's other untagged images too.

use std::path::Path;
use std::process::Stdio;

use crate::config::PreviewConfig;
use crate::state::AppState;
use crate::t;

const DAY: u64 = 24 * 3600;
/// An image no preview used for this many days goes, when `image_prune` is on.
pub const UNUSED_DAYS: u64 = 90;

/// Megabytes to download and on disk of the default images, measured (amd64, 2026-10).
const SIZES: &[(&str, u32, u32)] = &[
    ("docker.io/texlive/texlive:latest", 2800, 5900),
    ("docker.io/texlive/texlive:latest-medium", 960, 2200),
    ("docker.io/plantuml/plantuml:latest", 141, 353),
    ("docker.io/pandoc/core:latest", 57, 246),
];

const TOOLS: [&str; 5] = ["latex", "libreoffice", "plantuml", "pandoc", "duckdb"];

/// Megabytes as "2.8 GB" or "141 MB".
fn shown(mb: u32) -> String {
    crate::i18n::digits(if mb >= 1000 {
        format!("{}.{} {}", mb / 1000, mb % 1000 / 100, t!("unit.GB"))
    } else {
        format!("{mb} {}", t!("unit.MB"))
    })
}

fn sizes(image: &str) -> Option<(u32, u32)> {
    let full = if image.starts_with(&format!("{}/", crate::settings::registry(image))) {
        image.to_string()
    } else {
        format!("docker.io/{image}")
    };
    SIZES.iter().find(|s| s.0 == full).map(|s| (s.1, s.2))
}

/// How much `image` downloads, "2.8 GB", when it is one of the default ones.
pub fn download(image: &str) -> Option<String> {
    sizes(image).map(|(d, _)| shown(d))
}

/// What the image of `tool` gives, its size, and what it costs: shown where it is pulled
/// (Settings → Previews) and in Disk use. None for a name under `[preview.images]` that is no
/// tool's.
pub fn about(tool: &str, image: &str) -> Option<String> {
    if !TOOLS.contains(&tool) {
        return None;
    }
    let size = match sizes(image) {
        Some((d, disk)) => t!("image.size", "download" => shown(d), "disk" => shown(disk)),
        None => t!("image.size_unknown"),
    };
    [
        crate::i18n::tr(&format!("image.what.{tool}"), &[]),
        size,
        crate::i18n::tr(&format!("image.cost.{tool}"), &[]),
    ]
    .join(" ")
    .into()
}

/// Days between two rounds of updates for `[preview] image_updates`; none when off.
pub fn every(updates: &str) -> Option<u64> {
    match updates {
        "weekly" => Some(7),
        "monthly" => Some(30),
        _ => None,
    }
}

/// Is a round of updates due at `now`?
pub fn update_due(cfg: &PreviewConfig, st: &AppState, now: u64) -> bool {
    cfg.container != "off"
        && every(&cfg.image_updates)
            .is_some_and(|d| now.saturating_sub(st.images_checked) >= d * DAY)
}

/// A preview just ran in `image`.
pub fn used(st: &mut AppState, image: &str, now: u64) {
    st.images_used.insert(image.to_string(), now);
}

/// Of `pulled`, the images no preview used for `UNUSED_DAYS`. One with no use recorded counts
/// as used now, so turning the option on never removes everything at once.
pub fn stale(st: &mut AppState, pulled: &[String], now: u64) -> Vec<String> {
    pulled
        .iter()
        .filter(|i| {
            let last = *st.images_used.entry(i.to_string()).or_insert(now);
            now.saturating_sub(last) >= UNUSED_DAYS * DAY
        })
        .cloned()
        .collect()
}

/// The configured images the runtime `rt` has, each once.
pub fn pulled(cfg: &PreviewConfig, rt: &Path) -> Vec<String> {
    let mut v: Vec<String> = cfg
        .images
        .values()
        .filter(|i| !i.is_empty())
        .cloned()
        .collect();
    v.sort();
    v.dedup();
    v.retain(|i| crate::tools::image_size(rt, i).is_some());
    v
}

/// The id of the copy of `image` the runtime has.
pub fn id(rt: &Path, image: &str) -> Option<String> {
    let out = crate::tools::command(rt)
        .args(["image", "inspect", "--format", "{{.Id}}", image])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty()).then_some(id)
}

/// `<rt> image rm what`, a name or an id; the runtime's last error line when it refuses.
pub fn remove(rt: &Path, what: &str) -> Result<(), String> {
    let out = crate::tools::command(rt)
        .args(["image", "rm", what])
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        return Ok(());
    }
    Err(String::from_utf8_lossy(&out.stderr)
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default()
        .to_string())
}

/// Update `image` with `pull`; when that brought a new copy, the one it replaced is removed by
/// its id at once. A copy that cannot go (a container still uses it, another name points at
/// it) stays, quietly.
pub fn update(
    rt: &Path,
    image: &str,
    pull: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let old = id(rt, image);
    pull()?;
    if let Some(old) = replaced(old, id(rt, image)) {
        let _ = remove(rt, &old);
    }
    Ok(())
}

/// A round of updates: every image in `images` is tried, each with `pull`. Done when there was
/// none or at least one pull worked; when all fail (offline) it stays due for the next hour,
/// and one image that always fails does not make the others pull every hour.
pub fn update_round(
    rt: &Path,
    images: &[String],
    pull: impl Fn(&str) -> Result<(), String>,
) -> bool {
    let ok: Vec<bool> = images
        .iter()
        .map(|i| update(rt, i, || pull(i)).is_ok())
        .collect();
    ok.is_empty() || ok.contains(&true)
}

/// The copy a pull replaced: the old id, only when a new copy with another id is there. A new
/// id that cannot be read keeps the old one, which may be the only copy.
fn replaced(old: Option<String>, new: Option<String>) -> Option<String> {
    old.filter(|o| new.is_some_and(|n| &n != o))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn images_updates_come_weekly_or_monthly_and_never_without_containers() {
        let mut cfg = PreviewConfig::default();
        let mut st = AppState::default();
        let now = 100 * DAY;
        assert!(!update_due(&cfg, &st, now), "off by default");
        cfg.image_updates = "weekly".into();
        assert!(update_due(&cfg, &st, now));
        st.images_checked = now - 6 * DAY;
        assert!(!update_due(&cfg, &st, now));
        st.images_checked = now - 7 * DAY;
        assert!(update_due(&cfg, &st, now));
        cfg.image_updates = "monthly".into();
        assert!(!update_due(&cfg, &st, now));
        st.images_checked = now - 30 * DAY;
        assert!(update_due(&cfg, &st, now));
        cfg.container = "off".into();
        assert!(!update_due(&cfg, &st, now));
    }

    #[test]
    fn images_unused_for_90_days_are_stale_and_one_never_seen_starts_now() {
        let mut st = AppState::default();
        let now = 1000 * DAY;
        used(&mut st, "a", now - 91 * DAY);
        used(&mut st, "b", now - 10 * DAY);
        let pulled = ["a", "b", "c"].map(String::from);
        assert_eq!(stale(&mut st, &pulled, now), ["a"]);
        assert_eq!(
            st.images_used.get("c"),
            Some(&now),
            "first seen: counted as used now"
        );
        assert_eq!(stale(&mut st, &pulled, now + 89 * DAY), ["a", "b"]);
        assert_eq!(stale(&mut st, &pulled, now + 90 * DAY), ["a", "b", "c"]);
    }

    #[test]
    fn images_update_removes_the_old_copy_only_when_a_new_one_is_there() {
        let s = |x: &str| Some(x.to_string());
        assert_eq!(replaced(s("old"), s("new")), s("old"));
        assert_eq!(replaced(s("old"), s("old")), None, "nothing new pulled");
        assert_eq!(
            replaced(s("old"), None),
            None,
            "the new id unread: keep the old"
        );
        assert_eq!(replaced(None, s("new")), None);
    }

    #[test]
    fn images_update_round_tries_every_image() {
        // No runtime there: no id is read, so nothing is removed.
        let rt = Path::new("/nonexistent/coxswain-test-runtime");
        let images = ["a", "bad", "c"].map(String::from);
        let tried = std::cell::RefCell::new(vec![]);
        let pull = |i: &str| {
            tried.borrow_mut().push(i.to_string());
            if i == "bad" { Err("no".into()) } else { Ok(()) }
        };
        assert!(
            update_round(rt, &images, pull),
            "one failing image ends the round"
        );
        assert_eq!(
            *tried.borrow(),
            images,
            "the one after a failure is tried too"
        );
        assert!(
            !update_round(rt, &images, |_| Err("offline".into())),
            "all failing: still due"
        );
        assert!(update_round(rt, &[], |_| Err("never".into())));
    }

    #[test]
    fn images_are_explained_with_their_sizes() {
        crate::i18n::set_language("en-GB");
        let about = about("latex", "docker.io/texlive/texlive:latest").unwrap();
        assert!(
            about.contains("2.8 GB to download, 5.9 GB on disk"),
            "{about}"
        );
        assert_eq!(
            download("texlive/texlive:latest-medium").as_deref(),
            Some("960 MB")
        );
        assert_eq!(download("ghcr.io/me/texlive:latest"), None);
        assert!(
            super::about("duckdb", "ghcr.io/me/duckdb")
                .unwrap()
                .contains(&t!("image.size_unknown"))
        );
        assert_eq!(super::about("other", "x"), None);
        for tool in TOOLS {
            assert!(
                !super::about(tool, "").unwrap().contains("image."),
                "{tool}: every text is there"
            );
        }
    }
}
