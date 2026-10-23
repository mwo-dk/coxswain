#!/usr/bin/env python3
"""Which systems' workflows a push to master runs: those whose pull-request `paths` match a file
the push changed, so master runs what its pull request ran (release.yml's version job).

    .github/changed-systems.py <before> <after>    prints `freebsd=true` … one line each

Every system runs when the files cannot be told (no <before>, a forced push, no PyYAML).
"""
import re
import subprocess
import sys

SYSTEMS = ["freebsd", "netbsd", "openbsd", "illumos", "termux", "flatpak", "nix"]


def glob(pattern: str) -> re.Pattern:
    """GitHub's path pattern: `**` crosses folders, `*` does not."""
    out = re.escape(pattern).replace(r"\*\*/", "(?:.*/)?").replace(r"\*\*", ".*").replace(r"\*", "[^/]*")
    return re.compile(out + r"\Z")


def main() -> None:
    before, after = sys.argv[1:3]
    try:
        import yaml

        files = subprocess.run(["git", "diff", "--name-only", before, after], capture_output=True, text=True, check=True).stdout.split()
    except Exception as e:  # noqa: BLE001  anything unknown: test everything
        print(f"files not known ({e}): every system runs", file=sys.stderr)
        files = None
    for name in SYSTEMS:
        run = True
        if files is not None:
            # A workflow whose `paths` cannot be read runs: a crash here would print no line for
            # it (and the ones after it), and release.yml would skip them.
            try:
                on = yaml.safe_load(open(f".github/workflows/{name}.yml"))
                on = on.get("on", on.get(True))  # PyYAML reads the key `on` as true
                paths = [glob(p) for p in on["pull_request"]["paths"]]
                run = any(p.match(f) for p in paths for f in files)
            except Exception as e:  # noqa: BLE001
                print(f"{name}: paths not known ({e}): it runs", file=sys.stderr)
        print(f"{name}={'true' if run else 'false'}")


if __name__ == "__main__":
    main()
