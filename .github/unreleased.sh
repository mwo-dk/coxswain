#!/usr/bin/env bash
# Fails, naming it, when the version in Cargo.toml has no published GitHub release and no other
# Release run is under way that could still publish it: the bump's release run failed, and later
# pushes only test (release.yml), so nothing else would say so. Run by release.yml on a push that
# did not bump the version, and by nightly.yml. Needs GH_TOKEN and REPO; run from the root.
# The fix: finish that release (fix what failed, then `gh workflow run release.yml` on master).
set -euo pipefail
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
tag="v$version"
if out=$(gh release view "$tag" -R "$REPO" --json isDraft -q .isDraft 2>&1); then
  if [ "$out" = false ]; then echo "$tag is released."; exit 0; fi
elif ! grep -q "release not found" <<<"$out"; then
  echo "$out" >&2; exit 1
fi
# A run that may still publish it: one started by hand, or the bump's push (the release pull
# request's merge, titled "Release X.Y.Z"). Other pushes' runs only test.
others=$(gh run list -R "$REPO" --workflow release.yml --limit 100 --json databaseId,status,event,displayTitle \
  -q "[.[] | select(.status != \"completed\" and .databaseId != ${GITHUB_RUN_ID:-0}
        and (.event == \"workflow_dispatch\" or (.displayTitle + \" \" | startswith(\"Release $version \"))))] | length")
if [ "$others" -gt 0 ]; then
  echo "::notice title=$tag not released yet::a Release run that may still publish it is under way."
  exit 0
fi
echo "::error title=$tag is not released::$tag, the version in Cargo.toml, has no published GitHub release and no Release run is under way for it. Fix what stopped its release run, then start Release by hand on master (gh workflow run release.yml). Do not bump past it: release-pr.py refuses."
exit 1
