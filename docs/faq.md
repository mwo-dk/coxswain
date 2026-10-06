[← README](../README.md) · [Docs index](README.md)

# Questions, collected

The questions people ask most, with a short answer and a link to the full one. Every feature page has its own *Questions* section with more.

## Coxswain 2.0

- **Coxswain 2.0 changed my config.toml: what happened?** On its first start it renamed seven keys to the words the apps use (`mkdir` → `new_folder`, `select_group` → `mark_group`, `[search] exclude` → `name_exclude` …), once, keeping your values, comments and order; a notice lists what it changed. [Answer](reference/configuration.md#renamed-in-20)
- **My `[keys] mkdir` stopped working.** It is `new_folder` since 2.0; an old name typed in after the first start is an unknown action. [Answer](customise/keys.md#my-keys-mkdir-stopped-working-in-20-why)
- **How do I see the first-run guide again?** Settings → Overview → *Show the guide again*, in both apps; or from Help: F1 → *Show the guide again* (desktop app), F1 then G (terminal app). [Answer](customise/settings.md#how-do-i-see-the-first-run-guide-again)
- **A notice says to install tesseract: how?** The notice ends with the command for your system (`sudo apt install tesseract-ocr`, `brew install tesseract`, `pkg install tesseract` …); click **Copy** (desktop app), or press Space on the row in Settings → Finding files (terminal app), and run it in a shell. Coxswain never runs it. [Answer](search/scans.md#a-notice-says-to-install-tesseract-how)
- **Why does `--settings=meaning` open the Overview now?** 2.0 takes only an area or an option's name: use `--settings=search_meaning` (and `ask_model`, `overview`, `search_cloud` for 1.x's `ask`, `news`, `cloud`). [Answer](customise/settings.md#why-does---settingsmeaning-open-the-overview-now)
- **Where did "Something went wrong" go?** Errors now say what failed (*Could not copy "report.pdf"*), the cause in one line, and every file with its reason under *Details*. [Answer](files/copy.md#where-did-something-went-wrong-go)
- **Why does the desktop F-key bar say Move, New folder and Commands?** 2.0 uses plain names in both apps; only the terminal app's bar keeps NC's *RenMov*, *Mkdir* and *PullDn*. [Answer](panels/the-screen.md#why-does-the-f-key-bar-say-move-in-one-app-and-renmov-in-the-other)

## Panels and keys

- **Why do Left and Right page instead of moving into folders?** That is Norton Commander's way; rebind `parent` and `open` in `[keys]`, and the desktop app's columns view already walks the tree. [Answer](panels/moving.md#why-do-left-and-right-page-instead-of-moving-into-folders)
- **Why does Alt+T not jump to names with a "t"?** Alt+T is bound to Colour tag, and a bound key does its action; unbind it with `tag = []` or start with another letter. [Answer](panels/quick-search.md#why-does-altt-not-jump-to-names-with-a-t)
- **Why did `+` not mark any folders?** Groups mark files only, as in NC; mark folders with Insert. [Answer](panels/marking.md#why-did--not-mark-any-folders)
- **Why is a folder's size not shown?** It is still being measured, you left the folder, measuring is off, or it is /proc, /sys, /dev or /run. [Answer](panels/folder-sizes.md#why-is-a-folders-size-not-shown)
- **I committed in a terminal and the git line did not change.** Both apps watch `.git` and follow a commit within a moment; where the file system sends no change events, press Ctrl+R. [Answer](panels/git.md#i-committed-in-a-terminal-and-the-git-line-did-not-change)
- **How do I see who changed a file, and when?** The Last commit column (desktop app) or the line under the panel (terminal app); Ctrl+G lists every commit. [Answer](panels/git-history.md#how-do-i-see-who-changed-a-file-and-when)
- **How do I get back an old version of a file?** Ctrl+G on it, Enter on the commit, F5 on the file. [Answer](panels/git-history.md#how-do-i-get-back-an-old-version-of-a-file)
- **Why does a file say "older" in Last commit?** It has not changed in the newest 5000 commits of its folder, where the walk stops; Ctrl+G shows when. [Answer](panels/git.md#why-does-a-file-say-older-in-last-commit)
- **How do I switch to another branch?** Alt+B lists the branches; Alt+S on one, then Enter. [Answer](panels/git-branches.md#how-do-i-switch-to-another-branch)
- **Why did the switch say my changes would be overwritten?** A file you changed differs between the branches; git refuses, and so does Coxswain. Commit or stash first. [Answer](panels/git-branches.md#why-did-the-switch-say-my-changes-would-be-overwritten)
- **What does "merging" on the git line mean?** git is in the middle of a merge (or rebase, cherry-pick, revert, bisect); finish or abort it. [Answer](panels/git.md#what-does-merging-or-rebasing-on-the-git-line-mean)
- **Does a file's history go past a rename?** Yes: it follows the file under its earlier names. [Answer](panels/git-history.md#does-a-files-history-go-past-a-rename)
- **Why is there a new tab in the left pane every time I start the desktop app?** The folder it started in is not open in any left-pane tab, so it is added; keep a tab on it (home, for menu starts). [Answer](panels/the-screen.md#why-is-there-a-new-tab-in-the-left-pane-every-time-i-start-the-desktop-app)
- **Where did "search: names · text" in the title go?** To Find's footer: *563 files indexed · text of 112 files · meaning for 112*. [Answer](panels/the-screen.md#where-did-search-names--text-in-the-title-go)
- **I bound a key and now the desktop app ignores my whole config. Why?** An unreadable key name makes the config invalid: the desktop app falls back to defaults, the terminal app refuses to start. [Answer](panels/keys.md#i-bound-a-key-and-now-the-desktop-app-ignores-my-whole-config-why)

## Tags, notes, favourites and the sidebar

- **I renamed a file and its colour tag is gone.** Tags and notes belong to a path, so a rename or move leaves them at the old path; tag it again. [Answer](organise/tags.md#i-renamed-a-file-and-its-tag-is-gone)
- **Does the terminal app have tags, notes, favourites or the sidebar?** No; Alt+T, Alt+N and Ctrl+B there say the feature is in the desktop app. [Answer](organise/tags.md#in-the-terminal-app)
- **Is my folder note saved while I type?** It is saved when you leave the field (Esc or a click elsewhere), so press Esc before closing. [Answer](organise/notes.md#is-my-note-saved-while-i-type)
- **I clicked + on a favourites group and nothing was added.** The + adds the folder you are in, not the one under the cursor, and never twice. [Answer](organise/favourites.md#i-clicked--and-nothing-was-added)
- **Deleting a favourites group asks nothing. Can I undo it?** No; the folders are untouched, add them to a new group. [Answer](organise/favourites.md#deleting-a-group-asks-nothing-can-i-undo-it)
- **I plugged in a USB stick and it is not in the sidebar.** Drives are read again every 30 seconds and when the window comes back to the front; mounts under /run (except /run/media) are left out, go there with Ctrl+L. [Answer](organise/sidebar.md#i-plugged-in-a-usb-stick-and-it-is-not-in-the-sidebar)
- **Why is my Desktop not under Places?** Only folders your system names (user-dirs.dirs on Linux) and that exist are listed, each once. [Answer](organise/sidebar.md#why-is-my-desktop-or-music-or-videos-not-under-places)

## Commands, the user menu and scripts

- **Can I run vim, less or ssh from the command line?** In the terminal app yes; in the desktop app no, commands get no input and their output is collected. [Answer](commands/command-line.md#can-i-run-vim-less-ssh-or-sudo-from-the-command-line)
- **The output flashed by in the terminal app. Where is it?** Ctrl+O shows the terminal with the last output; wait = true in the user menu holds it. [Answer](commands/command-line.md#the-output-flashed-by-in-the-terminal-app-where-is-it)
- **Why does cd - not work?** cd takes a folder; use Alt+Left (Back) in the desktop app. [Answer](commands/command-line.md#why-does-cd---not-work)
- **My user-menu entries replaced the git ones. How do I keep both?** Your [[user_menu]] replaces the whole list; copy the defaults from coxswain --dump-config. [Answer](commands/user-menu.md#my-entries-replaced-the-git-ones-how-do-i-keep-both)
- **Why does %f give only the name?** The command runs in the panel's folder; use %d/%f or %s for full paths. [Answer](commands/user-menu.md#why-does-f-give-only-the-name)
- **My script is in the F2 menu but nothing happens.** Make it executable with a #! line; the error is in the status line. [Answer](commands/scripts.md#my-script-is-in-the-menu-but-nothing-happens)
- **F4 in the desktop app does nothing, though editor = "hx" works in the terminal.** The desktop app has no terminal for a terminal editor; set a windowed editor or use $EDITOR for the terminal app. [Answer](commands/view-and-edit.md#f4-in-the-desktop-app-does-nothing-though-editor--hx-works-in-the-terminal-app)
- **Enter on a file does nothing, and there is no error.** The opener started but found no application for the type; set a default in your system settings. [Answer](commands/opening-files.md#enter-on-a-file-does-nothing-and-there-is-no-error)

## Search

- **Why doesn't a file I just saved show up in text search?** It is read within seconds, unless it is outside the folders read, too large, the laptop is on battery, or the helper still has a backlog. [Answer](search/text.md#why-doesnt-a-file-i-just-saved-show-up-in-text-search)
- **How do I search only this folder?** Press Ctrl+F (or Alt+F7) inside Find: the scope switches to "In <folder>", for names, words, meaning and Ask. [Answer](search/find-file.md#how-do-i-search-only-this-folder)
- **How do I search names only?** Tab once in Find, to the *Names* kind; a query with name syntax (`*.pdf`, `ext:md`) shows names only by itself. [Answer](search/find-file.md#how-do-i-search-names-only)
- **Why did my file show under About this?** It is close in meaning but lacks your words: another language, other words, a scan. [Answer](search/find-file.md#why-did-my-file-show-under-about-this)
- **Why does the order of the groups change in Find?** It follows what you typed: a word or two puts Names first, three words or a question puts In files and About this first. [Answer](search/find-file.md#why-does-the-order-of-the-groups-change)
- **Why is search by meaning off?** It needs a 465 MB model and CPU time, so you choose: Settings → Finding files → the level *Names, text and meaning*, or `coxswain --meaning on`. [Answer](search/meaning.md#why-is-search-by-meaning-off)
- **Why is search by meaning re-reading everything?** Once after the update to 1.39.0: whole documents get vectors now, not only their first 960 words, so the helper makes them anew in the background; the text is not read again. [Answer](search/meaning.md#why-is-search-by-meaning-re-reading-everything)
- **How do I search inside files, or ask?** Just type in Find (Ctrl+F): words in files are a group of their own, and Ctrl+Enter (terminal: Alt+Enter) asks. Shift+F7 opens Find at *In files* alone, Ctrl+F7 at *Ask*. [Answer](search/find-file.md#keys)
- **Why is there no Ask row for one word?** One word is almost always a name; type a second word, end with `?`, or press Ctrl+Enter. [Answer](search/find-file.md#why-is-there-no-ask-row-for-one-word)
- **How do I set up search by meaning and Ask?** The guided setup: **Set up…** in Settings → Overview or Finding files (terminal: `coxswain --setup-search`). It finds your model servers and says what suits your machine. [Answer](search/setup.md)
- **It says the model runs on the processor. What do I do?** Install your server's GPU build (Arch: `ollama-cuda` or `ollama-rocm`), or choose a Hybrid/NPU model in Lemonade. [Answer](search/setup.md#it-says-the-model-runs-on-the-processor-what-do-i-do)
- **Can I ask my files a question?** Yes: type it in Find and press Ctrl+Enter (terminal: Alt+Enter), with search by meaning on and a chat model on your server. [Answer](search/ask.md)
- **Why does Ask take long before the first word?** Mostly the model loading; a model that thinks first (Qwen3) is asked not to, so with the model loaded the first word comes in under a second. [Answer](search/ask.md#thinking)
- **The first answer takes long. Why?** The server loads the model on the first question; *Waiting for … to answer* shows until the first word. [Answer](search/ask.md#the-first-answer-takes-long-why)
- **Ask says my model only reads meaning and cannot answer.** The chat model is an embedding model such as `bge-m3`; choose a chat model such as `qwen3:8b`. [Answer](search/ask.md#it-says-my-model-only-reads-meaning-and-cannot-answer-why)
- **What does Ask send, and where?** [Answer](search/ask.md#what-is-sent-and-where)
- **Does the built-in model use my Mac's GPU?** On Apple Silicon, yes: through Metal, several times faster; Intel Macs stay on the CPU. Settings → Finding files → Details → Meaning and `coxswain --meaning` say which; *Use the CPU only* or `coxswain --meaning cpu` keeps it off the GPU. [Answer](search/meaning.md#does-the-built-in-model-use-my-macs-gpu)
- **Can Ollama or Lemonade make the vectors instead?** Yes: pick it under Settings → Finding files → Details → Meaning → *Made by*, or `coxswain --meaning ollama` / `--meaning server URL MODEL`. [Answer](search/servers.md#which-model-should-i-pick-on-a-server)
- **Will Coxswain download my OneDrive?** No: files only in OneDrive, Dropbox, Google Drive, Proton Drive or iCloud are found by name and left in the cloud until you open one. [Answer](search/cloud-files.md#will-coxswain-download-my-onedrive)
- **How do I search inside my Dropbox files?** Make the folder available offline in Dropbox, or read it anyway in Settings. [Answer](search/cloud-files.md#how-do-i-search-inside-my-dropbox-files)
- **How do I keep a folder's text out of the index?** Add it to Names only in Settings, or put an empty `.nosearch` file in it. [Answer](search/folders.md#how-do-i-make-a-folder-names-only)
- **How do I leave out files like `*.log`?** Add the pattern under Settings → Finding files → Details → Folders → Left out everywhere, or to `text_exclude`. [Answer](search/folders.md#how-do-i-leave-out-files-like-log-or-one-file)
- **Where is search.db, and can I look inside?** In the cache folder; Settings → Finding files → Details → Background reading → Show in panel opens it, and the preview or F3 shows its tables. It never indexes itself. [Answer](search/folders.md#where-is-searchdb-and-can-i-look-inside)
- **Is Coxswain running in the background after I close it?** The search helper stays ten minutes, or for good if started with your session. [Answer](search/helper.md#is-coxswain-running-in-the-background-after-i-close-it)
- **Search inside files and meaning stopped after an upgrade.** The helper registered with your session pointed at the removed program; from 1.29.0 the first app you open registers itself instead. [Answer](search/helper.md#search-inside-files-and-meaning-stopped-after-an-upgrade-why)
- **What does the number on the Settings button count?** Tips not dismissed and versions whose changes you have not read; a click opens Settings → Overview → What's new. [Answer](search/notices.md#what-does-the-number-on-the-settings-button-count)
- **I installed tesseract and nothing happened.** The helper looks for programs when it starts; let it start again. [Answer](search/scans.md#i-installed-tesseract-and-nothing-happened)
- **Does Find look inside my zip files?** Yes, in the archives of the folders read (your home folder by default, not hidden folders or caches): by name and by their text, and it follows their changes. [Answer](search/archives.md#does-find-look-inside-my-zip-files)
- **Why are the archives in ~/.m2 or ~/.cache not looked into?** They are programs' archives; tick *Archives everywhere* to find their entries by name. [Answer](search/archives.md#why-are-the-jars-in-m2-or-the-archives-in-cache-not-looked-into)
- **Why is a file in my .tar.xz not found?** A compressed tar over 256 MB is found by its own name only; there are limits per archive too. [Answer](search/archives.md#why-is-a-file-in-my-tarxz-not-found)
- **Can Find search commit messages?** Yes: commits of the repositories in the folders read are found under *History* in Find, and Enter opens the commit. [Answer](search/history.md)
- **Why does Find miss my latest commit?** Repositories are read at the helper's scans, at most ten minutes apart. [Answer](search/history.md#why-does-find-miss-my-latest-commit)
- **Why does a search for "browser entra" find my diagram?** Every arrow becomes a sentence such as "Browser to Entra ID: sign in.". [Answer](search/diagrams.md#why-does-a-search-for-browser-entra-find-my-diagram-when-no-box-says-both)

## The preview pane

- **Space types a space instead of opening the preview.** The command line has text, so Space belongs to it; press Esc to clear it, or use F3. [Answer](previews/text-and-code.md#space-types-a-space-in-the-command-line-instead-of-opening-the-preview)
- **Why does an HTML page look broken or empty in the preview?** No script runs and nothing comes from the web, on purpose; Enter opens it in your browser. [Answer](previews/html.md#why-does-the-page-look-broken-or-empty)
- **Do I need LibreOffice to see a PowerPoint deck?** No, the slides are drawn in the app at once; LibreOffice only adds the exact view. [Answer](previews/office.md#do-i-need-libreoffice-to-see-a-powerpoint-deck)
- **Why is a Word `.doc` file not shown?** `.doc` needs LibreOffice (installed or as an image); `.docx` needs nothing. [Answer](previews/office.md#why-is-a-word-doc-file-not-shown)
- **Why does LaTeX build with XeLaTeX here?** A `% !TEX program` line, a XeTeX package such as fontspec, or pdfLaTeX stopping asked for it. [Answer](previews/latex.md#why-does-latex-build-with-xelatex-here)
- **The LaTeX container never starts by itself.** Its 5 GB image is not pulled yet, and a pull waits for a click on Build PDF or Pull. [Answer](previews/containers.md#the-latex-container-never-starts-by-itself)
- **Can I preview a file inside an archive?** Yes: the cursor on it, with the preview pane open, or F3 in the terminal app; a copy is made in the cache folder. [Answer](previews/media.md#how-is-a-file-inside-an-archive-previewed)
- **Previewing a video made the window go blank or stop. Why?** GStreamer's good plugins are missing; from 1.29.1 the preview says so instead. [Answer](previews/media.md#previewing-a-video-made-the-window-go-blank-or-stop-why)
- **Why does a video not play?** The webview lacks the codec; on Linux install the GStreamer plugins. [Answer](previews/media.md#why-does-a-video-not-play)
- **How do I get the old F3 back for a BOM in the terminal app?** Press F3 (or `s`) again in the viewer, or set `bom_viewer = false`. [Answer](previews/bom.md#how-do-i-get-the-old-f3-back-in-the-terminal-app)

## Files
- **How do I get one file out of a zip without unpacking all of it?** Press Enter on the zip, go to the file and press F5: only that file is copied out. [Answer](files/archives.md#how-do-i-get-one-file-out-of-a-zip-without-unpacking-all-of-it)
- **Can the desktop app and the terminal app change one archive at once?** Yes: one waits for the other, so neither change is lost. [Answer](files/archives.md#can-i-change-one-archive-from-the-desktop-app-and-the-terminal-app-at-once)
- **Is it safe to change an archive?** Yes: it is written anew into a `.coxswain-tmp` file next to it and only then renamed over the old one; a failure leaves it as it was. [Answer](files/archives.md#is-it-safe-to-change-an-archive-what-if-the-power-goes-off)
- **Can I pack into a zip or 7z with a password?** Yes: Alt+F5, then type it twice; AES-256, and a 7z can hide its file names too. [Answer](files/archive-passwords.md#locking-a-new-archive)
- **Which encryption is used when packing with a password?** AES-256 for zip (AE-2) and 7z; ZipCrypto is never written. [Answer](files/archive-passwords.md#which-encryption-is-used)
- **Does Coxswain save my archive passwords?** No: a password lives in the running app's memory until it closes and is never written to disk. [Answer](files/archive-passwords.md#does-coxswain-save-my-archive-passwords)
- **Which format should I pick when packing?** Zip to share, 7z for the smallest and to hide the names, `.tar.gz` on Linux and macOS; Alt+F5 has a *Format* list (Tab in the terminal app). [Answer](files/pack-and-extract.md#which-format-should-i-pick)
- **Can I make a .tar.gz?** Yes: choose *tar.gz* in the Alt+F5 dialog, or press Tab in the terminal app's prompt until the name ends in `.tar.gz`. [Answer](files/pack-and-extract.md#can-i-make-a-targz)
- **Why does opening a .rar do something else?** RAR is not supported (its format may only be read with RAR's own code, under its own licence), so Enter hands it to your system's program. [Answer](files/archives.md#why-does-opening-a-rar-do-something-else)
- **How do I overwrite a file that exists?** Coxswain never overwrites; delete the old one first or copy under another name. [Answer](files/copy.md#how-do-i-overwrite-a-file-that-exists)
- **How do I stop Coxswain asking before deleting?** Untick Settings → Behaviour → Ask before deleting (`confirm_delete = false`); Shift+F8 then deletes for good at once. [Answer](files/delete.md#how-do-i-stop-coxswain-asking-every-time)
- **I cut files in Coxswain and pasted them elsewhere; why were they copied?** There is no common way to mark a cut on the clipboard, so other programs see a copy. [Answer](files/clipboard.md#i-cut-files-in-coxswain-and-pasted-them-in-my-other-file-manager-they-were-copied)

## Customising

- **I changed the theme and the terminal app did not change.** The two apps have a theme each: in Settings → Looks, switch *For* to *Terminal app* (or set the top-level `theme`), then restart the terminal app; its own Settings (**F9** → *Settings* → *Looks*) changes it at once. [Answer](customise/settings.md#i-changed-the-theme-and-the-terminal-app-did-not-change)
- **Does the terminal app have Settings?** Yes: **F9** → *Settings*, or `coxswain --settings`. The same areas and options as the desktop app, full screen; Space flips, Enter types, saved at once. [Answer](customise/settings.md#how-do-i-change-a-setting-in-the-terminal-app)
- **Will Settings mess up my hand-written config?** No: it changes the one value in place and keeps every comment and other key. [Answer](customise/settings.md#will-settings-mess-up-my-hand-written-config)
- **The icons are empty boxes.** No Nerd Font is installed (desktop *Icon font*) or set in the terminal; install one or choose *Plain characters (ASCII)*. [Answer](customise/glyphs-and-fonts.md#the-icons-are-empty-boxes)
- **Why is the font I picked not used?** The Windows, Mac, Cyber and NC themes bring their own font; *Font* applies to the modern themes. [Answer](customise/glyphs-and-fonts.md#why-is-the-font-i-picked-not-used)
- **I set `[themes.cyber.panel]` to tweak Cyber, and everything else turned blue.** Your table replaces the built-in Cyber and unset slots take NC's colours; copy the whole theme from `coxswain --dump-config`. [Answer](customise/own-theme.md#i-set-themescyberpanel-to-tweak-cyber-and-everything-else-turned-blue)
- **How do I keep the default key and add my own?** List both in `[keys]`: listing an action replaces all its defaults. [Answer](customise/keys.md#how-do-i-keep-the-default-key-and-add-my-own)
- **My system is in US English. Why does Coxswain say "colour" but "trash"?** US English gets Canadian English: British spelling, North American words. [Answer](customise/languages.md#my-system-is-in-us-english-why-does-coxswain-say-colour-but-trash)
- **A word in Polish, Czech, Ukrainian or Greek reads wrong. Where do I say so?** These four are new; a GitHub issue or a pull request on `crates/coxswain-core/locales/<code>.json` is welcome. [Answer](customise/languages.md#improving-a-translation)
- **The colours in my terminal look washed out, or wrong.** The themes use exact RGB and need a true-colour terminal; enable it in tmux. [Answer](customise/themes.md#the-colours-in-my-terminal-look-washed-out-or-wrong)

## Reference

- **What can the terminal app not do, and why?** No preview pane, tabs, sidebar, tags, clipboard, batch rename or duplicates: a terminal cannot draw them or has no room; their keys say "is available in the desktop app". [Answer](reference/terminal-app.md#what-only-the-desktop-app-has)
- **How do I turn on search by meaning without the desktop app?** `coxswain --meaning on` or `coxswain --setup-search`, then Find: files found by meaning are under *About this*. [Answer](reference/terminal-app.md#how-do-i-turn-on-search-by-meaning-without-the-desktop-app)
- **How do I find out which version I have?** `coxswain --version`, or the title of Help (F1) and the window title in the desktop app. [Answer](reference/command-line-flags.md#how-do-i-find-out-which-version-i-have)
- **I edited config.toml and nothing changed.** Both apps read it at start (the desktop app also after Settings); restart, and `[search]` needs a new helper. [Answer](reference/configuration.md#i-edited-configtoml-and-nothing-changed)
- **I misspelt a key and Coxswain said nothing.** Only unreadable files are refused; an unknown setting name is ignored and the default stays. [Answer](reference/configuration.md#i-misspelt-a-key-and-coxswain-said-nothing)
- **Does anything leave my machine?** Only the daily update check, a model download, a remote meaning server, image pulls and web pictures in previews you open. [Answer](reference/privacy.md#does-anything-leave-my-machine)
- **How do I see what a new version brought?** Click the count on Settings in the desktop app (Settings → Overview → What's new), or run `coxswain --whats-new`. [Answer](search/notices.md#how-do-i-see-what-an-upgrade-brought)
- **Does Coxswain update itself?** No; it tells you once a day, with the command for your package manager. [Answer](reference/updates.md#does-coxswain-update-itself)
- **Where are the licences of what Coxswain is built from?** In `THIRD-PARTY-NOTICES.md`: next to `LICENSE` in the terminal archive, in the desktop app's install folder, and on every release's Assets list, with an SBOM per app and a CBOM. [Answer](reference/bills-of-materials.md#where-are-the-licences-of-the-apps-i-installed)
- **Why does Coxswain's CBOM list RC4 and MD5, rated broken?** It uses them only to read old encrypted PDFs for search inside files, never to protect anything; the CBOM says so with each one. [Answer](reference/bills-of-materials.md#why-are-rc4-and-md5-in-the-cbom-rated-broken)
- **Does Coxswain run on FreeBSD?** Yes, both apps on FreeBSD 14 and 15 (amd64), installed with one line: `fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh`. The desktop app is experimental there. [Answer](reference/freebsd.md)
- **Why is the desktop app experimental on FreeBSD?** Tauri does not support FreeBSD officially; two crates are patched until their fixes are released. Your files are handled by the same tested core. [Answer](reference/freebsd.md#the-desktop-app-what-experimental-means)
- **How do I get back a file from a ZFS snapshot?** **Alt+Z** in its folder lists the dataset's snapshots; **Enter** on one from before, then **F5** on the file copies it out. No root needed; nothing is rolled back. [Answer](files/zfs-snapshots.md#how-do-i-get-back-a-file-i-deleted-or-an-older-version-of-it)
- **Which package does this file belong to?** **Alt+Enter** on it shows *Package git-2.56.0* on FreeBSD; *F9 → Files of this package* lists all its files. [Answer](files/properties.md#zfs-packages-and-file-flags)
- **Can I set chflags from Coxswain?** Your own user flags (`nodump`, `hidden`, and `uchg`, `uappnd` off ZFS) in Properties, or *F9 → File flags* in the terminal app. System flags are root's. [Answer](files/properties.md#zfs-packages-and-file-flags)
- **Where are my boot environments and jails?** In the desktop app's sidebar, and under **Alt+F1** in the terminal app; mounted ones open as folders. [Answer](reference/freebsd.md#boot-environments-and-jails)
- **How do I start the search helper on FreeBSD without a desktop session?** The rc.d script: `doas sysrc coxswain_index_enable=YES coxswain_index_user=$USER` and `doas service coxswain_index start`, or one line in `~/.profile`. [Answer](reference/freebsd.md#the-search-helper)
- **The cache folder is large. What takes the room?** Mostly search.db and the 488 MB model, then previews/. [Answer](reference/where-things-are-kept.md#the-cache-folder-is-large-what-takes-the-room)

---
[← Previous: FreeBSD](reference/freebsd.md) · [Next: Docs index →](README.md)
