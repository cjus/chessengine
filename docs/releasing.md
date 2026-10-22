# Releasing cb2026

Releases are built by the `Release` workflow (`.github/workflows/release.yml`) from a version
tag. Claude's cloud sessions can't push tags, so the tag is Carlos's step.

## Before a release

1. The milestone's gates have passed (`docs/PRD.md`, "Milestones"): for v1.0 (M5), the
   rating gauntlet, a clean 10,000-game robustness run (`scripts/mac-robustness.sh`;
   `docs/testing.md`, "Robustness runs") and the GUI compatibility matrix (`docs/gui-compat.md`).
2. The version in the root `Cargo.toml` (`[workspace.package] version`) is the new version, and
   `docs/elo-ledger.md` and the book are up to date. Claude prepares this in a PR.

## Making the release

```sh
git checkout main && git pull
git tag v1.0.0            # the version from Cargo.toml, with a v
git push origin v1.0.0
```

The workflow builds `cb2026` for Linux (x86-64), macOS (Apple Silicon and Intel) and Windows
(x86-64), packages each with the README, the licences (`LICENSE-MIT`, `LICENSE-APACHE`,
and `LICENSE-GPL-3.0`, since the release binaries include Syzygy probing) and the
third-party notices (`THIRD-PARTY-NOTICES.txt`, below), adds
`SHA256SUMS.txt`, and creates a **draft** release with notes generated from the merged PRs. Check the draft on GitHub
(Releases), edit the notes if needed, and publish it. The `Cross-platform` workflow also runs
the tests on macOS and Windows for the tag.

The binaries are built for each platform's baseline CPU, so they run everywhere; building from
source with `RUSTFLAGS="-C target-cpu=native"` is a little faster on a given machine.

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
allow it. Solrac, which is to ship cb2026 beside it (cjus/chessgui#98), is to take this
file with the cb2026 it ships.
