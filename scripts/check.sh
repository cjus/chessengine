#!/usr/bin/env bash
# Checks a change before its PR merges: everything the CI workflow checks, run locally.
#
#   scripts/check.sh
#
# Pull requests aren't checked on GitHub (Actions minutes, cjus/chessgui#73): run this on the
# PR's latest commit before merging. It formats nothing and changes nothing; it stops at the
# first failure. The nightly CI run (.github/workflows/ci.yml) runs the same steps on main:
# change both together. Needs cargo-deny and mdbook (`brew install cargo-deny mdbook`).
set -euo pipefail

cd "$(dirname "$0")/.."

step() {
  echo
  echo "==> $*"
  "$@"
}

# The tablebase tests run when the small test tables are here (downloaded once, then reused);
# if the download site is down they are skipped, as in CI.
if scripts/download-syzygy.sh syzygy-tables test; then
  export CB2026_SYZYGY_PATH="$PWD/syzygy-tables"
else
  echo "warning: no test tablebases, so the tablebase tests are skipped" >&2
fi

step cargo fmt --all --check
step cargo clippy --workspace --all-targets --locked -- -D warnings
step cargo test --workspace --locked
step cargo run --release --locked -p cb2026-tools --bin perft-suite
RUSTDOCFLAGS="-D warnings" step cargo doc --workspace --no-deps --document-private-items --locked
step cargo deny check
step mdbook build book

echo
echo "All checks passed."
