# Texel tuning on self-play games

*Stage 16 · tag `stage-16-tuning` · code in `crates/engine/src/tuning.rs`,
`crates/tools/src/selfplay.rs`, `crates/tools/src/texel.rs`*

Chapters 9 and 15 gave cb2026 an evaluation with 411 pairs of numbers: piece values, 384
piece-square table entries, and the positional terms. Every one was set by hand, from chess
principles and a feel for what's reasonable. Hand-setting gets the signs right and the
magnitudes roughly right. It can't get hundreds of numbers right *together*, since they
interact (a passed-pawn bonus and the pawn table both reward advanced pawns). This chapter
lets the games decide.

## The idea

An evaluation predicts the outcome of a game. Positions it scores +2 pawns should usually be
won, positions near 0 should often be drawn. Given a large set of positions from real games,
each labelled with the game's result, we can measure how well the evaluation predicts, and
adjust the numbers to predict better. This is **Texel tuning**, named after Peter
Österlund's engine Texel, which made it popular around 2014.

1. Turn a score into an expected result with a sigmoid:
   `σ(s) = 1 / (1 + 10^(−K·s/400))`. A score of 0 expects ½, large scores expect 1 or 0.
2. The **error** is the mean squared difference between expected and actual results.
3. Fit `K` once, with the starting values. It only sets the scale between centipawns and
   results.
4. Adjust the parameters to minimise the error.

```rust,ignore
{{#include ../../crates/tools/src/texel.rs:error}}
```

## The data: cb2026's own games

The positions come from cb2026 playing itself, so no other engine's games or data are used.
The `datagen` tool plays thousands of short games:

- each game starts from an opening position plus 8 random moves, so no two games are alike;
- each move gets a fixed 6,000-node search, which is fast and gives the same games on any
  machine;
- a game ends by the rules, or is **adjudicated** once the search sees a forced mate, or as a
  draw when the score stays near zero for long enough.

From each game it keeps the **quiet** positions: not in check, best move not a capture or
promotion, no mate in sight. The evaluation is only ever asked about quiet positions, because
the quiescence search resolves captures first, so those are the ones it should be fitted to.

```rust,ignore
{{#include ../../crates/tools/src/selfplay.rs:play_game}}
```

## The evaluation as a dot product

Tuning hundreds of parameters on hundreds of thousands of positions needs the error's
**gradient**: for each parameter, which direction reduces the error. Computing that by trying
each parameter one step up and down (the original Texel method) means hundreds of full
passes over the data per step. There is a much faster way.

Every term of cb2026's evaluation has the form "this parameter, this many times, for this
side". The pawn on e4 is "pawn piece-square entry e4, once, for White". A knight with 6 safe
squares is "knight mobility, 6 − typical times". So the evaluation can report its terms to
any **sink** instead of adding them up:

```rust,ignore
{{#include ../../crates/engine/src/eval.rs:param}}
```

The engine's own evaluation is one sink: it multiplies each term by its value and adds it up.
The tuner's sink records **coefficients** instead: for each parameter, how many times White
gets it minus how many times Black does. After that, the middlegame and endgame scores are
simple sums of coefficient × value, and the derivative of the score with respect to a
parameter is just its coefficient (times its share of the phase blend). The engine and the
tuner share one implementation of the terms, so they can't drift apart, and a test checks
that the coefficients reproduce the evaluation exactly.

King danger is the one term that isn't linear: it is the *square* of the attack count. The
tuner treats it as a constant computed with the current values and doesn't tune it.

## Fitting: gradient descent with Adam

```rust,ignore
{{#include ../../crates/tools/src/texel.rs:gradient}}
```

Plain gradient descent struggles here. Material appears in every position, while "knight
on a8" appears in a handful, so their gradients differ by orders of magnitude, and one step
size can't suit both. **Adam** keeps running averages of each parameter's gradient and of its
square, and scales each parameter's step by them. Rare and common parameters then move at
similar rates.

```rust,ignore
{{#include ../../crates/tools/src/texel.rs:adam}}
```

A unit test checks the gradient against finite differences, which is the standard guard
against a sign or factor error in hand-derived gradients.

## Running it

```text
cargo run --release -p cb2026-tools --bin datagen -- --out data.txt --games 8000 --seed 2026
cargo run --release -p cb2026-tools --bin datagen -- --out data2.txt --games 16000 --seed 3033
cat data.txt data2.txt > data-all.txt
cargo run --release -p cb2026-tools --bin tune -- --data data-all.txt --from hand \
    --epochs 1500 --lambda 3e-9 --min-count 500
```

`tune` writes `crates/engine/src/tuned.rs`, a generated table of all 411 values, available
as `EvalParams::tuned()` (and in the shell with `set eval tuned`). The engine's default is
still the hand-written set, `EvalParams::hand_set()`: the tuned values didn't measure stronger
(see "What it's worth" below). The data file isn't committed (it's large, and
anyone can regenerate it).

## What happened: overfitting, then regularisation

The first run used 498,522 positions from 8,000 games and tuned all parameters freely for
3,000 epochs. The error dropped from 0.0845 to 0.0798, which looked like a success. The
values said otherwise:

| Parameter | Hand-set (mg, eg) | First run (mg, eg) |
| --- | ---: | ---: |
| Knight value | 320, 300 | 346, 321 |
| Knight on g8 | −35, −35 | **−530, +321** |
| White king on c8 | −50, −25 | **+1246, −221** |

Material moved sensibly. The piece-square entries for squares a piece almost never visits went
wild, because Adam moves every parameter at a similar rate whether a hundred positions
support it or a hundred thousand. Those numbers fit the training positions and mean nothing
anywhere else. This is **overfitting**.

Two standard remedies fixed it:

- **A minimum number of occurrences.** A parameter seen in fewer than 500 positions keeps its
  starting value.
- **L2 regularisation.** Each parameter is pulled back toward its starting value with
  strength λ, so it only moves as far as the evidence pushes it.

```rust,ignore
{{#include ../../crates/tools/src/texel.rs:regularize}}
```

To choose λ honestly, the last 10% of the data (about 50,000 positions, mostly from whole
games not used for training) is held out as a **validation set**. Its error shows whether a
fit generalises:

| λ | Training error | Validation error |
| --- | ---: | ---: |
| (hand-set values) | 0.08432 | 0.08570 |
| 0 (no regularisation) | 0.07937 | 0.08704 |
| 2 × 10⁻⁹ | 0.08014 | 0.08546 |
| 1 × 10⁻⁸ | 0.08099 | 0.08512 |
| **3 × 10⁻⁸** | 0.08166 | **0.08508** |
| 1 × 10⁻⁷ | 0.08237 | 0.08514 |

Without regularisation, the training error fell the most but the validation error *rose*:
the fit made predictions on new games worse. The best λ improved validation by only 0.7%.
500,000 positions from 8,000 games are not much evidence for 822 numbers.

So a second batch of 16,000 games was generated (a different random seed), for 1.5 million
positions in all, and the comparison repeated:

| λ | Training error | Validation error |
| --- | ---: | ---: |
| (hand-set values) | 0.08522 | 0.08467 |
| 0 | 0.08212 | 0.08368 |
| 1 × 10⁻⁹ | 0.08238 | 0.08333 |
| **3 × 10⁻⁹** | 0.08260 | **0.08328** |
| 1 × 10⁻⁸ | 0.08294 | 0.08335 |
| 3 × 10⁻⁸ | 0.08330 | 0.08351 |

With three times the data, even the unregularised fit now generalises. The best validation
gain doubles to 1.7%, and the best λ is ten times weaker, because the data can now justify
larger moves on its own. That's the usual pattern: regularisation matters most when data is
scarce. The values in `tuned.rs` come from the λ = 3 × 10⁻⁹ run.

## Pitfalls

- **Overfitting.** With few positions, rare parameters get fitted to noise, as the first run
  above shows. Hold out a validation set, regularise, and get more data.
- **Correlated data.** Positions from the same game share its result, so 500,000 positions
  from 8,000 games carry less information than 500,000 independent ones.
- **The engine that made the data.** The games were played by the engine being tuned.
  Repeating the cycle (generate with the tuned engine, tune again) usually helps a little
  more.
- **Error isn't Elo.** A lower error is a good sign, not proof. Only games decide, so the
  tuned values must pass an SPRT like any other change.

## What it's worth

| Test | Result |
| --- | --- |
| Tuned vs hand-set values, 8+0.08, SPRT [0, 5], cloud | +310 −313 =187 over 810 games, −1.3 ± 21.0 Elo: no verdict (LLR −0.17) when the run hit the session's two-hour limit |

No measurable gain. The 95% interval runs from about −22 to +20 Elo, so the tuned values might
be a little better or a little worse, but they aren't the clear win the lower error promised.
That is the "error isn't Elo" pitfall above, met for real. Likely causes:

- **Too little data.** About 1.5 million positions from 24,000 short games is small for 411
  parameters; strong engines tune on hundreds of millions.
- **Strong regularisation.** The settings that stopped the overfitting also kept most values
  close to the hand-set ones, so the evaluation barely changed.
- **The hand-set values were reasonable.** Tuning helps most when the starting point is poor.

So the engine keeps the hand-set values, and this stage contributes the tools: self-play data
generation, the tuner, and a tuned set anyone can try (`set eval tuned`, or an SPRT of their
own). A later round, with more data from the stronger engine of later stages, can try again.
With the hand-set values as default the stage changes nothing in play: the bench is the same
as stage 15's.

## Further reading

- Chess Programming Wiki: [Texel's Tuning Method](https://www.chessprogramming.org/Texel%27s_Tuning_Method),
  [Automated Tuning](https://www.chessprogramming.org/Automated_Tuning).
- Peter Österlund, the original description of the method on the TalkChess forum (2014).
- Diederik Kingma and Jimmy Ba, "Adam: A Method for Stochastic Optimization", ICLR 2015.
