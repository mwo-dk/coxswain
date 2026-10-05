Coxswain is a two-pane file manager in the Norton Commander tradition. The terminal app shows git status in every panel, and the desktop app adds tabs, a sidebar, previews and themes. Both share Everything-style instant search.

## Download

Every file comes from this release's **Assets** list below.

| You have | Desktop app | Terminal app |
|---|---|---|
| Windows 10/11 | `Coxswain_…_x64-setup.exe` (or the `.msi`) | `coxswain-terminal-…-x86_64-pc-windows-msvc.zip` |
| Mac with Apple Silicon (M1 or newer) | `Coxswain_…_aarch64.dmg` | `coxswain-terminal-…-aarch64-apple-darwin.tar.gz` |
| Mac with Intel | `Coxswain_…_x64.dmg` | `coxswain-terminal-…-x86_64-apple-darwin.tar.gz` |
| Linux (Debian, Ubuntu) | `Coxswain_…_amd64.deb` | `coxswain-terminal-…-x86_64-unknown-linux-musl.tar.gz` |
| Linux (Fedora, openSUSE) | `Coxswain-…x86_64.rpm` | same as above |
| Linux (any distro) | `Coxswain_…_amd64.AppImage` | same; `aarch64-…` for ARM |
| FreeBSD 14, 15 (amd64) | `coxswain-desktop-…-x86_64-unknown-freebsd.tar.gz` (experimental) | `coxswain-terminal-…-x86_64-unknown-freebsd.tar.gz` |

On FreeBSD one line installs both, with the packages the desktop app needs:
`fetch -qo - https://raw.githubusercontent.com/mwo-dk/coxswain/master/install/install-freebsd.sh | sh`
([FreeBSD](https://github.com/mwo-dk/coxswain/blob/master/docs/reference/freebsd.md)).

The terminal app is a single file named `coxswain` (`coxswain.exe` on Windows). Put it anywhere on your `PATH`.

## What it is made of

`coxswain-terminal-…sbom.cdx.json` and `coxswain-desktop-…sbom.cdx.json` list every component of each app (CycloneDX SBOMs), `coxswain-….cbom.cdx.json` the cryptography both use (a CBOM, which either app shows as a rated tree), and `THIRD-PARTY-NOTICES.md` every component's licence; it also ships inside the apps. See [Licences and bills of materials](https://github.com/mwo-dk/coxswain/blob/master/docs/reference/bills-of-materials.md).

## The apps are not code-signed

Coxswain is free, and the builds are not signed with a paid certificate, so your system warns you the first time:

- **Windows:** SmartScreen says "Windows protected your PC". Click **More info**, then **Run anyway**.
- **macOS:** the app "can't be opened" or "is damaged". Drag Coxswain to Applications first, then run this once in Terminal:
  `xattr -cr /Applications/Coxswain.app`. Do the same for the terminal app's `coxswain` file.
- **Linux:** no warning. Make the AppImage executable (`chmod +x`) and run it.

## Build it yourself

You can build from source instead:

```sh
git clone https://github.com/mwo-dk/coxswain.git
cd coxswain
./install/install.sh          # Linux or macOS
```

On Windows:

```powershell
git clone https://github.com/mwo-dk/coxswain.git
cd coxswain
powershell -ExecutionPolicy Bypass -File install\install.ps1
```

The script offers to install Rust and Node.js if they are missing, and it asks before it does. See [INSTALL.md](https://github.com/mwo-dk/coxswain/blob/master/install/INSTALL.md) for details.

Git glyphs and file icons need a [Nerd Font](https://www.nerdfonts.com).
