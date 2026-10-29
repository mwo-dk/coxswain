[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Xiangqi games

**Xiangqi**, Chinese chess, is kept in PGN files like chess: tags in brackets, then the moves.
A PGN file whose tags say `[Game "Chinese Chess"]` (or `[Variant "xiangqi"]`) opens in the same
**table of games** as [chess games](chess.md), and each game on a **xiangqi board**: nine files
and ten ranks of lines, the river across the middle, the palaces with their diagonals, the
pieces as their characters in red and black. It is stepped through move by move, a piece that
is taken goes. Both apps have it. Coxswain only reads: it does not play or check a game.

The games of Chinese xiangqi websites come as **DhtmlXQ** files instead, one game a file, often
with no ending at all (`0001-对兵互进右马局`) or as `.che`: they open the same way, with their
variations and comments ([DhtmlXQ games](#dhtmlxq-games)).

![A xiangqi game in the desktop app's window (Alt+K): the board on the Cyber theme's colours with the river 楚河 漢界 and the palaces, red and black pieces as discs with their characters, the last move's points ringed in yellow, beside it the players Mei Lin – Tomas Berg, the event and the moves in Chinese with a comment and a variation](../screenshots/gui-xiangqi.png)

![The terminal app's xiangqi view in Classic blue: the board drawn with box lines on the theme's wood, the pieces as red and black characters, the river line 楚 河 漢 界, files numbered 1 to 9 at the top and 九 to 一 at the bottom, the moves beside it](../screenshots/tui-xiangqi.png)

![The terminal app's xiangqi view in Classic blue on a taller window: the same game with each piece a light disc drawn from block characters, its character in red or black on it, the lines running between them, the river the gap where only the edge files cross, the palaces' centres ╳](../screenshots/tui-xiangqi-discs.png)

- [Using it](#using-it)
- [The table of games](#the-table-of-games)
- [A game on the board](#a-game-on-the-board)
- [Notations](#notations)
- [DhtmlXQ games](#dhtmlxq-games)
- [Finding games](#finding-games)
- [Files it reads](#files-it-reads)
- [Questions](#questions)

## Using it

**Desktop app.** Put the cursor on the `.pgn` file (or a DhtmlXQ game) and the preview pane
(**Space**) shows its table of games. **Enter**, a double-click, **⤢** at the top right, **Alt+K** or *Open games* in the action menu
(**Shift+F10**) and the command list (**F9**) open it in a window of its own; **Esc** closes
that window.

**Terminal app.** **Enter**, **F3** or **Alt+K** on the file opens it full screen; so does *Open games* in
the action menu (**Shift+F10**) and **F9**. **Esc** closes it.

**Inside an archive.** A games file inside a `.zip` or another archive opened as a folder
(**Enter** on it) shows the same way: in the desktop app the preview pane shows the copy
Coxswain takes out of the archive when the cursor rests on it, and **Enter** opens that copy in
the games window; in the terminal app **Enter** or **F3** opens it full screen.

It is the same action as for chess, [Go](go.md) and [draughts](draughts.md): while the cursor is on it the command line (the status bar in the desktop app) says *Enter: Open games*, and the first few times the cursor rests on a PGN file, the status line says *Alt+K opens its games: a table,
then a board with every move*. Whether the file is chess or xiangqi is told by its first tags
when it is opened. **F1** → *Features* → *Xiangqi (Chinese chess) games* has the keys and the
questions below.

## The table of games

The table is the chess one, without the opening columns:

| Column | What it shows |
|---|---|
| **#** | The game's place in the file |
| **Red**, **Black** and their **Elo** | The players, from the `Red` (or `White`) and `Black` tags, and `WhiteElo`, `BlackElo` when the file has them |
| **Result** | `1-0` (Red won), `0-1`, `½-½` or `*` |
| **Date**, **Event** | From the `Date` and `Event` tags |
| **Moves** | How many full moves the main line has |

**Filtering.** In the desktop app, the fields above the table: *Player*, the result, *From year*
and *To year*, *Rating at least*. In the terminal app: **/** player, **r** cycles the result,
**y** years, **e** rating, **c** clears them all; type, then **Enter**.

**Sorting.** Desktop: a click on a heading sorts by it, a second click reverses. Terminal: **s**
takes the next column, **S** reverses.

**Opening a game.** Desktop: **Enter** or a double-click; in the window (**Alt+K**) the table has
the keys at once, in the preview pane click it first. Terminal: **Enter**.

## A game on the board

Red is at the bottom. The pieces are their characters: Red's 帥 仕 相 傌 俥 炮 兵 in red, Black's
將 士 象 馬 車 砲 卒 in black. The river 楚河 漢界 lies between the fifth and sixth ranks; only
the edge files cross it. The palaces have their diagonals in the desktop app; the terminal app
marks their centres ╳. The files are numbered as each side counts them: Black's 1 to 9 along
the top, Red's 九 to 一 along the bottom, both from that side's right. The points the last move
went from and to are ringed yellow in the desktop app and tinted in the terminal app.

Where the window has room for points of three rows (about 38 rows and 110 columns), the
terminal app draws each piece as a light disc of block characters with its character on it as
text, and the river is the gap in the lines; where the terminal shows pictures, the desktop
app's discs themselves ([Game pieces in the terminal](game-pieces.md)). On a smaller window
the pieces are their characters on the lines, with the river's words between the banks.

Beside the board: the players and the result, the event, place and date, *Game 2 of 7*, then the
moves as the file writes them, with move numbers, comments in italics and variations in
brackets, the move you are on highlighted. Under the board it says *Red to move* or *Black to
move*.

| Key | Desktop app | Terminal app |
|---|---|---|
| **→** / **←** | Next / previous move (click the board first) | Next / previous move |
| **↓** / **↑** | Into the variations of this move, and back | The same |
| **Home** / **End** | The start position / the last move of the line | The same |
| **f** | Turn the board round (also *Flip*) | The same |
| **PgDn** / **PgUp** | The next / previous game of the table | The same |
| **Esc** | Back to the table (*Games* too) | Back to the table |
| A click on a move | Shows it | – |

The desktop app also has **|◀ ◀ ▶ ▶|** under the board.

A move that cannot be played (no piece of the side to move where it starts, its own piece where
it goes) stops the positions of its line: the moves after it are still listed, and the line
above the board says *… could not be played*. Moves are not checked further: a move the rules
would not allow is played as written.

## Notations

| Notation | Looks like | Read as |
|---|---|---|
| ICCS | `h2e2`, `H2-E2` | From and to: files `a` to `i` from Red's left, ranks `0` to `9` from Red's side |
| WXF | `C2.5`, `H8+7`, `R1-2`, `+C.5` or `C+.5` | Piece (K A E/B H/N R C P), its file as its side counts it, `+` forward, `-` back, `.` or `=` across, then the file it goes to or the ranks it moves; `+`/`-` in place of the file for the front or rear of two on one file |
| Chinese | `炮二平五`, `马８进７`, `前車退一`, `後馬進四` | The same in characters, simplified or traditional: 进/進 forward, 退 back, 平 across, 前 and 后/後 front and rear, Red's numbers in Chinese, Black's in digits |

A game may start from a position of its own: a `FEN` tag, Red in capitals
(`rnbakabnr/9/1c5c1/p1p1p1p1p/9/9/P1P1P1P1P/1C5C1/9/RNBAKABNR w`); `h` and `e` are read as the
horse and the elephant too.

## DhtmlXQ games

DhtmlXQ is the format of the Chinese web viewers of that name and of the sites built on them,
and of community collections exported from those sites (one repository holds 190,000 games,
each a file with no ending). A game is a list of tags in brackets:

```text
[DhtmlXQ]
[DhtmlXQ_title]0001-对兵互进右马局[/DhtmlXQ_title]
[DhtmlXQ_red]布局圣手[/DhtmlXQ_red]
[DhtmlXQ_black]兰帝[/DhtmlXQ_black]
[DhtmlXQ_result]红胜[/DhtmlXQ_result]
[DhtmlXQ_binit]0919293949596979891777062646668600102030405060708012720323436383[/DhtmlXQ_binit]
[DhtmlXQ_movelist]26256364[/DhtmlXQ_movelist]
```

Coxswain knows such a file by its first bytes, `[DhtmlXQ`, when its name has no ending or ends
in `.che`, `.txt` or `.dhtmlxq`, and reads it as one game. A dot inside the name does not make an
ending: in `001-1.古谱的启示`, `002.Opening game` or `game.abc` what follows the dot is no file
type Coxswain knows, so such files are looked at too; `game.json` or `game.png` are not. Only their first 64 bytes
are read, and never those of a file kept only in the cloud.

| Tag | Read as |
|---|---|
| `title`, `event` | The **Event** (the event, else the title) |
| `red`, `black` | The players |
| `date`, `place`, `round` | **Date**, the place, the round |
| `result` | 红胜, 紅勝 or 红先胜 Red won (`1-0`), 黑胜 or 黑勝 Black won (`0-1`), 和局, 和棋 or 和 a draw (also written `1-0`, `0-1`, `1/2-1/2`), anything else `*` |
| `open` | The opening's name |
| `binit` | The start: the 32 pieces, each as its column (0 to 8 from Red's left) and row (0 to 9 from Black's side), `99` for one not on the board; Black moves first when the first move is Black's |
| `movelist` | The moves, four digits each: from column and row, to column and row; shown in ICCS (`c3c4`) |
| `move_A_B_C` | A variation: line C in place of the Bth move of line A (the main line is 0), shown in brackets after that move, inside one another as deep as the file has them |
| `commentN`, `commentC_N` | The comment after the Nth move of the main line or of line C (`comment0` before the first); `||` is a new line |

Other tags (the viewer's size, the site, a picture) are left. A file with moves that cannot be
played shows its moves up to there (of the 191,234 games in the collection above, 13 have such a
move). Binary files from other programs that also end in `.che` are not read.

## Finding games

The search store reads xiangqi files with the chess reader: a line per game with the players,
event, place, date and result, so **Find** (**Ctrl+F**) finds a game by a player's name or a
tournament, and a hit opens that game. Nothing is read again for this: a xiangqi file already
in the store keeps its line until it changes; one read from now on has Red's name from the
`Red` tag too.

DhtmlXQ games are not in the search store: Find does not find them by player or event.

## Files it reads

| File | What |
|---|---|
| `.pgn` (also `.pgn.gz`, `.pgn.zst`) whose tags say `[Game "Chinese Chess"]`, `[Game "Xiangqi"]` or `[Variant "xiangqi"]` in its first 64 KB | Xiangqi games: tags, moves in ICCS, WXF or Chinese, comments, variations, `!` `?` and `$1` annotations, a `FEN` start |
| A file with no ending, `.che`, `.txt` or `.dhtmlxq` that starts with `[DhtmlXQ` | One DhtmlXQ game, with its variations and comments, up to 16 MB ([DhtmlXQ games](#dhtmlxq-games)) |

Binary files (`.xqf`) are not read. What cannot be read is skipped; nothing in a file can stop
the app, and no nesting of variations, however deep, can run it out of stack: variations deeper
than 64 are skipped, and a game is read up to 50,000 moves with its variations.

## Questions

#### Which files does it read?

PGN files whose tags say `Game "Chinese Chess"` or `Variant "xiangqi"`, with moves in ICCS
(`h2e2`), WXF (`C2.5`) or Chinese (`炮二平五`), and a `FEN` tag for a start of their own. See
[Notations](#notations). Also DhtmlXQ games, the format of Chinese xiangqi websites (no ending,
`.che`, `.txt` or `.dhtmlxq`), inside archives too: see [DhtmlXQ games](#dhtmlxq-games).

#### I downloaded a zip of xiangqi games: how do I look at them?

Press **Enter** on the zip: it opens like a folder. Go into the folders and rest the cursor on a
game: the desktop app's preview pane shows it on the board, the terminal app's **F3** opens it
full screen. To keep a collection at hand, copy it out of the zip with **F5**.

#### Why does a file with no ending show as a xiangqi game?

Its first bytes are `[DhtmlXQ`: it is a DhtmlXQ game, as xiangqi websites keep them. The same
goes for a name whose dot starts no file type's ending, such as `001-1.古谱的启示`. Any other
file with no ending shows as before.

#### Why does my xiangqi file open as chess?

Its first tags do not say it is xiangqi. Add `[Game "Chinese Chess"]` to the first game's tags:
Coxswain looks at the first 64 KB of the file.

#### Does it check the moves?

Only that the piece is there and the point is not its own side's: a move is played as written,
and its line stops at one that cannot be. A horse jumping over a blocked leg, or a cannon
taking without a screen, is shown as the file has it.

#### Can I play a game against a program?

Not yet: Coxswain shows xiangqi games but does not play.

---
[← Previous: Draughts games](draughts.md) · [Next: Shogi games →](shogi.md)
