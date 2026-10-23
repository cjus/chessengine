# Summary

[Introduction](introduction.md)
[How to use this book](how-to-use.md)

# Part I: The rules of chess

- [Board representation and FEN](ch01-board.md)
- [Move generation and perft](ch02-movegen.md)
- [Magic bitboards](ch03-magics.md)
- [Zobrist hashing and talking to GUIs (UCI)](ch04-uci.md)

# Part II: Searching

- [Material, negamax and alpha-beta](ch05-alphabeta.md)
- [Iterative deepening and time management](ch06-iterative.md)
    - [Spending time where it's needed](ch06-1-adaptive-time.md)
- [Quiescence search and MVV-LVA](ch07-quiescence.md)
- [The transposition table](ch08-tt.md)
    - [Buckets: one cache line per key](ch08-1-tt-buckets.md)
- [Piece-square tables and tapered evaluation](ch09-pst.md)
- [Move ordering](ch10-ordering.md)
    - [Continuation history](ch10-1-continuation-history.md)
    - [Capture history](ch10-2-capture-history.md)
- [Principal variation search and aspiration windows](ch11-pvs.md)
- [Null-move pruning](ch12-nullmove.md)
    - [ProbCut](ch12-1-probcut.md)
- [Late move reductions and pruning](ch13-lmr.md)
    - [History pruning](ch13-1-history-pruning.md)
- [Futility, razoring and SEE pruning](ch14-pruning.md)
    - [Extensions and reductions](ch14-1-extensions.md)
    - [Correction history](ch14-2-correction.md)

# Part III: Evaluation

- [A full hand-crafted evaluation](ch15-hce.md)
- [Texel tuning on self-play games](ch16-tuning.md)

# Part IV: A complete engine

- [Lazy SMP](ch17-smp.md)
- [Tablebases, opening books and Chess960](ch18-endgames.md)
- [Strength levels](ch19-levels.md)
- [XBoard, pondering and playing on Lichess](ch20-xboard.md)

# Part V: Neural networks

- [NNUE](ch21-nnue.md)
