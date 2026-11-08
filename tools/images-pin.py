#!/usr/bin/env python3
"""Bring the container images Coxswain pins up to date.

Every image Coxswain uses is written as `name:tag@sha256:…` (the multi-arch index digest): its
defaults in `crates/coxswain-core/src` (`preview_tools::IMAGES`), and the `FROM` lines of
`packaging/images/*/Dockerfile`.
This asks each image's registry (anonymously, as for any public image) which digest its tag has
now, writes the new ones into those files, and prints a Markdown table of what changed, old to
new. Nothing changed: no output. Run it before a release, check each image's release notes and
the sizes in `preview_tools::IMAGES`, and run the tests on the new pins.

    python3 tools/images-pin.py            # update the files, print the table
    python3 tools/images-pin.py --check    # print only, change nothing
"""

import json
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

HOLDS_PINS = re.compile(r"crates/coxswain-core/src/.+\.rs|packaging/images/[^/]+/Dockerfile")
PINNED = re.compile(r"\b((?:[a-z0-9.-]+\.[a-z]{2,}/)?[a-z0-9._/-]+):([\w.-]+)@(sha256:[0-9a-f]{64})")
ACCEPT = ", ".join(
    [
        "application/vnd.oci.image.index.v1+json",
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.oci.image.manifest.v1+json",
        "application/vnd.docker.distribution.manifest.v2+json",
    ]
)


def split(name):
    """(registry host, repository) of an image name, as `docker pull` reads it."""
    first, _, rest = name.partition("/")
    if rest and ("." in first or ":" in first):
        host, repo = first, rest
    else:
        host, repo = "docker.io", name
    if host == "docker.io":
        host = "registry-1.docker.io"
        if "/" not in repo:
            repo = "library/" + repo
    return host, repo


def request(name, ref, method):
    """The registry's answer for the manifest `name:ref` (a tag or a digest), anonymously."""
    host, repo = split(name)
    url = f"https://{host}/v2/{repo}/manifests/{ref}"
    headers = {"Accept": ACCEPT, "User-Agent": "coxswain-images-pin"}
    for _ in range(2):
        req = urllib.request.Request(url, method=method, headers=headers)
        try:
            with urllib.request.urlopen(req, timeout=60) as r:
                return r.headers, r.read()
        except urllib.error.HTTPError as e:
            challenge = e.headers.get("WWW-Authenticate", "")
            if e.code != 401 or not challenge.startswith("Bearer ") or "Authorization" in headers:
                raise
            # An anonymous pull token, as the registry asks for one.
            params = dict(re.findall(r'(\w+)="([^"]*)"', challenge))
            realm = params.pop("realm")
            with urllib.request.urlopen(f"{realm}?{urllib.parse.urlencode(params)}", timeout=60) as t:
                body = json.load(t)
            headers["Authorization"] = "Bearer " + (body.get("token") or body["access_token"])
    raise RuntimeError(f"{name}:{ref}: no answer")


def digest(name, tag):
    """The digest `name:tag` has now: the index's, for a multi-arch image."""
    return request(name, tag, "HEAD")[0]["Docker-Content-Digest"]


def manifest(name, ref):
    """The manifest (or index) `name:ref` as JSON."""
    return json.loads(request(name, ref, "GET")[1])


def files(ref=None):
    """The files that hold pins: in the working tree, or at a git ref."""
    cmd = ["git", "ls-files"] if ref is None else ["git", "ls-tree", "-r", "--name-only", ref]
    out = subprocess.run(cmd, check=True, capture_output=True, text=True).stdout
    return sorted(p for p in out.splitlines() if HOLDS_PINS.fullmatch(p))


def pins(ref):
    """Every (name, tag, digest) pinned at a git ref; fails when git cannot read it."""
    found = set()
    for f in files(ref):
        text = subprocess.run(["git", "show", f"{ref}:{f}"], check=True, capture_output=True, text=True).stdout
        found |= {m.groups() for m in PINNED.finditer(text)}
    return found


def main():
    check = "--check" in sys.argv
    texts = {f: open(f, encoding="utf-8").read() for f in files()}
    texts = {f: t for f, t in texts.items() if PINNED.search(t)}
    found = sorted({m.groups() for t in texts.values() for m in PINNED.finditer(t)})
    changed, failed = [], []
    for name, tag, old in found:
        try:
            new = digest(name, tag)
        except Exception as e:  # one registry down does not hold up the others
            failed.append(f"{name}:{tag}: {e}")
            continue
        if new and new != old:
            changed.append((name, tag, old, new))
            for f in texts:
                texts[f] = texts[f].replace(f"{name}:{tag}@{old}", f"{name}:{tag}@{new}")
    if not check:
        for f, t in texts.items():
            if t != open(f, encoding="utf-8").read():
                open(f, "w", encoding="utf-8").write(t)
    if changed:
        print("| Image | Old digest | New digest |\n|---|---|---|")
        for name, tag, old, new in changed:
            print(f"| `{name}:{tag}` | `{old[:19]}…` | `{new[:19]}…` |")
    for f in failed:
        print(f"::warning title=Digest not read::{f}", file=sys.stderr)
    # Every image failing is the network, not the images: say so with a failure.
    return 1 if found and len(failed) == len(found) else 0


if __name__ == "__main__":
    sys.exit(main())
