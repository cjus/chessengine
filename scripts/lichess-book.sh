#!/usr/bin/env bash
# Makes a Polyglot opening book from one month of Lichess's rated games.
#
#   scripts/lichess-book.sh <YYYY-MM> <book.bin> [make-book options]
#
# The month's file (about 30 GB, zstd) is streamed from https://database.lichess.org/ into
# the make-book tool and decompressed on the way, never stored. With the defaults, the book
# has the moves of the first 20 plies of games where both players are rated 2200 or more, at
# blitz or slower, with no bots: moves played in 20 games or more that scored 35% or more,
# weighted by their points (cb2026_tools::book_maker). Options after the book's name go to
# make-book, such as --games N to stop after N games, or --min-rating 2300. A whole month takes
# about an hour, nearly all of it the download: September 2026's (29 GB) took 58 minutes on an
# M3 Max. Point the engine's BookFile option (or the shell's `set book`) at the file and turn
# OwnBook on.
#
# Lichess's games are released under the Creative Commons CC0 licence. Books are never
# committed to the repository. Needs cargo, curl and zstd (`brew install zstd`).
set -euo pipefail

if [[ $# -lt 2 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,17p' "$0"
  exit 2
fi
MONTH="$1"
# The book's full path: the script runs from the repository's root.
mkdir -p "$(dirname "$2")"
OUT="$(cd "$(dirname "$2")" && pwd)/$(basename "$2")"
shift 2
URL="https://database.lichess.org/standard/lichess_db_standard_rated_$MONTH.pgn.zst"

cd "$(dirname "$0")/.."
if ! curl -fsIL -o /dev/null "$URL"; then
  echo "No file for $MONTH at $URL" >&2
  exit 1
fi
cargo build --release -q -p cb2026-tools --bin make-book
echo "Reading $URL"
errors="$(mktemp)"
trap 'rm -f "$errors"' EXIT
# When make-book stops before the end (--games), curl can't write any more and fails: that's
# expected. Any other curl failure means the download broke off.
set +o pipefail
curl -fsSL "$URL" 2>"$errors" | zstdcat 2>/dev/null | target/release/make-book --out "$OUT" "$@"
statuses=("${PIPESTATUS[@]}")
if [[ "${statuses[2]}" -ne 0 ]]; then
  exit "${statuses[2]}"
fi
if [[ "${statuses[0]}" -ne 0 ]] && ! grep -q "Failure writing output" "$errors"; then
  echo "The download broke off, so the book has only the games read before it:" >&2
  cat "$errors" >&2
  exit 1
fi
