# NNUE

*Stage 21 · in progress · code in `crates/nnue`, `crates/tools/src/selfplay.rs`,
`crates/trainer`*

> This chapter is being written as stage 21 is built. So far it covers the network's
> definition (its inputs, its layers and its whole-number weights), its training data and the
> trainer. The engine's use of the network comes next, with what it's worth once it can be
> measured.

Chapters 15 and 16 built an evaluation from terms a person designed and tuned their weights on
cb2026's own games. An **NNUE** ("efficiently updatable neural network") replaces those terms
with a small neural network that learns what matters from millions of positions. Since 2020 it
has been the evaluation of almost every top engine. This chapter builds one from scratch, with a
trainer of cb2026's own, so every step can be followed.

## The idea

A neural network is layers of numbers. Each value in a layer is a weighted sum of the values in
the layer before, passed through a simple function (the **activation**). Training adjusts the
weights so the last layer predicts something: here, how good the position is.

Two things make this work for chess at engine speed:

1. **Inputs that barely change.** The network's inputs say which piece stands on which square.
   A move changes only a few of them, so the first layer, which does almost all of the work,
   can be updated by adding and subtracting a few rows of weights instead of being recomputed.
   That running first layer is the **accumulator**.
2. **Whole numbers.** The weights are rounded to integers, so the arithmetic is exact and the
   CPU can do many small integer operations at once.

## The inputs

An input is one fact, such as "a white knight stands on f3". There are 2 colours × 6 piece
types × 64 squares = 768, one on for each piece. The network reads them from both sides' points
of view: from a side's view its own pieces are "ours", and Black sees the board flipped top to
bottom. So the same weights describe White's knight on f3 for White and Black's knight on f6
for Black.

```rust,ignore
{{#include ../../crates/nnue/src/features.rs:index}}
```

None of these inputs depends on where a king stands, which keeps the network small and its
updates cheap: no move ever forces a full recomputation. Stronger networks add the king's square
("king buckets"); they need much more training data, and cb2026 will try them later, measured
like everything else.

A move switches off the inputs of the pieces it lifts and switches on those it puts down. A
capture also lifts the captured piece, en passant lifts a pawn beside the destination, and
castling lifts and puts down both the king and the rook. That is at most two off and two on:

```rust,ignore
{{#include ../../crates/nnue/src/features.rs:changes}}
```

## The layers

```text
  768 inputs ──► 256 hidden ─┐                 (from the side to move's view)
                             ├──► 1 output ──► centipawns
  768 inputs ──► 256 hidden ─┘                 (from the other side's view)
```

Each view has a hidden layer of 256 values. Both views use the same first-layer weights, which
the flipped board makes possible. The output layer takes the side to move's 256 values first,
then the other side's. A position and its mirror image (colours swapped, board flipped) give
exactly the same inputs in the same order, so they get exactly the same score, which a test
checks.

The activation clips each hidden value to the range 0 to 1 and squares it (**SCReLU**, the
squared clipped ReLU). Squaring lets the network weigh a large hidden value more than a middling
one. Plain clipping (**CReLU**) is also supported, so the two can be compared.

```rust,ignore
{{#include ../../crates/nnue/src/float.rs:output}}
```

That is the network as the trainer sees it, in floating point. A hidden layer of 256 means about
200,000 weights, a good size for the 100 million positions a first network trains on. The size
is stored in the network's file, so networks of 128 or 512 can be tried too.

## Whole numbers instead of floats

The engine runs the same network with whole-number weights: the first layer's weights times
255, the output weights times 64.

```rust,ignore
{{#include ../../crates/nnue/src/network.rs:scales}}
```

A hidden value of 1.0 becomes 255, so the clipping is a clamp to 0..255. The accumulator is
the first layer kept up to date move by move:

```rust,ignore
{{#include ../../crates/nnue/src/network.rs:accumulator}}
```

Integers can overflow, so the sizes are chosen to make that impossible. The trainer keeps
every weight within ±1.98, so a first-layer weight is at most 505 and an output weight at most
127. A legal position has at most 32 pieces, so a hidden value is at most 33 × 505 = 16,665 (the
32 pieces plus the bias), inside an `i16`'s 32,767. An activation times an output weight is at
most 255 × 127 = 32,385, which fits too.

```rust,ignore
{{#include ../../crates/nnue/src/network.rs:activation}}
```

Rounding costs a little accuracy. With the random weights the tests use, the integer network is
on average 2–4 centipawns from the floating-point one. Given the rounded weights, it computes
the same thing to within a centipawn.

## The output's scale

The output, times an **eval scale**, is in centipawns. The scale isn't arbitrary: it has to
match the hand-crafted evaluation's centipawns, because the search's pruning margins were all
tuned in those centipawns.

The trainer will fit the network to two things at once, the search score of each position and
the result of its game, turning scores into expected results with a sigmoid, `σ(cp / scale)`.
If the scale doesn't match how cb2026's centipawns actually relate to results, the two targets
pull against each other and the network's centipawns stretch. Chapter 16's Texel tuner fits
exactly this relation as its `K`: on 40,304 self-play positions it came out at 1.19, which is a
scale of 400 / (1.19 × ln 10) ≈ 146. Many engines use 400; cb2026's centipawns are steeper.

```rust,ignore
{{#include ../../crates/nnue/src/network.rs:evaluate}}
```

## The network file

A network is stored as a 48-byte header followed by the numbers. The header says everything
about the network's shape (inputs, hidden size, activation, scales) and carries a checksum, so a
damaged or foreign file is refused before it is used. Each input's weights are one row, so
switching an input on or off adds one row to the accumulator. Networks aren't committed to the
repository: they are published with the releases.

## The training data

The network learns from positions of cb2026's own games, the same self-play games as chapter
16's tuner, played at a fixed number of nodes per move. The tuner only needed each position and
its game's result. The trainer also wants the search's score, and since producing data takes the
test Mac about 12 hours per 100 million positions, the format should keep anything a later
network might want. Each position becomes a **record** of 32 bytes:

| Bytes | Field |
| --- | --- |
| 0–7 | which squares are occupied (a bitboard) |
| 8–23 | a 4-bit code for each occupied square's piece, in square order |
| 24 | the side to move and the en passant square |
| 25 | the halfmove clock |
| 26–27 | the search's score, in centipawns from White's side |
| 28 | the game's result |
| 29–30 | the best move |
| 31 | the format's version |

A position has at most 32 pieces, so 32 four-bit codes, 16 bytes, hold them all. The codes are
the piece types plus 8 for Black, and one more: code 6 is a rook that can still castle. That is
how a record keeps castling rights, and it works for Chess960, where a right belongs to one
particular rook. The layout follows marlinformat, the format of the trainer of the engine
Marlin.

```rust,ignore
{{#include ../../crates/nnue/src/record.rs:from_position}}
```

The record keeps the position itself, not the network's inputs. The trainer turns the pieces
into inputs as it reads them, so a later network with different inputs, such as king buckets,
can learn from the same records without generating them again:

```rust,ignore
{{#include ../../crates/nnue/src/record.rs:pieces}}
```

### Balanced starts

Each game starts from one of 50 balanced openings plus 8 random moves, so that no two games are
alike. Random moves are often bad ones, though, and many games began with one side already far
ahead. In 600 games, only 18.5% of the recorded positions were within a pawn of level and 37.5%
were more than five pawns from it; White won 54% of the games.

So when writing records, `datagen` searches each start like a move of the game and draws the
random moves again while the start scores beyond 200 centipawns:

```rust,ignore
{{#include ../../crates/tools/src/selfplay.rs:balanced_start}}
```

| Largest start score | Within ±100 cp | Beyond ±500 cp | White / draw / Black |
| --- | --- | --- | --- |
| no limit | 18.5% | 37.5% | 54 / 17 / 29% |
| 300 cp | 24.4% | 27.9% | 45 / 21 / 34% |
| 200 cp | 30.0% | 25.3% | 41 / 23 / 37% |

A quarter of the positions are still far from level, because games at a few thousand nodes a
move do get decided, and a network has to judge won positions too. What the limit removes is
games that were lopsided from their first move. It costs nothing: balanced games last longer
and give more positions each, which pays for the extra search. The tuner's text output keeps
every start, so chapter 16's runs can still be repeated.

`datagen --format records` writes records, and `records` checks a file and shows what is in
it. Sixty games make about 4,100 records, 132 KB; 100 million records take 3.2 GB. Records have
no file header, so files from separate runs can simply be joined.

## The trainer

The trainer (`crates/trainer`) learns the weights. It uses no machine-learning library: every
gradient is worked out by hand, so each step can be read.

### What it learns to predict

The network's output `y` means `y × eval scale` centipawns, and `σ(y)` the expected result for
the side to move. Each record gives two estimates of what `σ(y)` should be: the search's score,
`σ(score / eval scale)`, which is precise but sometimes wrong, and the game's result, which is
right about the game but noisy about any one position in it. The **target** blends them, a
quarter result by default:

```rust,ignore
{{#include ../../crates/trainer/src/example.rs:example}}
```

The eval scale comes from the data, fitted as chapter 16 fitted `K`: on 134,000 records with
balanced starts it came out at 159, close to the 146 found earlier.

### Forward, loss, backward

The forward pass is the network itself, run on the example's inputs:

```rust,ignore
{{#include ../../crates/trainer/src/backprop.rs:forward}}
```

The loss is the squared difference between `σ(y)` and the target. Training needs its
**gradient**: for every weight, how much the loss changes as that weight changes. The chain
rule gives it, working back from the output. The loss's slope with respect to `y` is
`2 (σ(y) − target) σ(y) (1 − σ(y))`:

```rust,ignore
{{#include ../../crates/trainer/src/backprop.rs:loss}}
```

From there, an output weight's gradient is that slope times its hidden value's activation, and
a hidden value's is the slope times its output weight times the activation's own slope, which is
zero wherever the clipping holds the value at 0 or 1. A first-layer weight collects its hidden
value's gradient once for each view in which its input is on: only about 32 of the 768 rows
change for any one position.

```rust,ignore
{{#include ../../crates/trainer/src/backprop.rs:backward}}
```

Hand-written gradients need checking. The test nudges weights a little each way, measures how
the loss moves, and compares that with the formulas: they agree to 0.1%. The check itself
taught two lessons. Near the clipping's corners a nudge measures the wrong slope, so those
hidden values are left out. And in `f32`, σ(y) near ½ is only exact to about 6 × 10⁻⁸, a few
percent of the change a small nudge makes, so the test measures the loss in `f64`.

### Stepping: AdamW

The gradients of a batch of records are averaged, and **AdamW** moves each weight against its
gradient, as chapter 16's tuner did. It keeps a running average of each weight's gradient and
of its square, and steps by about their ratio, so rarely used inputs learn as fast as common
ones. It also pulls every weight a little towards zero (weight decay), and afterwards clips it
to ±1.98, the limit the whole-number network relies on:

```rust,ignore
{{#include ../../crates/trainer/src/adam.rs:step}}
```

### The loop

Each **epoch** visits every training record once in a new random order, in batches of 16,384.
A batch is shared out among the CPU's threads, and their gradients are added up in a fixed
order, so the same settings always give the same network. The learning rate drops to a tenth
for the last quarter of the epochs. After each epoch the trainer writes the network, with
whole-number weights, as a file the engine can read, and a checkpoint to resume from.

The last 1% of the records, whole games the training never sees, are the **validation set**.
The training loss shows how well the network fits the positions it learns from; the validation
loss shows how well it has learnt chess. A test makes the difference plain. A student network
learns the scores of a random "teacher" network of the same shape: on 1,920 positions its
training loss falls 200-fold, but on unseen positions it is only 38% better than guessing the
average, and with 576 positions no better at all. It has learnt the positions by heart. Only
many positions teach a network chess, which is why a first one needs about 100 million.

On the M1 Max, with 8 threads, the trainer handles about 2.4 million records a second, so an
epoch of 100 million records takes about 45 seconds and 20 epochs about 15 minutes. Generating
those records takes about 12 hours: data, not training, is what takes the time.

## Pitfalls

- **Trainer and engine must agree** on the numbering, the layers and the scales. They share
  `crates/nnue`, and tests pin it down: every input has its own number, a move's changes turn
  one position's inputs into the next one's, updates match recomputing, and mirrored positions
  score the same.
- **An eval scale that doesn't match** quietly changes every margin in the search.
- **Overflow** in the integer arithmetic gives silently wrong scores. The weight limit and the
  bounds above rule it out, and a test runs the largest possible weights.
- **Scores from the wrong side.** The search scores for the side to move; records keep White's
  side, like the result, and the trainer turns both to the side to move's view. Mixing the two
  up teaches the network nonsense for every position with Black to move.

## Further reading

- Chess Programming Wiki: [NNUE](https://www.chessprogramming.org/NNUE).
- Yu Nasu, *Efficiently Updatable Neural-Network-based Evaluation Functions for Computer
  Shogi* (2018), which introduced NNUE.
