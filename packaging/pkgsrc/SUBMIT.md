# Submitting Coxswain to pkgsrc-wip

**Owner steps, in order:** [termux-packages](../termux/SUBMIT.md), **pkgsrc-wip**, [omnios-extra](../omnios/SUBMIT.md), [oi-userland](../openindiana/SUBMIT.md), [openbsd-wip](../openbsd/port/SUBMIT.md), [nixpkgs](../nix/SUBMIT.md). This is step 2 of 6; see [packaging/README.md](../README.md#submitting-by-hand).

The package is [`coxswain/`](coxswain/) in this folder, for `wip/coxswain` in
[pkgsrc-wip](https://pkgsrc.org/wip/). pkgsrc builds on NetBSD, SmartOS and other illumos
distributions, and more; this one package covers them. It builds the terminal app from the
**crates.io** release, so the desktop app's crates are not fetched.

| File | What |
|---|---|
| `Makefile` | The recipe: `lang/rust/cargo.mk`, the `cox` link, the manual page |
| `cargo-depends.mk`, `distinfo` | The crates from the release's `Cargo.lock` and their checksums; written by `update.py` |
| `DESCR`, `PLIST` | Description and the installed files |
| `files/coxswain_index.sh` | rc.d script for the search helper (`RCD_SCRIPTS`): NetBSD and the other BSDs |
| `files/smf/manifest.xml` | SMF service for the search helper, used where pkgsrc installs SMF (SmartOS, illumos): `svc:/pkgsrc/coxswain-index` |

Nothing is submitted for you, and the announcement drafts are kept locally by the owner.

## Updating to a release

```sh
python3 packaging/pkgsrc/update.py 2.8.1
```

It downloads the crate and every dependency from crates.io (checked against `Cargo.lock`),
writes `cargo-depends.mk` and `distinfo` as `make print-cargo-depends` and `make makesum` would,
and sets `DISTNAME`. Commit the result here; CI then builds it.

## What is tested, and what is not

Tested in CI (`.github/workflows/pkgsrc.yml`, in a NetBSD 10.1 VM, whenever this
folder changes): `make package` with the pkgsrc tree of the current quarterly branch and the
binary Rust from pkgin, `pkg_add` of the result, `coxswain --version` and `cox --version`,
`man -w coxswain`, and the rc.d script installed under `share/examples/rc.d`.

Not tested: `pkglint` (run it, step 3), SmartOS and other illumos systems with pkgsrc, the SMF
manifest's install through pkgsrc, and builds with pkgsrc's own Rust built from source.

## AI-written work

As of 2026-10-06 pkgsrc-wip has no written rule on AI-assisted contributions; NetBSD's
commit guidelines (<https://www.netbsd.org/developers/commit-guidelines.html>) have one for
developers with commit rights. Check both on the day and follow them. This package was drafted
with an AI assistant (Claude): read every line, build it yourself, and say so in the commit
message's last line if their rules ask for it.

## Steps

1. **Get wip access.** As <https://pkgsrc.org/wip/> says (read 2026-10-06): mail Thomas
   Klausner your public SSH key, the user name you would like, and a word about the package.
   The same page says how commits are made, including the `COMMIT_MSG` file; follow it.
2. **Check out pkgsrc and wip** on a NetBSD (or SmartOS) machine:

   ```sh
   cd /usr && cvs -q -z2 -d anoncvs@anoncvs.NetBSD.org:/cvsroot checkout -P pkgsrc
   cd /usr/pkgsrc && git clone git://wip.pkgsrc.org/pkgsrc-wip.git wip   # push with the URL you are given
   cp -r /path/to/coxswain/packaging/pkgsrc/coxswain wip/coxswain
   ```

3. **Build and check:**

   ```sh
   cd /usr/pkgsrc/wip/coxswain
   make package install
   coxswain --version
   pkg_info -L coxswain
   pkglint -Wall
   ```

   Fix what `pkglint` reports (here, then copy again).
4. **Add it to wip's list:** a line `SUBDIR+=	coxswain` in `wip/Makefile`, in order.
5. **Commit** with wip's message form (first line the package and what it is; wip also wants
   a `COMMIT_MSG` file in the package while it is in wip, see their page):

   ```sh
   cd /usr/pkgsrc/wip
   git add coxswain Makefile
   git commit -m "coxswain: add version 2.8.1

   Two-panel file manager for the terminal, Norton Commander style."
   git push
   ```

6. Later, a pkgsrc developer may move it to `sysutils/coxswain`; `make package` from the main
   tree and `pkgin install coxswain` then follow. Ask on pkgsrc-users@NetBSD.org when it has
   been in wip a while.
