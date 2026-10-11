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
