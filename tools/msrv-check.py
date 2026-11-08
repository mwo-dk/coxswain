#!/usr/bin/env python3
"""Fail when a system cannot build the locked dependencies with the Rust it packages.

Pull requests do not run the BSD machines, and those take Rust from their own packages
(OpenBSD has no rustup), which can be older than what a fresh dependency needs. The update
would otherwise show up only in the nightly run or the release. Only what each system builds
is checked, so nothing is held back on the others.
"""
import json
import subprocess
import sys

# The Rust each system's packages give the VM workflows (pkg_add / pkgin rust). Raise a line
# when that system's packages move on; FreeBSD and illumos package the current release.
PACKAGED_RUST = {
    "x86_64-unknown-openbsd": "1.94",  # OpenBSD 7.9
    "x86_64-unknown-netbsd": "1.96",   # NetBSD 10.1
}


def version(v):
    parts = [int(p) for p in v.split(".")[:3]]
    return tuple(parts + [0] * (3 - len(parts)))


def too_new(target, rust):
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked", "--filter-platform", target],
        check=True, capture_output=True, text=True).stdout)
    built = {n["id"] for n in meta["resolve"]["nodes"]}
    return sorted(
        f'{p["name"]} {p["version"]} needs Rust {p["rust_version"]}'
        for p in meta["packages"]
        if p["id"] in built and p.get("rust_version") and version(p["rust_version"]) > version(rust))


def main():
    failed = False
    for target, rust in PACKAGED_RUST.items():
        bad = too_new(target, rust)
        if bad:
            failed = True
            print(f"{target} packages Rust {rust}, which cannot build:")
            print("\n".join(f"  {b}" for b in bad))
        else:
            print(f"{target}: every dependency builds with Rust {rust}.")
    if failed:
        print("Keep the older version (cargo update <name> --precise <version>) until that system"
              " packages a newer Rust, then raise its line in tools/msrv-check.py.")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
