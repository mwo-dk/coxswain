[← README](../../README.md) · [Docs index](../README.md) · [Panels and keys](README.md)

# Quick search

Hold **Alt** and type the start of a name: the cursor jumps to the first entry in the active
panel that starts with it. It is the fastest way to a file you can see, without the mouse.

<!-- screenshot: panels-quick-search.png: terminal app, Classic blue (NC) theme, the command line row reading "Quick search: bud" and the cursor on budget.xlsx in the right panel -->

## How to use it

1. Press **Alt** with the first letter of the name, for example **Alt+B**. The cursor jumps
   to the first name starting with `b`.
2. Keep typing, plain letters now (Alt is no longer needed; with Alt works too): `u`, `d`.
   The cursor moves to the first name starting with `bud`.
3. End it:

| Key | Does |
|---|---|
| **Esc** | Ends the quick search; nothing else happens |
| **Enter** | Ends it and opens the entry under the cursor |
| **Backspace** | Takes the last letter back (the cursor stays where it is) |
| Any other key | Ends it, then does its own work (**F5** copies, **Down** moves, …) |

It is case-insensitive and looks at the start of names only. Names that do not match leave
the cursor where it was.

## What you see

The command line row shows `Quick search: bud` while it lasts, in place of the folder and the
prompt. The cursor moves in the panel; nothing is marked or filtered.

## Settings and config.toml

None. Quick search starts on any **Alt** key that is not bound to an action; the keys bound by
default are **Alt+T** (`tag`), **Alt+N** (`notes`), **Alt+V** (`toggle_view`), **Alt+O**
(`same_dir`), **Alt+.** (`toggle_hidden`) and the Alt+F-keys. To free one, unbind its action:
`[keys] tag = []` ([Changing keys](../customise/keys.md)).

## In the terminal app

The same. Two small differences:

- The terminal app starts a quick search on **Alt** with any character that is not bound; the
  desktop app on **Alt** with a letter A-Z or a digit.
- Many terminals send **Alt+letter** as **Esc** followed by the letter. That works; but on a
  Mac, Terminal and iTerm2 need *Use Option as Meta key* (or *Esc+*) for it.

## Questions

#### Why does Alt+T not jump to names with a "t"?

**Alt+T** is bound to *Colour tag*, and a bound key does its action. The same goes for
**Alt+N**, **Alt+V** and **Alt+O**. In the terminal app, where tags, notes and views do not
exist, **Alt+T** says *Colour tag is available in the desktop app (coxswain-gui)*. Start with
another letter of the name, or unbind the key (`[keys] tag = []`). Once a quick search runs,
**Alt+T** extends it like a plain `t`.

#### Can it find a name that contains the letters in the middle?

No, only the start of names. For "names in this folder" anywhere in the name, open
[Find file](../search/find-file.md) (**Alt+F7**) and press **Tab** once: it searches this
folder, with the full [name syntax](../search/name-syntax.md).

#### Why does Backspace not move the cursor back?

It shortens what you typed, and the next letter searches again from the start of the list.
The cursor stays on the last match meanwhile.

#### Typing letters without Alt goes to the command line. Why?

Plain letters only extend a quick search that is running. Without one, they are typed into
the command line, as in NC. Start with **Alt**.

#### Does it look in hidden files?

Only in what the panel shows. With hidden files hidden (**Alt+.**), they are not in the list,
so it cannot land on them ([Hidden files](sorting.md#hidden-files)).

#### Alt+letter does nothing in my terminal. Why?

The terminal keeps Alt for itself (menus), or on a Mac turns Option+letter into a special
character. Turn on *Use Option as Meta key* (Terminal) or set Option to *Esc+* (iTerm2), or
check the terminal's own key settings.

---
[← Previous: Moving around and going to a folder](moving.md) · [Next: Marking files →](marking.md)
