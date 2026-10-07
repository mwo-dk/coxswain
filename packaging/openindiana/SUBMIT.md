# Submitting Coxswain to oi-userland

**Owner steps, in order:** [termux-packages](../termux/SUBMIT.md), [pkgsrc-wip](../pkgsrc/SUBMIT.md), [omnios-extra](../omnios/SUBMIT.md), **oi-userland**, [openbsd-wip](../openbsd/port/SUBMIT.md), [nixpkgs](../nix/SUBMIT.md). This is step 4 of 6; see [packaging/README.md](../README.md#submitting-by-hand).

The component is [`coxswain/`](coxswain/) in this folder. It goes to
`components/file/coxswain` in [oi-userland](https://github.com/OpenIndiana/oi-userland) and
makes `file/coxswain` (next to `file/mc`). It uses `BUILD_STYLE=cargo` on the **crates.io**
release, as `components/sysutils/zoxide` does: that crate is the terminal app alone with its
own `Cargo.lock`, so none of the desktop app's crates are vendored.

| Path | What |
|---|---|
| `/usr/bin/coxswain`, `/usr/bin/cox` | The program and its short name (a link) |
| `/usr/share/man/man1/coxswain.1` | The manual page |
| `/lib/svc/manifest/application/coxswain-index.xml` | The search helper's SMF service, disabled until a user is named |

`coxswain.license` is a copy of the repository's `LICENSE`, because the crate carries none.

The pull request is opened by hand, from your account; nothing here opens it. The announcement
drafts are kept locally by the owner.

**Untested:** no CI here builds this component; oi-userland's build needs a full OpenIndiana
build zone. What *is* tested is that the release archive built on OmniOS installs and runs on
OpenIndiana 2026.04 (the *illumos* workflow). So step 3 is the first real build; expect to fix
small things there.

## AI-written work

As of 2026-10-06, oi-userland has no written rule on AI-assisted contributions in its
README or CONTRIBUTING notes; check again. This component was drafted with an AI assistant
(Claude): read every line, build it yourself, write the pull request text yourself, and say in
one sentence that an assistant helped draft it.

## Steps

1. **Pick the release and its hash.** `HUMAN_VERSION` in the Makefile must be a version on
   crates.io (2.8.1 or later; `cargo search coxswain` shows the newest). Then:

   It is 2.8.1 now, with that crate's hash. For a newer one, from this repository's root:

   ```sh
   V=2.8.1
   sum=$(curl -sL https://static.crates.io/crates/coxswain/coxswain-$V.crate | sha256sum | cut -d' ' -f1)
   sed -i -e "s/^HUMAN_VERSION=.*/HUMAN_VERSION=\t\t$V/" \
     -e "s/^COMPONENT_ARCHIVE_HASH=.*/COMPONENT_ARCHIVE_HASH=\tsha256:$sum/" \
     packaging/openindiana/coxswain/Makefile
   ```
2. **Set up a build machine** with OpenIndiana Hipster, as oi-userland's
   [README](https://github.com/OpenIndiana/oi-userland#readme) and the
   [OI docs](https://docs.openindiana.org/dev/userland/) describe (`pkg install build-essential`,
   clone your fork, `gmake setup`).
3. **Build:**

   ```sh
   git checkout -b coxswain
   cp -r /path/to/coxswain/packaging/openindiana/coxswain components/file/coxswain
   cd components/file/coxswain
   gmake env-prep          # installs what the component needs, rustc among it
   gmake publish
   gmake REQUIRED_PACKAGES # fills in the run-time dependencies at the end of the Makefile
   gmake sample-manifest   # writes manifests/sample-manifest.p5m; compare with coxswain.p5m
   ```

   `gmake publish` also writes `pkg5`. Commit both generated files with the component, as other
   components do.
4. **Try it:** `pfexec pkg install -g <your build repo> file/coxswain`, then `coxswain --version`,
   `man coxswain` and `svcs application/coxswain-index`.
5. **Commit** in their style, `file/coxswain: new component, 2.8.1`, push, and open the pull
   request to `OpenIndiana/oi-userland` `oi/hipster` with your own text: what it is, what the
   package delivers, how you tested it, that you are upstream, and the sentence about the
   assistant.
6. **Mail oi-dev** once the pull request is open, if you want a review sooner.

## After it is in

New releases: change `HUMAN_VERSION` and `COMPONENT_ARCHIVE_HASH` (step 1), `gmake publish`,
and open a pull request `file/coxswain: update to <version>`. Keep this folder the same as their
copy.
