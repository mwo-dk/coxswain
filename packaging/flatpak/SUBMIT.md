# Flathub: what stands in the way, and the owner's steps

The Flatpak in this folder builds and runs (the *Flatpak* workflow builds it from the checkout,
runs Flathub's linter on it, installs it and starts it). Putting it **on Flathub** is a
different matter. Two of Flathub's rules apply to Coxswain as it is today, and both are for
you to settle before anything is submitted.

## 1. Flathub's rules on AI-generated material

From [Flathub's requirements, *Generative AI policy*](https://docs.flathub.org/docs/for-app-authors/requirements#generative-ai-policy):

- **Flathub manifests must not contain AI-generated or AI-assisted content.** Disclosing it does
  not make it allowed. The manifest in this folder (`io.github.mwo_dk.Coxswain.yml`) was written
  with Claude, so it cannot be the one you submit, and neither can a copy of it. Flathub's
  manifest lives in its own repository (`flathub/io.github.mwo_dk.Coxswain`), apart from ours.
- **AI tools must not open or automate the submission pull request,** or write its description,
  its commit messages, review comments or replies. That is why there is no ready-made pull
  request text here: you write it yourself.
- **AI-generated code, documentation or packaging in the application itself must be disclosed**
  in the pull request, with the parts and the rough extent. Reviewers decide case by case and may
  reject a submission on it. For Coxswain that means saying how much of the code, the docs and
  the files the build installs (the desktop file and the metainfo in this folder) were written
  with AI.

## 2. Development history

[*Insufficient development history*](https://docs.flathub.org/docs/for-app-authors/requirements#insufficient-development-history):
an application must show a sustained history, real-world use and a commitment to maintenance;
one that has only existed for a short time "will generally not be accepted". The repository's
first commit is from 2026-09-25. Waiting some months, with releases and issues from users, makes
an acceptance far more likely than submitting now.

## Until then

Users can build the Flatpak themselves ([docs/reference/flatpak.md](../../docs/reference/flatpak.md#building-it-yourself)).
The app is ready for it: in a Flatpak it runs the editor, commands, scripts and preview programs
on the host with `flatpak-spawn --host`, keeps the search helper in the sandbox, registers
*Start with my session* as an XDG autostart entry, puts temporary files where the host can read
them and names `flatpak update` in the update notice.

## The owner's steps, when you decide to submit

1. **GitHub:** turn on two-factor authentication for `mwo-dk` (Flathub requires it before it
   gives you write access to the app's repository).
2. **App id:** `io.github.mwo_dk.Coxswain` follows [Flathub's id rules](https://docs.flathub.org/docs/for-app-authors/requirements#application-id)
   for a GitHub project (`io.github.<user>.<repo>`, the dash in `mwo-dk` as an underscore), and
   lets you verify the app by signing in to Flathub with the GitHub account that owns the
   repository. The app itself already uses it (the desktop file, the metainfo, the update hint,
   the autostart entry); a different id means changing those too.
3. **Write the manifest yourself.** What it has to achieve, which you can check against the
   app's code rather than against the manifest here:
   - the GNOME runtime 51 (WebKitGTK 4.1 for Tauri 2), the `rust-stable` and `node24` SDK
     extensions, the frontend (`gui`, `npm ci && npm run build`) built before
     `cargo build --release --locked -p coxswain-gui`, all offline;
   - the offline sources: `sh packaging/flatpak/sources.sh <checkout> <out>` runs Flathub's own
     generators (`flatpak-cargo-generator`, `flatpak-node-generator`, pinned) on the lock files;
     what they write is tool output, not AI output;
   - git built into `/app` (the app runs git with environment settings that `flatpak-spawn`
     would drop on the host);
   - `coxswain-gui` in `/app/bin`, the desktop file, the icons (`gui/src-tauri/icons`) and the
     metainfo (`python3 packaging/flatpak/metainfo.py > …metainfo.xml`) installed under the id;
   - the source as a `git` source at the release tag and its commit.
4. **Build and lint it locally** as Flathub's guide says:
   `flatpak run org.flatpak.Builder --force-clean --user --install --install-deps-from=flathub build io.github.mwo_dk.Coxswain.yml`,
   then `flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.mwo_dk.Coxswain.yml`
   and `… repo repo`. Expect two errors that need an exception from the reviewers:
   `finish-args-host-filesystem-access` and `finish-args-flatpak-spawn-access`.
5. **The permissions to justify** in the pull request, in your words. The facts:
   - `--filesystem=host`: a two-panel file manager; the panels, copy, move, delete, Find and
     previews work on any folder the user opens;
   - `--talk-name=org.freedesktop.Flatpak`: the user's editor (F4), shell commands, F2 scripts
     and user-menu commands run on the host (`tools::user_command`), and so do the user's
     installed preview and text readers found under `/run/host/usr` (tesseract, pdftoppm,
     LibreOffice, pandoc, LaTeX, PlantUML, podman or docker), stopped with `--watch-bus` on their
     time limits;
   - `--share=network`: a daily update check that can be turned off, plus the opt-in model
     download and servers for search by meaning;
   - `--socket=pulseaudio`: audio and video previews.
6. **Open the pull request** following [the submission guide](https://docs.flathub.org/docs/for-app-authors/submission):
   fork `flathub/flathub` with all branches, branch from `new-pr`, add the manifest and the two
   generated source files, push, open the pull request **against `new-pr`** titled
   `Add io.github.mwo_dk.Coxswain`, fill in Flathub's template yourself (description, a video of
   the Flatpak running, the AI disclosure, the history question), answer the reviewers, and
   comment `bot, build` for a test build when they are satisfied.
7. **After approval:** accept the invitation to `flathub/io.github.mwo_dk.Coxswain` within a
   week, then verify the app in Flathub's developer portal by signing in with GitHub as `mwo-dk`.

## How updates flow once it is listed

- Flathub's **External Data Checker** runs every two hours on the app's repository and opens a
  pull request when a source with `x-checker-data` has a new version (a git source can follow
  tags matching `^v([\d.]+)$`). Merging a pull request there makes the official build, which
  Flathub publishes.
- For Coxswain a new tag is not enough: `Cargo.lock` and `package-lock.json` change with most
  releases, so `cargo-sources.json` and `node-sources.json` must be made again for each one, or
  the offline build fails. Flathub's maintenance guide shows the usual way: a weekly workflow in
  the app's Flathub repository that runs the checker with `--edit-only`, then a script that
  regenerates the sources (here: `sources.sh` from the new tag), and opens one pull request, with
  `"disable-external-data-checker": true` in `flathub.json` so the global checker does not open
  a second one. That workflow is part of Flathub's packaging, so the same rule as for the
  manifest applies: you write it.
- Releases here are often several a day; Flathub asks for update checks no more often than about
  once a week, so Flathub will trail the releases page by up to a week.
- Users get the update with `flatpak update`, their software centre, or the command the app's
  update notice shows (`flatpak update io.github.mwo_dk.Coxswain`).
- The bundled git (`git-…tar.xz` in the manifest) is updated by hand in this folder; its
  `x-checker-data` lets the checker do it in Flathub's copy.
- GNOME 52 may drop WebKitGTK 4.1. When a newer runtime lacks it, the manifest stays on the
  last runtime that has it until Tauri moves to WebKitGTK 6, or builds WebKitGTK 4.1 itself.
