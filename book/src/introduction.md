# Introduction

This book explains how **cb2026** works, from an empty board to a master-strength chess engine,
one technique at a time.

Every chapter matches a *stage* of the engine's development and a git tag in the repository
(`stage-01-board`, `stage-02-movegen`, ...). At each tag the engine builds, passes its tests and
plays legal chess, so you can check out any stage, run it, change it and measure the result.

Each chapter covers:

- **The idea:** what the technique does and why it works.
- **The code:** the real source, included from the repository so it can't drift out of date.
- **Testing it:** how we know it's correct.
- **What it's worth:** the Elo gain measured when it was added, from the
  [Elo ledger](https://github.com/cjus/chessengine/blob/main/docs/elo-ledger.md).
- **Further reading:** the Chess Programming Wiki, papers and other engines.

Chapters in the table of contents that can't be opened yet are the ones still to be written.
They appear as the engine reaches each stage.
