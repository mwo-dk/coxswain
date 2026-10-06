# FreeBSD packaging

| File | What it is |
|---|---|
| `coxswain_index` | The rc.d script for the search helper, as the install script and the release archives ship it (`/usr/local/etc/rc.d/coxswain_index`) |
| `coxswain.desktop` | The desktop app's menu entry, in the desktop release archive |
| `update-port.sh`, `file-bugzilla.py` | The release workflow's port update and its Bugzilla filing ([below](#after-it-is-in-the-tree-automatic)) |
| `sysutils/coxswain/` | A port of the terminal app for the ports tree, ready to submit: `Makefile`, `Makefile.crates`, `distinfo`, `pkg-descr`, `files/coxswain_index.in` |

The user-facing guide is [docs/reference/freebsd.md](../../docs/reference/freebsd.md).

## The port

`sysutils/coxswain` builds the terminal app with `USES=cargo` from the GitHub tag, and installs
`bin/coxswain`, `bin/cox`, `coxswain(1)` and the rc.d script `coxswain_index` (with `%%PREFIX%%`).
The desktop app is left out: it needs two crates patched from git (`[patch.crates-io]` in
`Cargo.toml`) until their releases, which a port should not carry.

### Updating it for a release

In a FreeBSD VM with a ports tree (`git clone --depth 1 https://git.FreeBSD.org/ports.git /usr/ports`):

```sh
cp -R packaging/freebsd/sysutils/coxswain /usr/ports/sysutils/
cd /usr/ports/sysutils/coxswain
# set DISTVERSION in the Makefile, then:
rm -f Makefile.crates distinfo
make makesum              # the GitHub tarball's sum
make cargo-crates | grep -v '^===>' > Makefile.crates
make makesum              # every crate's sum into distinfo
make stage check-plist package
make clean && git -C /usr/ports add sysutils/coxswain && portlint -AC   # pkg install portlint
```

Copy `Makefile`, `Makefile.crates` and `distinfo` back here. `make cargo-crates` lists every
crate in `Cargo.lock`, the desktop app's and the two git ones too: cargo needs them all to read
the workspace, though only the terminal app is built (`CARGO_BUILD_ARGS=--package coxswain`).
Oniguruma comes from `devel/oniguruma` (`LIB_DEPENDS`), as the ports framework wants.

### Submitting

The first submission is by hand, as a new port:

1. Make a diff against the ports tree: `git -C /usr/ports add sysutils/coxswain` and
   `git -C /usr/ports diff --staged > coxswain.diff`; add `sysutils/coxswain` to
   `sysutils/Makefile` (`SUBDIR += coxswain`, sorted) in the same diff.
2. File it at <https://bugs.freebsd.org/bugzilla/enter_bug.cgi?product=Ports%20%26%20Packages>,
   component *Individual Port(s)*, summary `[NEW PORT] sysutils/coxswain: Two-panel file manager
   for the terminal`, with the diff attached and a short description of the port (the owner keeps the
   prepared text locally).
3. A committer reviews it; answer in the PR. Once it is in the tree, updates are
   `sysutils/coxswain: Update to X.Y.Z` PRs with the diff and *maintainer-approval* set to `+`.

### After it is in the tree: automatic

For every release, the `freebsd-port` job in `release.yml` runs `update-port.sh` in a FreeBSD
14.5 VM: it sets `DISTVERSION`, regenerates `distinfo` and `Makefile.crates`, builds and checks
the package (`make stage check-plist package`, `portlint -AC`), and makes `port.diff` against
the port in the ports tree. The port and the diff are the run's `freebsd-port` artifact; copy
the three files back here in a pull request of our own.

Then `file-bugzilla.py` files *sysutils/coxswain: Update to X.Y.Z* (product *Ports & Packages*,
component *Individual Port(s)*) with the diff attached, *maintainer-approval* set to `+`, and the
release's changelog row as the description. It does so only when both hold:

- the port is in the ports tree (until then the job leaves a notice), and
- the repository secret `FREEBSD_BUGZILLA_KEY` is set (until then, a notice too).

The key: log in at <https://bugs.freebsd.org/bugzilla/> with the maintainer's account (the
`MAINTAINER` address), *Preferences → API Keys*, generate one with a description such as
"coxswain releases", and store it: `gh secret set FREEBSD_BUGZILLA_KEY`. Revoke it there when
it is no longer needed.
