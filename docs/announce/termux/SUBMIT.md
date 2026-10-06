# Announcing Coxswain to Termux users

Drafts only. Nothing here is posted for you: every post goes out by hand, from your own
accounts, in your own words where you want them.

| File | Where |
|---|---|
| [reddit.md](reddit.md) | r/termux |
| [github-discussion.md](github-discussion.md) | <https://github.com/termux/termux-app/discussions>, category *Show and tell* |
| [matrix.md](matrix.md) | The Termux Matrix room (its address is on <https://termux.dev>, *Community*) |

## The order

The package first, then the posts. A post that says `pkg install coxswain` before the package
is in Termux's repository gets *Unable to locate package* replies.

1. **Submit the package** to termux-packages: follow
   [packaging/termux/SUBMIT.md](../../../packaging/termux/SUBMIT.md) to the end.
2. **Wait until it lands.** When the pull request is merged, check in Termux on a phone:
   `pkg update && pkg install coxswain && cox --version`. It usually reaches the mirrors
   within a day.
3. **Update the repository** in a pull request of its own: the README's install row and
   `docs/reference/termux.md` say `pkg install coxswain` as the plain command (step 7 of the
   packaging SUBMIT.md).
4. **Check F8.** If [#141](https://github.com/mwo-dk/coxswain/pull/141) (a trash in Termux) has
   merged and that version is the one in Termux's repository, swap the F8 line in each draft
   for the one in its comment. Otherwise leave the drafts as they are.
5. **Record the GIF** (below) and upload it with the posts.
6. **Post on r/termux** ([reddit.md](reddit.md)). Read the subreddit's rules on the day first;
   choose a flair if one is asked for. Delete the HTML comments before pasting.
7. **Post in Discussions** ([github-discussion.md](github-discussion.md)) a day or two later,
   so you can fold in what Reddit asked.
8. **Say it in Matrix** ([matrix.md](matrix.md)), once, and stay a while for questions.
9. Answer questions in the first days; bugs go to issues in mwo-dk/coxswain.

If the package was declined and went to the Termux User Repository instead, every install line
becomes `pkg install tur-repo && pkg install coxswain`. If it is in neither yet and you want to
post anyway, use the `cargo install` lines in the comments, and say plainly that a package is
on its way.

## The GIF

The drafts carry a marker where it goes:

```
<!-- gif: termux-alt-f1.gif: … -->
```

What it must show: Coxswain in Termux on a real phone (portrait), **Alt+F1** opening the go-to
list with *Phone: downloads* and *Phone: dcim*, **Enter** on Downloads, then **Enter** on a
picture opening Android's *Open with* chooser. Under ten seconds, Classic blue (NC) theme. Use a
phone, or a storage folder, with no personal files in it: a fresh user profile, or an emulator
with a few demo pictures from `docs/screenshots/demo-home.sh`. Android's own screen recorder
works; convert with `ffmpeg -i in.mp4 -vf "fps=12,scale=540:-1" termux-alt-f1.gif`. Keep it
under 5 MB for Reddit and GitHub, and save it as `docs/screenshots/termux-alt-f1.gif` too.

## What not to do

- Do not post from a bot or a fresh account made for it.
- Do not open issues or pull requests in Termux's repositories to advertise; the package pull
  request is the only one.
- Do not post the same text in several places on the same day.
