# Elo ledger

The measured strength gain of every stage and technique, so learners can see what each one is
worth and we can see where the engine's strength comes from.

Each row is an SPRT run of the change against the `main` it was merged into (see `docs/PRD.md`,
"Testing and strength measurement"), or a gauntlet result at a milestone gate. Elo is from
self-play unless noted, so it overstates gains against other engines.

| Date | Stage / technique | PR | Test | Time control | Games | Elo ± 95% | LLR (bounds) |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 2026-10-10 | Stage 5: material + negamax alpha-beta (vs stage 4 random mover) | stage 5 PR | SPRT | 8+0.08, cloud, 3 games at a time | 286 | +285 −0 =1 (no meaningful Elo) | passed, [0, 5] |
