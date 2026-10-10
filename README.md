# cb2026

**cb2026** (ChessBrain 2026) is a modern chess engine in Rust, written to be both strong
(target 2400–2700 CCRL) and easy to learn from. It speaks UCI and XBoard, so it works in Cute
Chess, Banksia, En Croissant, HIARCS Chess Explorer, ChessBase, Shredder and other GUIs, and it
comes with an interactive shell for playing games, watching the engine think and looking inside it.

Status: planning. See [`docs/PRD.md`](docs/PRD.md) for the product requirements.

## Planned usage

```
cb2026            # interactive shell in a terminal
cb2026 --uci      # UCI mode (GUIs switch to it automatically by sending "uci")
cb2026 --xboard   # XBoard/CECP mode
cb2026 bench      # fixed benchmark; prints nodes and speed
```

## License

GPL-3.0. See [`LICENSE`](LICENSE).
