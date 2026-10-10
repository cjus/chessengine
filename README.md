# chessengine

A modern chess engine in Rust, written to be both strong (target 2400–2700 CCRL) and easy to
learn from. It speaks UCI and XBoard, so it works in Cute Chess, Arena, Banksia, En Croissant,
ChessBase/Fritz, Hiarcs, Shredder and other GUIs, and it comes with an interactive shell for
playing games, watching the engine think and looking inside it.

Status: planning. See [`docs/PRD.md`](docs/PRD.md) for the product requirements.

## Planned usage

```
chessengine              # interactive shell in a terminal
chessengine --uci        # UCI mode (GUIs switch to it automatically by sending "uci")
chessengine --xboard     # XBoard/CECP mode
chessengine bench        # fixed benchmark; prints nodes and speed
```

## License

GPL-3.0. See [`LICENSE`](LICENSE).
