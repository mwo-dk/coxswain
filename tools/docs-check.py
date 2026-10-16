#!/usr/bin/env python3
"""The docs check: README.md and docs/ are held to the code, on every pull request.

  python3 tools/docs-check.py [--base <ref>]

1. Every relative link and #anchor in README.md, docs/**/*.md and design/*.md points to a file and a heading
   (GitHub's anchors: lowercase, spaces to '-', punctuation dropped, duplicates -1, -2 …; also
   <a id>/<a name>/id= anchors). Pictures and GIFs too.
2. The version in Cargo.toml has its row in docs/changelog.md, and the README's changelog table is
   the newest ten rows of docs/changelog.md, word for word (links compared by where they lead).
3. No version older than the current major.minor is shown as Coxswain's own (`coxswain 1.20.0`,
   `User-Agent: coxswain/1.20.0`, `coxswain-terminal-v1.23.1-…`) outside the changelogs and picture
   descriptions: such examples go stale, so they say `X.Y.Z`.
4. No `<!-- screenshot:` placeholder is left when the version raises its minor or major number
   over the base branch's (--base, else origin/$GITHUB_BASE_REF, else origin/master); otherwise
   each one is a warning.

What the code shows (themes, actions, config keys, flags, features) is checked by
`cargo test -p coxswain-core --test docs`. Python 3.11+, standard library only.
"""

import argparse
import os
import re
import subprocess
import sys
import tomllib
import urllib.parse
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
GHA = os.environ.get("GITHUB_ACTIONS") == "true"
errors: list[str] = []


def error(where: str, msg: str) -> None:
    errors.append(f"{where}: {msg}")
    if GHA:
        file, _, line = where.partition(":")
        print(f"::error file={file},line={line or 1}::{msg}")


def warning(where: str, msg: str) -> None:
    print(f"::warning file={where.partition(':')[0]}::{msg}" if GHA else f"warning: {where}: {msg}")


def pages() -> list[Path]:
    return [ROOT / "README.md", *sorted((ROOT / "docs").rglob("*.md"))]


def rel(p: Path) -> str:
    return p.relative_to(ROOT).as_posix()


FENCE = re.compile(r"^\s*(```|~~~)")


def prose_lines(text: str):
    """(line number, line) outside fenced code blocks."""
    fence = None
    for n, line in enumerate(text.splitlines(), 1):
        m = FENCE.match(line)
        if m:
            fence = None if fence == m.group(1) else (fence or m.group(1))
            continue
        if fence is None:
            yield n, line


def slug(heading: str) -> str:
    """GitHub's anchor for a heading's text (github-slugger)."""
    t = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", heading)  # links and pictures: their text
    # HTML tags go, but not in code, where `<name>` is text
    t = "".join(s if i % 2 else re.sub(r"<[^>]+>", "", s) for i, s in enumerate(re.split(r"`+", t))).lower()
    return "".join("-" if c == " " else c for c in t if c.isalnum() or c in "-_ ")


_anchors: dict[Path, set[str]] = {}


def anchors(page: Path) -> set[str]:
    if page not in _anchors:
        text = page.read_text(encoding="utf-8")
        seen: dict[str, int] = {}
        found = set(re.findall(r"""(?:\bid|<a\s+name)\s*=\s*["']([^"']+)["']""", text))
        for _, line in prose_lines(text):
            m = re.match(r"^#{1,6}\s+(.*?)\s*#*\s*$", line)
            if m:
                s = slug(m.group(1))
                n = seen.get(s, 0)
                seen[s] = n + 1
                found.add(s if n == 0 else f"{s}-{n}")
        _anchors[page] = found
    return _anchors[page]


LINK = re.compile(r"\]\(\s*<?([^)\s>]+)")
HTML = re.compile(r"""\b(?:href|src|srcset)\s*=\s*["']([^"']+)["']""")
REFDEF = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*<?(\S+?)>?(?:\s|$)")


def links(page: Path):
    """(line number, target) of every link, picture and HTML reference outside code."""
    for n, line in prose_lines(page.read_text(encoding="utf-8")):
        line = re.sub(r"(`+).*?\1", "", line)  # inline code
        for m in (*LINK.finditer(line), *HTML.finditer(line)):
            yield n, m.group(1)
        m = REFDEF.match(line)
        if m:
            yield n, m.group(1)


def check_links() -> int:
    count = 0
    for page in pages() + sorted((ROOT / "design").glob("*.md")):
        for n, target in links(page):
            if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:|^//", target):
                continue  # https:, mailto: …
            count += 1
            where = f"{rel(page)}:{n}"
            path, _, anchor = target.partition("#")
            dest = (page.parent / urllib.parse.unquote(path)).resolve() if path else page
            if not dest.exists():
                error(where, f"{target}: no such file")
            elif anchor and dest.suffix == ".md":
                if urllib.parse.unquote(anchor) not in anchors(dest):
                    error(where, f"{target}: no heading or id #{anchor} in {rel(dest)}")
    return count


ROW = re.compile(r"^\|\s*\*\*(\d+\.\d+\.\d+)\*\*\s*\|")


def changelog_rows(page: Path) -> list[tuple[str, str]]:
    """(version, row with every relative link made relative to the repository) of a table."""
    rows = []
    for line in page.read_text(encoding="utf-8").splitlines():
        m = ROW.match(line)
        if m:
            def to_root(l: re.Match) -> str:
                t = l.group(1)
                if re.match(r"^[a-zA-Z][a-zA-Z0-9+.-]*:", t):
                    return l.group(0)
                path, hash_, anchor = t.partition("#")
                return f"]({os.path.relpath(page.parent / path if path else page, ROOT).replace(os.sep, '/')}{hash_}{anchor}"
            rows.append((m.group(1), LINK.sub(to_root, line.strip())))
    return rows


def version(cargo: str) -> str:
    return tomllib.loads(cargo)["workspace"]["package"]["version"]


def check_changelog(v: str) -> None:
    full = changelog_rows(ROOT / "docs/changelog.md")
    readme = changelog_rows(ROOT / "README.md")
    if v not in [r[0] for r in full]:
        error("docs/changelog.md", f"no row for {v}, the version in Cargo.toml")
    if v not in [r[0] for r in readme]:
        error("README.md", f"no changelog row for {v}, the version in Cargo.toml")
    want = full[:10]
    if readme != want:
        for i in range(max(len(readme), len(want))):
            a = readme[i] if i < len(readme) else None
            b = want[i] if i < len(want) else None
            if a != b:
                error(
                    "README.md",
                    f"changelog row {i + 1} must be docs/changelog.md's "
                    f"({b[0] if b else 'none, only ten'}), is {a[0] if a else 'missing'}"
                    + (" with other text" if a and b and a[0] == b[0] else ""),
                )
                break


OWN_VERSION = re.compile(r"\b[Cc]oxswain[\w-]*[ /-]v?(\d+)\.(\d+)\.\d+\b")
HISTORY = {"docs/changelog.md", "docs/whats-new-2.md"}


def check_stale_versions(v: str) -> None:
    now = tuple(map(int, v.split(".")[:2]))
    for page in pages():
        if rel(page) in HISTORY:
            continue
        for n, line in enumerate(page.read_text(encoding="utf-8").splitlines(), 1):
            line = re.sub(r"!\[[^\]]*\]", "", line)  # a picture's description says what it shows
            for m in OWN_VERSION.finditer(line):
                history = re.search(r"\b(until|before|since|after|from)\W*$", line[: m.start()], re.I) or re.match(
                    r"\s+and (later|earlier)", line[m.end() :]
                )  # "until Coxswain 1.17.0", "2.1.1 and later": history, not an example
                if (int(m.group(1)), int(m.group(2))) < now and not history:
                    error(f"{rel(page)}:{n}", f"{m.group(0)}: an old version as an example; say X.Y.Z")


def base_version(base: str | None) -> str | None:
    if not base:
        ref = os.environ.get("GITHUB_BASE_REF")
        base = f"origin/{ref}" if ref else "origin/master"
    try:
        cargo = subprocess.run(
            ["git", "show", f"{base}:Cargo.toml"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError):
        warning("Cargo.toml", f"cannot read Cargo.toml at {base}: placeholders are only warned about")
        return None
    return version(cargo)


def check_placeholders(v: str, base: str | None) -> int:
    was = base_version(base)
    raised = was is not None and tuple(map(int, v.split(".")[:2])) > tuple(map(int, was.split(".")[:2]))
    count = 0
    for page in pages():
        for n, line in enumerate(page.read_text(encoding="utf-8").splitlines(), 1):
            if "<!-- screenshot:" in line:
                count += 1
                msg = f"screenshot placeholder: {line.strip()}"
                if raised:
                    error(f"{rel(page)}:{n}", f"{msg} ({was} → {v} is a minor or major release: take it)")
                else:
                    warning(f"{rel(page)}:{n}", msg)
    return count


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--base", help="the branch the version is compared with (default origin/master)")
    args = ap.parse_args()
    v = version((ROOT / "Cargo.toml").read_text(encoding="utf-8"))
    n = check_links()
    check_changelog(v)
    check_stale_versions(v)
    p = check_placeholders(v, args.base)
    for e in errors:
        print(e, file=sys.stderr)
    print(f"docs check: {len(pages())} pages, {n} links, {p} placeholders, version {v}: "
          + (f"{len(errors)} problems" if errors else "all good"))
    return 1 if errors else 0


if __name__ == "__main__":
    sys.exit(main())
