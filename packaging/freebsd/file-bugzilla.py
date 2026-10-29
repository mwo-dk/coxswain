#!/usr/bin/env python3
"""File "sysutils/coxswain: Update to X.Y.Z" in FreeBSD's Bugzilla, with port.diff attached and
maintainer-approval set. Reads FREEBSD_BUGZILLA_KEY, VERSION and the changelog row from README.md.
Run by release.yml only when the key is set and the port is in the ports tree."""

import base64
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://bugs.freebsd.org/bugzilla/rest"
key = os.environ["FREEBSD_BUGZILLA_KEY"]
version = os.environ["VERSION"].lstrip("v")
diff = open(sys.argv[1], "rb").read()


HEADERS = {
    "Content-Type": "application/json",
    "X-BUGZILLA-API-KEY": key,
    "User-Agent": "coxswain-release (+https://github.com/mwo-dk/coxswain)",
}


def call(path, body=None):
    """One request, tried again on a passing fault (5xx, network): Bugzilla answered 503 to a
    release run once (27 Oct)."""
    data = None if body is None else json.dumps(body).encode()
    for wait in (30, 60, 120, 0):
        try:
            req = urllib.request.Request(f"{API}/{path}", data, HEADERS)
            with urllib.request.urlopen(req, timeout=60) as r:
                return json.load(r)
        except urllib.error.HTTPError as e:
            if e.code < 500 or not wait:
                raise
        except urllib.error.URLError:
            if not wait:
                raise
        print(f"Bugzilla did not answer; trying again in {wait} s", file=sys.stderr)
        time.sleep(wait)


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
# A try that Bugzilla took but answered with an error must not file the update twice.
found = call("bug?" + urllib.parse.urlencode({"product": "Ports & Packages", "summary": summary, "include_fields": "id,summary"}))["bugs"]
found = [b["id"] for b in found if b["summary"] == summary]
if found and call(f"bug/{found[0]}/attachment")["bugs"].get(str(found[0])):
    print(f"Already filed: https://bugs.freebsd.org/bugzilla/show_bug.cgi?id={found[0]}")
    sys.exit(0)
bug = found[0] if found else call("bug", {"product": "Ports & Packages", "component": "Individual Port(s)", "version": "Latest", "summary": summary, "description": body})["id"]
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
