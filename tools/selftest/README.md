# Self-test on a Mac

`macos.sh` tests a released Coxswain on a Mac that nobody can reach from outside: it downloads
the release into a folder of its own under `/tmp`, runs both apps there with a fake home folder,
and writes a Markdown report to bring back (users' side:
[Testing a release on your Mac](../../docs/systems/macos.md#testing-a-release-on-your-mac)).

```sh
curl -fsSL https://raw.githubusercontent.com/mwo-dk/coxswain/master/tools/selftest/macos.sh | bash
bash tools/selftest/macos.sh --version X.Y.Z      # from a clone, a given release
bash tools/selftest/macos.sh --help
```

It takes a few minutes, plus the checklist by hand at the end.

## What it does

| Part | Checked |
|---|---|
| Downloads | `coxswain-terminal-vX.Y.Z-<arch>-apple-darwin.tar.gz` and its `.sha256`, `Coxswain_X.Y.Z_<aarch64\|x64>.dmg`, `SHA256SUMS` (names as in `packaging/release-assets.sh`); the `.dmg` opened read-only with `hdiutil attach -nobrowse -readonly`, the app copied out |
| Signature | `codesign --verify`, `codesign -dv` (ad-hoc or Developer ID), `spctl -a -vv` (notarised or not), a quarantined copy and `xattr -cr` |
| Terminal app | `--version`, `cox --version`, `--help` against `docs/reference/command-line-flags.md` of that tag, `--paths` all in the fake home, `config.toml` read from there |
| Terminal app by keys | under `/usr/bin/expect`, 120 x 40: the first-run guide, F3 and `/` in the viewer, F5, F6 and Ctrl+Z, F7, Shift+F8, an archive opened with Enter, Alt+Enter on an ISO, Alt+C, Alt+B, Ctrl+G, Alt+K, Space, Alt+F7 Find, F12, F1 and Tab, F10; each step timed and with a time limit, so a hang is a FAIL |
| Search helper | the name index and the search store built in the fake home; Find finds a planted word in a file's text. Search by meaning is skipped: no model is ever downloaded |
| Desktop app | `--version`, starts and stays up 20 s, `--settings=search`, stopped with SIGTERM; screenshots if you agree; then 15 points to try by hand, your y/n answers in the report |
| Clean-up | stops only the processes with this run's fake home, removes the folder, reports anything of Coxswain's that changed or appeared in your real home |

F8 (to the Trash) is not tried: on a Mac that is your own Trash, and undo cannot take things back
out of it there. Shift+F8 deletes the demo file for good instead.

## What it never does

- touch your Coxswain `config.toml`, state or cache: every program runs with `HOME`,
  `CFFIXED_USER_HOME` and `XDG_CONFIG_HOME`, `XDG_CACHE_HOME`, `XDG_DATA_HOME` in the temporary
  folder, and a `config.toml` there that turns the update check off and keeps search inside it;
- `sudo`, or install anything (with `--brew` it installs the formula and the cask, after you say
  yes, and offers to remove them again);
- send anything anywhere: the only network use is the downloads from GitHub. The report stays on
  the Mac (on the Desktop when you say yes, else in the kept temporary folder);
- remove anything outside its own temporary folder. What macOS makes in your home for the test's
  app by its bundle id (`~/Library/WebKit/dk.mwo.coxswain` and the like), or anything of
  Coxswain's that shows up there, is only named in the report, with the `rm` line that removes it;
- run `launchctl`. Do not turn on *Start with my session* in the test copy: it would hand the
  test's search helper to your session's launchd. If it was turned on, the report says so (FAIL)
  and gives the `launchctl bootout` line that undoes it;
- link your cloud folders, unless you say yes for the Clouds check (links only, removed at the end).

## Options

| Option | Does |
|---|---|
| `--version X.Y.Z` | that release instead of the latest |
| `--brew` | the Homebrew formula and cask instead of the downloads: what is installed already is tested as it is; what is not is installed after you say yes (adding the tap `mwo-dk/coxswain`), and offered to remove at the end |
| `--keep` | keeps the temporary folder: fake home, downloads, logs |
| `--no-ask` | asks nothing: no screenshots, no checklist, the report to the Desktop |
| `--out DIR` | the report and screenshots to `DIR` |
| `--ci` | unattended on GitHub's macOS runners: `--no-ask`, screenshots, `--out` (default `./selftest-out`), the report in `$GITHUB_STEP_SUMMARY`, `::error` for each FAIL, exit 1 on any |
| `--terminal-bin PATH` | a local build of the terminal app instead of the download |
| `--dry-run` | on Linux: what is not macOS-specific (the script refuses other systems without it) |

## In CI

`release-check.yml`'s *macOS self-test* job runs `macos.sh --ci` after every release on
`macos-latest` (Apple silicon) and `macos-15-intel`, and uploads the report, screenshots and logs
as the `macos-selftest-<arch>` artifact.

## Checking the script

Bash 3.2 (what macOS ships): no associative arrays, `${x,,}` or `mapfile`.

```sh
bash -n tools/selftest/macos.sh && shellcheck tools/selftest/macos.sh
# On Linux, with a local build and expect (and tmux to look at a screen by hand):
cargo build --release -p coxswain
bash tools/selftest/macos.sh --dry-run --no-ask --terminal-bin target/release/coxswain
```

`EXPECT=/path/to/expect` picks another expect. A failed key step leaves the end of what the app
drew in `logs/tui-<step>.txt`, and the report quotes it.
