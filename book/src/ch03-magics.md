# Magic bitboards

*Stage 3 · tag `stage-03-magics` · code in `crates/chess/src/magic.rs`*

Stage 2 found a rook's attacks by walking each ray square by square. That's correct but slow:
up to 14 steps, each with a branch the CPU can't predict well. This chapter replaces it with a
table lookup that gives the same answer in four instructions.

## The idea

A rook on d4 attacks along four rays. Which squares it attacks depends only on which squares
**on those rays** are occupied, and not even all of those:

1. **Mask the relevant squares.** The last square of each ray never matters: the rook attacks
   it whether it's empty or not. For a rook on d4 that leaves 10 relevant squares, so there
   are only 2¹⁰ = 1024 occupancy patterns to care about. `occupied & mask` keeps those bits.
2. **Hash them into an index.** Multiply by a carefully chosen 64-bit *magic number* and keep
   the top bits: `((occupied & mask) * magic) >> (64 − 10)`. The multiplication gathers the
   scattered occupancy bits into the top of the word.
3. **Look up the answer** in a table precomputed for every one of the 1024 patterns.

```rust,ignore
{{#include ../../crates/chess/src/magic.rs:magic}}
```

```rust,ignore
{{#include ../../crates/chess/src/magic.rs:lookup}}
```

### What makes a number "magic"

Two patterns that need *different* attack sets must never land on the same index. Two that
need the *same* attack set may share one; that's a harmless, "constructive" collision.
Nobody knows a formula for such numbers, so we search: try random 64-bit numbers with few bits
set until one works for the square.

```rust,ignore
{{#include ../../crates/chess/src/magic.rs:find_magic}}
```

The search takes about half a second for all 128 square/piece pairs. That's too slow to run
every time the engine starts, because GUIs expect an instant reply. So the `find-magics` tool
runs the search once and writes the numbers to `magic/magic_numbers.rs`:

```text
cargo run --release -p cb2026-tools --bin find-magics
```

At startup cb2026 just fills in the tables for those numbers, about 4 ms, and checks each
number still works.

### "Fancy" tables

Each square needs a table of `2^(relevant squares)` entries: 4096 for a rook in a corner, but
only 1024 in the centre. Instead of 64 fixed-size tables, all of them share one array and
each square remembers its *offset*. Rooks and bishops together need 107,648 entries, about
840 KB.

## Testing it

Magic tables are easy to get subtly wrong, so the test is exhaustive. For every square, for
every one of its relevant occupancy patterns (plus some irrelevant pieces that mustn't change
anything), the magic lookup must equal stage 2's ray walk. That is 107,648 comparisons, and it
runs in under a second. The perft suites from stage 2 still pass at every depth.

## What it's worth

Perft speed on the full test suites (666 million positions), cloud machine, release build:

| Sliding attacks | Speed |
| --- | --- |
| Walking rays (stage 2) | 133 million positions/s |
| Magic bitboards | 235 million positions/s |

That's about 1.8× faster, and every later stage gets it for free, since search spends much of
its time generating moves and testing attacks. Still no Elo yet: there is nothing to search
with until stage 5.

## Try it

```text
> magic d4                    # step through the lookup for a rook and bishop on d4
> set sliders rays
> perft 5                     # note the speed
> set sliders magics
> perft 5                     # and compare
```

## Going further

- **PEXT bitboards.** CPUs with the BMI2 instruction set have `pext`, which does step 2's
  gathering of bits in a single instruction, with no magic numbers needed. It's slow on AMD
  Zen 1 and 2, so it would be an optional build. The PRD lists it as a could-have.
- **Smaller tables.** Better magic numbers can use fewer index bits than the number of relevant
  squares; dedicated searches have found many such "black magics".

## Further reading

- Chess Programming Wiki: [Magic Bitboards](https://www.chessprogramming.org/Magic_Bitboards),
  [Looking for Magics](https://www.chessprogramming.org/Looking_for_Magics) (Tord Romstad's
  public-domain search, which `find_magic` follows),
  [BMI2 / PEXT](https://www.chessprogramming.org/BMI2#PEXTBitboards).
