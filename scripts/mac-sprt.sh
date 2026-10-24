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
# NNUE networks: NET gives the PR's engine an evaluation through UCI EvalFile (a .nnue file,
# `builtin` for its built-in network, or `none` for the hand-crafted evaluation), so the SPRT
# measures it against main's default (its built-in network, or the hand-crafted evaluation if
# it has none); BASE_NET as well gives main's engine one, to compare any two. Before the match
# the script checks that each engine takes its setting (one that can't load a network says so
# and plays on without it, which would quietly test something else), names each side's
# evaluation in the comments (a file by the start of its SHA-256), and puts it in the PGN
# file's name.
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,28p' "$0"
  exit 2
fi
PR="$1"
ELO0="${2:-0}"
ELO1="${3:-5}"
TC="${4:-8+0.08}"

# The networks: files as absolute paths, resolved before changing directory, or the words
# `builtin` and `none`.
absolute() {
  (cd "$(dirname "$1")" && echo "$PWD/$(basename "$1")")
}
is_word() {
  [[ "$1" == builtin || "$1" == none ]]
}
NET="${NET:-}"
BASE_NET="${BASE_NET:-}"
if [[ -n "$BASE_NET" && -z "$NET" ]]; then
  echo "mac-sprt: BASE_NET needs NET too (NET=builtin keeps the PR's own built-in network)" >&2
  exit 2
fi
for net in ${NET:+"$NET"} ${BASE_NET:+"$BASE_NET"}; do
  if ! is_word "$net" && [[ ! -f "$net" ]]; then
    echo "mac-sprt: no network file '$net' (or the words builtin and none)" >&2
    exit 2
  fi
done
if [[ -n "$NET" ]] && ! is_word "$NET"; then NET="$(absolute "$NET")"; fi
if [[ -n "$BASE_NET" ]] && ! is_word "$BASE_NET"; then BASE_NET="$(absolute "$BASE_NET")"; fi

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
# A side's evaluation in words, and in the PGN file's name.
eval_label() {
  case "$1" in
    builtin) echo "its built-in network" ;;
    none) echo "the hand-crafted evaluation" ;;
    *) echo "network \`$(basename "$1")\` ($(shasum -a 256 "$1" | cut -c1-12))" ;;
  esac
}
eval_tag() {
  case "$1" in
    builtin) echo builtin ;;
    none) echo hce ;;
    *) echo "net-$(shasum -a 256 "$1" | cut -c1-8)" ;;
  esac
}
# What the engine $1 evaluates with when not told: its built-in network, if it has one.
default_label() {
  if printf 'uci\nquit\n' | "$1" 2>/dev/null |
    grep -q '^option name EvalFile type string default <built-in>'; then
    echo "its built-in network"
  else
    echo "the hand-crafted evaluation"
  fi
}
# Exits unless the engine $1 takes EvalFile $2 through UCI, as the match will ask it to.
check_eval() {
  local reply taken
  reply="$(printf 'uci\nsetoption name EvalFile value %s\nisready\nquit\n' "$2" | "$1" 2>&1)" || true
  if [[ "$2" == none ]]; then
    taken="$(grep -E '^info string no NNUE network' <<< "$reply" || true)"
  else
    taken="$(grep -E '^info string NNUE network ' <<< "$reply" | grep -v 'not loaded' || true)"
  fi
  if [[ -z "$taken" ]]; then
    echo "mac-sprt: the engine didn't take EvalFile $2:" >&2
    grep '^info string' <<< "$reply" >&2 || true
    exit 2
  fi
  echo "Evaluation: ${taken#info string }"
}
if [[ -n "$NET" ]]; then
  check_eval "$WORK/new/target/release/cb2026" "$NET"
  NEW_LABEL="PR #$PR with $(eval_label "$NET")"
  BASE_LABEL="main with $(default_label "$WORK/base/target/release/cb2026")"
  NET_TAG="-$(eval_tag "$NET")"
fi
if [[ -n "$BASE_NET" ]]; then
  check_eval "$WORK/base/target/release/cb2026" "$BASE_NET"
  BASE_LABEL="main with $(eval_label "$BASE_NET")"
  NET_TAG="$NET_TAG-vs-$(eval_tag "$BASE_NET")"
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
