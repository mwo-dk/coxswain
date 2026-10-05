# Installing Coxswain

Coxswain is a file manager in the Norton Commander tradition. It comes in two parts:

- **`coxswain`**: the terminal app. It needs only Rust to build.
- **Coxswain desktop** (`coxswain-gui`): a window app with tabs, a sidebar, previews and themes.
  It also needs Node.js, plus a few system libraries on Linux.

The install script builds both from source and installs them for your user only; it never
needs admin rights except to add system libraries on Linux, and it asks first. If Rust or
Node.js is missing, it offers to install them and waits for your yes.

The first build takes a few minutes. After that, rebuilding is quick.

**Rather not build?** Ready-made installers for Windows, macOS and Linux are on the
[releases page](https://github.com/mwo-dk/coxswain/releases/latest).

**On FreeBSD** there is a script of its own that installs both apps from the release, with `pkg`
for what the desktop app needs: see [FreeBSD](../docs/reference/freebsd.md).

## Getting the code

Clone it with git, or unpack an archive you were sent:

```sh
git clone https://github.com/mwo-dk/coxswain.git
cd coxswain
```

To update later: `git pull`, then run the install script again.

## Linux and macOS

In the `coxswain` folder, run:

```sh
./install/install.sh
```

Options:

| Option | Effect |
|---|---|
| `--tui-only` | Build only the terminal app (skips Node.js and the system libraries) |
| `--yes` | Answer yes to every question |
| `PREFIX=/some/dir ./install/install.sh` | Install somewhere other than `~/.local` |

What you get:

- `~/.local/bin/coxswain`: the terminal app.
- `~/.local/bin/coxswain-gui`: the desktop app, plus a "Coxswain" entry in your application menu.
  On macOS the desktop app goes to `~/Applications/Coxswain.app` instead.

If `~/.local/bin` is not on your `PATH`, the script tells you the line to add.

### What the script may ask to install

- **Rust**, through the official installer from <https://rustup.rs>. It lives in `~/.cargo`
  and `~/.rustup`. You can remove it later with `rustup self uninstall`.
- **Linux system libraries for the desktop app.** The script shows the exact command before
  running it with `sudo`. It knows apt (Debian, Ubuntu, Mint), dnf (Fedora), pacman (Arch,
  Manjaro) and zypper (openSUSE). The main one is WebKitGTK.
- **Node.js**, from your package manager or Homebrew.
- **macOS:** Apple's Command Line Tools, if they aren't installed yet.

Say no to any of these and the script skips the desktop app, but still builds the terminal
app.

## Windows 10 and 11

In the `coxswain` folder, open PowerShell and run:

```powershell
powershell -ExecutionPolicy Bypass -File install\install.ps1
```

Add `-TuiOnly` for just the terminal app, or `-Yes` to skip the questions.

What you get:

- **Location:** both apps are installed into `%LOCALAPPDATA%\Programs\Coxswain`, and that folder
  is added to your user `PATH`. Open a new terminal to use `coxswain`.
- **Shortcut:** a "Coxswain" shortcut in the Start Menu opens the desktop app.

### What the script may ask to install

- **Rust**, through the official `rustup-init.exe`. When it offers to install the Visual
  Studio C++ Build Tools, say yes: Rust needs them on Windows.
- **Node.js LTS**, through `winget`.

The desktop app uses WebView2, which is already part of Windows 10 and 11.

## Afterwards

- **Nerd Font.** File icons and git symbols need a [Nerd Font](https://www.nerdfonts.com),
  such as "JetBrainsMono Nerd Font". In the terminal, set it as your terminal's font. The
  desktop app picks it up automatically once it's installed. Without one, add
  `glyphs = "ascii"` to the config.
- **Settings.** `coxswain --config-path` shows where the config file goes.
  `coxswain --dump-config` prints every option, including keys, colors and fonts.
- **Keys.** Norton Commander's: F3 view, F5 copy, F6 move, F7 new folder, F8 delete, F10
  quit, and Alt+F7 to search every file on the computer. F1 lists them all, and F9 opens a
  searchable list of every command.

## Building by hand

If you'd rather not use the script, run these in the folder:

```sh
cargo build --release -p coxswain            # -> target/release/coxswain
cd gui && npm ci && npx tauri build           # -> installers in target/release/bundle
```

The build prerequisites for each platform are listed at <https://v2.tauri.app/start/prerequisites/>.

## Uninstalling

- **Linux and macOS:** delete `~/.local/bin/coxswain` and `~/.local/bin/coxswain-gui`.
  - Linux: also delete `~/.local/share/applications/coxswain.desktop`.
  - macOS: also delete `~/Applications/Coxswain.app`.
- **Windows:** delete `%LOCALAPPDATA%\Programs\Coxswain` and the Start Menu shortcut.
- **Settings and search index:**
  - Linux: `~/.config/coxswain`, `~/.local/share/coxswain` and `~/.cache/coxswain`.
  - macOS: under `~/Library` (Application Support and Caches).
  - Windows: under `%APPDATA%` and `%LOCALAPPDATA%`.
