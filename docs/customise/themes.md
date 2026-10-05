[← README](../../README.md) · [Docs index](../README.md) · [Customising](README.md)

# Themes

A theme is a set of colours. Eighteen come built in: Cyber, a green phosphor terminal and the
desktop app's default; Norton Commander blue, the terminal app's default; four modern ones; and
the looks of Windows 3.11 to 11 and Mac System 7 to today. Each app has its own theme, so the
two can differ, and you can [make your own](own-theme.md).

![Nine of the Windows and Mac themes side by side in the desktop app, light and dark, each with its own sidebar, selection colour and chrome](../screenshots/gui-themes.png)
*Nine of the Windows and Mac themes. In the desktop app a theme also brings a [look](looks.md).*

## How to use it

| | Desktop app | Terminal app |
|---|---|---|
| Key in `config.toml` | `[gui] theme`, default `"cyber"` | `theme` (top level), default `"nc"` |
| Pick it | Settings (**Ctrl+,**) → *Looks*, *For*: *Desktop app*, then click a swatch | The desktop app's Settings → *Looks*, *For*: *Terminal app*, then a swatch; or set `theme = "win95"` in `config.toml` |
| Or | **F9**, type `theme`, pick *Theme: Windows 95* | |
| Takes effect | At once, and saved | At the next start |

In the desktop app:

1. Press **Ctrl+,** and choose *Looks*. With *For* on *Desktop app*, the theme list shows every
   built-in theme as a small window in its own colours, then your own themes.
2. Click one. The whole window repaints; the choice is written to `[gui] theme`.

Or, without Settings:

1. Press **F9** and type `theme`. The command list shows *Theme: Cyber*, *Theme: Dark* and so on;
   the one in use says *current* where the others show a key.
2. Pick one with **Up**/**Down** and **Enter**. It is applied and saved, as in Settings.

In the terminal app, open `config.toml` (`coxswain --config-path` prints where), put
`theme = "win95"` at the top, above the first `[table]`, and start the app again.

## What you see

| Name in the config | Shown as | Look | What it is |
|---|---|---|---|
| `cyber` | Cyber | `crt` | Green phosphor on black, as in WarGames: glowing text, faint scanlines, cyan folders, amber instead of red. Monospaced throughout. The desktop app's default |
| `dark` | Dark | `modern` | Neutral dark, blue accent |
| `light` | Light | `modern` | White, blue accent |
| `nord` | Nord | `modern` | The Nord palette |
| `midnight` | Tokyo Night | `modern` | The Tokyo Night palette |
| `nc` | Classic blue (NC) | `dos` | Norton Commander 5: cyan on blue, a black-on-cyan cursor, yellow marks, in exact CGA colours. The terminal app's default |
| `win31` | Windows 3.11 | `win31` | White lists, grey chrome, navy selection, square corners |
| `win95` | Windows 95 | `win95` | Grey 3D bevels, navy title bars, navy selection |
| `winxp` | Windows XP | `winxp` | Luna: beige chrome, blue task pane, rounded blue title bars |
| `win7` | Windows 7 | `win7` | Aero: pale blue navigation, glassy light-blue selection |
| `win10` | Windows 10 | `win10` | Flat and white, square corners |
| `win11` | Windows 11 | `win11` | Mica, light, rounded corners |
| `win11-dark` | Windows 11 (dark) | `win11` | Mica, dark |
| `system7` | Mac System 7 | `system7` | Black on white, one-pixel lines, striped title bars, inverted selection |
| `platinum` | Mac OS 9 | `platinum` | Grey bevels, lavender selection |
| `aqua` | Mac OS X Aqua | `aqua` | Pinstripes, gel buttons, blue selection (10.0 to 10.4) |
| `macos` | macOS | `macos` | Current macOS, light |
| `macos-dark` | macOS (dark) | `macos` | Current macOS, dark |

This is the order Settings shows them in. In Cyber, file icons take the text colour instead of
their own, so everything is one phosphor, and the age chips go from cyan (new) to dim green.
Lit buttons and badges glow as a green outline on the dark rather than being filled
([Looks](looks.md#what-you-see)).

![The desktop app in Cyber: two panels of green text on black, cyan folders, green age chips, the sidebar on the left](../screenshots/gui-details.png)
*Cyber, the desktop app's default.*

![The terminal app in Classic blue (NC): two panels with double borders, cyan text on blue, yellow column headers, the F-key bar at the bottom](../screenshots/tui-panels.png)
*Classic blue (NC), the terminal app's default.*

The desktop app also tells the system whether the theme is light or dark (from the panel
background), so native parts such as scroll bars and the pickers in forms follow it.

## Settings and config.toml

| Setting | Key | Type, default |
|---|---|---|
| Settings → *Looks*, *For*: *Desktop app* | `[gui] theme` | text, `"cyber"` |
| Settings → *Looks*, *For*: *Terminal app* | `theme` | text, `"nc"` |
| Your own themes | `[themes.<name>]` | tables; see [Your own theme](own-theme.md) |

```toml
theme = "nc"          # the terminal app

[gui]
theme = "win95"       # the desktop app
```

A name that is neither built in nor one of your `[themes.<name>]` is ignored without a word: the
desktop app shows Cyber, the terminal app NC.

## In the terminal app

The terminal app takes a theme's **colours only**: the text and background of each
[colour slot](own-theme.md#the-colour-slots). The look (corners, bevels, fonts, title bars) is
the desktop app's; the terminal app's panels always have NC's double borders, and its font is
the terminal's. Slots only the desktop app has (`accent`, `sidebar`, `tab`, `tab_active`,
`preview`) are not used there.

It has no theme picker of its own: there is no Settings window, and the *Theme: …* entries are not
in its command list. Pick its theme in the desktop app's Settings → *Looks* with *For* on
*Terminal app*, or set `theme` in `config.toml`, and start it again.

## Questions

#### Why does the terminal app not have the Windows 95 bevels?

A terminal draws characters in cells, so the terminal app takes a theme's colours only. Bevels,
title bars, rounded corners and fonts are the desktop app's [looks](looks.md). The colours do
come across: `theme = "win95"` gives white panels, navy cursor and grey status lines.

#### The colours in my terminal look washed out, or wrong.

The built-in themes use exact `#rrggbb` colours, so they need a terminal with true colour
(24-bit). Most modern terminals have it. In `tmux`, enable it with
`set -ga terminal-overrides ",*:Tc"`; over `ssh`, check that `COLORTERM=truecolor` reaches the
other side.

#### Can the two apps use the same theme?

Yes: give both keys the same name, `theme = "nord"` at the top and `theme = "nord"` under
`[gui]`. They stay separate keys so that you can run, say, NC in the terminal and macOS on the
desktop.

#### Where are my own themes in the F9 list?

After the built-in ones, under the name you gave them (`[themes.<name>]`), as in Settings.

#### Which theme is closest to Midnight Commander or Far Manager?

*Classic blue (NC)*, `nc`: Norton Commander's cyan on blue in exact CGA colours, which those
programs copied. In the desktop app it comes with the `dos` look: square corners, double pane
borders and the monospaced font for everything.

#### Does the theme change the colour tags?

No. [Colour tags](../organise/tags.md) keep their own fixed colours (red, orange, yellow, green,
blue, purple, grey) in every theme, so a tag means the same whatever you pick.

#### Why is a draw.io diagram white in a dark theme, when Mermaid is green in Cyber?

A draw.io drawing carries its own colours, made for a white page, so the preview pane puts it on
white whatever the theme, to keep its lines and labels readable. Mermaid diagrams are drawn by
Coxswain itself and take the theme's colours: the panel background, boxes in the `status`
slot's background, lines in the `border` colour. See [Diagrams](../previews/diagrams.md).

---
[← Previous: The Settings window](settings.md) · [Next: Looks →](looks.md)
