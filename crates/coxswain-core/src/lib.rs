//! Coxswain core: everything that is not UI, shared by the TUI and the GUI.

pub mod archive;
pub mod bom;
pub mod chat;
pub mod cloud;
pub mod config;
pub mod i18n;
pub mod dupes;
pub mod extract;
pub mod find;
pub mod fs;
pub mod git;
pub mod guide;
pub mod helper;
pub mod branches;
pub mod history;
pub mod icons;
pub mod index;
pub mod machine;
pub mod notices;
pub mod meaning;
pub mod migrate;
pub mod provenance;
pub mod rename;
pub mod service;
pub mod settings;
pub mod setup;
pub mod sizes;
pub mod state;
pub mod store;
pub mod tables;
pub mod termux;
pub mod tools;
pub mod undo;
#[cfg(any(target_os = "android", all(test, unix)))]
mod xdg_trash;
pub mod zfs;
pub mod bsd;
pub mod flags;
pub mod update;
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
mod ports;
#[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
mod kq;

/// The watcher for folders: kqueue folder by folder on the BSDs (`kq`), event ports on illumos
/// (`ports`), notify's elsewhere.
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
pub type DirWatcher = ports::PortWatcher;
#[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
pub type DirWatcher = kq::KqueueWatcher;
#[cfg(not(any(target_os = "illumos", target_os = "solaris", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd")))]
pub type DirWatcher = notify::RecommendedWatcher;

/// A watched folder changed (or is gone): the event names the folder, and an entry in it, so that
/// the index, which reads again the folders changed entries are in, reads this one.
#[cfg(any(target_os = "illumos", target_os = "solaris", target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
fn folder_event(dir: &std::path::Path, gone: bool) -> notify::Event {
    use notify::event::{EventKind, ModifyKind, RemoveKind};
    let mut e = notify::Event::new(if gone { EventKind::Remove(RemoveKind::Folder) } else { EventKind::Modify(ModifyKind::Any) }).add_path(dir.to_path_buf());
    if let Some(first) = std::fs::read_dir(dir).ok().and_then(|mut rd| rd.next()).and_then(Result::ok) {
        e = e.add_path(first.path());
    }
    e
}
