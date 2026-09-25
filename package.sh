#!/usr/bin/env bash
# Bundle the source and the install guide into coxswain-<version>.tar.gz and .zip, next to
# this script, ready to mail. Friends unpack either one and follow INSTALL.md.
#
# Packs the last commit (git archive), so build output and node_modules never go in.
set -euo pipefail
cd "$(dirname "$0")"

version="$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)"
name="coxswain-$version"

if [ -n "$(git status --porcelain)" ]; then
  echo "Note: you have uncommitted changes; they are NOT in the archives (only the last commit is)."
fi

# INSTALL.md also goes at the top level, so it is the first thing anyone sees.
for fmt in tar.gz zip; do
  git archive --format="$fmt" --prefix="$name/" --add-file=install/INSTALL.md -o "$name.$fmt" HEAD
done

du -h "$name.tar.gz" "$name.zip"
