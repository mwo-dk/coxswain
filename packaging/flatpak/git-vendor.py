#!/usr/bin/env python3
"""Give the crates from git a vendor folder of their own in cargo-sources.json (argument 1).

flatpak-cargo-generator puts them into the same folder as crates.io's, so tao-macros 0.1.4
from tao's repository and from crates.io (Cargo.lock has both) become one crate to Cargo, and
its checksum check fails."""
import json
import re
import sys

VENDOR, GIT = "cargo/vendor/", "cargo/vendor-git/"


def main(path):
    sources = json.load(open(path))
    # The generator copies each git crate with: cp -r … "flatpak-cargo/git/<repo>/<dir>" "cargo/vendor/<name>"
    git = {m.group(1) for s in sources for c in s.get("commands", []) for m in [re.search(r'"flatpak-cargo/git/[^"]+" "cargo/vendor/([^"]+)"$', c)] if m}
    for s in sources:
        name = s.get("dest", "")[len(VENDOR):]
        if s.get("dest", "").startswith(VENDOR) and name in git:
            s["dest"] = GIT + name
        if "commands" in s:
            s["commands"] = [re.sub(r'"cargo/vendor/([^"]+)"$', lambda m: f'"{GIT}{m.group(1)}"' if m.group(1) in git else m.group(0), c) for c in s["commands"]]
            if any(GIT in c for c in s["commands"]):
                s["commands"].insert(0, f"mkdir -p {GIT}")
        if s.get("dest-filename") == "config":
            # The [source."<git url>"] tables come after crates-io's, which keeps vendored-sources.
            head, sep, rest = s["contents"].partition('[source."')
            s["contents"] = head + sep + rest.replace('replace-with = "vendored-sources"', 'replace-with = "vendored-git"') + f'\n[source.vendored-git]\ndirectory = "{GIT.rstrip("/")}"\n'
    assert git, "no crates from git: is this still needed?"
    json.dump(sources, open(path, "w"), indent=4)


if __name__ == "__main__":
    main(sys.argv[1])
