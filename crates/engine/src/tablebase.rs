//! Endgame tablebases: perfect play with few pieces left.
//!
//! With seven pieces or fewer on the board, every position has been solved: a **tablebase**
//! stores, for each one, whether it is won, drawn or lost with best play. **Syzygy** tables, the
//! format every modern engine uses, come in two kinds:
//!
//! - **WDL** (win/draw/loss) tables answer "who wins?" and are small enough to probe inside the
//!   search. A search that reaches a 5-piece position doesn't need to search it any more: it
//!   knows the result.
//! - **DTZ** (distance to zeroing) tables say how many moves until the next capture or pawn move
//!   on the fastest winning path. They're used at the root, to pick a move that actually makes
//!   progress, so the engine converts a won ending instead of shuffling.
//!
//! Syzygy results respect the fifty-move rule. A **cursed win** can be forced, but not before
//! the fifty-move rule draws the game; a **blessed loss** is its mirror image. cb2026 scores both
//! as draws.
//!
//! cb2026 doesn't read the files itself. The search only sees the [`Tablebase`] trait; the
//! `cb2026-syzygy` crate implements it with the `shakmaty-syzygy` library (the one exception to
//! "no chess crates", see `docs/PRD.md`, "Tablebase probing"). That keeps this crate free of
//! I/O, and tests can use a small in-memory stand-in.
//!
//! Probes need the position to be "clean": no castling rights (the tables don't include them),
//! and in the search only right after a capture or pawn move (halfmove clock 0), because a
//! WDL result assumes the fifty-move counter has just been reset.
//!
//! See the Chess Programming Wiki, [Syzygy Bases](https://www.chessprogramming.org/Syzygy_Bases)
//! and [Endgame Tablebases](https://www.chessprogramming.org/Endgame_Tablebases).

use std::fmt;
use std::sync::Arc;

use cb2026_chess::{Move, Position};

use crate::search::{MATE, MAX_PLY};

/// A tablebase result, from the side to move's point of view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TbWdl {
    /// Lost, even with the fifty-move rule.
    Loss,
    /// Lost, but the fifty-move rule saves the game: a draw.
    BlessedLoss,
    /// Drawn.
    Draw,
    /// Won, but not before the fifty-move rule draws the game: a draw.
    CursedWin,
    /// Won.
    Win,
}

/// A source of tablebase results.
pub trait Tablebase: fmt::Debug + Send + Sync {
    /// The most pieces (kings included) the available tables cover.
    fn max_pieces(&self) -> u32;

    /// The result of `pos` with the fifty-move counter just reset, if the tables cover it.
    fn probe_wdl(&self, pos: &Position) -> Option<TbWdl>;

    /// For a position the tables cover: its result, and the move that keeps the best result
    /// and makes the fastest progress (from the DTZ tables). `None` if there is no table, or
    /// the position has no legal move.
    fn probe_root(&self, pos: &Position) -> Option<(TbWdl, Move)>;
}

/// Opens tablebases from a path setting (several directories separated by `;`, or `:` outside
/// Windows). Supplied by the program that embeds the engine, since opening files is I/O.
pub type TablebaseLoader = fn(&str) -> Result<Arc<dyn Tablebase>, String>;

// ANCHOR: params
/// When to probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TablebaseParams {
    /// Probe in the search only with at least this much depth left (the UCI
    /// `SyzygyProbeDepth`; 1 probes at every node).
    pub probe_depth: u32,
    /// Probe only positions with at most this many pieces (`SyzygyProbeLimit`).
    pub probe_limit: u32,
}

impl Default for TablebaseParams {
    fn default() -> TablebaseParams {
        TablebaseParams {
            probe_depth: 1,
            probe_limit: 7,
        }
    }
}
// ANCHOR_END: params

/// The score of a tablebase win: below every mate score, above every evaluation. A win
/// found `ply` plies from the root scores `TB_WIN - ply`, so nearer wins are preferred.
pub const TB_WIN: i32 = MATE - 2 * MAX_PLY as i32;

/// Whether `pos` is one the tables can answer: few enough pieces and no castling rights.
pub fn covers(tb: &dyn Tablebase, params: &TablebaseParams, pos: &Position) -> bool {
    let pieces = pos.occupied().count();
    pieces <= params.probe_limit.min(tb.max_pieces()) && pos.castling().is_empty()
}

/// The search score for a tablebase result at `ply` plies from the root.
pub fn score(wdl: TbWdl, ply: u32) -> i32 {
    match wdl {
        TbWdl::Win => TB_WIN - ply as i32,
        TbWdl::Loss => -TB_WIN + ply as i32,
        TbWdl::CursedWin | TbWdl::BlessedLoss | TbWdl::Draw => 0,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use cb2026_chess::PieceType;

    /// An in-memory "tablebase" for tests: with exactly two kings and one queen or rook on
    /// the board, the side with the extra piece wins. It covers 3 pieces.
    #[derive(Debug)]
    pub(crate) struct ToyTablebase;

    impl Tablebase for ToyTablebase {
        fn max_pieces(&self) -> u32 {
            3
        }

        fn probe_wdl(&self, pos: &Position) -> Option<TbWdl> {
            if pos.occupied().count() != 3 {
                return None;
            }
            let us = pos.side_to_move();
            let heavy = |color| {
                !(pos.pieces(color, PieceType::Queen) | pos.pieces(color, PieceType::Rook))
                    .is_empty()
            };
            Some(if heavy(us) {
                TbWdl::Win
            } else if heavy(!us) {
                TbWdl::Loss
            } else {
                TbWdl::Draw
            })
        }

        fn probe_root(&self, pos: &Position) -> Option<(TbWdl, Move)> {
            let wdl = self.probe_wdl(pos)?;
            pos.legal_moves().first().map(|&mv| (wdl, mv))
        }
    }

    #[test]
    fn scores_rank_wins_below_mates() {
        assert!(score(TbWdl::Win, 3) < MATE - MAX_PLY as i32);
        assert!(score(TbWdl::Win, 3) > 10_000);
        assert!(score(TbWdl::Win, 3) > score(TbWdl::Win, 5));
        assert_eq!(score(TbWdl::CursedWin, 4), 0);
        assert_eq!(score(TbWdl::Loss, 2), -score(TbWdl::Win, 2));
    }

    #[test]
    fn covers_needs_few_pieces_and_no_castling() {
        let tb = ToyTablebase;
        let params = TablebaseParams::default();
        let kqk = Position::from_fen("8/8/8/4k3/8/8/8/3QK3 w - - 0 1").unwrap();
        assert!(covers(&tb, &params, &kqk));
        let castling = Position::from_fen("4k3/8/8/8/8/8/8/R3K3 w Q - 0 1").unwrap();
        assert!(!covers(&tb, &params, &castling));
        assert!(!covers(&tb, &params, &Position::startpos()));
        let limited = TablebaseParams {
            probe_limit: 2,
            ..params
        };
        assert!(!covers(&tb, &limited, &kqk));
    }
}
