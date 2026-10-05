# Bugzilla: new port

**Product:** Ports & Packages
**Component:** Individual Port(s)
**Summary:** [NEW PORT] sysutils/coxswain: Two-panel file manager for the terminal
**Attachment:** coxswain.diff (the new port directory and the `SUBDIR` line in `sysutils/Makefile`)

**Description:**

Coxswain is a two-panel file manager in the Norton Commander tradition, written in Rust (MIT).
The panels show git status for every file, the function keys copy, move and delete, and zip, 7z
and tar archives open like folders. It finds any file by name as you type, searches the text
inside documents, and can search by meaning with a model that runs locally.

The port builds the terminal app with USES=cargo from the GitHub release tag and installs:

- bin/coxswain and bin/cox
- share/man/man1/coxswain.1.gz
- etc/rc.d/coxswain_index (USE_RC_SUBR): runs the search helper as one user from boot;
  coxswain_index_enable, coxswain_index_user

The binary links only against base libraries. The desktop app (Tauri, WebKitGTK) builds on
FreeBSD too, but needs two crates from git until their upstream releases, so it is not part of
this port.

Upstream tests on FreeBSD 14.5 in CI on every change and ships FreeBSD amd64 archives with each
release.

Tested on 14.5-RELEASE amd64:

- make stage check-plist package: OK
- portlint -AC: 0 fatal errors (the only warning asks for DEVELOPER=yes)
- installed the package; `coxswain --version`, `man coxswain`, and `service coxswain_index start`
  with coxswain_index_user set

WWW: https://github.com/mwo-dk/coxswain
FreeBSD notes: https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md

I volunteer as maintainer.
