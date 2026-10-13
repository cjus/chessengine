# Elo ledger

The measured strength gain of every stage and technique, so learners can see what each one is
worth and we can see where the engine's strength comes from.

Each row is an SPRT run of the change against the `main` it was merged into (see `docs/PRD.md`,
"Testing and strength measurement"), or a gauntlet result at a milestone gate. Elo is from
self-play unless noted, so it overstates gains against other engines.

| Date | Stage / technique | PR | Test | Time control | Games | Elo ± 95% | LLR (bounds) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-10-10 | Stage 5: material + negamax alpha-beta (vs stage 4 random mover) | cjus/chessengine#26 | SPRT | 8+0.08, cloud, 3 games at a time | 286 | +285 −0 =1 (no meaningful Elo) | passed, [0, 5] |
| 2026-10-11 | Stage 6: iterative deepening + time management (vs stage 5) | cjus/chessengine#28 | SPRT | 8+0.08, cloud, 3 games at a time | 41 | +33 −0 =8, about +390 ± 140 | passed (3.00 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 7: quiescence search + MVV-LVA (vs stage 6) | cjus/chessengine#29 | SPRT | 8+0.08, cloud, 3 games at a time | 282 | +123 −31 =128, about +118 ± 30 | passed (2.94 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 8: transposition table (vs stage 7) | cjus/chessengine#30 | SPRT | 8+0.08, cloud, 3 games at a time | 287 | +48 −9 =230, about +48 ± 17 | passed (2.95 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 9: piece-square tables + tapered evaluation (vs stage 8) | cjus/chessengine#31 | SPRT | 8+0.08, cloud, 3 games at a time | 14 | +14 −0 =0 (no meaningful Elo) | passed (3.21 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 10: killers, countermoves, history, staged move picking (vs stage 9) | cjus/chessengine#32 | SPRT | 8+0.08, cloud, 3 games at a time | 477 | +251 −110 =116, about +106 ± 28 | passed (2.96 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 11: principal variation search + aspiration windows (vs stage 10) | cjus/chessengine#33 | SPRT | 8+0.08, test Mac (M1 Max), 7 games at a time | 2001 | +818 −654 =529, about +28.5 ± 13.1 | passed (2.95 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 12: null-move pruning (vs stage 11) | cjus/chessengine#41 | SPRT | 8+0.08, cloud, 3 games at a time | 255 | +155 −39 =61, about +170 ± 40 | passed (2.97 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 13: late move reductions + late move pruning (vs stage 12) | cjus/chessengine#42 | SPRT | 8+0.08, cloud, 3 games at a time | 113 | +73 −5 =35, about +242 ± 59 | passed (2.95 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 14: SEE, reverse futility, razoring, futility, SEE and delta pruning (vs stage 13) | cjus/chessengine#43 | SPRT | 8+0.08, cloud, 3 games at a time | 328 | +174 −55 =99, about +132 ± 33 | passed (2.96 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 15: full hand-crafted evaluation (vs stage 14) | cjus/chessengine#44 | SPRT | 8+0.08, cloud, 3 games at a time | 243 | +163 −41 =39, about +192 ± 45 | passed (2.95 ≥ 2.94), [0, 5] |
| 2026-10-11 | Stage 16: Texel-tuned evaluation values (vs stage 15's hand-set values) | cjus/chessengine#45 | SPRT | 8+0.08, cloud, 3 games at a time | 810 | +310 −313 =187, about −1 ± 21 | no verdict (LLR −0.17), [0, 5]; tuned values not adopted, the default stays hand-set |
| 2026-10-11 | Stage 17: Lazy SMP with a lock-free transposition table, one thread each (vs stage 16; non-regression) | cjus/chessengine#46 | SPRT | 8+0.08, test Mac (M1 Max), 7 games at a time | 15188 | +4196 −4192 =6800, about +0.1 ± 4.1 | passed (2.95 ≥ 2.94), [−5, 0] |
| 2026-10-11 | Stage 17: Lazy SMP, 4 threads vs 1 thread (same build) | cjus/chessengine#46 | SPRT | 8+0.08, test Mac (M1 Max), one game at a time | 38 | +22 −3 =13, about +191 ± 99 | passed (3.11 ≥ 2.94), [0, 25] |
| 2026-10-11 | Stage 18, part 1: Syzygy tablebases + Chess960 start positions | cjus/chessengine#49 | None: no functional change | — | — | — | bench unchanged |
| 2026-10-11 | Stage 18, part 2: Polyglot opening books | cjus/chessengine#50 | None: no functional change | — | — | — | bench unchanged |
| 2026-10-11 | Stage 19: strength levels + MultiPV (levels not yet calibrated, #36) | cjus/chessengine#51 | None: no functional change | — | — | — | bench unchanged |
| 2026-10-11 | Stage 20: XBoard/CECP, pondering, conformance tests, Lichess setup | cjus/chessengine#52 | None: no functional change | — | — | — | bench unchanged |
| 2026-10-11 | M5: check extensions, switched on (vs stage 20; off by default) | cjus/chessengine#57 | SPRT | 8+0.08, cloud, 3 games at a time | 840 | +229 −249 =362, about −8.3 ± 17.7 | no verdict (LLR −0.66), [0, 5]; no gain, so off by default |
| 2026-10-12 | M5: internal iterative reductions (vs `main` without it) | cjus/chessengine#58 | SPRT | 8+0.08, test Mac (M1 Max), 7 games at a time | 2337 | +705 −576 =1056, about +19.2 ± 10.4 | passed (2.96 ≥ 2.94), [0, 5] |
| 2026-10-12 | M5: improving heuristic (vs `main` with IIR; off by default) | cjus/chessengine#60 | SPRT | 8+0.08, test Mac (M1 Max), 7 games at a time | 1142 | +267 −372 =503, about −32.0 ± 15.1 | failed (LLR −2.96), [0, 5]; off by default |
| 2026-10-12 | M5: improving, half a ply off the margin (vs `main` without it; off by default) | cjus/chessengine#63 | SPRT | 8+0.08, test Mac (M3 Max), 9 games at a time | 1691 | +362 −448 =881, about −17.7 ± 11.5 | failed (LLR −2.97), [0, 5]; off by default |
| 2026-10-12 | M5: improving, the reverse futility margin only (vs `main` without it; off by default) | cjus/chessengine#65 | SPRT | 8+0.08, test Mac (M3 Max), 9 games at a time | 1036 | +221 −319 =496, about −33.0 ± 15.3 | failed (LLR −2.96), [0, 5]; off by default, improving set aside |
| 2026-10-13 | M5: continuation history (vs `main` without it) | cjus/chessengine#69 | SPRT | 8+0.08, test Mac (M3 Max), 9 games at a time | 4532 | +1257 −1117 =2158, about +10.7 ± 7.3 | passed (2.96 ≥ 2.94), [0, 5] |
