# freebsd-ports@FreeBSD.org

**Subject:** New port: sysutils/coxswain, a two-panel file manager for the terminal

Hello,

I would like to introduce a new port, sysutils/coxswain, and will file it in Bugzilla shortly.

Coxswain is a two-panel file manager in the Norton Commander tradition (MIT licence, Rust).
The port builds the terminal app with USES=cargo: `coxswain` and `cox`, the manual page
coxswain(1), and an rc.d script, coxswain_index, that runs the search helper as one user
(coxswain_index_enable, coxswain_index_user). The binary links only against base libraries.

Upstream builds and tests on FreeBSD 14.5 in CI on every change, and every release carries
FreeBSD amd64 archives. The desktop app (Tauri, WebKitGTK) also builds on FreeBSD but needs
two patched crates until their upstream releases, so it is left out of the port for now.

The port was checked with `make stage check-plist package` on 14.5-RELEASE amd64.

  WWW: https://github.com/mwo-dk/coxswain
  FreeBSD notes: https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md

I am happy to maintain it. Comments on the port are welcome.

Michael W. Olesen
