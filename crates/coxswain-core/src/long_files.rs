//! What search keeps of a long file: *Sample* (the default: the first `extract::MAX_TEXT` of
//! its text for words, `meaning::PASSAGES` passages spread over all of it for meaning) or
//! *Everything* (all its text, up to `extract::EVERYTHING_TEXT`, and a passage for every part
//! of it). Chosen for every folder in Settings → Meaning (`[search] long_files`) or for one
//! folder from the action menu (`long_files_everything`, `long_files_sample`), the deepest
//! folder listed deciding. Before a change is saved its cost is said: the long files it
//! touches, the room on the disk and the time (`warning`). The store does the work
//! (`store::Store::long_files_changed`); *Sample* is a part of *Everything*, so going back
//! reads nothing again: the extra vectors and text go and the store gives the room back.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

use crate::config::SearchConfig;
use crate::t;

/// Which files are kept whole: every one unless a folder says otherwise (`everything`), and the
/// folders read whole and sampled.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rule {
    pub everything: bool,
    pub whole: Vec<PathBuf>,
    pub sample: Vec<PathBuf>,
}

impl Rule {
    pub fn of(s: &SearchConfig) -> Rule {
        Rule {
            everything: s.long_files == "everything",
            whole: s.long_files_everything.clone(),
            sample: s.long_files_sample.clone(),
        }
    }

    /// Whether the file (or folder) at `path` is kept whole: the deepest folder listed above it
    /// decides, *Sample* when one is in both lists, the default when none is.
    pub fn whole(&self, path: &Path) -> bool {
        let deepest = |l: &[PathBuf]| {
            l.iter()
                .filter(|d| !d.as_os_str().is_empty() && path.starts_with(d))
                .map(|d| d.components().count())
                .max()
        };
        match (deepest(&self.whole), deepest(&self.sample)) {
            (Some(a), Some(b)) => a > b,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => self.everything,
        }
    }

    /// Whether any file at all is kept whole.
    pub fn any(&self) -> bool {
        self.everything || !self.whole.is_empty()
    }
}

/// What a change of the rule costs, as the store works it out (`Store::long_files_cost`):
/// the long files to be kept whole, the passages they get besides those they have, the bytes
/// that adds to the store and the bytes read again; and the files to be sampled again, with
/// the bytes that frees.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    pub files: usize,
    pub passages: usize,
    pub bytes: u64,
    pub read: u64,
    pub back: usize,
    pub freed: u64,
}

/// About how fast text is read, tidied and put into search by words, in bytes a second.
const READ_SPEED: f64 = 20e6;

/// What a passage takes to get its vector here, in milliseconds: as the helper measured it
/// (and kept), else the speed test of the engine `cfg` names; 0 when neither can be had.
fn ms_per_passage(cfg: &SearchConfig, measured: usize) -> f64 {
    if measured > 0 {
        return measured as f64;
    }
    if !cfg.meaning {
        return 0.0;
    }
    crate::meaning::bench(cfg)
        .map(|(_, took)| took.as_secs_f64() * 1000.0 / crate::meaning::BENCH_PASSAGES as f64)
        .unwrap_or(0.0)
}

/// The cost in words: what *Everything* adds (the long files, the room, the time) and what
/// going back to *Sample* frees; that nothing changes now when no long file is touched.
pub fn warning(cost: &Cost, cfg: &SearchConfig, measured: usize) -> String {
    let mut parts = vec![];
    if cost.files > 0 {
        let secs = cost.passages as f64 * ms_per_passage(cfg, measured) / 1000.0
            + cost.read as f64 / READ_SPEED;
        parts.push(t!("long.more",
            "files" => crate::settings::count(cost.files),
            "size" => crate::settings::human(cost.bytes),
            "time" => crate::meaning::about(secs)));
    }
    if cost.back > 0 {
        parts.push(t!("long.back",
            "files" => crate::settings::count(cost.back),
            "size" => crate::settings::human(cost.freed)));
    }
    if parts.is_empty() {
        t!("long.none")
    } else {
        parts.join(" ")
    }
}

/// What changing the search settings from `old` to `new` costs, said before it is saved; None
/// when what is kept of long files stays. `cost` asks the store (the helper) about a rule.
pub fn change_notice(
    old: &SearchConfig,
    new: &SearchConfig,
    measured: usize,
    cost: impl Fn(&Rule) -> Option<Cost>,
) -> Option<String> {
    let rule = Rule::of(new);
    if Rule::of(old) == rule {
        return None;
    }
    Some(match cost(&rule) {
        Some(c) => warning(&c, new, measured),
        None => t!("long.unknown"),
    })
}

/// A choice for one folder in the action menu: its label (with what it costs), whether it is
/// the one in force, and the settings it writes.
#[derive(Clone, Debug, Serialize)]
pub struct Choice {
    pub label: String,
    pub current: bool,
    pub changes: Map<String, Value>,
}

/// The settings that make `dir` *Everything* (`Some(true)`), *Sample* (`Some(false)`), or
/// follow the folders above it (`None`): `dir` taken out of both lists, then put in one.
pub fn changes_for(s: &SearchConfig, dir: &Path, whole: Option<bool>) -> Map<String, Value> {
    let list = |l: &[PathBuf], put: bool| -> Value {
        let mut v: Vec<Value> = l
            .iter()
            .filter(|d| d.as_path() != dir)
            .map(|d| Value::String(d.to_string_lossy().into_owned()))
            .collect();
        if put {
            v.push(Value::String(dir.to_string_lossy().into_owned()));
        }
        Value::Array(v)
    };
    let mut m = Map::new();
    m.insert("long_files_everything".into(), list(&s.long_files_everything, whole == Some(true)));
    m.insert("long_files_sample".into(), list(&s.long_files_sample, whole == Some(false)));
    m
}

/// `s` with `changes` (from `changes_for`) made.
fn with(s: &SearchConfig, changes: &Map<String, Value>) -> SearchConfig {
    let paths = |k: &str| -> Vec<PathBuf> {
        changes[k]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(PathBuf::from)).collect())
            .unwrap_or_default()
    };
    SearchConfig {
        long_files_everything: paths("long_files_everything"),
        long_files_sample: paths("long_files_sample"),
        ..s.clone()
    }
}

/// The action menu's *Long files* for the folder `dir`: *Everything here*, *Sample here* and
/// *Like the folders above*, each with what it would cost (`cost` asks the store), the one in
/// force marked.
pub fn choices(
    s: &SearchConfig,
    dir: &Path,
    measured: usize,
    cost: impl Fn(&Rule) -> Option<Cost>,
) -> Vec<Choice> {
    let listed = |l: &[PathBuf]| l.iter().any(|d| d == dir);
    let inherit = with(s, &changes_for(s, dir, None));
    let above = if Rule::of(&inherit).whole(dir) {
        t!("settings.long_files.everything")
    } else {
        t!("settings.long_files.sample")
    };
    [
        (Some(true), t!("long.everything_here"), listed(&s.long_files_everything)),
        (Some(false), t!("long.sample_here"), listed(&s.long_files_sample)),
        (None, t!("long.inherit", "value" => above), !listed(&s.long_files_everything) && !listed(&s.long_files_sample)),
    ]
    .into_iter()
    .map(|(whole, label, current)| {
        let changes = changes_for(s, dir, whole);
        let label = if current {
            format!("{label} · {}", t!("ptools.current"))
        } else {
            let new = with(s, &changes);
            match change_notice(s, &new, measured, &cost) {
                Some(why) => format!("{label}: {why}"),
                None => label,
            }
        };
        Choice { label, current, changes }
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(everything: bool, whole: &[&str], sample: &[&str]) -> Rule {
        Rule {
            everything,
            whole: whole.iter().map(PathBuf::from).collect(),
            sample: sample.iter().map(PathBuf::from).collect(),
        }
    }

    /// The deepest folder listed decides; the default where none is; *Sample* when one folder
    /// is in both lists; a folder named like the start of another is not above it.
    #[test]
    fn long_files_the_deepest_folder_decides() {
        let r = rule(false, &["/home/me/logs"], &["/home/me/logs/old"]);
        assert!(r.whole(Path::new("/home/me/logs/today.log")));
        assert!(!r.whole(Path::new("/home/me/logs/old/2019.log")));
        assert!(!r.whole(Path::new("/home/me/logs2/a.log")));
        assert!(!r.whole(Path::new("/home/me/notes.txt")));
        let r = rule(true, &["/a/b/c"], &["/a"]);
        assert!(r.whole(Path::new("/a/b/c/x")), "deeper Everything inside a Sample folder");
        assert!(!r.whole(Path::new("/a/b/x")));
        assert!(r.whole(Path::new("/elsewhere/x")), "the default");
        assert!(!rule(false, &["/a"], &["/a"]).whole(Path::new("/a/x")), "in both: Sample");
        assert!(!rule(false, &[""], &[]).whole(Path::new("/a")), "an empty entry is no folder");
    }

    /// A folder's choices write it into one list (out of the other), or out of both.
    #[test]
    fn long_files_a_folder_goes_into_one_list() {
        let s = SearchConfig {
            long_files_everything: vec!["/x".into(), "/a".into()],
            long_files_sample: vec!["/y".into()],
            ..SearchConfig::default()
        };
        let a = Path::new("/a");
        let c = changes_for(&s, a, Some(false));
        assert_eq!(c["long_files_everything"], serde_json::json!(["/x"]));
        assert_eq!(c["long_files_sample"], serde_json::json!(["/y", "/a"]));
        let c = changes_for(&s, a, None);
        assert_eq!(c["long_files_sample"], serde_json::json!(["/y"]));
        assert!(!Rule::of(&with(&s, &c)).whole(Path::new("/a/f")));
        let picked = choices(&s, a, 0, |_| Some(Cost::default()));
        assert_eq!(picked.iter().map(|c| c.current).collect::<Vec<_>>(), [true, false, false]);
    }

    /// The warning gives the long files, the room and the time at the measured speed; what
    /// going back frees; and nothing when no long file is touched.
    #[test]
    fn long_files_warning_numbers() {
        crate::i18n::set_language("en-GB");
        let s = SearchConfig { meaning: true, ..SearchConfig::default() };
        let c = Cost { files: 3, passages: 7200, bytes: 50 << 20, read: 0, back: 0, freed: 0 };
        // 7,200 passages at 50 ms: six minutes.
        let w = warning(&c, &s, 50);
        assert!(w.contains('3') && w.contains("50.0 MB") && w.contains("6 minutes"), "{w}");
        let back = Cost { back: 2, freed: 3 << 30, ..Cost::default() };
        let w = warning(&back, &s, 50);
        assert!(w.contains("3.0 GB") && w.contains('2') && !w.contains("minutes"), "{w}");
        assert_eq!(warning(&Cost::default(), &s, 50), t!("long.none"));
        assert_eq!(change_notice(&s, &s.clone(), 50, |_| None), None, "the same rule: nothing to say");
        let new = SearchConfig { long_files: "everything".into(), ..s.clone() };
        assert_eq!(change_notice(&s, &new, 50, |_| None), Some(t!("long.unknown")));
    }
}
