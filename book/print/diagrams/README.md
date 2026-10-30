# Figures in the print edition

`figures.py` draws them; `stage.py` places them (its `FIGURES` list). Only the print edition
(PDF and EPUB) has them; the web book doesn't.

Every chapter was read in full before choosing. The aim is a figure wherever a reader would
otherwise have to hold a shape in their head, and nowhere else.

## Drawn

| Figure | Chapter | Why, and where its facts come from |
|---|---|---|
| `check-mask-and-pin.svg` | 2 | Check masks and pins are spatial; the prose describes squares on a line. Both positions checked with cb2026's `legal` command (Ne2 Ne4; Bb4 Bc3 Bxa5). |
| `magic-index.svg` | 3 | The 10 relevant squares of a rook on d4 are a board shape, and the four-step index is easier to follow beside it. |
| `alpha-beta-cutoff.svg` | 5 | The prose draws a tree in words ("Suppose White's first move turns out to be worth +1..."). |
| `time-limits.svg` | 6 | A timeline whose order is the point: depth 8 crosses the soft limit, so depth 9 never starts. The times are chapter 6's UCI output (5, 40, 194, 3096 ms; soft 2.75 s, hard 8.25 s). |
| `tt-bucket.svg` | 8.1 | A memory layout: one slot read out of a fetched line, against all eight. |
| `singular-outcomes.svg` | 14.1 | One search score, four outcomes, split by `singular_beta`, the double extension margin and beta. Drawn from the singular extension code in `search.rs`, with the margins of 3 and 50. |
| `nnue-accumulator.svg` | 21 | The accumulator update is vector arithmetic in two views; the prose asks the reader to picture rows being added and subtracted. |

## Considered and passed over

- **1 Board:** the square numbering is already a text grid in the chapter.
- **4 UCI:** a session transcript already shows the exchange.
- **6.1, 8, 9, 10, 10.1, 10.2, 12.1, 13.1, 14.2, 16, 19:** their key content is a table already
  in the text; a chart would restate it.
- **7 Quiescence:** the FEN and the shell output carry the horizon example well.
- **11 PVS, 12 Null move, 13 LMR, 14 Pruning:** lines of argument and short lists, which prose
  handles.
- **15 HCE:** the king zone and pawn shield are described exactly in one sentence each.
- **17 Lazy SMP, 18 Endgames, 20 XBoard:** linear flows or protocol transcripts already in the
  text.

A new chapter gets the same decision: add it to one of the two lists.
