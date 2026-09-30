[← README](../README.md)

# Coxswain documentation

Every feature of both apps, the terminal app (`coxswain`) and the desktop app
(`coxswain-gui`), walked through in detail. The [README](../README.md) is the overview.

## Pages

Read in this order; each page links to the next.

| Page | What it covers |
|---|---|
| [The preview pane](previews.md) | Every format the desktop app previews, the switches at its top, previews made by tools (LaTeX, LibreOffice, PlantUML) in containers, columns and folder sizes, safety |
| [Finding duplicates](duplicates.md) | Duplicate files and folders by content, marking the extra copies, how a scan stays fast |
| [Cryptography bills of materials](bom.md) | CycloneDX CBOMs as a rated tree or sunburst in both apps: filters, reasons, "Found in", comparing two scans |
| [Languages](languages.md) | The 18 languages, which one you get, right to left, improving a translation |
| [Questions](faq.md) | The questions people ask most, with short answers and links to the full ones |

## Still in the README

These have no page of their own yet; the README walks through them:

- [Keys](../README.md#keys-defaults): the default keys of both apps.
- [Find file: deep search](../README.md#find-file-deep-search): names everywhere, text in
  files, scans and pictures, search by meaning, and the [query syntax](../README.md#query-syntax).
- [Settings](../README.md#settings) and [configuration](../README.md#configuration):
  `config.toml`, its options and where Coxswain keeps things.
- [Themes](../README.md#themes) and [layout](../README.md#layout).
- [How search stays fast](../README.md#how-search-stays-fast): the name index and the search
  helper.

## For contributors

- [Working on Coxswain](../CLAUDE.md): releases, pull requests, the rules for features, docs and
  screenshots.
- [Design: a CBOM viewer](design/bom-viewer.md): how the BOM viewer was planned and built.
- [Plan](PLAN.md) and [the desktop app's second plan](PLAN-gui-v2.md): how the apps were
  planned.
