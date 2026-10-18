#!/usr/bin/env bash
# Runs a rating gauntlet on this machine and posts the result to a GitHub issue or PR.
#
#   scripts/mac-gauntlet.sh <engines file> <issue/PR number> [games per engine] [time control] [git ref]
#
# The engines file lists the reference engines, one per line: the engine's rating on the
# reference list (CCRL Blitz), then the path to its binary. Lines starting with # are comments:
#
#   # rating  path
#   2150      /Users/carlos/engines/engine-a
#   2400      /Users/carlos/engines/engine-b
#
# scripts/fetch-gauntlet-engines.sh builds the reference engines and writes this file. Defaults:
# 800 games per engine, 8+0.08, the current origin/main. cb2026 is built at the git ref (a stage
# tag, a branch, a commit); the gauntlet tool and the openings always come from origin/main,
# since the stage tags predate them and main's match runner handles engine failures (#91). The
# reference engines are never committed to the repository (see docs/PRD.md, "License and IP").
#
# Written for the test Mac (Apple Silicon): one game per performance core minus one, the Mac
# kept awake with caffeinate. Needs git, cargo and the GitHub CLI (`gh`, logged in). Set
# OPENINGS=<file> to use another opening book. If the run breaks, the comment says **Gauntlet
# run failed** and quotes the end of the log. See docs/PRD.md, "Test machine".
set -euo pipefail

if [[ $# -lt 2 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,22p' "$0"
  exit 2
fi
ENGINES_FILE="$1"
TARGET="$2"
GAMES="${3:-800}"
TC="${4:-8+0.08}"
REF="${5:-origin/main}"

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-gauntlet: '$tool' is not installed" >&2; exit 2; }
done
[[ -r "$ENGINES_FILE" ]] || { echo "mac-gauntlet: can't read $ENGINES_FILE" >&2; exit 2; }

OPPONENTS=()
while read -r rating path; do
  [[ -z "${rating:-}" || "$rating" == \#* ]] && continue
  [[ -x "$path" ]] || { echo "mac-gauntlet: $path is not an executable file" >&2; exit 2; }
  OPPONENTS+=(--opponent "$path" "$rating")
done < "$ENGINES_FILE"
[[ ${#OPPONENTS[@]} -gt 0 ]] || { echo "mac-gauntlet: no engines in $ENGINES_FILE" >&2; exit 2; }

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-gauntlet.XXXXXX")"
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

CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
CONCURRENCY=$(( CORES > 1 ? CORES - 1 : 1 ))
OPENINGS="${OPENINGS:-$TOOLS/tests/data/openings.epd}"
KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)

LOG="$WORK/gauntlet.log"
echo "Running the gauntlet: $GAMES games per engine at $TC, $CONCURRENCY at a time..."
set +e
${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$TOOLS/target/release/gauntlet" \
  --engine "$WORK/tree/target/release/cb2026" "${OPPONENTS[@]}" \
  --games "$GAMES" --tc "$TC" --concurrency "$CONCURRENCY" \
  --openings "$OPENINGS" 2>&1 | tee "$LOG"
STATUS="${PIPESTATUS[0]}"
set -e

MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
RUN="cb2026 at \`$SHA\` ($REF), $GAMES games per engine, $TC, $CONCURRENCY games at a time; the gauntlet tool at main \`$TOOLS_SHA\`"
# cb2026's path in the log, replaced by its name.
names() {
  sed -e "s|$WORK/tree/target/release/cb2026|cb2026|g"
}
if [[ "$STATUS" -ne 0 ]] || ! grep -q '^Rating:' "$LOG"; then
  gh issue comment "$TARGET" --body "<!-- gauntlet -->
**Gauntlet run failed** (\`scripts/mac-gauntlet.sh\` on $MACHINE): the gauntlet tool stopped with exit status $STATUS before reaching a rating.

- $RUN

The end of the log:

\`\`\`
$(tail -15 "$LOG" | names)
\`\`\`"
  echo "The gauntlet stopped without a rating (exit status $STATUS); posted the log's end to #$TARGET."
  exit 1
fi

RESULTS="$(sed -n '/^Results:/,$p' "$LOG")"
# Only the engines that had a problem; the rest had none.
PROBLEMS="$(grep 'Problems for' "$LOG" \
  | grep -v ': 0 time losses, 0 illegal moves, 0 failures, 0 aborted games' | names)" || true
if [[ -n "$PROBLEMS" ]]; then
  PROBLEMS="
\`\`\`
$PROBLEMS
\`\`\`"
else
  PROBLEMS="none, for any engine."
fi
gh issue comment "$TARGET" --body "<!-- gauntlet -->
**Gauntlet result** (run by \`scripts/mac-gauntlet.sh\` on $MACHINE)

- $RUN

\`\`\`
$RESULTS
\`\`\`

Time losses, illegal moves, failures and aborted games: $PROBLEMS"
echo "Posted the result to #$TARGET."
