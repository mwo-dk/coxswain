#!/usr/bin/env python3
"""File "sysutils/coxswain: Update to X.Y.Z" in FreeBSD's Bugzilla, with port.diff attached and
maintainer-approval set. Reads FREEBSD_BUGZILLA_KEY, VERSION and the changelog row from README.md.
Run by release.yml only when the key is set and the port is in the ports tree."""

import base64
import json
import os
import re
import sys
import urllib.request

API = "https://bugs.freebsd.org/bugzilla/rest"
key = os.environ["FREEBSD_BUGZILLA_KEY"]
version = os.environ["VERSION"].lstrip("v")
diff = open(sys.argv[1], "rb").read()


def call(path, body):
    req = urllib.request.Request(f"{API}/{path}", json.dumps(body).encode(), {"Content-Type": "application/json", "X-BUGZILLA-API-KEY": key})
    with urllib.request.urlopen(req, timeout=60) as r:
        return json.load(r)


def changelog(v):
    """The README's changelog row for `v`, as plain text."""
    for line in open("README.md", encoding="utf-8"):
        if line.startswith(f"| **{v}** |"):
            text = line.split("|", 3)[3].rsplit("|", 1)[0].strip().replace("\\|", "|")
            return re.sub(r"\[([^\]]+)\]\(([^)]+)\)", r"\1", text).replace("**", "").replace("*", "")
    return ""


summary = f"sysutils/coxswain: Update to {version}"
notes = changelog(version)
body = (
    f"Update sysutils/coxswain to {version}.\n\n"
    + (f"Changes: {notes}\n\n" if notes else "")
    + f"Release: https://github.com/mwo-dk/coxswain/releases/tag/v{version}\n\n"
    "Built and checked on FreeBSD 14.5 amd64 by the release workflow: make stage check-plist package, portlint -AC.\n"
    "I am the maintainer."
)
bug = call("bug", {"product": "Ports & Packages", "component": "Individual Port(s)", "version": "Latest", "summary": summary, "description": body})["id"]
call(
    f"bug/{bug}/attachment",
    {
        "ids": [bug],
        "data": base64.b64encode(diff).decode(),
        "file_name": f"coxswain-{version}.diff",
        "summary": summary,
        "content_type": "text/plain",
        "is_patch": True,
        "flags": [{"name": "maintainer-approval", "status": "+"}],
    },
)
print(f"Filed https://bugs.freebsd.org/bugzilla/show_bug.cgi?id={bug}")
