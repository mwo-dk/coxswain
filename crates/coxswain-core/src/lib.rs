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
pub mod zfs;
pub mod bsd;
pub mod flags;
pub mod update;
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
mod ports;

/// The watcher for folders: notify's, but event ports on illumos, where notify would poll.
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
pub type DirWatcher = ports::PortWatcher;
/// The watcher for folders: notify's, but event ports on illumos, where notify would poll.
#[cfg(not(any(target_os = "illumos", target_os = "solaris")))]
pub type DirWatcher = notify::RecommendedWatcher;
