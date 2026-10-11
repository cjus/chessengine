#!/usr/bin/env bash
# Measures how cb2026 scales with threads (Lazy SMP) and posts the result to a pull request.
#
#   scripts/mac-smp.sh <PR number> [threads] [time control]
#
# Builds the PR branch and plays it with N threads (default 4) against the same build with
# one thread, as an SPRT with bounds [0, 5]. Defaults to 8+0.08. Both engines share the
# machine, so it runs one game at a time and needs N + 1 free performance cores.
#
# Written for the test Mac (Apple Silicon): keeps the Mac awake with caffeinate, and needs
# git, cargo and the GitHub CLI (`gh`, logged in). Run it on its own, not alongside other
# tests. See docs/PRD.md, "Test machine".
set -euo pipefail

if [[ $# -lt 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,12p' "$0"
  exit 2
fi
PR="$1"
THREADS="${2:-4}"
TC="${3:-8+0.08}"

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-smp: '$tool' is not installed" >&2; exit 2; }
done

CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
if (( THREADS + 1 > CORES )); then
  echo "mac-smp: $THREADS threads plus a one-thread opponent need $((THREADS + 1)) cores; this machine has $CORES performance cores" >&2
  exit 2
fi

ROOT="$(git rev-parse --show-toplevel)"
cd "$ROOT"
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-smp.XXXXXX")"
cleanup() {
  git worktree remove --force "$WORK/pr" 2>/dev/null || true
  rm -rf "$WORK"
}
trap cleanup EXIT

echo "Fetching PR #$PR..."
git fetch -q origin "pull/$PR/head:refs/remotes/origin/smp-pr-$PR"
git worktree add -q --detach "$WORK/pr" "origin/smp-pr-$PR"
SHA="$(git -C "$WORK/pr" rev-parse --short HEAD)"

echo "Building PR #$PR ($SHA)..."
(cd "$WORK/pr" && cargo build --release -q -p cb2026 -p cb2026-tools)

KEEP_AWAKE=()
command -v caffeinate >/dev/null && KEEP_AWAKE=(caffeinate -i)
ENGINE="$WORK/pr/target/release/cb2026"
LOG="$WORK/smp.log"
echo "Running SPRT [0, 5]: $THREADS threads vs 1 thread at $TC, one game at a time..."
set +e
${KEEP_AWAKE[@]+"${KEEP_AWAKE[@]}"} "$WORK/pr/target/release/match" \
  --engine "$ENGINE" --engine "$ENGINE" \
  --first-option "Threads=$THREADS" \
  --tc "$TC" --games 4000 --concurrency 1 \
  --openings "$WORK/pr/tests/data/openings.epd" --sprt 0 5 | tee "$LOG"
set -e

FINAL="$(grep '^Final:' "$LOG" | tail -1)"
VERDICT="$(grep '^SPRT:' "$LOG" | tail -1)"
LAST="$(grep 'LLR' "$LOG" | tail -1)"
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"

gh pr comment "$PR" --body "<!-- smp -->
**Lazy SMP scaling** (run by \`scripts/mac-smp.sh\` on $MACHINE)

- PR #$PR at \`$SHA\`: $THREADS threads vs 1 thread, time control $TC, one game at a time
- ${FINAL#Final: }
- ${LAST}
- **${VERDICT#SPRT: }**"
echo "Posted the result to PR #$PR."
