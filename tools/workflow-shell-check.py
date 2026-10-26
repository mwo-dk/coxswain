#!/usr/bin/env python3
"""Checks the shell scripts in .github/workflows before any of them runs (CI, docs job).

Every `run:` (and a VM action's `prepare:`) block goes through `bash -n` and shellcheck, and so
does every script inside a single-quoted `bash -c '…'` / `sh -c '…'` in them: an apostrophe in a
comment there once broke a job only after a long VM or container start. `${{ … }}` expressions
stand in as a plain word. A step with `shell: vmsh {0}` (a BSD VM's /bin/sh) is checked as sh, one
with `shell: pwsh` or `python` is skipped. Prints `file:line: …` for each fault, exits 1 on any.

    python3 tools/workflow-shell-check.py [workflow.yml …]   (default: .github/workflows/*.yml)

Needs shellcheck (preinstalled on GitHub's Ubuntu runners). Plain text parsing, no YAML library:
only block scalars (`run: |`) and one-line values are read, which is all the workflows use.
"""
import re
import shlex
import subprocess
import sys
from pathlib import Path

KEY = re.compile(r"^(\s*)(- )?(run|prepare):[ \t]*(.*)$")
EXPR = re.compile(r"\$\{\{.*?\}\}")
INNER = re.compile(r"\b(?:bash|sh)\s+(?:-[a-zA-Z]+\s+)*-[a-zA-Z]*c\s+'")
# Off in every script: SC2016 ($ in single quotes is what `bash -c '…'` is for), SC1091 (sourced
# files are not followed), and style: SC2129, SC2034 (a retry loop's unused `i`), SC2155 (a step
# fails on its own line anyway), SC2174 (mkdir -m -p of one folder).
EXCLUDE = "SC2016,SC1091,SC2129,SC2034,SC2155,SC2174"


def step_shell(lines: list[str], at: int, indent: int) -> str:
    """The `shell:` key next to line `at` in the same step (same indent), or ''."""
    for rng in (range(at - 1, -1, -1), range(at + 1, len(lines))):
        for i in rng:
            line = lines[i]
            if not line.strip() or line.lstrip().startswith("#"):
                continue
            ind = len(line) - len(line.lstrip())
            if ind < indent - 2 or (ind == indent - 2 and not line.lstrip().startswith("- ")):
                break
            if ind == indent and (m := re.match(r"\s*shell:\s*(\S+)", line)):
                return m.group(1)
            if ind == indent - 2:  # this step's `- ` line: no further back
                if (m := re.match(r"\s*- shell:\s*(\S+)", line)):
                    return m.group(1)
                break
    return ""


def blocks(path: Path):
    """(first line number, script, dialect) for each run/prepare block of a workflow."""
    lines = path.read_text(encoding="utf-8").split("\n")
    for i, line in enumerate(lines):
        m = KEY.match(line)
        if not m or not m.group(4) or m.group(4).startswith("#"):
            continue
        indent = len(m.group(1)) + (2 if m.group(2) else 0)
        shell = step_shell(lines, i, indent)
        if shell.startswith(("pwsh", "powershell", "python", "cmd", "node")):
            continue
        dialect = "sh" if shell.startswith(("vmsh", "sh")) else "bash"
        value = m.group(4)
        if value[0] in "|>":
            body = []
            for line2 in lines[i + 1:]:
                if line2.strip() and len(line2) - len(line2.lstrip()) <= indent:
                    break
                body.append(line2)
            while body and not body[-1].strip():
                body.pop()
            cut = min((len(b) - len(b.lstrip()) for b in body if b.strip()), default=0)
            yield i + 2, "\n".join(b[cut:] for b in body) + "\n", dialect
        else:
            value = re.sub(r"\s+#.*$", "", value)
            if value[0] == "'" and value.endswith("'"):
                value = value[1:-1].replace("''", "'")
            elif value[0] == '"' and value.endswith('"'):
                value = value[1:-1].replace('\\"', '"')
            yield i + 1, value + "\n", dialect


def inner(script: str):
    """(line offset, script) for each `bash -c '…'` / `sh -c '…'` inside a script."""
    for m in INNER.finditer(script):
        lex = shlex.shlex(script[m.end() - 1:], posix=True)
        lex.whitespace_split = True
        lex.commenters = ""
        try:
            text = lex.get_token()
        except ValueError as e:  # no closing quote: bash -n of the outer script says so too
            yield script.count("\n", 0, m.start()), None, str(e)
            continue
        yield script.count("\n", 0, m.start()), text, None


def check(where: str, script: str, dialect: str) -> list[str]:
    """`bash -n` and shellcheck faults, as `where+line: text`."""
    file, first = where.rsplit(":", 1)
    first = int(first)
    script = EXPR.sub("GH_EXPR", script)
    faults = []
    r = subprocess.run(["bash", "-n"], input=script, capture_output=True, text=True)
    for e in r.stderr.splitlines():
        n = re.search(r"line (\d+):", e)
        faults.append(f"{file}:{first + int(n.group(1)) - 1 if n else first}: bash -n: {e}")
    r = subprocess.run(["shellcheck", "-s", dialect, "-S", "warning", "-e", EXCLUDE, "-f", "gcc", "-"],
                       input=script, capture_output=True, text=True)
    for e in r.stdout.splitlines():
        if (n := re.match(r"-:(\d+):\d+: (.*)$", e)):
            faults.append(f"{file}:{first + int(n.group(1)) - 1}: shellcheck: {n.group(2)}")
    return faults


def main() -> int:
    files = [Path(a) for a in sys.argv[1:]] or sorted(Path(".github/workflows").glob("*.yml"))
    faults, count = [], 0
    for f in files:
        for line, script, dialect in blocks(f):
            count += 1
            faults += check(f"{f}:{line}", script, dialect)
            for off, text, err in inner(script):
                if err:
                    faults.append(f"{f}:{line + off}: bash -c '…': {err}")
                    continue
                count += 1
                # The inner script runs in a container or VM: sh unless it says bash.
                dialect2 = "bash" if re.search(r"\bbash\s", script.split("\n")[off]) else "sh"
                faults += check(f"{f}:{line + off}", text, dialect2)
    for x in faults:
        print(x)
    print(f"workflow-shell-check: {count} scripts, {len(faults)} faults", file=sys.stderr)
    return 1 if faults else 0


if __name__ == "__main__":
    sys.exit(main())
