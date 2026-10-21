[← README](../../README.md) · [Docs index](../README.md)

# Systems

Coxswain runs on FreeBSD, the other BSDs, illumos, Linux, Android, ChromeOS, macOS and Windows.
Most of it is the same everywhere; these pages say what is particular to one system: how to
install it there, what it needs, how the search helper starts without the usual session
service, and what a system has that others do not (ZFS snapshots, packages, file flags, boot
environments and jails on FreeBSD; folder prompts on a Mac). Installing in one line on each
system: [the README](../../README.md#install).

| Page | What it covers |
|---|---|
| [FreeBSD](freebsd.md) | Both apps on FreeBSD: the one-line install, the packages and what each is for, the search helper from the session, rc.d or the login shell, search by meaning, how FreeBSD differs, updating, the desktop app's experimental status |
| [NetBSD](netbsd.md) | Both apps on NetBSD (the desktop app experimental): the one-line install, the packages from pkgin, the search helper from rc.d, how NetBSD differs |
| [OpenBSD](openbsd.md) | Both apps on OpenBSD (the desktop app experimental): the one-line install, the packages, the search helper with rcctl, OpenBSD's limits and what Coxswain does about them |
| DragonFly BSD | Not supported; its Rust (1.85 in DPorts) is too old for Coxswain's dependencies |
| [illumos](illumos.md) | The terminal app on OmniOS and OpenIndiana: the one-line install, the search helper as an SMF service, ZFS snapshots and datasets, how illumos differs |
| [TrueNAS](truenas.md) | The terminal app on TrueNAS over SSH: which build, installing into your home, snapshots, leaving the system's datasets alone, indexing the shares |
| [Linux on ARM](linux-arm.md) | Both apps on 64-bit ARM Linux: which file for a Raspberry Pi, Asahi or an ARM server, what they need (glibc 2.35, WebKitGTK 4.1), installing, search by meaning on ARM |
| [Flatpak](flatpak.md) | The desktop app as a Flatpak (not on Flathub yet: built from the manifest): what the sandbox lets it do and why, your editor and commands on the host, the programs previews use, the search helper, where its files are kept, updating, building it yourself |
| [Nix](nix.md) | The flake: `nix run github:mwo-dk/coxswain`, `nix profile install`, NixOS and Home Manager, the desktop app on Linux, updating, the package prepared for nixpkgs |
| [Termux on Android](termux.md) | The terminal app on a phone or tablet: installing with `pkg` or `cargo`, the phone's folders (`termux-setup-storage`, Alt+F1), opening with termux-open, clipboard and battery through Termux:API, the helper without a session service, a trash of its own, F-keys on Termux's extra keys row, updating, the package recipe and how CI builds and starts it |
| [ChromeOS](chromeos.md) | Both apps in ChromeOS's Linux: which `.deb`, installing it, sharing ChromeOS folders with Linux, the helper, no GPU for meaning search, what the builds need |
| [macOS](macos.md) | The folder prompts macOS shows and what to answer, what Coxswain never reads on a Mac (`~/Library`, Photos and Music libraries), Full Disk Access (not needed), why a new version may ask again, search by meaning on the GPU with no server |
| [Windows on ARM](windows-arm.md) | Native ARM64 builds of both apps for Snapdragon laptops and ARM virtual machines: which file, installing, WinGet, changing from the x64 build, search by meaning |

---
[← Previous: Performance](../reference/performance.md) · [Next: FreeBSD →](freebsd.md)
