#!/bin/sh
# Flathub's copy of the Flatpak for release $1 (v2.2.0), written into folder $2 (a clone of
# github.com/flathub/io.github.mwo_dk.Coxswain): the manifest building that tag, and the offline
# sources from that tag's lock files. Needs git and python3. Used for the first submission
# (SUBMIT.md) and by release.yml for every release after it.
set -eu
tag=$1
out=$(cd "$2" && pwd)
here=$(cd "$(dirname "$0")" && pwd)
repo=https://github.com/mwo-dk/coxswain.git
# A tag made by `gh release create` is a plain one; an annotated tag's commit is under ^{}.
commit=$(git ls-remote "$repo" "refs/tags/$tag^{}" "refs/tags/$tag" | sort -k2 | tail -1 | cut -f1)
[ -n "$commit" ] || { echo "flathub.sh: no tag $tag in $repo" >&2; exit 1; }
src=$(mktemp -d)
trap 'rm -rf "$src"' EXIT
git clone -q --depth 1 --branch "$tag" "$repo" "$src"
manifest=io.github.mwo_dk.Coxswain.yml
{
  echo "# Written by packaging/flatpak/flathub.sh in github.com/mwo-dk/coxswain for $tag: change it there."
  awk -v tag="$tag" -v commit="$commit" -v repo="$repo" '
    NR == 1, /^id:/ { if (!/^id:/) next }
    /# source:/ { print "      - type: git"; print "        url: " repo; print "        tag: " tag; print "        commit: " commit; skip = 1; next }
    /# end source/ { skip = 0; next }
    !skip
  ' "$src/packaging/flatpak/$manifest"
} > "$out/$manifest"
sh "$here/sources.sh" "$src" "$out"
echo "flathub.sh: $out has $manifest for $tag ($commit), cargo-sources.json and node-sources.json"
