#!/bin/bash
# SessionStart hook for Claude Code cloud sessions: makes sure the pinned Rust toolchain,
# cargo-deny and mdbook are available, so fmt, clippy, tests, licence checks and the book build
# all work. Does nothing on a local machine.
set -euo pipefail

if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

cd "${CLAUDE_PROJECT_DIR:-$(dirname "$0")/../..}"

# Installs the toolchain and components named in rust-toolchain.toml (no-op if present).
rustup toolchain install

# Tools installed with `cargo install` are cached with the container, so this only compiles
# them on the first run.
for tool in cargo-deny mdbook; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    cargo install --locked "$tool"
  fi
done

# Download crate sources now so later builds don't wait on the network.
cargo fetch --locked
