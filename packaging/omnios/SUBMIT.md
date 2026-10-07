# Submitting Coxswain to omnios-extra

The recipe is [`coxswain/`](coxswain/) in this folder: `build.sh`, `local.mog` and the SMF
manifest template in `files/`. It builds the terminal app from the GitHub release tarball with
`ooce/developer/rust` and makes `ooce/application/coxswain`:

| Path | What |
|---|---|
| `/opt/ooce/bin/coxswain`, `/opt/ooce/bin/cox` | The program and its short name (a link) |
| `/opt/ooce/share/man/man1/coxswain.1` | The manual page |
| `/lib/svc/manifest/application/coxswain-index.xml` | The search helper's SMF service, `application/coxswain-index`, disabled until a user is named |

The pull request is opened by hand, from your account. Nothing here opens it, and the
announcement drafts are kept locally by the owner.

CI (`.github/workflows/omnios-extra.yml`) builds this recipe with
omnios-extra's own build system in an OmniOS VM whenever the recipe changes, installs the
package from the local repository it makes, and runs it. See *What is tested* below.

## AI-written work: read their rule first

omnios-extra's [CONTRIBUTING.md](https://github.com/omniosorg/omnios-extra/blob/master/CONTRIBUTING.md)
(read 2026-10-06) says, in short: using an AI is fine, but you must understand and be able to
defend every line; do not paste long AI-written explanations; they close pull requests that look
AI-orchestrated with no human behind them; and an AI agent must not open the pull request
itself. These files were drafted with an AI assistant (Claude). So:

- Read every line of `build.sh`, `local.mog` and the manifest, and build it yourself (step 3).
- Write the pull request text yourself, short, in your own words.
- Say in one sentence that an assistant helped draft the recipe and that you reviewed and
  built it.

They also ask whether a package is *generally useful* to OmniOS users, since they carry the
maintenance. Say why it is (a terminal file manager with ZFS snapshot browsing, for servers
you reach over SSH) and that you will keep it building. If they would rather not carry it,
take their answer: the install script still works.

## Steps

1. **Pick the release.** `VER` in `build.sh` must be a released version that builds on illumos
   (2.8.1 or later). Change it if a newer one is out.
2. **Fork and clone** <https://github.com/omniosorg/omnios-extra> on an OmniOS machine (the
   latest *bloody* release is what they ask for; r151058 works too):

   ```sh
   pfexec pkg install ooce/extra-build-tools ooce/developer/rust
   git clone https://github.com/<you>/omnios-extra && cd omnios-extra
   git checkout -b coxswain
   cp -r /path/to/coxswain/packaging/omnios/coxswain build/coxswain
   ```

3. **Build it once.** Their mirror does not have the source yet, so point the build at
   GitHub (`-M`) and skip the checksum (`-s`) for this first build:

   ```sh
   cd build/coxswain
   ./build.sh -b -M https://github.com/mwo-dk/coxswain/archive/refs/tags -s
   ```

   If the download fails, `build.log` shows the path it tried; the tarball must be reachable
   as `<mirror>/coxswain/v<VER>.tar.gz` (put it in `/tmp/m/coxswain/` and use `-M /tmp/m`).
   The package lands in `../../tmp.repo`. Install and try it:

   A local build is not signed, and `extra.omnios` asks for signatures by default, so allow
   unsigned packages on this build machine first (`verify` still checks signed ones):

   ```sh
   pfexec pkg set-publisher --set-property signature-policy=verify extra.omnios
   pfexec pkg install -g ../../tmp.repo ooce/application/coxswain
   /opt/ooce/bin/coxswain --version
   svcs application/coxswain-index
   ```

   Check that the licence is recognised: `../../tools/licence <the unpacked LICENSE>` should
   say MIT. If it does not, tell the maintainers rather than editing `doc/licences`.
4. **Add the package to their lists**, in the same branch:
   - `doc/packages.md`, in alphabetical order:
     `| ooce/application/coxswain	| 2.8.1		| https://github.com/mwo-dk/coxswain/releases | [mwo-dk](https://github.com/mwo-dk)`
   - `doc/baseline`: the line `extra.omnios ooce/application/coxswain` in order (copy the form
     of the lines around it).
5. **Commit and push** (their history uses short messages, e.g. `coxswain 2.8.1 (new package)`):

   ```sh
   git add build/coxswain doc/packages.md doc/baseline
   git commit -m "coxswain 2.8.1 (new package)"
   git push -u origin coxswain
   ```

6. **Open the pull request** to `omniosorg/omnios-extra` `master` with your own short text:
   what it is, that only the terminal app is built, the SMF service and that it is delivered
   disabled, how you tested it (step 3, and the CI link), that you are upstream and will keep
   it building, and the one sentence about the assistant.
7. **Answer the review.** Maintainers often adjust recipes themselves. They upload the source
   to `mirrors.omnios.org`; you do not.

## After it is in

New releases: bump `VER` in `build/coxswain/build.sh` and its line in `doc/packages.md`, build
once (step 3), and open a pull request titled `coxswain <version>`. Keep this folder the same as
their copy, apart from what they changed, so CI tests what they build.

## What is tested, and what is not

Tested in CI, on OmniOS r151058 (global zone): `build.sh -b` with omnios-extra's build system
from its `master`, the source from a local mirror, the package installed from the repository it
publishes to, `coxswain --version` and `cox --version`, the manual page, and the SMF service present and
disabled, then online for a named user with the helper's address file written.

Not tested: the *bloody* release, pkglint's full run as their CI does it, the aarch64 cross
build (`set_arch 64` builds amd64 only), and non-global zones.
