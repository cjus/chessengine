//! # cb2026-syzygy: reading Syzygy endgame tablebases
//!
//! The engine probes tablebases through its `Tablebase` trait (see
//! `cb2026_engine::tablebase`) and never touches files. This crate implements that trait with
//! [`shakmaty-syzygy`](https://crates.io/crates/shakmaty-syzygy) (GPL-3.0-or-later), the
//! prober behind Lichess's tablebase server. It is the one documented exception to the "no
//! chess crates" rule: the Syzygy format is an intricate compressed encoding, and writing a
//! prober from scratch would be thousands of lines for no strength gain (`docs/PRD.md`,
//! "Tablebase probing").
//!
//! `shakmaty` has its own position type, so each probe converts cb2026's position through its
//! FEN. Probes only happen with seven pieces or fewer, where that costs little next to reading
//! the table.
//!
//! The tables themselves are not part of the repository: `scripts/download-syzygy.sh` fetches
//! them, and [`load`] opens a directory of `.rtbw` (WDL) and `.rtbz` (DTZ) files.

#![forbid(unsafe_code)]

use std::sync::Arc;

use cb2026_chess::{Move, Position};
use cb2026_engine::tablebase::{Tablebase, TbWdl};
use shakmaty::fen::Fen;
use shakmaty::uci::UciMove;
use shakmaty::{CastlingMode, Chess};
use shakmaty_syzygy::{AmbiguousWdl, Tablebase as Tables, Wdl};

/// Syzygy tables opened from one or more directories.
pub struct SyzygyTablebase {
    tables: Tables<Chess>,
}

impl std::fmt::Debug for SyzygyTablebase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyzygyTablebase")
            .field("max_pieces", &self.tables.max_pieces())
            .finish()
    }
}

impl SyzygyTablebase {
    /// Opens every table in the directories listed in `path`, separated by `;` (or `:` outside
    /// Windows), like the UCI `SyzygyPath` option.
    pub fn open(path: &str) -> Result<SyzygyTablebase, String> {
        let mut tables = Tables::new();
        let mut found = 0;
        for dir in std::env::split_paths(path).chain(path.split(';').map(Into::into)) {
            if dir.as_os_str().is_empty() || !dir.is_dir() {
                continue;
            }
            found += tables
                .add_directory(&dir)
                .map_err(|e| format!("{}: {e}", dir.display()))?;
        }
        if found == 0 {
            return Err(format!("no Syzygy tables found in '{path}'"));
        }
        Ok(SyzygyTablebase { tables })
    }

    /// `pos` as a `shakmaty` position, through its FEN.
    fn convert(pos: &Position) -> Option<Chess> {
        let fen: Fen = pos.to_fen().parse().ok()?;
        fen.into_position(CastlingMode::Chess960).ok()
    }
}

/// Converts a `shakmaty` result to cb2026's.
fn from_wdl(wdl: Wdl) -> TbWdl {
    match wdl {
        Wdl::Loss => TbWdl::Loss,
        Wdl::BlessedLoss => TbWdl::BlessedLoss,
        Wdl::Draw => TbWdl::Draw,
        Wdl::CursedWin => TbWdl::CursedWin,
        Wdl::Win => TbWdl::Win,
    }
}

impl Tablebase for SyzygyTablebase {
    fn max_pieces(&self) -> u32 {
        self.tables.max_pieces() as u32
    }

    fn probe_wdl(&self, pos: &Position) -> Option<TbWdl> {
        let chess = SyzygyTablebase::convert(pos)?;
        let wdl = self.tables.probe_wdl_after_zeroing(&chess).ok()?;
        Some(from_wdl(wdl))
    }

    fn probe_root(&self, pos: &Position) -> Option<(TbWdl, Move)> {
        let chess = SyzygyTablebase::convert(pos)?;
        let (best, _dtz) = self.tables.best_move(&chess).ok()??;
        let wdl = match self.tables.probe_wdl(&chess).ok()? {
            AmbiguousWdl::Win | AmbiguousWdl::MaybeWin => TbWdl::Win,
            AmbiguousWdl::CursedWin => TbWdl::CursedWin,
            AmbiguousWdl::Draw => TbWdl::Draw,
            AmbiguousWdl::BlessedLoss => TbWdl::BlessedLoss,
            AmbiguousWdl::Loss | AmbiguousWdl::MaybeLoss => TbWdl::Loss,
        };
        let uci = UciMove::from_standard(best).to_string();
        Some((wdl, pos.parse_uci_move(&uci)?))
    }
}

/// Opens tablebases for the engine: the function to put in `Engine::tablebase_loader`.
pub fn load(path: &str) -> Result<Arc<dyn Tablebase>, String> {
    Ok(Arc::new(SyzygyTablebase::open(path)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The directory of test tables (`scripts/download-syzygy.sh` fetches a few small ones,
    /// and CI sets this variable). Without it these tests check nothing.
    fn tables() -> Option<SyzygyTablebase> {
        let path = std::env::var("CB2026_SYZYGY_PATH").ok()?;
        Some(SyzygyTablebase::open(&path).expect("test tables open"))
    }

    fn wdl(tb: &SyzygyTablebase, fen: &str) -> Option<TbWdl> {
        tb.probe_wdl(&Position::from_fen(fen).unwrap())
    }

    #[test]
    fn missing_directory_is_an_error() {
        assert!(SyzygyTablebase::open("/no/such/directory").is_err());
    }

    #[test]
    fn known_results() {
        let Some(tb) = tables() else { return };
        assert!(tb.max_pieces() >= 4);
        // King and queen against king: won for the queen's side, whoever moves.
        assert_eq!(wdl(&tb, "8/8/8/4k3/8/8/8/3QK3 w - - 0 1"), Some(TbWdl::Win));
        assert_eq!(
            wdl(&tb, "8/8/8/4k3/8/8/8/3QK3 b - - 0 1"),
            Some(TbWdl::Loss)
        );
        // King and rook against king and rook: a draw.
        assert_eq!(
            wdl(&tb, "4k3/8/8/r7/8/8/3R4/4K3 w - - 0 1"),
            Some(TbWdl::Draw)
        );
        // King and pawn against king: the king in front of its pawn on the sixth wins; the
        // defending king blockading the pawn draws.
        assert_eq!(
            wdl(&tb, "4k3/8/4K3/4P3/8/8/8/8 w - - 0 1"),
            Some(TbWdl::Win)
        );
        assert_eq!(
            wdl(&tb, "8/8/8/4k3/4P3/4K3/8/8 w - - 0 1"),
            Some(TbWdl::Draw)
        );
    }

    #[test]
    fn root_move_keeps_the_win() {
        let Some(tb) = tables() else { return };
        let pos = Position::from_fen("8/8/8/4k3/8/8/8/3QK3 w - - 0 1").unwrap();
        let (result, mv) = tb.probe_root(&pos).unwrap();
        assert_eq!(result, TbWdl::Win);
        let mut after = pos.clone();
        after.make_move(mv);
        assert_eq!(tb.probe_wdl(&after), Some(TbWdl::Loss));
    }

    #[test]
    fn positions_beyond_the_tables_are_not_answered() {
        let Some(tb) = tables() else { return };
        assert_eq!(tb.probe_wdl(&Position::startpos()), None);
    }
}
