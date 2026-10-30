# Releasing cb2026

Releases are built by the `Release` workflow (`.github/workflows/release.yml`) from a version
tag. Claude's cloud sessions can't push tags, so the tag is Carlos's step.

## Before a release

1. The milestone's gates have passed (`docs/PRD.md`, "Milestones"): for v1.0 (M5), the
   rating gauntlet, a clean 10,000-game robustness run (`scripts/mac-robustness.sh`;
   `docs/testing.md`, "Robustness runs") and the GUI compatibility matrix (`docs/gui-compat.md`).
2. The version in the root `Cargo.toml` (`[workspace.package] version`) is the new version, and
   `docs/elo-ledger.md` and the book are up to date. Claude prepares this in a PR.
3. For v1.0.0: the robustness run again, since the engine now evaluates with the built-in NNUE
   network (#196) and the first run was with the hand-crafted evaluation. Clean on 2026-10-24
   (#197); the tag is #199.

## Making the release

```sh
git checkout main && git pull
git tag v1.0.0            # the version from Cargo.toml, with a v
git push origin v1.0.0
```

The workflow builds `cb2026` for Linux (x86-64), macOS (Apple Silicon and Intel) and Windows
(x86-64), packages each with the README, the licences (`LICENSE-MIT`, `LICENSE-APACHE`,
and `LICENSE-GPL-3.0`, since the release binaries include Syzygy probing) and the
third-party notices (`THIRD-PARTY-NOTICES.txt`, below), attaches the default NNUE network
(`nets/*.nnue`, which the binaries also have built in), adds `SHA256SUMS.txt`, and creates a
**draft** release with notes generated from the merged PRs. Before publishing it, sign the Mac
binaries and upload the book (below). Then check the draft on GitHub (Releases), edit the notes
if needed, and publish it. The `Cross-platform` workflow also runs the tests on macOS and
Windows for the tag.

The binaries are built for each platform's baseline CPU, so they run everywhere; building from
source with `RUSTFLAGS="-C target-cpu=native"` is a little faster on a given machine.

## Signing the Mac binaries

The workflow's macOS binaries are unsigned. That is because the Developer ID key is in a Mac's
keychain, not on GitHub. Downloaded with a browser, an unsigned binary is blocked by macOS's
Gatekeeper the first time it runs. So once the draft is there, run this on the Mac that has the
key (the M1 Max):

```sh
scripts/mac-sign-release.sh v1.0.0
```

It takes about a minute, most of it Apple's notary service:
1. It downloads the two macOS packages and checks them against `SHA256SUMS.txt`.
2. It signs each `cb2026` with the hardened runtime and a secure timestamp.
3. It has Apple notarise both, and checks that macOS then finds them notarised.
4. It runs the bench on each (the Intel one through Rosetta).
5. It repacks them and replaces them on the draft, with their two lines of `SHA256SUMS.txt`
   rewritten and every other line kept.

Run again, it sees the binaries are notarised and does nothing. It refuses a published release
unless `ALLOW_PUBLISHED=1`. The book can go up before or after, but not while it runs, since
both rewrite `SHA256SUMS.txt`. A notarisation can't be stapled to a bare binary, so macOS asks
Apple online the first time a downloaded `cb2026` runs; after that it opens with no warning.

It needs two things in the Mac's keychain, set up once (2026-10-24, on the M1 Max):
- **A Developer ID Application certificate**, here `Developer ID Application: Carlos Justiniano
  (JJ4X2Z6RUH)`. Make it at developer.apple.com (Certificates, **+**, Developer ID Application)
  from a request made in Keychain Access (Certificate Assistant, Request a Certificate From a
  Certificate Authority), with Profile Type **G2 Sub-CA**. Don't use Xcode's **+**: it made one
  under Apple's previous intermediate, which expires on 2027-02-01. Apple issues them for about
  13 months; this one expires on **2027-11-23**, so renew it each autumn the same way. Signed
  binaries keep working after it expires, since they carry a timestamp. Keep a `.p12` export of
  it (certificate and key) somewhere safe: it is needed to sign on another Mac.
- **notarytool credentials**, saved as `cb2026-notary` with an app-specific password from
  account.apple.com (Sign-In and Security, App-Specific Passwords):
  `xcrun notarytool store-credentials cb2026-notary --apple-id <Apple ID> --team-id JJ4X2Z6RUH`.

`SIGN_IDENTITY` and `NOTARY_PROFILE` choose others. The script picks the keychain's only Developer
ID Application identity by itself, and asks for `SIGN_IDENTITY` (its SHA-1, from `security
find-identity -v -p codesigning`) when there are two, as while a renewed certificate overlaps
the old one.

## The opening book

A release can also carry an opening book, which is made on a Mac, not by the workflow (book
chapter 18, `scripts/lichess-book.sh`). v1.0.0 carries September 2026's (cjus/chessengine#183).
Upload it to the draft before publishing, and say in the notes what it is: Polyglot, from
Lichess's games (CC0), for `OwnBook` and `BookFile`.

The workflow wrote `SHA256SUMS.txt` before the book was there, so add the book's line to it and
replace it on the draft (the same format as the workflow's: `shasum -a 256 -c SHA256SUMS.txt`
checks every file):

```sh
cd "$(mktemp -d)"
gh release upload v1.0.0 -R cjus/chessengine ~/books/cb2026-lichess-2026-09.bin
gh release download v1.0.0 -R cjus/chessengine -p SHA256SUMS.txt
(cd ~/books && shasum -a 256 cb2026-lichess-2026-09.bin) >> SHA256SUMS.txt
gh release upload v1.0.0 -R cjus/chessengine SHA256SUMS.txt --clobber
gh release download v1.0.0 -R cjus/chessengine -p SHA256SUMS.txt -O -   # 7 lines, the book's last
```

## Third-party notices

The licences of the crates cb2026 is built with ask that their notices go with every copy:
the MIT and BSD licences' copyright lines, Apache-2.0's NOTICE files. `scripts/notices.sh`
gathers them, with cargo-about, into `target/licences/THIRD-PARTY-NOTICES.txt`. The release
workflow runs it once on Linux and puts the file in every package, and the release lists it
beside the packages too. To look at it before a release, run it by hand: it needs cargo-about
0.9 (`cargo install --locked cargo-about@0.9.2 --features cli`) and the network, and takes a
few seconds.

A new dependency under a licence that `scripts/notices/about.toml` doesn't accept makes the
script fail, as `cargo deny check` would in CI; add the licence to both, if `deny.toml`'s rules
allow it. Solrac, which is to ship cb2026 beside it (Solrac Chess AI #98), is to take this
file with the cb2026 it ships.
