#!/usr/bin/env bash
# Runs an SPRT for a pull request against main on this machine and posts the result to the PR.
#
#   scripts/mac-sprt.sh <PR number> [elo0 elo1] [time control]
#
# Defaults: bounds [0, 5] (a "gainer" test) and 8+0.08 (short time control). Use
# `scripts/mac-sprt.sh 42 -5 0` for a non-regression test of a simplification, or
# `scripts/mac-sprt.sh 42 0 5 40+0.4` for the long time control.
#
# Written for the test Mac (Apple Silicon): it runs one game per performance core minus one,
# keeps the Mac awake with caffeinate, and needs git, cargo and the GitHub CLI (`gh`, logged
# in). Set OPENINGS=<file> to use another opening book (default: tests/data/openings.epd).
# The games are saved to target/sprt/pr<PR>-<commit>.pgn; time losses, illegal moves and
# failures are listed in the PR comment. Every PROGRESS_EVERY games (default 2000) it also
# posts the score so far to the PR, as one comment it keeps editing, so that a long run can be
# followed and stopped (scripts/watch-sprt.sh reads it). If the match tool stops without a
# result (an engine that won't start, a worker that failed: exit status 2), it posts the end of
# the log instead, so a broken run is never mistaken for a quiet one. See docs/PRD.md, "Test
# machine".
#
# NNUE networks: NET=<file> gives the PR's engine that network (UCI EvalFile), so the SPRT
# measures it against main's evaluation, the hand-crafted one; with BASE_NET=<file> as well,
# main's engine gets that network, to compare two. Before the match the script checks that each
# network loads (an engine that can't load one says so and plays on without it, which would
# quietly test the hand-crafted evaluation instead), names it with the start of its SHA-256 in
# the comments, and puts it in the PGN file's name.
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,26p' "$0"
  exit 2
fi
PR="$1"
ELO0="${2:-0}"
ELO1="${3:-5}"
TC="${4:-8+0.08}"

# The networks as absolute paths, resolved before changing directory.
absolute() {
  (cd "$(dirname "$1")" && echo "$PWD/$(basename "$1")")
}
NET="${NET:-}"
BASE_NET="${BASE_NET:-}"
if [[ -n "$BASE_NET" && -z "$NET" ]]; then
  echo "mac-sprt: BASE_NET needs NET too (the PR's engine plays one network, main's the other)" >&2
  exit 2
fi
for net in ${NET:+"$NET"} ${BASE_NET:+"$BASE_NET"}; do
  [[ -f "$net" ]] || { echo "mac-sprt: no network file '$net'" >&2; exit 2; }
done
[[ -n "$NET" ]] && NET="$(absolute "$NET")"
[[ -n "$BASE_NET" ]] && BASE_NET="$(absolute "$BASE_NET")"

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-sprt: '$tool' is not installed" >&2; exit 2; }
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-sprt.XXXXXX")"
PROGRESS_PID=""
cleanup() {
  [[ -n "$PROGRESS_PID" ]] && kill "$PROGRESS_PID" 2>/dev/null || true
  git worktree remove --force "$WORK/new" 2>/dev/null || true
  git worktree remove --force "$WORK/base" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "Fetching main and PR #$PR..."
git fetch -q origin main "pull/$PR/head:refs/remotes/origin/sprt-pr-$PR"
git worktree add -q --detach "$WORK/new" "origin/sprt-pr-$PR"
git worktree add -q --detach "$WORK/base" origin/main
NEW_SHA="$(git -C "$WORK/new" rev-parse --short HEAD)"
BASE_SHA="$(git -C "$WORK/base" rev-parse --short HEAD)"

echo "Building PR #$PR ($NEW_SHA) and main ($BASE_SHA)..."
(cd "$WORK/new" && cargo build --release -q -p cb2026 -p cb2026-tools)
(cd "$WORK/base" && cargo build --release -q -p cb2026)

# What each side plays, for the comments, and the networks' options for the match.
NEW_LABEL="PR #$PR"
BASE_LABEL="main"
NET_TAG=""
MATCH_OPTIONS=()
# "`name.nnue` (start of its SHA-256)".
net_label() {
  echo "network \`$(basename "$1")\` ($(shasum -a 256 "$1" | cut -c1-12))"
}
# Exits unless the engine $1 loads the network $2 through UCI, as the match will ask it to.
check_net() {
  local reply loaded
  reply="$(printf 'uci\nsetoption name EvalFile value %s\nisready\nquit\n' "$2" | "$1" 2>&1)" || true
  loaded="$(grep -E '^info string NNUE network ' <<< "$reply" | grep -v 'not loaded' || true)"
  if [[ -z "$loaded" ]]; then
    echo "mac-sprt: the engine didn't load the network $2:" >&2
    grep '^info string' <<< "$reply" >&2 || true
    exit 2
  fi
  echo "Network: ${loaded#info string }"
}
if [[ -n "$NET" ]]; then
  check_net "$WORK/new/target/release/cb2026" "$NET"
  NEW_LABEL="PR #$PR with $(net_label "$NET")"
  BASE_LABEL="main with the hand-crafted evaluation"
  NET_TAG="-net-$(shasum -a 256 "$NET" | cut -c1-8)"
fi
if [[ -n "$BASE_NET" ]]; then
  check_net "$WORK/base/target/release/cb2026" "$BASE_NET"
  BASE_LABEL="main with $(net_label "$BASE_NET")"
  NET_TAG="$NET_TAG-vs-$(shasum -a 256 "$BASE_NET" | cut -c1-8)"
  # Sent to both engines, then the PR's own NET after it, which replaces it.
  MATCH_OPTIONS+=(--option "EvalFile=$BASE_NET")
fi
[[ -n "$NET" ]] && MATCH_OPTIONS+=(--first-option "EvalFile=$NET")

# One game per performance core, minus one for the system (efficiency cores would skew timing).
CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
CONCURRENCY=$(( CORES > 1 ? CORES - 1 : 1 ))
OPENINGS="${OPENINGS:-$WORK/new/tests/data/openings.epd}"
KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)

LOG="$WORK/sprt.log"
mkdir -p "$ROOT/target/sprt"
PGN="$ROOT/target/sprt/pr$PR-$NEW_SHA$NET_TAG.pgn"
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
PROGRESS_EVERY="${PROGRESS_EVERY:-2000}"

# Runs in the background while the match plays: once a minute, reads the latest score line
# from the log ("2000 games: +500 -480 =1020  Elo +3.5 ± 9.9  LLR 0.41"), and each time
# another PROGRESS_EVERY games are done, posts it to the PR. The first time it creates the
# comment; after that it edits the same one, so a long run leaves one comment, not dozens.
post_progress() {
  local next="$PROGRESS_EVERY" id="" line games body
  while sleep 60; do
    line="$(grep -E '^[0-9]+ games:.*LLR' "$LOG" 2>/dev/null | tail -1)" || continue
    games="${line%% *}"
    [[ -n "$games" && "$games" -ge "$next" ]] || continue
    body="<!-- sprt-progress -->
**SPRT progress** (\`scripts/mac-sprt.sh\` on $MACHINE, updated every $PROGRESS_EVERY games; the result follows in its own comment)

- $NEW_LABEL at \`$NEW_SHA\` vs $BASE_LABEL at \`$BASE_SHA\`, bounds [$ELO0, $ELO1], $TC
- $line
- Updated $(date '+%Y-%m-%d %H:%M %Z'). The SPRT decides when the LLR leaves its $(grep -m1 -oE 'bounds \[[^]]*\]' "$LOG" || echo bounds)."
    if [[ -z "$id" ]]; then
      id="$(gh api "repos/{owner}/{repo}/issues/$PR/comments" -f body="$body" --jq .id 2>/dev/null)" || id=""
    else
      gh api -X PATCH "repos/{owner}/{repo}/issues/comments/$id" -f body="$body" >/dev/null 2>&1 || true
    fi
    next=$(( (games / PROGRESS_EVERY + 1) * PROGRESS_EVERY ))
  done
}

echo "Running SPRT [$ELO0, $ELO1] at $TC, $CONCURRENCY games at a time..."
: > "$LOG"
if (( PROGRESS_EVERY > 0 )); then
  post_progress &
  PROGRESS_PID=$!
fi
set +e
${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$WORK/new/target/release/match" \
  --engine "$WORK/new/target/release/cb2026" \
  --engine "$WORK/base/target/release/cb2026" \
  ${MATCH_OPTIONS[@]+"${MATCH_OPTIONS[@]}"} \
  --tc "$TC" --games 40000 --concurrency "$CONCURRENCY" \
  --openings "$OPENINGS" --sprt "$ELO0" "$ELO1" --pgn "$PGN" 2>&1 | tee "$LOG"
STATUS="${PIPESTATUS[0]}"
set -e
if [[ -n "$PROGRESS_PID" ]]; then
  kill "$PROGRESS_PID" 2>/dev/null || true
  wait "$PROGRESS_PID" 2>/dev/null || true
  PROGRESS_PID=""
fi

FINAL="$(grep '^Final:' "$LOG" | tail -1)" || true
# The verdict line, not the "SPRT: H0 elo ..." line the match starts with.
VERDICT="$(grep -E '^SPRT: (H[01] accepted|inconclusive|no games)' "$LOG" | tail -1)" || true
LAST="$(grep 'LLR' "$LOG" | tail -1)" || true
# The engines' paths, replaced by their names.
names() {
  sed -e "s|$WORK/new/target/release/cb2026|PR #$PR|g" -e "s|$WORK/base/target/release/cb2026|main|g"
}

if [[ "$STATUS" -ge 2 || -z "$FINAL" || -z "$VERDICT" ]]; then
  gh pr comment "$PR" --body "<!-- sprt -->
**SPRT run failed** (\`scripts/mac-sprt.sh\` on $MACHINE): the match tool stopped with exit status $STATUS before reaching a result, so there is no verdict.

- $NEW_LABEL at \`$NEW_SHA\` vs $BASE_LABEL at \`$BASE_SHA\`, bounds [$ELO0, $ELO1], $TC
- Last score: ${LAST:-none}

The end of the log:

\`\`\`
$(tail -15 "$LOG" | names)
\`\`\`

Games: \`${PGN#"$ROOT"/}\` on the test machine."
  echo "The match stopped without a result (exit status $STATUS); posted the log's end to PR #$PR."
  exit 1
fi
# The "Problems" lines after the final score, with the engines' paths replaced by their names.
PROBLEMS="$(sed -n '/^Final:/,$p' "$LOG" | grep -E '^(Problems for |  )' | names |
  sed -e 's|^Problems for |- |' -e 's|^  |  - |' || true)"

gh pr comment "$PR" --body "<!-- sprt -->
**SPRT result** (run by \`scripts/mac-sprt.sh\` on $MACHINE)

- $NEW_LABEL at \`$NEW_SHA\` vs $BASE_LABEL at \`$BASE_SHA\`
- Bounds [$ELO0, $ELO1], time control $TC, $CONCURRENCY games at a time
- ${FINAL#Final: }
- ${LAST}
- **${VERDICT#SPRT: }**

Problems (time losses, illegal moves, failures, aborted games):
${PROBLEMS:-- none reported}

Games: \`${PGN#"$ROOT"/}\` on the test machine."
echo "Posted the result to PR #$PR."
