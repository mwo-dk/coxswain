[← README](../../README.md) · [Docs index](../README.md) · [Reference](README.md)

# DragonFly BSD

Coxswain does not yet ship for DragonFly BSD. The code that differs between systems is ready
for it (battery, memory, drives, file watching and the update line are written for DragonFly),
but three things outside Coxswain stop a build, and this page says exactly which, so that you
can build it yourself once they are fixed, or help fix them.

## Contents

- [What stops a build](#what-stops-a-build)
- [What is ready](#what-is-ready)
- [Trying it yourself](#trying-it-yourself)
- [Questions](#questions)

## What stops a build

Checked on DragonFly 6.4.2 (amd64) in a virtual machine, and by building from Linux for
DragonFly with Rust's nightly compiler:

| What | The problem | Where it would be fixed |
|---|---|---|
| Rust | Rust supports DragonFly at tier 3: no `rustup` toolchain. DPorts packages Rust **1.85.1**; Coxswain's dependencies need **1.93** or later (`sevenz-rust2` 1.93; `zip`, `parquet`, `ratatui` 1.88) | A newer `lang/rust` in DPorts |
| `notify` 8.2 (file watching) | Chooses its kqueue backend on DragonFly but does not depend on the `kqueue` crate there, so it does not compile | notify's `Cargo.toml`: add `dragonfly` to the targets that use kqueue |
| `trash` 5.2 (F8) | Uses `libc::MNT_WAIT`, which the `libc` crate does not define for DragonFly | `libc` (DragonFly's `MNT_WAIT` is 1), or `trash` |

Each is small, and none is in Coxswain's own code. A build made on another system for DragonFly
would get past the first, but not the other two.

## What is ready

| Feature | On DragonFly |
|---|---|
| Battery | `sysctl hw.acpi.acline`, as on FreeBSD |
| Memory in the setup guide | `sysconf(_SC_PHYS_PAGES)` |
| Drives in the desktop app's sidebar | getmntinfo(3) |
| File watching | kqueue, folders only, within the process's open-file limit (as on [OpenBSD](openbsd.md#openbsds-limits-and-what-coxswain-does-about-them)) |
| Installing tools | `pkg install …` with FreeBSD's package names (DPorts uses them) |
| The update notice | `fetch -qo - …/install/install-unix.sh \| sh` |
| HAMMER2 snapshots | Not read: Coxswain's snapshots are ZFS's |

The desktop app would need WebKitGTK 4.1: DPorts has `webkit2-gtk_41` 2.46, so it could follow
the terminal app.

## Trying it yourself

When DPorts has Rust 1.93 or later, and with the two crates patched (a `[patch.crates-io]`
section in `Cargo.toml` pointing at fixed copies):

```sh
doas pkg install git rust
git clone https://github.com/mwo-dk/coxswain.git && cd coxswain
cargo build --release --locked -p coxswain
```

Please tell what you find in an [issue](https://github.com/mwo-dk/coxswain/issues): when it
builds, DragonFly gets CI, release builds and the install script like the other BSDs.

## Questions

**Will the FreeBSD build run on DragonFly?** No. DragonFly left FreeBSD's binary interface long
ago; it has no FreeBSD compatibility layer.

**Why not build it on Linux for DragonFly and ship that?** That gets past the old Rust, but not
the two crates above: they do not compile for DragonFly from anywhere.

**Is the rest of Coxswain known to work there?** Not yet: nothing has run on DragonFly. The parts
written for it follow FreeBSD's, which are tested.

---
[← Previous: OpenBSD](openbsd.md) · [Next: illumos →](illumos.md)
