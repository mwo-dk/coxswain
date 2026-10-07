#!/usr/bin/env bash
# The files every release carries: what release.yml's build jobs make, matrix entry by matrix
# entry. Before publishing, release.yml's `verify` job checks the draft against this list, and
# release-check.yml checks the published release: a file missing or one too many fails both, so
# a new matrix entry needs its line here.
#
#   packaging/release-assets.sh v2.11.0          the list
#   packaging/release-assets.sh v2.11.0 <dir>    check the downloads in <dir>: exactly the list
#                                                (and SHA256SUMS, when there), each .sha256 and
#                                                SHA256SUMS matching, SHA256SUMS naming every file
set -euo pipefail
export LC_ALL=C
tag="$1" v="${1#v}"
[[ "$tag" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || { echo "::error::Not a release tag: $tag"; exit 1; }

expected() {
  # desktop: tauri-action, one line per matrix entry
  echo "Coxswain_${v}_amd64.deb" "Coxswain_${v}_amd64.AppImage" "Coxswain-${v}-1.x86_64.rpm"          # ubuntu-22.04
  echo "Coxswain_${v}_arm64.deb" "Coxswain_${v}_aarch64.AppImage" "Coxswain-${v}-1.aarch64.rpm"       # ubuntu-22.04-arm
  echo "Coxswain_${v}_aarch64.dmg" "Coxswain_${v}_aarch64.app.tar.gz"                                 # macOS, aarch64
  echo "Coxswain_${v}_x64.dmg" "Coxswain_${v}_x64.app.tar.gz"                                         # macOS, x86_64
  echo "Coxswain_${v}_x64-setup.exe" "Coxswain_${v}_x64_en-US.msi"                                    # windows-latest
  echo "Coxswain_${v}_arm64-setup.exe" "Coxswain_${v}_arm64_en-US.msi"                                # windows-11-arm
  # terminal, freebsd and other-unix: the terminal app, each archive with its .sha256
  for t in x86_64-unknown-freebsd x86_64-unknown-netbsd x86_64-unknown-openbsd x86_64-unknown-illumos \
    x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin x86_64-apple-darwin; do
    echo "coxswain-terminal-$tag-$t.tar.gz" "coxswain-terminal-$tag-$t.tar.gz.sha256"
  done
  for t in x86_64-pc-windows-msvc aarch64-pc-windows-msvc; do
    echo "coxswain-terminal-$tag-$t.zip" "coxswain-terminal-$tag-$t.zip.sha256"
  done
  # freebsd and other-unix (OpenBSD): the desktop app
  for t in x86_64-unknown-freebsd x86_64-unknown-openbsd; do
    echo "coxswain-desktop-$tag-$t.tar.gz" "coxswain-desktop-$tag-$t.tar.gz.sha256"
  done
  # boms
  echo "coxswain-$v.cbom.cdx.json" "coxswain-desktop-$v.sbom.cdx.json" "coxswain-terminal-$v.sbom.cdx.json" THIRD-PARTY-NOTICES.md
}
list() { expected | tr ' ' '\n' | sort; }

if [ $# -lt 2 ]; then list; exit 0; fi

cd "$2"
bad=0
fail() { echo "::error title=$tag is not complete::$*"; bad=1; }
have=$(ls -A | grep -vx SHA256SUMS | sort)
missing=$(comm -23 <(list) <(printf '%s\n' "$have") | xargs)
extra=$(comm -13 <(list) <(printf '%s\n' "$have") | xargs)
[ -z "$missing" ] || fail "missing: $missing"
[ -z "$extra" ] || fail "not expected: $extra"
for s in *.sha256; do
  [ -f "$s" ] || continue
  f="${s%.sha256}"
  [ -f "$f" ] || continue
  [ "$(awk '{print $1; exit}' "$s")" = "$(sha256sum "$f" | awk '{print $1}')" ] || fail "$s does not match $f"
done
for f in $have; do [ -s "$f" ] || fail "$f is empty"; done
if [ -f SHA256SUMS ]; then
  sha256sum --quiet -c SHA256SUMS || fail "SHA256SUMS does not match the files"
  unlisted=$(comm -23 <(printf '%s\n' "$have") <(awk '{sub(/^\*/, "", $2); print $2}' SHA256SUMS | sort) | xargs)
  [ -z "$unlisted" ] || fail "not in SHA256SUMS: $unlisted"
fi
[ "$bad" = 0 ] && echo "$tag: all $(list | wc -l) files, their checksums match."
exit "$bad"
