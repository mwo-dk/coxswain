[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Shogi games

**KIF** and **CSA** files hold shogi games: who played, the event, the handicap, and every
move, KIF files often with variations and comments. Coxswain shows their games in the same
**table of games** as [chess games](chess.md), and each game on a **9×9 shogi board** with the
pieces each side holds in hand, stepped through move by move: a piece taken goes to the hand
of the side that took it, a drop comes out of it, a promoted piece turns over. Both apps have
it. Coxswain only reads: it does not play or check a game.

![A shogi game in the desktop app's window (Alt+K): the 9×9 board on the Cyber theme's colours, the pieces as five-sided tiles with their kanji, Gote's turned round, the last move's squares tinted, Gote's pieces in hand above the board and Sente's below, beside it the players Ada Vance – Kenji Mori and the moves in Japanese with a comment and a variation](../screenshots/gui-shogi.png)

![The terminal app's shogi view in Classic blue: the board as rows of kanji, Gote's pieces written with a v before them in red, empty squares ・, the files ９ to １ above and the ranks 一 to 九 beside, the pieces in hand above and below, the moves beside it](../screenshots/tui-shogi.png)

- [Using it](#using-it)
- [The table of games](#the-table-of-games)
- [A game on the board](#a-game-on-the-board)
- [Variations and comments](#variations-and-comments)
- [Finding games](#finding-games)
- [Files it reads](#files-it-reads)
- [Questions](#questions)

## Using it

**Desktop app.** Put the cursor on a `.kif`, `.kifu` or `.csa` file and the preview pane
(**Space**) shows its table of games. **⤢** at the top right, **Alt+K** or *Open games* in the
action menu (**Shift+F10**) and the command list (**F9**) open it in a window of its own;
**Esc** closes that window.

**Terminal app.** **F3** or **Alt+K** on the file opens it full screen; so does *Open games* in
the action menu (**Shift+F10**) and **F9**. **Esc** closes it.

It is the same action as for chess, [Go](go.md), [draughts](draughts.md) and
[xiangqi](xiangqi.md): the first few times the cursor rests on a shogi file, the status line
says *Alt+K opens its games: a table, then a board with every move*. **F1** → *Features* →
*Shogi games* has the keys and the questions below.

## The table of games

The table is the chess one, without the opening columns. A KIF file is one game; a CSA file
can hold many, one after another.

| Column | What it shows |
|---|---|
| **#** | The game's place in the file |
| **Sente**, **Gote** | The players: KIF's `先手` and `後手` (`下手` and `上手` in a handicap game), CSA's `N+` and `N-` |
| **Result** | `1-0` when Sente won, `0-1` when Gote did, `1/2-1/2` for a repetition or impasse, `*` when the game stops unfinished |
| **Date**, **Event** | KIF's `開始日時` and `棋戦`, CSA's `$START_TIME` and `$EVENT` |
| **Moves** | How many moves the main line has, each side's counted |

**Filtering and sorting** are as for chess: in the desktop app the fields above the table and
a click on a heading; in the terminal app **/** player, **r** result, **y** years, **c** clear,
**s** and **S** to sort.

**Opening a game.** Desktop: **Enter** or a double-click; in the window (**Alt+K**) the table has
the keys at once, in the preview pane click it first. Terminal: **Enter**.

## A game on the board

Sente is at the bottom. Each piece is its kanji: 歩 香 桂 銀 金 角 飛 and the kings 玉 (Sente's)
and 王 (Gote's); promoted, と 杏 圭 全 馬 龍. In the desktop app a piece is a five-sided tile, Gote's
turned round, a promoted one written in red. In the terminal app Gote's pieces have a `v` before
them and are red, as KIF's board diagrams write them; an empty square is `・`. The files are
numbered ９ to １ above the board, the ranks 一 to 九 beside it. The squares the last move went
from and to are tinted yellow.

Above the board: *Gote in hand:* and the pieces Gote holds (`歩2 角`); below it *Sente in
hand:*; `–` for none. Under them: *Sente (▲) to move* or *Gote (△) to move*.

Beside the board: the players and the result, the event, place and date, *Game 1 of 2*, then the
moves in Japanese, numbered from the first, each with ▲ for Sente or △ for Gote: `▲７六歩`,
`△同　銀` (onto the square of the move before), `▲４五角打` (a drop), `▲２二角成` (a
promotion). CSA moves are written the same way.

| Key | Desktop app | Terminal app |
|---|---|---|
| **→** / **←** | Next / previous move (click the board first) | Next / previous move |
| **↓** / **↑** | Into the variations of this move, and back | The same |
| **Home** / **End** | The start position / the last move of the line | The same |
| **f** | Turn the board round (also *Flip*): Gote at the bottom | The same |
| **PgDn** / **PgUp** | The next / previous game of the table | The same |
| **Esc** | Back to the table (*Games* too) | Back to the table |
| A click on a move | Shows it | – |

A move that cannot be played (no piece of the side to move where it starts, its own piece where
it goes, a drop of a piece not in hand) stops the positions of its line, and the line above the
board says *… could not be played*. Moves are not checked further.

**Handicaps and set positions.** KIF's `手合割` takes the handicap's pieces off Gote's side
(`香落ち`, `角落ち`, `飛車落ち`, `飛香落ち`, `二枚落ち`, `四枚落ち`, `六枚落ち`, `八枚落ち`,
`十枚落ち`), and Gote moves first. A KIF board diagram (the `|v香v桂 ・…|` lines with
`先手の持駒` and `後手の持駒`), or CSA's `P1` … `P9`, `P+`, `P-` and `PI` lines, sets the
position the game starts from.

## Variations and comments

A KIF file's `変化：12手` starts a variation at move 12: it goes on from the position before
that move, in the line read last that reached it. Variations show in brackets in the moves, as
for chess; **↓** and **↑** go between a move and the ones played instead of it. A comment
(`*` lines in KIF, `'` lines in CSA) shows in italics after its move.

## Finding games

The search store reads a shogi file as a line per game: the players, event, place, date and
result. So **Find** (**Ctrl+F**) finds a game by a player's name or a tournament, and a hit
opens that game. A store made before this release reads every `.kif`, `.kifu` and `.csa` file
again once, for this: only those files, nothing else is read again.

## Files it reads

| Ending | What |
|---|---|
| `.kif`, `.kifu` | KIF, as Kifu for Windows and most shogi programs write it: the header, the moves with the squares they come from, `同`, `打`, `成` and `不成`, comments, variations, handicaps, board diagrams. Text in UTF-8, else Shift_JIS (a `.kif` file's usual) |
| `.csa` | CSA: one game or many, cut at a line of `/`; players, event, start time, the position (`PI`, `P1` … `P9`, `P+`, `P-`), moves such as `+7776FU`, the end (`%TORYO`, `%SENNICHITE` …), comments. Several statements on a line, split by commas |

Files are read as they stream by, a game at a time. What cannot be read is skipped; nothing in
a file can stop the app or run it out of stack: a game is read up to 16 MB and 50,000 moves with
its variations, at most 10,000 variations, and the first 100,000 games of a CSA file.

## Questions

#### Which files does it read?

KIF (`.kif` in Shift_JIS, `.kifu` in UTF-8) with their variations and comments, and CSA
(`.csa`), one game or many; handicaps and board diagrams start a game of their own. See
[Files it reads](#files-it-reads).

#### Why are Gote's pieces marked v?

Gote's pieces point the other way: the desktop app turns them round, the terminal app writes
them with a `v` before them and in red, as KIF's board diagrams do.

#### Why does a game stop showing positions halfway?

A move could not be played: the line above the board names it. Often the KIF file has no
square the piece came from (`７六歩` without `(77)`) and two pieces of that kind could go
there; Coxswain then takes it for a drop.

#### Can I play a game against a program?

Not yet: Coxswain shows shogi games but does not play.

---
[← Previous: Xiangqi games](xiangqi.md) · [Next: Neural network models →](neural-networks.md)
