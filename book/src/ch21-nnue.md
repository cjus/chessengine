# NNUE

*Stage 21 · in progress · code in `crates/nnue`, `crates/tools/src/selfplay.rs`,
`crates/trainer`, `crates/engine/src/search.rs`*

> This chapter is being written as stage 21 is built. So far it covers the network's
> definition (its inputs, its layers and its whole-number weights), its training data, the
> trainer and the network in the search. What it's worth comes once a network has been trained
> on the full data and measured.

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
switching an input on or off adds one row to the accumulator. The default network is part of
the source, in `nets/`, and each release carries it as a download too; other networks aren't
kept in the repository.

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

That makes a 12-hour run safe to interrupt. `scripts/mac-nnue-data.sh` makes the data in chunks
of 56,000 games, each its own file, and the trainer reads them all (`--data` once per file). A
run that stops loses only the chunk it was on: started again, it keeps the finished files and
makes the rest. Each game's random opening moves come from its number, and chunk k's numbers
start where chunk k − 1's ended, so the chunks never repeat a game, and a chunk made twice holds
the same games.

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

On the M1 Max, with its 8 performance cores, the trainer handles about 2 to 2.4 million
records a second, so an epoch of 100 million records takes under a minute and 20 epochs under
20 minutes. Generating those records takes about 12 hours: data, not training, is what takes the
time. Every thread gets an equal share of each batch and the batch waits for the slowest, so the
trainer leaves the efficiency cores out: with them, 10 threads were 17% slower than 8.

### On the GPU

The same training runs on the Mac's GPU with `--backend gpu` (in a trainer built with
`--features gpu`). It goes through wgpu, a Rust library for WebGPU, which runs on Metal on the
Mac and on Vulkan or DirectX 12 elsewhere, with an API that is safe Rust. The code that runs on
the GPU, a **kernel**, is written in WGSL, WebGPU's shading language. Thousands of copies of a
kernel, its **invocations**, run at once, each computing one number, such as one hidden value of
one example, in **workgroups** of 64 that the GPU schedules together. A training step is a row
of kernels, each one step of the formulas above: the hidden layer, the output and loss, the
hidden values' slopes, the gradients, AdamW, the loss's running total. The network and AdamW's
averages stay on the GPU for the whole run; only each batch's inputs and targets go up, and the
network comes back once an epoch, for the file and the checkpoint. The CPU and GPU versions sit
behind one interface, `Backend`, with one call per batch, so the loop above drives both.

A GPU is fast at work that splits into many equal, independent pieces, and slow at anything
else. Two parts of the backward pass weren't like that at first.

**Sums over the batch.** An output weight's gradient is a sum over all 16,384 examples. With one
invocation per weight looping over the whole batch, a network of 256 had only 513 invocations
for a GPU that runs tens of thousands, and those sums took 25 of a batch's 35 milliseconds. So
the batch is cut into chunks of 128 examples: one kernel sums each weight over each chunk, and a
second adds up the chunks.

**The sparse gradient.** A first-layer weight collects a term from every example in which its
input is on. On the CPU, each thread adds its examples' terms into its own copy of the
gradients. On the GPU, thousands of invocations would be adding into the same weights at once,
which needs atomic addition, and WGSL has none for floats. So the CPU sorts each batch's rows by
input first, and each weight adds up its own list with nobody else writing to it. The sort is a
counting sort: count each input's rows, turn the counts into starting places, then put each row
in its place:

```rust,ignore
{{#include ../../crates/trainer/src/gpu.rs:count_sort}}
```

The lists turned out to be very uneven: in a batch the average input is on in about 700 of the
32,768 rows, but a king on its first square is on in over 11,000. A kernel is as slow as its
slowest invocation, so one invocation per weight, looping over its list, made this kernel as
slow as its longest list, whatever the network's size: a network of 512 trained no faster than
one of 1,024. So the lists are also cut into segments of up to 256 rows. One kernel sums each
segment for every hidden value, and a second adds up each input's segments:

```wgsl
{{#include ../../crates/trainer/src/gpu/input_partials.wgsl}}
```

That doubled the speed of networks of 512. Every sum runs in a fixed order, so the same GPU
gives the same network every time, but not quite the CPU's: floats added in another order round
differently. The tests run both on the same batch and compare the gradients, an AdamW step and a
short training run; breaking the segments on purpose makes the gradients' test fail.

Measured on the M1 Max (32 GPU cores) on 2.8 million records, in records a second:

| Hidden layer | CPU, 8 threads | GPU |
| --- | --- | --- |
| 256 | 2.3 million | 5.3 million |
| 512 | 1.1 million | 3.0 million |
| 1,024 | 0.54 million | 1.4 million |

The training losses were the same to six decimals. The CPU's time grows with the network, since
it does every multiplication itself. The GPU's grows more slowly. At first it didn't even set
the pace at 256: the CPU prepared each batch, decoding the records and sorting the rows, on one
thread in about 4 milliseconds, while the GPU's work took 3.5, so the GPU waited. Now each of the
trainer's threads prepares a share of the batch, and the shares are joined input by input, in
order, so the lists come out exactly as one thread would make them:

```rust,ignore
{{#include ../../crates/trainer/src/gpu.rs:group_by_input}}
```

On 8 threads the preparation takes 0.8 milliseconds, and training at 256 went from 2.7 to 5.3
million records a second, with the same losses. The GPU's own work is now the limit at every
size.

Measuring had a trap of its own. On a file of 141,000 records the GPU seemed no faster than the
CPU at 256. An epoch was only nine batches there, and the trainer reads the loss back 20 times
an epoch for its progress line, so after every batch the CPU waited for the GPU to finish, and
the two never worked at the same time. A real run has thousands of batches an epoch. A
benchmark must look like the run it stands for.

### Other Macs

The numbers belong to one Mac. The M3 Max, where cb2026's long runs go, differs from the M1 Max
in each of the things that set them. Its performance cores are faster, and it has 10 or 12 of
them rather than 8, so the CPU trainer speeds up.
Its GPU is a newer design, with 30 or 40 cores rather than 24 or 32, that gives each running
kernel only the on-chip memory it uses rather than the most it might need, so more invocations
fit at once (Apple calls this Dynamic Caching). Its memory bandwidth is 300 or 400 GB/s,
depending on the model, against the M1 Max's 400, and the sparse gradient's kernels do little
arithmetic for each number they read, so memory is likely to bound them. So the GPU's lead,
2.3 to 2.8 times on the M1 Max, could grow or shrink there. `scripts/mac-trainer-speed.sh` measures both backends on the Mac it runs
on, and its numbers, not these, should pick the backend for a long run. A GPU of another design
may also round a little differently, so a network is only repeated exactly on the same machine,
backend and thread count.

## The network in the search

The search evaluates millions of positions a second, each a move away from one it has just
seen. So each search thread keeps a stack of accumulators, one per ply: the one at ply `p`
belongs to the position `p` moves from the root. Wherever the search is about to look at a
child, it builds the child's accumulator from its own with the move's changes; for a null move,
which moves no piece, it copies it. The root's is computed from scratch at the start of each
search.

```rust,ignore
{{#include ../../crates/nnue/src/network.rs:update_into}}
```

```rust,ignore
{{#include ../../crates/engine/src/search.rs:nnue_search}}
```

Getting this right everywhere is the hard part. The search makes moves in five places: the
main move loop, null-move pruning, ProbCut, the quiescence search and the root. Miss one and
the network quietly evaluates the wrong position. So in a debug build every evaluation
compares the accumulator it was handed with one computed from scratch, and a test searches
positions with castling, en passant, promotions, Chess960 castling and checks. Leaving out the
null move's copy, or the quiescence search's update, fails it at once.

The network's score is clamped to ±20,000 centipawns, so that nothing it computes could be
taken for a tablebase win or a mate. Correction history (chapter 14.2) corrects it just as it
corrects the hand-crafted evaluation.

Since the first network passed its test (below), it is built in and used by default. The UCI
option `EvalFile`, the shell's `set evalfile` and `cb2026 bench NET` load another; `none`, or
the `nnue` switch, goes back to the hand-crafted evaluation, and then the bench is what it was
before networks, to the node.

### A lesson from a random network

The first test searched with a network of random weights, and on one position, Kiwipete, it
took 11 million nodes to reach depth 4, where the hand-crafted evaluation needs 5,000. The
accumulators were right. The evaluation was the problem: random scores tell the quiescence
search nothing, so almost no capture sequence is cut off early, and Kiwipete is full of
captures. A network built by hand to count material, a sensible evaluation if a simple one,
searched 26,000. A network trained on only 134,000 positions searches the bench in 4.6 times as
many nodes as the hand-crafted evaluation. Pruning is only as good as the evaluation it trusts.

Each node costs about 5% more with a 256-wide network than with the hand-crafted evaluation:
1.50 million nodes a second against 1.57 million on the M1 Max, in plain Rust that the
compiler vectorises.

## The first network

The first network's data took the M3 Max 12.4 hours: 1,344,000 games of the hand-crafted
evaluation against itself, on 9 threads, gave 104 million positions, 3.3 GB. White won 40.5% of
the games, 23.8% were drawn and Black won 35.6%, and 30% of the scores were within a pawn of
level, thanks to the balanced starts. Training took 7 minutes on the M3 Max's GPU for a network
of 256 and 13 minutes for one of 512.

The losses tell how it went. The 256 network's validation loss fell from 0.0103 after the first
epoch to 0.0094 after the twentieth, with a visible step at the sixteenth, where the learning
rate drops. Its training loss ended at 0.0090, close to the validation loss: it learnt chess, not
the positions. (The 134,000-position sample above ended at 0.047.) The 512 network did a little
better on unseen games, 0.0093, but its training loss pulled further away, 0.0086: with 100
million positions, a wider network starts to learn the positions by heart.

Then the checks that a network is sane before it plays a game:

| Position (side to move) | Hand-crafted | 256 | 512 |
| --- | --- | --- | --- |
| The starting position | +10 | +24 | +15 |
| White without its queen | −931 | −819 | −785 |
| White without a knight | −269 | −227 | −236 |
| White without a pawn | −52 | −19 | −44 |
| A pawn on the 7th, kings far away | +290 | +752 | +745 |
| King and bishop against king, a dead draw | +269 | +148 | +149 |

The material is roughly where the hand-crafted evaluation puts it, the pawn about to promote is
worth far more to the networks than to the hand-crafted evaluation, and every position mirrored
(the colours swapped and the board turned round) scores exactly the same. The bench, the other
side of the lesson above: with the 256 network the search needs 716,942 nodes, fewer than the
hand-crafted evaluation's 749,181, where the weak networks needed several times as many. A good
evaluation prunes well.

The first games: 200 at one second each plus 0.01 a move, the 256 network against the
hand-crafted evaluation, ended 172 wins, 4 losses and 24 draws, about +424 Elo ± 71. Games that
fast exaggerate any difference in evaluation, so that is a first sign rather than a measurement;
the SPRT at the usual 8 seconds plus 0.08 is what counts. It passed after 14 games, all of them
won: the log-likelihood ratio reached 3.21, beyond the 2.94 that accepts the gain. A result that
lopsided proves the network stronger but not by how much: with no losses or draws there is
nothing to measure a difference from, and that takes a fixed number of games. Stage 9's
piece-square tables passed the same way, 14 to 0.

## The network by default

Passing made the network the default. Its file is part of the source (395 KB, in `nets/`, with
a note of how it was made), and the build puts its bytes into the binary, where the engine reads
them the first time it needs them:

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:builtin}}
```

`EvalFile` now shows `<built-in>`. Empty, or `none`, means the hand-crafted evaluation, and a
path another network; the shell's `set evalfile`, `cb2026 bench` and `datagen --evalfile` read
the same words, decided in one place (`EvalChoice`). Two kinds of position keep the
hand-crafted evaluation even so.

**The strength levels.** Chapter 19 calibrated them with the hand-crafted evaluation. With the
network the same node limits would play stronger, and the ratings would be wrong, so below full
strength the search keeps the evaluation they were measured with, until they are calibrated
again.

**A lone king.** Making the network the default broke one test: king and queen against king,
played at 3,000 nodes a move, with which the self-play tools check that they record a won game.
The network drew it. Five such endings, each played out by both evaluations:

| Nodes a move | Hand-crafted wins | Network wins |
| --- | --- | --- |
| 3,000 | 5 of 5 | 0 of 5 |
| 20,000 | 5 of 5 | 2 of 5 |
| 100,000 | 5 of 5 | 4 of 5 |
| 500,000 | 5 of 5 | 5 of 5, more slowly |

To mate a lone king, the side ahead drives it to the edge, and the hand-crafted evaluation
rewards that, through its king tables and mobility. The network was trained on balanced games,
where a bare king is rare, and it scores those positions much alike: a big lead, whatever the
kings do. So the search shuffles until a repetition or the fifty-move rule ends the game. A deep
search finds the mate anyway, which is why the games at 8 seconds never showed it, but fast games
and shallow analysis don't. Stockfish kept special evaluations for such endings beside its
networks for years. Here, when exactly one side has only its king, the search uses the
hand-crafted evaluation (the `loneking` switch), and the network wins all five at 3,000 nodes,
as quickly as the hand-crafted evaluation.

**Small pawnless endings.** The lone king wasn't the only gap. Before v1.0.0 was tagged, a test
of endgames played out won positions, each confirmed by the Lichess tablebases, with cb2026 on
both sides and no tablebases of its own, once with each evaluation. Where the defender keeps a
pawn, or a rook against a rook and pawn, the network converted as often as the hand-crafted
evaluation or more. With a queen against a single piece it failed, at 200,000 nodes a move,
about what a game at 8+0.08 gives a move:

| Ending | Network | Hand-crafted |
| --- | --- | --- |
| K+Q against K+R (45 positions) | 15 | 32 |
| K+Q against K+B (15) | 12 | 15 |
| K+Q against K+N (15) | 9 | 15 |

The cause is the lone king's again. A queen ahead is a big lead to the network wherever the
kings stand, and nothing in its few such training positions showed it how to make progress, so
nearly every failure was a draw by the fifty-move rule. A queen against a rook is hard for the
hand-crafted evaluation too, since the rook can harass the king for many moves, but its king
terms push the defending king towards the edge, and that is enough most of the time. So the
`pawnless` switch widens the rule: in a pawnless ending of at most five men (three pieces
besides the kings, `PAWNLESS_ENDING_PIECES`), the search uses the hand-crafted evaluation.
Those are the endings the common Syzygy tables solve, so an engine with tablebases plays them
perfectly with either evaluation. With the switch, the network converts exactly what the
hand-crafted evaluation does, 32, 15 and 15, since the search is the same. Its SPRT, from 100
such endings, wins and draws (`tests/data/pawnless-endings.epd`), passed at +39 ± 16 Elo in
1,195 games, and 1,000 games from the usual openings showed nothing else changed, +1.7 ± 15.3
(cjus/chessengine#208).

The bench now searches with the network: 715,827 nodes, against the hand-crafted evaluation's
749,181. A few of its positions reach a bare king or a small pawnless ending deep in the
search, where the hand-crafted evaluation takes over. Every commit message's `Bench:` line
changes with it.

A new evaluation is a new way for the engine to fail: an accumulator updated at every move, and
nodes about 6% slower, so less time to spare. Before v1.0.0 was tagged, the release's robustness
run was played again with the network: 10,000 games against itself, 6,000 of them at 1+0.01, with
no time losses, illegal moves, failures or aborted games.

## Pitfalls

- **Trainer and engine must agree** on the numbering, the layers and the scales. They share
  `crates/nnue`, and tests pin it down: every input has its own number, a move's changes turn
  one position's inputs into the next one's, updates match recomputing, and mirrored positions
  score the same.
- **An eval scale that doesn't match** quietly changes every margin in the search.
- **Overflow** in the integer arithmetic gives silently wrong scores. The weight limit and the
  bounds above rule it out, and a test runs the largest possible weights.
- **Endings the data never showed.** A network learns only what its positions show. Balanced
  self-play has few bare kings, so the first network couldn't drive one to the edge. Test the
  endings every beginner must win, king and queen or rook against king, at low node counts,
  where the search can't cover the gap.
- **A network that quietly doesn't load.** An engine that can't read its `EvalFile` says so
  in an `info string` and plays on with the hand-crafted evaluation, and a match tool ignores
  info strings, so a test of the network would measure the old evaluation against itself.
  `mac-sprt.sh` loads each network through UCI before the match and stops if it doesn't load.
- **A move the accumulators miss.** Every place the search makes a move must update the
  child's accumulator; a debug build checks each evaluation.
- **A GPU kernel is as slow as its slowest invocation.** One invocation per weight looks
  natural, but if some weights have far more work than others (the kings' inputs in a batch),
  the GPU sits idle while they finish. Cut the work into equal pieces and add them up after.
- **A benchmark that doesn't look like the real run.** A small file made the GPU wait at every
  progress report and look slower than it is.
- **Scores from the wrong side.** The search scores for the side to move; records keep White's
  side, like the result, and the trainer turns both to the side to move's view. Mixing the two
  up teaches the network nonsense for every position with Black to move.

## Further reading

- Chess Programming Wiki: [NNUE](https://www.chessprogramming.org/NNUE).
- [WGSL](https://www.w3.org/TR/WGSL/), WebGPU's shading language, and
  [wgpu](https://wgpu.rs), the Rust library the trainer's GPU backend runs on.
- Yu Nasu, *Efficiently Updatable Neural-Network-based Evaluation Functions for Computer
  Shogi* (2018), which introduced NNUE.
