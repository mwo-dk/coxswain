[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Game pieces in the terminal

The terminal app draws the pieces of a [chess game](chess.md) from **block characters**: each
piece is the desktop app's own picture (Uray M. János's set), drawn in dots, two dots across
a character cell and up to four down, so a knight looks like a knight and not like a letter.
The board grows to the largest squares the window has room for. On a small window it falls
back to the chess characters of your font (♚ ♛ ♜ ♝ ♞ ♟) on small squares, as before.

![The terminal app's game view in Classic blue: Morphy's Opera Game on a board of blue squares, each piece drawn in sextants, white pieces with a dark outline, black pieces solid, the yellow squares of the last move, the moves beside it](../screenshots/tui-game-pieces.png)

- [How it looks](#how-it-looks)
- [The block characters](#the-block-characters)
- [The setting](#the-setting)
- [Questions](#questions)

## How it looks

A piece is drawn in three tones: the light and dark of its picture, and the square's colour
around it. A white piece keeps the dark outline of its picture, so it stands out on a light
square; a black piece gets a light outline on a square almost as dark as itself. The squares are the theme's (or
the tournament board's: *Settings → Previews → Chess board*, `[chess] board`), the same as
the desktop app's; the squares of the last move are tinted yellow, as before.

Each character cell has two colours only, the terminal's foreground and background: where a
cell holds the light and the dark of a piece and some of the square, the fewest dots take the
colour of the next fewest. The finer the block characters, the fewer such cells.

A square is twice as wide as high in cells, so that it is square on the screen. The view
needs about 30 rows and 85 columns for drawn pieces (38 rows with half blocks); on a smaller
window, or with *Figurines* chosen, it shows the chess characters of your font.

The desktop app draws the pictures themselves, as it always has.

## The block characters

| Choice | Characters | Dots in a cell | Needs |
|---|---|---|---|
| *Octants* | Unicode 16's octants (U+1CD00 …) | 2 × 4 | A terminal that draws them itself: kitty, Ghostty, foot |
| *Sextants* | Sextants (U+1FB00 …) | 2 × 3 | A terminal that draws them, or a font that has them |
| *Quadrants* | ▖ ▗ ▘ ▝ ▙ ▛ ▜ ▟ ▚ ▞ | 2 × 2 | Nearly every monospace font |
| *Half blocks* | ▀ ▄ | 1 × 2 | Every terminal, the Linux console too |
| *Figurines* | ♚ ♛ ♜ ♝ ♞ ♟ | | A font with the chess characters |

*As fine as the terminal draws* (the default) picks by the terminal: octants in kitty, Ghostty
and foot, sextants in WezTerm, Alacritty and Konsole, half blocks on the Linux console and in
a VT terminal, quadrants everywhere else (inside tmux and screen too, which hide the terminal
outside). It reads `TERM` and `TERM_PROGRAM`, nothing else.

## The setting

*Settings → Previews → Game pieces in the terminal*, in both apps (the desktop app keeps it
for the terminal app):

| Setting | `config.toml` | Values |
|---|---|---|
| *Game pieces in the terminal* | `[chess] pieces` | `auto` (default), `octants`, `sextants`, `quadrants`, `halves`, `figurines` |

```toml
[chess]
pieces = "sextants"
```

## Questions

#### The pieces show as boxes or gaps.

The terminal's font lacks those block characters, and the terminal does not draw them itself.
Choose *Quadrants* or *Half blocks* in *Settings → Previews → Game pieces in the terminal*
(`[chess] pieces = "quadrants"`): nearly every font has them.

#### Why do I see figurines instead?

The board is too small for drawn pieces: a square needs three rows (four with half blocks),
so the view needs about 30 rows and 85 columns. Make the window larger, or choose *Figurines*
to keep the small board.

#### Which block characters does it use?

With *As fine as the terminal draws*: octants in kitty, Ghostty and foot, sextants in WezTerm,
Alacritty and Konsole, half blocks on the Linux console, quadrants elsewhere. The table under
[The block characters](#the-block-characters) has them all; Settings chooses one by hand.

#### Why do the pieces look rough at the edges?

A cell has two colours, and a piece is a few dozen dots: a 6 × 3 square in sextants is 12 × 9
dots. A larger window gives larger squares and finer pieces; octants are the finest.

---
[← Previous: Chess games](chess.md) · [Next: Go games →](go.md)
