# Lazy SMP

*Stage 17 · tag `stage-17-smp` · code in `crates/engine/src/lib.rs` and `tt.rs`*

Every computer that runs cb2026 has several cores, and until now it used one. Splitting an
alpha-beta search across threads is famously hard: the algorithm is sequential at heart,
since what one move's search learns (alpha) decides how much of the next move's tree can be
skipped. Decades of clever schemes divided the tree between threads and synchronised them
carefully. The method nearly every engine uses today is the opposite of clever. It is called
**Lazy SMP**.

## The idea

Start several threads, and let every one of them search the *same* position with the same
iterative deepening. They share exactly one thing: the transposition table.

This sounds like it should waste all but one thread's work, and it would without the shared
table. But the threads never stay in step. Each finds results the others haven't reached yet
and writes them to the table, and the others then read them and get cutoffs and good moves
for free. The threads end up exploring slightly different parts of the tree, filling the table
for each other. The main thread decides when to stop and which move to play. The helpers'
work only reaches it through the table.

```rust,ignore
{{#include ../../crates/engine/src/lib.rs:helper}}
```

Half the helpers start one ply deeper than the others. That small push keeps the threads from
searching the same depth at the same moment, which increases how much they help each other.

## A table for many threads

The transposition table now has several writers at once, so it has to cope with two threads
writing the same slot at the same time. A lock per slot would be slow. cb2026 uses the
**lockless hashing** trick of Hyatt and Mann instead:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:slot}}
```

Each entry is packed into one 64-bit word, and the slot stores that word alongside
`key XOR word`:

```rust,ignore
{{#include ../../crates/engine/src/tt.rs:pack}}
```

A reader loads both words and XORs them back. If another thread was halfway through writing
the slot, the reader gets one word from each write, the XOR doesn't give back the key, and the
entry is treated as a miss. A torn entry can never be mistaken for a real one, and no thread
ever waits. The words are Rust atomics, so all of this is safe code: no `unsafe` is needed.
The stored move is still checked for legality before it is played, as before.

## In practice

`setoption name Threads value 4` in UCI, or `set threads 4` in the shell. With one thread
(the default) the search is exactly the single-threaded one from the previous chapters.

A rough check on the cloud machine, with Kiwipete and 3 seconds per search (another test was
using the other cores, so only the ratio means anything):

| Threads | Depth | Nodes per second |
| ---: | ---: | ---: |
| 1 | 15 | 1.18 million |
| 2 | 16 | 2.19 million |

Twice the threads gives almost twice the nodes per second, but not twice the useful work:
much of the helpers' effort overlaps with the main thread's. Lazy SMP is usually measured
in Elo against a one-thread copy of the same engine, playing on otherwise idle cores. That
needs a machine with more cores than the cloud sessions have, so it runs on the test Mac
(`scripts/mac-smp.sh`).

## Pitfalls

- **Non-determinism.** With more than one thread, two runs of the same search can differ, since
  thread timing decides who writes the table first. The bench and every test that compares node
  counts use one thread.
- **Node counts.** Each helper adds its nodes to a shared counter, so `info nodes` shows the
  total over all threads. A `go nodes N` limit applies to the main thread only.
- **Torn entries.** Without the XOR check, a reader could combine one thread's key with
  another's data and use a score for the wrong position.
- **Shared state.** Only the transposition table is shared. Each helper has its own killers,
  history and search stack, so the threads never contend for anything else.

## What it's worth

| Test | Result |
| --- | --- |
| Stage 17 vs stage 16, 1 thread each, 8+0.08, test Mac | **+0.1 ± 4.1 Elo** over 15,188 games (+4196 −4192 =6800): no regression, SPRT [−5, 0] passed |
| 4 threads vs 1 thread, 8+0.08, test Mac | **+191 ± 99 Elo** over 38 games (+22 −3 =13): a clear gain, SPRT [0, 25] passed |

With one thread the search is unchanged except for the table's packing, so about 0 Elo is the
expected result. Confirming it took 15,188 games, about 15 hours on the test Mac: the test has
to tell 0 from −5 Elo, and when the truth sits right at one of the two hypotheses, the SPRT's
evidence grows very slowly. Changes worth +100 Elo or more are decided in a few hundred games.

The thread-scaling test is the opposite case. It only had to show that four threads clearly gain
(bounds [0, 25]), and 38 games were enough. The size of the gain is rough, though: the 95%
interval runs from about +90 to +290 Elo, and pinning it down would take many more games.

## Further reading

- Chess Programming Wiki: [Lazy SMP](https://www.chessprogramming.org/Lazy_SMP),
  [Parallel Search](https://www.chessprogramming.org/Parallel_Search),
  [Shared Hash Table](https://www.chessprogramming.org/Shared_Hash_Table).
- Robert Hyatt and Timothy Mann, "A lock-less transposition table implementation for parallel
  search chess engines", *ICGA Journal* 25 (2002).
