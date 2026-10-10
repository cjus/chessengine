# Board representation and FEN

*Stage 1 · tag `stage-01-board` · code in `crates/chess`*

Before an engine can search or evaluate anything, it needs a way to hold a chess position in
memory. That choice shapes everything built on top of it, so it's worth getting right first.

## The idea

A position is more than where the pieces stand. To know which moves are legal and how the game
can end, an engine also needs:

- **whose turn it is**;
- **castling rights**: whether each king and rook have moved;
- the **en passant square**: where a pawn that just made a double step could be captured;
- the **halfmove clock**, for the fifty-move rule;
- the **fullmove number**.

### Numbering the squares

cb2026 numbers the squares 0 to 63, starting at a1 and going along each rank:

```text
 8 | 56 57 58 59 60 61 62 63
 7 | 48 49 50 51 52 53 54 55
 ...
 2 |  8  9 10 11 12 13 14 15
 1 |  0  1  2  3  4  5  6  7
      a  b  c  d  e  f  g  h
```

So `index = rank * 8 + file`, a step north is `+8` and a step east is `+1`. Most bitboard
engines use this layout (the Chess Programming Wiki calls it *little-endian rank-file*).

```rust,ignore
{{#include ../../crates/chess/src/square.rs:square}}
```

### Bitboards

A **bitboard** is a set of squares packed into one 64-bit integer: bit `i` set means square `i`
is in the set. One bitboard can hold "all white pawns" or "every square a knight on g1
attacks", and set operations become single instructions:

| Question | Bitboard code |
| --- | --- |
| Squares in either set | `a \| b` |
| Squares in both sets | `a & b` |
| Squares in `a` but not `b` | `a & !b` |
| How many squares? | `a.count()` (one `popcnt` instruction) |
| Which is the first square? | `a.first()` (one "count trailing zeros" instruction) |

```rust,ignore
{{#include ../../crates/chess/src/bitboard.rs:bitboard}}
```

Iterating over a bitboard's squares takes one step per square *in the set*, not one per square
on the board: find the lowest set bit, then clear it with `x & (x - 1)`.

### Two views of the board

cb2026 keeps the pieces in two forms at once:

- **Bitboards**, one per piece type and one per colour. "Where are White's knights?" is the
  intersection of the knight set and the white set.
- **A mailbox**: a 64-entry array saying what is on each square. "What's on e4?" is one array
  read.

```rust,ignore
{{#include ../../crates/chess/src/position.rs:position}}
```

Every change to the board updates both, so they can never disagree:

```rust,ignore
{{#include ../../crates/chess/src/position.rs:put_piece}}
```

The cost is a little extra work per move; the gain is that both kinds of question are cheap.

### Castling rights that also work for Chess960

In standard chess the castling rooks start on a and h, so four flags (`KQkq`) are enough. In
Chess960 the rooks can start on other files, so cb2026 stores *the file of the rook* for each
right. Standard chess is just the case where those files are a and h, and one piece of code
handles both games.

## FEN: a position as one line of text

**Forsyth–Edwards Notation** writes a whole position on one line. The starting position is:

```text
rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1
```

Its six fields are:
1. the pieces, rank 8 first, with digits counting empty squares;
2. the side to move;
3. castling rights;
4. the en passant square;
5. the halfmove clock;
6. the fullmove number.

GUIs send positions to engines in FEN, test suites are written in it, and the shell's `fen`
command uses it.

For Chess960, cb2026 reads both common extensions: **X-FEN**, where `K`/`Q` mean the outermost
rook and a file letter names an inner one, and **Shredder-FEN**, which always uses file letters,
like `HAha`. It writes X-FEN, which is plain `KQkq` in standard chess.

The parser also rejects positions that can't occur in a game:
- a side without exactly one king;
- a pawn on the first or last rank;
- castling rights without the king and rook in place;
- an en passant square with no pawn that could have just made a double step.

Each error explains itself, for example:

```text
Invalid FEN: pawn on d1: pawns can't stand on the first or last rank
```

## Try it

```text
$ cargo run --release
> fen r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1
> bb N          # White's knights as a bitboard
> bb occupied
> flip
> board ascii
```

## Testing it

- Every square's name parses back to the same square, and `flip_rank` twice is the identity.
- Bitboard set operations, iteration order and the grid display.
- A list of positions, including Chess960 ones, survives FEN → position → FEN unchanged.
- Each kind of bad FEN gives the expected error.
- `Position::is_consistent` checks that the bitboards and the mailbox agree; every test position
  passes it.

## What it's worth

Nothing yet in Elo: stage 1 can't play. It is the foundation that move generation (stage 2)
builds on.

## Further reading

- Chess Programming Wiki: [Board Representation](https://www.chessprogramming.org/Board_Representation),
  [Bitboards](https://www.chessprogramming.org/Bitboards),
  [Square Mapping Considerations](https://www.chessprogramming.org/Square_Mapping_Considerations),
  [Mailbox](https://www.chessprogramming.org/Mailbox),
  [Forsyth-Edwards Notation](https://www.chessprogramming.org/Forsyth-Edwards_Notation),
  [X-FEN](https://www.chessprogramming.org/X-FEN).
