[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Notices and the window title

The window's title says which version runs and which kinds of search are on. And once, in the
status line, both apps tell you of search you could turn on, of the git history once you are in
a repository, and after an update where to read what it brought. Nothing is shown twice.

<!-- screenshot: search-notice.png: desktop app, Cyber theme, the command line row at the bottom with the notice button "New: search by meaning finds files about your words, in any language. Turn it on" and its ×, and the window title "Coxswain 1.16.0 · search: names · text" in the title bar -->

## How to use it

**The title** needs nothing. It reads, for example:

```text
Coxswain 1.16.0 · search: names · text · meaning
```

| Part | Shown when |
|---|---|
| `names` | Always: the [name index](names.md) needs nothing |
| `text` | *Search inside files* is on (`search.text`) |
| `meaning` | [Search by meaning](meaning.md) is running in the helper |

The desktop app sets it on its window, and on Linux on the title bar GTK draws too (which
otherwise kept the title it was made with). The terminal app sets it as the terminal's title. Both
refresh it every few seconds, so turning a depth on or off shows there soon after.

**A notice** comes by itself:

| Key or click | Desktop app | Terminal app |
|---|---|---|
| Act on it | Click the notice: it opens the right Settings section (or the release notes) | Run the command it names |
| Dismiss it | Click **×** next to it | Nothing to do: shown once, it counts as seen |

## What you see

One notice at a time, in this order:

| Notice | When | Desktop app opens |
|---|---|---|
| *Search inside files has stopped: …* | A scan of the files failed; nothing further is read, and no vectors come, until one works. Dismissed, it comes back with another reason | *Settings → Search inside files* |
| *Search by meaning gets no vectors: …* | The server did not answer, or refused (a model that is not pulled) | *Settings → Search by meaning* ([servers](servers.md)) |
| *Updated to 1.16.0: see what's new* | The first start after an update (not the first start ever) | The release notes on GitHub |
| *New: Ctrl+G on a file or folder in a git repository shows its history, commit by commit* | A folder of a git repository has been opened (the key is yours from `[keys]`) | Nothing: it only tells ([Git history](../panels/git-history.md)) |
| *New: search by meaning finds files about your words, in any language. Turn it on* | Text search is on and has files, and search by meaning is off. Terminal app: *…in any language: coxswain --meaning on* | *Settings → Search by meaning* |
| *Ollama runs here: search by meaning could use its GPU. Choose it* | The built-in model is in use and Ollama answers on this machine. Terminal app: *…could use its GPU: coxswain --meaning ollama* | *Settings → Search by meaning* ([servers](servers.md)) |
| *Install tesseract to search the words in scans, screenshots and pictures* | Text search is on and the helper found no tesseract | *Settings → Search inside files* ([Scans](scans.md)) |

**Desktop app:** a button at the right of the command line row, after *Settings*, with **×**
beside it. While an [update is available](../reference/updates.md), its button takes the place and
the notice waits.

**Terminal app:** the text in the status line, once, when no dialog is open and the index is
ready. The update notice ends with the release notes' address,
`https://github.com/mwo-dk/coxswain/releases/latest`.

A notice dismissed or acted on is not shown again, in either app: both remember it in the shared
state file ([What the apps remember](../panels/session.md)).

## Settings and config.toml

None. Notices cannot be turned off; each shows once. The title cannot be changed.

## In the terminal app

The same notices and title. Differences: a notice names the command instead of a Settings
section, it is shown once rather than kept with a **×**, and the title is the terminal's (a
terminal that does not show titles shows nothing).

## Questions

#### Why does the title say "search: names" only?
*Search inside files* is off, or the helper cannot be reached. Turn it on in
*Settings → Search inside files*. `meaning` joins once [search by meaning](meaning.md) runs.

#### The title on Linux used to say just "Coxswain". Why the change?
GTK's title bar kept the title the window was made with. The desktop app now sets that bar's title
too, so the version and depths show on Linux as on macOS and Windows.

#### I dismissed a notice by mistake. Can I get it back?
Not from the app: dismissed notices are remembered in the state file. Each one's action is still
there: *Settings → Search by meaning*, `coxswain --meaning on`, and so on.

#### I use both apps. Will I see each notice twice?
No. Both read and write the same state file, so a notice seen in one is seen in the other.

#### Why did the Ollama notice appear?
Search by meaning runs on the built-in model on your CPU, and Ollama answered on
`http://localhost:11434`. It could make the vectors on its GPU, much faster: see
[servers](servers.md).

#### Where did the update notice go?
In the desktop app, while a newer version is available its button replaces the notice. In the
terminal app it was shown once in the status line.

#### Does the title show the search depth I am in?
No. It shows which depths are on, not which one Find file is at. Find file's own prompt or
highlighted button shows that.

---
[← Previous: Battery](battery.md) · [Next: Search settings →](settings.md)
