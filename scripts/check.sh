#!/usr/bin/env bash
# Checks a change before its PR merges: everything the CI workflow checks, run locally.
#
#   scripts/check.sh
#
# Run this on the PR's latest commit before merging; GitHub's `CI` check
# (.github/workflows/ci.yml) runs the same steps on every PR and on main each night, so change
# both together. This run also covers the Mac: the GPU tests run on Metal here. It formats
# nothing and changes nothing; it stops at the first failure. Needs cargo-deny and mdbook
# (`brew install cargo-deny mdbook`).
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
# The trainer's GPU backend sits behind its `gpu` feature, so it is checked on its own. Its tests
# run on the GPU (Metal on a Mac); with no GPU (Linux CI) they say so and pass.
step cargo clippy -p cb2026-trainer --features gpu --all-targets --locked -- -D warnings
step cargo test --workspace --locked
step cargo test -p cb2026-trainer --features gpu --locked gpu::
step cargo run --release --locked -p cb2026-tools --bin perft-suite
RUSTDOCFLAGS="-D warnings" step cargo doc --workspace --no-deps --document-private-items --locked
RUSTDOCFLAGS="-D warnings" step cargo doc -p cb2026-trainer --features gpu --no-deps --document-private-items --locked
step cargo deny check
step mdbook build book

echo
echo "All checks passed."
