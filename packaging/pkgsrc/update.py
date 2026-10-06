#!/usr/bin/env python3
"""Point the pkgsrc recipe at a coxswain release on crates.io.

    python3 packaging/pkgsrc/update.py 2.8.0

Writes coxswain/cargo-depends.mk (what `make print-cargo-depends` prints) and
coxswain/distinfo (what `make makesum` writes), and sets DISTNAME in the Makefile.
Needs only Python 3; no pkgsrc tree. Every crate is checked against the SHA-256
in the release's Cargo.lock. Downloads are kept in ~/.cache/coxswain-pkgsrc.
"""
import concurrent.futures, hashlib, io, pathlib, re, sys, tarfile, tomllib, urllib.request

HERE = pathlib.Path(__file__).resolve().parent / "coxswain"
CACHE = pathlib.Path.home() / ".cache" / "coxswain-pkgsrc"
CRATES = "https://static.crates.io/crates"


def fetch(name, version, sha256=None):
    path = CACHE / f"{name}-{version}.crate"
    if not path.exists():
        req = urllib.request.Request(f"{CRATES}/{name}/{name}-{version}.crate",
                                     headers={"User-Agent": "coxswain-pkgsrc-update"})
        with urllib.request.urlopen(req) as r:
            data = r.read()
        if sha256 and hashlib.sha256(data).hexdigest() != sha256:
            sys.exit(f"{path.name}: SHA-256 differs from Cargo.lock")
        path.write_bytes(data)
    return path


def distinfo_lines(path):
    data = path.read_bytes()
    n = path.name
    return [f"BLAKE2s ({n}) = {hashlib.blake2s(data).hexdigest()}",
            f"SHA512 ({n}) = {hashlib.sha512(data).hexdigest()}",
            f"Size ({n}) = {len(data)} bytes"]


def main():
    if len(sys.argv) != 2 or not re.fullmatch(r"\d+\.\d+\.\d+", sys.argv[1]):
        sys.exit(__doc__)
    version = sys.argv[1]
    CACHE.mkdir(parents=True, exist_ok=True)
    main_crate = fetch("coxswain", version)
    with tarfile.open(main_crate) as t:
        lock = tomllib.load(io.BytesIO(t.extractfile(f"coxswain-{version}/Cargo.lock").read()))
    deps = [p for p in lock["package"] if p.get("source")]
    if any(not p["source"].startswith("registry+") for p in deps):
        sys.exit("Cargo.lock has a git dependency; pkgsrc needs CARGO_GITHUB_CRATES for it")

    with concurrent.futures.ThreadPoolExecutor(16) as pool:
        paths = list(pool.map(lambda p: fetch(p["name"], p["version"], p["checksum"]), deps))

    (HERE / "cargo-depends.mk").write_text(
        "# $NetBSD$\n\n"
        + "".join(f"CARGO_CRATE_DEPENDS+=\t{p['name']}-{p['version']}\n" for p in deps))
    lines = []
    for path in sorted({main_crate, *paths}, key=lambda p: p.name):
        lines += distinfo_lines(path)
    (HERE / "distinfo").write_text("$NetBSD$\n\n" + "\n".join(lines) + "\n")
    mk = HERE / "Makefile"
    mk.write_text(re.sub(r"(?m)^DISTNAME=\tcoxswain-.*$", f"DISTNAME=\tcoxswain-{version}", mk.read_text()))
    print(f"coxswain {version}: {len(deps)} crates in cargo-depends.mk and distinfo")


if __name__ == "__main__":
    main()
