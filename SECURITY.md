# Security policy

## Reporting a vulnerability

Please report a security problem privately, through GitHub's
[private vulnerability reporting](https://github.com/cjus/chessengine/security/advisories/new)
(the repository's **Security** tab, then **Report a vulnerability**), not in a public issue.
Say what you found, how to reproduce it (a command, a position, a file), and what it lets an
attacker do. You'll get an answer within a week.

## What counts

cb2026 reads input it didn't write, and a crash, a hang or memory use without bound on any of
it is worth reporting:

- the commands a GUI or a script sends over UCI, XBoard or the Chess Node Protocol;
- FEN, EPD and PGN text;
- files it is pointed at: Polyglot opening books (`BookFile`), NNUE networks (`EvalFile`) and
  Syzygy tablebases (`SyzygyPath`).

A weak move or a lost game is a strength problem, not a security one: open an issue for those.

## Supported versions

Fixes go into `main` and the next release. Only the latest release is supported.
