#!/usr/bin/env python3
"""The metainfo file with its screenshots (from docs/screenshots, at this version's tag) and its
releases (the newest rows of README.md's changelog), written to stdout. Run from the repository
root: python3 packaging/flatpak/metainfo.py > io.github.mwo_dk.Coxswain.metainfo.xml"""
import html
import re
import sys

HERE = "packaging/flatpak/io.github.mwo_dk.Coxswain.metainfo.xml"
SHOTS = [
    ("gui-details.png", "Two panes in the Cyber theme, with git status and colour tags"),
    ("gui-previews.png", "Previews of Markdown, code, pictures and data"),
    ("search-find.png", "Find: names, words inside files and meaning in one list"),
    ("git-history-commits.png", "The git history of a folder"),
    ("gui-themes.png", "Some of the eighteen themes"),
    ("gui-settings.png", "Settings, by task"),
]
KEEP = 10


def plain(md):
    """A changelog cell as plain text: the docs links at its end go, other links become their
    words, emphasis and code marks go."""
    md = re.sub(r"(\s*\[[^\]]*\]\([^)]*\)(\s*·)?)+\s*$", "", md)
    md = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", md)
    md = md.replace("\\|", "|").replace("**", "").replace("`", "")
    md = re.sub(r"(?<![\w*])\*([^*]+)\*(?![\w*])", r"\1", md)
    # AppStream allows no web addresses in a description: they keep their words without the scheme.
    return re.sub(r"https?://", "", md).strip()


def main():
    version = re.search(r'^version = "([^"]+)"', open("Cargo.toml").read(), re.M).group(1)
    rows = re.findall(r"^\| \*\*(\d+\.\d+\.\d+)\*\* \| (\d{4}-\d\d-\d\d) \| (.*) \|$", open("README.md").read(), re.M)
    if not rows or rows[0][0] != version:
        sys.exit(f"metainfo.py: README.md's changelog does not start with {version}")
    releases = "\n".join(
        f'    <release version="{v}" date="{d}">\n      <description>\n        <p>{html.escape(plain(t), quote=False)}</p>\n      </description>\n'
        f'      <url>https://github.com/mwo-dk/coxswain/releases/tag/v{v}</url>\n    </release>'
        for v, d, t in rows[:KEEP]
    )
    base = f"https://raw.githubusercontent.com/mwo-dk/coxswain/v{version}/docs/screenshots"
    shots = "\n".join(
        f'    <screenshot{" type=\"default\"" if i == 0 else ""}>\n      <caption>{c}</caption>\n      <image>{base}/{f}</image>\n    </screenshot>'
        for i, (f, c) in enumerate(SHOTS)
    )
    text = open(HERE).read()
    text = text.replace("  <screenshots/>", f"  <screenshots>\n{shots}\n  </screenshots>")
    text = text.replace("  <releases/>", f"  <releases>\n{releases}\n  </releases>")
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
