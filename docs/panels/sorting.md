[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Sorting and hidden files

Sort a panel by name, extension, time or size with **Ctrl+F3** to **Ctrl+F6**, or by
clicking a column header in the desktop app. **Alt+.** shows or hides hidden files.

![The desktop app's details view: the Name header carries the sort arrow, folders come before files](../screenshots/gui-folder-sizes.png)
*Sorted by name: the arrow next to* Name *marks the column and the direction.*

## Contents

- [How to use it](#how-to-use-it)
- [What you see](#what-you-see)
- [Hidden files](#hidden-files)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

| Key | Sort by | First order |
|---|---|---|
| **Ctrl+F3** | Name | A to Z |
| **Ctrl+F4** | Extension, then name | A to Z |
| **Ctrl+F5** | Time modified, then name | Newest first |
| **Ctrl+F6** | Size, then name | Largest first |

Press the same key again to reverse the order. Another key starts that order the normal way
round.

In the desktop app's details view, click a header instead: **Name**, **Type** (extension),
**Size** or **Modified**. A second click reverses. *Files* and *Created* do not sort.

The rules, whatever the order:

- `..` stays first, then folders, then files.
- Names sort naturally and case-insensitively: `file2` before `file10`, `a.txt` next to `A.txt`.
- Sorting by size uses the size the file system gives. Folders are not sorted by their
  measured size; among themselves they keep name order.

## What you see

- Terminal app: a letter at the bottom right of the panel's border: `n` name, `x`
  extension, `t` time, `s` size; upper case (`N`, `X`, `T`, `S`) when reversed.
- Desktop app: an arrow next to the header that sorts, pointing down for the normal order and
  up when reversed.

The cursor stays on the same name when the order changes.

## Hidden files

**Alt+.** shows or hides hidden files in both apps, in every panel. Hidden are names that
start with a dot, and on Windows files with the hidden attribute. Shown, they are drawn in the
theme's `hidden` colour (dim). Inside an archive the same rule applies to the names in it.

## Settings and config.toml

| Setting | config.toml | Type, default |
|---|---|---|
| *Settings → Behaviour → Show hidden files when Coxswain starts* | `show_hidden` | bool, `true` |
| – (keys) | `sort_name`, `sort_ext`, `sort_time`, `sort_size` | `Ctrl+F3`, `Ctrl+F4`, `Ctrl+F5`, `Ctrl+F6` |
| – (key) | `toggle_hidden` | `Alt+.` |

`show_hidden` is where both apps start. The desktop app then keeps your last choice in its
session, so it starts with the state you left; the terminal app starts from the config each
time.

## In the terminal app

The same keys, rules and hidden-file switch. There are no headers to click, so the letter at
the bottom right is the only sign of the order. Each panel has its own order, and every start
begins with name order. In the desktop app every tab has its own order, and it is kept in the
session ([What the apps remember](session.md)).

## Questions

#### Why are folders not sorted by their size?

The order uses the sizes the file system gives, and a folder's is not its contents. Folder
sizes are shown ([Folder sizes](folder-sizes.md)) but not sorted by; folders stay in name
order among themselves.

#### Ctrl+F3 does nothing in my terminal. Why?

Some terminals do not send Ctrl with function keys, or keep them for themselves. Pick the
order from the command list instead (**F9**, type `sort`), or bind other keys:
`[keys] sort_time = ["Ctrl+F5", "Ctrl+Q"]` ([Changing keys](../customise/keys.md)).

#### Why is `..` at the top even when I reverse the order?

It is the way up, not a folder of this one, and stays first so **Home** and **Backspace**
work the same in every order. Folders stay before files for the same reason.

#### How do I see the newest files first?

**Ctrl+F5**. Press it again for the oldest first.

#### Why are hidden files shown by default?

`show_hidden` is `true` by default, so nothing is hidden from you by surprise. Set it to
`false`, or untick *Show hidden files when Coxswain starts*, to start with them hidden.

#### I hid hidden files in the desktop app, and they stay hidden after a restart. Why?

The desktop app remembers the switch in its session, and the session wins over
`show_hidden`. Press **Alt+.** once to show them again.

#### Does sorting by extension group `.tar.gz` files?

It sorts by the part after the last dot, so `.tar.gz` files sort with the other `.gz` files.

---
[← Previous: Marking files](marking.md) · [Next: Tabs, back and forward, one pane or two →](tabs-and-panes.md)
