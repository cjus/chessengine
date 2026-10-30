#!/usr/bin/env bash
# Binds the book (book/src) into the print edition, a PDF and an EPUB, and publishes them.
#
#   scripts/build-print-book.sh [--no-upload] [makebook options]
#
# Run it on a clean checkout after the book changes. It draws the figures
# (book/print/figures.py), stages the chapters in a temporary folder with every {{#include}}
# filled in, the figures placed and the chapters labelled as the web book numbers them
# (book/print/stage.py), and binds them with the makebook skill of the bookcraft Claude Code
# plugin (marketplace cjus/cjus-skills), unmodified, into book/print/, which Git ignores. Then
# it uploads both to the PNX Studios CDN, under the commit they were bound from and as the
# latest edition:
#
#   https://cdn.pnxstudios.com/solrac/book/latest/building-a-chess-engine-in-rust.pdf (and .epub)
#   https://cdn.pnxstudios.com/solrac/book/<commit>/...
#
# It won't upload with uncommitted changes to book/src, crates or book/print, since the commit's
# copy has to be what that commit binds. --no-upload builds without publishing; makebook options,
# such as --type-size 17 for large print, build a variant and need --no-upload too.
#
# Needs python3, the bookcraft plugin with the `labels` and `code_wrap` keys (1.17.0 or later),
# set up once with its scripts/install.sh, and poppler (`brew install poppler`); epubcheck, if
# installed, checks the EPUB. Uploading needs the AWS CLI with write access to the
# pnxstudios-solrac bucket and CloudFront invalidations (AWS_PROFILE picks a profile).
# BOOKCRAFT_DIR picks a bookcraft other than the newest installed.
set -euo pipefail

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  sed -n '2,26p' "$0"
  exit 2
fi
cd "$(dirname "$0")/.."

BUCKET=pnxstudios-solrac
PREFIX=solrac/book
CDN=cdn.pnxstudios.com

upload=1
makebook=()
for arg in "$@"; do
  if [[ "$arg" == "--no-upload" ]]; then
    upload=0
  else
    makebook+=("$arg")
  fi
done
if [[ $upload == 1 && ${#makebook[@]} -gt 0 ]]; then
  echo "makebook options build a variant, which mustn't replace the published edition:" >&2
  echo "add --no-upload." >&2
  exit 1
fi
if [[ $upload == 1 ]] && ! git diff --quiet HEAD -- book/src crates book/print; then
  echo "book/src, crates or book/print has uncommitted changes, and the upload is filed under" >&2
  echo "the commit: commit them first, or build with --no-upload." >&2
  exit 1
fi

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
# committed (only possible with --no-upload).
COMMIT="$(git rev-parse --short HEAD)"
cover_commit="$COMMIT"
if ! git diff --quiet HEAD -- book/src crates; then
  cover_commit="$COMMIT, with uncommitted changes"
fi

stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
python3 book/print/figures.py book/print/diagrams
python3 book/print/stage.py book/src "$stage" "$cover_commit"
"$BOOKCRAFT_DIR/scripts/bookcraft-python" "$BINDER" "$TITLE" "$stage" \
  --out "book/print/$SLUG.pdf" ${makebook[@]+"${makebook[@]}"}

if command -v epubcheck >/dev/null; then
  epubcheck "book/print/$SLUG.epub" | grep -E '^Messages:'
else
  echo "epubcheck isn't installed, so the EPUB wasn't checked (brew install epubcheck)."
fi

if [[ $upload == 0 ]]; then
  echo "Built in book/print, not uploaded (--no-upload)."
  exit 0
fi

# A commit's copy never changes, so it may be cached for good; the latest is cached for five
# minutes at most, and invalidated below so a new edition shows at once.
put() {  # file, key, cache-control
  local type=application/pdf
  [[ "$1" == *.epub ]] && type=application/epub+zip
  aws s3 cp --only-show-errors "$1" "s3://$BUCKET/$2" --content-type "$type" \
    --cache-control "$3"
}
for ext in pdf epub; do
  put "book/print/$SLUG.$ext" "$PREFIX/$COMMIT/$SLUG.$ext" "public, max-age=31536000, immutable"
  put "book/print/$SLUG.$ext" "$PREFIX/latest/$SLUG.$ext" "public, max-age=300"
done
dist="$(aws cloudfront list-distributions --output text \
  --query "DistributionList.Items[?Aliases.Items && contains(Aliases.Items, '$CDN')].Id | [0]")"
if [[ -n "$dist" && "$dist" != "None" ]] && aws cloudfront create-invalidation \
  --distribution-id "$dist" --paths "/$PREFIX/latest/*" --output text >/dev/null; then
  echo "Invalidated /$PREFIX/latest/* on CloudFront ($dist)."
else
  echo "warning: no CloudFront invalidation, so the latest edition may take up to 5 minutes" >&2
  echo "to show (and longer where an old copy was cached before this script set max-age)." >&2
fi

for ext in pdf epub; do
  url="https://$CDN/$PREFIX/latest/$SLUG.$ext"
  echo "Published $url (and https://$CDN/$PREFIX/$COMMIT/$SLUG.$ext)"
done
