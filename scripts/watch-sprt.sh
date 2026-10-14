#!/usr/bin/env bash
# Waits for the SPRT result that scripts/mac-sprt.sh posts to a PR, prints it and exits.
#
#   scripts/watch-sprt.sh <PR number> [hours]
#
# Checks the PR's comments every 10 minutes for the one marked <!-- sprt -->, for up to
# `hours` hours (default 24), then exits 1. Claude runs it in the background after Carlos says
# an SPRT has started on the test Mac (approved 2026-10-12); it needs only `gh`, logged in.
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,8p' "$0"
  exit 2
fi
PR="$1"
HOURS="${2:-24}"
for _ in $(seq 1 $(( HOURS * 6 ))); do
  body="$(gh pr view "$PR" --json comments \
    --jq '[.comments[] | select(.body | contains("<!-- sprt -->"))] | last | .body // empty' \
    2>/dev/null || true)"
  if [[ -n "$body" ]]; then
    echo "$body"
    exit 0
  fi
  sleep 600
done
echo "No SPRT result on #$PR after $HOURS hours: ask Carlos for \`tail -5 ~/sprt-$PR.log\` on the test Mac."
exit 1
