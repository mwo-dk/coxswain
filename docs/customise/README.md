[← README](../../README.md) · [Docs index](../README.md)

# Customising

Coxswain's language, colours, shapes, fonts, glyphs and keys are yours to change. Both apps have
Settings (**Ctrl+,** or **F12**; the terminal app shows *12 Settings* on its F-key bar), which
change the most used parts at once; everything else is set in one file, `config.toml`, that
both apps read. These pages walk
through each part, with the keys and the exact `config.toml` lines.

![Settings at Looks: the last languages under their regions, among them The Middle East with עברית and فارسی and The Caucasus with Հայերեն and ქართული, under them the button Suggest a better translation, a line, then the For switch on Desktop app and the theme swatches under their first group, Coxswain's own, with Cyber chosen](../screenshots/settings-looks.png)
*The Settings window at Looks: the language, the theme, glyphs and fonts.*

| Page | What it covers |
|---|---|
| [The Settings window](settings.md) | Opening it in both apps, its areas (Overview, Finding files, Previews, Looks, Behaviour, Keys, Privacy and updates), *Find a setting…*, every item with the `config.toml` key it writes, how saving keeps your comments |
| [Themes](themes.md) | The 33 built-in themes, picking one in each app, what the terminal app takes from a theme |
| [Looks](looks.md) | The desktop app's shapes and chrome per era: corners, bevels, title bars and the fonts they ask for |
| [Your own theme and the colour slots](own-theme.md) | `[themes.<name>]`, the 31 colour slots, colour names and `#rrggbb`, starting from a built-in theme |
| [Languages](languages.md) | The 55 languages by region, how *Automatic* picks one, right to left in Hebrew and Persian, improving a translation |
| [Changing keys](keys.md) | `[keys]`: every action, key names, rules, unbinding, and what cannot be rebound |
| [Glyphs and fonts](glyphs-and-fonts.md) | Nerd Font or plain ASCII, your own `[glyph_set]`, the interface, monospaced and icon fonts, text size |

## Keys at a glance

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| **Ctrl+,** / **F12** | Opens Settings | **F12** opens Settings, full screen (**Ctrl+,** where the terminal passes it on) | Settings, saved to `config.toml` at once |
| **F9**, type `theme` | *Theme: …* entries | Not there: **F12** → *Looks* → *Theme of the terminal app* | Pick a built-in theme; saved at once |
| **F1** | Help, with your keys | Help, with your keys | Shows every action and the keys bound to it |
| **Alt+.** | Hidden files on and off | Hidden files on and off | `show_hidden` sets how each app starts |
| **Esc** | Closes Settings | | |

`coxswain --config-path` prints where `config.toml` is; `coxswain --dump-config` prints every
key with its default. Every key is listed in [Configuration: every key](../reference/configuration.md).

---
[← Previous: Scan for viruses (Alt+I)](../files/virus-scan.md) · [Next: The Settings window →](settings.md)
