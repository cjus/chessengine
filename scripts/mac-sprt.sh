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
# failures are listed in the PR comment. See docs/PRD.md, "Test machine".
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,13p' "$0"
  exit 2
fi
PR="$1"
ELO0="${2:-0}"
ELO1="${3:-5}"
TC="${4:-8+0.08}"

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-sprt: '$tool' is not installed" >&2; exit 2; }
done

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-sprt.XXXXXX")"
cleanup() {
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

# One game per performance core, minus one for the system (efficiency cores would skew timing).
CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
CONCURRENCY=$(( CORES > 1 ? CORES - 1 : 1 ))
OPENINGS="${OPENINGS:-$WORK/new/tests/data/openings.epd}"
KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)

LOG="$WORK/sprt.log"
mkdir -p "$ROOT/target/sprt"
PGN="$ROOT/target/sprt/pr$PR-$NEW_SHA.pgn"
echo "Running SPRT [$ELO0, $ELO1] at $TC, $CONCURRENCY games at a time..."
set +e
${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$WORK/new/target/release/match" \
  --engine "$WORK/new/target/release/cb2026" \
  --engine "$WORK/base/target/release/cb2026" \
  --tc "$TC" --games 40000 --concurrency "$CONCURRENCY" \
  --openings "$OPENINGS" --sprt "$ELO0" "$ELO1" --pgn "$PGN" | tee "$LOG"
set -e

FINAL="$(grep '^Final:' "$LOG" | tail -1)"
VERDICT="$(grep '^SPRT:' "$LOG" | tail -1)"
LAST="$(grep 'LLR' "$LOG" | tail -1)"
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
# The "Problems" lines after the final score, with the engines' paths replaced by their names.
PROBLEMS="$(sed -n '/^Final:/,$p' "$LOG" | grep -E '^(Problems for |  )' |
  sed -e "s|$WORK/new/target/release/cb2026|PR #$PR|g" -e "s|$WORK/base/target/release/cb2026|main|g" \
    -e 's|^Problems for |- |' -e 's|^  |  - |' || true)"

gh pr comment "$PR" --body "<!-- sprt -->
**SPRT result** (run by \`scripts/mac-sprt.sh\` on $MACHINE)

- PR #$PR at \`$NEW_SHA\` vs \`main\` at \`$BASE_SHA\`
- Bounds [$ELO0, $ELO1], time control $TC, $CONCURRENCY games at a time
- ${FINAL#Final: }
- ${LAST}
- **${VERDICT#SPRT: }**

Problems (time losses, illegal moves, failures):
${PROBLEMS:-- none reported}

Games: \`target/sprt/pr$PR-$NEW_SHA.pgn\` on the test machine."
echo "Posted the result to PR #$PR."
