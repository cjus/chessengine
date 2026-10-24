# Status and next steps

The single place that says where cb2026 stands: what is merged, what is built and waiting, what
is blocked on whom, and how to carry on. **Claude: read this first in a new session.** Update
it in every PR that changes the picture (a stage merged, a result in, a new blocker).

*Last updated 2026-10-24, 16:45 MDT (M5: the check extension, IIR and three versions of
improving merged, with chapter 14.1; improving is off by default and set aside; time losses not
reproduced, #66; continuation history merged, +10.7 ± 7.3, #69; history in LMR merged off by
default after no verdict, #70; singular extensions and multi-cut merged, +29.5 ± 13.3, #71;
adaptive time management merged, +33.5 ± 14.3, #72; history off loses −17.7 ± 9.9, so it stays
on, #73; history in LMR's second version merged, +22.2 ± 11.4, #74; adaptive time off at 40+0.4
loses −13.8 ± 8.6, so the gain holds at the long time control, #76; negative extensions merged,
+4.9 ± 4.0, #77; SPRT progress comments, #79; singular extensions from depth 6 merged, +5.0 ±
4.1, #78; singular extensions from depth 4 merged, +6.1 ± 4.8, #80; `singular_margin` 3
merged, +5.2 ± 4.2, #82; `singular_margin` 4 failed, −1.8 ± 5.3, so it stays at 3, #84;
transposition table buckets failed, −8.8 ± 8.6, so they are off by default, #86; engine
failures in matches can no longer count as draws, #91). 2026-10-17: one `Game` type in
`cb2026-chess` decides when every game is over, for the shell, the match tool, self-play and
chessgui, #94, merged; buckets at 40+0.4 about even, +1.6 ± 6.3 with no verdict, so they stay
off, #92; the gauntlet reference engines chosen, built by `scripts/fetch-gauntlet-engines.sh`,
#36; double extensions merged off by default after no verdict, +3.6 ± 4.9, #96; the M2/M3
gauntlets running on the M3, #36; **M2 passed**, stage 14 rated 2442 ± 11 by gauntlet, #36.
2026-10-18: **M3 passed**, stage 16 rated 2669 ± 10, #36; `main` rated 2714 ± 10, #36; five
engines from 1058 to 1794 chosen for the level calibration, #101, and `scripts/mac-levels.sh`
written, #104; the levels calibrated, 752–2599, and `UCI_Elo` fitted to them, #103;
`UCI_Elo` widened to 800–2600, #106; at 40+0.4 levels 4, 10 and 16 rated 111–272 lower, so
the levels were recalibrated at 40+0.4, #106. 2026-10-19: the levels at 40+0.4 rated 629–2483,
`levels::RATINGS` set to them and `UCI_Elo` narrowed to 650–2450, #106; `Position::parse_move`
reads a move in SAN or coordinates and says why text names none, including a promotion
without its piece, for the shell and Solrac's chat, #113; correction history merged, +28.0 ±
13.0, #112; capture history failed, −19.2 ± 11.9, so it is off by default, #115; ProbCut
failed, −10.2 ± 9.1, so it is off by default, #117; correction history's piece tables failed, −20.7
± 12.3, so they are off by default, #119. 2026-10-20: their second version, added to the pawn
table instead of averaged with it, failed too, −0.4 ± 4.3 after 11,433 games, #121;
`singular_tt_depth_margin` 4 instead of 3 failed, −7.4 ± 8.0, so it stays at 3, #123; double
extensions at a margin of 50 merged on, +13.0 ± 8.3, #125; a margin of 100 failed, −7.3 ± 8.0, so it
stays at 50, #127; capture history's second version, in Stockfish's form, failed too, −6.8
± 7.8, so it stays off by default, #129. 2026-10-21: `main` rated 2782 ± 11 by gauntlet, 68 ± 15
above 2026-10-18, #131; a flag falling against material that can't mate is a draw (FIDE 6.9),
#137; capture history's third version, in the main search only, failed too, −2.9 ± 5.9 after
6,301 games, so it stays off by default, #134; double extensions at a margin of 70 failed,
−20.5 ± 12.2, so the margin stays at 50, #138; history pruning failed, −5.3 ± 7.1, so it is off
by default, #140; cb2026 relicensed `MIT OR Apache-2.0`, with GPL only in a binary built with
Syzygy, the first step of Solrac's M1b (cjus/chessgui#71, #98);
cb2026 speaks the Chess Node Protocol (`--cnp`, `crates/cnp`), passing every conformance
session that applies (38; the window's is skipped), M1b's step 4 (cjus/chessgui#98; CI needs
the deploy key of #143); v1.0 started: `scripts/mac-robustness.sh` for the robustness run,
#146, which follows #145; larger reductions at expected cut nodes merged on, +11.6 ± 7.7, #145; the v1.0
robustness run running on the M3, #146; the strength levels on a human scale: many of a weaker
level's moves now come from a person's horizon (one or two plies, no quiescence search), fitted
so each level blunders as often as people rated 600 to 2500 on Lichess rapid do, counted in
Lichess's own games by the new `mistakes` tool; `UCI_Elo` 600–2500 is on that scale and the CCRL
ratings no longer hold; full strength unchanged (Carlos, cjus/chessgui#113; #151).
2026-10-22: the v1.0 robustness run clean, 10,000 games with no time losses, illegal moves or
failures, #146; cut-node reductions of 2 plies instead of 1 merged, +8.7 ± 6.4, #155; smaller
reductions at expected all-nodes failed, −8.5 ± 8.4, so they are off by default, #158; smaller
reductions where the principal variation was (Stockfish's `ttPv`) had no verdict, +0.3 ± 5.2
after 8,100 games, stopped, so they are off by default, #160;
CNP's `engine/evaluate` answered with the shell's `eval`, term by term, so Solrac's chat can say
why cb2026 likes a position (cjus/chessgui#145, cjus/chess-node-protocol#6); a search sent the
moment the last one answered is taken, no longer refused as `Busy` while the answered search's
thread ends (found by Solrac's chat, whose review sends two searches in a row); GitHub no longer
checks PRs, to save Actions minutes: `scripts/check.sh` checks a PR before it merges, `main`'s
ruleset requires no check, and CI runs nightly only if `main` changed (Carlos). 2026-10-23:
larger reductions after many fail highs merged on, +12.8 ± 8.2, #165; Stockfish 17's version of
them merged, +13.7 ± 8.5, #167; a string option cleared with an empty value (`setoption name BookFile
value`, as GUIs clear one) is set to empty, no longer taken for an unknown option, found during
the GUI matrix, #169; with MultiPV in XBoard mode every line is sent, each depth's best last as
XBoard expects, no longer the best line only, also found during the GUI matrix, #170; larger
reductions when the hash move is a capture failed, −6.5 ± 7.7, so they are off by default,
#171; stage 21's plan in the PRD:
our own NNUE trainer, with a CPU reference and a GPU backend through wgpu on Metal (Carlos);
the tablebase tests run again, the download script following the mirror's new directories,
#176; stage 21 started: the net's definition in `crates/nnue`, with chapter 21's first
sections, #177; NNUE training records, 32 bytes each, written by `datagen --format records`
and shown by `records`, #178; the `make-book` tool and the first Lichess opening book,
97,432 moves from 1.5 million games of September 2026, #173, #174; the second version of
larger reductions when the hash move is a capture, off the principal variation, failed too,
−7.5 ± 8.0, so both versions are off by default, #179; balanced starts for records, beyond 200 cp drawn again, #180; the CPU
NNUE trainer, about 2.4 million records a second on the M1 Max, #182; the engine evaluates with
an NNUE network when one is loaded (`EvalFile`), #185; the trainer trains on the GPU through
wgpu on Metal, 1.2× the CPU at a hidden layer of 256 and 2.8× at 1,024 on the M1 Max, #186;
measuring it on the M3 Max is Carlos's, #187; the GPU backend prepares batches on all the
trainer's threads, 5.3 million records a second at 256 instead of 2.7, the same network, #188;
`scripts/mac-nnue-data.sh` makes the first net's data on the M3 in resumable chunks and trains
on it, #189; the run itself is Carlos's, #190; `ci.yml` loads again after #186 broke it (a
command ending in a colon), #192; **the GUI matrix passed**, so M4 passed, in En Croissant,
Cute Chess, XBoard and HIARCS Chess Explorer, #37, and M1 with it, #24; the Lichess bot's games
follow v1.0, #191; the first Lichess book goes into the v1.0.0 release, #183; **version 1.0.0**, #154, so the
`v1.0.0` tag is next (Carlos); `mac-sprt.sh` can give the PR's engine an NNUE network (`NET=`), checking it loads,
ready for the net-vs-HCE SPRT, #194; the first network, gen1: 104.2M positions in 12.4 hours on the M3 Max, nets of 256 and 512
trained on its GPU in 7 and 13 minutes, sane evaluations, a bench of 716,942 nodes with the 256
net against the HCE's 749,181, and +172 −4 =24 against the HCE in 200 games at 1+0.01, #190;
**the 256 net passed its SPRT against the HCE**, +14 −0 =0 at 8+0.08 on the M3 Max (LLR 3.21),
#195; **the net is the default** (Carlos: committed in `nets/`, on the v1.0.0 release),
built into every build, with the HCE kept at strength levels and for a lone king (the net alone
drew K+Q and K+R against a lone king at 3,000 nodes a move), bench 749181 → 715674, #196; **the
robustness run again with the net clean**, 10,000 games at `72f50f1` with no time losses,
illegal moves, failures or aborted games, #197, so the `v1.0.0` tag is next (Carlos, #199).*

## Stages

`docs/PRD.md`, "Stages", has the plan. Merged means merged into `main` through its PR.

| Stage | State | PR / branch | Bench | Strength (SPRT at 8+0.08 unless noted) |
| --- | --- | --- | --- | --- |
| 1–10 | Merged | #16–#32 | | `docs/elo-ledger.md` |
| 11 PVS, aspiration | Merged | #33 | 248143 | +28.5 ± 13.1 (Mac, 2001 games) |
| 12 Null move | Merged | #41 | 209085 | +170 ± 40 |
| 13 LMR, LMP | Merged | #42 | 117856 | +242 ± 59 |
| 14 SEE and pruning | Merged | #43 | 1565035 (bench depth 10 from here) | +132 ± 33 |
| 15 Full HCE | Merged | #44 | 1200673 | +192 ± 45 |
| 16 Texel tuning | Merged | #45 | 1200673 | Tuned values −1 ± 21 (no verdict): **not adopted**, hand-set values stay the default |
| 17 Lazy SMP | Merged | #46 | 1207153 | One thread vs stage 16, [−5, 0]: +0.1 ± 4.1, 15,188 games, passed. 4 threads vs 1, [0, 25]: +191 ± 99, 38 games, passed (both on the Mac) |
| 18 Syzygy, Chess960 starts | Merged | #49 | 1207153 | No functional change |
| 18 Polyglot books | Merged | #50 | 1207153 | No functional change (book off by default) |
| 19 Levels, MultiPV | Merged | #51 | 1207153 | No functional change at full strength; levels not yet calibrated (#36) |
| 20 XBoard, pondering, conformance tests, Lichess docs | Merged | #52 | 1207153 | No functional change |
| 21 NNUE | In progress | #177 (the net's definition), #178 (training records), #180 (balanced starts), #182 (the CPU trainer), #185 (inference) | 749181 | No functional change yet; M6 stretch goal |

### Built on top of stage 20 (not stages)

| Work | PR / branch | Bench | State |
| --- | --- | --- | --- |
| Shell: `stats`, `tree`, `explain`, `eval diff`, `see`, `techniques`, `ab` | #53 | 1207153 | Merged, no functional change |
| Shell: `redo`, clocks, `hint`, `resign`, `draw`, PGN save/load | #54 | 1207153 | Merged, no functional change |
| Shell: `match`, `selfplay`, `uci`, `xboard` | #55 | 1207153 | Merged, no functional change |
| M5: check extensions | #57 | 1207153 | Merged, off by default: −8.3 ± 17.7 after 840 games (no gain) |
| M5: internal iterative reductions | #58 | 765380 | Merged: +19.2 ± 10.4, 2,337 games, [0, 5] passed (Mac) |
| M5: improving heuristic | #60 | 765380 | Merged, off by default: −32.0 ± 15.1, 1,142 games, [0, 5] failed (Mac) |
| M5: improving, half a ply off the margin; root evaluation fix | #63 | 765380 | Merged, off by default: −17.7 ± 11.5, 1,691 games, [0, 5] failed (M3 Max) |
| M5: improving, the reverse futility margin only | #65 | 765380 | Merged, off by default: −33.0 ± 15.3, 1,036 games, [0, 5] failed (M3 Max) |
| M5: continuation history (chapter 10.1) | #69 | 754226 | Merged: +10.7 ± 7.3, 4,532 games, [0, 5] passed (M3 Max) |
| M5: history in late move reductions (chapter 14.1) | #70 | 754226 | Merged, off by default: +2.1 ± 5.0, 8,910 games, [0, 5] no verdict (LLR −0.30), stopped (M3 Max) |
| M5: singular extensions and multi-cut (chapter 14.1) | #71 | 1115153 | Merged: +29.5 ± 13.3, 1,357 games, [0, 5] passed (M3 Max) |
| M5: adaptive time management (chapter 6.1) | #72 | 1115153 (bench is depth-limited; timed games change) | Merged: +33.5 ± 14.3, 1,062 games, [0, 5] passed (M3 Max) |
| M5 experiment: history heuristic off (chapter 10.1) | #73 | 1115153 | Merged as a record only: history off −17.7 ± 9.9, 2,124 games, [−5, 0] failed (M3 Max); history stays on |
| M5: history in LMR, second version (chapter 14.1) | #74 | 1126902 | Merged: +22.2 ± 11.4, 1,661 games, [0, 5] passed (M3 Max) |
| M5 experiment: adaptive time off at 40+0.4 (chapter 6.1) | #76 | 1126902 | Merged as a record only: adaptive time off −13.8 ± 8.6, 2,599 games, [−5, 0] at 40+0.4 failed (M3 Max); adaptive time stays on |
| M5: negative extensions (chapter 14.1) | #77 | 1100720 | Merged: +4.9 ± 4.0, 14,211 games, [0, 5] passed (M3 Max) |
| M5: singular extensions from depth 6 (chapter 14.1) | #78 | 1131028 | Merged: +5.0 ± 4.1, 12,282 games, [0, 5] passed (M3 Max) |
| M5: singular extensions from depth 4 (chapter 14.1) | #80 | 1343312 | Merged: +6.1 ± 4.8, 9,057 games, [0, 5] passed (M3 Max) |
| M5: `singular_margin` 3 (chapter 14.1) | #82 | 1098926 | Merged: +5.2 ± 4.2, 11,400 games, [0, 5] passed (M3 Max) |
| M5: `singular_margin` 4 (chapter 14.1) | #84 | 1098926 | Merged as a record, the margin back to 3: −1.8 ± 5.3, 7,476 games, [0, 5] failed (M3 Max) |
| M5: transposition table buckets (chapter 8.1) | #86 | 1098926 (1114877 with buckets on) | Merged, off by default: −8.8 ± 8.6, 2,685 games, [0, 5] failed (M3 Max) |
| M5: transposition table buckets at 40+0.4 (chapter 8.1) | #92 | 1098926 (1114877 with buckets on) | Merged as a record, off by default: +1.6 ± 6.3, 4,630 games, [0, 5] at 40+0.4 no verdict (LLR −0.45), stopped (M3 Max) |
| `cb2026_chess::game::Game`: one record of a game and one set of end-of-game rules, moved from chessgui and used by the shell, the match tool and self-play | #94 | 1098926 | Merged, no functional change; chessgui uses it from cjus/chessgui#31 |
| M5: double extensions (chapter 14.1) | #96 | 1098926 (1248666 with double extensions on) | Merged, off by default: +3.6 ± 4.9, 8,670 games, [0, 5] no verdict (LLR 0.92), stopped (M3 Max) |
| `Position::parse_move`: a move typed in SAN or coordinates, and `SanError::MissingPromotion` when a pawn promotes without its piece; the shell uses it, and Solrac's chat will (cjus/chessgui#38) | #113 | 1098926 | Merged, no functional change |
| A flag falling against material that can't mate is a draw (FIDE Laws 6.9): `Position::has_mating_material`, `Outcome::TimeoutDraw`, and `Game::lose_on_time` renamed `Game::time_out`; for Solrac's clocks (cjus/chessgui M1) and the shell's | #137 | 1136500 | Merged, no functional change |
| M5: correction history (chapter 14.2) | #112 | 1065754 | Merged: +28.0 ± 13.0, 1,308 games, [0, 5] passed (M3 Max) |
| M5: capture history (chapter 10.2) | #115 | 1065754 (943210 with capture history on) | Merged, off by default: −19.2 ± 11.9, 1,561 games, [0, 5] failed (M3 Max) |
| M5: ProbCut (chapter 12.1) | #117 | 1065754 (963637 with ProbCut on) | Merged, off by default: −10.2 ± 9.1, 2,593 games, [0, 5] failed (M3 Max) |
| M5: correction history's piece tables (chapter 14.2) | #119 | 1065754 (1019663 with the piece tables on) | Merged, off by default: −20.7 ± 12.3, 1,362 games, [0, 5] failed (M3 Max) |
| M5: the piece tables' second version, added to the pawn table (chapter 14.2) | #121 | 1065754 (1094228 with the piece tables on) | Merged, off by default: −0.4 ± 4.3, 11,433 games, [0, 5] failed (M3 Max) |
| M5: `singular_tt_depth_margin` 4 instead of 3 (chapter 14.1) | #123 | 1065754 (1028535 with the margin at 4) | Merged as a record, the margin back at 3: −7.4 ± 8.0, 3,345 games, [0, 5] failed (M3 Max) |
| M5: double extensions at a margin of 50, on (chapter 14.1) | #125 | 1136500 | Merged: +13.0 ± 8.3, 3,167 games, [0, 5] passed (M3 Max) |
| M5: double extensions at a margin of 100 instead of 50 (chapter 14.1) | #127 | 1136500 (945934 with the margin at 100) | Merged as a record, the margin back at 50: −7.3 ± 8.0, 3,368 games, [0, 5] failed (M3 Max) |
| M5: capture history, second version: the victim's value plus history, the attacker only breaking ties (chapter 10.2) | #129 | 1136500 (998700 with capture history on) | Merged, off by default: −6.8 ± 7.8, 3,512 games, [0, 5] failed (M3 Max) |
| M5: capture history, third version: the second in the main search only, the quiescence search keeping MVV-LVA (chapter 10.2) | #134 | 1136500 (1003431 with capture history on) | Merged, off by default: −2.9 ± 5.9, 6,301 games, [0, 5] failed (M3 Max) |
| M5: double extensions at a margin of 70 instead of 50 (chapter 14.1) | #138 | 1136500 (1054909 with the margin at 70) | Merged as a record, the margin back at 50: −20.5 ± 12.2, 1,407 games, [0, 5] failed (M3 Max) |
| M5: history pruning: quiet moves with a bad history skipped near the leaves (chapter 13.1) | #140 | 1136500 (1022919 with history pruning on) | Merged, off by default: −5.3 ± 7.1, 4,548 games, [0, 5] failed (M3 Max) |
| M5: larger reductions at expected cut nodes (chapter 14.1) | #145 | 1078861 | Merged: +11.6 ± 7.7, 3,687 games, [0, 5] passed (M3 Max) |
| M5: cut-node reductions of 2 plies instead of 1, as in Stockfish (chapter 14.1) | #155 | 1072184 | Merged: +8.7 ± 6.4, 5,803 games, [0, 5] passed (M3 Max) |
| M5: smaller reductions at expected all-nodes (chapter 14.1) | #158 | 1072184 (1284421 with them on) | Merged, off by default: −8.5 ± 8.4, 2,993 games, [0, 5] failed (M3 Max) |
| M5: smaller reductions where the principal variation was, Stockfish's `ttPv` (chapter 14.1) | #160 | 1072184 (1198340 with them on) | Merged, off by default: +0.3 ± 5.2, 8,100 games, [0, 5] no verdict (LLR −1.57), stopped (M3 Max) |
| M5: larger reductions after many fail highs, as in Stockfish (chapter 14.1) | #165 | 767859 | Merged: +12.8 ± 8.2, 3,144 games, [0, 5] passed (M3 Max) |
| M5: larger reductions after many fail highs in Stockfish 17's version (chapter 14.1) | #167 | 749181 | Merged: +13.7 ± 8.5, 2,953 games, [0, 5] passed (M3 Max) |
| M5: larger reductions when the hash move is a capture, as in Stockfish (chapter 14.1) | #171 | 749181 (719329 with them on) | Merged, off by default: −6.5 ± 7.7, 3,668 games, [0, 5] failed (M3 Max) |
| M5: larger reductions when the hash move is a capture, off the principal variation (chapter 14.1) | #179 | 749181 (754886 with them on) | Merged, off by default: −7.5 ± 8.0, 3,031 games, [0, 5] failed (M3 Max) |
| v1.0: opening books from people's games, the `make-book` tool and `scripts/lichess-book.sh` (chapter 18) | #173 | 749181 | Merged, no functional change. The first book, from Lichess's September 2026 games: 97,432 moves from 1,499,615 games, 1.6 MB, 58 minutes on the M3 (#174) |

**Nothing is running on the M3 now** (2026-10-23, 16:33 MDT). Next for it: the NNUE trainer's
speed (#187, a few minutes) and stage 21's data run, the first net's records and its training
(#190, about 12 hours; it doesn't need #187 first). No SPRT candidate is chosen yet.

**v1.0's gates have all passed.** The GUI matrix, the last, passed on 2026-10-23 (#37, recorded
in `docs/gui-compat.md` and the PRD): En Croissant, Cute Chess, XBoard and HIARCS Chess Explorer
on the M1 Max, with two fixes found on the way (#169, #170). The Lichess bot's two games follow
the release (Carlos, 2026-10-23, #191), and Scid vs. PC and ChessBase weren't checked. Next:
1. the version is 1.0.0 (#154, merged on 2026-10-23);
2. the robustness run again with the network as the default (#197): clean on 2026-10-24;
3. Carlos tags `v1.0.0`, and the Release workflow makes a draft release (#199);
4. the book is uploaded to the draft (`docs/releasing.md`; #183);
5. Carlos checks the draft and publishes it.

The robustness run was played again (#197) because #196 made the built-in network the default,
and v1.0's first run (#146) was with the hand-crafted evaluation. It finished on 2026-10-24 at
16:31 MDT, about 10 hours after it started at 06:22: **clean**. `main` at `72f50f1` played
itself 6,000 games at 1+0.01 (36 minutes), 3,000 at 10+0.1 (3 hours) and 1,000 at 40/60 (6.6
hours), 9 at a time with the default Move Overhead, with no time losses, illegal moves, failures
or aborted games. Its scores, +2.8 ± 6.1, +2.2 ± 7.6 and +1.4 ± 11.2, are noise, as they should
be. The network's slower nodes (about 6%) cost no flag even at 1+0.01. Since `72f50f1`, `main`
has changed only in its docs.

**Opening books (#173, merged).** Carlos asked for a book from the Lichess database. The
`make-book` tool (`crates/tools/src/book_maker.rs`) reads games in PGN from standard input and
writes a Polyglot book; `scripts/lichess-book.sh <YYYY-MM> <book.bin>` streams one month from
https://database.lichess.org/ (CC0, about 30 GB) into it. Defaults: both players 2200+, blitz or
slower, no bots, the first 20 plies, moves played in 20+ games scoring 35%+, weighted by points.
The first full book, from September 2026 (29 GB), took 58 minutes on the M3 on 2026-10-23,
nearly all of it the download (about 8.4 MB/s): of 89,616,462 games, 1,499,615 passed (1.7%);
of 8,036,080 different moves counted, 97,432 were kept, 1.6 MB. From the starting position: e4
47%, d4 33%, Nf3 9%, c4 6%, b3, g3 and f4 1% each, and 13 more under half a percent each. It is
at `~/books/cb2026-lichess-2026-09.bin` on the M3 and on the M1 Max, where cb2026 1.0.0 loads it
(97,432 entries) and answers 1.e4 with c5 most often. Books are never committed; this one goes
into the v1.0.0 release (Carlos, 2026-10-23, #183), uploaded to the draft before it is
published.

The SPRT of the second version of larger reductions when the hash move is a capture, off the
principal variation (#179), finished on 2026-10-23 at 16:23 MDT. It started at about 14:00 and
failed in 3,031 games, about 2 hours 20 minutes: **−7.5 ± 8.0**, with no time losses, illegal
moves or failures. Both versions are off by default, and the bench stays 749,181. The first
version made the tree smaller and the second barely changed it (+0.8% on the bench); both lost.

The SPRT of larger reductions when the hash move is a capture (#171) finished on 2026-10-23 at
12:36 MDT. It started at about 09:40 and failed in 3,668 games, about 3 hours: **−6.5 ± 7.7**,
with no time losses, illegal moves or failures. They are off by default, and the bench stays
749,181.

The SPRT of Stockfish 17's version of larger reductions after many fail highs (#167) finished on
2026-10-23 at 09:12 MDT. It started at about 06:50 and passed in 2,953 games, about 2 hours 20
minutes: **+13.7 ± 8.5** against Stockfish 16's version, with no time losses, illegal moves or
failures. It is the default, and the bench is 749,181.

The SPRT of larger reductions after many fail highs (#165) finished on 2026-10-23 at about 02:00
MDT. It started at about 23:30 the evening before and passed in 3,144 games, about 2 hours 30
minutes: **+12.8 ± 8.2**, with no time losses, illegal moves or failures. They are on by
default, and the bench is 767,859.

The SPRT of smaller reductions where the principal variation was (#160, Stockfish's `ttPv`)
started on 2026-10-22 at about 14:20 MDT. Undecided after 8,100 games, about 6 hours 25 minutes,
at **+0.3 ± 5.2** (LLR −1.57), it was stopped at about 20:45 (Carlos). The reductions are off
by default, and the bench is 1,072,184; the flag is still kept in the table, for another try.

The SPRT of smaller reductions at expected all-nodes (#158) finished on 2026-10-22 at 14:05 MDT.
It started at about 11:45 and failed in 2,993 games, about 2 hours 20 minutes: **−8.5 ± 8.4**,
with no time losses, illegal moves or failures. They grew the trees by a fifth or more. They are
off by default, and the bench is 1,072,184 again.

The SPRT of cut-node reductions of 2 plies instead of 1 (#155) finished on 2026-10-22 at 11:16
MDT. It started at about 06:35 and passed in 5,803 games, about 4 hours 40 minutes: **+8.7 ±
6.4** against 1 ply, with no time losses, illegal moves or failures. `lmr_cut_node` is 2, and
the bench is 1,072,184. The robustness run (#146) played `main` before this change, at 1 ply;
it changes one search parameter and nothing in time handling.

The v1.0 robustness run (#146) finished on 2026-10-22 at 06:17 MDT, about 10 hours after it
started: **clean**. `main` at `9cff7fc` played itself 6,000 games at 1+0.01, 3,000 at 10+0.1 and
1,000 at 40/60, with no time losses, illegal moves, failures or aborted games. `main` has changed
since only in the weaker levels (#151); the bench is the same, 1,078,861.

The SPRT of larger reductions at expected cut nodes (#145) finished on 2026-10-21 at 20:11 MDT.
It started at about 17:15 and passed in 3,687 games, about 3 hours: **+11.6 ± 7.7**. They are on
by default, and the bench is 1,078,861. `main` with them is the v1.0 candidate, so #146 can
start.

The SPRT of history pruning (#140) finished on 2026-10-21 at 16:29 MDT. It started at about
12:50 and failed in 4,548 games, about 3 hours 40 minutes: **−5.3 ± 7.1**, though it took 10%
off the bench. It is off by default; chapter 13.1 lists three settings for another try.

The SPRT of double extensions at a margin of 70 instead of 50 (#138) finished on 2026-10-21 at
12:08 MDT. It started at about 11:00 and failed in 1,407 games, about 1 hour 10 minutes:
**−20.5 ± 12.2**, though it took 7.2% off the bench. The margin stays at 50.

The SPRT of capture history's third version (#134, the second in the main search only, the
quiescence search keeping MVV-LVA's order) finished on 2026-10-21 at 10:33 MDT. It started at
about 05:33 and failed in 6,301 games, about 5 hours: **−2.9 ± 5.9**, though it took 11.7% off
the bench. Capture history stays off by default. All three versions have lost; chapter 10.2
lists two settings still untested.

The gauntlet on `main` (#131, `main` at `a422e69`) finished on 2026-10-21 at 01:16 MDT, about 4
hours after it started: **2782 ± 11**, 68 ± 15 above the gauntlet of 2026-10-18 (2714 ± 10,
#36). Correction history (+28.0) and double extensions (+13.0) passed in between, about +41 in
self-play, so this time the gauntlet found more than self-play did.

The SPRT of capture history's second version (#129) finished on 2026-10-20 at 18:41 MDT. It
started at about 15:55 and failed in 3,512 games, about 2 hours 45 minutes: **−6.8 ± 7.8**,
though it took 12.1% off the bench. Capture history is merged off by default, with the second
version's scoring kept for another try.

The SPRT of double extensions at a margin of 100 instead of 50 (#127) finished on 2026-10-20 at
14:48 MDT. It started at about 12:05 and failed in 3,368 games, about 2 hours 45 minutes:
**−7.3 ± 8.0**, though it took 17% off the bench. The margin stays at 50.

The SPRT of double extensions at a margin of 50 (#125) finished on 2026-10-20 at 11:48 MDT. It
started at about 09:20 and passed in 3,167 games, about 2 hours 30 minutes: **+13.0 ± 8.3**.
Double extensions are on by default now, at a margin of 50, and the bench is 1,136,500.

The SPRT of `singular_tt_depth_margin` 4 instead of 3 (#123) finished on 2026-10-20 at 09:00
MDT. It started at about 06:20 and failed in 3,345 games, about 2 hours 40 minutes: **−7.4 ±
8.0**, though it took 3.5% off the bench. The margin stays at 3, Stockfish's value.

The piece tables' second SPRT (#121) finished on 2026-10-20 at 05:05 MDT. It started at about
20:03 the evening before and failed after 11,433 games, about 9 hours: **−0.4 ± 4.3**. It was
undecided at 8,000 games (+0.0 ± 5.2) and Claude proposed stopping it; it reached its verdict
before Carlos answered. The piece tables are merged off by default.

The piece tables' first SPRT (#119) finished on 2026-10-19 at 17:23 MDT. It started at about
16:17 and failed in 1,362 games: **−20.7 ± 12.3**, though it took 4.3% off the bench. That
version averaged the tables; it is merged off by default.

ProbCut's SPRT (#117) finished on 2026-10-19 at 15:43 MDT. It started at about 13:41 and failed
in 2,593 games: **−10.2 ± 9.1**, though it took 9.6% off the bench. It is merged off by default,
with three untested settings for another try in chapter 12.1.

Capture history's SPRT (#115) finished on 2026-10-19 at 12:53 MDT. It started at about 11:39
and failed in 1,561 games: **−19.2 ± 11.9**, though it took 11.5% off the bench. It is merged
off by default, with three untested ideas for a second version in chapter 10.2.

Correction history's SPRT (#112) finished on 2026-10-19 at 10:53 MDT. It started at about 09:45
and passed in 1,308 games: **+28.0 ± 13.0**, about as much as singular extensions and multi-cut
(#71). It is on by default.

The recalibration of the levels at 40+0.4 (#106) finished on
2026-10-19 at 08:28 MDT. It started at about 21:45 MDT the evening before, and rated levels 0,
2, 6, 8, 12, 14, 18 and 19, 6,400 games in under 11 hours. With levels 4, 10 and 16 from the
check before it (2,400 games), Skill Levels 0–19 rate **629 to 2483** at 40+0.4 (± 23 to 55),
74 to 272 below the 8+0.08 calibration. The odd levels are halfway between their neighbours.
`levels::RATINGS` holds the new table, and `UCI_Elo` runs from level 0's rating to level 19's,
rounded inward to 50: **650–2450** (levels 0.3 to 18.8). The levels search a fixed number of
nodes, so they play the same at any time control, while the reference engines play better with
more time. CCRL rated those engines at 2'+1", slower than both, so 40+0.4 is the better
estimate. Walleye 1.3.0 lost 8 games by an illegal move (its en passant bug), and Arche 0.3.8 2
by answering `bestmove 0000` (no move), a bug of its own.

The level calibration (#103) finished on 2026-10-18 at about
16:10 MDT, 16,000 games in six and a half hours: Skill Levels 0–19 rated **752 to 2599**, and
`UCI_Elo` now maps onto those ratings, interpolating between levels (`levels::RATINGS`,
`level_for_elo`, `settings_at`; chapter 19). Walleye 1.3.0 lost about 1% of its games by an
illegal move: it ignores en passant captures in the moves it is sent, a bug of its own.
`UCI_Elo` became 800–2600 (levels 0.7 to 19.0), widened from 1000–2400 (Carlos, #106). The
recalibration at 40+0.4 replaced both.

The rating gauntlets (#36) finished on 2026-10-18 at 06:50 MDT, 800 games against each of the
seven reference engines at 8+0.08, about 4 hours a version: stage 14 **2442 ± 11** (M2 passed),
stage 16 **2669 ± 10** (M3 passed), and `main` **2714 ± 10**, all recorded in the PRD's
"Milestones" and the ledger. `main` is only 45 ± 14 above stage 16, though the M5 SPRTs that
passed add up to about +136 in self-play: self-play overstated gains against other engines about
threefold there. The next gauntlet on `main` (#131, 2026-10-21) found the opposite: **2782 ±
11**, +68 ± 15 for about +41 in self-play. Over M5 so far, from stage 16, the gauntlets found
+113 where the SPRTs add up to about +177.

When several PRs wait for the M3, they run one at a time, and **before each SPRT starts, rebase
that PR onto the current `main`** and re-measure its `Bench:` line: `scripts/mac-sprt.sh` plays
the PR's head against `main`, so an unrebased PR would be measured against changes it doesn't
have. For each: if it passes, record it in its chapter and the ledger, mark it ready and merge;
if it fails, switch it off by default (as with #57, #60, #63 and #65), record it and merge.

**The SPRT loop with Carlos.** Claude opens the PR as a draft with the M3 command in its
description, ending with `tail -f ~/sprt-<N>.log` so Carlos can watch it (Carlos asked for
that on every job, 2026-10-15). Carlos starts it on the M3 and says so ("SPRT started on the
M3 for #N"). Claude then runs `scripts/watch-sprt.sh <N>` in the background: it checks the PR every 10 minutes for
the comment `mac-sprt.sh` posts and prints it (Carlos approved this polling on 2026-10-12).
`mac-sprt.sh` also keeps a progress comment on the PR, updated every 2,000 games (Carlos,
2026-10-14). If it shows 8,000 games or more with no result, the watcher stops early (exit
3), and Claude proposes stopping the run to Carlos; to keep watching after that, run it again
with a higher threshold (`scripts/watch-sprt.sh <N> 24 12000`) or none (`… 24 0`).
On the result, Claude records it and merges, then rebases the next PR in the queue and tells
Carlos it's ready. If the watcher gives up, ask Carlos for `tail -5 ~/sprt-<N>.log` and
`pgrep -fl "match|mac-sprt"` on the M3.

**En Croissant on the M1 Max** uses `~/engines/cb2026`, a copy of a release build of `main`
made on 2026-10-12 at 33d37e8, before the M5 gains of 2026-10-13. To refresh it when Carlos
asks: build `main` (`cargo build --release`), check `bench` matches `main`'s, then
`cp target/release/cb2026 ~/engines/cb2026`.

## Milestones

`docs/PRD.md`, "Milestones", defines the gates and records the results. M0, M2 and M3 are
passed; the rest wait on GUI checks, level calibration or the v1.0 work.

| Milestone | Scope | State |
| --- | --- | --- |
| M0 Foundations | stages 1–3 | **Passed** 2026-10-11: perft suites exact, 278.8 Mnps on the M1 Max (#20); 369.6 on the M3 Max |
| M1 Playable | stages 4–7 | **Passed** 2026-10-23: complete 3+2 games in En Croissant and HIARCS during the GUI matrix (#24, #37) |
| M2 Classical search | stages 8–14 | **Passed** 2026-10-18: stage 14 rated 2442 ± 11 by gauntlet at 8+0.08 (gate ~2200 CCRL), #36 |
| M3 Evaluation | stages 15–16 | **Passed** 2026-10-18: stage 16 rated 2669 ± 10 by gauntlet at 8+0.08 (gate ≥2400 CCRL), #36 |
| M4 Complete engine | stages 17–20 | **Passed** 2026-10-23: levels calibrated at 40+0.4 on 2026-10-19, 629–2483 (#106); the GUI matrix passed (#37); the Lichess bot's games follow v1.0 (#191) |
| M5 v1.0 | strength work, release | Started (the M5 branches above); release workflow merged (#40); `main` rated 2782 ± 11 by gauntlet on 2026-10-21 (#131; 2714 ± 10 on 2026-10-18, #36), above the gate (≥2400, goal 2600+); the robustness run clean on 2026-10-22 (#146), and again with the network as the default on 2026-10-24 (#197); the GUI matrix passed on 2026-10-23 (#37); version 1.0.0 merged (#154); the `v1.0.0` tag is next (#199) |
| M6 NNUE | stage 21 | Started 2026-10-23: the net's definition (#177), training records (#178) |

## Waiting on Carlos

The open [`needs-carlos` issues](https://github.com/cjus/chessengine/issues?q=is%3Aopen+label%3Aneeds-carlos):

- #199: the `v1.0.0` tag (every gate passed), then publishing the draft release with the book
  on it (#183).
- #187: the NNUE trainer's speed on the M3 Max (a few minutes).
- #191: the Lichess bot's two games, after v1.0.
- #88: whether macOS honours the search threads' QoS outside Claude Code.

On hold: #61, running long tests on AWS Batch (closed 2026-10-12; reopen it if the M3 can't keep
up with the test queue).

Done on 2026-10-18: #36 (the M2/M3 gauntlets and one on `main`, run overnight on the M3:
2442, 2669 and 2714), #101 (five engines from 1058 to 1794 for the levels) and #103 (the
level calibration, 752–2599).

Done on 2026-10-19: #106 (the levels recalibrated at 40+0.4, 629–2483, and `UCI_Elo` 650–2450).

Done on 2026-10-12: #62 (the M3 Max is set up; its check: `bench` 765380, the perft suites
exact at 369.6 Mnps) and #66 (the time-loss checks on the M3: none).

Done on 2026-10-11: #17 (stage tags 1–16 pushed from the Mac; Claude now pushes each new stage
tag itself when it runs there, and has tagged stages 17–20), #6 (the Cross-platform workflow
passed on macOS and Windows), #47 (both stage 17 tests passed) and #20 (the M0 perft gate passed;
Claude ran it on the idle Mac).

## How to carry on

### Merging the stack

For each branch of a stack (none is open now), once it has its SPRT result and its book
section:

1. Rebase the branch onto the latest `origin/main`. The commits below it are already in `main`,
   so only its own commits replay.
2. Open its PR. Then commit the result to the chapter's "What it's worth"
   table and a `docs/elo-ledger.md` row with the PR number, as a separate commit.
3. Mark the PR ready (`gh pr ready`), run `scripts/check.sh` on it, then merge (CLAUDE.md:
   Claude may merge its own PRs once it passes).
   Push the stage's tag (`git tag -a stage-NN-<name> <merge commit> -m "..."`); from a cloud
   session, which can't push tags, add the command to a `needs-carlos` issue instead.
4. Rebase the rest of the stack onto the new branch head, and point each branch at its rebased
   commit (match by commit subject). Push with `--force-with-lease`.

When a change below a commit alters the search, every later commit's `Bench:` line must be
re-measured:

```sh
git rebase --onto <new base> <old base> <branch> \
  --exec 'cargo build --release -q -p cb2026 && b=$(./target/release/cb2026 bench | awk "/Nodes searched/{print \$4}") && git commit -q --amend -m "$(git log -1 --format=%B | sed "s/^Bench: [0-9]*\$/Bench: $b/")" && cargo test -q --workspace'
```

Run the full test suite on every commit you replay, not only at the tip: each becomes a PR head.

### Testing strength

- **The cloud machine** (4 cores) runs short SPRTs, 3 games at a time, but a background job
  stops after 2 hours. That's about 850 games, enough for gains of 30+ Elo. Smaller changes need
  the test Mac.
- **The test Mac** is, from 2026-10-12, an M3 Max on Carlos's network: 10 performance and 4
  efficiency cores, 36 GB (#62; the PRD's "Test machine" section has its speed). Carlos starts
  long tests there (`docs/testing.md`, "Long tests on the test Mac"), and the scripts post the
  results to the PR. `scripts/mac-sprt.sh <PR> [elo0 elo1]` runs one game per performance core
  minus one: 9 on the M3 Max, about 1,250 games an hour at 8+0.08 (the M1 Max, 7 at a time,
  played about 970; a game takes about 26 seconds whatever the CPU, because the clock sets its
  length). CLAUDE.md says long SPRTs and gauntlets are Carlos's to run.
- **The M1 Max is no longer a test machine.** Claude Code runs on it, Carlos needs it free and
  disconnects it, so don't start SPRTs, gauntlets or other long runs there.
- **Neutral changes take long.** When the true difference sits at one of the two hypotheses, the
  SPRT needs many games: stage 17's one-thread test (about 0 Elo, bounds [−5, 0]) took 15,188
  games, 15 hours on the M1 Max. Clear gains of +100 Elo or more end in a few hundred. A change
  worth about half the upper bound is the slowest of all: #70 (history in LMR) sat at +2 ± 5
  after 8,910 games and was stopped with no verdict (Carlos, 2026-10-13) and merged off. If an
  SPRT is still undecided after about 8,000 games with its estimate between the bounds,
  propose the same. It is a proposal, not a rule: #77 (negative extensions) was still going at
  8,000 games and passed at 14,211 with +4.9 ± 4.0, #80 (singular extensions from depth 4),
  which Carlos let run on, passed at 9,057 with +6.1 ± 4.8, and #82 (`singular_margin` 3),
  at +4.1 ± 5.0 after 8,000, passed at 11,400 with +5.2 ± 4.2.
- **The M3's pace varies.** #69 played about 1,390 games an hour; #70 averaged about 760 an
  hour overnight. Something slowed the M3 that night. Since it has a lid, closing it would make
  the Mac sleep even with `caffeinate -i`. Expect fewer games an hour than the estimate above
  when the M3 is in use or asleep for part of the run.
- **Thread scaling** (`scripts/mac-smp.sh`) plays one game at a time, about 140 an hour, so its
  bounds default to [0, 25]: it asks whether more threads clearly gain. Stage 17's 4-thread test
  passed in 38 games, 17 minutes.
- **Engine failures in a match** (#89, sections A and B, fixed 2026-10-16): a crash or hang
  loses the game and that worker starts both engines again; a game that can't start is
  aborted, listed (result `*`) and not scored; and a run that breaks posts **SPRT run failed**
  with the end of its log, never a quiet result. Before, a dead engine's later games counted as
  draws. Every SPRT comment from #69 on lists 0 failures for both engines, so those results
  weren't affected; the earlier comments (#46–#65) didn't list failures. The rest of #89
  (search and protocol findings) is still to review.
- **Bounds:** gainers use [0, 5]; refactors and anything meant to be neutral use [−5, 0]. A
  result without a verdict is recorded honestly. Stage 16, the check extension and improving
  went in switched off rather than claimed as gains.

### Next work, in PRD order

1. Time losses: **not reproduced** (2026-10-12, #66). IIR's SPRT had two in 2,337 games at 8+0.08
   on the M1 Max, one for each engine. Since then there have been none:

   | Run | Machine | Games | Time losses |
   | --- | --- | ---: | --- |
   | #60's SPRT, 8+0.08 | M1 Max, 7 at a time | 1,142 | 0 |
   | #63's SPRT, 8+0.08 | M3 Max, 9 at a time | 1,691 | 0 |
   | #65's SPRT, 8+0.08 | M3 Max, 9 at a time | 1,036 | 0 |
   | Stress test, `main` vs itself, 1+0.01 | M3 Max, 9 at a time | 2,000 | 0 (and no illegal moves or failures) |

   cb2026 never plans more than half its remaining clock and checks the clock every 1,024 nodes,
   so those two losses needed a stall of 200 ms or more: most likely the M1 Max, which was also
   Carlos's working machine. Nothing to fix now. If a loss happens again, #67's details (move,
   time taken, clock) say which kind it is, and every SPRT comment lists them. The M5 robustness
   run (10,000+ games at 1+0.01, 10+0.1 and 40/60) is the final check.
2. Milestone gates: the reference engines are chosen (#36, 2026-10-17; `docs/PRD.md`,
   "Testing and strength measurement"), and `scripts/fetch-gauntlet-engines.sh` builds them. Carlos
   started the M2 and M3 gauntlets on the M3 on 2026-10-17 (the stage 14 and 16 tags), then one
   on `main`. Stage 14 rated 2442 ± 11, so M2 passed; stage 16 2669 ± 10, so M3 passed; and
   `main` 2714 ± 10, then 2782 ± 11 on 2026-10-21 (#131). The levels are calibrated too (#103): five weaker engines (1058–1794,
   #101) joined the seven in `levels.txt`, `scripts/mac-levels.sh` rated each Skill Level
   against the four nearest it, from 752 (level 0) to 2599 (level 19), and `UCI_Elo` maps onto
   those ratings. The straight line it replaced was too flat: about 250 too high at the bottom,
   200 too low at the top. At 40+0.4 the levels rated 74–272 lower, so they were recalibrated
   there (#106): 629 to 2483, and `UCI_Elo` 650–2450.
3. More M5 strength work, each change with an SPRT. Passed on 2026-10-13: continuation history
   (#69, +10.7), singular extensions and multi-cut (#71, +29.5), adaptive time management (#72,
   +33.5), history in LMR's second version (#74, +22.2); on 2026-10-14, negative extensions
   (#77, +4.9 ± 4.0, after 14,211 games and about 13 hours); on 2026-10-15, singular extensions
   from depth 6 (#78, +5.0 ± 4.1, 12,282 games, about 10 hours) and from depth 4 (#80, +6.1 ±
   4.8, 9,057 games, about 7 hours); on 2026-10-16, `singular_margin` 3 (#82, +5.2 ± 4.2,
   11,400 games, about 9 hours); on 2026-10-19, correction history (#112, +28.0 ± 13.0, 1,308
   games, about 70 minutes); on 2026-10-20, double extensions at a margin of 50 (#125, +13.0 ±
   8.3, 3,167 games, about 2 hours 30 minutes); on 2026-10-21, larger reductions at expected cut
   nodes (#145, +11.6 ± 7.7, 3,687 games, about 3 hours); on 2026-10-22, 2 plies of them
   instead of 1 (#155, +8.7 ± 6.4, 5,803 games, about 4 hours 40 minutes); on 2026-10-23, larger
   reductions after many fail highs (#165, +12.8 ± 8.2, 3,144 games, about 2 hours 30 minutes).
   Failed, so not adopted: `singular_margin` 4 (#84, −1.8 ± 5.3
   against 3, 7,476 games, about 6 hours) and transposition table buckets (#86, −8.8 ± 8.6,
   2,685 games, about 2 hours; off by default) and, on 2026-10-19, capture history (#115,
   −19.2 ± 11.9, 1,561 games, about 75 minutes; off by default) and ProbCut (#117, −10.2 ±
   9.1, 2,593 games, about 2 hours; off by default) and correction history's piece tables
   (#119, −20.7 ± 12.3, 1,362 games, about 65 minutes; off by default) and, on 2026-10-20,
   their second version (#121, −0.4 ± 4.3, 11,433 games, about 9 hours; off by default) and
   `singular_tt_depth_margin` 4 (#123, −7.4 ± 8.0 against 3, 3,345 games, about 2 hours 40
   minutes) and double extensions at a margin of 100 (#127, −7.3 ± 8.0 against 50, 3,368 games,
   about 2 hours 45 minutes) and capture history's second version (#129, −6.8 ± 7.8, 3,512
   games, about 2 hours 45 minutes; off by default) and, on 2026-10-21, its third version
   (#134, −2.9 ± 5.9, 6,301 games, about 5 hours; off by default) and double extensions at a
   margin of 70 (#138, −20.5 ± 12.2 against 50, 1,407 games, about 1 hour 10 minutes) and
   history pruning (#140, −5.3 ± 7.1, 4,548 games, about 3 hours 40 minutes; off by default) and,
   on 2026-10-22, smaller reductions at expected all-nodes (#158, −8.5 ± 8.4, 2,993 games, about
   2 hours 20 minutes; off by default). No
   verdict, off: history in
   LMR's first version (#70) and, on 2026-10-17 at 40+0.4, transposition table buckets (#92,
   +1.6 ± 6.3 over 4,630 games, about 18 hours, stopped) and, at 8+0.08, double extensions
   at a margin of 20 (#96, +3.6 ± 4.9 over 8,670 games, about 7 hours, stopped; on at 50 since
   #125) and, on 2026-10-22, smaller reductions where the principal variation was (#160, +0.3
   ± 5.2 over 8,100 games, about 6 hours 25 minutes, stopped). Failed as a simplification, so kept:
   history off (#73, −17.7) and, on
   2026-10-14 at the long time control 40+0.4, adaptive time off (#76, −13.8 ± 8.6 over 2,599
   games; about 255 games an hour on the M3, 10 hours).

   Next, in a rough order of value:
   - **Mostly failures since correction history** (#112, 2026-10-19): of the seventeen tries
     after it, only double extensions at 50 (#125), larger reductions at expected cut nodes
     (#145, then 2 plies of them, #155) and larger reductions after many fail highs (#165, then
     Stockfish 17's version of them, #167) passed, and smaller reductions where the principal
     variation was (#160) had no verdict. Failed: capture history three times
     (#115, −19.2; #129 in Stockfish's form, −6.8; #134 in the main search only, −2.9),
     ProbCut (#117, −10.2), correction history's piece tables averaged with the pawn table
     (#119, −20.7) and added to it (#121, −0.4 over 11,433 games), `singular_tt_depth_margin`
     4 (#123, −7.4), double extensions at 100 (#127, −7.3) and 70 (#138, −20.5) and history
     pruning (#140, −5.3), and smaller reductions at expected all-nodes (#158, −8.5). Every
     failure but #121 and #158 made the bench smaller. Chapters 10.2, 12.1 and 14.2 list untested
     settings for another try. Capture history has lost three times, so it waits behind the
     candidates below.
   - **New techniques**: history pruning failed (#140, −5.3 ± 7.1; chapter 13.1 lists other
     settings). Larger reductions at expected cut nodes passed (#145, +11.6 ± 7.7, against
     Stockfish's own estimate of about 3 Elo for its version), and 2 plies, as in Stockfish,
     added +8.7 ± 6.4 (#155). A smaller reduction at expected all-nodes
     failed (#158, −8.5 ± 8.4). Smaller reductions where the principal variation was, as in
     Stockfish (`ttPv`, which it rates about 7 Elo), had no verdict (#160, +0.3 ± 5.2); chapter
     14.1 lists Stockfish's larger, graded version for another try. Larger reductions after many fail highs,
     as in Stockfish 16 (GPL-3.0, about 5 Elo by its own comment), passed (#165, +12.8 ± 8.2).
     Stockfish 17's version of #165 passed too (#167, +13.7 ± 8.5). A ply more of reduction
     when the hash move is a capture, also from Stockfish 16 and 17 (about 3 Elo by its
     comment), failed (#171, −6.5 ± 7.7), and so did a second version that leaves the principal
     variation alone (#179, −7.5 ± 8.0). The next candidate isn't chosen yet.
   - **Tuning singular extensions, continued**: depth 6 passed (#78) and depth 4 too (#80),
     which is about where Stockfish starts; `singular_margin` 3 passed too (#82), and 4
     failed (#84); `singular_tt_depth_margin` 4 failed against 3 (#123, −7.4 ± 8.0). That
     settles the start depth and both margins for now. Double extensions,
     which extend a move two plies when it is singular by a wide margin (as in Stockfish,
     GPL-3.0), had no verdict at a margin of 20 (#96, +3.6 ± 4.9) and passed at 50 (#125,
     +13.0 ± 8.3, 3,167 games); 100 failed against 50 (#127, −7.3 ± 8.0). A limit of 3 a
     line instead of 6 hardly binds at 50, so it wasn't tested (chapter 14.1); a margin of 70
     failed too (#138, −20.5 ± 12.2), so wider margins look worse; 35 is untested. Gains of
     about 5 Elo take 9,000–14,000 games, 7–13 hours on the M3, and more tuning of the margin
     may find nothing that size.
   - **Tuning adaptive time at 40+0.4**: the gain there (about 14 Elo) is smaller than at
     8+0.08 (about 33), and the scale's numbers were set by hand. Each run at 40+0.4 takes
     about 10 hours, so it waits behind cheaper candidates.
   - **Buckets with a smaller `Hash`** (low value): off after #86 lost at 8+0.08 and #92 was
     about even at 40+0.4. A small table at 8+0.08 would test a full table cheaply, but a gain
     there would say little about the default 16 MB.

   Improving is set aside (Carlos, 2026-10-12) after three failed versions (#60, #63, #65). The results point at
   reverse futility pruning, and the late-move-pruning part may have helped, so later
   candidates are improving in late move pruning only (`rfp_improving` = 0, `lmp_improving` =
   2) and a wider `rfp_margin`.
4. The v1.0 release (`docs/releasing.md`), then stage 21 (NNUE). Carlos asked to move to v1.0
   on 2026-10-21, after the run of failed M5 tries. The steps:
   - The gates: the gauntlet passed (2782 ± 11, #131), and the robustness run was clean
     (2026-10-22, #146: 10,000 games, `scripts/mac-robustness.sh`). The GUI matrix passed on
     2026-10-23 (#37).
   - Then a version PR: `[workspace.package] version` 1.0.0 in `Cargo.toml`, with the ledger,
     the PRD's milestones and the book up to date. The book has a chapter for every stage from
     1 to 20; NNUE (stage 21) is still to come.
   - Then Carlos tags `v1.0.0` (cloud sessions can't push tags), and the Release workflow builds
     the binaries and a draft release for him to check and publish.

   Stage 21's plan is in the PRD, "Evaluation (NNUE, M6)" (Carlos, 2026-10-23): our own trainer
   for its teaching value, a CPU reference and a GPU backend through wgpu (Metal on the Mac), in
   `crates/trainer`; inference in `crates/nnue`; nets as release assets. The net's definition,
   `datagen`'s scores and the trainer don't change how the engine plays, so they can start
   before the engine's inference, and before v1.0 is tagged. The first SPRT needs both.

   Done: the net's definition (#177): `crates/nnue` numbers the 768 inputs, says what a move
   changes, and holds the integer network (accumulator, evaluation, file format) and the float
   one the trainer will learn, with chapter 21's first sections. Training records (#178):
   32 bytes each, with the whole position (castling rights as rooks that can still castle, so
   Chess960 works), the search score and result from White's side, and the best move;
   `datagen --format records` writes them and `records` checks a file. Next, in order:
   - Done: **balanced starts** (#180). For records, `datagen` draws a start's random moves
     again while it scores beyond 200 cp. On 600 games: records within ±100 cp 18.5% → 30.0%,
     beyond ±500 37.5% → 25.3%, White's wins 54% → 41%, at the same speed. Limits of 300 and
     100, and 4 random plies, were measured too (chapter 21).
   - Done: **the CPU trainer** (#182), `crates/trainer` and its `trainer` command: hand-written
     gradients (matching finite differences to 0.1%), AdamW with clipping, multithreaded
     batches, validation, the eval scale fitted from the data, a network file per epoch and a
     checkpoint to resume from. Measured on the M1 Max with 8 threads: about 2.4 million records
     a second, so 20 epochs on 100M records take about 15 minutes. On a 134,000-record sample
     (1,800 games) the validation loss went from 0.105 to 0.047 in 12 epochs, still far above
     the training loss: it needs the full data.
   - Done: **the engine's inference** (#185): an accumulator per ply in every search
     thread, updated at each move (checked against a fresh computation in debug builds), the
     net's score clamped below tablebase and mate scores, the `nnue` switch, UCI `EvalFile`,
     the shell's `set evalfile` and `cb2026 bench NET`. With no net nothing changes (bench
     749181). About 5% slower per node than the HCE with a 256-wide net; weak nets search many
     more nodes, since a noisy evaluation prunes badly.
   - Done: **the GPU backend** (#186): `trainer --backend gpu`, built with `--features gpu`,
     WGSL kernels through wgpu (Metal on the Mac), each tested against the CPU. WGSL has no float
     atomics, so the CPU sorts each batch's rows by input and cuts the lists into segments of
     256 rows (the kings' lists are 16 times the average). The trainer's threads now default to
     the performance cores (10 threads were 17% slower than 8). `scripts/check.sh` checks the
     `gpu` feature on its own.
   - Done: **batches prepared on all the trainer's threads** (#188). On one thread the
     preparation (about 4 ms a batch) was longer than the GPU's work at 256 (3.5 ms); on 8 it
     takes 0.8 ms, and any number of threads gives the same lists (tested), so the same network.
     On the M1 Max, 2.8M records, records a second, CPU (8 threads) against GPU: 2.3M / 5.3M at
     256, 1.1M / 3.0M at 512, 0.54M / 1.4M at 1,024, the same losses. 20 epochs on 100M records
     at 256: about 6 minutes on the GPU. The GPU's work is now the limit at every size.
   - Next: **#187 (Carlos)**, `scripts/mac-trainer-speed.sh` on the M3 Max, a few minutes. Its
     cores, GPU and bandwidth all differ from the M1 Max's, so the GPU's lead may differ there;
     its table picks the backend for the long runs. Record it in chapter 21 ("Other Macs"), the
     PRD and here.
   - Done: **the data run's script** (#189), `scripts/mac-nnue-data.sh`: HCE self-play to 100M
     positions in chunks of 56,000 games (about half an hour each), each its own file in
     `~/nnue-data/gen1`, so a rerun after a stop keeps the finished chunks; chunk k's seeds start
     at 1 + k × 56,000, so no chunk repeats a game. Then it checks every record, trains nets of
     256 and 512 on the GPU, benches each, and posts a report. Tested at a tiny size on the M1
     Max, including an interrupted chunk redone with the same records.
   - Done: **the data run** (#190, closed): 104.2M positions from 1,344,000 games in 12.4 hours
     on the M3 Max (9 threads), in `~/nnue-data/gen1` there; results 40.5 / 23.8 / 35.6%, 30%
     of scores within ±100 cp. Nets trained on the M3 Max's GPU (20 epochs): 256 in 7 minutes,
     validation loss 0.0094 (training 0.0090); 512 in 13 minutes, 0.0093 (0.0086, starting to
     memorise). On the M1 Max as `~/engines/nets/gen1/gen1-256.nnue` (`ff8692c9b527…`) and
     `gen1-512.nnue` (`20b5a5e1e9ef…`). Checked: sane material and endgame evaluations, every
     mirrored position the same, bench 716,942 nodes with the 256 net (HCE 749,181), and 200
     games at 1+0.01 against the HCE +172 −4 =24 (about +424 ± 71, a first sign only).
   - Done: **SPRTs with a net** (#194): `NET=<file> scripts/mac-sprt.sh <PR>` gives the PR's
     engine the net (UCI `EvalFile`) and leaves main's on the HCE; `BASE_NET=<file>` gives main's
     one too, to compare two nets. It loads each net through UCI before the match and stops if
     it doesn't load (an engine that can't load its net plays on with the HCE), and names each
     net with the start of its SHA-256 in the comments.
   - Done: **the SPRT** (#195): the 256 net against the HCE, [0, 5] at 8+0.08 on the M3 Max,
     **passed after 14 games, +14 −0 =0** (LLR 3.21), with no time losses or failures. Too
     lopsided to give an Elo figure (as stage 9's 14–0); 200 games at 1+0.01 on the M1 Max gave
     about +424 ± 71.
   - Done: **the net is the default** (#196; Carlos, 2026-10-24: commit it, put it on the
     v1.0.0 release). `nets/cb2026-gen1-256.nnue` is committed (395 KB; a release's build runs
     before the release exists, so the file can't come from it) and built into every build
     (`cb2026_engine::builtin_network`); the Release workflow attaches it to each release.
     `EvalFile` defaults to `<built-in>`; empty or `none` is the HCE, the same words in the
     shell, `cb2026 bench [NET]` and `datagen --evalfile` (`EvalChoice`). The HCE stays at
     strength levels (calibrated with it; Carlos) and when one side has only its king (the
     `loneking` switch: the net alone drew all five K+Q and K+R endings tried at 3,000 nodes,
     with the rule it wins them as the HCE does). Bench 749181 → 715674. `datagen` and
     `mac-nnue-data.sh` now play with the net (gen2's data, in `~/nnue-data/gen2`) unless given
     `none`; `mac-sprt.sh`'s `NET`/`BASE_NET` take `builtin` and `none` too.
   - Done: **the robustness run again** (#197), since v1.0's was with the HCE: clean on
     2026-10-24, 10,000 games at `72f50f1` from 1+0.01 to 40/60, no time losses, illegal moves,
     failures or aborted games.
   - Next: **the `v1.0.0` tag (Carlos, #199)**, whose release carries the net and the Lichess
     book (#183). Later: an Elo figure for the net (a fixed number of games, or a gauntlet
     against the reference engines beside main's 2714 ± 10), the levels recalibrated with the
     net, 512 against 256 (`NET=… BASE_NET=…`), and gen2 from the net's own games.
