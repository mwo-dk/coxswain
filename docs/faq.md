[← README](../README.md) · [Docs index](README.md)

# Questions

The questions people ask most, answered briefly. Each links to the full answer.

- [Previews](#previews)
- [Duplicates](#duplicates)
- [Cryptography bills of materials](#cryptography-bills-of-materials)
- [Languages](#languages)
- [Search and configuration](#search-and-configuration)

## Previews

**Why does F3 in the terminal app open a pager, not a preview?**
The terminal app has no preview pane. F3 opens the file in your viewer: `viewer` in
`config.toml`, else `$PAGER`, else `less`. A CycloneDX BOM is the exception: it opens in its own
viewer. [The preview pane](previews.md)

**A Word, LibreOffice or LaTeX file shows no preview. What does it need?**
A real program: LibreOffice for Office files, `latexmk`, `tectonic` or `pdflatex` for LaTeX,
`plantuml` for PlantUML. Coxswain uses one that is installed, or runs it in a podman or docker
container. The buttons at the top of the preview show which it found.
[Previews made by tools](previews.md#previews-made-by-tools)

**Does a preview ever send my file anywhere?**
No. Every library is bundled with the app, and containers run without a network
(`--network=none`) with your folder mounted read-only. Only pulling a container image downloads
anything, and the engine button says so first. [Safety](previews.md#safety)

## Duplicates

**Are the duplicates I remove gone for good?**
No. **Move marked to trash** sends the marked copies to the trash (the Recycle Bin on Windows),
after a confirmation, so you can restore them. Every group always keeps at least one copy.
[Using it](duplicates.md#using-it)

**Why is a resized photo not a duplicate of the original?**
Everything is compared by content, byte for byte. A photo that was resized, re-saved or edited
is a different file. [Limits](duplicates.md#limits)

**Can the terminal app find duplicates?**
Not yet: **Ctrl+D** is in the desktop app. The engine is shared and ready for the terminal app.
[Limits](duplicates.md#limits)

## Cryptography bills of materials

**Why is an RSA key "Unknown", when RSA is fine?**
Its rating depends on a key size the BOM does not give, and a rating is never guessed.
[Questions about BOMs](bom.md#questions)

**Why is a whole folder red when almost everything in it is green?**
A folder shows the worst rating beneath it; its details name the asset responsible.
[Questions about BOMs](bom.md#questions)

**Enter or "Found in" does not reach the source file.**
Files are looked for in the BOM's own folder, where scanners write it. A BOM copied elsewhere
finds nothing there. [Questions about BOMs](bom.md#questions)

**How do I get the old F3 back in the terminal app?**
**F3** again (or `s`) in the viewer opens the pager; `bom_viewer = false` makes F3 always do so.
[Questions about BOMs](bom.md#questions)

## Languages

**How do I change the language?**
In the desktop app, **Settings** (Ctrl+,), then click a language; it applies at once. For both
apps, `language = "da"` in `config.toml`; the terminal app picks it up when it next starts.
[Choosing a language](languages.md#choosing-a-language)

**My system is in US English. Why do I get Canadian English?**
Coxswain has no US English; US English, and English without a region, get the nearest one it
has, Canadian English. [Which language you get](languages.md#which-language-you-get)

## Search and configuration

**How do I search inside files, not only names?**
**Alt+F7** or **Ctrl+F** in both apps opens Find file; **Tab** goes to *Text in files*.
[Find file: deep search](../README.md#find-file-deep-search)

**Where is the config file?**
`coxswain --config-path` shows it (`~/.config/coxswain/config.toml` on Linux), and
`coxswain --dump-config` prints every option with its default.
[Configuration](../README.md#configuration)

---

← [Languages](languages.md) · [Docs index](README.md)
