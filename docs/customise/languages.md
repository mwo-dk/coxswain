[← README](../../README.md) · [Docs index](../README.md) · [Customising](README.md)

# Languages

Both apps, the terminal app and the desktop app, speak 22 languages. They share one set of
translations and one setting, so they always agree. By default Coxswain follows your system's
language.

![The desktop app in Danish: Hjem, Drev and Git-repositorier in the sidebar, Navn, Størrelse and Ændret as column headers, the F-key bar with Hjælp, Kopiér, Flyt and Afslut, and sizes with a decimal comma](../screenshots/gui-lang-da.png)
*Coxswain in Danish. Sizes are written 32,7 KB, with the decimal comma.*

## Contents

- [The languages](#the-languages)
- [How to use it](#how-to-use-it)
- [Which language Automatic picks](#which-language-automatic-picks)
- [What you see](#what-you-see)
- [Capitals in Greek](#capitals-in-greek)
- [Right to left](#right-to-left)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)
- [Improving a translation](#improving-a-translation)

## The languages

Both apps list them by region, and by their own name within a region:

| | Language | Code |
|---|---|---|
| | **Nordic and Baltic** | |
| <img src="../flags/dk.svg" width="24" alt=""> | Dansk | `da` |
| <img src="../flags/ee.svg" width="24" alt=""> | Eesti | `et` |
| <img src="../flags/lv.svg" width="24" alt=""> | Latviešu | `lv` |
| <img src="../flags/lt.svg" width="24" alt=""> | Lietuvių | `lt` |
| <img src="../flags/fi.svg" width="24" alt=""> | Suomi | `fi` |
| <img src="../flags/se.svg" width="24" alt=""> | Svenska | `sv` |
| | **Western Europe** | |
| <img src="../flags/es-ct.svg" width="24" alt=""> | Català | `ca` |
| <img src="../flags/de.svg" width="24" alt=""> | Deutsch | `de` |
| <img src="../flags/gb.svg" width="24" alt=""> | English (United Kingdom) | `en-GB` |
| <img src="../flags/es-pv.svg" width="24" alt=""> | Euskara | `eu` |
| <img src="../flags/fr.svg" width="24" alt=""> | Français | `fr` |
| <img src="../flags/it.svg" width="24" alt=""> | Italiano | `it` |
| <img src="../flags/nl.svg" width="24" alt=""> | Nederlands | `nl` |
| | **Central and Eastern Europe** | |
| <img src="../flags/cz.svg" width="24" alt=""> | Čeština (Czech) *(new)* | `cs` |
| <img src="../flags/pl.svg" width="24" alt=""> | Polski (Polish) *(new)* | `pl` |
| <img src="../flags/ua.svg" width="24" alt=""> | Українська (Ukrainian) *(new)* | `uk` |
| | **Eastern Mediterranean** | |
| <img src="../flags/gr.svg" width="24" alt=""> | Ελληνικά (Greek) *(new)* | `el` |
| <img src="../flags/il.svg" width="24" alt=""> | עברית (Hebrew) | `he` |
| | **The Americas** | |
| <img src="../flags/ca.svg" width="24" alt=""> | English (Canada) | `en-CA` |
| <img src="../flags/ar.svg" width="24" alt=""> | Español (Argentina) | `es-AR` |
| | **Asia and the Pacific** | |
| <img src="../flags/au.svg" width="24" alt=""> | English (Australia) | `en-AU` |
| <img src="../flags/nz.svg" width="24" alt=""> | English (New Zealand) | `en-NZ` |

The ones marked *new* are fresh translations that no native speaker has checked yet: Settings
marks them *new*, and [Improving a translation](#improving-a-translation) says how to help.

British English is the reference: every text is written in it first. Australian and New Zealand
English use British spelling and say "bin"; Canadian English keeps British spelling but says
"trash" and uses -ize where British uses -ise. Argentinian Spanish uses *vos*.

## How to use it

**Desktop app:**

1. Open Settings: **Ctrl+,**, or **F9** → *Settings*, or start it with `coxswain-gui --settings`.
2. Under *Language*, click a language. The one in use is shown at the top, with its flag. Below
   it the languages are listed under their regions (*Nordic and Baltic*, *Western Europe*, …),
   each by its own name and flag; a fresh translation has a *new* badge.
3. The window redraws in the new language at once, and the choice is saved to `config.toml`.

*Automatic* goes back to following the system; below it, small, is the language that picks now.
Under the list, *Translations marked new are fresh…* opens [Improving a translation](#improving-a-translation)
in the browser.

**Terminal app, or by hand:**

1. Open `config.toml` (`coxswain --config-path` prints where).
2. Put `language = "da"` at the top, above the first `[table]`. `"auto"` follows the system.
3. Start the terminal app again (quit with **F10**). The desktop app reads it at its next start,
   or at once when set from Settings.

There are no keys for the language.

## Which language Automatic picks

With `language = "auto"`, the default, Coxswain reads your system's list of preferred languages
(on Linux, macOS and Windows alike), and takes the first one it has, in any regional form (any
English counts); otherwise the nearest relative of the first one:

| Your system | Coxswain uses |
|---|---|
| One of the languages above, in any region (`de-CH`, `fr-CA`, `sv-FI`, `ca-ES-valencia`, …) | That language |
| US English, or English without a region | Canadian English |
| Australian, Canadian, New Zealand English | That English |
| Any other English (Britain, Ireland, South Africa, India, …) | British English |
| Any Spanish (Spain, Mexico, …), Galician and Aragonese | Argentinian Spanish |
| Norwegian (Bokmål, Nynorsk) | Danish, the closest written language |
| Frisian, Afrikaans | Dutch |
| Swiss German, Luxembourgish | German |
| Occitan | Catalan |
| Polish, Czech, Ukrainian, Greek (`pl_PL`, `cs_CZ`, `uk_UA`, `el_GR`, `el_CY`, …) | That language |
| Slovak | Czech, which Slovak readers read |
| Hebrew (also the old code `iw`) | Hebrew |
| Anything else | British English |

A language code in `config.toml` goes through the same table, so `language = "nb"` gives Danish
and `language = "es"` Argentinian Spanish.

## What you see

Everything Coxswain itself says changes: menus, commands and the F-key bar, dialogs, the preview
pane and its facts, the duplicate finder, Settings, status messages, errors, sizes (`KB`, or `Ko`
in French, with the decimal comma where the language uses one) and the age chips (`5m`, `2d`).
Numbers are written the way the language writes them (1 234,5 or 1.234,5).

Not translated:

- key names (Ctrl, Alt, Enter, F5 …), which are the same in `config.toml` everywhere;
- file and folder names, and the names of your favourite groups;
- error texts that come from the operating system, which are in the system's own language;
- the names of programs and formats (Ollama, LibreOffice, PDF).

## Capitals in Greek

Some headings are written in capitals (Settings' section titles, the sidebar's headings, the
Cyber theme's dialog titles). Greek drops its accents in capitals and keeps a diaeresis:
*Ρυθμίσεις* becomes ΡΥΘΜΙΣΕΙΣ, not ΡΥΘΜΊΣΕΙΣ, and *τσάι* becomes ΤΣΑΪ. The desktop app tells
its web view that the page is Greek, which uppercases it so (WebKitGTK on Linux, WebKit on
macOS and WebView2 on Windows all do). The terminal app writes no translated text in capitals.

## Right to left

![The desktop app in Hebrew, mirrored: the sidebar on the right, file names aligned right with their icons on the right, the F-key bar running from F1 on the right](../screenshots/gui-lang-he.png)
*Coxswain in Hebrew: the whole layout mirrors.*

In Hebrew the desktop app mirrors its layout: the sidebar is on the right, text is aligned to
the right, the columns run from right to left, and the back arrow points right. File names,
paths and commands stay left to right inside it, as Hebrew systems show them, and notes and the
command line take the direction of what you type.

The terminal app is limited by the terminal: most terminals, Alacritty among them, do not do
right-to-left text, so Hebrew there shows its letters in reversed order. Terminals that support
it (for example Konsole, or GNOME Terminal with bidi on) show it correctly.

## Settings and config.toml

| Setting | Key | Type, default |
|---|---|---|
| Settings → *Language* | `language` | text: `"auto"` or a code from the table; `"auto"` |

One key for both apps.

## In the terminal app

The same 22 languages and the same translations, chosen by the same `language` key. It is read
when the app starts; there is no picker, but `coxswain --languages` prints the list by region,
the one in use and the new ones marked, and where to suggest a better word. Right to left depends on the terminal (see
[Right to left](#right-to-left)). The help (**F1**), the command list (**F9**), dialogs, the
F-key bar and status texts are all translated.

## Questions

#### My system is in US English. Why does Coxswain say "colour" but "trash"?

US English, and English without a region, get Canadian English: British spelling, North
American words. Pick *English (United Kingdom)* in Settings for "bin", or keep it.

#### I changed the language in Settings and the terminal app still speaks the old one.

The terminal app reads `language` when it starts. Quit it (**F10**) and start it again.

#### How do I go back to following the system?

Settings → *Language* → *Automatic*, or `language = "auto"` in `config.toml` (or delete the line).

#### My system lists British English first and Danish second, and Coxswain speaks Danish.

That is how *Automatic* works now: British English is also what an unknown language falls back
to, so it passes over every system language that comes out as British English (`en-GB`,
`en-IE`, `en-IN`, …) and takes the first that gives another language; only when there is none
does it use British English. Set `language = "en-GB"` (or pick *English (United Kingdom)* in
Settings) to keep English.

#### Polish, Czech, Ukrainian or Greek reads oddly in places. Why?

These four are new, marked *new* in Settings, and have not been read by a native speaker yet.
Coxswain shows the notice "*Polski is a new translation…*" once (in the desktop app under
Settings → *What's new*, in the terminal app in the status line) to say so. A better word is
very welcome: see [Improving a translation](#improving-a-translation).

#### Why are key names in English?

Keys are written the same everywhere (Ctrl, Alt, F5), so a key in `config.toml` means the same
in every language, and the names match what is printed on most keyboards.

#### Does the language change what Find file reads in pictures?

Yes: [text recognition](../search/scans.md) reads English and your system's language, when
tesseract has that language installed. [Search by meaning](../search/meaning.md) works across
languages whatever you pick: a query in English finds a Danish document.

#### The F-key bar in my language is cut short. Is that a mistake?

Labels on the F-key bar must fit about nine characters, so some are abbreviated ("Mkdir",
"PullDn" in English). A better short word is welcome; see
[Improving a translation](#improving-a-translation).

## Improving a translation

The translations were written with care but have not all been checked by native speakers yet.
The ones marked *new* (Polish, Czech, Ukrainian, Greek) need a native reader most, then Basque,
Latvian and Lithuanian. Corrections are very welcome, in either of two ways:

- **Tell us:** open an [issue on GitHub](https://github.com/mwo-dk/coxswain/issues/new) with the
  language, the text as it is (or where it shows) and what it should say. No setup needed.
- **Change it yourself,** in a pull request:

1. Each language is one file in [`crates/coxswain-core/locales/`](../../crates/coxswain-core/locales/),
   for example `da.json`. It maps a key to its text:
   ```json
   "dupes.keep_newest": "Markér alle undtagen den nyeste",
   "items": { "one": "{n} element", "other": "{n} elementer" }
   ```
2. Change the text, keeping every `{placeholder}` (you may move it). Texts with a count have one
   entry per plural form your language uses: `one`/`other` for most, `one`/`few`/`other` for
   Lithuanian, `zero`/`one`/`other` for Latvian, `one`/`two`/`other` for Hebrew, and
   `one`/`many`/`other` for French, Italian, Spanish and Catalan, `one`/`few`/`many`/`other`
   for Polish and Ukrainian, `one`/`few`/`other` for Czech.
3. Run `cargo test -p coxswain-core i18n`: it checks that every file parses, has no unknown
   keys, keeps the placeholders and has an `other` form.
4. Open a pull request.

**Adding a language:** add its code, name, flag and region to `LANGUAGES` and `source()` in
[`crates/coxswain-core/src/i18n.rs`](../../crates/coxswain-core/src/i18n.rs), map its system codes
in `nearest()`, give it plural rules in `plural()` if it needs other than one/other, copy
`en-GB.json` to the new file and translate it, and add the flag (from
[flag-icons](https://github.com/lipis/flag-icons)) to `docs/flags/` and to the list in
`gui/src/Settings.svelte`.

Flags: [flag-icons](https://github.com/lipis/flag-icons), MIT licence (`docs/flags/LICENSE`).

---
[← Previous: Your own theme and the colour slots](own-theme.md) · [Next: Changing keys →](keys.md)
