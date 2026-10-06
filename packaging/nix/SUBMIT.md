# Submitting Coxswain to nixpkgs

What is here, and what the owner does to get `coxswain` (and later `coxswain-gui`) into
[NixOS/nixpkgs](https://github.com/NixOS/nixpkgs). Nothing has been submitted yet.

## First: nixpkgs' rule on automated contributions

nixpkgs' [automation/AI policy](https://github.com/NixOS/nixpkgs/blob/master/CONTRIBUTING.md#automationai-policy)
applies to this submission. The package files and this page were drafted with an AI assistant
(Claude). The policy allows that, on two conditions:

1. **A responsible person reviews it before submission and answers for it.** Read both
   `package.nix` files line by line until you could have written them, build them yourself,
   and answer reviewers' questions in your own words (not by passing them to a tool).
2. **The use is disclosed.** Say so in the PR description, for example: *"The package
   expression was drafted with an AI assistant (Claude) and reviewed, built and tested by me."*
   The PR template's checkbox *Follows the automation/AI policy* is ticked only when both hold.

Write the PR description yourself. The facts it needs are under [The pull request](#the-pull-request);
there is no ready-made text to paste.

## What is ready

| File | Goes to | Status |
|---|---|---|
| [`nixpkgs/coxswain/package.nix`](nixpkgs/coxswain/package.nix) | `pkgs/by-name/co/coxswain/package.nix` | Terminal app, v2.1.0. Built by the *Nix* workflow's *nixpkgs package* job |
| [`nixpkgs/coxswain-gui/package.nix`](nixpkgs/coxswain-gui/package.nix) | `pkgs/by-name/co/coxswain-gui/package.nix` | Desktop app, Linux. Same job. Submit it in a second PR, after the first is in |
| The maintainer entry below | `maintainers/maintainer-list.nix` | Fill in your handle |

The flake in this repository (`flake.nix`, `package.nix`, `gui.nix` here) builds from the
checkout, with no hashes. The nixpkgs files fetch a tagged release instead and carry three
hashes (`hash`, `cargoHash`, and for the desktop app `npmDeps.hash`), which change with every
version.

## The maintainer entry

In `maintainers/maintainer-list.nix`, in alphabetical order. nixpkgs prefers the handle to be
the GitHub name, so `mwo-dk` is the natural choice:

```nix
  <NIXPKGS-HANDLE> = {
    name = "Michael W. Olesen";
    github = "mwo-dk";
    githubId = 1458516;
    # email = "…";   # optional: only if you want an address listed publicly
  };
```

In both `package.nix` files, `maintainers = with lib.maintainers; [ ];` becomes
`maintainers = with lib.maintainers; [ <NIXPKGS-HANDLE> ];` (with `mwo-dk` the line reads
`[ mwo-dk ]`).

## Step by step

1. **Accounts.** Your GitHub account is enough; nixpkgs has no separate sign-up. Install Nix
   with flakes on (`experimental-features = nix-command flakes`) on a Linux machine or VM.
   (On this machine no Nix was installed; the files were built in GitHub Actions only.)
2. **Fork** [NixOS/nixpkgs](https://github.com/NixOS/nixpkgs) on GitHub and clone your fork
   (`git clone --depth 1 https://github.com/mwo-dk/nixpkgs`; a full clone is several GB).
   Branch from `master`: `git switch -c coxswain-init`.
3. **Commit 1, the maintainer:** add the entry above, check it with
   `nix-build lib/tests/maintainers.nix`, and commit with the title
   `maintainers: add <NIXPKGS-HANDLE>`.
4. **Commit 2, the package:** copy `nixpkgs/coxswain/package.nix` here to
   `pkgs/by-name/co/coxswain/package.nix`, put your handle in `maintainers`, and set the
   version to the latest release. For a version other than 2.1.0 the hashes change: set
   `hash` and `cargoHash` to `lib.fakeHash`, run the build, and copy the `got: sha256-…`
   values from the two errors into the file, one after the other.
5. **Build and test:**
   ```sh
   nix-build -A coxswain
   ./result/bin/coxswain --version
   ./result/bin/coxswain          # look around, F10 quits
   man ./result/share/man/man1/coxswain.1.gz
   nix fmt pkgs/by-name/co/coxswain/package.nix   # nixfmt, as nixpkgs' CI checks
   ```
   Commit with the title `coxswain: init at 2.1.0` (the version you package).
6. **Review it as nixpkgs will:** `nix run nixpkgs#nixpkgs-review -- rev HEAD`. It builds the
   package and everything that depends on it, and opens a shell with the result to try.
7. **Push and open the PR** against `NixOS/nixpkgs` `master`, titled `coxswain: init at 2.1.0`,
   with the description you write (below) and the template's checklist.
8. **Answer the review.** Reviewers often ask for small changes in style. Push fixes to the same
   branch, and squash them into the two commits when asked.
9. **The desktop app**, once `coxswain` is merged: the same steps with
   `nixpkgs/coxswain-gui/package.nix` at `pkgs/by-name/co/coxswain-gui/package.nix`, one commit
   `coxswain-gui: init at 2.1.0`, built with `nix-build -A coxswain-gui` and started with
   `./result/bin/coxswain-gui`. It has a third hash, `npmDeps.hash`, found the same way.

## The pull request

Title: `coxswain: init at 2.1.0`.

The facts for the description you write:

- What it is: a two-panel file manager in the Norton Commander tradition, for the terminal,
  with git status in the panels, quick and full-text search, previews; homepage
  <https://github.com/mwo-dk/coxswain>; MIT; you are the upstream author.
- What the package builds: the `coxswain` binary only (`-p coxswain` in the Cargo workspace,
  which also holds the desktop app), plus the `cox` link and the manual page.
- Tests: the `coxswain` and `coxswain-core` crates' tests run in `checkPhase`, with git (for
  the history and branch tests) and a writable home; `versionCheckHook` checks `--version`.
- `oniguruma` from nixpkgs (`RUSTONIG_SYSTEM_LIBONIG`), for the tokenizer of search by meaning.
- Platforms: Linux and Darwin (the upstream CI builds both).
- The disclosure of the AI assistance, as above.

The template's checklist, as it applies:

- Built on platform: tick the ones you built on (`x86_64-linux` at least; `aarch64-darwin` if
  you have a Mac with Nix).
- Tested: *Package tests* is not used (no `passthru.tests`); the crate tests run in the build.
- *Ran nixpkgs-review on this PR*: tick after step 6.
- *Tested basic functionality of all binary files*: `coxswain`, `cox`.
- *Fits CONTRIBUTING.md …* and *Follows the automation/AI policy*: tick when true.

## After acceptance: updates

You do not need to submit each release. **r-ryantm**, the bot of
[nixpkgs-update](https://github.com/nix-community/nixpkgs-update), watches GitHub releases of
packaged projects; after a new Coxswain release it opens a PR `coxswain: 2.1.0 -> 2.2.0` with
the version and all hashes updated and the build checked. As maintainer you are asked to review
it; a comment that you tested it helps it get merged. When a release needs more than new
hashes (a new system library, a changed test), the bot's build fails and you make that PR by
hand, the same way as above.

The flake in this repository needs no such step: it builds whatever commit it is given.
