#!/usr/bin/env bash
# Rates cb2026's strength levels on this machine and posts the ratings to a GitHub issue or PR.
#
#   scripts/mac-levels.sh <engines file> <issue/PR number> [games per engine] [time control] [git ref]
#
# Each Skill Level (0 to 19; 20 is full strength) plays a gauntlet against the reference
# engines nearest its expected rating, and the gauntlet tool fits its rating on their scale
# (CCRL Blitz). The engines file is levels.txt from scripts/fetch-gauntlet-engines.sh: twelve
# engines rated 1058 to 3049. The first level is expected where the mapping before the first
# calibration put it (1000 + level × 1400 / 19). After that, each level is expected to
# gain as much per level as the last two rated levels did: at least 30 points, and the
# provisional 73 until two are rated; a level that won or lost every game counts as 200 past
# its bound. It plays the 4 engines nearest that (OPPONENTS=N for another number). Only the
# choice of opponents depends on the earlier levels: each rating comes from the reference
# engines alone. The games are saved in target/levels/.
#
# Defaults: 200 games per engine, 40+0.4 (the levels limit nodes, so they rate lower against
# opponents given more time, cjus/chessengine#106), all twenty levels (LEVELS="0 5 10" for
# some), the current origin/main. As in scripts/mac-gauntlet.sh, cb2026 is built at the ref and the
# gauntlet tool at origin/main, one game per performance core minus one, the Mac kept awake
# with caffeinate. A progress comment on the issue is updated after each level, and the result
# follows in its own comment: a table of the levels' ratings. If a level's gauntlet breaks, that
# comment says **Level calibration failed** and quotes the end of its log. Needs git, cargo and
# the GitHub CLI (`gh`, logged in). Set OPENINGS=<file> to use another opening book.
set -euo pipefail

if [[ $# -lt 2 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,24p' "$0"
  exit 2
fi
ENGINES_FILE="$1"
TARGET="$2"
GAMES="${3:-200}"
TC="${4:-40+0.4}"
REF="${5:-origin/main}"
LEVELS="${LEVELS:-0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19}"
NEAR="${OPPONENTS:-4}"
# The UCI_Elo of level 0 and level 19 before the first calibration (cjus/chessengine#103), the
# first guess; the smallest gain per level expected; and
# how far past its bound a level that won or lost every game is expected.
ELO_LOW=1000
ELO_HIGH=2400
MIN_GAIN=30
PAST_BOUND=200

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-levels: '$tool' is not installed" >&2; exit 2; }
done
[[ -r "$ENGINES_FILE" ]] || { echo "mac-levels: can't read $ENGINES_FILE" >&2; exit 2; }

# The reference engines, weakest first.
RATINGS=()
ENGINE_PATHS=()
while read -r rating path; do
  [[ -z "${rating:-}" || "$rating" == \#* ]] && continue
  [[ -x "$path" ]] || { echo "mac-levels: $path is not an executable file" >&2; exit 2; }
  path="$(cd "$(dirname "$path")" && pwd)/$(basename "$path")"
  RATINGS+=("$rating")
  ENGINE_PATHS+=("$path")
done < <(grep -v '^#' "$ENGINES_FILE" | sort -n)
if [[ ${#RATINGS[@]} -lt "$NEAR" ]]; then
  echo "mac-levels: $ENGINES_FILE has ${#RATINGS[@]} engines; each level plays $NEAR" >&2
  exit 2
fi

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-levels.XXXXXX")"
cleanup() {
  git worktree remove --force "$WORK/tree" 2>/dev/null || true
  git worktree remove --force "$WORK/tools" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "Fetching origin..."
git fetch -q --tags origin
git worktree add -q --detach "$WORK/tree" "$REF"
SHA="$(git -C "$WORK/tree" rev-parse --short HEAD)"
TOOLS_SHA="$(git rev-parse --short origin/main)"
if [[ "$(git -C "$WORK/tree" rev-parse HEAD)" == "$(git rev-parse origin/main)" ]]; then
  TOOLS="$WORK/tree"
else
  git worktree add -q --detach "$WORK/tools" origin/main
  TOOLS="$WORK/tools"
fi
echo "Building cb2026 at $REF ($SHA) and the gauntlet tool at main ($TOOLS_SHA)..."
(cd "$WORK/tree" && cargo build --release -q -p cb2026)
(cd "$TOOLS" && cargo build --release -q -p cb2026-tools)
CB2026="$WORK/tree/target/release/cb2026"

CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
CONCURRENCY=$(( CORES > 1 ? CORES - 1 : 1 ))
OPENINGS="${OPENINGS:-$TOOLS/tests/data/openings.epd}"
# Absolute, since the games run in a directory of their own.
OPENINGS="$(cd "$(dirname "$OPENINGS")" && pwd)/$(basename "$OPENINGS")"
KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)
mkdir -p "$ROOT/target/levels" "$WORK/run"
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
RUN="cb2026 at \`$SHA\` ($REF), $GAMES games per engine, $TC, $CONCURRENCY games at a time, the $NEAR engines nearest each level's expected rating; the gauntlet tool at main \`$TOOLS_SHA\`"

# The indexes of the NEAR engines whose ratings are nearest $1, weakest first.
nearest() {
  local i d
  for i in "${!RATINGS[@]}"; do
    d=$(( RATINGS[i] - $1 ))
    echo "${d#-} $i"
  done | sort -n | head -n "$NEAR" | awk '{print $2}' | sort -n
}

# cb2026's path in a log, replaced by its name.
names() {
  sed -e "s|$CB2026|cb2026|g"
}

TABLE="| Skill Level | Expected | Rating | Opponents: rating, cb2026's score | Problems |
| ---: | ---: | --- | --- | --- |"
PROGRESS_ID=""
# Creates the progress comment the first time, and edits it after that.
post_progress() {
  local body="<!-- levels-progress -->
**Level calibration progress** (\`scripts/mac-levels.sh\` on $MACHINE; the result follows in its own comment)

- $RUN
- $1 Updated $(date '+%Y-%m-%d %H:%M %Z').

$TABLE"
  if [[ -z "$PROGRESS_ID" ]]; then
    PROGRESS_ID="$(gh api "repos/{owner}/{repo}/issues/$TARGET/comments" -f body="$body" \
      --jq .id 2>/dev/null)" || PROGRESS_ID=""
  else
    gh api -X PATCH "repos/{owner}/{repo}/issues/comments/$PROGRESS_ID" -f body="$body" \
      >/dev/null 2>&1 || true
  fi
}

PREV=""
PREV_LEVEL=""
PREV_BOUNDED=1
GAIN=""
DONE=0
TOTAL="$(echo "$LEVELS" | wc -w | tr -d ' ')"
for level in $LEVELS; do
  if [[ -z "$PREV" ]]; then
    expected=$(( ELO_LOW + level * (ELO_HIGH - ELO_LOW) / 19 ))
  else
    expected=$(( PREV + ${GAIN:-$(( (ELO_HIGH - ELO_LOW) / 19 ))} * (level - PREV_LEVEL) ))
  fi
  OPPONENT_ARGS=()
  for i in $(nearest "$expected"); do
    OPPONENT_ARGS+=(--opponent "${ENGINE_PATHS[i]}" "${RATINGS[i]}")
  done
  LOG="$WORK/level-$level.log"
  echo
  echo "Skill Level $level, expected about $expected ($((DONE + 1)) of $TOTAL)..."
  set +e
  # From a directory of its own: Walleye 1.3.0 writes a log file into its working directory each
  # time it starts, and it starts once a game.
  (cd "$WORK/run" && ${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$TOOLS/target/release/gauntlet" \
    --engine "$CB2026" --option "Skill Level=$level" "${OPPONENT_ARGS[@]}" \
    --games "$GAMES" --tc "$TC" --concurrency "$CONCURRENCY" \
    --openings "$OPENINGS" --pgn "$ROOT/target/levels/$SHA-level-$level.pgn") 2>&1 | tee "$LOG"
  STATUS="${PIPESTATUS[0]}"
  set -e

  # "Rating: 1166 ± 98 (95%)", or "Rating: above 2400 (...)" or "below" when a level won or
  # lost every game. `value` is the rating, or the bound, and `estimate` what the next level's
  # expectation builds on.
  LINE="$(grep '^Rating:' "$LOG" | tail -1)" || true
  read -r _ first second third _ <<<"${LINE:-x}"
  bounded=1
  case "$first" in
    above)
      value="$second"
      estimate=$(( second + PAST_BOUND ))
      rating="above $second"
      ;;
    below)
      value="$second"
      estimate=$(( second - PAST_BOUND ))
      rating="below $second"
      ;;
    [0-9]*)
      value="$first"
      estimate="$first"
      bounded=0
      rating="**$first ± $third**"
      ;;
    *) value="" ;;
  esac
  if [[ "$STATUS" -ne 0 || -z "$value" ]]; then
    gh issue comment "$TARGET" --body "<!-- levels -->
**Level calibration failed** (\`scripts/mac-levels.sh\` on $MACHINE): the gauntlet for Skill Level $level stopped with exit status $STATUS before reaching a rating.

- $RUN

The levels rated before it:

$TABLE

The end of the log:

\`\`\`
$(tail -15 "$LOG" | names)
\`\`\`"
    echo "Skill Level $level stopped without a rating (exit status $STATUS); posted the log's end to #$TARGET."
    exit 1
  fi

  # "  Fatalii 0.1.1    rated   1058: +1 -1 =2  (50.0%), performance 1058" becomes
  # "Fatalii 0.1.1 1058, 50.0%".
  OPPONENTS_TEXT="$(sed -n '/^Results:/,/^Rating:/p' "$LOG" | grep ' rated ' |
    sed -E 's/^ *(.*[^ ]) +rated +([0-9]+): .*\(([0-9.]+%)\).*/\1 \2, \3/' | paste -sd ';' - |
    sed 's/;/; /g')" || true
  PROBLEMS="$(grep 'Problems for' "$LOG" |
    grep -v ': 0 time losses, 0 illegal moves, 0 failures, 0 aborted games' | names |
    sed -e 's/^ *Problems for //' | paste -sd ';' - | sed 's/;/; /g')" || true
  TABLE="$TABLE
| $level | $expected | $rating | $OPPONENTS_TEXT | ${PROBLEMS:-none} |"

  # The gain per level, from two rated levels; a bound says too little about it.
  if [[ -n "$PREV" && "$bounded" == 0 && "$PREV_BOUNDED" == 0 ]]; then
    GAIN=$(( (estimate - PREV) / (level - PREV_LEVEL) ))
    if [[ "$GAIN" -lt "$MIN_GAIN" ]]; then
      GAIN="$MIN_GAIN"
    fi
  fi
  PREV="$estimate"
  PREV_LEVEL="$level"
  PREV_BOUNDED="$bounded"
  DONE=$(( DONE + 1 ))
  post_progress "$DONE of $TOTAL levels rated."
done

gh issue comment "$TARGET" --body "<!-- levels -->
**Level calibration result** (run by \`scripts/mac-levels.sh\` on $MACHINE)

- $RUN

$TABLE

Ratings are on the reference engines' CCRL Blitz scale, from games at $TC, so they are approximate in the same way as the gauntlets' (docs/PRD.md, \"Rating\"). The ± is each level's own statistical error, with the reference ratings held fixed. Games: \`target/levels/$SHA-level-*.pgn\` on the test machine."
echo "Posted the result to #$TARGET."
