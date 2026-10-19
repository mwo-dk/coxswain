# Changelog fragments

One file per pull request with a change a user would notice, named after its branch (`changelog.d/<branch>.md`), holding one line:

    | **What a user gets.** In plain words. | [Page](path.md#anchor) |

Links are written as from `docs/`. The release pull request moves them into one row of `docs/changelog.md` and deletes the files. One file each, so two pull requests never clash.
