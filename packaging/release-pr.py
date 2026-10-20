#!/usr/bin/env python3
"""The release pull request's changes, made in the checkout it runs in (CLAUDE.md, Shipping).

    packaging/release-pr.py --next    print the next version and change nothing
    packaging/release-pr.py           make the release: print the version, then
        bump [workspace.package] and coxswain-core's `=X.Y.Z` in Cargo.toml, Cargo.lock with them
        (cargo update --workspace), every changelog.d/ row moved into one row for the version in
        docs/changelog.md and README.md (which keeps ten), and the changelog.d/ files deleted.

A minor release when any row ends with `<!-- feature -->`, a patch release otherwise; a major one
is made by hand. Exits 2, changing nothing, when changelog.d/ has no rows. Run from the
repository's root.
"""
import datetime
import re
import subprocess
import sys
from pathlib import Path

FEATURE = "<!-- feature -->"
LINK = re.compile(r"\]\(([^)]+)\)")


def main() -> int:
    frags = sorted(Path("changelog.d").glob("*.md"))
    if not frags:
        print("changelog.d/ has no rows: nothing to release.", file=sys.stderr)
        return 2
    rows = [f.read_text(encoding="utf-8").strip() for f in frags]
    features = [r for r in rows if r.endswith(FEATURE)]
    cargo = Path("Cargo.toml").read_text(encoding="utf-8")
    old = re.search(r'^\[workspace\.package\]\n(?:.*\n)*?version = "(\d+)\.(\d+)\.(\d+)"', cargo, re.M)
    major, minor, patch = map(int, old.groups())
    new = f"{major}.{minor + 1}.0" if features else f"{major}.{minor}.{patch + 1}"
    print(new)
    if sys.argv[1:] == ["--next"]:
        return 0
    was = ".".join(old.groups())

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
