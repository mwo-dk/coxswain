//! The built-in models on this machine, for both apps and `coxswain --models`: each one
//! downloaded (the embedding model and the chat models), with its size on disk, its folder,
//! when it was last used, whether it is loaded now and where, and whether the settings use it.
//! What is left over (an unfinished download, a model of an older pinned revision) is listed
//! too, as not in use. Each can be unloaded or deleted; deleting the one in use switches to
//! the recommended choice and says so.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::SearchConfig;
use crate::t;

/// What a model is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum For {
    /// Search by meaning: it makes the vectors.
    Meaning,
    /// Ask: it answers.
    Ask,
    /// Nothing any more: an unfinished download, an older version, files of no model.
    Leftover,
}

/// A built-in model on the disk.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Its folder's name, which `delete` takes.
    pub id: String,
    /// As the user sees it: "Qwen3 14B", "multilingual-e5-small".
    pub name: String,
    pub what: For,
    /// Why it is left over, in words.
    pub note: Option<String>,
    pub folder: PathBuf,
    pub bytes: u64,
    /// When it was last loaded or asked, in seconds since 1970; None when not yet.
    pub used: Option<u64>,
    /// The settings use it.
    pub in_use: bool,
    /// Loaded now: where ("on the CPU", "on the GPU (Metal)"), and about the memory it holds.
    pub loaded: Option<(String, u64)>,
    /// `line()`, for the desktop app.
    pub summary: String,
}

impl Entry {
    /// "2026-10-08", or "not used yet".
    pub fn used_text(&self) -> String {
        match self.used {
            Some(s) => t!("models.used", "date" => date(s)),
            None => t!("models.never_used"),
        }
    }

    /// One line about it: what it is for, its size, whether it is in use and loaded.
    pub fn line(&self) -> String {
        let what = match self.what {
            For::Meaning => t!("models.for_meaning"),
            For::Ask => t!("models.for_ask"),
            For::Leftover => self.note.clone().unwrap_or_default(),
        };
        let mut parts = vec![what, crate::settings::human(self.bytes), if self.in_use { t!("models.in_use") } else { t!("models.not_in_use") }];
        parts.push(match &self.loaded {
            Some((at, bytes)) => t!("models.loaded", "where" => at, "size" => crate::settings::human(*bytes)),
            None => t!("models.not_loaded"),
        });
        parts.push(self.used_text());
        parts.join(" · ")
    }
}

/// The date of `secs` since 1970, here.
fn date(secs: u64) -> String {
    let t = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d").to_string()
}

/// Where the built-in models are kept: the cache folder's `models`.
pub fn folder() -> Option<PathBuf> {
    Some(crate::helper::folder()?.join("models"))
}

/// The file whose time says when a model was last used.
const USED: &str = ".last-used";

/// Note that the model in `dir` was used now.
pub(crate) fn used(dir: &Path) {
    let _ = std::fs::write(dir.join(USED), "");
}

/// What is loaded in the search helper (or in this app, when it runs without one): the chat
/// model's id and whether on a Mac's GPU; where the embedding model runs, when it does.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Loaded {
    pub chat: Option<(String, bool)>,
    pub meaning: Option<crate::meaning::Runs>,
}

/// Every built-in model on the disk, those in use first.
pub fn list(cfg: &SearchConfig, loaded: &Loaded) -> Vec<Entry> {
    folder().map(|d| list_in(&d, cfg, loaded)).unwrap_or_default()
}

/// The models in `dir`.
fn list_in(dir: &Path, cfg: &SearchConfig, loaded: &Loaded) -> Vec<Entry> {
    let Ok(rd) = std::fs::read_dir(dir) else { return vec![] };
    let where_ = |metal: bool| if metal { t!("meaning.on_metal") } else { t!("meaning.on_cpu") };
    let mut out: Vec<Entry> = rd
        .flatten()
        .map(|e| {
            let path = e.path();
            let id = e.file_name().to_string_lossy().into_owned();
            let bytes = crate::fs::dir_size(&path).0;
            let used = std::fs::metadata(path.join(USED)).and_then(|m| m.modified()).ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_secs());
            let mut entry = Entry { id: id.clone(), name: id.clone(), what: For::Leftover, note: None, folder: path.clone(), bytes, used, in_use: false, loaded: None, summary: String::new() };
            if id == crate::meaning::folder_name() {
                entry.name = crate::meaning::MODEL.into();
                if crate::meaning::installed_in(&path) {
                    entry.what = For::Meaning;
                    entry.in_use = cfg.meaning && cfg.meaning_engine == "builtin";
                    entry.loaded = loaded.meaning.as_ref().map(|r| (r.text(), crate::meaning::size()));
                } else {
                    entry.note = Some(t!("models.unfinished"));
                }
            } else if let Some(m) = crate::chat::MODELS.iter().find(|m| m.folder_name() == id) {
                entry.name = m.name.into();
                if m.installed_in(&path) {
                    entry.what = For::Ask;
                    entry.in_use = cfg.ask_model == m.key();
                    entry.loaded = loaded.chat.as_ref().filter(|(id, _)| id == m.id).map(|(_, metal)| (where_(*metal), m.size()));
                } else {
                    entry.note = Some(t!("models.unfinished"));
                }
            } else {
                // A folder of another revision is named as its model, with eight other characters.
                let stem = id.rsplit_once('-').map_or(id.as_str(), |(s, _)| s);
                let older = crate::chat::MODELS.iter().find(|m| m.id == stem).map(|m| m.name).or((stem == crate::meaning::MODEL).then_some(crate::meaning::MODEL));
                entry.note = Some(match older {
                    Some(name) => {
                        entry.name = name.into();
                        t!("models.older")
                    }
                    None => t!("models.other"),
                });
            }
            entry.summary = entry.line();
            entry
        })
        .collect();
    out.sort_by_key(|e| (!e.in_use, e.what == For::Leftover, e.name.clone()));
    out
}

/// Bytes the models not in use take: what "Delete all not in use" frees.
pub fn unused_bytes(entries: &[Entry]) -> u64 {
    entries.iter().filter(|e| !e.in_use).map(|e| e.bytes).sum()
}

/// What deleting a model does to the settings, when it is in use: the options to save, and
/// what to tell the user. `look`: the servers asked, for the recommended choice.
pub fn instead(cfg: &SearchConfig, entry: &Entry, look: Option<&crate::setup::Look>) -> Option<(serde_json::Map<String, serde_json::Value>, String)> {
    if !entry.in_use {
        return None;
    }
    let mut changes = serde_json::Map::new();
    match entry.what {
        For::Ask => {
            // The recommended one, if it is at hand: a server's, or another built-in one downloaded.
            let next = crate::setup::recommend_ask(cfg, look);
            let next = match crate::chat::of(&next) {
                Some(m) if m.key() == cfg.ask_model || !m.installed() => crate::chat::MODELS.iter().rev().find(|m| m.key() != cfg.ask_model && m.installed() && crate::chat::fitting(crate::setup::ram_gb()).any(|f| f.id == m.id)).map(|m| m.key()).unwrap_or_default(),
                _ => next,
            };
            changes.insert("ask_model".into(), next.clone().into());
            let said = if next.is_empty() { t!("models.deleted_ask_off", "model" => entry.name.as_str()) } else { t!("models.deleted_ask_now", "model" => entry.name.as_str(), "next" => crate::chat::shown(&next)) };
            Some((changes, said))
        }
        For::Meaning => {
            let better = look.and_then(|l| crate::setup::better_meaning(false, &l.gpu_servers()).map(|(f, m, _)| (f.engine.clone(), f.url.clone(), m)));
            let said = match better {
                Some((engine, url, model)) => {
                    let said = t!("models.deleted_meaning_now", "model" => entry.name.as_str(), "next" => model.as_str());
                    changes.insert("meaning_engine".into(), engine.into());
                    changes.insert("meaning_url".into(), url.into());
                    changes.insert("meaning_model".into(), model.into());
                    said
                }
                None => {
                    changes.insert("search_meaning".into(), false.into());
                    t!("models.deleted_meaning_off", "model" => entry.name.as_str())
                }
            };
            Some((changes, said))
        }
        For::Leftover => None,
    }
}

/// The entry `name` names: its folder's name, its model's id or `ask_model`, or its name.
pub fn find<'a>(entries: &'a [Entry], name: &str) -> Option<&'a Entry> {
    let n = name.trim().trim_start_matches(crate::chat::PREFIX).to_lowercase();
    let id_of = |e: &Entry| crate::chat::MODELS.iter().find(|m| m.name == e.name).map(|m| m.id);
    entries.iter().find(|e| e.id.to_lowercase() == n).or_else(|| entries.iter().find(|e| e.what != For::Leftover && (e.name.to_lowercase() == n || id_of(e) == Some(n.as_str()))))
}

/// Delete a model's folder; a chat model is let go of first, here and in the helper.
pub fn delete(entry: &Entry, helper: &crate::helper::Client) -> std::io::Result<()> {
    if entry.what == For::Ask {
        crate::chat::unload();
        helper.unload();
    }
    match std::fs::remove_dir_all(&entry.folder) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Models of this revision, an unfinished download and an older revision, in a cache of a
    /// test's own: each is named, the leftovers are not in use, and a name finds its entry.
    #[test]
    fn the_models_on_the_disk_are_listed() {
        let dir = std::env::temp_dir().join(format!("coxswain-models-{}", std::process::id()));
        let small = &crate::chat::MODELS[0];
        let chat = dir.join(small.folder_name());
        std::fs::create_dir_all(&chat).unwrap();
        // Unfinished: a part of the weights.
        std::fs::write(chat.join("Qwen3-1.7B-Q4_K_M.gguf.part"), vec![0u8; 1000]).unwrap();
        let old = dir.join("qwen3-14b-0123abcd");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::write(old.join("weights.gguf"), vec![0u8; 5000]).unwrap();
        used(&old);
        let cfg = SearchConfig { ask_model: small.key(), ..SearchConfig::default() };
        let all = list_in(&dir, &cfg, &Loaded::default());
        assert_eq!(all.len(), 2, "{all:?}");
        let e = find(&all, "qwen3-14b-0123abcd").unwrap();
        assert_eq!((e.name.as_str(), e.what, e.in_use, e.bytes), ("Qwen3 14B", For::Leftover, false, 5000));
        assert!(e.used.is_some() && e.note.as_deref() == Some(t!("models.older").as_str()));
        let e = all.iter().find(|e| e.folder == chat).unwrap();
        assert_eq!((e.what, e.in_use), (For::Leftover, false), "an unfinished download is not in use");
        assert_eq!(unused_bytes(&all), 6000);
        assert!(instead(&cfg, e, None).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Deleting the chat model in use turns Ask to another one downloaded, or off.
    #[test]
    fn deleting_the_model_in_use_says_what_comes_instead() {
        let e = Entry { id: "x".into(), name: "Qwen3 14B".into(), what: For::Ask, note: None, folder: PathBuf::new(), bytes: 0, used: None, in_use: true, loaded: None, summary: String::new() };
        let cfg = SearchConfig { ask_model: "builtin:qwen3-14b".into(), ..SearchConfig::default() };
        let (changes, said) = instead(&cfg, &e, None).unwrap();
        assert!(changes["ask_model"].as_str().is_some_and(|m| m != "builtin:qwen3-14b"), "{changes:?}");
        assert!(said.contains("Qwen3 14B"), "{said}");
        let meaning = Entry { what: For::Meaning, ..e };
        let (changes, _) = instead(&cfg, &meaning, None).unwrap();
        assert_eq!(changes["search_meaning"], false);
    }
}
