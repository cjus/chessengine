# cb2026

**cb2026** (ChessBrain 2026) is a modern chess engine in Rust, written to be both strong
(target 2400–2700 CCRL) and easy to learn from. It speaks UCI and XBoard, so it works in Cute
Chess, Banksia, En Croissant, HIARCS Chess Explorer, ChessBase, Shredder and other GUIs, and it
comes with an interactive shell for playing games, watching the engine think and looking inside it.

Status: in development, stage by stage (see the stages in [`docs/PRD.md`](docs/PRD.md)).
The book in `book/` explains each technique as it's added.

## Usage

```
cb2026            # interactive shell in a terminal
cb2026 --uci      # UCI mode (GUIs switch to it automatically by sending "uci")
cb2026 --xboard   # XBoard/CECP mode
cb2026 bench      # fixed benchmark; prints nodes and speed
```

For GUIs see [`docs/gui-compat.md`](docs/gui-compat.md); to run it as a Lichess bot see
[`docs/lichess.md`](docs/lichess.md).

Releases are made from version tags; see [`docs/releasing.md`](docs/releasing.md).

## License

cb2026, its book and its tools are licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE))
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option.

The one exception is Syzygy tablebase probing. The `cb2026-syzygy` crate links
[`shakmaty-syzygy`](https://crates.io/crates/shakmaty-syzygy), which is GPL-3.0-or-later, so a
`cb2026` binary built with the `syzygy` feature (on by default, and in the released binaries)
is GPL-3.0-or-later as a whole: see
[`crates/syzygy/LICENSE-GPL-3.0`](crates/syzygy/LICENSE-GPL-3.0). Built with
`cargo build --release --no-default-features`, cb2026 has no Syzygy probing and is
`MIT OR Apache-2.0` throughout.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion
in the work by you, as defined in the Apache-2.0 license, shall be dual licensed as above,
without any additional terms or conditions.
