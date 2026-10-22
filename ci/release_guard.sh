#!/usr/bin/env bash
# Steps 3 and 5 of a release (Phase 3 spec §5): refuse unless this dispatched run belongs to
# the draft $TAG. A release runs on its tag; a dry run (v1.2.3-dryrun.N) runs on a branch
# (release_sign.py --ref, default main) that must still be the commit step 1 drafted.
set -euo pipefail
case "$TAG" in
  v[0-9]*.[0-9]*.[0-9]*-dryrun.[0-9]*) want=branch ;;
  v[0-9]*.[0-9]*.[0-9]*) want=tag ;;
  *) echo "not a release tag: $TAG" >&2; exit 1 ;;
esac
if [ "$GITHUB_REF_TYPE" != "$want" ]; then
  echo "$TAG must run on $([ "$want" = tag ] && echo "its tag" || echo "a branch"), not $GITHUB_REF" >&2
  exit 1
fi
if [ "$want" = tag ] && [ "$GITHUB_REF_NAME" != "$TAG" ]; then
  echo "this run is on $GITHUB_REF_NAME, not $TAG" >&2
  exit 1
fi
info=$(gh release view "$TAG" --json isDraft,targetCommitish --jq '"\(.isDraft) \(.targetCommitish)"')
if [ "${info%% *}" != true ]; then
  echo "$TAG is not a draft" >&2
  exit 1
fi
if [ "$want" = branch ] && [ "${info#* }" != "$GITHUB_SHA" ]; then
  echo "$GITHUB_REF_NAME moved since step 1 (${info#* } drafted, now $GITHUB_SHA); start a new dry run" >&2
  exit 1
fi
echo "release guard: $TAG on $GITHUB_REF at $GITHUB_SHA"
