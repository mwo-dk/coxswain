#!/usr/bin/env bash
# Fails at once unless the draft this Release run made (id $ID, tag $TAG) is still there and is
# still this run's: its notes name this run (`<!-- release run N -->`, written by the draft job).
# Run by release.yml at the start of each job that builds into the draft, before each upload by
# tag, and before publishing. Without it, `gh run rerun --failed` of a build job after `discard`
# deleted the draft built for an hour and then failed, and one after another run had made a new
# draft for the same tag uploaded this run's files, from its own commit, into that one.
# Needs GH_TOKEN, REPO, TAG and ID; run from the root.
set -euo pipefail
again="start this run again in full (gh run rerun $GITHUB_RUN_ID, without --failed), or Release by hand (gh workflow run release.yml)"
q="[.draft, .tag_name, (.body // \"\" | contains(\"<!-- release run $GITHUB_RUN_ID -->\"))] | @tsv"
if ! line=$(bash packaging/retry.sh gh api "repos/$REPO/releases/$ID" -q "$q" | tail -1); then
  echo "::error title=No draft::the draft $TAG (id $ID) this run made is gone (a failed job's draft is deleted): $again."
  exit 1
fi
IFS=$'\t' read -r draft tag ours <<<"$line"
if [ "$draft" != true ] || [ "$tag" != "$TAG" ] || [ "$ours" != true ]; then
  echo "::error title=Not this run's draft::release $ID is draft=$draft, tag $tag, this run's: $ours. Expected this run's draft of $TAG: $again."
  exit 1
fi
echo "The draft $TAG (id $ID) is this run's."
