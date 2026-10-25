//! Coxswain core: everything that is not UI, shared by the TUI and the GUI.

pub mod archive;
pub mod attrs;
pub mod ask;
pub mod bom;
pub mod branches;
pub mod bridge;
pub mod checksum;
pub mod chess;
pub mod bsd;
pub mod chat;
pub mod cloud;
pub mod compare;
pub mod clouds;
pub mod code;
pub mod collate;
pub mod config;
pub mod container_image;
pub mod dbf;
pub mod draughts;
pub mod dirs;
pub mod disk;
pub mod disk_image;
pub mod drag;
pub mod dupes;
pub mod edit;
pub mod ebook;
pub mod export;
pub mod extract;
pub mod facts;
pub mod features;
pub mod find;
pub mod flags;
pub mod fs;
pub mod geo;
pub mod git;
pub mod git_config;
pub mod go;
pub mod graph;
pub mod guide;
pub mod handoff;
pub mod helper;
pub mod history;
pub mod i18n;
pub mod images;
pub mod local_models;
pub mod long_files;
pub mod icons;
pub mod index;
pub mod kicad;
#[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
mod kq;
pub mod machine;
pub mod meaning;
pub mod media;
pub mod menu;
pub mod migrate;
pub mod mosaic;
pub mod models;
pub mod music;
pub mod neural;
pub mod notices;
pub mod pictures;
pub mod pinned;
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
mod ports;
pub mod preview_tools;
pub mod provenance;
pub mod rename;
pub mod replace;
pub mod scad;
pub mod split;
pub mod searchable;
pub mod service;
pub mod settings;
pub mod setup;
pub mod shogi;
pub mod sizes;
pub mod space;
pub mod state;
mod stop;
pub mod store;
pub mod tables;
mod tar_index;
pub mod termux;
pub mod tools;
pub mod transcribe;
pub mod undo;
pub mod update;
pub mod user_menu;
pub mod vcs;
pub mod wc;
pub mod view;
pub mod viruses;
#[cfg(any(
    target_os = "android",
    target_os = "illumos",
    target_os = "solaris",
    all(test, unix)
))]
mod xdg_trash;
pub mod xiangqi;
pub mod zfs;

/// The time limit for a test that guards against a hang or a blow-up (a crafted input that must
/// not take minutes), and for a test's wait for what a thread does: `normal` in a release build,
/// ten times it and at least two minutes in a debug build, and twice that again on a slow
/// machine: with `COXSWAIN_SLOW_CI=1` (the virtual machines in CI) and on the BSDs and illumos,
/// which are tested in virtual machines 5-10 times slower than Linux. Every time limit in a test
/// goes through here, so all scale alike; a limit only ever guards against a hang, whose bad
/// path is hours. What must be quick by a smaller margin is proved without a clock: by counting
/// the work, by the state the code is in, or against a good input timed on the same machine
/// (`assert_costs_at_most`).
#[doc(hidden)]
pub fn test_limit(normal: std::time::Duration) -> std::time::Duration {
    let limit = if cfg!(debug_assertions) {
        (normal * 10).max(std::time::Duration::from_secs(120))
    } else {
        normal
    };
    if slow_test_machine() {
        limit * 2
    } else {
        limit
    }
}

/// Whether the tests run on a slow machine: `COXSWAIN_SLOW_CI=1`, or a BSD or illumos.
pub(crate) fn slow_test_machine() -> bool {
    cfg!(any(
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd",
        target_os = "illumos",
        target_os = "solaris"
    )) || std::env::var_os("COXSWAIN_SLOW_CI").is_some_and(|v| v == "1")
}

/// That `bad` (a crafted input) costs at most `times` what `good` (a plain one of the same
/// size) does, or that work grows no faster than it should (`good` at size n, `bad` at 8n and
/// `times` 20: linear work takes 8 times as long, quadratic 64):
/// both timed here and now, so a slow or busy machine slows both alike and only the ratio
/// counts. Busy moments only ever add time, so each is tried again, up to five times, and the
/// least time of each is compared. Each should take some milliseconds at least, to be measured.
#[doc(hidden)]
pub fn assert_costs_at_most(times: u32, mut good: impl FnMut(), mut bad: impl FnMut()) {
    let time = |f: &mut dyn FnMut()| {
        let t = std::time::Instant::now();
        f();
        t.elapsed()
    };
    let (mut g, mut b) = (std::time::Duration::MAX, std::time::Duration::MAX);
    for _ in 0..5 {
        g = g.min(time(&mut good));
        b = b.min(time(&mut bad));
        if b <= g * times {
            return;
        }
    }
    panic!("{b:?} against {g:?}: more than {times} times as long");
}

/// The watcher for folders: kqueue folder by folder on the BSDs (`kq`), event ports on illumos
/// (`ports`), notify's elsewhere.
#[cfg(any(target_os = "illumos", target_os = "solaris"))]
pub type DirWatcher = ports::PortWatcher;
#[cfg(any(target_os = "freebsd", target_os = "netbsd", target_os = "openbsd"))]
pub type DirWatcher = kq::KqueueWatcher;
#[cfg(not(any(
    target_os = "illumos",
    target_os = "solaris",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
)))]
pub type DirWatcher = notify::RecommendedWatcher;

/// A watched folder changed (or is gone): the event names the folder, and an entry in it, so that
/// the index, which reads again the folders changed entries are in, reads this one.
#[cfg(any(
    target_os = "illumos",
    target_os = "solaris",
    target_os = "freebsd",
    target_os = "netbsd",
    target_os = "openbsd"
))]
fn folder_event(dir: &std::path::Path, gone: bool) -> notify::Event {
    use notify::event::{EventKind, ModifyKind, RemoveKind};
    let mut e = notify::Event::new(if gone {
        EventKind::Remove(RemoveKind::Folder)
    } else {
        EventKind::Modify(ModifyKind::Any)
    })
    .add_path(dir.to_path_buf());
    if let Some(first) = std::fs::read_dir(dir)
        .ok()
        .and_then(|mut rd| rd.next())
        .and_then(Result::ok)
    {
        e = e.add_path(first.path());
    }
    e
}
