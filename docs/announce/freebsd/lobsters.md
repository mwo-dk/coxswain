# Lobsters-style submission

**Title:** Coxswain: a Norton Commander style file manager, now on FreeBSD
**URL:** https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md
**Tags:** freebsd, rust

Coxswain is a two-panel file manager (terminal app in Rust/Ratatui, desktop app in
Tauri/WebKitGTK) with git status per file, archives as folders, and search by name, text and
meaning that runs entirely on the machine. Release 1.43.0 adds FreeBSD 14/15 amd64 builds with
a plain-sh installer that verifies SHA-256 sums and asks before `pkg install`, an rc.d script
for the indexing helper, and a manual page. The terminal app is tested on FreeBSD in CI; the
desktop app is marked experimental, as Tauri has no official FreeBSD support and two crates are
patched (tao's BSD fix is merged upstream, drag-rs's is a pending PR). The write-up covers
the kqueue watching budget, rc.d versus XDG autostart for the helper, and what is still missing
on NetBSD and OpenBSD.
