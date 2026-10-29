# Networks

cb2026's NNUE networks, trained by its own trainer (`crates/trainer`) on its own self-play
games: no other engine's networks or data (`docs/PRD.md`, "Evaluation (NNUE, M6)"). Like the
code, they are `MIT OR Apache-2.0`.

The engine builds the default network into the binary (`cb2026_engine::builtin_network`), so
release and development builds evaluate with it unless told otherwise: UCI `EvalFile` empty,
the shell's `set evalfile none`, or the `nnue` technique switch off gives the hand-crafted
evaluation, and `EvalFile <path>` another network. Strength levels (`Skill Level`,
`UCI_LimitStrength`) use the hand-crafted evaluation, which they were calibrated with. Each
release carries the default network as a download too.

Networks are small (a 256-wide one is 395 KB), so the default one is committed here: a release
build must compile it in, and the release is made after the build. Only the default is here;
earlier ones stay in the git history.

## cb2026-gen3-256.nnue (the default)

- **SHA-256:** `00d6517cd6bad49e7cb4da491c4548f3152f99030fd86d01cd3fa97980005256`
- **Shape:** 768 → 256 × 2 → 1, SCReLU, eval scale 153 (fitted from the data).
- **Data:** 200.7 million positions from 2,520,000 games of gen2 against itself, 6,000 nodes a
  move, balanced starts, twice gen2's data; 29.1 hours on the M3 Max
  (`scripts/mac-nnue-data.sh`, cjus/chessengine#219).
- **Training:** the same as gen1's (below): training loss 0.0096, validation loss 0.0097 (on
  gen3's own data). 13 minutes on the M3 Max's GPU.
- **Strength:** an SPRT against gen2 at 8+0.08 passed after 595 games, +176 −86 =333, about
  +53 Elo ± 19 (cjus/chessengine#222). Its 512-wide sibling was no better at 8+0.08, +1.1 ±
  4.8 in 8,180 games (cjus/chessengine#223), and is 25% slower.

## cb2026-gen2-256.nnue (v1.0.0's network; in the git history)

- **SHA-256:** `1b85528690048d883ba2962f55d1081e41b5adea860f3deab532bd5c8d36d486`
- **Shape:** 768 → 256 × 2 → 1, SCReLU, eval scale 149 (fitted from the data).
- **Data:** 102.9 million positions from 1,288,000 games of gen1 against itself, 6,000 nodes a
  move, balanced starts; 15.1 hours on the M3 Max (`scripts/mac-nnue-data.sh`,
  cjus/chessengine#211).
- **Training:** the same as gen1's (below): training loss 0.0094, validation loss 0.0097 (on
  gen2's own data, so not comparable with gen1's). 7 minutes on the M3 Max's GPU.
- **Strength:** an SPRT against gen1 at 8+0.08 passed after 209 games, +98 −16 =95, about +144
  Elo ± 35 (cjus/chessengine#214).

## cb2026-gen1-256.nnue (the first network; in the git history)

- **SHA-256:** `ff8692c9b527bb95d5b6572b7b4af2b978d8a6b5e275434756da520eeaa16309`
- **Shape:** 768 → 256 × 2 → 1, SCReLU, eval scale 167 (fitted from the data).
- **Data:** 104.2 million positions from 1,344,000 games of the hand-crafted evaluation against
  itself, 6,000 nodes a move, balanced starts; 12.4 hours on the M3 Max
  (`scripts/mac-nnue-data.sh`, cjus/chessengine#190).
- **Training:** 20 epochs, batches of 16,384, AdamW, a quarter game result in each target, the
  last 1% of the records held back: training loss 0.0090, validation loss 0.0094. 7 minutes on
  the M3 Max's GPU.
- **Strength:** an SPRT against the hand-crafted evaluation at 8+0.08 passed 14–0
  (cjus/chessengine#195); 200 games at 1+0.01 gave about +424 Elo ± 71.

Book chapter 21 tells the whole story.
