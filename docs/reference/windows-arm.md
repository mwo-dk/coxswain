[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# Windows on ARM

Both apps have native builds for Windows on ARM (`arm64`): laptops with a Snapdragon X or
another ARM processor, and Windows on ARM in a virtual machine on an Apple Silicon Mac. They are
the same apps as on an x64 PC, with every feature, built and tested on Windows 11 on ARM on every
change. Native means Windows runs them without x64 emulation.

## Contents

- [Which file to download](#which-file-to-download)
- [Installing](#installing)
- [From the x64 build to the ARM one](#from-the-x64-build-to-the-arm-one)
- [Search by meaning on ARM](#search-by-meaning-on-arm)
- [Questions](#questions)

## Which file to download

From the [releases page](https://github.com/mwo-dk/coxswain/releases/latest):

| App | File |
|---|---|
| Desktop app, installer | `Coxswain_<version>_arm64-setup.exe` |
| Desktop app, for managed installs | `Coxswain_<version>_arm64_en-US.msi` |
| Terminal app | `coxswain-terminal-<version>-aarch64-pc-windows-msvc.zip` |

*Settings → System → About* says *ARM-based processor* under *System type* on these machines.

## Installing

The installer works as the x64 one: it installs for you alone without asking for administrator
rights, adds Coxswain to the Start menu, and installs Microsoft's WebView2 if Windows lacks it
(Windows 11 has it). The builds are not code-signed: SmartScreen says *Windows protected your
PC*; click **More info**, then **Run anyway**.

For the terminal app, unpack the zip and put the folder with `coxswain.exe` and `cox.exe` on your
`PATH`.

**WinGet:** the terminal app is listed (`winget install mwo-dk.Coxswain.Terminal`); the desktop
app (`mwo-dk.Coxswain`) waits for Microsoft's review. Each release adds its ARM build beside the
x64 one, and WinGet takes the ARM one on an ARM machine by itself.

## From the x64 build to the ARM one

The x64 build runs on Windows on ARM too, under emulation. To change to the native one, uninstall
Coxswain (*Settings → Apps → Installed apps*), then run the ARM installer. Your settings, tags and
search index are in your user folders and stay ([Where things are kept](where-things-are-kept.md)).
In Task Manager's *Details* tab, the *Architecture* column says *ARM64* for the native app.

## Search by meaning on ARM

The built-in model runs on the processor, with ARM's vector instructions; it does not use the
NPU or the graphics card. A model server that does (one with Windows on ARM support, on this
machine or another) makes the vectors faster: *Settings → Finding files → Set up…* finds it
([Model servers](../search/servers.md)).

The usual Tesseract installer, for the words in scans, is x64; it runs under emulation and Coxswain finds
it in `C:\Program Files\Tesseract-OCR` as usual ([Scans and pictures](../search/scans.md)).

## Questions

#### Which installer do I need on a Snapdragon laptop?

`Coxswain_<version>_arm64-setup.exe`. The `_x64-setup.exe` works too, under emulation, but slower.

#### How do I know whether I am running the ARM build?

Task Manager, *Details* tab: the *Architecture* column shows *ARM64* for `coxswain-gui.exe` or
`coxswain.exe`. Add the column with a right-click on the column headings if it is not there.

#### Does search by meaning use the NPU?

The built-in model does not; it runs on the processor. A model server that uses the NPU or GPU
can do the work instead ([Model servers](../search/servers.md)).

#### Does Windows 10 on ARM work?

The builds target Windows 10 and 11 on ARM64; they are tested on Windows 11.

---
[← Previous: macOS](macos.md) · [Next: Questions, collected →](../faq.md)
