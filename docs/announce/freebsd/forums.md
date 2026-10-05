# FreeBSD Forums: Useful scripts and software

**Title:** Coxswain: a two-panel file manager (terminal and desktop) with instant search, now on FreeBSD

Hello all,

Coxswain is a two-panel file manager in the Norton Commander tradition, MIT licensed. As of
version 1.43.0 it ships FreeBSD builds with every release, for FreeBSD 14 and 15 on amd64.

It comes as two programs that share one Rust core and one configuration file:

- `coxswain` (also `cox`), the terminal app: two panels, the F-key bar, a command line, git
  status for every file, archives (zip, 7z, tar.gz/bz2/xz/zst) opened like folders.
- `coxswain-gui`, the desktop app: tabs, a sidebar, a preview pane for over 60 file types,
  themes. It is a Tauri (WebKitGTK) app.

Both find any file on the machine by name as you type, search the text inside documents, and
can search by meaning with a small model that runs locally (or a local Ollama, which is in
packages). Nothing leaves the machine unless you ask for it; the daily update check can be
turned off.

Install, as a user with doas or sudo:

    fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh

The script is plain sh. It checks the SHA-256 sums of the release archives, installs into
/usr/local (or ~/.local, your choice), shows the exact `pkg install` line for what the desktop
app needs (webkit2-gtk_41, gtk3, libsoup3, xdg-utils, gstreamer1-plugins-good) and runs it only
after you say yes. It also installs a manual page and an rc.d script, coxswain_index, which runs
the search helper for one user from boot:

    sysrc coxswain_index_enable=YES coxswain_index_user=alice
    service coxswain_index start

`--terminal-only` installs the terminal app alone; it links only against base libraries.

What to expect:

- The terminal app is a first-class build, tested in a FreeBSD 14.5 VM on every change.
- The desktop app is experimental. Browsing, previews, Find, copy/move, the trash and the
  clipboard work under X11. It builds against two crates patched until upstream releases
  (tao's BSD hit-test fix, already merged, and drag-rs's GTK backend for BSD, PR open).
  Wayland and FreeBSD 15 have not been tested by me yet; reports welcome.
- File watching uses kqueue on folders only, capped at 20,000, so the helper does not fill the
  file table.

Guide (packages, the helper without systemd, troubleshooting):
https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md

Source and releases: https://github.com/mwo-dk/coxswain

A sysutils/coxswain port is prepared and will be submitted to Bugzilla.

Michael
