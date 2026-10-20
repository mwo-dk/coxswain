# Changelog fragments

One file per pull request with a change a user would notice, named after its branch (`changelog.d/<branch>.md`), holding one line:

    | **What a user gets.** In plain words. | [Page](path.md#anchor) |

A feature (not a fix) ends the line with ` <!-- feature -->`, after the last `|`: the release is then a minor one, otherwise a patch (`packaging/release-pr.py`). Links are written as from `docs/`. The release pull request (opened on Sundays by `release-pr.yml`) moves them into one row of `docs/changelog.md` and deletes the files. One file each, so two pull requests never clash.
