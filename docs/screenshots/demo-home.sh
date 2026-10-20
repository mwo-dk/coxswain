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
# Local model stores (Ollama, Hugging Face, LM Studio): real headers, sparse weights.
python3 "$here/demo-local-models.py" "$d"
printf '# Notes\n\n- Book the ferry\n- Renew the domain\n- Try `coxswain` on the laptop\n' > Documents/notes.md
# A real ISO with a few files, for the disk image preview; an empty one without bsdtar.
if command -v bsdtar >/dev/null; then
  iso="$(mktemp -d)"
  mkdir -p "$iso/docs"
  printf 'Setup 1.0\n\nRun install.sh, or read docs/install.md.\n' > "$iso/README.txt"
  printf '#!/bin/sh\necho Installing...\n' > "$iso/install.sh"
  printf '# Installing\n\n1. Mount nothing: Coxswain reads it as it is.\n' > "$iso/docs/install.md"
  bsdtar --format=iso9660 --options 'iso9660:volume-id=SETUP_1_0,iso9660:rockridge' -cf Downloads/setup.iso -C "$iso" .
  rm -rf "$iso"
else
  truncate -s 5M Downloads/setup.iso
fi
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
# A fetch, for the last fetch the git line's tooltip and the info line show.
g fetch -q origin
# The bare repository's real path would show as the remote: a URL of the kind a project has.
g remote set-url origin https://git.example.org/demo/rocket.git
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
mkdir -p "$d/.config/coxswain" && printf 'check_updates = false\n\n# The name index of the demo home only: / in the sandbox is the host'"'"'s root.\n[search]\nname_roots = ["/home/demo"]\n' > "$d/.config/coxswain/config.toml"
# A git config of the demo user's own, for the Git config view (Alt+C): an include, a password
# it hides, and an email the repository sets again.
printf '[user]\n\tname = Demo User\n\temail = demo@example.com\n[init]\n\tdefaultBranch = master\n[sendemail]\n\tsmtpServer = smtp.example.com\n\tsmtpPass = launch-day-42\n[include]\n\tpath = .gitconfig-aliases\n' > "$d/.gitconfig"
printf '[alias]\n\tst = status --short\n\tlg = log --oneline --graph\n' > "$d/.gitconfig-aliases"
GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -C "$d/projects/rocket" config user.email demo@rocket.example.org
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
# Old files: an Amiga IFF picture, a comic of the pictures above, a dBase table, a tracker
# module and a mailbox (Python's own library only).
(cd "$d" && python3 "$here/demo-old.py")
# Neural network models: ONNX, TensorFlow Lite, Core ML (a file and a package) and Keras.
python3 "$here/demo-models.py" "$d/projects/vision"
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
# Old languages, for the highlighted previews and the terminal app's viewer.
mkdir -p "$d/projects/legacy"
cat > "$d/projects/legacy/PAYROLL.cbl" <<'EOF'
      *> Payroll for the launch crew, as it ran in 1979.
       IDENTIFICATION DIVISION.
       PROGRAM-ID. PAYROLL.
       DATA DIVISION.
       WORKING-STORAGE SECTION.
       01  CREW-COUNT        PIC 9(3)    VALUE 12.
       01  HOURLY-RATE       PIC 9(3)V99 VALUE 18.50.
       01  HOURS             PIC 9(3)    VALUE 40.
       01  WEEKLY-PAY        PIC 9(6)V99.
       PROCEDURE DIVISION.
       MAIN-PARA.
           MULTIPLY HOURLY-RATE BY HOURS GIVING WEEKLY-PAY.
           DISPLAY "CREW: " CREW-COUNT.
           DISPLAY "WEEKLY PAY EACH: " WEEKLY-PAY.
           IF WEEKLY-PAY > 500
               DISPLAY "OVERTIME REVIEW"
           END-IF.
           STOP RUN.
EOF
cat > "$d/projects/legacy/orbit.f90" <<'EOF'
! Orbital period of a circular orbit, in minutes.
program orbit
  implicit none
  real, parameter :: mu = 398600.4418   ! km^3/s^2, the Earth
  real :: radius, period
  integer :: i
  do i = 1, 5
    radius = 6371.0 + 200.0 * i
    period = 2.0 * 3.14159265 * sqrt(radius**3 / mu) / 60.0
    print '(A, F8.1, A, F6.1, A)', 'r = ', radius, ' km: ', period, ' min'
  end do
end program orbit
EOF
# Chess: three classics of the 1850s (public domain), one with comments and a variation, and a
# few positions, for Chess games (docs/previews/chess.md).
mkdir -p "$d/Documents/Chess"
cat > "$d/Documents/Chess/classics.pgn" <<'EOF'
[Event "Paris"]
[Site "Paris FRA"]
[Date "1858.11.02"]
[Round "?"]
[White "Morphy, Paul"]
[Black "Duke Karl / Count Isouard"]
[Result "1-0"]
[ECO "C41"]

1.e4 e5 2.Nf3 d6 3.d4 Bg4 {A weak move already: the bishop is given for a knight.} 4.dxe5
Bxf3 5.Qxf3 dxe5 6.Bc4 Nf6 7.Qb3 Qe7 8.Nc3 c6 9.Bg5 b5 $2 10.Nxb5! cxb5 11.Bxb5+ Nbd7
12.O-O-O Rd8 13.Rxd7! Rxd7 14.Rd1 Qe6 15.Bxd7+ Nxd7 (15...Qxd7 16.Qb8+ Ke7 17.Qxe5+ Kd8
18.Bxf6+ gxf6 19.Qxf6+ $18) 16.Qb8+!! Nxb8 17.Rd8# {The Opera Game.} 1-0

[Event "London"]
[Site "London ENG"]
[Date "1851.06.21"]
[Round "?"]
[White "Anderssen, Adolf"]
[Black "Kieseritzky, Lionel"]
[Result "1-0"]
[ECO "C33"]

1.e4 e5 2.f4 exf4 3.Bc4 Qh4+ 4.Kf1 b5 5.Bxb5 Nf6 6.Nf3 Qh6 7.d3 Nh5 8.Nh4 Qg5 9.Nf5 c6
10.g4 Nf6 11.Rg1 cxb5 12.h4 Qg6 13.h5 Qg5 14.Qf3 Ng8 15.Bxf4 Qf6 16.Nc3 Bc5 17.Nd5 Qxb2
18.Bd6 Bxg1 19.e5 Qxa1+ 20.Ke2 Na6 21.Nxg7+ Kd8 22.Qf6+ Nxf6 23.Be7# 1-0

[Event "Berlin"]
[Site "Berlin GER"]
[Date "1852.??.??"]
[Round "?"]
[White "Anderssen, Adolf"]
[Black "Dufresne, Jean"]
[Result "1-0"]
[ECO "C52"]

1.e4 e5 2.Nf3 Nc6 3.Bc4 Bc5 4.b4 Bxb4 5.c3 Ba5 6.d4 exd4 7.O-O d3 8.Qb3 Qf6 9.e5 Qg6
10.Re1 Nge7 11.Ba3 b5 12.Qxb5 Rb8 13.Qa4 Bb6 14.Nbd2 Bb7 15.Ne4 Qf5 16.Bxd3 Qh5
17.Nf6+ gxf6 18.exf6 Rg8 19.Rad1 Qxf3 20.Rxe7+ Nxe7 21.Qxd7+ Kxd7 22.Bf5+ Ke8 23.Bd7+ Kf8
24.Bxe7# 1-0
EOF
printf '%s\n' 'r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - bm Qxf7#; id "Scholar'"'"'s mate";' \
  '6k1/5ppp/8/8/8/8/5PPP/3R2K1 w - - bm Rd8#; id "Back rank";' > "$d/Documents/Chess/mates.epd"
# A tiny Polyglot opening book made for the demo (four positions, weights made up), for Opening
# books (docs/previews/chess.md#opening-books): next to the games, so their board shows its moves.
printf '\x08\x44\x93\x1a\x6e\xf4\xb9\xa0\x01\x5a\x00\x0a\x00\x00\x00\x00\x08\x44\x93\x1a\x6e\xf4\xb9\xa0\x01\x95\x00\x5a\x00\x00\x00\x00\x46\x3b\x96\x18\x16\x91\xfc\x9c\x01\x95\x00\x0c\x00\x00\x00\x00\x46\x3b\x96\x18\x16\x91\xfc\x9c\x02\x9a\x00\x08\x00\x00\x00\x00\x46\x3b\x96\x18\x16\x91\xfc\x9c\x02\xdb\x00\x23\x00\x00\x00\x00\x46\x3b\x96\x18\x16\x91\xfc\x9c\x03\x1c\x00\x2d\x00\x00\x00\x00\x82\x3c\x9b\x50\xfd\x11\x41\x96\x0c\xa2\x00\x2d\x00\x00\x00\x00\x82\x3c\x9b\x50\xfd\x11\x41\x96\x0c\xaa\x00\x05\x00\x00\x00\x00\x82\x3c\x9b\x50\xfd\x11\x41\x96\x0d\x24\x00\x28\x00\x00\x00\x00\x82\x3c\x9b\x50\xfd\x11\x41\x96\x0d\x2c\x00\x0a\x00\x00\x00\x00\x83\x0e\xb9\xb2\x07\x58\xd1\xde\x0c\xe3\x00\x32\x00\x00\x00\x00\x83\x0e\xb9\xb2\x07\x58\xd1\xde\x0f\xad\x00\x32\x00\x00\x00\x00' > "$d/Documents/Chess/openings.bin"
# Go: the ear-reddening game of 1846 (public domain; its moves as sgf-render's tests have them,
# MIT), a 9×9 study in variations and a life-and-death problem from the same tests, for Go
# games (docs/previews/go.md).
mkdir -p "$d/Documents/Go"
cat > "$d/Documents/Go/classics.sgf" <<'EOF'
(;GM[1]FF[4]CA[UTF-8]SZ[19]GN[The ear-reddening game]PB[Honinbo Shusaku]BR[4d]PW[Gennan Inseki]WR[8d]DT[1846]KM[0]RE[B+2]
;B[qd];W[dc];B[pq];W[oc];B[cp];W[cf];B[ep];W[qo];B[pe];W[np]
;B[po];W[pp];B[op];W[qp];B[oq];W[oo];B[pn];W[qq];B[nq];W[on]
;B[pm];W[om];B[pl];W[mp];B[mq];W[ol];B[pk];W[lq];B[lr];W[kr]
;B[lp];W[kq];B[qr];W[rr];B[rs];W[mr];B[nr];W[pr];B[ps];W[qs]
;B[no];W[mo];B[qr];W[rm];B[rl];W[qs];B[lo];W[mn];B[qr];W[qm]
;B[or];W[ql];B[qj];W[rj];B[ri];W[rk];B[ln];W[mm];B[qi];W[rq]
;B[jn];W[ls];B[ns];W[gq];B[go];W[ck];B[kc];W[ic];B[pc];W[nj]
;B[ke];W[og];B[oh];W[pb];B[qb];W[ng];B[mi];W[mj];B[nd];W[ph]
;B[qg];W[pg];B[hq];W[hr];B[ir];W[iq];B[hp];W[jr];B[fc];W[lc]
;B[ld];W[mc];B[lb];W[mb];B[md];W[qf];B[pf];W[qh];B[rg];W[rh]
;B[sh];W[rf];B[sg];W[pj];B[pi];W[oi];B[oj];W[ni];B[qk];W[ok]
;B[qe];W[kb];B[jb];W[ka];B[jc];W[ob];B[ja];W[la];B[db];W[cc]
;B[fe];W[cn];B[gr];W[is];B[fq];W[io];B[ji]C[The ear-reddening move. A physician watching the game saw Gennan's ears turn red, and knew he was shaken.];W[eb];B[fb];W[eg]
;B[dj];W[dk];B[ej];W[cj];B[dh];W[ij];B[hm];W[gj];B[eh];W[fl]
;B[fg];W[er];B[dm];W[fn];B[dn];W[gn];B[jj];W[jk];B[kk];W[ii]
;B[ik];W[jl];B[kl];W[il];B[jh];W[co];B[do];W[ih];B[hn];W[hl]
;B[bl];W[dg];B[gh];W[ch];B[ig];W[ec];B[cr];W[fd];B[gd];W[ed]
;B[gc];W[bk];B[cm];W[gs];B[gp];W[li];B[kg];W[in];B[lj];W[lg]
;B[gm];W[jf];B[jg];W[im];B[fm];W[kf];B[lf];W[mf];B[le];W[gf]
;B[hf];W[ff];B[gg];W[lk];B[kj];W[km];B[lm];W[ll];B[jm];W[ge]
;B[he];W[ef];B[ea];W[cb];B[fr];W[fs];B[dr];W[qa];B[ra];W[pa]
;B[rb];W[da];B[gi];W[fj];B[fi];W[fa];B[ga];W[gl];B[ek];W[em]
;B[ho];W[el];B[en];W[jo];B[kn];W[ci];B[lh];W[mh];B[mg];W[di]
;B[ei];W[lg];B[qn];W[rn];B[re];W[sl];B[mg];W[bm];B[am];W[lg]
;B[eq];W[es];B[mg];W[ha];B[gb];W[lg];B[ds];W[hs];B[mg];W[sj]
;B[si];W[lg];B[sr];W[sq];B[mg];W[hd];B[hb];W[lg];B[ro];W[so]
;B[mg];W[ss];B[qs];W[lg];B[sn];W[rp];B[mg];W[cl];B[bn];W[lg]
;B[ml];W[mk];B[mg];W[pj];B[sf];W[lg];B[nn];W[nl];B[mg];W[ib]
;B[ia];W[lg];B[nc];W[nb];B[mg];W[jd];B[kd];W[lg];B[ma];W[na]
;B[mg];W[qc];B[rc];W[lg];B[js];W[ks];B[mg];W[hc];B[id];W[lg]
;B[fk];W[hj];B[mg];W[hh];B[hg];W[lg];B[gk];W[hk];B[mg];W[ak]
;B[lg];W[al];B[bm];W[nf];B[od];W[ki];B[ms];W[kp];B[ip];W[jp]
;B[lr];W[oj];B[mr];W[ea];B[sr])

(;GM[1]FF[4]
CA[UTF-8]
SZ[9]
KM[6.5]
GN[A study in variations]
;B[ee];W[ec]
(;B[eg];W[eh];B[dh]
(;W[cd];B[gd];W[fh])
(;W[gd];B[fh]))
(;B[fc];W[fb]
(;B[fd]
(;W[gb]
(;B[eg])
(;B[dg])
(;B[ce]))
(;W[eg]
(;B[gb])
(;B[fg];W[fh])))
(;B[gc])
(;B[eg]))
)

(;GM[1]FF[4]SZ[19]HA[0]KM[0]GN[Life and death, elementary]AB[ab][bb][db][eb][cc][cd]AW[fa][cb][fb][dc][ec][fc][ad][dd][be][ce]C[Elementary](;B[ca];W[ea];B[bd];W[ac];B[da]C[Correct.])(;B[da]WV[];W[bc];B[ca]C[Wrong.])(;B[bc]WV[];W[ea];B[ca];W[da]C[Wrong.]))
EOF
# Geodata, for the maps and facts (docs/previews/geodata.md): a made-up launch range by the sea
# as GeoJSON (its area, the road and the pads) and a walk round it as GPX, with times.
mkdir -p "$d/Documents/Maps"
cat > "$d/Documents/Maps/launch-range.geojson" <<'EOF'
{"type":"FeatureCollection","name":"launch-range","features":[
{"type":"Feature","properties":{"name":"Range","kind":"area"},"geometry":{"type":"Polygon","coordinates":[[[16.000,69.280],[16.030,69.276],[16.052,69.285],[16.058,69.300],[16.040,69.312],[16.012,69.310],[15.995,69.297],[16.000,69.280]],[[16.020,69.292],[16.030,69.292],[16.030,69.298],[16.020,69.298],[16.020,69.292]]]}},
{"type":"Feature","properties":{"name":"Shore","kind":"water"},"geometry":{"type":"Polygon","coordinates":[[[16.058,69.270],[16.090,69.270],[16.090,69.320],[16.050,69.320],[16.062,69.300],[16.058,69.270]]]}},
{"type":"Feature","properties":{"name":"Range road","kind":"road"},"geometry":{"type":"LineString","coordinates":[[15.980,69.274],[15.998,69.283],[16.010,69.288],[16.024,69.290],[16.036,69.294],[16.046,69.302]]}},
{"type":"Feature","properties":{"name":"Pad A","kind":"pad"},"geometry":{"type":"Point","coordinates":[16.046,69.302]}},
{"type":"Feature","properties":{"name":"Pad B","kind":"pad"},"geometry":{"type":"Point","coordinates":[16.038,69.307]}},
{"type":"Feature","properties":{"name":"Control","kind":"building"},"geometry":{"type":"Point","coordinates":[16.010,69.288]}}]}
EOF
python3 - "$d/Documents/Maps/range-walk.gpx" <<'EOF'
import math, sys
pts = []
for i in range(48):
    a = i / 47 * 2 * math.pi
    pts.append((69.293 + 0.009 * math.sin(a) + 0.001 * math.sin(5 * a), 16.024 + 0.022 * math.cos(a)))
head = '<?xml version="1.0" encoding="UTF-8"?>\n<gpx version="1.1" creator="demo" xmlns="http://www.topografix.com/GPX/1/1">\n'
wpt = '<wpt lat="69.302" lon="16.046"><name>Pad A</name></wpt>\n'
trk = "".join(f'<trkpt lat="{la:.5f}" lon="{lo:.5f}"><time>2026-06-21T05:{i:02d}:00Z</time></trkpt>\n' for i, (la, lo) in enumerate(pts))
open(sys.argv[1], "w").write(head + wpt + '<trk><name>Round the range</name><trkseg>\n' + trk + "</trkseg></trk>\n</gpx>\n")
EOF
# An OpenSCAD model, for its picture and Export 3D model (docs/previews/openscad.md): the
# engine's bell on its mount, the bell's sizes in a file of their own.
mkdir -p "$d/Documents/Models"
printf '// The bell, in millimetres.\nthroat = 10;\nexit = 24;\nlength = 40;\nwall = 2;\n' > "$d/Documents/Models/sizes.scad"
cat > "$d/Documents/Models/engine-bell.scad" <<'EOF'
// The engine's bell on its mount, six bolt holes round it.
include <sizes.scad>
$fn = 96;

module bell() {
  difference() {
    cylinder(h = length, r1 = throat + wall, r2 = exit + wall);
    translate([0, 0, -1]) cylinder(h = length + 2, r1 = throat, r2 = exit);
  }
}

module mount() {
  difference() {
    cylinder(h = 8, r = throat + 8);
    translate([0, 0, -1]) cylinder(h = 10, r = throat);
    for (a = [0 : 60 : 300]) rotate(a) translate([throat + 5, 0, -1]) cylinder(h = 10, r = 1.6);
  }
}

mount();
translate([0, 0, 8]) bell();
EOF
# For Space (Alt+U): sizes of every kind, so the treemap has something to show. Sparse files:
# they take no room on the disk, but their length is what Space and the folder sizes count.
mkdir -p "$d/Videos/Flight 7" "$d/Music/Night Drive" "$d/Music/Podcasts" "$d/Pictures/Raw" "$d/VMs" "$d/Documents/Scans" "$d/projects/rocket/target/debug/deps"
truncate -s 1800M "$d/Videos/Flight 7/launch.mp4"
truncate -s 640M "$d/Videos/Flight 7/onboard-camera.mov"
truncate -s 410M "$d/Videos/countdown.mkv"
truncate -s 120M "$d/Videos/pad-tour.webm"
for i in 1 2 3 4 5 6 7 8; do truncate -s "$((28 + i * 3))M" "$d/Music/Night Drive/0$i Track $i.flac"; done
for i in 1 2 3 4 5; do truncate -s "$((50 + i * 7))M" "$d/Music/Podcasts/episode-$i.mp3"; done
for i in $(seq -w 1 24); do truncate -s "$((22 + 10#$i % 7))M" "$d/Pictures/Raw/DSC_00$i.nef"; done
truncate -s 3100M "$d/VMs/freebsd-15.qcow2"
truncate -s 1300M "$d/Downloads/installer.iso"
truncate -s 760M "$d/Backups/old-laptop-2019/photos.zip"
truncate -s 290M "$d/Backups/site-2026.tar.zst"
for n in tax-2025 lease insurance passport; do truncate -s "$(( ${#n} * 2 ))M" "$d/Documents/Scans/$n.pdf"; done
for n in libserde libtokio libregex librocket; do truncate -s "$(( ${#n} * 9 ))M" "$d/projects/rocket/target/debug/deps/$n.rlib"; done
truncate -s 85M "$d/Documents/fuel-log.sqlite"
# Each starts with its own name, so none is taken for a duplicate of another (still sparse).
for f in "$d/Videos/Flight 7"/* "$d/Videos/countdown.mkv" "$d/Videos/pad-tour.webm" "$d/Music/Night Drive"/* "$d/Music/Podcasts"/* \
  "$d/Pictures/Raw"/* "$d/VMs/freebsd-15.qcow2" "$d/Downloads/installer.iso" "$d/Backups/old-laptop-2019/photos.zip" \
  "$d/Backups/site-2026.tar.zst" "$d/Documents/Scans"/* "$d/projects/rocket/target/debug/deps"/* "$d/Documents/fuel-log.sqlite"; do
  printf '%s' "${f#"$d"/}" | dd of="$f" conv=notrunc status=none
done

# An e-book calibre reads (FictionBook is XML, so it is written here), with a cover, for the
# e-book previews (docs/previews/ebooks.md).
mkdir -p "$d/Documents/Books"
magick -size 600x900 gradient:'#0b2545'-'#13315c' -font "$(fc-match -f '%{file}' sans:bold)" -fill '#eef4ed' -pointsize 64 -gravity north \
  -annotate +0+260 'The Rocket' -annotate +0+340 'Garden' -fill '#f4a259' -pointsize 34 -annotate +0+700 'Ada Lovelace' "$d/Documents/Books/cover.png"
python3 - "$d/Documents/Books" <<'PY'
import base64, sys
d = sys.argv[1]
cover = base64.b64encode(open(f"{d}/cover.png", "rb").read()).decode()
chapters = [
    ("Launch window", "The rocket garden lay quiet before the launch window opened. Fuel budgets were checked twice, and the tower crew walked the gantry one last time while the sun came up behind the pad. Nobody spoke much; the countdown clock did the talking, one red second after another, and the engineers watched their screens for the small numbers that mattered."),
    ("Orbit", "In orbit the crew counted stars and wrote letters home about the blue planet below, its oceans bright in the morning light. The capsule turned slowly, and every ninety minutes the sun rose again over a horizon thin as paper."),
    ("Return", "The capsule came home over the sea, its parachutes opening like flowers above the waves. The recovery ship was already there, and the first thing the crew asked for was a cup of real coffee."),
]
body = "".join(f"<section><title><p>Chapter {i + 1}: {t}</p></title><p>{p}</p></section>" for i, (t, p) in enumerate(chapters))
open(f"{d}/the-rocket-garden.fb2", "w").write(f"""<?xml version="1.0" encoding="utf-8"?>
<FictionBook xmlns="http://www.gribuser.ru/xml/fictionbook/2.0" xmlns:l="http://www.w3.org/1999/xlink">
<description><title-info><genre>sf</genre><author><first-name>Ada</first-name><last-name>Lovelace</last-name></author><book-title>The Rocket Garden</book-title><coverpage><image l:href="#cover.png"/></coverpage><lang>en</lang></title-info></description>
<body>{body}</body>
<binary id="cover.png" content-type="image/png">{cover}</binary>
</FictionBook>
""")
PY
rm "$d/Documents/Books/cover.png"
