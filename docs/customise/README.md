[← README](../../README.md) · [Docs index](../README.md)

# Customising

Coxswain's language, colours, shapes, fonts, glyphs and keys are yours to change. The desktop
app's Settings window (**Ctrl+,**) changes the most used parts at once; everything else, and all
of the terminal app, is set in one file, `config.toml`, that both apps read. These pages walk
through each part, with the keys and the exact `config.toml` lines.

![The Settings window in Cyber: every language with its flag, then the theme swatches, glyphs and fonts](../screenshots/gui-settings.png)
*The Settings window, with its Language and Appearance sections.*

| Page | What it covers |
|---|---|
| [The Settings window](settings.md) | Opening it, every section and item with the `config.toml` key it writes, how saving keeps your comments |
| [Themes](themes.md) | The 18 built-in themes, picking one in each app, what the terminal app takes from a theme |
| [Looks](looks.md) | The desktop app's shapes and chrome per era: corners, bevels, title bars and the fonts they ask for |
| [Your own theme and the colour slots](own-theme.md) | `[themes.<name>]`, the 31 colour slots, colour names and `#rrggbb`, starting from a built-in theme |
| [Languages](languages.md) | The 18 languages, how *Automatic* picks one, right to left in Hebrew, improving a translation |
| [Changing keys](keys.md) | `[keys]`: every action, key names, rules, unbinding, and what cannot be rebound |
| [Glyphs and fonts](glyphs-and-fonts.md) | Nerd Font or plain ASCII, your own `[glyph_set]`, the interface, monospaced and icon fonts, text size |

## Keys at a glance

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| **Ctrl+,** | Opens Settings | Not there: edit `config.toml` | The Settings window |
| **F9**, type `theme` | *Theme: …* entries | Not there: set `theme` | Pick a built-in theme; saved at once |
| **F1** | Help, with your keys | Help, with your keys | Shows every action and the keys bound to it |
| **Alt+.** | Hidden files on and off | Hidden files on and off | `show_hidden` sets how each app starts |
| **Esc** | Closes Settings | | |

`coxswain --config-path` prints where `config.toml` is; `coxswain --dump-config` prints every
key with its default. Every key is listed in [Configuration: every key](../reference/configuration.md).

---
[← Previous: Finding duplicates](../files/duplicates.md) · [Next: The Settings window →](settings.md)
