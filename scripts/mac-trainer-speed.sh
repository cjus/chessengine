#!/usr/bin/env bash
# Measures the NNUE trainer's speed on this Mac, on the CPU and on the GPU, and posts the
# results to a GitHub issue or PR as a comment.
#
#   scripts/mac-trainer-speed.sh <issue or PR number, or - to print the results instead>
#
# It builds the trainer with the GPU backend, runs the GPU tests (each kernel against the CPU
# reference), generates GAMES self-play games of training records (default 1800, about 140,000
# records), repeats them 20 times (about 2.8 million records: an epoch of only a few batches
# would stop the GPU at every progress report, and repeated records train as fast as new ones),
# then trains 2 epochs on each backend with hidden layers of 256, 512 and 1024, and once more on
# the CPU with every core instead of the performance cores only. A few minutes on an M1 Max.
# Speeds differ between Macs (GPU cores, memory bandwidth, how many cores of each kind), so
# measure on the Mac that will train. Needs git, cargo and the GitHub CLI (`gh`, logged in).
set -euo pipefail

if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,14p' "$0"
  exit 2
fi
TARGET="$1"
GAMES="${GAMES:-1800}"
COPIES=20
EPOCHS=2

for tool in git cargo gh; do
  command -v "$tool" >/dev/null || { echo "mac-trainer-speed: '$tool' is not installed" >&2; exit 2; }
done

cd "$(dirname "$0")/.."
WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-trainer-speed.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT

PERFORMANCE="$(sysctl -n hw.perflevel0.physicalcpu 2>/dev/null || getconf _NPROCESSORS_ONLN)"
EFFICIENCY="$(sysctl -n hw.perflevel1.physicalcpu 2>/dev/null || echo 0)"
ALL=$(( PERFORMANCE + EFFICIENCY ))
CHIP="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || uname -m)"
GPU_CORES="$(system_profiler SPDisplaysDataType 2>/dev/null | sed -n 's/.*Total Number of Cores: //p' | head -1)"
MEMORY_GB=$(( $(sysctl -n hw.memsize 2>/dev/null || echo 0) / 1073741824 ))
COMMIT="$(git rev-parse --short HEAD)"

echo "Building the trainer (with the GPU backend) and datagen at $COMMIT..."
cargo build --release --locked -q -p cb2026-trainer --features gpu
cargo build --release --locked -q -p cb2026-tools --bin datagen

echo "Running the GPU tests..."
if cargo test --release --locked -q -p cb2026-trainer --features gpu gpu:: > "$WORK/tests.log" 2>&1; then
  TESTS="$(grep -m1 -oE '[0-9]+ passed' "$WORK/tests.log")"
else
  TESTS="**failed** (log below)"
fi

echo "Generating $GAMES games of records on $PERFORMANCE threads..."
target/release/datagen --out "$WORK/games.bin" --format records --games "$GAMES" \
  --threads "$PERFORMANCE" --seed 2026 | tail -1
for _ in $(seq "$COPIES"); do cat "$WORK/games.bin"; done > "$WORK/records.bin"
RECORDS=$(( $(wc -c < "$WORK/records.bin") / 32 ))

# Trains EPOCHS epochs and prints the last epoch's speed and training loss.
train() {
  local backend="$1" hidden="$2" threads="$3"
  rm -rf "$WORK/net"
  target/release/trainer --data "$WORK/records.bin" --out "$WORK/net" --epochs "$EPOCHS" \
    --hidden "$hidden" --threads "$threads" --validation 0.01 --backend "$backend" \
    > "$WORK/train.log" 2>&1 || { echo "failed"; return; }
  [[ "$backend" == gpu ]] && cp "$WORK/train.log" "$WORK/gpu.log"
  sed -n "s/^epoch $EPOCHS\/$EPOCHS: training loss \([0-9.]*\), .* \([0-9]*\) records\/s.*/\2 \1/p" \
    "$WORK/train.log"
}
# 1234567 → 1.23M
millions() {
  [[ "$1" =~ ^[0-9]+$ ]] && awk -v n="$1" 'BEGIN { printf "%.2fM", n / 1e6 }' || echo "$1"
}

TABLE="| Hidden | CPU, $PERFORMANCE threads | GPU | GPU ÷ CPU | Same loss |"$'\n'"| --- | --- | --- | --- | --- |"
for hidden in 256 512 1024; do
  echo "Training with a hidden layer of $hidden..."
  read -r cpu cpu_loss <<< "$(train cpu "$hidden" "$PERFORMANCE")"
  read -r gpu gpu_loss <<< "$(train gpu "$hidden" "$PERFORMANCE")"
  ratio="$(awk -v g="$gpu" -v c="$cpu" 'BEGIN { if (c > 0 && g > 0) printf "%.2f×", g / c; else print "-" }')"
  same="$([[ -n "$cpu_loss" && "$cpu_loss" == "$gpu_loss" ]] && echo "yes ($cpu_loss)" || echo "no ($cpu_loss, $gpu_loss)")"
  TABLE+=$'\n'"| $hidden | $(millions "$cpu") | $(millions "$gpu") | $ratio | $same |"
done
echo "Training on the CPU with all $ALL cores..."
read -r every_core _ <<< "$(train cpu 256 "$ALL")"
ADAPTER="$(sed -n 's/^training on the GPU: //p' "$WORK/gpu.log" 2>/dev/null | head -1)"
[[ -z "$ADAPTER" ]] && ADAPTER="(see the log)"

BODY="$WORK/comment.md"
{
  echo "### NNUE trainer speed on $CHIP"
  echo
  echo "$PERFORMANCE performance and $EFFICIENCY efficiency cores, ${GPU_CORES:-?} GPU cores, $MEMORY_GB GB, macOS $(sw_vers -productVersion 2>/dev/null || echo ?); commit \`$COMMIT\`. GPU tests: $TESTS."
  echo
  echo "Records a second in epoch $EPOCHS of $EPOCHS, on $RECORDS records ($GAMES games, $COPIES copies), batches of 16,384:"
  echo
  echo "$TABLE"
  echo
  echo "The CPU at 256 with all $ALL cores: $(millions "$every_core") records a second (against $PERFORMANCE performance cores above). GPU: $ADAPTER."
  if [[ "$TESTS" == *failed* ]]; then
    echo
    echo '```'
    tail -40 "$WORK/tests.log"
    echo '```'
  fi
  echo
  echo "<sub>Posted by \`scripts/mac-trainer-speed.sh\`.</sub>"
} > "$BODY"

if [[ "$TARGET" == "-" ]]; then
  cat "$BODY"
else
  gh api "repos/{owner}/{repo}/issues/$TARGET/comments" -F "body=@$BODY" --jq .html_url
fi
echo "Done."
