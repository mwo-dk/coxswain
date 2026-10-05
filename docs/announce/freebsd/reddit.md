# r/freebsd

**Title:** Coxswain, a Norton Commander style file manager with instant search, now has FreeBSD builds (terminal + experimental desktop app)

Coxswain is an MIT-licensed two-panel file manager: a terminal app (`coxswain`, Rust/Ratatui)
and a desktop app (`coxswain-gui`, Tauri/WebKitGTK) on one shared core. Git status in every
panel, archives as folders, a preview pane, name search across the whole machine as you type,
text search inside documents, and optional search by meaning with a local model.

Since 1.43.0 every release has FreeBSD 14/15 amd64 builds. One line installs both:

    fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh

It is plain sh, checks SHA-256 sums, asks before `pkg install` (and shows the line), uses doas
or sudo, and installs a man page and an rc.d script for the search helper. `--terminal-only`
for just the terminal app, which needs nothing outside base.

Status, plainly: the terminal app is tested on FreeBSD in CI on every change. The desktop app
is experimental: it works under X11 (browse, preview, Find, copy/move, trash, clipboard), it
needs two patched Tauri-stack crates until upstream releases, and it is untested on Wayland
and on 15.x so far.

Guide: https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md
Repo: https://github.com/mwo-dk/coxswain

Feedback from real FreeBSD desktops is very welcome, especially KDE Plasma on Wayland.
