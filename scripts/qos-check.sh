#!/usr/bin/env bash
# Shows the scheduler priority of cb2026's search threads, to check whether macOS honours the
# user-initiated quality of service they ask for (crates/engine/src/qos.rs).
#
#   scripts/qos-check.sh [path to cb2026]
#
# It searches the start position over UCI with 2 threads for 4 seconds and prints `ps -M` for
# the engine halfway through. In the PRI column, a thread at "default" shows 31; at
# "user-initiated", 37 if macOS allows it. Run it from the context to check (Terminal.app, or
# with the binary a GUI uses): the cap depends on how the process was started.
set -euo pipefail
if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,10p' "$0"
  exit 2
fi
BIN="${1:-target/release/cb2026}"
[[ -x "$BIN" ]] || { echo "qos-check: no engine at $BIN (cargo build --release first)" >&2; exit 2; }
[[ "$(uname)" == "Darwin" ]] || { echo "qos-check: QoS classes exist only on macOS" >&2; exit 2; }
(printf 'uci\nsetoption name Threads value 2\nisready\nposition startpos\ngo movetime 4000\n'
 sleep 5
 printf 'quit\n') | "$BIN" > /dev/null &
ENGINE=$!
sleep 2
echo "cb2026 ($BIN), searching with 2 threads; the busy rows (%CPU near 100) are the search:"
ps -M -p "$ENGINE"
wait
