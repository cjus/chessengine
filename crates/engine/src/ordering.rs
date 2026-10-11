//! Move ordering: which moves to search first.
//!
//! Alpha-beta gives the same answer whatever order the moves come in, but its speed depends
//! hugely on that order. If the best move is searched first, every other move is refuted by
//! the first reply that's good enough, and the tree shrinks from about `b^d` positions toward
//! `b^(d/2)`. Searched last, it saves almost nothing.
//!
//! ## MVV-LVA
//!
//! Captures are the moves most likely to be best, and the cheapest good guess at which capture
//! is best is **Most Valuable Victim, Least Valuable Attacker**: take the biggest piece you
//! can, and among the ways to take it, use your smallest piece. Pawn takes queen comes first,
//! queen takes pawn last among captures. It ignores whether the capturing piece is defended
//! against; stage 14's static exchange evaluation (SEE) does better.
//!
//! Promotions to a queen are treated like captures of a queen-sized gain, so they're searched
//! early too.
//!
//! ## Ordering quiet moves (stage 10)
//!
//! Most moves are quiet, and MVV-LVA says nothing about them. Three cheap heuristics learn
//! which quiet moves tend to cause cutoffs, from the search itself:
//!
//! - **Killer moves.** A quiet move that caused a beta cutoff at some ply probably refutes the
//!   other moves at that ply too: if Qxh7# works after one of White's moves, it likely works
//!   after White's next try. Two killers are kept per ply.
//! - **Countermoves.** The quiet move that last refuted a given opponent move (identified by
//!   the piece that moved and its destination) is tried early when that move is played again.
//! - **History.** Every quiet move that causes a cutoff gets a bonus in a table indexed by side,
//!   from-square and to-square, bigger for deeper searches; the quiet moves searched before it
//!   at that node get a penalty. Quiet moves are tried in order of their history score. The
//!   update uses "gravity": bonuses shrink as a score nears its cap, so scores stay bounded
//!   and old knowledge fades.
//!
//! ## The order, and staged picking
//!
//! 1. The transposition table move.
//! 2. Captures and queen promotions, by MVV-LVA (from stage 14, only those that don't lose
//!    material by static exchange evaluation).
//! 3. The two killers, then the countermove.
//! 4. The other quiet moves, by history.
//! 5. Captures that lose material (stage 14).
//!
//! The moves are not sorted up front. A [`MovePicker`] picks the best remaining move each time
//! one is asked for. Most nodes that cut off do so on the first move or two, so the work of
//! ordering the rest is never done. The quiescence search goes further and generates only
//! captures and promotions ([`Position::noisy_moves`]).
//!
//! See the Chess Programming Wiki, [Move Ordering](https://www.chessprogramming.org/Move_Ordering),
//! [MVV-LVA](https://www.chessprogramming.org/MVV-LVA),
//! [Killer Heuristic](https://www.chessprogramming.org/Killer_Heuristic),
//! [Countermove Heuristic](https://www.chessprogramming.org/Countermove_Heuristic) and
//! [History Heuristic](https://www.chessprogramming.org/History_Heuristic).

use cb2026_chess::{Color, Move, MoveKind, PieceType, Position};

use crate::search::{MAX_PLY, Techniques};
use crate::see::see;

// ANCHOR: mvv_lva
/// The MVV-LVA key of `mv` in `pos`: higher means search earlier. Quiet moves score 0;
/// captures and queen promotions score above 0, by victim (or promoted piece) first and by
/// attacker second.
///
/// The key uses the pieces' *rank* (pawn 1 … queen 5), not their values: only the order
/// matters, and ranks keep the victim strictly more important than the attacker.
pub fn mvv_lva(pos: &Position, mv: Move) -> i32 {
    let rank = |pt: PieceType| pt.index() as i32 + 1;
    let mut gain = 0;
    if mv.is_capture() {
        let victim = match mv.kind() {
            MoveKind::EnPassant => PieceType::Pawn,
            _ => pos
                .piece_at(mv.to())
                .map_or(PieceType::Pawn, |p| p.piece_type),
        };
        gain += rank(victim);
    }
    if mv.promotion() == Some(PieceType::Queen) {
        gain += rank(PieceType::Queen);
    }
    if gain == 0 {
        return 0;
    }
    let attacker = pos
        .piece_at(mv.from())
        .map_or(PieceType::Pawn, |p| p.piece_type);
    // Six attacker ranks fit between consecutive victim ranks, so the victim always decides.
    gain * 8 - rank(attacker)
}
// ANCHOR_END: mvv_lva

/// Sorts `moves` best first by [`mvv_lva`]. The sort is stable, so moves with equal keys (all
/// the quiet moves, for example) keep their generation order.
pub fn order_moves(pos: &Position, moves: &mut [Move]) {
    moves.sort_by_cached_key(|&mv| -mvv_lva(pos, mv));
}

/// Whether the quiescence search looks at `mv`: captures and queen promotions (but not
/// under-promotions, which almost never matter).
pub fn is_noisy(mv: Move) -> bool {
    match mv.promotion() {
        Some(piece) => piece == PieceType::Queen,
        None => mv.is_capture(),
    }
}

/// Ordering keys: higher is searched first. The bands keep the stages apart; history scores
/// stay within ±[`HISTORY_MAX`], below every band.
const TT_MOVE_KEY: i32 = 3_000_000;
const NOISY_KEY: i32 = 2_000_000;
const KILLER_KEYS: [i32; 2] = [1_000_000, 900_000];
const COUNTERMOVE_KEY: i32 = 800_000;
const BAD_CAPTURE_KEY: i32 = -1_000_000;
/// The largest a history score can get (in either direction).
pub const HISTORY_MAX: i32 = 16_384;

// ANCHOR: tables
/// What the search has learned about quiet moves: killers per ply, countermoves, history.
#[derive(Clone, Debug)]
pub struct OrderingTables {
    /// Two killer moves per ply.
    killers: Vec<[Option<Move>; 2]>,
    /// The quiet move that last refuted a move, by the piece that moved and its destination.
    countermoves: [[Option<Move>; 64]; 12],
    /// Cutoff history by side, from-square and to-square.
    history: Box<[[[i32; 64]; 64]; 2]>,
}
// ANCHOR_END: tables

impl Default for OrderingTables {
    fn default() -> OrderingTables {
        OrderingTables {
            killers: vec![[None; 2]; MAX_PLY as usize + 1],
            countermoves: [[None; 64]; 12],
            history: Box::new([[[0; 64]; 64]; 2]),
        }
    }
}

/// The countermove table's index for the piece standing on `mv`'s destination.
fn piece_slot(pos: &Position, mv: Move) -> Option<usize> {
    pos.piece_at(mv.to())
        .map(|p| p.color.index() * 6 + p.piece_type.index())
}

impl OrderingTables {
    /// Forgets everything (a new game).
    pub fn clear(&mut self) {
        *self = OrderingTables::default();
    }

    /// Prepares for a new search: killers are about the old search's plies, so they go;
    /// history is halved, so it still helps but new results soon dominate.
    pub fn new_search(&mut self) {
        self.killers.fill([None; 2]);
        for value in self.history.iter_mut().flatten().flatten() {
            *value /= 2;
        }
    }

    /// The history score of `mv` for `side`.
    pub fn history(&self, side: Color, mv: Move) -> i32 {
        self.history[side.index()][mv.from().index()][mv.to().index()]
    }

    /// The killer moves at `ply`.
    pub fn killers(&self, ply: u32) -> [Option<Move>; 2] {
        self.killers[ply as usize]
    }

    /// The countermove to `previous` (the move that led to `pos`), if one is known.
    pub fn countermove(&self, pos: &Position, previous: Option<Move>) -> Option<Move> {
        let prev = previous?;
        self.countermoves[piece_slot(pos, prev)?][prev.to().index()]
    }

    // ANCHOR: update
    /// Records that the quiet move `best` caused a beta cutoff at `ply` with `depth` left,
    /// after the quiet moves in `tried` failed to.
    pub fn record_cutoff(
        &mut self,
        pos: &Position,
        best: Move,
        tried: &[Move],
        ply: u32,
        depth: u32,
        previous: Option<Move>,
    ) {
        let killers = &mut self.killers[ply as usize];
        if killers[0] != Some(best) {
            killers[1] = killers[0];
            killers[0] = Some(best);
        }
        if let Some(prev) = previous
            && let Some(slot) = piece_slot(pos, prev)
        {
            self.countermoves[slot][prev.to().index()] = Some(best);
        }
        let side = pos.side_to_move().index();
        let bonus = (depth * depth).min(HISTORY_MAX as u32) as i32;
        let mut update = |mv: Move, change: i32| {
            let entry = &mut self.history[side][mv.from().index()][mv.to().index()];
            // Gravity: the nearer the cap, the smaller the change, so scores stay in range.
            *entry += change - *entry * change.abs() / HISTORY_MAX;
        };
        update(best, bonus);
        for &mv in tried {
            update(mv, -bonus);
        }
    }
    // ANCHOR_END: update
}

// ANCHOR: picker
/// Hands out moves best first, choosing each one only when it is asked for.
pub struct MovePicker {
    moves: Vec<(Move, i32)>,
    next: usize,
}

impl MovePicker {
    /// Orders `moves` in `pos` for the search at `ply`. `tt_move` comes first; `previous` is
    /// the move that led here (for the countermove).
    pub fn new(
        pos: &Position,
        moves: &[Move],
        tt_move: Option<Move>,
        ply: u32,
        previous: Option<Move>,
        tables: &OrderingTables,
        techniques: &Techniques,
    ) -> MovePicker {
        let killers = if techniques.killers {
            tables.killers(ply)
        } else {
            [None; 2]
        };
        let countermove = if techniques.countermoves {
            tables.countermove(pos, previous)
        } else {
            None
        };
        let side = pos.side_to_move();
        let key = |mv: Move| {
            if Some(mv) == tt_move {
                TT_MOVE_KEY
            } else if techniques.mvv_lva && is_noisy(mv) {
                // Captures that lose material in the exchange go after the quiet moves.
                if techniques.see_ordering && see(pos, mv) < 0 {
                    BAD_CAPTURE_KEY + mvv_lva(pos, mv)
                } else {
                    NOISY_KEY + mvv_lva(pos, mv)
                }
            } else if Some(mv) == killers[0] {
                KILLER_KEYS[0]
            } else if Some(mv) == killers[1] {
                KILLER_KEYS[1]
            } else if Some(mv) == countermove {
                COUNTERMOVE_KEY
            } else if techniques.history {
                tables.history(side, mv)
            } else {
                0
            }
        };
        MovePicker {
            moves: moves.iter().map(|&mv| (mv, key(mv))).collect(),
            next: 0,
        }
    }
}

impl Iterator for MovePicker {
    type Item = Move;

    /// The best move not yet handed out: one pass of a selection sort. Ties go to the move
    /// generated first, so the order is deterministic.
    fn next(&mut self) -> Option<Move> {
        let rest = self.moves.get(self.next..)?;
        let mut best = 0;
        for (i, &(_, key)) in rest.iter().enumerate() {
            if key > rest[best].1 {
                best = i;
            }
        }
        let (mv, _) = *rest.get(best)?;
        // Move the chosen one to the front; keep the others in their order.
        self.moves[self.next..=self.next + best].rotate_right(1);
        self.next += 1;
        Some(mv)
    }
}
// ANCHOR_END: picker

#[cfg(test)]
mod tests {
    use super::*;

    fn ordered(fen: &str) -> Vec<String> {
        let pos = Position::from_fen(fen).unwrap();
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        order_moves(&pos, &mut moves);
        moves.iter().map(|mv| pos.to_san(*mv)).collect()
    }

    #[test]
    fn biggest_victim_first_then_smallest_attacker() {
        // White can take the queen on d5 with the pawn or the rook, or the knight on f5 with
        // the pawn.
        let moves = ordered("4k3/8/8/3q1n2/4P3/8/8/3RK3 w - - 0 1");
        assert_eq!(&moves[..3], ["exd5", "Rxd5", "exf5"]);
    }

    #[test]
    fn queen_promotions_come_early_and_quiet_moves_last() {
        let pos = Position::from_fen("4k3/1P6/8/8/8/8/8/4K2R w K - 0 1").unwrap();
        let mut moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        order_moves(&pos, &mut moves);
        assert_eq!(pos.to_san(moves[0]), "b8=Q+");
        assert!(moves[1..].iter().all(|&mv| mvv_lva(&pos, mv) == 0));
    }

    #[test]
    fn en_passant_is_a_pawn_capture() {
        let pos = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        let ep = pos.parse_uci_move("e5d6").unwrap();
        assert_eq!(mvv_lva(&pos, ep), 8 - 1);
        assert!(is_noisy(ep));
    }

    #[test]
    fn picker_order_tt_move_captures_killers_countermove_history() {
        // White can take the queen on d5 (exd5, Rxd5) or play quiet moves.
        let pos = Position::from_fen("4k3/8/8/3q4/4P3/8/8/3RK3 w - - 0 1").unwrap();
        let moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let mut tables = OrderingTables::default();
        // Rd2 is a killer at ply 3, Rd3 has history.
        tables.record_cutoff(&pos, mv("d1d3"), &[], 9, 10, None);
        tables.record_cutoff(&pos, mv("d1d2"), &[], 3, 1, None);
        let picked: Vec<String> = MovePicker::new(
            &pos,
            &moves,
            Some(mv("e1f2")),
            3,
            None,
            &tables,
            &Techniques::default(),
        )
        .map(|m| pos.to_san(m))
        .collect();
        assert_eq!(&picked[..4], ["Kf2", "exd5", "Rxd5", "Rd2"]);
        // Rd3's history beats the other quiet moves.
        assert_eq!(picked[4], "Rd3");
        assert_eq!(picked.len(), moves.len());
    }

    #[test]
    fn killers_shift_and_history_stays_bounded() {
        let pos = Position::startpos();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let mut tables = OrderingTables::default();
        tables.record_cutoff(&pos, mv("g1f3"), &[], 2, 3, None);
        tables.record_cutoff(&pos, mv("b1c3"), &[], 2, 3, None);
        tables.record_cutoff(&pos, mv("b1c3"), &[], 2, 3, None);
        assert_eq!(tables.killers(2), [Some(mv("b1c3")), Some(mv("g1f3"))]);
        for _ in 0..1000 {
            tables.record_cutoff(&pos, mv("e2e4"), &[mv("a2a3")], 5, 40, None);
        }
        let side = Color::White;
        assert!(tables.history(side, mv("e2e4")) <= HISTORY_MAX);
        assert!(tables.history(side, mv("e2e4")) > HISTORY_MAX / 2);
        assert!(tables.history(side, mv("a2a3")) >= -HISTORY_MAX);
        assert!(tables.history(side, mv("a2a3")) < 0);
        tables.new_search();
        assert_eq!(tables.killers(2), [None, None]);
        assert!(tables.history(side, mv("e2e4")) > 0);
    }

    #[test]
    fn countermove_is_remembered_by_piece_and_square() {
        let mut pos = Position::startpos();
        let e4 = pos.parse_uci_move("e2e4").unwrap();
        pos.make_move(e4);
        let reply = pos.parse_uci_move("c7c5").unwrap();
        let mut tables = OrderingTables::default();
        tables.record_cutoff(&pos, reply, &[], 1, 2, Some(e4));
        assert_eq!(tables.countermove(&pos, Some(e4)), Some(reply));
        assert_eq!(tables.countermove(&pos, None), None);
    }

    #[test]
    fn under_promotions_are_quiet_for_quiescence() {
        let pos = Position::from_fen("4k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let knight = pos.parse_uci_move("b7b8n").unwrap();
        let queen = pos.parse_uci_move("b7b8q").unwrap();
        assert!(!is_noisy(knight));
        assert!(is_noisy(queen));
    }
}
