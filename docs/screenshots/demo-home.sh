#!/usr/bin/env bash
# shellcheck disable=SC2016 # backticks below are markdown, not command substitution
# Build an anonymous demo home folder for screenshots: a few documents and two git
# repositories, one clean and one with every kind of change the git glyphs show.
#
#   docs/screenshots/demo-home.sh DIR
set -euo pipefail
d="${1:?usage: demo-home.sh DIR}"
rm -rf "$d"
mkdir -p "$d"/{Desktop,Documents,Downloads,Music,Pictures,Videos,projects/rocket/src,projects/website}
cd "$d"

g() {
  GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_NOSYSTEM=1 git -c user.name="Demo User" \
    -c user.email=demo@example.com -c commit.gpgsign=false -c init.defaultBranch=master "$@"
}

truncate -s 40K Documents/budget.xlsx
truncate -s 12K Documents/letter.odt
printf '# Notes\n\n- Book the ferry\n- Renew the domain\n- Try `coxswain` on the laptop\n' > Documents/notes.md
truncate -s 5M Downloads/setup.iso
truncate -s 800K Downloads/wallpaper.jpg
truncate -s 2M Pictures/holiday.jpg

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

g init -q --bare "$d/.remotes/rocket.git"
g remote add origin "$d/.remotes/rocket.git"
g push -q -u origin master
printf '\n## Build\n\n    cargo build --release\n' >> README.md && g commit -qam "Document the build"

echo "Launch window: Tuesday" >> README.md && g stash -q
sed -i 's/30.0/45.0/' src/main.rs
printf 'pub const TANK: f64 = 45.0;\n' > src/fuel.rs && g add src/fuel.rs
printf 'Add a landing burn\n' > TODO.txt
