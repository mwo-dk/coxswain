#!/usr/bin/env python3
"""Delete the superseded untagged versions of our own container images on ghcr.io.

Each weekly rebuild of `ghcr.io/mwo-dk/coxswain-*` that changes leaves the build it replaced in
GHCR untagged. This deletes those, keeping:

- every tagged version;
- every digest pinned on this checkout (`master`) and in the last three releases (their tags),
  read through `tools/images-pin.py`, so older installs can still pull what they pin;
- the newest five images;
- everything a kept version holds or that refers to it: an index's per-processor manifests and
  attestations, and signatures or attestations whose `subject` is a kept version.

It lists every version as kept (and why) or to delete before deleting anything. A pin source,
the version list or a manifest it cannot read stops it before any deletion.

    python3 tools/ghcr-cleanup.py                      # every coxswain-* image pinned here: dry run
    python3 tools/ghcr-cleanup.py coxswain-duckdb      # one image: dry run
    python3 tools/ghcr-cleanup.py --delete [package]   # delete (needs gh with packages: write)

`--dry-run` is the default and may be given. Run it from the repository's top folder.
"""

import importlib.util
import json
import os
import re
import subprocess
import sys

REGISTRY = "ghcr.io/mwo-dk/"
NEWEST = 5
RELEASES = 3

_spec = importlib.util.spec_from_file_location("images_pin", os.path.join(os.path.dirname(__file__), "images-pin.py"))
images_pin = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(images_pin)


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True, text=True).stdout


def release_tags():
    """The last RELEASES release tags, fetched (shallow) so their files can be read."""
    tags = re.findall(r"refs/tags/(v\d+\.\d+\.\d+)$", git("ls-remote", "--tags", "--refs", "origin", "v*"), re.M)
    tags = sorted(tags, key=lambda t: tuple(map(int, t[1:].split("."))))[-RELEASES:]
    if len(tags) < RELEASES:
        raise RuntimeError(f"found {len(tags)} release tags, expected at least {RELEASES}")
    for t in tags:
        git("fetch", "--quiet", "--no-tags", "--depth=1", "origin", f"+refs/tags/{t}:refs/tags/{t}")
    return tags


def gh_api(*args):
    return subprocess.run(["gh", "api", *args], check=True, capture_output=True, text=True).stdout


def select(versions, manifests, pinned, newest=NEWEST):
    """Split the versions into kept {id: reason} and a list to delete.

    versions: [{"id", "name" (the digest), "created_at", "tags"}]; manifests: {digest: manifest};
    pinned: a set of digests. A pure function: the tests call it.
    """
    by_digest = {v["name"]: v for v in versions}
    held = {d: [m["digest"] for m in man.get("manifests", [])] for d, man in manifests.items()}
    children = {c for cs in held.values() for c in cs}
    keep = {}
    for v in versions:
        if v["tags"]:
            keep[v["name"]] = "tagged " + ",".join(v["tags"])
        elif v["name"] in pinned:
            keep[v["name"]] = "pinned"
    # Images: not held by an index, and no signature or attestation of another version.
    roots = [v for v in versions if v["name"] not in children and not manifests[v["name"]].get("subject")]
    roots.sort(key=lambda v: v["created_at"], reverse=True)
    for v in roots[:newest]:
        keep.setdefault(v["name"], "newest")
    # What a kept version holds, and what points at one (a signature, an attestation): kept too.
    grew = True
    while grew:
        grew = False
        for d, man in manifests.items():
            if d in keep:
                for c in held.get(d, []):
                    if c not in keep:
                        keep[c], grew = f"held by {d[:19]}", True
            elif (man.get("subject") or {}).get("digest") in keep:
                keep[d], grew = f"refers to {man['subject']['digest'][:19]}", True
    kept = {by_digest[d]["id"]: why for d, why in keep.items() if d in by_digest}
    delete = [v for v in versions if v["id"] not in kept and not v["tags"]]
    return kept, delete


def clean(package, pinned, delete_now):
    image = REGISTRY + package
    path = f"/users/{REGISTRY.split('/')[1]}/packages/container/{package}/versions"
    versions = [json.loads(line) for line in gh_api("--paginate", "--jq", ".[]", path).splitlines()]
    versions = [
        {"id": v["id"], "name": v["name"], "created_at": v["created_at"], "tags": v["metadata"]["container"]["tags"]}
        for v in versions
    ]
    manifests = {v["name"]: images_pin.manifest(image, v["name"]) for v in versions}
    kept, delete = select(versions, manifests, pinned)
    print(f"## {package}: {len(versions)} versions, {len(kept)} kept, {len(delete)} to delete")
    for v in sorted(versions, key=lambda v: v["created_at"], reverse=True):
        what = f"keep ({kept[v['id']]})" if v["id"] in kept else "DELETE"
        print(f"{v['created_at']}  {v['name']}  {what}")
    if delete_now:
        for v in delete:
            try:
                gh_api("-X", "DELETE", f"{path}/{v['id']}")
            except subprocess.CalledProcessError as e:
                if "404" not in e.stderr:  # deleted already, by a run beside this one
                    raise
            print(f"deleted {v['name']}")
    return len(delete)


def main():
    args = sys.argv[1:]
    delete_now = "--delete" in args
    packages = [a for a in args if not a.startswith("--")]
    # Every pin source first: any one unreadable and nothing is deleted.
    sources = ["HEAD", *release_tags()]
    pinned, head = {}, set()
    for ref in sources:
        for name, _tag, d in images_pin.pins(ref):
            if name.startswith(REGISTRY):
                pinned.setdefault(name[len(REGISTRY) :], set()).add(d)
                if ref == "HEAD":
                    head.add(name[len(REGISTRY) :])
    print(f"Pins read from {', '.join(sources)}")
    for p in packages or sorted(head):
        if p not in head:
            raise RuntimeError(f"{p} has no pin on this checkout: refusing to clean it")
    for p in packages or sorted(head):
        clean(p, pinned[p], delete_now)
    return 0


if __name__ == "__main__":
    sys.exit(main())
