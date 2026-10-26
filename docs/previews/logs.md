[← README](../../README.md) · [Docs index](../README.md) · [The preview pane](README.md)

# Logs: follow, filter, columns

A log, a JSON Lines file or any text too big to read at once opens as a log view in both apps.
Its lines are coloured by level, and only the lines on screen are read, so a 20 GB log opens at
once. You can follow it as it is written, and stop at the next error. You can keep only the
warnings and errors, or the lines with a text or a field's value. Its fields can show as
columns, and its stack traces fold under the line they belong to.

![The desktop app in the Cyber theme with launch.log under the cursor in Documents: the preview pane at the right shows the log view with All levels, the filter box, Fold traces on, Follow and Stop at errors above the lines, the line log · 7 of 10 lines, and the log's lines with line numbers, the DEBUG line grey, the WARN line yellow and the ERROR line orange](../screenshots/logs-gui.png)
*launch.log in the desktop app's preview: its three lines of stack trace are folded under the ERROR line (7 of 10 lines).*

![The terminal app's viewer in Classic blue on telemetry.jsonl: the title names JSON Lines, UTF-8 and 5 lines, a header row time, level, msg, thrust_kn, user above five rows in columns coloured by level (debug grey italics, warn yellow, error red), the line Line 1 · 100% · traces folded and the key line starting F follow · E stop at errors](../screenshots/logs-tui.png)
*telemetry.jsonl in the terminal app's viewer, its fields as columns (**c**).*

## Contents

- [How to use it](#how-to-use-it)
- [Which files](#which-files)
- [Formats and their fields](#formats-and-their-fields)
- [Levels](#levels)
- [Stack traces](#stack-traces)
- [Following a log](#following-a-log)
- [Filtering](#filtering)
- [What you see](#what-you-see)
- [In the terminal app](#in-the-terminal-app)
- [Settings and config.toml](#settings-and-configtoml)
- [Questions](#questions)

## How to use it

1. Put the cursor on a log and press **F3**: the desktop app's preview pane shows it, the
   terminal app's [built-in viewer](../commands/view-and-edit.md#the-built-in-viewer) opens it
   full screen.
2. To follow it from the start, choose **Follow log** in the action menu (**Shift+F10**) or in
   **F9**.
3. Use the keys below. In the desktop app, click into the lines first: the keys work while the
   log view has the focus, and **Esc** gives them back to the panels. Each also has a control
   above the lines.

| Key | Does | Desktop app control |
|---|---|---|
| **F** | Follow the file as it is written; **F** again stops | **Follow** |
| **E** | While following, stop at the next error | **Stop at errors** |
| **e** | All levels → warnings and errors → errors only → all again | The level list |
| **&** | Keep only the lines with a text, or `name=value` | The filter box |
| **c** | Fields as columns, or the lines as they are | **Columns** |
| **z** | Fold stack traces under their line, or show them in full | **Fold traces** |

**F**, **E** and **&** work on any text file. **e**, **c** and **z** work on logs.

## Which files

- **By name:** `.log`, `.jsonl` and `.ndjson`, rotated ones (`app.log.1`, `syslog.2`), and the
  usual names in `/var/log` without an extension: `syslog`, `messages`, `kern.log`,
  `auth.log`, `daemon.log`, `secure`, `maillog`, `access_log`, `error_log`, `dmesg`.
- **By content:** any text file whose first lines are in one of the formats below. A
  `server.out` in JSON Lines is a log too.
- **By size, in the desktop app:** a text file over 512 KB opens in the log view, plain, so
  all of it can be read and searched with the filter. Files under that size are highlighted
  as code, as before ([Text and code](text-and-code.md)).

A compressed log (`syslog.2.gz`) opens as an [archive](media.md); extract it first.

## Formats and their fields

The format is told from the first 50 lines. Empty lines are left out.

| Format | Told by | Fields (columns) |
|---|---|---|
| JSON Lines | 60 % of the lines are JSON objects | Every key; a nested object as `http.status`; time, level and message first; at most 12 |
| logfmt | 60 % have three `key=value` pairs or more | The same, from its keys |
| Access log (Apache, nginx: common and combined) | 80 % match the format | `client`, `ident`, `user`, `time`, `request`, `status`, `size`, `referrer`, `agent` |
| syslog (RFC 3164, RFC 5424, rsyslog's ISO time) | Half the lines | `time`, `host`, `app`, `pid`, `message` (and `msgid`, `data` in RFC 5424) |
| W3C extended (IIS) | Its `#Fields:` line | The fields that line names |
| Plain text | Anything else | None: the lines as they are |

A field that has no column (a key only some lines have) shows after the columns as
`name=value`. A line without fields (the next line of a stack trace) shows as it is.

## Levels

A line is coloured by its level, in the theme's colours:

| Level | Colour | From |
|---|---|---|
| Error | the git-deleted red | `ERROR`, `ERR`, `FATAL`, `CRIT`, `CRITICAL`, `PANIC`, `SEVERE`, `[error]`, `[crit]`, `[alert]`, `[emerg]` |
| Warning | the git-modified yellow | `WARN`, `WARNING`, `[warn]` |
| Info | the git-added green | `INFO`, `NOTICE`, `[info]`, `[notice]` |
| Debug | the hidden grey | `DEBUG`, `TRACE`, `VERBOSE`, `[debug]` |

- **JSON Lines and logfmt:** the field `level`, `lvl`, `severity`, `loglevel` (or a few
  others), in any case, or pino's and bunyan's numbers (50 and up error, 40 warning, 30 info).
- **Access and W3C logs:** status 500 and up is an error, 400 to 499 a warning; the rest have
  no level.
- **syslog:** the priority's severity, when the line has one; else the words above.
- **Plain text:** the words above, in capitals or in brackets. The highest one a line names
  wins.

The lines of a stack trace take the level of the line they belong to.

## Stack traces

These lines belong to the line above them:

- an indented line (a space or a tab first);
- Java's `at …(File.java:12)`, `Caused by:`, `Suppressed:`, `... 12 more`, and the exception's
  own line (`java.lang.IllegalStateException: no fuel`);
- a Python traceback, from `Traceback (most recent call last):` to its last line
  (`ValueError: …`), with `During handling of the above exception` and the traceback after it;
- Go's `goroutine 1 [running]:`;
- in JSON Lines, any line that is no JSON object (what a program printed among them).

With **Fold traces** on (**z**), they fold under their line, which ends in **▸ +12 lines**. A log
up to 64 MB opens folded. A bigger one opens unfolded, so its end shows at once; press **z** to
fold it. In the desktop app a click on **▸ +12 lines** unfolds them all.

## Following a log

**F** (**Follow** in the desktop app) looks at the file four times a second:

- Lines written to its end show at the bottom, and the view stays at the end.
- A log rotated by renaming it (`app.log` → `app.log.1`, and a new `app.log`) is read again
  from the new file's start. So is a log cut short in place (logrotate's `copytruncate`).
- Moving up (**↑**, **PgUp**, **Home**, the mouse wheel up) stops following. **End** goes to the
  end again; **F** follows again.

**E** (**Stop at errors**) makes following stop at the next error written: the view stays on
it, its line number is marked, and the line under the text says *Stopped following at an error
on line 812*. Only errors written while following count, not the ones already there.

## Filtering

- **Level:** **e**, or the level list: *All levels*, *Warnings and errors*, *Errors only*.
- **Text:** **&**, type the text, **Enter**. Case does not count. An empty filter shows every
  line again; so does **×** in the desktop app's filter box.
- **A field's value:** type `name=value` (`user=ada`, `status=503`): only the lines whose field
  has that value, case aside. In the terminal app **Tab** finishes a field's name, and the line
  under the text lists the fields. In the desktop app a click on a value in a column filters on
  it.

A filter keeps a line with the stack trace under it: a text found only in the trace keeps the
whole entry. Filters run through the file on a thread as its lines are found. The title says
*312 of 48210 lines* (*312 so far of …* until it has gone through all of them), and the view
stays on the line it showed.

## What you see

- **Desktop app:** the controls above the lines, then a line with the format and the count
  (*JSON Lines · 2 of 2 lines*) and what the last control did. With **Columns** on, the column
  names stay above the rows as you scroll. Line numbers are the file's, also when a filter
  leaves lines out.
- **Terminal app:** the title names the format (*JSON Lines*, *access log*, *syslog*), the
  encoding and the count. The line under the text says where you are and what is on:
  *Line 120 · 34% · following · Warnings and errors · filter: timeout · traces folded*. The key
  line starts with the log keys.
- The first few times a log is shown, a hint says what the keys do.

## In the terminal app

The same view, in the [built-in viewer](../commands/view-and-edit.md#the-built-in-viewer). Both
apps use one reader in `coxswain-core` (`logs`), so a file's format, levels, folds and filters
are the same in both. **/** searches the rows shown, **g** goes to a line number (or the line
above it a filter keeps). A log set with `viewer` in `config.toml` goes to that program
instead, except *Follow log*, which always uses the built-in viewer.

## Settings and config.toml

None of its own: every switch is per file and lasts while it is open. *Follow log* has no key
by default; give it one under `[keys]` in `config.toml`, or in Settings → *Keys*:

| Key | Type | Default |
|---|---|---|
| `[keys]` `follow_log` | list of keys | `[]`: none, reached from the action menu and **F9** |

## Questions

#### How do I follow a log as it is written?

Press **F3** on it, then **F** in the terminal app's viewer, or **Follow** in the desktop app's
preview. *Follow log* in the action menu (**Shift+F10**) or **F9** opens it already following.
**E** (**Stop at errors**) stops at the next error written.

#### Why do some lines end in ▸ +12 lines?

The lines of a stack trace are folded under the line they belong to. Press **z** in the viewer,
or **Fold traces** in the preview, to show them in full. In the desktop app a click on the
**▸** does it too.

#### Which log formats are read?

JSON Lines, logfmt, Apache and nginx access logs, syslog and W3C (IIS) logs, told from their
first lines. Any other text is a plain log, coloured by the level words in it.

#### How do I see only the lines of one user or one status?

Type `user=ada` or `status=503` in the filter (**&** in the terminal app). In the desktop app,
with **Columns** on, click the value.

#### The log was rotated while I followed it. What happens?

The new file under the same name is read from its start, and following goes on there. The old
lines are gone from the view; open `app.log.1` for them.

#### Why does a big log open unfolded?

Folding means going through every line first. A log over 64 MB opens unfolded, so its end
shows at once. Press **z** to fold it; the title says *so far* until every line is gone
through.

#### Why does a filter show only a part of the file at first?

A filter goes through the file on a thread. The rows it has found show at once, and more come
as it reads. The title says *so far* until it reaches the end.

---
[← Previous: Text and code](text-and-code.md) · [Next: Documents →](documents.md)
