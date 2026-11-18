#!/usr/bin/env python3
"""The release pull request's changes, made in the checkout it runs in (CLAUDE.md, Shipping).

    packaging/release-pr.py --next    print the next version and change nothing
    --unreleased-ok                   (with either) go on although the current version has no
                                      published GitHub release (the owner's decision only)
    --open-issues-ok                  (with either) go on although the new version's milestone
                                      has open issues (the owner's decision only)
    packaging/release-pr.py           make the release: print the version, then
        bump [workspace.package] and coxswain-core's `=X.Y.Z` in Cargo.toml, Cargo.lock with them
        (cargo update --workspace), every changelog.d/ row moved into one row for the version in
        docs/changelog.md and README.md (which keeps ten), and the changelog.d/ files deleted.

A minor release when any row ends with `<!-- feature -->`, a patch release otherwise; a major one
is made by hand. Exits 2, changing nothing, when changelog.d/ has no rows. Exits 3, changing
nothing, when the current version has no published GitHub release (`gh release view`): bumping
past it would leave it unpublished for good (2.33.0 was), so its release run is finished first.
Exits 4, changing nothing, while the new version's GitHub milestone (named X.Y.Z) has open issues:
a release is done when its milestone is empty. Issues labelled `post-release` or titled "After the
release…" (verifying the channels) are left open on purpose and do not count. When the new version
has no milestone of its own, every open milestone above the current version up to the next minor
holds it instead, so a week of fixes only does not skip the planned minor's open issues (a patch
release between Sundays gets a milestone of its own first).
Run from the repository's root.
"""
import datetime
import json
import re
import subprocess
import sys
from pathlib import Path

FEATURE = "<!-- feature -->"
LINK = re.compile(r"\]\(([^)]+)\)")


def published(tag: str) -> bool:
    """Whether `tag` has a published (not draft) GitHub release; exits on any other gh fault."""
    try:
        r = subprocess.run(["gh", "release", "view", tag, "--json", "isDraft", "-q", ".isDraft"],
                           capture_output=True, text=True)
    except FileNotFoundError:
        sys.exit("release-pr.py: gh is needed to see whether the current version is released "
                 "(or pass --unreleased-ok)")
    if r.returncode == 0:
        return r.stdout.strip() == "false"
    if "release not found" in r.stderr:
        return False
    sys.exit(f"release-pr.py: gh release view {tag} failed: {r.stderr.strip()}")


def gh_json(path: str) -> list:
    """`gh api --paginate` of a list endpoint; exits on any gh fault."""
    try:
        r = subprocess.run(["gh", "api", "--paginate", path], capture_output=True, text=True)
    except FileNotFoundError:
        sys.exit("release-pr.py: gh is needed to read the milestone (or pass --open-issues-ok)")
    if r.returncode != 0:
        sys.exit(f"release-pr.py: gh api {path} failed: {r.stderr.strip()}")
    return pages(r.stdout)


def pages(text: str) -> list:
    """`gh api --paginate` prints one JSON array per page, back to back: their items, in order."""
    dec, at, items = json.JSONDecoder(), 0, []
    while (at := len(text) - len(text[at:].lstrip())) < len(text):
        page, at = dec.raw_decode(text, at)
        items += page
    return items


def version(title: str) -> tuple | None:
    m = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", title.strip())
    return tuple(map(int, m.groups())) if m else None


def post_release(issue: dict) -> bool:
    """Left open on purpose until the release is out: checking every channel."""
    return (any(l["name"] == "post-release" for l in issue.get("labels", []))
            or issue["title"].lower().startswith("after the release"))


def holding(stones: list[dict], new: str, was: str) -> list[dict]:
    """The milestones that hold release `new` over `was`: its own, or when it has none, every open
    one above `was` up to the next minor (a patch must not skip the planned minor's issues)."""
    own = [m for m in stones if m["title"] == new]
    if own:
        return own[:1]
    lo = version(was)
    hi = (lo[0], lo[1] + 1, 0)
    return [m for m in stones if m["state"] == "open" and (v := version(m["title"])) and lo < v <= hi]


def open_issues(new: str, was: str) -> list[dict]:
    """The open issues (not pull requests) that hold release `new`, each with its milestone."""
    held = []
    for m in holding(gh_json("repos/{owner}/{repo}/milestones?state=all&per_page=100"), new, was):
        issues = gh_json(f"repos/{{owner}}/{{repo}}/issues?milestone={m['number']}&state=open&per_page=100")
        held += [dict(i, stone=m["title"]) for i in issues if "pull_request" not in i and not post_release(i)]
    return held


def main() -> int:
    args = sys.argv[1:]
    unreleased_ok = "--unreleased-ok" in args
    open_ok = "--open-issues-ok" in args
    args = [a for a in args if a not in ("--unreleased-ok", "--open-issues-ok")]
    frags = sorted(Path("changelog.d").glob("*.md"))
    if not frags:
        print("changelog.d/ has no rows: nothing to release.", file=sys.stderr)
        return 2
    rows = [f.read_text(encoding="utf-8").strip() for f in frags]
    features = [r for r in rows if r.endswith(FEATURE)]
    cargo = Path("Cargo.toml").read_text(encoding="utf-8")
    old = re.search(r'^\[workspace\.package\]\n(?:.*\n)*?version = "(\d+)\.(\d+)\.(\d+)"', cargo, re.M)
    major, minor, patch = map(int, old.groups())
    was = ".".join(old.groups())
    if not unreleased_ok and not published(f"v{was}"):
        print(f"release-pr.py: v{was}, the version in Cargo.toml, has no published GitHub release. "
              f"A new release would skip it for good. Finish its release first: fix what failed, "
              f"then start Release by hand (gh workflow run release.yml). To skip v{was} on "
              f"purpose, pass --unreleased-ok.", file=sys.stderr)
        return 3
    new = f"{major}.{minor + 1}.0" if features else f"{major}.{minor}.{patch + 1}"
    if not open_ok and (held := open_issues(new, was)):
        names = ", ".join(sorted({i["stone"] for i in held}))
        print(f"release-pr.py: {new} is held by {len(held)} open issue(s) in milestone {names}; a "
              f"release is done when its milestone is empty. Close them, or move them to a later "
              f"milestone:", file=sys.stderr)
        for i in held:
            print(f"  #{i['number']} {i['title']} ({i['stone']})", file=sys.stderr)
        print("To release anyway, pass --open-issues-ok (the owner's decision only).", file=sys.stderr)
        return 4
    print(new)
    if args == ["--next"]:
        return 0

    count = 0
    cargo, n = re.subn(rf'^version = "{re.escape(was)}"$', f'version = "{new}"', cargo, count=1, flags=re.M)
    count += n
    cargo, n = re.subn(rf'^(coxswain-core = \{{.*version = "=){re.escape(was)}(".*)$', rf"\g<1>{new}\g<2>", cargo, flags=re.M)
    count += n
    if count != 2:
        sys.exit(f"release-pr.py: Cargo.toml's two {was} lines not found")
    Path("Cargo.toml").write_text(cargo, encoding="utf-8")

    # Features first, then fixes; each row's text and then its links, as from docs/.
    text = []
    for r in features + [r for r in rows if r not in features]:
        cells = [c.strip() for c in re.split(r"(?<!\\)\|", r.removesuffix(FEATURE).strip())[1:-1]]
        text.append(" ".join(cells))
    date = datetime.datetime.now(datetime.timezone.utc).date().isoformat()
    row = f"| **{new}** | {date} | {' '.join(text)} |"

    def readme_link(m: re.Match) -> str:
        t = m.group(1)
        return m.group(0) if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:|^[#/]", t) else f"](docs/{t})"

    head = "| Version | Date | What's new |\n|---|---|---|\n"
    for page, line, keep in (("docs/changelog.md", row, None), ("README.md", LINK.sub(readme_link, row), 10)):
        body = Path(page).read_text(encoding="utf-8")
        at = body.index(head, body.index("## Versions" if page.startswith("docs") else "## Changelog")) + len(head)
        body = body[:at] + line + "\n" + body[at:]
        if keep:
            lines = body.split("\n")
            versions = [i for i, l in enumerate(lines) if re.match(r"^\| \*\*\d+\.\d+\.\d+\*\* \|", l)]
            for i in reversed(versions[keep:]):
                del lines[i]
            body = "\n".join(lines)
        Path(page).write_text(body, encoding="utf-8")

    for f in frags:
        f.unlink()
    subprocess.run(["cargo", "update", "--workspace", "--quiet"], check=True)
    return 0


if __name__ == "__main__":
    sys.exit(main())
