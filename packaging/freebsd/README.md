# FreeBSD packaging

| File | What it is |
|---|---|
| `coxswain_index` | The rc.d script for the search helper, as the install script and the release archives ship it (`/usr/local/etc/rc.d/coxswain_index`) |
| `coxswain.desktop` | The desktop app's menu entry, in the desktop release archive |
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
   for the terminal`, with the diff attached and the text from
   [docs/announce/freebsd/bugzilla-new-port.md](../../docs/announce/freebsd/bugzilla-new-port.md).
3. A committer reviews it; answer in the PR. Once it is in the tree, updates are
   `sysutils/coxswain: Update to X.Y.Z` PRs with the diff and *maintainer-approval* set to `+`.

For updates by the release workflow later, a Bugzilla API key is made under
*Preferences → API Keys* at bugs.freebsd.org and stored as the repository secret
`FREEBSD_BUGZILLA_KEY`.
