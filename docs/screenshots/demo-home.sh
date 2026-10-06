#!/usr/bin/env bash
# shellcheck disable=SC2016 # backticks below are markdown, not command substitution
# Build an anonymous demo home folder for screenshots: a few documents and two git
# repositories, one clean and one with every kind of change the git glyphs show.
#
#   docs/screenshots/demo-home.sh DIR
set -euo pipefail
d="${1:?usage: demo-home.sh DIR}"
here="$(cd "$(dirname "$0")" && pwd)"
rm -rf "$d"
mkdir -p "$d"/{Desktop,Documents,Downloads,Music,Pictures,Videos,projects/rocket/src,projects/website}
cd "$d"

# Each git call an hour after the last, the last one an hour ago: a history with real dates.
n=0
g() {
  n=$((n + 1))
  local when=$(( $(date +%s) - (40 - n) * 3600 ))
  GIT_AUTHOR_DATE="@$when" GIT_COMMITTER_DATE="@$when" GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -c user.name="Demo User" \
    -c user.email=demo@example.com -c commit.gpgsign=false -c init.defaultBranch=master "$@"
}

truncate -s 12K Documents/letter.odt
python3 "$here/demo-docs.py" "$d"
printf '# Notes\n\n- Book the ferry\n- Renew the domain\n- Try `coxswain` on the laptop\n' > Documents/notes.md
truncate -s 5M Downloads/setup.iso
# Real pictures for the thumbnail view and the preview (ImageMagick draws them).
img() { magick -size 1200x800 "$@"; }
img gradient:'#0b3d91'-'#f4a261' -swirl 60 Pictures/sunset.jpg
img plasma:'#2a9d8f'-'#264653' -blur 0x2 Pictures/lagoon.jpg
img radial-gradient:'#ffd166'-'#ef476f' Pictures/glow.jpg
img plasma:'#8ecae6'-'#023047' Pictures/holiday.jpg
img gradient:'#606c38'-'#fefae0' -wave 60x300 -resize 1200x800! Pictures/hills.png
img xc:'#1d3557' -fill '#e63946' -draw 'circle 600,400 600,150' -fill '#f1faee' -draw 'circle 600,400 600,300' Pictures/target.png
cp Pictures/lagoon.jpg Downloads/wallpaper.jpg
# A one-page PDF for the preview pane.
magick -size 1240x1754 xc:white -font "$(fc-match -f '%{file}' sans)" -fill '#1d3557' -pointsize 64 -annotate +120+220 'Launch report' \
  -fill '#333' -pointsize 34 -annotate +120+320 'Rocket, flight 7: nominal ascent, early engine cut-off.' \
  -fill '#2a9d8f' -draw 'rectangle 120,420 1120,900' -fill white -pointsize 40 -annotate +160+680 'Speed over time' \
  Documents/launch-report.pdf

cd projects/website
cat > package.json <<'EOF'
{
  "name": "website",
  "version": "0.3.0",
  "scripts": { "dev": "vite", "build": "vite build" }
}
EOF
printf '# Website\n\nThe landing page.\n' > README.md
g init -q && g add README.md package.json && g commit -qm "Start the site"
printf '<!doctype html>\n<title>Rocket</title>\n<link rel="stylesheet" href="style.css">\n<h1>Rocket</h1>\n' > index.html
g add index.html && g commit -qm "Add the landing page"
printf 'body { font-family: sans-serif; margin: 2rem; }\nh1 { color: #0a6; }\n' > style.css
g add style.css && g commit -qm "Style the heading"

cd ../rocket
cat > Cargo.toml <<'EOF'
[package]
name = "rocket"
version = "0.1.0"
edition = "2024"
EOF
printf 'target/\n' > .gitignore
printf '# Rocket\n\nA tiny launch simulator.\nRun it with `cargo run`.\n' > README.md
g init -q && g add . && g commit -qm "Cargo project"
cat > src/engine.rs <<'EOF'
pub struct Engine {
    pub thrust: f64,
    pub fuel: f64,
}

impl Engine {
    pub fn new(thrust: f64, fuel: f64) -> Self {
        Engine { thrust, fuel }
    }

    /// Burn for `secs` seconds and return the impulse delivered.
    pub fn burn(&mut self, secs: f64) -> f64 {
        let used = secs.min(self.fuel);
        self.fuel -= used;
        used * self.thrust
    }
}
EOF
g add src/engine.rs && g commit -qm "Add the engine"
cat > src/main.rs <<'EOF'
mod engine;

use engine::Engine;

fn main() {
    let mut engine = Engine::new(9.8, 30.0);
    let mut speed = 0.0;
    for second in 0..10 {
        speed += engine.burn(1.0) - 9.81;
        println!("t={second:>2}s speed={speed:.1} m/s");
    }
}
EOF
g add src/main.rs && g commit -qm "Fly for ten seconds"
mkdir -p target/debug && truncate -s 3M target/debug/rocket
sed -i 's/tiny/small/' README.md && g commit -qam "Reword the readme"
# Another author, a fix and a rename, for the history (Ctrl+G) and the Last commit column.
ada() { g -c user.name=Ada -c user.email=ada@example.com "$@"; }
mkdir -p docs && printf '# Launch plan\n\n1. Fuel the tanks.\n2. Clear the pad.\n3. Arm the igniter.\n4. Count down from ten.\n5. Lift off.\n' > docs/plan.md
g add docs && g commit -qm "Write the launch plan"
sed -i 's|let used = secs.min(self.fuel);|let used = secs.min(self.fuel); // the valve closes on time now|' src/engine.rs
ada commit -qam "Fix the fuel valve"
g mv docs/plan.md docs/launch-plan.md && printf 'Hold at T-10 if the wind is over 12 m/s.\n' >> docs/launch-plan.md
g add docs && ada commit -qm "Rename the plan, add the wind rule"

g init -q --bare "$d/.remotes/rocket.git"
g remote add origin "$d/.remotes/rocket.git"
g push -q -u origin master
printf '\n## Build\n\n    cargo build --release\n' >> README.md && g commit -qam "Document the build"
# A branch one commit ahead, for the branches folder (Alt+B).
g checkout -q -b feature/engine && printf 'pub fn throttle(level: f64) -> f64 {\n    level.clamp(0.0, 1.0)\n}\n' > src/throttle.rs
g add src/throttle.rs && g commit -qm "Add the throttle" && g checkout -q master

echo "Launch window: Tuesday" >> README.md && g stash -q
sed -i 's/30.0/45.0/' src/main.rs
printf 'pub const TANK: f64 = 45.0;\n' > src/fuel.rs && g add src/fuel.rs
printf 'Add a landing burn\n' > TODO.txt

# Archives, for the preview's contents list, browsing and extracting: a tar.gz, a zip and a 7z,
# and two locked ones (password "rocket"), the 7z with its names locked too.
cd "$d" && tar czf Downloads/website-0.3.0.tar.gz --exclude=.git -C projects website
python3 -c 'import sys, zipfile
d = sys.argv[1]
with zipfile.ZipFile(d + "/Downloads/rocket-src.zip", "w", zipfile.ZIP_DEFLATED) as z:
    for f in ("Cargo.toml", "README.md", "src/main.rs", "src/engine.rs"):
        z.write(d + "/projects/rocket/" + f, "rocket/" + f)' "$d"
if command -v 7z >/dev/null; then
  (cd projects && 7z a -bd -bso0 "$d/Downloads/website-0.3.0.7z" website '-xr!.git')
  (cd Documents && 7z a -bd -bso0 -tzip -procket "$d/Documents/secret.zip" debrief.docx notes.md)
  (cd Documents && 7z a -bd -bso0 -procket -mhe=on "$d/Documents/secret.7z" budget.xlsx notes.md)
fi
# A Parquet table (pyarrow through uv; left out without uv).
if command -v uv >/dev/null; then
  uv run -q --with pyarrow python -c 'import sys, pyarrow as pa, pyarrow.parquet as pq
months = ["January", "February", "March", "April", "May", "June"] * 50
pq.write_table(pa.table({"flight": list(range(1, 301)), "month": months,
    "fuel_t": [round(38 + (i * 7) % 23 + 0.5, 1) for i in range(300)]}), sys.argv[1] + "/Documents/flights.parquet")' "$d" || true
fi

# Duplicates, the way an old disk has them: a backup of Pictures, a PDF downloaded twice and a
# document kept in two places.
mkdir -p "$d/Backups/old-laptop-2019"
cp -r "$d/Pictures" "$d/Backups/old-laptop-2019/Pictures"
cp "$d/Documents/launch-report.pdf" "$d/Downloads/launch-report (1).pdf"
cp "$d/Documents/debrief.docx" "$d/Backups/old-laptop-2019/debrief.docx"
touch -d "2019-06-01 12:00" "$d/Backups/old-laptop-2019/debrief.docx"

# A folder nothing can be written to, for the picture of a copy that fails.
mkdir -p "$d/Backups/read-only" && chmod 555 "$d/Backups/read-only"

# No "new version" notice in the pictures.
mkdir -p "$d/.config/coxswain" && printf 'check_updates = false\n' > "$d/.config/coxswain/config.toml"
# Build provenance for a release in Downloads: built from the commit before HEAD, named in a
# GitHub remote that is never fetched (projects/rocket in the other pane finds the checkout).
# One archive matches, one was changed after the build, one is missing, and an image.
g -C "$d/projects/rocket" remote add github https://github.com/demo/rocket.git
built="$(g -C "$d/projects/rocket" rev-parse HEAD~1)"
mkdir -p "$d/Downloads/rocket-1.4.0"
python3 - "$d/Downloads/rocket-1.4.0" "$built" <<'PY'
import base64, hashlib, json, sys
out, commit = sys.argv[1], sys.argv[2]
files = {"rocket-1.4.0-x86_64-linux.tar.gz": b"rocket 1.4.0 for x86_64\n", "rocket-1.4.0-aarch64-linux.tar.gz": b"rocket 1.4.0 for aarch64\n"}
for name, data in files.items():
    open(f"{out}/{name}", "wb").write(data)
subject = lambda name, data: {"name": name, "digest": {"sha256": hashlib.sha256(data).hexdigest()}}
statement = {
    "_type": "https://in-toto.io/Statement/v1",
    "subject": [subject(n, d) for n, d in files.items()] + [subject("rocket-1.4.0-x86_64.msi", b"msi"), {"name": "ghcr.io/demo/rocket", "digest": {"sha256": hashlib.sha256(b"image").hexdigest()}}],
    "predicateType": "https://slsa.dev/provenance/v1",
    "predicate": {
        "buildDefinition": {
            "buildType": "https://actions.github.io/buildtypes/workflow/v1",
            "externalParameters": {"workflow": {"ref": "refs/tags/v1.4.0", "repository": "https://github.com/demo/rocket", "path": ".github/workflows/release.yml"}},
            "internalParameters": {"github": {"event_name": "push", "repository_id": "1", "repository_owner_id": "2", "runner_environment": "github-hosted"}},
            "resolvedDependencies": [{"uri": "git+https://github.com/demo/rocket@refs/tags/v1.4.0", "digest": {"gitCommit": commit}}],
        },
        "runDetails": {
            "builder": {"id": "https://github.com/actions/runner/github-hosted"},
            "metadata": {"invocationId": "https://github.com/demo/rocket/actions/runs/7/attempts/1", "startedOn": "2026-10-05T09:12:00Z", "finishedOn": "2026-10-05T09:26:00Z"},
        },
    },
}
# Changed after the build: this archive no longer has the digest the provenance names.
open(f"{out}/rocket-1.4.0-aarch64-linux.tar.gz", "ab").write(b"patched\n")
envelope = {"payloadType": "application/vnd.in-toto+json", "payload": base64.b64encode(json.dumps(statement).encode()).decode(), "signatures": []}
open(f"{out}/rocket-1.4.0.intoto.jsonl", "w").write(json.dumps(envelope) + "\n")
PY

# A CBOM next to the source it was scanned from, and an older scan to compare with: CBOMkit's
# real scan of Keycloak (Apache-2.0, from the BOM test fixtures), with a stub for every file it
# names, so "Found in" finds them.
mkdir -p "$d/projects/keycloak" "$d/projects/keycloak-last-month"
cp "$here/../../crates/coxswain-core/src/bom/testdata/cbomkit/keycloak.cdx.json" "$d/projects/keycloak/cbom.cdx.json"
python3 - "$d/projects" <<'PY'
import json, os, sys
projects = sys.argv[1]
doc = json.load(open(f"{projects}/keycloak/cbom.cdx.json"))
for c in doc["components"]:
    for o in c.get("evidence", {}).get("occurrences", []):
        path = os.path.join(projects, "keycloak", o["location"])
        os.makedirs(os.path.dirname(path), exist_ok=True)
        name = os.path.basename(path).rsplit(".", 1)[0]
        open(path, "w").write(f"package org.keycloak;\n\npublic class {name} {{\n}}\n")
# A month earlier: SHA-256 where SHA-1 is now, no DSA yet, and an RC4 that has since gone.
c = doc["components"]
sha1 = next(x for x in c if x["name"] == "SHA1")
sha1["name"] = "SHA256"
sha1["cryptoProperties"]["oid"] = "2.16.840.1.101.3.4.2.1"
sha1["cryptoProperties"].setdefault("algorithmProperties", {})["parameterSetIdentifier"] = "256"
dsa = lambda x: x["name"] == "DSA" or x.get("evidence", {}).get("occurrences", [{}])[0].get("location", "").endswith("DSAKeyValueType.java")
gone = {x["bom-ref"] for x in c if dsa(x)}
doc["components"] = [x for x in c if x["bom-ref"] not in gone]
doc["dependencies"] = [x for x in doc.get("dependencies", []) if x["ref"] not in gone]
doc["components"].append({"type": "cryptographic-asset", "bom-ref": "rc4", "name": "RC4",
    "cryptoProperties": {"assetType": "algorithm", "algorithmProperties": {"primitive": "stream-cipher"}},
    "evidence": {"occurrences": [{"location": "services/src/main/java/org/keycloak/keys/LegacyCipher.java", "line": 40}]}})
json.dump(doc, open(f"{projects}/keycloak-last-month/cbom.cdx.json", "w"), indent=2)
PY

# For the pictures of single features: two scripts for the F2 menu, a script to look at the
# properties of, a draft to delete, a report to move, budgets in two languages for search by
# meaning and Ask, holiday pictures to batch-rename (one name already taken), a photo with EXIF,
# a slide deck and a draw.io diagram with two pages.
mkdir -p "$d/.config/coxswain/scripts" "$d/Pictures/Holiday"
printf '#!/bin/sh\nfor f in "$@"; do magick "$f" -resize 400x "thumb-$(basename "$f")"; done\n' > "$d/.config/coxswain/scripts/Make thumbnails"
printf '#!/bin/sh\nrsync -a "$@" backup:pictures/\n' > "$d/.config/coxswain/scripts/Upload"
chmod +x "$d/.config/coxswain/scripts/"*
printf '#!/bin/sh\n# Deploy the site\nrsync -a --delete dist/ web:/srv/rocket/\n' > "$d/projects/rocket/deploy.sh" && chmod 755 "$d/projects/rocket/deploy.sh"
cp "$d/Documents/debrief.docx" "$d/Documents/old-draft.docx"
cp "$d/Documents/launch-report.pdf" "$d/Documents/report.pdf"
printf 'Fuel budget 2026\n\nThe rocket fuel costs 2,105 kEUR in April and 2,655 kEUR in June: fuel is the largest cost of each launch.\n' > "$d/Documents/budget.txt"
printf 'Brændstofbudget 2026\n\nRaketbrændstoffet koster 2.105 kEUR i april og 2.655 kEUR i juni: brændstof er den største udgift ved hver opsendelse.\n' > "$d/Documents/budget-da.txt"
for i in $(seq -w 1 12); do magick -size 600x400 -seed "$i" plasma:'#8ecae6'-'#023047' "$d/Pictures/Holiday/IMG_00$i.jpg"; done
cp "$d/Pictures/Holiday/IMG_0003.jpg" "$d/Pictures/Holiday/Holiday-0004.jpg"
cat > "$d/projects/paper/launch-pad.drawio" <<'EOF'
<mxfile host="Coxswain demo"><diagram id="p1" name="Pad"><mxGraphModel dx="800" dy="600" grid="1" gridSize="10"><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="Fuel tank" style="shape=cylinder3;whiteSpace=wrap;html=1;fillColor=#dae8fc;strokeColor=#6c8ebf;" vertex="1" parent="1"><mxGeometry x="40" y="60" width="90" height="110" as="geometry"/></mxCell><mxCell id="3" value="Valve" style="rhombus;whiteSpace=wrap;html=1;fillColor=#fff2cc;strokeColor=#d6b656;" vertex="1" parent="1"><mxGeometry x="200" y="80" width="80" height="70" as="geometry"/></mxCell><mxCell id="4" value="Engine" style="rounded=1;whiteSpace=wrap;html=1;fillColor=#f8cecc;strokeColor=#b85450;" vertex="1" parent="1"><mxGeometry x="350" y="85" width="110" height="60" as="geometry"/></mxCell><mxCell id="5" edge="1" parent="1" source="2" target="3" style="endArrow=classic;html=1;"><mxGeometry relative="1" as="geometry"/></mxCell><mxCell id="6" edge="1" parent="1" source="3" target="4" style="endArrow=classic;html=1;"><mxGeometry relative="1" as="geometry"/></mxCell></root></mxGraphModel></diagram><diagram id="p2" name="Countdown"><mxGraphModel dx="800" dy="600" grid="1" gridSize="10"><root><mxCell id="0"/><mxCell id="1" parent="0"/><mxCell id="2" value="T-10 Hold?" style="rounded=1;whiteSpace=wrap;html=1;" vertex="1" parent="1"><mxGeometry x="40" y="40" width="120" height="60" as="geometry"/></mxCell><mxCell id="3" value="Lift-off" style="ellipse;whiteSpace=wrap;html=1;fillColor=#d5e8d4;" vertex="1" parent="1"><mxGeometry x="240" y="40" width="120" height="60" as="geometry"/></mxCell><mxCell id="4" edge="1" parent="1" source="2" target="3" style="endArrow=classic;html=1;"><mxGeometry relative="1" as="geometry"/></mxCell></root></mxGraphModel></diagram></mxfile>
EOF
# The photo's EXIF and the deck need Pillow and python-pptx (through uv; left out without it).
if command -v uv >/dev/null; then
  (cd "$d" && uv run -q --with pillow --with python-pptx python "$here/demo-extras.py") || true
fi
# A web page saved from the web: its own stylesheet beside it, its pictures still on the web.
printf 'body { font-family: serif; margin: 2rem; background: #fdf6e3; color: #333; }\nh1 { color: #b58900; }\nimg { width: 240px; height: 140px; border: 1px solid #ccc; }\n' > "$d/Downloads/launch-news.css"
cat > "$d/Downloads/launch-news.html" <<'EOF'
<!doctype html>
<title>Launch news</title>
<link rel="stylesheet" href="launch-news.css">
<h1>Flight 7 lifts off</h1>
<p>The rocket left the pad at 08:00 and flew for 92 seconds.</p>
<img src="https://example.com/images/liftoff.jpg" alt="Lift-off"> <img src="https://example.com/images/crowd.jpg" alt="The crowd">
<p>Pictures: the launch team.</p>
EOF
