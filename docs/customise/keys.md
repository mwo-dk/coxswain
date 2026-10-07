[← README](../../README.md) · [Docs index](../README.md) · [Customising](README.md)

# Changing keys

Every command of the panels has a name, an *action*, and the keys that run it. The `[keys]`
table in `config.toml` gives an action other keys, more keys, or none. Both apps read the same
table, so a key you change works the same in the terminal app and the desktop app.

![The desktop app's Help (F1) after the [keys] example on this page: Find shows Ctrl+P and Folder sizes Ctrl+K, among the other actions with their keys; the F-key bar at the bottom](../screenshots/customise-keys.png)

## Contents

- [How to use it](#how-to-use-it)
- [Key names](#key-names)
- [Rules](#rules)
- [The actions](#the-actions)
- [Keys you cannot change](#keys-you-cannot-change)
- [What you see](#what-you-see)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

1. Open `config.toml` (`coxswain --config-path` prints where).
2. Add a `[keys]` table, with an action and the list of keys that run it:

   ```toml
   [keys]
   quit = ["F10", "Ctrl+Q"]    # listing an action replaces its default keys
   search = ["Ctrl+P"]         # Find on Ctrl+P only; Alt+F7 and Ctrl+F are free now
   tag = []                    # [] unbinds it
   folder_sizes = ["Ctrl+K"]   # an action without a default key gets one
   ```

3. Start the app again: the terminal app, and the desktop app, read `[keys]` when they start.
   (The desktop app also reads it again after any change made in Settings.)
4. Press **F1**: the help lists every action with the keys it has now.

The action names and their defaults are in [the actions](#the-actions) below;
`coxswain --dump-config` prints the whole `[keys]` table with every default, to copy from.
What each action does: [Every default key](../panels/keys.md).

## Key names

| Write | For |
|---|---|
| `a`, `7`, `+`, `.`, `,` | A letter, a digit or a sign |
| `A` | Shift+A: a bare capital means Shift. `Ctrl+F` means Ctrl and the F key, without Shift |
| `F1` to `F24` | Function keys |
| `Enter` (or `Return`), `Esc` (`Escape`), `Tab`, `Backspace`, `Space` | |
| `Delete` (`Del`), `Insert` (`Ins`), `Home`, `End`, `PageUp` (`PgUp`), `PageDown` (`PgDn`) | |
| `Up`, `Down`, `Left`, `Right` | The arrow keys |
| `Ctrl+`, `Alt+`, `Shift+` in front | Held with it, in any order and any case: `ctrl+alt+x`, `Shift+F8` |

Names are the same in every [language](languages.md). The Mac's **Cmd** and the Windows key are
not modifiers Coxswain reads; use **Ctrl** or **Alt**.

## Rules

- **Listing an action replaces all its default keys.** List the defaults again to keep them:
  `quit = ["F10", "Ctrl+Q"]` keeps F10.
- **`[]` unbinds an action.** It stays in **F9**'s command list, without a key.
- **A plain character** bound to an action (`+`, `-`, `*`, a letter) only works while the
  command line is empty; once you have typed something, it goes into the command line.
- **A key bound to two actions runs one of them**, the one further down the table below, with no
  warning. Bind each key once; unbind the default holder first.
- **An Alt+letter that you bind is no longer a [quick search](../panels/quick-search.md)** for
  that letter.
- **An unknown key name is refused.** The terminal app does not start and says
  `coxswain: unknown key 'Ctlr+P'`; the desktop app starts with every default and prints
  `coxswain: unknown key 'Ctlr+P'; using defaults` on the terminal it was started from. An
  unknown *action* name is refused the same way, as a config error; that includes the names
  2.0 renamed (`mkdir`, `dir_sizes`, `select_group` …), see
  [Renamed in 2.0](../reference/configuration.md#renamed-in-20).

## The actions

| Action | Default | Action | Default |
|---|---|---|---|
| `help` | **F1** | `goto_left` | **Alt+F1** |
| `user_menu` | **F2** | `goto_right` | **Alt+F2** |
| `view` | **F3** | `same_dir` | **Alt+O** |
| `edit` | **F4** | `sort_name` | **Ctrl+F3** |
| `copy` | **F5** | `sort_ext` | **Ctrl+F4** |
| `move` | **F6** | `sort_time` | **Ctrl+F5** |
| `new_folder` | **F7** | `sort_size` | **Ctrl+F6** |
| `delete` | **F8**, **Delete** | `copy_path` | **Ctrl+Enter**, **Ctrl+J** |
| `delete_forever` | **Shift+F8**, **Shift+Delete** | `new_tab` ¹ | **Ctrl+T** |
| `menu` | **F9** | `close_tab` ¹ | **Ctrl+W** |
| `quit` | **F10** | `next_tab` ¹ | **Ctrl+Tab** |
| `up` | **Up** | `prev_tab` ¹ | **Ctrl+Shift+Tab** |
| `down` | **Down** | `toggle_preview` ¹ | **Space** |
| `page_up` | **PageUp**, **Left** | `toggle_view` ¹ | **Alt+V** |
| `page_down` | **PageDown**, **Right** | `toggle_sidebar` ¹ | **Ctrl+B** |
| `home` | **Home** | `edit_path` ¹ | **Ctrl+L** |
| `end` | **End** | `folder_sizes` | none |
| `open` | **Enter** | `batch_rename` ¹ | **Ctrl+M** |
| `parent` | **Ctrl+PageUp**, **Backspace** | `tag` ¹ | **Alt+T** |
| `switch_panel` | **Tab** | `notes` ¹ | **Alt+N** |
| `mark` | **Insert**, **Shift+Down** | `back` ¹ | **Alt+Left** |
| `mark_group` | **+** | `forward` ¹ | **Alt+Right** |
| `unmark_group` | **-** | `clip_copy` ¹ | **Ctrl+C** |
| `invert_marks` | **\*** | `clip_cut` ¹ | **Ctrl+X** |
| `search` | **Alt+F7**, **Ctrl+F** | `paste` ¹ | **Ctrl+V** |
| `search_text` | **Shift+F7**, **Ctrl+Shift+F** | `properties` | **Alt+Enter** |
| `ask` | **Ctrl+F7** | `extract` | **Ctrl+E** |
| `refresh` | **Ctrl+R** | `pack` | **Alt+F5** |
| `swap_panels` | **Ctrl+U** | `columns` ¹ | none |
| `toggle_panels` | **Ctrl+O** | `duplicates` ¹ | **Ctrl+D** |
| `toggle_hidden` | **Alt+.** | `settings` ² | **Ctrl+,** |
| `history` | **Ctrl+G** | `branches` | **Alt+B** |
| `worktrees` | **Alt+W** | `switch_branch` | **Alt+S** |
| `new_branch` | none | `snapshots` | **Alt+Z** |
| `flags` | none | `package` | none |
| `undo` | **Ctrl+Z** | | |

¹ The desktop app only. In the terminal app the key still belongs to the action, and pressing it
says *… is available in the desktop app (coxswain-gui)* on the command line.

² Both apps, but most terminals never pass **Ctrl+,** on: in the terminal app use **F9** →
*Settings*, or bind it to a key the terminal sends, such as `settings = ["Alt+,"]`.

"Further down" for two actions on one key means: first the left column from top to bottom, then
the right column.

## Keys you cannot change

These belong to where they are, not to an action:

| Where | Keys | Desktop app | Terminal app |
|---|---|---|---|
| Every dialog | **Esc** closes, **Enter** confirms | Yes | Yes; the `quit` key (**F10**) closes a dialog too |
| A *Delete* or other question | **Y** / **N** as well as Enter / Esc | Yes | Yes |
| Find | **Tab** / **Shift+Tab** go to the next / previous kind, **Up**/**Down**/**PageUp**/**PageDown** move over the rows, **Ctrl+Enter** (desktop) / **Alt+Enter** (terminal) asks, **F1** shows the syntax, **Delete** sends a tip away (the `search`, `search_text` and `ask` keys, which you can change, switch the scope, *In files* and ask) | Yes | Yes |
| *Colour tag* | The digits pick a colour | Yes | (not there) |
| Quick search | **Alt+letter** starts it, letters extend it, **Backspace** shortens, **Esc** ends | Yes | Yes |
| The command line, with text in it | **Enter** runs, **Esc** clears, **Backspace**, and in the desktop app **Left**/**Right**/**Home**/**End**/**Delete** edit | Yes | **Enter**, **Esc**, **Backspace** |
| The preview pane | **Esc** closes it | Yes | (no preview) |
| Thumbnails view | Arrows move in two dimensions | Yes | (no thumbnails) |
| Columns view | **Left** / **Right** walk up and down the tree | Yes | (no columns view) |
| A text field (notes, the path bar) | Typing stays in the field; **Esc** leaves it; function keys still work | Yes | |

## What you see

The **F-key bar** at the bottom shows, for **F1** to **F10**, the action you bound to each, with
its name in your language: bind `F2` to `pack` and the bar says *Pack into an archive* there.
The desktop app's bar says *Move* (**F6**), *New folder* (**F7**) and *Commands* (**F9**); the
terminal app's bar keeps Norton Commander's *RenMov*, *Mkdir* and *PullDn* for those three
actions, wherever you bind them.
**F1** (help) lists every action with all the keys bound to it; **F9** (the command list) shows
each action with its first key. An action without a key has an empty key column. In the desktop app the Settings button's tooltip
shows the `settings` key.

## Settings and config.toml

| Key | Type, default |
|---|---|
| `[keys] <action>` | a list of key names; the defaults in [the actions](#the-actions) |

Settings has no key editor: keys are set in `config.toml` only. Settings → *Keys*
([Keys](settings.md#keys)) lists every action with its keys, read-only, with a filter, the line
*Keys are changed in config.toml under [keys]* and **Open config.toml**. **Open config.toml** opens `config.toml` at its `[keys]` table: in your `editor` at that line when
the editor takes `+line` (vi, vim, nvim, nano, emacs, micro, kak, …), else with the program
your system opens `.toml` files with. When the file has no `[keys]` yet, a commented `[keys]`
example is written at its end first, so there is a place to start.

In the terminal app, Settings → *Keys* starts with the row *Change keys in config.toml, under
[keys]*: **Enter** on it opens your editor (`editor`, `$VISUAL`, `$EDITOR`, else `vi` or
Notepad) at `[keys]`, and Settings comes back when you quit the editor.

## In the terminal app

The same `[keys]` table, read at start. Two differences come from the terminal:

- **Some keys never reach it.** Many terminals keep **Ctrl+Tab**, **Ctrl+Enter**,
  **Ctrl+PageUp**, **Shift+Down** or **Ctrl+,** for themselves, or send them as something else.
  This is why several actions have a second default (**Ctrl+J** for `copy_path`, **Backspace**
  for `parent`, **Insert** for `mark`, **Shift+F7** for `search_text`: many terminals send
  **Ctrl+Shift+F** as **Ctrl+F**, which opens Find at names). If a key does nothing, bind the action to one the
  terminal passes on.
- **Desktop-only actions** (marked ¹ above) are in neither its help nor its command list.

## Questions

#### Where do I change a key?

In `config.toml`, under `[keys]`. The quickest way there: Settings (**Ctrl+,**) → *Keys* →
**Open config.toml** in the desktop app, or the row *Change keys in config.toml, under [keys]*
and **Enter** in the terminal app. Both open the file at `[keys]`. Restart the app afterwards.

#### How do I keep the default key and add my own?

List both: `search = ["Alt+F7", "Ctrl+F", "Ctrl+P"]`. Listing an action replaces all of its
defaults, so a list with only `"Ctrl+P"` takes Alt+F7 and Ctrl+F away.

#### I bound a key and the app will not start. What is wrong?

A key name it does not know, such as `"Ctlr+P"` or `"Hyper+Q"`. The terminal app stops with
`coxswain: unknown key 'Ctlr+P'`; the desktop app starts with the defaults and prints the reason
on its terminal. Check the spelling against [Key names](#key-names).

#### My `[keys] mkdir` stopped working in 2.0. Why?

2.0 renamed it `new_folder`; `dir_sizes` is `folder_sizes`, and `select_group`,
`unselect_group`, `invert_selection` are `mark_group`, `unmark_group`, `invert_marks`. The
first start of 2.0 renames them in `config.toml` itself and says so in a notice (*Coxswain 2.0
renamed keys in config.toml, your comments kept: [keys] mkdir → new_folder*). If you typed the
old name in after that, or copied an old config over, the terminal app refuses to start and
the desktop app uses the defaults: write the new name. The table is in
[Renamed in 2.0](../reference/configuration.md#renamed-in-20).

#### I had both `mkdir` and `new_folder`. Which one stayed?

`new_folder`. When the new name is there already, it wins and the old line is removed; the
notice says *[keys] mkdir (new_folder kept)*.

#### Can I use Cmd on a Mac?

No. Coxswain reads Ctrl, Alt (Option) and Shift. Bind the action to a Ctrl or Alt key.

#### Why does my `+` key do nothing after I start typing a command?

A plain character bound to an action only runs it while the command line is empty; with text
there, `+` is part of the command. Press **Esc** to clear the line, or bind the action to a key
with Ctrl or Alt.

#### I bound Alt+G to something, and now quick search does not jump to "g" files.

A bound Alt+letter runs its action; only unbound Alt+letters start a
[quick search](../panels/quick-search.md). Pick a letter you do not quick-search with, or use
Ctrl.

#### How do I give Folder sizes or the Columns menu a key?

They have none by default (`folder_sizes`, `columns`): add one, `folder_sizes = ["Ctrl+K"]`. On a
Mac, Ctrl+Space belongs to the system, which is why `folder_sizes` has no default.

#### Can I change the keys inside dialogs, such as Tab in Find?

No: keys inside dialogs, quick search and the command line are fixed (see
[Keys you cannot change](#keys-you-cannot-change)). Only the actions in `[keys]` can be moved.
Inside Find the keys of `search`, `search_text` and `ask` act too: `search` switches the
scope, `search_text` switches to *In files* and back, `ask` asks. Moving those keys moves that
too.

#### How do I go back to the default keys?

Delete the action's line from `[keys]`, or the whole table. `coxswain --dump-config` shows what
the defaults are.

---
[← Previous: Languages](languages.md) · [Next: Glyphs and fonts →](glyphs-and-fonts.md)
