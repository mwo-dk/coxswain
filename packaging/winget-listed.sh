#!/usr/bin/env bash
# Whether a package is on WinGet (microsoft/winget-pkgs), and whether a version of it has its
# pull request there, open or merged. Used by release.yml and release-check.yml.
#
#   packaging/winget-listed.sh mwo-dk.Coxswain.Terminal          listed at all?
#   packaging/winget-listed.sh mwo-dk.Coxswain.Terminal 2.11.0   a pull request for 2.11.0?
#
# Exits 0 for yes, 1 for no, 2 when GitHub did not answer (never taken for a no).
set -uo pipefail
id="$1" v="${2:-}"
if [ -z "$v" ]; then
  # A version folder (a name starting with a digit) in the package's manifest folder.
  first=$(printf %s "${id:0:1}" | tr '[:upper:]' '[:lower:]')
  path="manifests/$first/${id//.//}"
  if out=$(gh api "repos/microsoft/winget-pkgs/contents/$path" -q '.[].name' 2>&1); then
    grep -q '^[0-9]' <<<"$out" && exit 0
    exit 1
  fi
  grep -q 'HTTP 404' <<<"$out" && exit 1
  echo "$out" >&2; exit 2
fi
titles=$(gh pr list -R microsoft/winget-pkgs --state all --search "$id version $v in:title" \
  --json title,state -q '.[] | select(.state != "CLOSED") | .title') || exit 2
grep -qxE "(New|Add|Update) version: ${id//./\\.} version ${v//./\\.}" <<<"$titles"
