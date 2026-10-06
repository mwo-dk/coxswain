#!/bin/sh
# cargo-sources.json and node-sources.json for the Flatpak, from the lock files of the
# repository at $1 (default: this checkout), written to $2 (default: this folder). Needs python3
# (with venv) and git. The generators are pinned to a commit of flatpak/flatpak-builder-tools.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
src=$(cd "${1:-$here/../..}" && pwd)
out=$(cd "${2:-$here}" && pwd)
tools=74697c75b630d7330e77250fc13cb5ea688d9479
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
git -C "$work" init -q tools
git -C "$work/tools" fetch -q --depth 1 https://github.com/flatpak/flatpak-builder-tools.git "$tools"
git -C "$work/tools" checkout -q FETCH_HEAD
python3 -m venv "$work/venv"
"$work/venv/bin/pip" install -q aiohttp tomlkit PyYAML "$work/tools/node"
"$work/venv/bin/python" "$work/tools/cargo/flatpak-cargo-generator.py" "$src/Cargo.lock" -o "$out/cargo-sources.json"
"$work/venv/bin/python" "$here/git-vendor.py" "$out/cargo-sources.json"
"$work/venv/bin/flatpak-node-generator" npm "$src/gui/package-lock.json" -o "$out/node-sources.json"
