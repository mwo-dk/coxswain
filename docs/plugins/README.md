[← README](../../README.md) · [Docs index](../README.md)

# Plugins

> **A design, not yet built.** Nothing on these pages is in either app yet. They record what
> was decided in [#863](https://github.com/mwo-dk/coxswain/issues/863), so that publishers and
> contributors can see where plugins are going before the first line is written. Details may
> change after the spike, [#1024](https://github.com/mwo-dk/coxswain/issues/1024).

A plugin teaches Coxswain a file format it does not know: it reads the file and shows it, in both
apps, and hands Coxswain the text for Find and Ask. A plugin is sealed by default: it sees only the
file it was handed, never a path, a folder or the network.

| Page | What it covers |
|---|---|
| [The plugin design](design.md) | What a plugin is (a sealed WebAssembly component, or a separate program, with one contract), what it may read, its UI in each app, theming, threads, trusted programs and services, installing, file types, search and caches, games moving to bragd's plugin, SDKs and tools, the list of plugins, open points, worked examples for users and a minimal plugin in every planned language |

Related issues: the spike [#1024](https://github.com/mwo-dk/coxswain/issues/1024), game readers
shared with bragd [#862](https://github.com/mwo-dk/coxswain/issues/862) (replaced by the game
plugin, below), and removable disks [#1010](https://github.com/mwo-dk/coxswain/issues/1010).

---
[← Previous: Windows on ARM](../systems/windows-arm.md) · [Next: The plugin design →](design.md)
