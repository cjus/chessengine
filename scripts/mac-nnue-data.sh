#!/usr/bin/env bash
# Generates an NNUE network's training data on this Mac, trains networks on it, and
# reports to a GitHub issue.
#
#   scripts/mac-nnue-data.sh <issue number, or - to print the reports instead>
#
# Self-play (`datagen --format records`: balanced starts, 6,000 nodes a move) with EVALFILE
# (default builtin: the built-in network, gen3, so the data is for gen4; `none` repeats gen1's,
# which the hand-crafted evaluation played) until there are POSITIONS records (default
# 100,000,000, about 3.2 GB), one game per performance core minus one. About 12 hours on the M3
# Max, so the work is split into chunks of CHUNK_GAMES games (default 56,000, about 4.4 million
# records and half an hour), each its own file in OUT (default ~/nnue-data/gen4). Run the same
# command again after a stop: it keeps the
# finished chunks and redoes only the unfinished one. Chunk k holds the games a single long run
# would number from k × CHUNK_GAMES, so no two chunks share a game.
#
# Then it checks every record (`records`), trains a network of each width in WIDTHS (default
# "256 512") on BACKEND (default gpu) for EPOCHS epochs (default 20), and runs `cb2026 bench`
# with each network and without. It keeps one progress comment on the issue, edited after each
# chunk, and posts a report at the end. It won't start while a match (an SPRT) is running, since
# both need every core. Needs git, cargo and the GitHub CLI (`gh`, logged in).
set -euo pipefail

if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,21p' "$0"
  exit 2
fi
TARGET="$1"
POSITIONS="${POSITIONS:-100000000}"
CHUNK_GAMES="${CHUNK_GAMES:-56000}"
EVALFILE="${EVALFILE:-builtin}"
# The built-in network is gen3, so its games make gen4's data.
OUT="${OUT:-$HOME/nnue-data/gen4}"
WIDTHS="${WIDTHS-256 512}"
BACKEND="${BACKEND:-gpu}"
EPOCHS="${EPOCHS:-20}"
RECORD_BYTES=32

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-nnue-data: '$tool' is not installed" >&2; exit 2; }
done
cd "$(dirname "$0")/.."

if pgrep -f 'target/release/match' >/dev/null; then
  echo "mac-nnue-data: a match (an SPRT?) is running, and both need every core; start this when it ends" >&2
  exit 2
fi

CORES="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
THREADS=$(( CORES > 1 ? CORES - 1 : 1 ))
MACHINE="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
COMMIT="$(git rev-parse --short HEAD)"

size() {
  stat -f%z "$1" 2>/dev/null || stat -c%s "$1"
}

# Every chunk must come from the same engine, or the data would mix two versions' play.
mkdir -p "$OUT"
if [[ -f "$OUT/commit" && "$(cat "$OUT/commit")" != "$COMMIT" ]]; then
  echo "mac-nnue-data: $OUT was started at commit $(cat "$OUT/commit"), and this is $COMMIT." >&2
  echo "Check that commit out to carry on, or delete $OUT/commit to mix the two on purpose." >&2
  exit 2
fi
echo "$COMMIT" > "$OUT/commit"

# Room for the records still to come, and a tenth more.
DONE_BYTES=0
for chunk in "$OUT"/chunk-*.bin; do
  [[ -f "$chunk" ]] && DONE_BYTES=$(( DONE_BYTES + $(size "$chunk") ))
done
NEEDED_KB=$(( (POSITIONS * RECORD_BYTES - DONE_BYTES) / 1024 * 11 / 10 ))
FREE_KB="$(df -k "$OUT" | awk 'NR == 2 { print $4 }')"
if (( NEEDED_KB > FREE_KB )); then
  echo "mac-nnue-data: $OUT needs about $(( NEEDED_KB / 1048576 + 1 )) GB free and has $(( FREE_KB / 1048576 )) GB" >&2
  exit 2
fi

# Keep the Mac awake for as long as this script runs.
command -v caffeinate >/dev/null && { caffeinate -i -w $$ & }

# One progress comment on the issue, created once and then edited (found again after a restart).
MARKER="<!-- nnue-data-progress -->"
PROGRESS_ID=""
if [[ "$TARGET" != "-" ]]; then
  PROGRESS_ID="$(gh api --paginate "repos/{owner}/{repo}/issues/$TARGET/comments" \
    --jq ".[] | select(.body | startswith(\"$MARKER\")) | .id" 2>/dev/null | tail -1)" || PROGRESS_ID=""
fi
progress() {
  local body="$MARKER
**NNUE data progress** (\`scripts/mac-nnue-data.sh\` on $MACHINE at \`$COMMIT\`, $THREADS threads, \`--evalfile $EVALFILE\`, into \`$OUT\`; updated after each chunk)

$1

Updated $(date '+%Y-%m-%d %H:%M %Z')."
  if [[ "$TARGET" == "-" ]]; then
    echo "$body"
  elif [[ -z "$PROGRESS_ID" ]]; then
    PROGRESS_ID="$(gh api "repos/{owner}/{repo}/issues/$TARGET/comments" -f body="$body" --jq .id 2>/dev/null)" || PROGRESS_ID=""
  else
    gh api -X PATCH "repos/{owner}/{repo}/issues/comments/$PROGRESS_ID" -f body="$body" >/dev/null 2>&1 || true
  fi
}
# Posts the file $1 to the issue as a comment of its own, or prints it.
post() {
  if [[ "$TARGET" == "-" ]]; then
    cat "$1"
  else
    gh api "repos/{owner}/{repo}/issues/$TARGET/comments" -F "body=@$1" --jq .html_url || cat "$1"
  fi
}
# 12345678 → 12.3M
millions() {
  awk -v n="$1" 'BEGIN { printf "%.1fM", n / 1e6 }'
}

echo "Building datagen, records, the trainer and cb2026 at $COMMIT..."
cargo build --release --locked -q -p cb2026-tools --bin datagen --bin records
cargo build --release --locked -q -p cb2026-trainer --features gpu
cargo build --release --locked -q -p cb2026

# The data, chunk by chunk.
START=$(date +%s)
CHUNKS=()
total=0 made=0 k=0
progress "Starting: $(millions "$POSITIONS") positions wanted, in chunks of $CHUNK_GAMES games."
while (( total < POSITIONS )); do
  chunk="$OUT/chunk-$(printf %03d "$k").bin"
  if [[ ! -f "$chunk" ]]; then
    echo "Chunk $k: $CHUNK_GAMES games, numbered from $(( k * CHUNK_GAMES ))..."
    rm -f "$chunk.part"
    if ! target/release/datagen --out "$chunk.part" --format records --games "$CHUNK_GAMES" \
      --threads "$THREADS" --seed $(( 1 + k * CHUNK_GAMES )) --evalfile "$EVALFILE"; then
      progress "**Stopped:** datagen failed in chunk $k, after $(millions "$total") positions. The end of the log says why; run the same command again to carry on."
      exit 1
    fi
    mv "$chunk.part" "$chunk"
    made=$(( made + $(size "$chunk") / RECORD_BYTES ))
    fresh=1
  else
    fresh=0
  fi
  CHUNKS+=("$chunk")
  total=$(( total + $(size "$chunk") / RECORD_BYTES ))
  k=$(( k + 1 ))
  if (( fresh )); then
    elapsed=$(( $(date +%s) - START ))
    per_hour=$(( made * 3600 / (elapsed > 0 ? elapsed : 1) ))
    left=$(( POSITIONS > total ? POSITIONS - total : 0 ))
    eta="$(awk -v l="$left" -v r="$per_hour" 'BEGIN { if (r > 0) printf "%.1f", l / r; else print "?" }')"
    progress "- $k chunks, $(millions "$total") of $(millions "$POSITIONS") positions ($(( total >= POSITIONS ? 100 : total * 100 / POSITIONS ))%)
- $(millions "$per_hour") positions an hour; about $eta hours to go, then a few minutes per network trained"
  fi
done
DATA_HOURS="$(awk -v s="$(( $(date +%s) - START ))" 'BEGIN { printf "%.1f", s / 3600 }')"

echo "Checking every record..."
cat "${CHUNKS[@]}" | target/release/records /dev/stdin --show 3 > "$OUT/records.txt"
cat "$OUT/records.txt"

# The networks.
DATA_ARGS=()
for chunk in "${CHUNKS[@]}"; do DATA_ARGS+=(--data "$chunk"); done
NETS=()
for width in $WIDTHS; do
  progress "- Data done: $(millions "$total") positions in $k chunks
- Training a network of $width on the $BACKEND ($EPOCHS epochs)"
  echo "Training a network of $width on the $BACKEND..."
  rm -rf "$OUT/net-$width"
  if target/release/trainer "${DATA_ARGS[@]}" --out "$OUT/net-$width" --hidden "$width" \
    --epochs "$EPOCHS" --backend "$BACKEND" 2>&1 | tee "$OUT/train-$width.log"; then
    NETS+=("$width")
  fi
done

echo "Benchmarking..."
bench() {
  target/release/cb2026 bench "$@" 2>&1 | awk '/^Nodes searched/ { n = $NF } /^Nodes\/second/ { s = $NF } END { print n, s }'
}
read -r hce_nodes hce_speed <<< "$(bench none)"
BENCH="| Evaluation | Bench nodes | Nodes a second |"$'\n'"| --- | --- | --- |"$'\n'"| HCE | $hce_nodes | $hce_speed |"
for width in ${NETS[@]+"${NETS[@]}"}; do
  net="$OUT/net-$width/epoch-$(printf %02d "$EPOCHS").nnue"
  read -r nodes speed <<< "$(bench "$net")"
  BENCH+=$'\n'"| Net, $width | $nodes | $speed |"
done

REPORT="$OUT/report.md"
{
  echo "### NNUE data and networks on $MACHINE"
  echo
  echo "Commit \`$COMMIT\`; $THREADS threads; games played with \`--evalfile $EVALFILE\`; files in \`$OUT\`."
  echo
  echo "**Data:** $(millions "$total") positions in $k chunks of $CHUNK_GAMES games; $DATA_HOURS hours for the $(millions "$made") made in this run."
  echo
  echo '```'
  cat "$OUT/records.txt"
  echo '```'
  echo
  echo "**Networks** ($EPOCHS epochs on the $BACKEND):"
  echo
  echo "| Width | Eval scale | Training loss | Validation loss | Records a second | Minutes | File |"
  echo "| --- | --- | --- | --- | --- | --- | --- |"
  for width in $WIDTHS; do
    log="$OUT/train-$width.log"
    scale="$(sed -n 's/^eval scale \([0-9.]*\).*/\1/p' "$log" | head -1)"
    last="$(grep -E "^epoch $EPOCHS/$EPOCHS:" "$log" | tail -1)"
    if [[ " ${NETS[*]-} " == *" $width "* && -n "$last" ]]; then
      train_loss="$(sed -n 's/.*training loss \([0-9.]*\).*/\1/p' <<< "$last")"
      valid_loss="$(sed -n 's/.*validation \([0-9.]*\).*/\1/p' <<< "$last")"
      speed="$(sed -n 's/.* \([0-9]*\) records\/s.*/\1/p' <<< "$last")"
      minutes="$(sed -n 's/.* \([0-9]*\) s in all.*/\1/p' <<< "$last" | awk '{ printf "%.0f", $1 / 60 }')"
      net="net-$width/epoch-$(printf %02d "$EPOCHS").nnue"
      hash="$(shasum -a 256 "$OUT/$net" | cut -c1-12)"
      echo "| $width | $scale | $train_loss | $valid_loss | $(millions "$speed") | $minutes | \`$net\` ($hash) |"
    else
      echo "| $width | ${scale:-?} | **failed**: see \`train-$width.log\` | | | | |"
    fi
  done
  for width in $WIDTHS; do
    echo
    echo "<details><summary>Every epoch, width $width</summary>"
    echo
    echo '```'
    grep -E '^(eval scale|network|training on|epoch [0-9]+/)' "$OUT/train-$width.log" || true
    echo '```'
    echo "</details>"
  done
  echo
  echo "**Bench** (\`cb2026 bench\`; a weak network searches far more nodes):"
  echo
  echo "$BENCH"
  echo
  echo "<sub>Posted by \`scripts/mac-nnue-data.sh\`.</sub>"
} > "$REPORT"

progress "- Done: $(millions "$total") positions, $(( ${#NETS[@]} )) network(s) trained. The report is the next comment."
post "$REPORT"
echo "Done. The report is in $REPORT."
