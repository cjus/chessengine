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
