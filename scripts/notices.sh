#!/usr/bin/env bash
# Writes the third-party notices that go with each release of cb2026, into
# target/licences/THIRD-PARTY-NOTICES.txt.
#
#   scripts/notices.sh
#
# The licences of the crates cb2026 is built with ask that their notices go with every copy of
# the program: the MIT and BSD licences' copyright lines, Apache-2.0's NOTICE files. This gathers
# them with cargo-about (scripts/notices/about.toml and the template scripts/notices/notices.hbs),
# which reads each crate's own licence files, and adds any crate's NOTICE file (none today),
# found through a second template, scripts/notices/crates.hbs, which lists the crates' folders.
# The release workflow runs it once, on Linux, and puts the file in every package.
#
# Needs cargo-about 0.9 (cargo install --locked cargo-about@0.9.2 --features cli) and the
# network: it fetches, from a crate's repository, the licence files the crate didn't package.
set -euo pipefail

if [[ $# -gt 0 ]]; then
  sed -n '2,15p' "$0"
  exit 2
fi
cd "$(dirname "$0")/.."
if ! cargo about --version >/dev/null 2>&1; then
  echo "cargo-about isn't installed: cargo install --locked cargo-about@0.9.2 --features cli" >&2
  exit 1
fi

OUT="${CARGO_TARGET_DIR:-target}/licences"
NOTICES="$OUT/THIRD-PARTY-NOTICES.txt"
CRATES=$(mktemp)
FOLDERS=$(mktemp)
trap 'rm -f "$CRATES" "$FOLDERS"' EXIT
mkdir -p "$OUT"

cargo about generate --locked -c scripts/notices/about.toml -o "$CRATES" scripts/notices/notices.hbs
cargo about generate --locked -c scripts/notices/about.toml -o "$FOLDERS" scripts/notices/crates.hbs

RULE="--------------------------------------------------------------------------------"
{
  cat "$CRATES"
  shopt -s nullglob nocaseglob
  while read -r manifest; do
    [[ -n "$manifest" ]] || continue
    folder=$(dirname "$manifest")
    for notice in "$folder"/NOTICE*; do
      printf '\n%s\nThe NOTICE file of %s\n\n' "$RULE" "$(basename "$folder")"
      cat "$notice"
    done
  done < "$FOLDERS"
} > "$NOTICES"
echo "Wrote $NOTICES"
