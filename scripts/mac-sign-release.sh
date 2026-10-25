#!/usr/bin/env bash
# Signs and notarises a draft release's macOS binaries, and puts them back on the draft.
#
#   scripts/mac-sign-release.sh <tag>          for example: scripts/mac-sign-release.sh v1.0.0
#
# The Release workflow builds cb2026 for Apple Silicon and Intel Macs, but it can't sign them:
# the Developer ID key is in a Mac's keychain, not on GitHub. Run this on that Mac once the
# workflow has made the draft. It downloads the two macOS packages and checks them against
# SHA256SUMS.txt; signs each binary with the hardened runtime and a secure timestamp; sends both
# to Apple's notary service in one zip and waits for it (a few minutes); checks that macOS finds
# them notarised and that they still run; then repacks them, rewrites their two lines of
# SHA256SUMS.txt (every other line, the book's included, is kept) and replaces the three files
# on the draft. A notarisation can't be stapled to a bare binary, so macOS asks Apple online
# the first time a downloaded cb2026 runs.
#
# Needs the GitHub CLI (`gh`, logged in), Xcode's command-line tools, a "Developer ID
# Application" identity in the keychain (set SIGN_IDENTITY to its name or SHA-1 when there are
# several) and notarytool credentials saved as NOTARY_PROFILE (default cb2026-notary). It
# refuses a published release unless ALLOW_PUBLISHED=1, since replacing its files changes what
# people have downloaded, and does nothing when the binaries are already notarised. Don't run
# it while the book is being uploaded: both rewrite SHA256SUMS.txt. REPO defaults to
# cjus/chessengine. See docs/releasing.md, "Signing the Mac binaries".
set -euo pipefail

if [[ $# -ne 1 || "$1" == "-h" || "$1" == "--help" ]]; then
  sed -n '2,22p' "$0"
  exit 2
fi
TAG="$1"
REPO="${REPO:-cjus/chessengine}"
NOTARY_PROFILE="${NOTARY_PROFILE:-cb2026-notary}"
# Each Mac target the workflow builds, and the architecture its binary must have.
TARGETS="aarch64-apple-darwin:arm64 x86_64-apple-darwin:x86_64"

fail() {
  echo "mac-sign-release: $*" >&2
  exit 1
}

for tool in gh codesign security xcrun ditto lipo shasum plutil; do
  command -v "$tool" >/dev/null || { echo "mac-sign-release: '$tool' is not installed" >&2; exit 2; }
done

# The identity: SIGN_IDENTITY, or the keychain's only Developer ID Application one, by its SHA-1
# so that a renewed certificate beside an old one of the same name can't make it ambiguous.
IDENTITIES="$(security find-identity -v -p codesigning)"
if [[ -z "${SIGN_IDENTITY:-}" ]]; then
  FOUND="$(printf '%s\n' "$IDENTITIES" | grep '"Developer ID Application: ' || true)"
  COUNT="$(printf '%s' "$FOUND" | grep -c . || true)"
  [[ "$COUNT" -ge 1 ]] ||
    fail "no Developer ID Application identity in the keychain (docs/releasing.md, \"Signing the Mac binaries\")"
  [[ "$COUNT" -eq 1 ]] ||
    fail "$COUNT Developer ID Application identities in the keychain; set SIGN_IDENTITY to one's SHA-1:
$FOUND"
  SIGN_IDENTITY="$(printf '%s' "$FOUND" | awk '{print $2}')"
fi
SIGNER="$(printf '%s\n' "$IDENTITIES" | { grep -F -- "$SIGN_IDENTITY" || true; } | sed -n 's/^[^"]*"\(.*\)"$/\1/p' | sed -n 1p)"
[[ -n "$SIGNER" ]] || fail "no valid signing identity matches SIGN_IDENTITY=$SIGN_IDENTITY"

DRAFT="$(gh release view "$TAG" -R "$REPO" --json isDraft --jq .isDraft 2>/dev/null)" ||
  fail "no release $TAG in $REPO: push the tag and wait for the Release workflow's draft"
if [[ "$DRAFT" != "true" && "${ALLOW_PUBLISHED:-0}" != "1" ]]; then
  fail "$TAG is published, and replacing its files changes what people have downloaded; set ALLOW_PUBLISHED=1 to do it anyway"
fi

WORK="$(mktemp -d "${TMPDIR:-/tmp}/cb2026-sign.XXXXXX")"
trap 'rm -rf "$WORK"' EXIT
cd "$WORK"
mkdir downloaded unpacked notarize signed uploaded

PATTERNS=(-p SHA256SUMS.txt)
for spec in $TARGETS; do
  PATTERNS+=(-p "cb2026-$TAG-${spec%%:*}.tar.gz")
done
echo "Downloading the macOS packages and SHA256SUMS.txt from $TAG ($REPO)..."
gh release download "$TAG" -R "$REPO" -D downloaded "${PATTERNS[@]}"

# Each package is checked against the line the workflow wrote for it, then unpacked.
for spec in $TARGETS; do
  target="${spec%%:*}"
  arch="${spec#*:}"
  name="cb2026-$TAG-$target"
  [[ -f "downloaded/$name.tar.gz" ]] || fail "$TAG has no $name.tar.gz"
  line="$(awk -v file="$name.tar.gz" '$2 == file' downloaded/SHA256SUMS.txt)"
  [[ -n "$line" ]] || fail "SHA256SUMS.txt has no line for $name.tar.gz"
  (cd downloaded && printf '%s\n' "$line" | shasum -a 256 -c - >/dev/null) ||
    fail "$name.tar.gz doesn't match its line in SHA256SUMS.txt"
  tar -xzf "downloaded/$name.tar.gz" -C unpacked
  [[ -f "unpacked/$name/cb2026" ]] || fail "$name.tar.gz has no $name/cb2026"
  [[ "$(lipo -archs "unpacked/$name/cb2026")" == "$arch" ]] ||
    fail "$name/cb2026 is $(lipo -archs "unpacked/$name/cb2026"), not $arch"
done

# codesign's "notarized" requirement looks only at the tickets this Mac already has; asking
# Gatekeeper first (spctl) fetches the binary's from Apple. spctl itself always rejects a bare
# binary ("does not seem to be an app"), so only codesign's answer counts.
notarised() {
  spctl --assess --type execute "$1" >/dev/null 2>&1 || true
  codesign --verify --strict --test-requirement="=notarized" "$1" >/dev/null 2>&1
}

ALREADY=1
for spec in $TARGETS; do
  notarised "unpacked/cb2026-$TAG-${spec%%:*}/cb2026" || ALREADY=0
done
if [[ "$ALREADY" == 1 ]]; then
  echo "Both macOS binaries in $TAG are already signed and notarised: nothing to do."
  exit 0
fi

echo "Signing with $SIGNER..."
for spec in $TARGETS; do
  target="${spec%%:*}"
  bin="unpacked/cb2026-$TAG-$target/cb2026"
  codesign --force --options runtime --timestamp --sign "$SIGN_IDENTITY" "$bin"
  codesign --verify --strict "$bin"
  info="$(codesign -dvv "$bin" 2>&1)"
  grep -qxF "Authority=$SIGNER" <<< "$info" || fail "$bin isn't signed by $SIGNER"
  mkdir -p "notarize/$target"
  cp "$bin" "notarize/$target/cb2026"
done

echo "Sending both binaries to Apple's notary service and waiting for it..."
ditto -c -k --keepParent notarize cb2026-notarize.zip
xcrun notarytool submit cb2026-notarize.zip --keychain-profile "$NOTARY_PROFILE" --wait \
  --timeout 1h --output-format json > notary.json || true
STATUS="$(plutil -extract status raw -o - notary.json 2>/dev/null || echo unknown)"
SUBMISSION="$(plutil -extract id raw -o - notary.json 2>/dev/null || echo unknown)"
if [[ "$STATUS" != "Accepted" ]]; then
  cat notary.json >&2 || true
  if [[ "$SUBMISSION" != "unknown" ]]; then
    xcrun notarytool log "$SUBMISSION" --keychain-profile "$NOTARY_PROFILE" >&2 || true
  fi
  fail "the notary service answered '$STATUS' for submission $SUBMISSION, so nothing was uploaded"
fi
echo "Notarised: submission $SUBMISSION, $STATUS."

# macOS asks Apple for the notarisation; it is usually there at once, but give it a minute.
for spec in $TARGETS; do
  target="${spec%%:*}"
  bin="unpacked/cb2026-$TAG-$target/cb2026"
  for attempt in 1 2 3 4 5 6; do
    notarised "$bin" && break
    [[ "$attempt" -lt 6 ]] || fail "macOS doesn't find $target's binary notarised"
    sleep 10
  done
done

# Signing must not change what the binary does: the bench on each, the Intel one through
# Rosetta when it is installed.
for spec in $TARGETS; do
  target="${spec%%:*}"
  arch="${spec#*:}"
  bin="unpacked/cb2026-$TAG-$target/cb2026"
  if arch "-$arch" /usr/bin/true 2>/dev/null; then
    nodes="$({ arch "-$arch" "$bin" bench 2>/dev/null || true; } | sed -n 's/^Nodes searched *: *//p')"
    [[ -n "$nodes" ]] || fail "$target's signed binary doesn't run its bench"
    echo "$target: runs, bench $nodes nodes."
  else
    echo "$target: not run (this Mac can't run $arch binaries; install Rosetta to check it)."
  fi
done

# Repacked as the workflow packs them, without macOS's extended attributes. Only their lines
# of SHA256SUMS.txt change.
cp downloaded/SHA256SUMS.txt signed/SHA256SUMS.txt
UPLOADS=()
for spec in $TARGETS; do
  name="cb2026-$TAG-${spec%%:*}"
  (cd unpacked && COPYFILE_DISABLE=1 tar --no-mac-metadata --no-xattrs -czf "../signed/$name.tar.gz" "$name")
  [[ "$(tar -tzf "downloaded/$name.tar.gz" | sort)" == "$(tar -tzf "signed/$name.tar.gz" | sort)" ]] ||
    fail "the repacked $name.tar.gz doesn't hold the same files as the original"
  sum="$(shasum -a 256 "signed/$name.tar.gz" | awk '{print $1}')"
  awk -v file="$name.tar.gz" -v sum="$sum" '$2 == file {print sum "  " file; next} {print}' \
    signed/SHA256SUMS.txt > signed/SHA256SUMS.next
  mv signed/SHA256SUMS.next signed/SHA256SUMS.txt
  (cd signed && awk -v file="$name.tar.gz" '$2 == file' SHA256SUMS.txt | shasum -a 256 -c - >/dev/null) ||
    fail "the new SHA256SUMS.txt doesn't match the repacked $name.tar.gz"
  UPLOADS+=("signed/$name.tar.gz")
done
UPLOADS+=(signed/SHA256SUMS.txt)

echo "Replacing the two packages and SHA256SUMS.txt on $TAG..."
gh release upload "$TAG" -R "$REPO" --clobber "${UPLOADS[@]}"
gh release download "$TAG" -R "$REPO" -D uploaded "${PATTERNS[@]}"
for file in "${UPLOADS[@]}"; do
  cmp -s "$file" "uploaded/$(basename "$file")" || fail "$(basename "$file") on $TAG isn't the file uploaded"
done

echo
echo "Done: $TAG's macOS binaries are signed by $SIGNER and notarised (submission $SUBMISSION)."
echo "SHA256SUMS.txt on $TAG now reads:"
cat signed/SHA256SUMS.txt
