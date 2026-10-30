#!/usr/bin/env bash
# Binds the book (book/src) into the print edition: a PDF and an EPUB in book/print.
#
#   scripts/build-print-book.sh [makebook options]
#
# Run it after the book changes, and commit the two files it writes with the change. It draws
# the figures (book/print/figures.py), stages the chapters in a temporary folder with every
# {{#include}} filled in, the figures placed and the chapters labelled as the web book numbers
# them (book/print/stage.py), and binds them with the makebook skill of the bookcraft Claude Code
# plugin (marketplace cjus/cjus-skills), unmodified. book/print/README.md says what to look at
# afterwards. Options go to makebook, such as --type-size 17 for large print (with --out, or it
# replaces the usual PDF).
#
# Needs python3, the bookcraft plugin with the `labels` and `code_wrap` keys, set up once with
# its scripts/install.sh, and poppler (`brew install poppler`); epubcheck, if installed, checks
# the EPUB. BOOKCRAFT_DIR picks a bookcraft other than the newest installed.
set -euo pipefail

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,16p' "$0"
  exit 2
fi
cd "$(dirname "$0")/.."

PLUGINS="$HOME/.claude/plugins/cache/cjus-skills/bookcraft"
if [[ -z "${BOOKCRAFT_DIR:-}" ]]; then
  BOOKCRAFT_DIR="$(ls -d "$PLUGINS"/*/ 2>/dev/null | sort -V | tail -1)"
fi
BOOKCRAFT_DIR="${BOOKCRAFT_DIR%/}"
BINDER="$BOOKCRAFT_DIR/skills/makebook/scripts/build-book.py"
if [[ ! -f "$BINDER" ]]; then
  echo "No bookcraft plugin found (looked in $PLUGINS). Install it from the cjus-skills" >&2
  echo "marketplace, run its scripts/install.sh, or set BOOKCRAFT_DIR." >&2
  exit 1
fi
if ! grep -q 'code_wrap' "$BOOKCRAFT_DIR/skills/makebook/SKILL.md"; then
  echo "bookcraft at $BOOKCRAFT_DIR is too old: the print edition needs makebook's" >&2
  echo "\`labels\` and \`code_wrap\` keys. Update the plugin (/plugin in Claude Code)." >&2
  exit 1
fi
echo "Binding with bookcraft $(sed -n 's/.*"version": "\(.*\)".*/\1/p' \
  "$BOOKCRAFT_DIR/.claude-plugin/plugin.json") ($BOOKCRAFT_DIR)"

TITLE="$(sed -n 's/^title = "\(.*\)"$/\1/p' book/book.toml)"
SLUG="$(echo "$TITLE" | tr '[:upper:]' '[:lower:]' | tr -cs 'a-z0-9' '-' | sed 's/^-//; s/-$//')"
# The commit the text and the code excerpts come from, marked when either has changes not yet
# committed.
COMMIT="$(git rev-parse --short HEAD)"
if ! git diff --quiet HEAD -- book/src crates; then
  COMMIT="$COMMIT, with uncommitted changes"
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
python3 book/print/figures.py book/print/diagrams
python3 book/print/stage.py book/src "$stage" "$COMMIT"
"$BOOKCRAFT_DIR/scripts/bookcraft-python" "$BINDER" "$TITLE" "$stage" \
  --out "book/print/$SLUG.pdf" "$@"

if command -v epubcheck >/dev/null; then
  epubcheck "book/print/$SLUG.epub" | grep -E '^Messages:'
else
  echo "epubcheck isn't installed, so the EPUB wasn't checked (brew install epubcheck)."
fi
