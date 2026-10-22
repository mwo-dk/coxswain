[← README](../../README.md) · [Docs index](../README.md) · [Search](README.md)

# Long files: sample or everything

A long log, a book or a data dump is more than search keeps of most files. By default it keeps a
**sample**: the first 4 MB of a file's text for [search by words](text.md), and 256 passages
spread from the file's start to its end for [search by meaning](meaning.md). That is plenty for
almost every file, and it keeps `search.db` small. **Everything** keeps all of a long file's text
and gives every part of it a passage with a vector of its own, so a line deep inside a 300 MB log
is found by its words and by what it is about. Choose it for every folder in Settings, or for one
folder from the action menu. Before a choice is saved, Coxswain says what it costs here: how many
long files it touches, about how much more room they take, and about how long reading them and
making their vectors takes on this machine.

<!-- screenshot: long-files-menu.png: the desktop app's action menu on a folder of logs, Long files: sample or everything… open with Everything here: Long files kept whole: 3; about 180 MB more on the disk; about 25 minutes …, Sample here and Like the folders above (Sample) · now -->

## Contents

- [How to use it](#how-to-use-it)
- [One folder](#one-folder)
- [What each keeps](#what-each-keeps)
- [What it costs](#what-it-costs)
- [Back to Sample](#back-to-sample)
- [What is read again](#what-is-read-again)
- [Settings and config.toml](#settings-and-configtoml)
- [In the terminal app](#in-the-terminal-app)
- [Questions](#questions)

## How to use it

For every folder: *Settings → Finding files → Details → Meaning → Long files*, *Sample* or
*Everything*. In the terminal app: **F12** → *Finding files*, the *Long files* row, **Enter**,
the choice, **Enter**. The cost shows before anything is written: under the option in the desktop
app, with **Apply** and **Cancel**; on the line at the bottom of Settings in the terminal app,
with **Enter** to apply and **Esc** to cancel.

For one folder, see [below](#one-folder): the action menu on it.

It needs *Words inside files* (`text`, on by default). Without [search by meaning](meaning.md) it
keeps all the text for words and makes no vectors. With *Words inside files* off, a notice says
once that nothing is kept of any file until it is on.

## One folder

Put the cursor on a folder and open the action menu (**Shift+F10**, or the Menu key; in the
desktop app also a right-click when *Right-click* is set to the menu). Under *Search*: *Long
files: sample or everything…*. It is in the **F9** command list under *Search* too. It has no key
of its own. A menu opens with three choices, each with what it would cost:

| Choice | Does |
|---|---|
| **1** *Everything here* | The folder's long files, and those of the folders inside it, are kept whole, whatever *Long files* says |
| **2** *Sample here* | They are sampled, whatever *Long files* says |
| **3** *Like the folders above (Sample)* | The folder follows the nearest folder above it that has a choice of its own, else *Long files* |

The one in force ends with *· now*. Choosing one writes `long_files_everything` or
`long_files_sample` in `config.toml`; the status line says *Long files in logs: saved; the search
helper takes it up.* and the [search helper](helper.md) restarts with it. The deepest folder
listed decides: *Everything* for `~/logs` and *Sample* for `~/logs/old` keeps `~/logs/today.log`
whole and samples `~/logs/old/2019.log`. A folder in both lists is sampled.

Settings shows the folders chosen this way under *Folders kept whole* and *Folders sampled*, each
with **Remove**; removing one says its cost first too.

A hint on the status line names the action menu a few times when the cursor is on a folder.

## What each keeps

| | Sample (the default) | Everything |
|---|---|---|
| Text for words, of text, code, CSV and logs | The first 4 MB | All of it, up to 256 MB a file |
| Text for words, of PDF, Word, slides and other documents | The first 4 MB | The first 4 MB: their readers stop there |
| Passages for meaning | 256 at most: the first 16, the last 4, the first under each heading, the rest spread evenly over the whole file | One for every part of the file, about every 100 words |
| A short file (under 4 MB of text and 256 passages) | All of it | The same: nothing differs |

A text, code, CSV or log file kept whole is read line by line, never held whole, and its passages
go to the model 64 at a time. Its passages are kept in `search.db` with their vectors, with where
each section starts.

## What it costs

Room on the disk and time, for the long files only. The cost is worked out from what the store
knows about each file, before anything is read:

- **Long files kept whole**: the files in the folders the choice reaches with more than 4 MB of
  text or more than 256 passages.
- **More on the disk**: their extra text twice over (the text and its index), and each extra
  passage's vector and text, about 1 KB to 2 KB a passage.
- **Time**: the extra passages at the speed the helper measured making vectors here (kept from
  one run to the next), or the speed test of [*Measure speed*](meaning.md#how-fast-is-it-here-and-would-another-model-be-faster)
  when it has measured nothing yet, plus reading the files again.

A line such as *Long files kept whole: 3; about 180 MB more on the disk; about 25 minutes to read
them and make their vectors.* When the choice touches no long file now: *No long file changes
now; files read later follow the choice.* When the helper does not answer: *What this costs
cannot be said now …*.

## Back to Sample

*Sample* is a part of *Everything*: the passages Sample keeps are among those Everything made. So
going back reads nothing again and makes no vector again. For each file the helper keeps the
passages Sample would choose and drops the rest with their vectors, cuts the text back to its
first 4 MB as reading it again would, and then gives the room back to the disk (`search.db`
shrinks; a store from an older version is compacted whole once, after which it frees room bit by
bit). The choice says first what it frees: *Long files sampled again: 2; frees about 1.2 GB;
nothing is read again.*

## What is read again

A change of the choice is work your machine does once, for the long files it reaches:

| Change | Files | What is redone |
|---|---|---|
| To *Everything* | Text, code, CSV and logs whose text was cut at 4 MB | Read again, and all their passages get vectors |
| To *Everything* | Other files with more than 256 passages | Their vectors made again from the text kept, every passage |
| To *Sample* | Files kept whole | Nothing read or made: passages and text pruned, the store compacted |

Short files are never redone. An older version of Coxswain leaves what *Everything* made in
place: it reads and searches it, and only cuts a file back when the file itself changes.

## Settings and config.toml

| Settings item | Key under `[search]` | Type | Default |
|---|---|---|---|
| *Long files* | `long_files` | `"sample"` or `"everything"` | `"sample"` |
| *Folders kept whole* | `long_files_everything` | list of paths | `[]` |
| *Folders sampled* | `long_files_sample` | list of paths | `[]` |

```toml
[search]
long_files = "sample"
long_files_everything = ["/home/me/logs"]
long_files_sample = ["/home/me/logs/old"]
```

`--settings=long_files` opens Settings at the option.

## In the terminal app

The same choices and the same costs. The action menu (**Shift+F10**) and **F9** list *Long files:
sample or everything…*; its menu takes **1**, **2** and **3**, or **Up**/**Down** and **Enter**.
In Settings (**F12** → *Finding files*) the cost is the red line at the bottom, *Enter: Apply ·
Esc: Cancel*.

## Questions

#### Why does search by meaning miss something deep inside a long log?

With *Sample* a long file gets 256 passages spread from its start to its end, so a part between
them has no vector of its own. Choose *Everything* for its folder: the action menu on the folder
(**Shift+F10**) → *Long files: sample or everything…* → *Everything here*.

#### Why are the last lines of a big log not found by their words?

*Sample* keeps the first 4 MB of a file's text for words. *Everything* keeps all of it, up to
256 MB a file, for text, code, CSV and logs.

#### What does Everything cost?

Room on the disk and time. Before the choice is saved it says how many long files it touches,
about how much more room they take and how long reading them and making their vectors takes on
this machine ([What it costs](#what-it-costs)).

#### Does going back to Sample read the files again?

No. Sample is a part of Everything: the passages Sample keeps stay, the rest go with their
vectors, the text is cut back to its first 4 MB and the store gives the room back to the disk.

#### Does Everything keep all the text of a big PDF?

No. PDF, Word, slides and the other documents have readers that stop at 4 MB of text; Everything
gives every part of that text a passage. Text, code, CSV and logs keep all of theirs, up to
256 MB a file.

#### Why is the time only a guess?

It is the passages still to make times the time a passage took here, measured by the helper (or
by the speed test until it has measured anything), plus reading. A busy machine, a server that
is faster on long batches, or files that changed meanwhile make it differ.

#### Can I use Everything for my whole home folder?

Yes, *Long files → Everything* in Settings. Only long files cost more; the warning says how
many there are and what they take before you apply it. Folders you want sampled can still be
set to *Sample here*.

---
[← Previous: Search by meaning](meaning.md) · [Next: Search by meaning on a server →](servers.md)
