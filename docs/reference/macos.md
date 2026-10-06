[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# macOS

Both apps run on macOS, on Apple silicon and on Intel. This page covers what is
particular to a Mac: the privacy prompts macOS shows when Coxswain's search helper reads your
folders, what Coxswain never reads there, why a new version may ask again, the search helper at
login, and search by meaning on the Mac's GPU without any server.

## Contents

- [The folder prompts](#the-folder-prompts)
- [What Coxswain leaves out on a Mac](#what-coxswain-leaves-out-on-a-mac)
- [Full Disk Access](#full-disk-access)
- [Why a new version may ask again](#why-a-new-version-may-ask-again)
- [The search helper at login](#the-search-helper-at-login)
- [Search by meaning on the Mac's GPU](#search-by-meaning-on-the-macs-gpu)
- [Desktop app and terminal app](#desktop-app-and-terminal-app)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## The folder prompts

macOS guards some folders behind its privacy system (TCC, *Transparency, Consent and Control*).
The first time a program opens one, macOS asks you. Coxswain's search helper reads the names of
your files, and the text of those in your home folder, so it opens these folders once and macOS
may ask:

| Prompt | Why Coxswain opens it | What to answer |
|---|---|---|
| *"Coxswain" would like to access files in your Desktop folder.* (also Documents, Downloads) | To find your files there by name and by their text in Find | **Allow**, if you want them found. **Don't Allow** leaves them out; nothing else breaks |
| *"Coxswain" would like to access files on a removable volume.* / *on a network volume.* | A USB disk or a network share under `/Volumes` is in the name index | **Allow** to find files on it by name |
| *"Coxswain" would like to access data from other apps.* | Coxswain 2.1.1 and later never ask for this: see below | If an older version asks, **Don't Allow**, and update |

Each folder is asked for once. A folder you refused is not tried again while the helper runs,
so the prompt does not come back with every pass. macOS remembers your answer for this version
of the app (see [Why a new version may ask again](#why-a-new-version-may-ask-again)).

To change an answer later: **System Settings → Privacy & Security → Files and Folders →
Coxswain**, and tick or untick each folder. The search helper picks it up when it starts again
(log out and back in).

On the first start on a Mac, both apps show a notice once: *macOS may ask whether Coxswain can
open Desktop, Documents, Downloads, or a removable or network volume …*. In the desktop app it is
under *Settings → What's new*; in the terminal app it is the status line's notice.

## What Coxswain leaves out on a Mac

Coxswain's walks (the name index, search inside files, search by meaning, folder sizes, the
duplicate finder) never open these, and never look at a file inside them either:

| Left out | What it is |
|---|---|
| `~/Library`, except `~/Library/CloudStorage` and `~/Library/Mobile Documents` | Other apps' data: Containers, Group Containers, Mail, Messages, Safari, Application Support, Caches and the rest |
| `*.photoslibrary`, `*.photolibrary`, `*.migratedphotolibrary`, `*.aplibrary` | Photos, iPhoto and Aperture libraries |
| `*.musiclibrary`, `*.tvlibrary` | Music and TV libraries |
| `~/.Trash` | The Trash |
| `/System/Volumes` | The Data volume's second view of the whole disk (the same files as `/Users`, `/Applications` …) |
| `/private/var/folders` | Each user's temporary and cache folders |

The folders in `~/Library` keep their names in the name index (Find finds *Containers*), but
not what is inside them. iCloud Drive (`~/Library/Mobile Documents`) and the cloud apps' folders
(`~/Library/CloudStorage`: OneDrive, Dropbox, Google Drive) are walked by the
[cloud rules](../search/cloud-files.md): files that are only online are found by name and never
downloaded.

You can still open any of these folders yourself in a pane: the left-out list is for the walks,
not for browsing. Opening `~/Library/Containers` in a pane may raise *would like to access data
from other apps* then, because you asked to look.

Folder sizes (in a listing, and in Properties) do not count what is inside a
left-out folder below the one measured. Measuring `~/Library` gives the size without
`Containers` and the others; measuring `~/Library/Containers` itself counts it.

## Full Disk Access

Coxswain does not need it and never asks for it. Without it, the folders macOS guards stay
behind their prompts, and other apps' data stays out of reach, which is what Coxswain wants.
Giving Coxswain Full Disk Access in System Settings makes no difference to what it reads: the
list above is left out either way.

## Why a new version may ask again

macOS remembers a privacy answer for an app by its code signature. The release builds of
Coxswain are **ad-hoc signed** (signed with no developer identity, `signingIdentity: "-"`):
each new version has a different signature, so macOS takes it for a new app and may ask about
Desktop, Documents and Downloads again after an update. Your files are not read any differently;
answer as before.

A build signed with an Apple Developer ID certificate and notarised by Apple keeps the same
identity from version to version, and macOS keeps the answers. The release workflow signs and
notarises the desktop app when the project's Apple signing secrets are set
(`APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_ID`, `APPLE_PASSWORD`,
`APPLE_TEAM_ID`); without them it builds ad-hoc signed, as now, and says so in the workflow's
log.

Because the builds are not notarised, the first start of a downloaded `.dmg` may say the app
*is damaged*: run `xattr -cr /Applications/Coxswain.app` once (the Homebrew cask does it).

## The search helper at login

*Start with my session* (*Settings → Finding files → Details → Background reading*, or
`coxswain --index-service on`) writes a LaunchAgent,
`~/Library/LaunchAgents/dk.mwo.coxswain.index.plist`, and hands it to launchd with `launchctl
bootstrap gui/$(id -u)`. macOS then lists Coxswain under **System Settings → General → Login Items
& Extensions** as allowed in the background. Its errors go to `~/Library/Caches/coxswain/helper.log`.

When that switch is off, or a managed Mac does not allow background items, launchd does not start
the helper. The app asks launchd (`launchctl kickstart`) twice; if no helper answers within eight
seconds it starts one itself, which stays until you log out, and a notice says so once and where
to allow it. [The search helper](../search/helper.md#start-with-my-session-is-on-but-background-reading-does-not-start-why)

## Search by meaning on the Mac's GPU

Search by meaning needs no server on a Mac. The built-in model (multilingual-e5-small, 465 MB)
runs inside the search helper on the GPU through Metal on Apple silicon, and checks once that the
GPU's results match the processor's. Nothing leaves the machine but the one download of the
model from huggingface.co.

In the setup guide (**Ctrl+,** → **Set up…**, or `coxswain --setup-search`) with no Ollama or
LM Studio running, step 2 says *Apple silicon with 64 GB of shared memory and no model server:
the built-in model makes the vectors on its GPU (Metal) …*, *The built-in model* is chosen and
marked **recommended**, and step 3's **Download the built-in model (465 MB)** fetches it, turns
search by meaning on and says *Search by meaning is on, with the built-in model.* Ask is the
only part that needs a server (LM Studio or Ollama), and it is optional.
[The setup guide](../search/setup.md)

<!-- screenshot: macos-setup-builtin.png: the setup guide on a Mac with no server: step 2 with The built-in model marked recommended, step 3 with Search by meaning is on, step 4 Optional -->

When the model runs, a notice says *Search by meaning now uses your Mac's GPU (Metal): about 6×
faster*, or why it fell back to the processor.

## Desktop app and terminal app

The left-out list, the once-per-folder rule and the notice are the same in both (the logic is
in `coxswain-core`). Who macOS names in the prompt differs:

| App | The prompt names |
|---|---|
| Desktop app | *Coxswain*, for the app and for its search helper |
| Terminal app, run in a terminal | The terminal (Terminal, iTerm2 …): macOS asks on its behalf |
| Search helper started by *Start with my session* | The program the LaunchAgent starts: *Coxswain* or *coxswain* |

## Settings and config.toml

There is no key for the left-out list: it is always on, on a Mac. To leave out more, use
`name_exclude` (names) and `text_exclude` (text) as everywhere else
([Configuration](configuration.md)).

## Questions

#### Why does Coxswain want my Desktop, Documents and Downloads?
To find your files there by name and by their text, which is what search is for. Answer
**Don't Allow** and those folders are simply not searched; everything else works.

#### It kept asking to "access data from other apps". Why, and is it gone?
Versions before 2.1.1 walked `~/Library` with the rest of your home folder, and each touch of
another app's container could raise that prompt, again after every update. Since 2.1.1 the walks
leave `~/Library` out, but its cloud folders, and never open another app's data, so the prompt
does not come from the search helper any more.

#### Does Coxswain need Full Disk Access?
No, and it never asks. See [Full Disk Access](#full-disk-access).

#### After an update macOS asked about my folders again. Why?
The builds are ad-hoc signed, so each version looks like a new app to macOS. See
[Why a new version may ask again](#why-a-new-version-may-ask-again).

#### I refused a folder and want it searched now. What do I do?
**System Settings → Privacy & Security → Files and Folders → Coxswain**, tick the folder, then
log out and back in so the search helper starts again. A folder refused once is not tried again until the helper starts again.

#### Do I need Ollama for search by meaning on a Mac?
No. The built-in model runs on the Mac's GPU. See
[Search by meaning on the Mac's GPU](#search-by-meaning-on-the-macs-gpu).

#### Search inside files is on, but background reading does not start. What do I do?
Open the app once (2.1.2 or later): it starts the helper itself when launchd does not, and says
so. Then turn on **System Settings → General → Login Items & Extensions → Coxswain → Allow in the
Background**, and untick and tick *Start with my session*. To see what launchd did, run
`launchctl print gui/$(id -u)/dk.mwo.coxswain.index` (look at `state`, `runs` and `last exit
code`) and read `~/Library/Caches/coxswain/helper.log`. See [The search helper at login](#the-search-helper-at-login).

#### Can I search the files in `~/Library`?
Not through the index: it never reads other apps' data. Open the folder in a pane and type
the start of a name there ([quick search](../panels/quick-search.md)).

---
[← Previous: Termux on Android](termux.md) · [Next: Questions, collected →](../faq.md)
