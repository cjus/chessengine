#!/usr/bin/env bash
# Waits for the SPRT result that scripts/mac-sprt.sh posts to a PR, prints it and exits.
#
#   scripts/watch-sprt.sh <PR number> [hours] [undecided games]
#
# Checks the PR's comments every 10 minutes for the one marked <!-- sprt --> (the result, or
# "SPRT run failed" with the end of the log), for up to `hours` hours (default 24), then exits
# 1. If the progress comment (<!-- sprt-progress -->) shows `undecided games` games or more
# (default 8000; 0 turns this off) with no result yet, it prints that comment and exits 3: time
# to propose stopping the run. Claude runs it in the background after Carlos says an SPRT has
# started on the test Mac (approved 2026-10-12); it needs only `gh`, logged in.
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,11p' "$0"
  exit 2
fi
PR="$1"
HOURS="${2:-24}"
UNDECIDED="${3:-8000}"

# The body of the PR's last comment containing `marker`, or nothing.
last_comment() {
  gh pr view "$PR" --json comments \
    --jq "[.comments[] | select(.body | contains(\"$1\"))] | last | .body // empty" \
    2>/dev/null || true
}

for _ in $(seq 1 $(( HOURS * 6 ))); do
  body="$(last_comment '<!-- sprt -->')"
  if [[ -n "$body" ]]; then
    echo "$body"
    exit 0
  fi
  if (( UNDECIDED > 0 )); then
    progress="$(last_comment '<!-- sprt-progress -->')"
    # The score line reads "- 8000 games: +2000 -1950 =4050  Elo +2.2 ± 5.0  LLR 0.85".
    games="$(grep -oE '^- [0-9]+ games:' <<<"$progress" | grep -oE '[0-9]+' | tail -1 || true)"
    if [[ -n "$games" ]] && (( games >= UNDECIDED )); then
      echo "$progress"
      echo
      echo "Undecided after $games games (threshold $UNDECIDED): propose stopping the run to Carlos."
      exit 3
    fi
  fi
  sleep 600
done
echo "No SPRT result on #$PR after $HOURS hours: ask Carlos for \`tail -5 ~/sprt-$PR.log\` on the test Mac."
exit 1
