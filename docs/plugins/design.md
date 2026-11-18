[← README](../../README.md) · [Docs index](../README.md) · [Plugins](README.md)

# The plugin design

> **A design, not yet built.** Neither app loads plugins yet. This page records the decisions
> made in [#863](https://github.com/mwo-dk/coxswain/issues/863), for publishers and
> contributors. The spike, [#1024](https://github.com/mwo-dk/coxswain/issues/1024), checks the
> riskiest parts first; what it finds may change the details below. The examples show screens
> and keys as designed, not as they are.

A plugin teaches Coxswain a file format. It reads a file it is handed and returns what Coxswain
shows, searches and asks about. The aim is that Coxswain itself knows nothing about chess or any
other game in the end: plugins bring that knowledge, and Coxswain hosts them.

## Contents

- [Two kinds, one contract](#two-kinds-one-contract)
- [What a plugin may read](#what-a-plugin-may-read)
- [What a plugin shows](#what-a-plugin-shows)
- [Theming](#theming)
- [Heavy work on several threads](#heavy-work-on-several-threads)
- [Trusted programs and services](#trusted-programs-and-services)
- [Installing](#installing)
- [File types and choosing a plugin](#file-types-and-choosing-a-plugin)
- [Search, meaning and caches](#search-meaning-and-caches)
- [Games move to bragd's plugin](#games-move-to-bragds-plugin)
- [SDKs, guides and tools for authors](#sdks-guides-and-tools-for-authors)
- [The list of plugins](#the-list-of-plugins)
- [Open points](#open-points)
- [Examples for users](#examples-for-users)
- [Example for authors: a minimal plugin](#example-for-authors-a-minimal-plugin)

## Two kinds, one contract

| | WebAssembly component | Separate program |
|---|---|---|
| What it is | One `.wasm` file | A program Coxswain starts |
| Runs on | Every system, the same file | The systems its author builds it for |
| Sandbox | Sealed: no files, network or processes except what Coxswain hands it | The OS sandbox where the system has one; or none, when the user trusts it ([below](#trusted-programs-and-services)) |
| Talks over | The `.wit` interface, called directly | The same interface as JSON messages over stdin/stdout, or HTTP or gRPC |
| For | Rust, C/C++, Go (TinyGo), JavaScript/TypeScript, Python, C# | Languages that do not target WebAssembly well (Java, Julia), and existing programs such as UCI chess engines |
| When installed | The manifest is shown | The manifest is shown, with a clear trust warning |

Both kinds have **one contract**: a single `.wit` interface. The SDKs are generated from it, and a
separate program speaks the same calls as JSON messages, so one SDK design serves both kinds.

No plugin is ever loaded into Coxswain's own process: there are no DLLs or shared libraries. A
plugin that crashes ends only itself.

The planned runtime is wasmtime, subject to the spike. It checks that wasmtime builds and runs on FreeBSD, NetBSD,
OpenBSD and illumos with each system's packaged Rust; where it does not, an interpreter such as
`wasmi` runs plugins on that system instead.

## What a plugin may read

- **The file it was handed**, read-only.
- **The companion files its manifest names**, by pattern. A ChessBase `.cbh` comes with its
  `.cbg`, `.cba` and the rest; a SCID `.si4` with its `.sg4` and `.sn4`.

Coxswain opens these files and hands them in. A plugin never sees a path, a folder or anything
else on the disk, and gets no network. The one exception is a trusted program, which the user
has chosen to give full rights ([below](#trusted-programs-and-services)).

## What a plugin shows

A plugin brings its own UI; Coxswain only hosts it. (This replaces an earlier decision that
plugins return typed content only and draw nothing.)

| Where | What the plugin gives | What Coxswain does |
|---|---|---|
| Desktop app | A web bundle: HTML, JavaScript, CSS | Runs it in a sealed, cross-origin frame with a strict content security policy: no network, no Tauri API, no access to Coxswain's page or to files. The frame's only channel is messages to and from its own plugin, through Coxswain |
| Terminal app | Characters and colours drawn into a grid Coxswain hands it | Gives it the keys while it has focus |
| Both, always | Plain text and a title | Uses them for Find and Ask, so search works without the UI |
| Both, as a fallback | An interactive picture: PNG, or SVG with scripts and links stripped, plus named regions with their tooltips | Handles the mouse and keys, sends the plugin events (clicked region X, back, zoom) and shows the next picture it returns. Hover and tooltips need no round trip |

The **interactive picture** is for plugins that ship no web bundle, and for drawings no other kind
fits, such as a company format drawn as a sunburst. In the terminal app it is shown where the
terminal can show pictures (kitty or sixel); elsewhere the terminal app shows a text or tree
version the plugin also returns.

## Theming

- **Desktop app:** Coxswain puts the theme's CSS variables and fonts into the plugin's frame, so a
  plugin follows the theme the user picked, and changes with it.
- **Terminal app:** the plugin draws with named colours (such as text, background, selection),
  which Coxswain maps to the theme.

The variable names and the colour names become a **versioned public contract**: a plugin written
against one version keeps working when Coxswain's own themes change.

## Heavy work on several threads

A large file (a SCID base of millions of games, a PGN of several gigabytes) is read on several
threads. A plugin may export three functions:

| Function | Takes | Returns |
|---|---|---|
| *split* | The file | Chunks: byte ranges, ranges of games |
| *work* | One chunk | A partial result |
| *merge* | Partial results | One result |

Coxswain compiles the plugin once and runs one sealed instance per thread, each on its own
chunks, then merges, the way the chess position index merges its sorted runs today
([#938](https://github.com/mwo-dk/coxswain/issues/938)). Each instance
has its own memory, so the sandbox holds across threads. The spike measures this against today's
built-in readers on a large SCID or PGN file.

## Trusted programs and services

### Trusted programs

A separate program may run **unsandboxed**: at full native speed, with threads and the user's
files. It runs that way only when the user trusts it explicitly:

- per plugin, never for all;
- after a plain warning that the program can do anything the user can;
- Settings → Plugins marks it as **unsandboxed** from then on.

It still runs as a process of its own, never inside Coxswain.

### Services over HTTP or gRPC

A plugin can also be a service. The messages are the same over stdin/stdout, HTTP and gRPC:
HTTP with JSON first, gRPC generated from the same schema.

| | Local service (`localhost`) | Remote service |
|---|---|---|
| Counts as | A trusted program | File contents leaving the machine |
| Turning it on | As a trusted program | Opt-in, per plugin |
| Settings shows | That it is trusted | The host the files go to |
| Connection | Coxswain connects out | Coxswain connects out, over TLS only |
| First use | | Asks once, for each plugin |

Coxswain only ever connects out. It never opens a listening port.

## Installing

Version one installs plugins **by hand only**: Settings → Plugins → *Install from file*. Before
installing, Coxswain shows the plugin's manifest:

| Shown | For example |
|---|---|
| Name, publisher, version | bragd games, bragd |
| The formats it reads | `.pgn`, `.si4`, `.sgf`, … |
| Its kind | Sealed WebAssembly, or a separate program with a trust warning |
| The companion files it may open | `.sg4`, `.sn4` beside a `.si4` |

There is no online catalogue in version one, because nothing leaves the machine unless the user
asks. A catalogue and signed packages may come later, opt-in.

## File types and choosing a plugin

- A plugin declares what it reads **by extension and by the first bytes** of the file.
- A plugin may come **in parts**. Bragd's game plugin has a part each for chess, Go, shogi and the
  others.
- Settings → Plugins → *File types* lists every type and the plugin that reads it. When several
  plugins claim one type, the user picks. Any part of a plugin can be turned off.
- *Open with…* on a single file overrides the choice once.

## Search, meaning and caches

Search, meaning and caches stay Coxswain's.

- **Text:** a plugin returns text passages. Coxswain indexes and embeds them in its own store, as
  it does for its built-in readers.
- **Content version:** each plugin is a reader with its own content version, as `readers` in the
  store works today. Only a raised content version reads that plugin's formats again. A bug-fix
  release of a plugin that keeps its content version redoes no vectors.
- **A private cache:** each plugin gets a cache store of its own, as a handle, never a path, for
  things like analysed games or a position index. It has a size cap, shows in
  [Disk use](../reference/disk-use.md), and is deleted when the plugin is uninstalled.

## Games move to bragd's plugin

bragd, the owner's board-game app, builds one sealed game plugin. It reads chess, Go, shogi,
xiangqi, draughts, bridge, shatranj, senet and Ur. Coxswain ships it preinstalled and updates it
with each release.

Once the plugin matches today's game features (for example [chess](../previews/chess.md),
[Go](../previews/go.md), [shogi](../previews/shogi.md), [xiangqi](../previews/xiangqi.md),
[draughts](../previews/draughts.md), [bridge](../previews/bridge.md)), Coxswain deletes its own
game code: `chess/`, `go.rs`, `xiangqi.rs`, `shogi.rs`, `draughts.rs`, `bridge.rs` and their Svelte
views. A game returns a position for every move, so no game rules live in Coxswain.

This replaces the shared crate planned in [#862](https://github.com/mwo-dk/coxswain/issues/862).
Other publishers may publish game plugins too.

## SDKs, guides and tools for authors

| What | Plan |
|---|---|
| SDKs | Rust, C/C++, Go, JavaScript/TypeScript, Python and C# for WebAssembly components; Java for separate programs. All generated from the one `.wit` |
| Guides in `docs/plugins/` | A first plugin in 15 minutes for each language; the contract reference, generated from the `.wit`; the security model; theming; the UI frame and the terminal grid; threads; services; publishing |
| Samples | A sample plugin per language |
| `coxswain plugin test <plugin> <files>` | Runs a plugin without the UI and checks its output against the contract |
| Conformance suite | Run by `coxswain plugin test`, and in an author's own CI: contract tests, malformed and fuzzed inputs, limits on time and memory, theming checks, and the UI frame's content security policy |
| IDE extension, VS Code first | A new plugin from a template, `.wit` support, running the test suite, and a live preview of a file through the plugin being written |

The IDE extension and the conformance suite are tools for authors to use themselves. Coxswain
does not run them on anyone's behalf.

## The list of plugins

The list will be a page in this area: one line per plugin, saying what it reads, with a link to
the plugin's own page (on GitHub or any website). Publishers add their line by pull request.
Review checks the line and the link only.

Coxswain builds, tests, hosts and vouches for nothing on the list, and the apps never fetch it.

| Plugin | What it reads | Its page |
|---|---|---|
| *None yet* | | |

## Open points

- **Engine analysis.** Today's chess analysis runs an engine the user chose. In a sealed plugin
  that needs either a narrow permission, "may run the engine the user chose", or a trusted-program
  part of the plugin. Not decided.
- **The spike's checks** ([#1024](https://github.com/mwo-dk/coxswain/issues/1024)):
  - wasmtime builds and runs on FreeBSD, NetBSD, OpenBSD and illumos with each system's packaged
    Rust; if not, an interpreter such as `wasmi` covers those systems;
  - a plugin's speed, with split, work and merge on several threads, against today's built-in
    readers on a large SCID or PGN file.

Related: removable disks, [#1010](https://github.com/mwo-dk/coxswain/issues/1010).

## Examples for users

> **As designed, not yet built.** The screens and keys below show how the design is meant to
> work. Names of buttons and settings may change before it ships.

Settings opens with **Ctrl+,** or **F12** in the desktop app, and **F12** in the terminal app.

### Installing bragd's game plugin and opening a PGN file

Bragd's plugin comes preinstalled. These steps show installing it, or a newer copy, by hand.

1. Open Settings and go to **Plugins**. Press **Install from file** and pick
   `bragd-games.wasm`.
2. A dialog shows the manifest before anything is installed:
   - **Name:** bragd games. **Publisher:** bragd. **Version:** its version.
   - **Kind:** sealed WebAssembly: no files, network or programs beyond the file it is handed.
   - **Reads:** `.pgn`, `.cbh`, `.si4`, `.sgf` and the other game formats Coxswain reads today,
     part by part (chess, Go, shogi, xiangqi, draughts, bridge, shatranj, senet, Ur).
   - **May also open:** the companion files of a base, such as `.cbg` and `.cba` beside a
     `.cbh`, and `.sg4` and `.sn4` beside a `.si4`.
3. Press **Install**. The plugin appears in the list, marked *sealed*.
4. In a panel, move the cursor to `games.pgn`. The preview pane shows the plugin's own view in its
   frame: the board, in the theme's colours, and the list of games.
5. In the terminal app the same file shows the board drawn in the character grid; it takes the
   keys while it has focus.
6. Find with the words of a player's name finds the file: the plugin returned the games' text.

### Bragd for chess and Go, another publisher for shogi

1. Install the other publisher's shogi plugin from file, as above. Its manifest says it reads
   `.kif` and `.kifu`.
2. Open Settings → Plugins → **File types**. `.kif` now has two plugins that claim it: bragd games
   (shogi part) and the new one.
3. Pick the new plugin for `.kif` and `.kifu`.
4. Optionally turn off bragd's shogi part altogether. Chess and Go stay with bragd.
5. A `.kif` file now opens in the new plugin; a `.pgn` or `.sgf` file still opens in bragd's.

### A company format drawn as a sunburst

A company's in-house format has a plugin that ships no web bundle. It returns an interactive
picture: a sunburst, each ring a level, each segment a named region with a tooltip.

1. Move the cursor to the file. The preview shows the sunburst.
2. Hover over a segment. Its tooltip shows at once: Coxswain has the regions, so this needs no
   call to the plugin.
3. Click a ring segment. Coxswain sends the plugin "clicked region X"; the plugin returns the
   next picture, with that segment at the centre.
4. Go back to the level above with the back event (the keys for back and zoom are not settled).
5. In a terminal that shows pictures (kitty or sixel), the terminal app shows the same picture.
   In another terminal, it shows the tree the plugin returns as well.

### A trusted program for your own Java tool

1. Settings → Plugins → **Install from file**, and pick the tool's manifest.
2. The manifest says **Kind: separate program**. Coxswain shows a plain warning: *this program
   runs with your rights: it can read and change your files and use the network*.
3. You choose to trust it, for this plugin only.
4. Settings → Plugins lists it marked **unsandboxed**. You can take the trust back there.
5. If the tool crashes, its preview says so; Coxswain keeps running.

### A company service plugin on a server

1. Install the service plugin's manifest from file. It names a remote host.
2. The service is **off** until you turn it on: remote services are opt-in, per plugin.
3. Turn it on. Settings → Plugins names the host your files' contents go to, and requires TLS.
4. The first time you open a file it reads, Coxswain asks once: *send this file's contents to
   the host?* You answer once, for this plugin.
5. Coxswain only connects out to the host. It opens no port on your machine.

### Open with… for one file

1. Move the cursor to a `.kif` file normally read by your chosen shogi plugin.
2. Open **Open with…** (for example from the action menu, **Shift+F10**) and pick bragd games.
3. This file opens in bragd's shogi part this once. The choice in Settings → Plugins → *File
   types* is unchanged.

### A plugin update that raises its content version

1. A new version of a plugin is installed from file. Its manifest raises its content version,
   because it now returns better text (say, the comments inside games).
2. Coxswain reads that plugin's formats again, and only those: their text is indexed again and,
   with search by meaning on, embedded again.
3. Files of other formats, and of other plugins, are not read again.
4. A later bug-fix version that keeps the same content version reads nothing again and redoes no
   vectors.
5. The plugin's private cache is its own to keep or rebuild; it shows in Settings → Disk use.

## Example for authors: a minimal plugin

> **Sketches of the planned SDKs.** None of these SDKs exist yet. The calls, names and the
> manifest's fields are placeholders, and will change after the spike
> ([#1024](https://github.com/mwo-dk/coxswain/issues/1024)). The per-language guides, a first
> plugin in 15 minutes each, will come with the SDKs.

Each sketch is the same plugin: a reader for `.note` files. It returns the first line as the
title and the whole file as text. Every language has the same shape: read the file's bytes,
return a title and a text.

### The manifest

```toml
# coxswain-plugin.toml
id = "example.notes"
name = "Notes"
publisher = "Example"
version = "0.1.0"
kind = "wasm"            # "program" for a separate program
content-version = 1      # raise it when the returned text changes

[[reads]]
extensions = ["note"]
first-bytes = "NOTE"     # optional
companions = []          # patterns of files opened alongside, such as "*.sg4"
```

### Testing it

```
$ coxswain plugin test notes.wasm samples/*.note
notes.wasm: contract ok
samples/shopping.note: title "Shopping", 112 bytes of text
samples/empty.note: title "", 0 bytes of text
conformance: malformed inputs ok, time and memory within limits
```

### Rust

```rust
use coxswain_plugin::{export, Content, File, Reader};

struct Notes;

impl Reader for Notes {
    fn read(file: File) -> Result<Content, String> {
        let text = String::from_utf8_lossy(&file.bytes()).into_owned();
        let title = text.lines().next().unwrap_or("").to_string();
        Ok(Content { title, text })
    }
}

export!(Notes);
```

### C

```c
#include <coxswain_plugin.h>
#include <string.h>

bool reader_read(cx_file_t file, cx_content_t *out, cx_string_t *err) {
    cx_bytes_t bytes = cx_file_bytes(file);
    const char *end = memchr(bytes.ptr, '\n', bytes.len);
    size_t first = end ? (size_t)(end - (const char *)bytes.ptr) : bytes.len;
    cx_string_set(&out->title, (const char *)bytes.ptr, first);
    cx_string_set(&out->text, (const char *)bytes.ptr, bytes.len);
    return true;
}
```

### Go (TinyGo)

```go
package main

import (
	"strings"

	cx "github.com/example/coxswain-plugin-go"
)

func init() {
	cx.Reader(func(file cx.File) (cx.Content, error) {
		text := string(file.Bytes())
		title, _, _ := strings.Cut(text, "\n")
		return cx.Content{Title: title, Text: text}, nil
	})
}

func main() {}
```

### TypeScript

```ts
import { File, Content } from "coxswain:plugin/reader";

export function read(file: File): Content {
  const text = new TextDecoder().decode(file.bytes());
  const title = text.split("\n", 1)[0];
  return { title, text };
}
```

### Python

```python
from coxswain_plugin import File, Content, reader

@reader
def read(file: File) -> Content:
    text = file.bytes().decode("utf-8", errors="replace")
    title = text.split("\n", 1)[0]
    return Content(title=title, text=text)
```

### C#

```csharp
using Coxswain.Plugin;

public class Notes : IReader
{
    public Content Read(File file)
    {
        var text = System.Text.Encoding.UTF8.GetString(file.Bytes());
        var title = text.Split('\n', 2)[0];
        return new Content(title, text);
    }
}
```

### Java (a separate program)

Java runs as a separate program: `kind = "program"` in the manifest, with the command that starts
it. The SDK reads the JSON messages on stdin and writes the answers on stdout; the plugin sees only
the same calls.

```java
import coxswain.plugin.*;
import java.nio.charset.StandardCharsets;

public class Notes implements Reader {
    public Content read(PluginFile file) {
        String text = new String(file.bytes(), StandardCharsets.UTF_8);
        String title = text.split("\n", 2)[0];
        return new Content(title, text);
    }

    public static void main(String[] args) {
        Program.serve(new Notes()); // JSON messages over stdin and stdout
    }
}
```

`coxswain plugin test` runs a program plugin the same way: `coxswain plugin test
coxswain-plugin.toml samples/*.note`.

---
[← Previous: Plugins](README.md) · [Next: Questions, collected →](../faq.md)
