# Integration tests

Most tests here run with `cargo test --workspace`; a few are opt-in (each file's top comment
says how). `real_files.rs` is one: it runs with `COXSWAIN_REAL_FILES=1`, and in every nightly
run (`.github/workflows/nightly.yml`), never on pull requests.

## Real files

`real_files.rs` downloads files that Excel and ChessBase made, from where they are published,
and reads each as the apps do: a workbook's preview text and search text (and its passages
from the whole file), a ChessBase archive's table of games, every game on the board (no move
left unplayed, none cut short), its annotations and Find's line per game; then the search store
reads the folder of them all and must find each by a word of its own. `gui/src/real-files.test.js`
then shows each workbook's sheets as the desktop app's table preview does (SheetJS).

```sh
COXSWAIN_REAL_FILES=1 cargo test -p coxswain-core --test real_files -- --nocapture
cd gui && COXSWAIN_REAL_FILES=1 node --test src/real-files.test.js
```

- Each file is pinned by its SHA-256 and kept in `target/tmp/real-files` (never the user's
  cache). The test runs in a home of its own there (`HOME`, `XDG_*`).
- Offline, or a site down: the file is skipped with a `::warning::` line (the nightly run shows
  it on its page), and the test passes. A file whose bytes changed fails the test: look at the
  new file, then pin it again.
- No file is committed. Their licences:

| Files | Source | Licence |
|---|---|---|
| `excel-2010.xlsb`, `excel-2013.xlsb`, `excel-2016.xlsb` (8 KB each) | [DBHeise/fileid.testfiles](https://github.com/DBHeise/fileid.testfiles), saved by Excel 2010, 2013, 2016 | MIT |
| `attica-kteo-2018.xlsb` (10 KB, Greek) | [data.gov.gr](https://data.gov.gr/dataset/pepragmena-genikhs-diey8ynshs-metaforwn) | CC BY 4.0 |
| `barnet-absence-2019-09.xlsb` (45 KB, dates) | [open.barnet.gov.uk](https://open.barnet.gov.uk/) | Open Government Licence |
| `wrmp24-supply-demand.xlsb` (5.7 MB, 17 sheets, a million cells) | [Environment Agency](https://www.data.gov.uk/dataset/water-resource-management-plan-supply-demand-balance-forecast-collated-data) | Open Government Licence |
| `yorkshire-water-pr19-*.xlsb` (9.3 MB, 65 sheets; 5.8 MB, 154 sheets) | [Yorkshire Water on Data Mill North](https://datamillnorth.org/dataset/pr19-models-and-data-tables) | CC BY 4.0 |
| `twic1134.cbv` (0.8 MB, 6,117 games) | The Week in Chess 1134, as kept in [antoyo/uncbv](https://github.com/antoyo/uncbv)'s tests | The repository GPL-3.0; the games TWIC's, free for personal use |
| `WA_*.cbv`, `NWC-*.cbv` (12–48 KB, 7–230 games, annotated) | [Northwest Chess](https://nwchess.com/articles/games/WA_games_cb.htm), published for download | None stated: downloaded, never committed |

`NWC-2026-02-pgn.cbv` is ChessBase's backup of a PGN database: the archive holds a `.pgn` file,
no `.cbh`. No real archive of several databases was found (ChessBase's *Backup database* makes
one archive per database).
