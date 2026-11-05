[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Chess: position index and opening tree

A [games table](chess.md) can get a **position index**: every position its games reach in their
first 15 or 30 moves, or in the whole game, each with the games that reach it and the move each
played there. With it, the board shows the **opening tree** of the position on it: each next
move with its number of games, how often White won, drew or lost after it, the average Elo and
the best-rated player of the side making it, and the first and last year it was played. A click
on a move plays it and the tree follows; **List them** cuts the games list to the games that reach
the position. Both apps have it. It is off until you ask for it: building an index reads every
game once and takes room on the disk.

<!-- screenshot: gui-chess-tree.png: the desktop app (Cyber) on a game of a database with a position index, Tree on, the opening tree of the position after 1.e4 c5 2.Nf3 with Black's replies, games, the White · draw · Black bars, average Elo, years and best players -->

<!-- screenshot: tui-chess-tree.png: the terminal app (Classic blue) on a game of a database with a position index, t pressed, the opening tree of the position after 1.e4 c5 2.Nf3 in place of the moves, a move chosen, with the tree's keys -->

- [Building the index](#building-the-index)
- [The opening tree](#the-opening-tree)
- [The games reaching a position](#the-games-reaching-a-position)
- [Transpositions](#transpositions)
- [When the database changes](#when-the-database-changes)
- [Room on the disk](#room-on-the-disk)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## Building the index

Every games table of chess games can have one: a PGN file (also `.pgn.gz`, `.pgn.zst` and a PGN
file inside an archive), a SCID database (`.si4`, `.si5`), a ChessBase database (`.cbh`, `.cbv`),
and xiangqi games (DhtmlXQ, or a PGN file that says it holds xiangqi). Draughts, shogi and bridge
tables have none.

| | Desktop app | Terminal app |
|---|---|---|
| Build one | *Build position index (≈ 3.2 GB, ≈ 4 min)* under the games list's count | **i** in the games list; the state line says *i build position index (≈ 3.2 GB, ≈ 4 min)* |
| While it is built | *Building position index: 42 %* and *Stop* | The state line says *Building position index: 42 %*; **i** stops it |
| Once it is there | *Position index: 30 moves, 3.2 GB* and *Remove* | The state line says *Position index: 30 moves, 3.2 GB*; **I** (Shift+I) removes it |

The button says what the index will take before it starts: about how large it will be on the
disk and about how long it takes, worked out from the games' lengths in the table. It is built in
the background: the table, the games and everything else go on as before, and the tree is there
the moment it is done. Closing the view does not stop it; *Stop* (**i**) does, and keeps nothing.
Quitting the app stops it too.

Only a game's main line is read, up to the first move that cannot be played or a null move
(`--`). How far into each game it reads is *Settings → Games → Position index → Position index depth*:
15 moves, 30 moves (the default) or the whole game. *Position index for every games table*
builds one for each games table you open, without the button.

On a database of 12.3 million games (a 12 GB PGN file), at 30 moves, the index took about three
minutes on a 16-core machine and 6.1 GB: see [Room on the disk](#room-on-the-disk).

## The opening tree

On a game's board, *Tree* (**t** in both apps) shows the opening tree of the position on the
board, in the moves pane:

| Column | What it says |
|---|---|
| Move | The next move, in figurines or letters as the moves are shown |
| Games | How many games played it here |
| White · draw · Black | Of those games, how many White won, drew and Black won, in per cent (in the desktop app also as a bar) |
| Elo | The average Elo of the player who made the move, of the games that give one |
| Years | The first and last year it was played |
| Best player | The best-rated player who made it, with that Elo |

The most played move is first. Above the moves it says how many games reach the position.
The player and the Elo are always the side making the move: White's for a White move, Black's
for a Black move, whatever move order the game took to get there.

| | Desktop app | Terminal app |
|---|---|---|
| Play a move of the tree | A click on it: it is played on the board as a move of yours would be (the game's own next move is stepped to; any other starts [your line](chess-play.md#your-line)), and the tree follows | **↑ ↓** choose it, **Enter** plays it (xiangqi: shown only) |
| Step through the game | **← →** and the other keys of the board; the tree follows | **← →** and the other keys of the board; the tree follows |
| Hide the tree | *Tree* again, or **t** | **t** or **Esc** |

A position past the index's depth shows how many games reach it, and *Past the index's depth (30
moves): no next moves*. A position the index does not have says *0 games*. In xiangqi the tree's
moves are shown in ICCS (`h2e2`), and played from the board itself.

## The games reaching a position

*List them* above the tree (**l** in the terminal app's tree) goes back to the games list,
cut to the games that reach the position on the board, whatever their move order. The other
filters, the sort and **Enter** work as always. The list says *Only the games reaching the
tree's position*; *Show all games* (**c** in the terminal app, which clears every filter)
shows all again.

## Transpositions

A position is the same whatever move order led to it: 1.d4 Nf6 2.c4 e6 and 1.c4 e6 2.d4 Nf6 meet,
and the tree after either has every game of both. A position is told by its pieces, the side to
move, the castling rights and an en passant capture that can be made (a Zobrist key, as
Polyglot books have them). The same pieces with the other side to move are another position, and
so is one where a side can no longer castle.

## When the database changes

When the file changes, the index is out of date: the tree is still shown from it, marked *out of
date, built again in the background*, and a new one is built in the background (the same depth) as
soon as the table is open, whether or not *Position index for every games table* is on. When it is done, it takes the old one's place and the old one
is removed. A PGN file that only grew (games added at its end) keeps the old index's games and
reads only the new ones, and the last old one again. Any other change reads every game again.

## Room on the disk

An index takes about 9 bytes for each position of each game it reads, so about 0.5 KB a game at
30 moves. It is kept in the cache folder's `positions/`, apart from the games tables. The games
tables' limit of 4 GB does not count it, and nothing prunes it: it stays until you remove it,
or until its database file is gone from a folder that is still there (a disk that is not plugged
in keeps its indexes; an empty folder where a network share or a disk is mounted does not).

*Settings → Privacy and updates → Disk use* (`coxswain --disk`) has a row for each index,
`positions/<file name>`, with *Remove*. *Clear everything that can be built again* leaves the
indexes, as building one again takes minutes.

Measured on a 16-core machine on a database of 12.3 million games (a 12 GB PGN file):

| Depth | Built in | On the disk | Memory while it is built |
|---|---|---|---|
| 15 moves | 2 min 9 s | 2.8 GB | 290 MB |
| 30 moves | 3 min 11 s to 3 min 29 s (two runs) | 6.1 GB | 300 MB |
| The whole game | 4 min 55 s | 8.9 GB | 300 MB |

It uses up to half the computer's cores (eight at most); the memory is the same for a database of
any size, but for 8 bytes a game. A tree is read in 10 to 30 ms, the start position's (all 12.3
million games) too; the games list cut to the games after 1.e4 e5 (1.7 million of them) in under
a second. While it is built, the disk holds the sorted parts too, about as much again as the
index, removed once it is whole.

## Settings and config.toml

*Settings → Games → Position index*, in both apps:

| Setting | `config.toml` | Values |
|---|---|---|
| *Position index for every games table* | `[chess] position_index` | `false` (default): only a database whose *Build position index* is pressed gets one; `true`: every games table opened builds one in the background |
| *Position index depth* | `[chess] position_depth` | `"15"` or `"30"` (default) moves of each game, or `"all"`: the whole game |

```toml
[chess]
position_index = true
position_depth = "all"
```

`coxswain --settings=chess_positions` opens Settings there; `--settings=chess_position_index` and
`--settings=chess_position_depth` at each option. A change of the depth is used by
the next index built; one already there keeps its own depth, which the games list shows.

## Questions

#### Why does the tree say there is no position index?

The database has none yet: the index is built only when asked for. Press *Build position index*
under the games list's count (**i** in the terminal app's games list), or turn on *Settings →
Games → Position index for every games table*.

#### Does building the index change my file?

No. It reads every game once and writes the index into the cache folder; the database is never
changed.

#### How large will the index be, and how long will it take?

The button says it before it starts, from the games' lengths: about 9 bytes for each position
read, so about 0.5 KB a game at 30 moves, and roughly a minute for a million games on a
four-core computer. 15 moves takes about half of that, the whole game about half as much again.

#### Why is the best player not the one I expected?

The best player and the average Elo are those of the side making the move: White's for a White
move, Black's for a Black move. A game without an Elo for that side is not counted in either.

#### Why do two move orders show the same tree?

They reach the same position: the index finds a position by its pieces, the side to move, the
castling rights and an en passant capture, not by the moves that led there.

#### Does the index go when I clear the cache?

*Clear everything that can be built again* leaves it, since building it again takes minutes. Its
own row in *Disk use* removes it, and so does *Remove* (**I** in the terminal app) in the games
list. It also goes when its database file is gone.

#### What happens when I add games to my PGN file?

The index is marked out of date and built again in the background as soon as its table is open;
the old tree is shown meanwhile. A file that only grew reads only the games added.

#### Why does a deep position show games but no moves?

It is past the index's depth: an index of 30 moves knows the positions up to move 30 and the
games that reach them, not the moves after. *Position index depth* set to the whole game, and
the index built again (remove it, then build it), has them all.

---
[← Previous: Chess: playing on and the engine](chess-play.md) · [Next: Go games →](go.md)
