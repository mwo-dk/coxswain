#!/usr/bin/env python3
"""Coxswain's bills of materials and licence notices (tools/bom/README.md).

  sbom      one product's SBOM, from cargo-cyclonedx's and cyclonedx-npm's
  cbom      the CBOM, from crypto.toml, checked against the build and the source
  licenses  the npm packages' licences against deny.toml's list (cargo deny checks Rust's)
  notices   THIRD-PARTY-NOTICES.md: every bundled component's licence text

generate.sh runs them in order. Python's standard library only, so CI needs nothing more.
"""

import argparse
import json
import os
import re
import sys
import tomllib
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.normpath(os.path.join(HERE, "..", ".."))
SPEC = "1.6"
HOME = "https://github.com/mwo-dk/coxswain"
PRODUCTS = {
    "terminal": ("coxswain", "Coxswain terminal app"),
    "desktop": ("coxswain-gui", "Coxswain desktop app"),
}
TOOL = {"type": "application", "name": "coxswain tools/bom/bom.py", "externalReferences": [{"type": "vcs", "url": HOME}]}


def fail(msg):
    sys.exit(f"bom.py: {msg}")


def load(path):
    with open(path, encoding="utf-8") as f:
        return json.load(f)


def save(doc, path):
    with open(path, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=1, ensure_ascii=False)
        f.write("\n")


def relative_paths(value):
    """The tools write absolute paths into bom-refs (path+file:///home/runner/...): they would
    show the build machine and make every build differ. Make them relative to the repository."""
    if isinstance(value, dict):
        return {k: relative_paths(v) for k, v in value.items()}
    if isinstance(value, list):
        return [relative_paths(v) for v in value]
    if isinstance(value, str) and REPO in value:
        return value.replace("file://" + REPO, "file://.").replace(REPO, ".")
    return value


def product_component(product, version):
    ref, name = PRODUCTS[product]
    return {
        "type": "application",
        "bom-ref": f"product:{ref}",
        "name": name,
        "version": version,
        "licenses": [{"license": {"id": "MIT"}}],
        "purl": f"pkg:github/mwo-dk/coxswain@v{version}",
        "externalReferences": [{"type": "vcs", "url": HOME}, {"type": "website", "url": HOME}],
    }


def envelope(kind, product_ref, version, timestamp, component, tools, components, dependencies):
    """A CycloneDX document. The serial number is derived from what it describes, so the same
    release always gets the same one."""
    return {
        "bomFormat": "CycloneDX",
        "specVersion": SPEC,
        "serialNumber": "urn:uuid:" + str(uuid.uuid5(uuid.NAMESPACE_URL, f"{HOME}/{kind}/{product_ref}/{version}")),
        "version": 1,
        "metadata": {"timestamp": timestamp, "tools": {"components": tools}, "component": component},
        "components": components,
        "dependencies": dependencies,
    }


def tools_of(doc):
    tools = doc.get("metadata", {}).get("tools", {})
    return tools.get("components", []) if isinstance(tools, dict) else []


# ------------------------------------------------------------------------------ sbom


def cmd_sbom(a):
    product = product_component(a.product, a.version)
    parts = [relative_paths(load(a.rust))] + ([relative_paths(load(a.npm))] if a.npm else [])
    components, by_ref, dependencies, tops, tools = [], {}, {}, [], [TOOL]
    for i, doc in enumerate(parts):
        root = doc["metadata"]["component"]
        renamed = {}
        # Rust and npm name the desktop app's two halves alike: keep their refs apart.
        if i > 0:
            for c in [root] + doc.get("components", []):
                if c.get("bom-ref") in by_ref:
                    renamed[c["bom-ref"]] = "npm:" + c["bom-ref"]
            doc = json.loads(json.dumps(doc), object_hook=lambda o: {k: renamed.get(v, v) if k in ("bom-ref", "ref") else v for k, v in o.items()})
            doc["dependencies"] = [
                {**d, "dependsOn": [renamed.get(x, x) for x in d.get("dependsOn", [])]} for d in doc.get("dependencies", [])
            ]
            root = doc["metadata"]["component"]
        for c in [root] + doc.get("components", []):
            ref = c.get("bom-ref")
            if ref not in by_ref:
                by_ref[ref] = c
                components.append(c)
        for d in doc.get("dependencies", []):
            dependencies.setdefault(d["ref"], set()).update(d.get("dependsOn", []))
        tops.append(root["bom-ref"])
        tools += tools_of(doc)
    dependencies[product["bom-ref"]] = set(tops)
    deps = [{"ref": r, "dependsOn": sorted(d)} for r, d in sorted(dependencies.items())]
    save(envelope("sbom", product["bom-ref"], a.version, a.timestamp, product, tools, components, deps), a.out)
    print(f"{a.out}: {len(components)} components")


# ------------------------------------------------------------------------------ cbom


def locked_crates(metadata_path):
    """Crates in the build, by name: their versions and licences (from `cargo metadata`)."""
    meta = load(metadata_path)
    used = {n["id"] for n in meta["resolve"]["nodes"]}
    crates = {}
    for p in meta["packages"]:
        if p["id"] in used and p.get("source"):
            crates.setdefault(p["name"], []).append(p)
    return crates


def occurrences(found):
    out = []
    for f in found:
        path = os.path.join(REPO, f["file"])
        try:
            lines = open(path, encoding="utf-8").read().splitlines()
        except OSError:
            fail(f"crypto.toml names {f['file']}, which is not there")
        hits = [i + 1 for i, line in enumerate(lines) if f["pattern"] in line]
        if not hits:
            fail(f"crypto.toml: \"{f['pattern']}\" is no longer in {f['file']}; update the pattern")
        out += [{"location": f["file"], "line": n} for n in hits]
    return out


def asset_component(asset):
    kind = asset["type"]
    props = {"assetType": kind}
    if kind == "algorithm":
        ap = {"primitive": asset["primitive"]}
        for key, field in (("param_set", "parameterSetIdentifier"), ("mode", "mode"), ("curve", "curve")):
            if key in asset:
                ap[field] = asset[key]
        if "functions" in asset:
            ap["cryptoFunctions"] = asset["functions"]
        props["algorithmProperties"] = ap
    elif kind == "protocol":
        props["protocolProperties"] = {
            "type": asset["protocol"],
            "version": asset["version"],
            "cipherSuites": asset.get("cipher_suites", []),
            **({"cryptoRefArray": asset["uses"]} if asset.get("uses") else {}),
        }
    else:
        fail(f"crypto.toml: {asset['ref']} has type {kind}; only algorithm and protocol are known")
    if "oid" in asset:
        props["oid"] = asset["oid"]
    c = {"type": "cryptographic-asset", "bom-ref": asset["ref"], "name": asset["name"], "cryptoProperties": props}
    if asset.get("found"):
        c["evidence"] = {"occurrences": occurrences(asset["found"])}
    if asset.get("purpose"):
        c["properties"] = [{"name": "coxswain:purpose", "value": asset["purpose"]}]
    return c


def cmd_cbom(a):
    inv = tomllib.load(open(a.inventory, "rb"))
    crates = locked_crates(a.metadata)
    libs = {lib["crate"]: lib for lib in inv["library"]}
    assets = {x["ref"]: x for x in inv["asset"]}

    problems = [f"{w} is in the build but not in crypto.toml: describe what it does there" for w in inv["watch"] if w in crates and w not in libs]
    problems += [f"crypto.toml describes {c}, which is no longer in the build" for c in libs if c not in crates]
    for lib in inv["library"]:
        problems += [f"{lib['crate']} provides {r}, which crypto.toml does not describe" for r in lib["provides"] if r not in assets]
    for x in assets.values():
        for r in [r for s in x.get("cipher_suites", []) for r in s["algorithms"]] + x.get("uses", []):
            if r not in assets:
                problems.append(f"{x['ref']} refers to {r}, which crypto.toml does not describe")
    if problems:
        fail("\n  " + "\n  ".join(problems))

    root = product_component("terminal", a.version) | {"bom-ref": "product:coxswain-suite", "name": "Coxswain"}
    apps = {p: product_component(p, a.version) for p in PRODUCTS}
    components = list(apps.values())
    dependencies = [{"ref": root["bom-ref"], "dependsOn": [c["bom-ref"] for c in apps.values()]}]
    for p, app in apps.items():
        refs = [f"pkg:cargo/{lib['crate']}@{crates[lib['crate']][0]['version']}" for lib in inv["library"] if p in lib["apps"]]
        dependencies.append({"ref": app["bom-ref"], "dependsOn": refs})
    for lib in inv["library"]:
        for pkg in crates[lib["crate"]]:
            ref = f"pkg:cargo/{pkg['name']}@{pkg['version']}"
            components.append({
                "type": "library",
                "bom-ref": ref,
                "name": pkg["name"],
                "version": pkg["version"],
                "purl": ref,
                **({"licenses": [{"expression": pkg["license"]}]} if pkg.get("license") else {}),
            })
            if lib["provides"]:
                dependencies.append({"ref": ref, "provides": lib["provides"]})
    components += [asset_component(x) for x in inv["asset"]]
    save(envelope("cbom", root["bom-ref"], a.version, a.timestamp, root, [TOOL], components, dependencies), a.out)
    print(f"{a.out}: {len(assets)} cryptographic assets, {len(inv['library'])} libraries")


# ------------------------------------------------------------------------------ licences


def spdx_ok(expr, allowed):
    """Whether an SPDX expression is satisfied by the allowed licences: OR needs one side,
    AND both, and `X WITH exception` counts as X."""
    tokens = re.findall(r"\(|\)|[^\s()]+", expr)
    pos = 0

    def term():
        nonlocal pos
        if tokens[pos] == "(":
            pos += 1
            v = disjunction()
            pos += 1  # ")"
            return v
        lic = tokens[pos]
        pos += 1
        if pos < len(tokens) and tokens[pos] == "WITH":
            pos += 2
        return lic.rstrip("+") in allowed

    def conjunction():
        nonlocal pos
        v = term()
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            v = term() and v
        return v

    def disjunction():
        nonlocal pos
        v = conjunction()
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            v = conjunction() or v
        return v

    return disjunction()


def declared(c):
    """A component's licence as one SPDX expression, or None."""
    ids = []
    for entry in c.get("licenses", []):
        if "expression" in entry:
            ids.append(f"({entry['expression']})")
        elif "license" in entry:
            ids.append(entry["license"].get("id") or entry["license"].get("name", ""))
    return " AND ".join(i for i in ids if i) or None


def cmd_licenses(a):
    policy = tomllib.load(open(a.policy, "rb"))
    deny = tomllib.load(open(os.path.join(REPO, "deny.toml"), "rb"))
    allowed = set(deny["licenses"]["allow"]) | set(policy["npm"]["also_allow"])
    overrides = policy["npm"]["overrides"]
    bad = []
    for c in load(a.sbom).get("components", []):
        key = f"{c.get('name')}@{c.get('version')}"
        if c.get("group"):
            key = f"{c['group']}/{key}"
        expr = overrides.get(key) or declared(c)
        if not expr:
            bad.append(f"{key}: no licence declared; check its files and add it to npm.overrides")
        elif not spdx_ok(expr, allowed):
            bad.append(f"{key}: {expr} is not on the allowed list (deny.toml, licenses.toml)")
    if bad:
        fail("npm licences:\n  " + "\n  ".join(bad))
    print(f"{a.sbom}: every npm licence is allowed")


# ------------------------------------------------------------------------------ notices

LICENCE_FILES = re.compile(r"^(licen[cs]e|copying|notice)([-._].*)?$", re.I)


def package_texts(directory):
    try:
        names = sorted(n for n in os.listdir(directory) if LICENCE_FILES.match(n))
    except OSError:
        return []
    return [(n, open(os.path.join(directory, n), encoding="utf-8", errors="replace").read().strip()) for n in names]


def fenced(text):
    """A code block around a licence text, its fence longer than any run of backticks in it."""
    fence = "`" * max(3, max((len(r) for r in re.findall(r"`+", text)), default=0) + 1)
    return [fence, text, fence]


def cmd_notices(a):
    policy = tomllib.load(open(a.policy, "rb"))
    out = [
        "# Third-party notices",
        "",
        f"Coxswain {a.version} is MIT-licensed (see LICENSE). It is built from the components below, each",
        "under its own licence, whose terms and notices follow. Generated by tools/bom/generate.sh.",
        "",
        "## Bundled outside a package manager",
        "",
    ]
    crates = locked_crates(a.metadata)
    for b in policy["bundled"]:
        out += [f"### {b['name']}", "", f"{b['what']} Licence: {b['license']}. Source: <{b['source']}>.", ""]
        files = [os.path.join(REPO, b["file"])] if b.get("file") else []
        if b.get("crate"):
            if b["crate"] not in crates:
                fail(f"licenses.toml: {b['name']} comes with the crate {b['crate']}, which is no longer in the build")
            directory = os.path.dirname(crates[b["crate"]][0]["manifest_path"])
            files += [os.path.join(directory, f) for f in b["crate_files"]]
        for path in files:
            try:
                text = open(path, encoding="utf-8").read().strip()
            except OSError:
                fail(f"licenses.toml: {b['name']}'s licence file {path} is not there")
            out += [f"{os.path.basename(path)}:", ""] + fenced(text) + [""]
    out += ["## The desktop app's web frontend (npm)", ""]
    overrides = policy["npm"]["overrides"]
    for c in sorted(load(a.npm_sbom).get("components", []), key=lambda c: (c.get("group", ""), c["name"])):
        name = f"{c['group']}/{c['name']}" if c.get("group") else c["name"]
        expr = overrides.get(f"{name}@{c.get('version')}") or declared(c) or "unknown"
        out += [f"### {name} {c.get('version', '')}", "", f"Licence: {expr}.", ""]
        texts = package_texts(os.path.join(a.node_modules, name))
        for file, text in texts:
            out += [f"{file}:", ""] + fenced(text) + [""]
        if not texts:
            out += ["The package ships no licence file; its terms are those of the licence named above.", ""]
    out += ["## The Rust crates", ""]
    rust = open(a.rust, encoding="utf-8").read().strip()
    out += [rust, ""]
    with open(a.out, "w", encoding="utf-8") as f:
        f.write("\n".join(out))
    print(f"{a.out}: written")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = p.add_subparsers(dest="cmd", required=True)
    s = sub.add_parser("sbom")
    s.add_argument("--product", choices=PRODUCTS, required=True)
    s.add_argument("--rust", required=True)
    s.add_argument("--npm")
    for q in [s, c := sub.add_parser("cbom")]:
        q.add_argument("--version", required=True)
        q.add_argument("--timestamp", required=True)
        q.add_argument("--out", required=True)
    c.add_argument("--inventory", default=os.path.join(HERE, "crypto.toml"))
    c.add_argument("--metadata", required=True)
    lic = sub.add_parser("licenses")
    lic.add_argument("--sbom", required=True)
    lic.add_argument("--policy", default=os.path.join(HERE, "licenses.toml"))
    n = sub.add_parser("notices")
    n.add_argument("--version", required=True)
    n.add_argument("--rust", required=True)
    n.add_argument("--npm-sbom", required=True)
    n.add_argument("--metadata", required=True)
    n.add_argument("--node-modules", default=os.path.join(REPO, "gui", "node_modules"))
    n.add_argument("--policy", default=os.path.join(HERE, "licenses.toml"))
    n.add_argument("--out", required=True)
    a = p.parse_args()
    {"sbom": cmd_sbom, "cbom": cmd_cbom, "licenses": cmd_licenses, "notices": cmd_notices}[a.cmd](a)


if __name__ == "__main__":
    main()
