# Submitting Coxswain to openbsd-wip, then to OpenBSD ports

**Owner steps, in order:** [termux-packages](../../termux/SUBMIT.md), [pkgsrc-wip](../../pkgsrc/SUBMIT.md), [omnios-extra](../../omnios/SUBMIT.md), [oi-userland](../../openindiana/SUBMIT.md), **openbsd-wip**, [nixpkgs](../../nix/SUBMIT.md). This is step 5 of 6; see [packaging/README.md](../../README.md#submitting-by-hand).

The port is [`sysutils/coxswain/`](sysutils/coxswain/) in this folder, laid out as in
[openbsd-wip](https://github.com/jasperla/openbsd-wip) and the ports tree. It builds the
terminal app from the **crates.io** release (that crate is the terminal app alone, with its own
`Cargo.lock`), with `MODULES = devel/cargo` and the system's `oniguruma` and `sqlite3`.

| File | What |
|---|---|
| `Makefile` | The port: the crate from crates.io, the `cox` link, the manual page |
| `crates.inc` | The crates from the release's `Cargo.lock`, with their licences (`make modcargo-gen-crates-licenses`) |
| `distinfo` | Checksums of the crate and every dependency (`make makesum`) |
| `pkg/DESCR`, `pkg/PLIST` | Description and the installed files |
| `patches/patch-modcargo-crates_coxswain-core-2_8_1_Cargo_toml` | Drops rusqlite's `bundled` feature, so the system's sqlite3 is linked (the cargo module removes the bundled C sources; FTS5, which the search store needs, is in OpenBSD's sqlite3) |
| `pkg/coxswain_index.rc` | rc.d script for the search helper; the ports tree installs `pkg/*.rc` as `/etc/rc.d/coxswain_index` |

Nothing here sends mail or opens pull requests: you do, and the
announcement drafts are kept locally by the owner.

## What is tested, and what is not

Tested in CI, when started by hand (`gh workflow run openbsd.yml`, job *OpenBSD 7.9, the port
for openbsd-wip*), on OpenBSD 7.9 amd64 with the 7.9 `ports.tar.gz` and openbsd-wip on
`PORTSDIR_PATH`: `make makesum` and `make modcargo-gen-crates-licenses` made again and compared
with the files here, `make package`, `make port-lib-depends-check` clean, `make update-plist`
unchanged, `portcheck` (its output is in the run's artifact), the package installed with
`pkg_add`, `coxswain --version`, `cox --version`, `man -w coxswain`, and the rc.d script started
for a named user, checked and stopped.

Not tested: architectures other than amd64 and -current. `make test` runs, but its result is
only logged: in 2.8.1 five tests of the program (`bom::tests::*`, `provenance::tests::*`) fail
there, because they read test files from `../coxswain-core/src/…/testdata` in the repository,
which the crates.io crate does not carry. Reviewers will ask about it; the answer is that it is
a packaging bug of the tests, not of the program, to be fixed in a later release.

## AI-written work

The port, this file and the CI job were drafted with an AI assistant (Claude). As of
2026-10-07 neither OpenBSD nor openbsd-wip has a written rule on AI-assisted contributions:
there is none in openbsd-wip's README, in the ports FAQ and porting guide
(<https://www.openbsd.org/faq/ports/>), or in the copyright policy. What they do say:

- The porting guide: *"Test, then re-test, and finally test again!"*, and that *"In OpenBSD
  culture, MAINTAINERship is not a status item, but a responsibility."*
- The copyright policy (<https://www.openbsd.org/policy.html>): *"OpenBSD strives to provide
  code that can be freely used, copied, modified, and distributed by anyone and for any
  purpose."* NetBSD, close by, presumes LLM-written code tainted; OpenBSD developers may ask
  the same question.

So: read every line of the port until you could have written it, build it yourself (step 3),
write the mail and the pull request text yourself, and say in one sentence that an assistant
helped draft the port and that you reviewed and tested it. Check both places again on the day.

## Steps

1. **Pick the release.** The port names 2.8.1. For a newer one, set `V` in the `Makefile`, then
   on OpenBSD with the port in the tree (step 3) remake the two generated files:

   ```sh
   make makesum
   make modcargo-gen-crates | grep '^MODCARGO' > crates.inc   # the crate list first
   make makesum                                               # its crates' checksums
   make modcargo-gen-crates-licenses | grep '^MODCARGO' > crates.inc.new && mv crates.inc.new crates.inc
   ```

   Rename the patch to the new version (`make update-patches` writes it again), copy
   `crates.inc`, `distinfo` and `patches/` back here, and run the CI job.
2. **Fork** <https://github.com/jasperla/openbsd-wip> on GitHub (one click, *Fork*).
3. **Build and check it on OpenBSD** (7.9 or -current, amd64):

   ```sh
   doas pkg_add rust cargo-generate-vendor oniguruma sqlite3
   cd /tmp && ftp https://cdn.openbsd.org/pub/OpenBSD/$(uname -r)/ports.tar.gz
   cd /usr && doas tar xzf /tmp/ports.tar.gz        # skip if /usr/ports is there
   cd /usr/ports && git clone https://github.com/<you>/openbsd-wip
   echo 'PORTSDIR_PATH=${PORTSDIR}:${PORTSDIR}/openbsd-wip' | doas tee -a /etc/mk.conf
   cp -R /path/to/coxswain/packaging/openbsd/port/sysutils/coxswain openbsd-wip/sysutils/
   cd openbsd-wip/sysutils/coxswain
   make package
   make port-lib-depends-check      # prints nothing when WANTLIB is right
   make update-plist && git diff pkg/PLIST   # no change
   /usr/ports/infrastructure/bin/portcheck
   doas make install
   coxswain --version && man -w coxswain
   doas make clean=all
   ```

   Fix what `portcheck` or the library check reports, here and in your copy.
4. **Commit and push** in openbsd-wip's style (the port's path, then what it is):

   ```sh
   cd /usr/ports/openbsd-wip
   git checkout -b coxswain
   git add sysutils/coxswain
   git commit -m "sysutils/coxswain: new port, two-panel file manager for the terminal"
   git push -u origin coxswain
   ```

5. **Open the pull request** from your fork's `coxswain` branch to `jasperla/openbsd-wip`
   `master`, with your own short text: what it is, that only the terminal app is built, how
   you tested it (step 3 and the CI link), that you are upstream, and the sentence about the
   assistant. openbsd-wip also takes direct commits from people with write access: their
   README says to ask for it if you want it.
6. **Mail ports@openbsd.org** once it builds cleanly there. The porting guide: *"Submit the
   port. Create a gzipped tarball of the port directory. You can then either place it on a
   public HTTP server, sending its URL to ports@openbsd.org, or send the port MIME encoded to
   the same address."* and *"Mail ports@openbsd.org with a description, the homepage (if any),
   and a short note asking for comments and testing."* Make the tarball from the ports tree,
   so it unpacks as `sysutils/coxswain`:

   ```sh
   cd /usr/ports/openbsd-wip
   tar czf /tmp/coxswain-2.8.1.tgz sysutils/coxswain
   ```

   Subject in their form: `NEW: sysutils/coxswain`. In the mail, in plain text: the `DESCR`,
   the homepage, that you built and ran it on amd64 with `port-lib-depends-check` and
   `portcheck`, the sentence about the assistant, and a request for comments and an OK.
   Subscribe to ports@ first (<https://www.openbsd.org/mail.html>) to see the answers.
7. **Answer the review.** A developer who gives an OK imports it into CVS. Then add a line to
   openbsd-wip's `FINISHED` (`sysutils/coxswain: ready for import, sent to ports@ (<you>)`)
   while you wait, and remove the port from openbsd-wip once it is in the tree, as its README
   asks.

## After it is in

New releases go to ports@ as a diff against the tree, not through openbsd-wip: step 1, then
`cvs diff -uNp` in `/usr/ports/sysutils/coxswain`, mailed with the subject
`UPDATE: sysutils/coxswain 2.8.1 -> <version>`. Keep this folder the same as the tree's copy.
