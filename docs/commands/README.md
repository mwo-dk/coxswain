[← README](../../README.md) · [Docs index](../README.md)

# Commands, the user menu and scripts

Under the panels is a command line, as in Norton Commander: whatever you type that is not a key
goes there, and **Enter** runs it in the active panel's folder. **F2** runs commands you keep in
`config.toml` and, in the desktop app, scripts you keep in a folder. **F3** and **F4** view and
edit, and **Enter** opens a file in its default application. The two apps differ most here: the
terminal app hands the terminal to the program, the desktop app collects its output.

![The terminal app in the Classic blue theme: two panels, and under them the command line with the prompt /home/demo/projects/rocket> and a cursor, then the F-key bar with 2 Menu, 3 View and 4 Edit](../screenshots/tui-panels.png)
*The terminal app's command line under the panels, with the folder as its prompt.*

| Page | What it covers |
|---|---|
| [The command line and its output](command-line.md) | Typing a command, `cd`, **Ctrl+Enter** to add a name, the shell used, and where the output shows in each app (**Ctrl+O**, the *Command output* pane) |
| [The user menu, F2](user-menu.md) | A terminal here, a file's SHA-256 and `git status` per system, *Add your own command…*, `[[user_menu]]`, `%f` `%d` `%s` `%%`, `wait`, `when`, entries to copy |
| [Scripts](scripts.md) | The desktop app's `scripts` folder: every file in it is an entry of **F2**, run with the marked files as arguments |
| [View and edit, F3 and F4](view-and-edit.md) | The viewer and the editor: `viewer`, `editor`, `$PAGER`, `$VISUAL`, `$EDITOR`, and why **F3** is the preview in the desktop app |
| [Opening files](opening-files.md) | **Enter** and a double-click: folders, archives, programs, and files in their default application |

## Keys at a glance

| Key | Desktop app | Terminal app | Does |
|---|---|---|---|
| Any plain character | Goes into the command line | Goes into the command line | Starts a command |
| **Enter** | Runs the line (when it has text), else opens | Runs the line (when it has text), else opens | [Command line](command-line.md), [Opening files](opening-files.md) |
| **Esc** | Clears the line | Clears the line | |
| **Ctrl+Enter**, **Ctrl+J** | Adds the name under the cursor to the line | Same | *Path to command line* |
| **Ctrl+O** | One pane or two | Shows the last command's output | *Panels on/off* |
| **F2** | *Scripts* menu: `[[user_menu]]` and script files | *User menu*: `[[user_menu]]` | *Menu* |
| **F3** | Shows or hides the preview pane | Opens the file in the viewer (`less`) | *View* |
| **F4** | Opens the file in `editor`, else its default application | Opens the file in the editor (`vi`) | *Edit* |

Every key can be changed under `[keys]` ([Changing keys](../customise/keys.md)).

---
[← Previous: The sidebar](../organise/sidebar.md) · [Next: The command line and its output →](command-line.md)
