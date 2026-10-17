#!/usr/bin/env bash
# Downloads and builds the rating gauntlet's reference engines, and writes the engines file
# that scripts/mac-gauntlet.sh reads.
#
#   scripts/fetch-gauntlet-engines.sh [directory]
#
# Each engine is cloned from GitHub at the release CCRL rated (the commit is pinned, so a moved
# tag is refused), built from source for this machine, checked to answer the UCI handshake and
# play a move, and listed in <directory>/gauntlet.txt with its CCRL Blitz rating. The default
# directory is ~/engines/gauntlet. Engines already built there are only checked again; delete
# one to build it again. Needs git, make, a C compiler, Go (`brew install go`) and cargo.
#
# The engines are for testing only and never committed to this repository (docs/PRD.md,
# "License and IP"). Their ratings are CCRL Blitz, read on 2026-10-17 from
# https://computerchess.org.uk/ccrl/404/ (cjus/chessengine#36). CCRL rated x86 builds, so each
# version here was checked to behave the same built for Apple Silicon: Stash 27.0's `bench`
# matches the node count in its commit message; Weiss 1.0's perft is exact (its one undefined
# behaviour, a shift while it builds its attack tables, does no harm); Stash 15.3 runs clean
# under the undefined behaviour sanitizer; Go and safe Rust have none. Two first choices
# failed: Weiss 0.10 crashes at startup on Apple Silicon, and Berserk 2.0.0 indexes arrays at
# -1 during the search.
set -euo pipefail

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,15p' "$0"
  exit 2
fi
DIR="${1:-$HOME/engines/gauntlet}"

for tool in git make cc go cargo; do
  command -v "$tool" >/dev/null || {
    echo "fetch-gauntlet-engines: '$tool' is not installed" >&2
    exit 2
  }
done

# name, CCRL Blitz rating, GitHub repository, tag, the commit the tag must point to, build.
ENGINES=(
  "blunder-5.0.0 2018 algerbrex/blunder v5.0.0 82e4fd405194a69384df574c9ced4bc5cb55fed6 go:./blunder"
  "stash-15.3 2174 mhouppin/stash-bot v15.3 7d17ff80eb53b9d4808fd285c7439e641f1c5a3b stash"
  "zahak-3.0 2364 amanjpro/zahak 3.0 1e75088dfdad6a56a77989025c5bf29110cf1d48 zahak"
  "velvet-1.0.0 2513 mhonert/velvet-chess v1.0.0 8cc4c1462310dce50839fc8029b0ea8d234b9676 cargo"
  "blunder-8.5.5 2663 algerbrex/blunder v8.5.5 89230a74a966b3400610271eca49c4be241e94cd go:./blunder"
  "weiss-1.0 2896 TerjeKir/weiss v1.0 f44a4c654c67139ecbd5f3afd9da1ebe41241464 weiss"
  "stash-27.0 3049 mhouppin/stash-bot v27.0 8f7808e2650a0765f7f4a3117461eecce0f92be0 stash"
)

# Builds the engine checked out in `src` at `tag` and copies it to `out`.
build() {
  local how="$1" src="$2" tag="$3" out="$4"
  case "$how" in
    go:*)
      (cd "$src" && go build -o "$out" "${how#go:}")
      ;;
    zahak)
      # Zahak's Makefile sets the version it reports at link time; without it, it is "dev".
      (cd "$src" && go build -ldflags "-X main.version=$tag" -o "$out" ./zahak)
      ;;
    stash)
      # Stash's Makefile knows only x86 architectures; any other ARCH leaves out the x86 flags.
      # Its portable popcount still compiles to the native instruction (`cnt` on Apple
      # Silicon); only memory prefetching is lost.
      (cd "$src/src" && make -j 8 ARCH=generic >/dev/null && cp stash-bot "$out")
      ;;
    cargo)
      (cd "$src" && cargo build --release -q && cp target/release/velvet "$out")
      ;;
    weiss)
      # `bench-basic` is the plain native build, without the x86-only PEXT.
      (cd "$src/src" && make bench-basic COMP=cc >/dev/null && cp weiss "$out")
      ;;
    *)
      echo "fetch-gauntlet-engines: unknown build '$how'" >&2
      return 1
      ;;
  esac
}

# Waits up to 10 seconds (the match tool's limit for `uciok` and `readyok`) for a line of `log`
# matching `pattern`.
await() {
  local log="$1" pattern="$2"
  for _ in $(seq 1 100); do
    grep -q "$pattern" "$log" && return 0
    sleep 0.1
  done
  return 1
}

# Prints the engine's name if it plays the match tool's handshake: `uci`, then `isready` once
# `uciok` is back (Blunder 8.5.5 loses input sent before it), then a move on a clock (`go wtime
# ...`; Blunder 5.0.0 ignores `go movetime`). Then it gets `quit`, and is killed if it is still
# running: not every engine exits when its input closes.
check() {
  local bin="$1" log fifo pid name ok=1
  # An engine that dies at once must not take the script with it.
  trap '' PIPE
  log="$(mktemp)"
  fifo="$(mktemp -u)"
  mkfifo "$fifo"
  "$bin" <"$fifo" >"$log" 2>&1 &
  pid=$!
  exec 3>"$fifo"
  echo uci >&3
  if await "$log" '^ *uciok'; then
    echo isready >&3
    if await "$log" '^ *readyok'; then
      printf 'position startpos moves e2e4\ngo wtime 8000 btime 8000 winc 80 binc 80\n' >&3
      await "$log" '^ *bestmove [a-h][1-8]' && ok=0
    fi
  fi
  echo quit >&3 2>/dev/null || true
  exec 3>&-
  sleep 0.5
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  rm -f "$fifo"
  if [[ "$ok" == 0 ]]; then
    name="$(grep -m1 '^id name ' "$log" | cut -c9-)"
    rm -f "$log"
    echo "${name:-unnamed}"
  else
    echo "fetch-gauntlet-engines: $bin didn't answer uci, isready and go:" >&2
    sed 's/^/  /' "$log" >&2
    rm -f "$log"
    return 1
  fi
}

mkdir -p "$DIR/src"
LIST="$(mktemp)"
trap 'rm -f "$LIST"' EXIT
echo "# CCRL Blitz rating, path (written by scripts/fetch-gauntlet-engines.sh)" >"$LIST"
for line in "${ENGINES[@]}"; do
  read -r name rating repo tag commit how <<<"$line"
  out="$DIR/$name"
  if [[ ! -x "$out" ]]; then
    src="$DIR/src/$name"
    rm -rf "$src"
    echo "Building $name ($repo at $tag)..."
    git -c advice.detachedHead=false clone -q --depth 1 --branch "$tag" \
      "https://github.com/$repo.git" "$src"
    actual="$(git -C "$src" rev-parse HEAD)"
    if [[ "$actual" != "$commit" ]]; then
      echo "fetch-gauntlet-engines: $repo's tag $tag is at $actual, not $commit" >&2
      exit 1
    fi
    build "$how" "$src" "$tag" "$out" || {
      echo "fetch-gauntlet-engines: building $name failed" >&2
      exit 1
    }
  fi
  uci_name="$(check "$out")" || exit 1
  printf '%-14s %s  %s\n' "$name" "$rating" "$uci_name"
  echo "$rating $out" >>"$LIST"
done
cp "$LIST" "$DIR/gauntlet.txt"
echo "Wrote $DIR/gauntlet.txt. Next: scripts/mac-gauntlet.sh $DIR/gauntlet.txt <issue or PR>"
