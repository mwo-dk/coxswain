# Languages

Both apps, the terminal app and the desktop app, speak 18 languages. They share one set of
translations, so they always agree.

| | Language | Code | | Language | Code |
|---|---|---|---|---|---|
| <img src="flags/gb.svg" width="24" alt=""> | English (United Kingdom) | `en-GB` | <img src="flags/de.svg" width="24" alt=""> | Deutsch | `de` |
| <img src="flags/au.svg" width="24" alt=""> | English (Australia) | `en-AU` | <img src="flags/fr.svg" width="24" alt=""> | Français | `fr` |
| <img src="flags/ca.svg" width="24" alt=""> | English (Canada) | `en-CA` | <img src="flags/it.svg" width="24" alt=""> | Italiano | `it` |
| <img src="flags/nz.svg" width="24" alt=""> | English (New Zealand) | `en-NZ` | <img src="flags/nl.svg" width="24" alt=""> | Nederlands | `nl` |
| <img src="flags/dk.svg" width="24" alt=""> | Dansk | `da` | <img src="flags/ar.svg" width="24" alt=""> | Español (Argentina) | `es-AR` |
| <img src="flags/se.svg" width="24" alt=""> | Svenska | `sv` | <img src="flags/es-ct.svg" width="24" alt=""> | Català | `ca` |
| <img src="flags/fi.svg" width="24" alt=""> | Suomi | `fi` | <img src="flags/es-pv.svg" width="24" alt=""> | Euskara | `eu` |
| <img src="flags/ee.svg" width="24" alt=""> | Eesti | `et` | <img src="flags/il.svg" width="24" alt=""> | עברית (Hebrew) | `he` |
| <img src="flags/lv.svg" width="24" alt=""> | Latviešu | `lv` | | | |
| <img src="flags/lt.svg" width="24" alt=""> | Lietuvių | `lt` | | | |

![Bosum in Danish](screenshots/gui-lang-da.png)

British English is the reference: every text is written in it first. Australian and New Zealand
English use British spelling and say "bin"; Canadian English keeps British spelling but says
"trash" and uses -ize where British uses -ise. Argentinian Spanish uses *vos*.

## Which language you get

By default (`language = "auto"`) Bosum follows your system's language settings, on Linux, macOS
and Windows alike. It takes the first of your system's languages that it has, in any regional
form, and otherwise the nearest relative:

| Your system | Bosum uses |
|---|---|
| One of the languages above, in any region (`de-CH`, `fr-CA`, `sv-FI`, `ca-ES-valencia`, …) | That language |
| US English, or English without a region | Canadian English |
| Australian, Canadian, New Zealand English | That English |
| Any other English (Ireland, South Africa, India, …) | British English |
| Any Spanish (Spain, Mexico, …) and Galician | Argentinian Spanish |
| Norwegian (Bokmål, Nynorsk) | Danish, the closest written language |
| Frisian, Afrikaans | Dutch |
| Swiss German dialects, Luxembourgish | German |
| Occitan | Catalan |
| Hebrew (also the old code `iw`) | Hebrew |
| Anything else | British English |

## Choosing a language

![The Settings window with every language and its flag](screenshots/gui-settings.png)

- **Desktop app:** open **Settings** (Ctrl+, or F9 → *Settings*, or start it with
  `bosum-gui --settings`) and click a language. Every
  language is listed by its own name, with its flag. The change applies at once, and is saved.
  **Automatic** goes back to following the system and shows which language that picks.
- **Config file:** `language = "da"` in `config.toml` (`bosum --config-path` shows where). Both
  apps read it; the terminal app picks it up the next time it starts.

## Right to left

![Bosum in Hebrew, mirrored](screenshots/gui-lang-he.png)

In Hebrew the desktop app mirrors its layout: the sidebar is on the right, text is aligned to
the right, and the back arrow points right. File names, paths and commands stay left to right
inside it, as Hebrew systems show them, and notes and commands take the direction of what you
type.

The terminal app is limited by the terminal: most terminals, Alacritty among them, do not do
right-to-left text, so Hebrew there shows its letters in reversed order. Terminals that support
it (for example Konsole, or GNOME Terminal with bidi on) show it correctly.

## What is translated

Everything Bosum itself says: menus, commands and the F-key bar, dialogs, the preview pane and
its facts, the duplicate finder, settings, status messages, errors, sizes (`KB`, or `Ko` in
French, with the decimal comma where the language uses one) and the age chips (`5m`, `2d`).
Numbers are written the way the language writes them (1 234,5 or 1.234,5).

Not translated: key names (Ctrl, Alt, Enter, F5 …), which are the same in the config file
everywhere; file and folder names; and error texts that come from the operating system, which
are in the system's own language.

## Improving a translation

The translations were written with care but have not all been checked by native speakers yet.
Basque, Latvian and Lithuanian need a native reader most, and short labels such as the F-key
bar ("Mkdir", "PullDn") are abbreviated to fit nine characters. Corrections are very welcome:

1. Each language is one file in [`crates/bosum-core/locales/`](../crates/bosum-core/locales/),
   for example `da.json`. It maps a key to its text:
   ```json
   "dupes.keep_newest": "Markér alle undtagen den nyeste",
   "items": { "one": "{n} element", "other": "{n} elementer" }
   ```
2. Change the text, keeping every `{placeholder}` (you may move it). Texts with a count have one
   entry per plural form your language uses: `one`/`other` for most, `one`/`few`/`other` for
   Lithuanian, `zero`/`one`/`other` for Latvian, `one`/`two`/`other` for Hebrew, and
   `one`/`many`/`other` for French, Italian, Spanish and Catalan.
3. Run `cargo test -p bosum-core i18n`: it checks that every file parses, has no unknown keys,
   keeps the placeholders and has an `other` form.
4. Open a pull request.

**Adding a language:** add its code, name and flag to `LANGUAGES` and `source()` in
[`crates/bosum-core/src/i18n.rs`](../crates/bosum-core/src/i18n.rs), map its system codes in
`nearest()`, give it plural rules in `plural()` if it needs other than one/other, copy
`en-GB.json` to the new file and translate it, and add the flag (from
[flag-icons](https://github.com/lipis/flag-icons)) to `docs/flags/` and the Settings window.

Flags: [flag-icons](https://github.com/lipis/flag-icons), MIT licence (`docs/flags/LICENSE`).
