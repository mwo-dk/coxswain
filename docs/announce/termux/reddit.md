# r/termux

**Title:** Coxswain: a two-panel, Norton Commander style file manager that knows it is in Termux (phone folders on Alt+F1, termux-open, Termux:API)

Hi all,

Coxswain is a two-panel file manager for the terminal, in the Norton Commander tradition,
written in Rust and MIT licensed. It is now in Termux's package repository:

    pkg install coxswain

<!-- Until the termux-packages pull request is merged, use instead:
    pkg install rust git
    cargo install coxswain
(a few minutes on a phone; add ~/.cargo/bin to PATH if `coxswain` is not found) -->

What it does everywhere: two panels and the F-key bar (F5 copy, F6 move, F8 delete), git
status for every file, zip, 7z and tar archives opened like folders, and a search that finds
any file by name as you type and the words inside documents. Nothing leaves the phone apart
from a daily update check, which can be turned off.

What is special in Termux:

- **Your phone's folders on Alt+F1.** Run `termux-setup-storage` once and allow it. Alt+F1
  (left panel) or Alt+F2 (right) then lists *Phone: shared*, *Phone: downloads*,
  *Phone: dcim* and the rest, and a memory card when there is one. Until you have run it, the
  status line tells you so once.
- **Enter opens files in their Android app** through `termux-open`, so a photo or a PDF goes
  to Android's *Open with* chooser.
- **Clipboard and battery through Termux:API.** With the Termux:API app and
  `pkg install termux-api`, copied lines land on Android's clipboard, and the search helper
  waits while the phone runs on its battery instead of reading files.
- **F8 and the phone's storage.** Android has no trash, so F8 says so, and Shift+F8 deletes
  for good after asking.
  <!-- If #141 has merged and is in the released package, use this line instead:
  **A trash in Termux.** F8 moves files to `~/.local/share/Trash`; on the phone's shared
  storage, which the trash cannot reach without copying, it asks before deleting for good. -->
- No F-keys on the phone keyboard? Put them in Termux's extra keys row; the guide has the
  two lines for `~/.termux/termux.properties`.

<!-- gif: termux-alt-f1.gif: Coxswain in Termux on a phone, Alt+F1 opening the go-to list with
*Phone: downloads* and *Phone: dcim*, Enter on Downloads, then Enter on a picture opening
Android's chooser. Under 10 seconds, portrait. -->

Honest limits: this is the terminal app only. The desktop app needs WebKitGTK on a Linux
desktop and does not run on Android. Search by meaning works on the phone's processor but is
slow for many files; a model server on another computer does it faster. Android may stop the
background reading when the screen is off unless you take a wakelock (`termux-wake-lock`).

Termux guide: https://github.com/mwo-dk/coxswain/blob/master/docs/reference/termux.md
Source: https://github.com/mwo-dk/coxswain

I wrote it and would love to hear how it behaves on your phone, and which keys feel awkward on
a touch keyboard.
