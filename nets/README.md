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
build must compile it in, and the release is made after the build.

## cb2026-gen1-256.nnue (the default)

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
