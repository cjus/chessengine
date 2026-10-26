#!/usr/bin/env bash
# Runs a release's robustness run on this machine and posts the result to a GitHub issue or PR.
#
#   scripts/mac-robustness.sh <issue/PR number> [git ref]
#
# cb2026, built at the git ref (default origin/main), plays itself at each of the time controls
# in the PRD's robustness requirements, with the default Move Overhead: by default 6,000 games
# at 1+0.01, 3,000 at 10+0.1 and 1,000 at 40/60 (40 moves in 60 seconds, repeating), 10,000 in
# all, about 10-11 hours on an M3 Max. Set ROBUSTNESS="1+0.01:6000 10+0.1:3000 40/60:1000" to
# change them. Both sides are cb2026, so each game tests it twice; the two sides are separate
# copies of the binary, so the match tool's problem counts say which side was at fault.
#
# The run is clean when no game had a time loss, an illegal move, a failure or an aborted game.
# After each time control it updates one progress comment; at the end it posts the result,
# marked <!-- robustness -->, with a row per time control and the first few problems. Games
# are saved to target/robustness/<commit>-<time control>.pgn. If a match breaks (the match
# tool's exit status 2), the comment says **Robustness run failed** and quotes the end of the
# log, and the remaining time controls aren't played.
#
# Written for the test Mac (Apple Silicon): one game per performance core minus one, the Mac
# kept awake with caffeinate. Needs git, cargo and the GitHub CLI (`gh`, logged in). Set
# OPENINGS=<file> to use another opening book. Set THREADS=N to have both sides search with N
# threads (Lazy SMP); the games at a time are then the performance cores minus one, divided by
# N, since only the side to move searches. See docs/testing.md, "Robustness runs".
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,24p' "$0"
  exit 2
fi
TARGET="$1"
REF="${2:-origin/main}"
ROBUSTNESS="${ROBUSTNESS:-1+0.01:6000 10+0.1:3000 40/60:1000}"

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-robustness: '$tool' is not installed" >&2; exit 2; }
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-robustness.XXXXXX")"
cleanup() {
  git worktree remove --force "$WORK/src" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "Fetching $REF..."
git fetch -q origin
git worktree add -q --detach "$WORK/src" "$REF"
SHA="$(git -C "$WORK/src" rev-parse --short HEAD)"

echo "Building cb2026 and the match tool at $SHA..."
(cd "$WORK/src" && cargo build --release -q -p cb2026 -p cb2026-tools)
BIN="$WORK/src/target/release"
mkdir -p "$WORK/first" "$WORK/second"
cp "$BIN/cb2026" "$WORK/first/cb2026"
cp "$BIN/cb2026" "$WORK/second/cb2026"
VERSION="$("$WORK/first/cb2026" --version 2>/dev/null || echo cb2026)"

CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
THREADS="${THREADS:-1}"
[[ "$THREADS" =~ ^[1-9][0-9]*$ ]] || { echo "mac-robustness: THREADS must be a number of threads" >&2; exit 2; }
CONCURRENCY=$(( CORES > 1 ? (CORES - 1) / THREADS : 1 ))
(( CONCURRENCY >= 1 )) || CONCURRENCY=1
# Threads is left at the engine's default (1) unless THREADS asks for more.
THREAD_OPTIONS=()
(( THREADS > 1 )) && THREAD_OPTIONS=(--option "Threads=$THREADS")
PLAYING="$CONCURRENCY games at a time"
(( THREADS > 1 )) && PLAYING="$PLAYING, $THREADS threads each"
OPENINGS="${OPENINGS:-$WORK/src/tests/data/openings.epd}"
KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
mkdir -p "$ROOT/target/robustness"

# The engines' paths, replaced by which side they were.
names() {
  sed -e "s|$WORK/first/cb2026|first copy|g" -e "s|$WORK/second/cb2026|second copy|g"
}

# The sum of the problem counts in a log: "Problems for X: 0 time losses, 0 illegal moves,
# 0 failures, 0 aborted games", once per engine. Prints four numbers.
problem_counts() {
  grep -E '^Problems for ' "$1" | sed -E 's/^Problems for [^:]*: //' |
    awk -F', ' '{for (i = 1; i <= 4; i++) {split($i, part, " "); total[i] += part[1]}}
      END {printf "%d %d %d %d\n", total[1], total[2], total[3], total[4]}'
}

ROWS=""
DONE=""
DETAILS=""
TOTAL_GAMES=0
TOTAL_PROBLEMS=0
PROGRESS_ID=""
STARTED="$(date '+%Y-%m-%d %H:%M %Z')"

post_progress() {
  local body="<!-- robustness-progress -->
**Robustness run progress** (\`scripts/mac-robustness.sh\` on $MACHINE; the result follows in its own comment)

- $VERSION at \`$SHA\` against itself, $PLAYING, started $STARTED
- Time controls: $ROBUSTNESS

| Time control | Games | Score (first copy) | Time losses | Illegal moves | Failures | Aborted |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
$ROWS
Updated $(date '+%Y-%m-%d %H:%M %Z')."
  if [[ -z "$PROGRESS_ID" ]]; then
    PROGRESS_ID="$(gh api "repos/{owner}/{repo}/issues/$TARGET/comments" -f body="$body" --jq .id 2>/dev/null)" || PROGRESS_ID=""
  else
    gh api -X PATCH "repos/{owner}/{repo}/issues/comments/$PROGRESS_ID" -f body="$body" >/dev/null 2>&1 || true
  fi
}

for spec in $ROBUSTNESS; do
  TC="${spec%:*}"
  GAMES="${spec#*:}"
  SLUG="$(printf '%s' "$TC" | tr '/+' '-p')"
  LOG="$WORK/robustness-$SLUG.log"
  PGN="$ROOT/target/robustness/$SHA-$SLUG.pgn"
  echo "Playing $GAMES games at $TC, $PLAYING..."
  set +e
  ${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$BIN/match" \
    --engine "$WORK/first/cb2026" --engine "$WORK/second/cb2026" \
    --tc "$TC" --games "$GAMES" --concurrency "$CONCURRENCY" \
    --openings "$OPENINGS" --pgn "$PGN" ${THREAD_OPTIONS[@]+"${THREAD_OPTIONS[@]}"} 2>&1 | tee "$LOG"
  STATUS="${PIPESTATUS[0]}"
  set -e
  FINAL="$(grep '^Final:' "$LOG" | tail -1)" || true
  if [[ "$STATUS" -ge 2 || -z "$FINAL" ]]; then
    gh issue comment "$TARGET" --body "<!-- robustness -->
**Robustness run failed** (\`scripts/mac-robustness.sh\` on $MACHINE): the match at $TC stopped with exit status $STATUS before it finished, so the run has no result.

- $VERSION at \`$SHA\` against itself, $PLAYING, started $STARTED
- Finished before it: ${DONE:-nothing}

The end of the log:

\`\`\`
$(tail -15 "$LOG" | names)
\`\`\`

Games: \`target/robustness/$SHA-$SLUG.pgn\` on the test machine."
    echo "The match at $TC stopped without a result (exit status $STATUS); posted the log's end to #$TARGET."
    exit 1
  fi
  SCORE="${FINAL#Final: }"
  PLAYED="${SCORE%% *}"
  read -r TIME_LOSSES ILLEGAL FAILURES ABORTED <<< "$(problem_counts "$LOG")"
  ROWS="$ROWS| $TC | $PLAYED | ${SCORE#* games: } | $TIME_LOSSES | $ILLEGAL | $FAILURES | $ABORTED |
"
  PROBLEMS=$(( TIME_LOSSES + ILLEGAL + FAILURES + ABORTED ))
  DONE="$DONE${DONE:+; }$TC, $PLAYED games, problems: $PROBLEMS"
  TOTAL_GAMES=$(( TOTAL_GAMES + PLAYED ))
  TOTAL_PROBLEMS=$(( TOTAL_PROBLEMS + PROBLEMS ))
  # The reasons the match tool lists under each engine's counts.
  REASONS="$(sed -n '/^Final:/,$p' "$LOG" | grep -E '^  ' | names | head -10 || true)"
  if [[ -n "$REASONS" ]]; then
    DETAILS="$DETAILS
At $TC:

\`\`\`
$REASONS
\`\`\`
"
  fi
  post_progress
done

if (( TOTAL_PROBLEMS == 0 )); then
  VERDICT="**Clean**: no time losses, illegal moves, failures or aborted games in $TOTAL_GAMES games."
else
  PROBLEM_WORD="problems"
  (( TOTAL_PROBLEMS == 1 )) && PROBLEM_WORD="problem"
  VERDICT="**Not clean**: $TOTAL_PROBLEMS $PROBLEM_WORD in $TOTAL_GAMES games (the first few are below)."
fi
gh issue comment "$TARGET" --body "<!-- robustness -->
**Robustness run result** (run by \`scripts/mac-robustness.sh\` on $MACHINE)

- $VERSION at \`$SHA\` against itself, $PLAYING, default Move Overhead
- Started $STARTED, finished $(date '+%Y-%m-%d %H:%M %Z')
- $VERDICT

| Time control | Games | Score (first copy) | Time losses | Illegal moves | Failures | Aborted |
| --- | ---: | --- | ---: | ---: | ---: | ---: |
$ROWS${DETAILS}
Games: \`target/robustness/$SHA-*.pgn\` on the test machine."
echo "Posted the result to #$TARGET."
