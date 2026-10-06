# termux/termux-app Discussions: Show and tell

**Category:** Show and tell
**Title:** Coxswain: a two-panel file manager with Termux support (phone folders, termux-open, Termux:API)

Hello,

I would like to show a file manager I make that has learnt Termux's ways. Coxswain is a
two-panel, Norton Commander style file manager for the terminal (Rust, MIT). It is packaged in
termux-packages:

    pkg install coxswain

<!-- Until the termux-packages pull request is merged, use instead:
    pkg install rust git
    cargo install coxswain -->

It tells it is in Termux from `TERMUX_VERSION` (or `$PREFIX` under `/data/data/com.termux/`)
and then:

| Where | What happens |
|---|---|
| Alt+F1 / Alt+F2 | After `termux-setup-storage`, the go-to list has the links in `~/storage`: *Phone: shared*, *Phone: downloads*, *Phone: dcim* … A missing memory card is left out. Until storage is set up, the status line says once how to do it. |
| Enter on a file | `termux-open`, so Android's chooser or the file's app opens it |
| Copying a line (Settings) | `termux-clipboard-set` when Termux:API is installed, else OSC 52 |
| The search helper | Asks `termux-battery-status` and waits while the phone is on battery; each Termux:API call gets a few seconds, so a missing app never hangs it |
| F8 | Android has no trash: F8 says so, and Shift+F8 deletes for good after asking |
| HTTPS | Termux's own certificates in `$PREFIX/etc/tls/cert.pem` |

<!-- If #141 has merged and is in the released package, replace the F8 row with:
| F8 | Moves to `~/.local/share/Trash`; on the phone's shared storage, which is another filesystem, it asks before deleting for good | -->

<!-- gif: termux-alt-f1.gif: Coxswain in Termux, Alt+F1 listing the phone's folders, Enter on
Downloads, Enter on a picture opening Android's chooser. -->

Limits: it is the terminal app only (the project's desktop app needs WebKitGTK). There is no
session service on Android, so the search helper starts with Coxswain and stays ten minutes
after it quits; `termux-wake-lock` keeps Android from stopping it with the screen off.

The package is built for aarch64 with termux-packages' builder and started in the Termux
Docker image in the project's CI on every change.

Guide: https://github.com/mwo-dk/coxswain/blob/master/docs/reference/termux.md
Source: https://github.com/mwo-dk/coxswain

Thank you for Termux; it made this a pleasure to build. Feedback welcome.
