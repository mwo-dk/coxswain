[← README](../README.md) · [Docs index](README.md)

# Questions, collected

The questions people ask most, with a short answer and a link to the full one. Every feature page has its own *Questions* section with more.

## Panels and keys

- **Why do Left and Right page instead of moving into folders?** That is Norton Commander's way; rebind `parent` and `open` in `[keys]`, and the desktop app's columns view already walks the tree. [Answer](panels/moving.md#why-do-left-and-right-page-instead-of-moving-into-folders)
- **Why does Alt+T not jump to names with a "t"?** Alt+T is bound to Colour tag, and a bound key does its action; unbind it with `tag = []` or start with another letter. [Answer](panels/quick-search.md#why-does-altt-not-jump-to-names-with-a-t)
- **Why did `+` not mark any folders?** Groups mark files only, as in NC; mark folders with Insert. [Answer](panels/marking.md#why-did--not-mark-any-folders)
- **Why is a folder's size not shown?** It is still being measured, you left the folder, measuring is off, or it is /proc, /sys, /dev or /run. [Answer](panels/folder-sizes.md#why-is-a-folders-size-not-shown)
- **I committed in a terminal and the git line did not change.** A commit only changes `.git`, so nothing tells Coxswain; press Ctrl+R. [Answer](panels/git.md#i-committed-in-a-terminal-and-the-git-line-did-not-change)
- **Why is there a new tab in the left pane every time I start the desktop app?** The folder it started in is not open in any left-pane tab, so it is added; keep a tab on it (home, for menu starts). [Answer](panels/the-screen.md#why-is-there-a-new-tab-in-the-left-pane-every-time-i-start-the-desktop-app)
- **What does "search: names · text" in the title mean?** The kinds of search Find file can do now: names always, text when file text is kept, meaning when search by meaning runs. [Answer](panels/the-screen.md#what-does-search-names--text-in-the-title-mean)
- **I bound a key and now the desktop app ignores my whole config. Why?** An unreadable key name makes the config invalid: the desktop app falls back to defaults, the terminal app refuses to start. [Answer](panels/keys.md#i-bound-a-key-and-now-the-desktop-app-ignores-my-whole-config-why)

## Tags, notes, favourites and the sidebar

- **I renamed a file and its colour tag is gone.** Tags and notes belong to a path, so a rename or move leaves them at the old path; tag it again. [Answer](organise/tags.md#i-renamed-a-file-and-its-tag-is-gone)
- **Does the terminal app have tags, notes, favourites or the sidebar?** No; Alt+T, Alt+N and Ctrl+B there say the feature is in the desktop app. [Answer](organise/tags.md#in-the-terminal-app)
- **Is my folder note saved while I type?** It is saved when you leave the field (Esc or a click elsewhere), so press Esc before closing. [Answer](organise/notes.md#is-my-note-saved-while-i-type)
- **I clicked + on a favourites group and nothing was added.** The + adds the folder you are in, not the one under the cursor, and never twice. [Answer](organise/favourites.md#i-clicked--and-nothing-was-added)
- **Deleting a favourites group asks nothing. Can I undo it?** No; the folders are untouched, add them to a new group. [Answer](organise/favourites.md#deleting-a-group-asks-nothing-can-i-undo-it)
- **I plugged in a USB stick and it is not in the sidebar.** Drives are read when the window opens, and mounts under /run are left out; go there with Ctrl+L. [Answer](organise/sidebar.md#i-plugged-in-a-usb-stick-and-it-is-not-in-the-sidebar)
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
- **How do I search only this folder?** Press Tab once in Find file: "In <folder>" searches names in the active panel's folder and below. [Answer](search/find-file.md#how-do-i-search-only-this-folder)
- **Why is search by meaning off?** It needs a 465 MB model and CPU time, so you choose: Settings → Search by meaning, or `coxswain --meaning on`. [Answer](search/meaning.md#why-is-search-by-meaning-off)
- **Can I ask my files a question?** Yes: Tab to *Ask* in Find file, with search by meaning on and a chat model on your server. [Answer](search/ask.md)
- **What does Ask send, and where?** [Answer](search/ask.md#what-is-sent-and-where)
- **Can Ollama or Lemonade make the vectors instead?** Yes: pick it under Vectors made by, or `coxswain --meaning ollama` / `--meaning server URL MODEL`. [Answer](search/servers.md#which-model-should-i-pick-on-a-server)
- **How do I keep a folder's text out of the index?** Add it to Names only in Settings, or put an empty `.nosearch` file in it. [Answer](search/folders.md#how-do-i-make-a-folder-names-only)
- **Is Coxswain running in the background after I close it?** The search helper stays ten minutes, or for good if started with your session. [Answer](search/helper.md#is-coxswain-running-in-the-background-after-i-close-it)
- **I installed tesseract and nothing happened.** The helper looks for programs when it starts; let it start again. [Answer](search/scans.md#i-installed-tesseract-and-nothing-happened)
- **Why does a search for "browser entra" find my diagram?** Every arrow becomes a sentence such as "Browser to Entra ID: sign in.". [Answer](search/diagrams.md#why-does-a-search-for-browser-entra-find-my-diagram-when-no-box-says-both)

## The preview pane

- **Space types a space instead of opening the preview.** The command line has text, so Space belongs to it; press Esc to clear it, or use F3. [Answer](previews/text-and-code.md#space-types-a-space-in-the-command-line-instead-of-opening-the-preview)
- **Why does an HTML page look broken or empty in the preview?** No script runs and nothing comes from the web, on purpose; Enter opens it in your browser. [Answer](previews/html.md#why-does-the-page-look-broken-or-empty)
- **Do I need LibreOffice to see a PowerPoint deck?** No, the slides are drawn in the app at once; LibreOffice only adds the exact view. [Answer](previews/office.md#do-i-need-libreoffice-to-see-a-powerpoint-deck)
- **Why is a Word `.doc` file not shown?** `.doc` needs LibreOffice (installed or as an image); `.docx` needs nothing. [Answer](previews/office.md#why-is-a-word-doc-file-not-shown)
- **Why does LaTeX build with XeLaTeX here?** A `% !TEX program` line, a XeTeX package such as fontspec, or pdfLaTeX stopping asked for it. [Answer](previews/latex.md#why-does-latex-build-with-xelatex-here)
- **The LaTeX container never starts by itself.** Its 5 GB image is not pulled yet, and a pull waits for a click on Build PDF or Pull. [Answer](previews/containers.md#the-latex-container-never-starts-by-itself)
- **Why does a file inside an archive have no preview?** It is not on disk; copy it out with F5 and preview the copy. [Answer](previews/media.md#why-does-a-file-inside-an-archive-have-no-preview)
- **Why does a video not play?** The webview lacks the codec; on Linux install the GStreamer plugins. [Answer](previews/media.md#why-does-a-video-not-play)
- **How do I get the old F3 back for a BOM in the terminal app?** Press F3 (or `s`) again in the viewer, or set `bom_viewer = false`. [Answer](previews/bom.md#how-do-i-get-the-old-f3-back-in-the-terminal-app)

## Files
- **How do I get one file out of a zip without unpacking all of it?** Press Enter on the zip, go to the file and press F5: only that file is copied out. [Answer](files/archives.md#how-do-i-get-one-file-out-of-a-zip-without-unpacking-all-of-it)
- **Is it safe to change an archive?** Yes: it is written anew into a `.coxswain-tmp` file next to it and only then renamed over the old one; a failure leaves it as it was. [Answer](files/archives.md#is-it-safe-to-change-an-archive-what-if-the-power-goes-off)
- **Does Coxswain save my archive passwords?** No: a password lives in the running app's memory until it closes and is never written to disk. [Answer](files/archive-passwords.md#does-coxswain-save-my-archive-passwords)
- **How do I choose between zip, 7z and tar.gz when packing?** By the ending you type in the Alt+F5 dialog; every ending Coxswain reads can be packed. [Answer](files/pack-and-extract.md#how-do-i-choose-between-zip-7z-and-targz)
- **Why does opening a .rar do something else?** RAR is not supported (its format may only be read with RAR's own code, under its own licence), so Enter hands it to your system's program. [Answer](files/archives.md#why-does-opening-a-rar-do-something-else)
- **How do I overwrite a file that exists?** Coxswain never overwrites; delete the old one first or copy under another name. [Answer](files/copy.md#how-do-i-overwrite-a-file-that-exists)
- **How do I stop Coxswain asking before deleting?** Untick Settings → Behaviour → Ask before deleting (`confirm_delete = false`); Shift+F8 then deletes for good at once. [Answer](files/delete.md#how-do-i-stop-coxswain-asking-every-time)
- **I cut files in Coxswain and pasted them elsewhere; why were they copied?** There is no common way to mark a cut on the clipboard, so other programs see a copy. [Answer](files/clipboard.md#i-cut-files-in-coxswain-and-pasted-them-in-my-other-file-manager-they-were-copied)

## Customising

- **I changed the theme in Settings and the terminal app did not change.** The desktop app's theme is `[gui] theme`, the terminal app's is the top-level `theme`; set that and restart it. [Answer](customise/settings.md#i-changed-the-theme-in-settings-and-the-terminal-app-did-not-change)
- **Will Settings mess up my hand-written config?** No: it changes the one value in place and keeps every comment and other key. [Answer](customise/settings.md#will-settings-mess-up-my-hand-written-config)
- **The icons are empty boxes.** No Nerd Font is installed (desktop *Icon font*) or set in the terminal; install one or choose *Plain characters (ASCII)*. [Answer](customise/glyphs-and-fonts.md#the-icons-are-empty-boxes)
- **Why is the font I picked not used?** The Windows, Mac, Cyber and NC themes bring their own font; *Font* applies to the modern themes. [Answer](customise/glyphs-and-fonts.md#why-is-the-font-i-picked-not-used)
- **I set `[themes.cyber.panel]` to tweak Cyber, and everything else turned blue.** Your table replaces the built-in Cyber and unset slots take NC's colours; copy the whole theme from `coxswain --dump-config`. [Answer](customise/own-theme.md#i-set-themescyberpanel-to-tweak-cyber-and-everything-else-turned-blue)
- **How do I keep the default key and add my own?** List both in `[keys]`: listing an action replaces all its defaults. [Answer](customise/keys.md#how-do-i-keep-the-default-key-and-add-my-own)
- **My system is in US English. Why does Coxswain say "colour" but "trash"?** US English gets Canadian English: British spelling, North American words. [Answer](customise/languages.md#my-system-is-in-us-english-why-does-coxswain-say-colour-but-trash)
- **The colours in my terminal look washed out, or wrong.** The themes use exact RGB and need a true-colour terminal; enable it in tmux. [Answer](customise/themes.md#the-colours-in-my-terminal-look-washed-out-or-wrong)

## Reference

- **What can the terminal app not do, and why?** No preview pane, tabs, sidebar, tags, clipboard, batch rename, duplicates or Settings: a terminal cannot draw them or has no room; their keys say "is available in the desktop app". [Answer](reference/terminal-app.md#what-only-the-desktop-app-has)
- **How do I turn on search by meaning without the desktop app?** `coxswain --meaning on`, then Find file's text depth (Alt+F7, Tab, Tab). [Answer](reference/terminal-app.md#how-do-i-turn-on-search-by-meaning-without-the-desktop-app)
- **How do I find out which version I have?** `coxswain --version`, or the title of Help (F1) and the window title in the desktop app. [Answer](reference/command-line-flags.md#how-do-i-find-out-which-version-i-have)
- **I edited config.toml and nothing changed.** Both apps read it at start (the desktop app also after Settings); restart, and `[search]` needs a new helper. [Answer](reference/configuration.md#i-edited-configtoml-and-nothing-changed)
- **I misspelt a key and Coxswain said nothing.** Only unreadable files are refused; an unknown setting name is ignored and the default stays. [Answer](reference/configuration.md#i-misspelt-a-key-and-coxswain-said-nothing)
- **Does anything leave my machine?** Only the daily update check, a model download, a remote meaning server, image pulls and web pictures in previews you open. [Answer](reference/privacy.md#does-anything-leave-my-machine)
- **Does Coxswain update itself?** No; it tells you once a day, with the command for your package manager. [Answer](reference/updates.md#does-coxswain-update-itself)
- **The cache folder is large. What takes the room?** Mostly search.db and the 488 MB model, then previews/. [Answer](reference/where-things-are-kept.md#the-cache-folder-is-large-what-takes-the-room)

---
[← Previous: Security](reference/security.md) · [Next: Docs index →](README.md)
