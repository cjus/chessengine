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
//! ## Continuation history (M5)
//!
//! History asks whether a quiet move has been good *anywhere*. **Continuation history** asks
//! whether it has been good *after the moves just played*: a table indexed by an earlier move
//! (the piece that moved and its destination) and the candidate (its piece and destination).
//! cb2026 keeps two, after the opponent's last move (one ply back) and after the side to
//! move's own move before that (two plies back). A quiet move's ordering score is the sum of
//! its history and both continuation scores, and a cutoff updates all three the same way, with
//! the same bonus, penalty and gravity.
//!
//! It refines the countermove: the countermove table remembers one reply per opponent move,
//! continuation history scores every reply. The two-ply table catches plans: after Nf3, a
//! knight move to g5 or e5 is often the follow-up, whatever the opponent did in between.
//!
//! The pitfalls are size and dilution. Each table has 768 × 768 entries (12 pieces × 64
//! squares, twice), about 2.4 MB, so it learns more slowly than the 2 × 64 × 64 history
//! table and costs cache misses. Summing the scores lets a good continuation lift a move that
//! history rates badly, which is the point, but also lets a stale one mislead.
//!
//! The idea is from Stockfish (GPL-3.0), which calls it continuation history (counter-move
//! history and follow-up history in its first versions). This code is cb2026's own.
//!
//! ## Capture history (M5)
//!
//! MVV-LVA orders captures by what they take and what takes, and nothing else: Bxf7 comes
//! before Rxf7 in every position, whether or not the bishop's capture has ever worked.
//! **Capture history** learns which captures cause cutoffs, as history does for quiet moves. Its
//! table is indexed by the piece that moves, its destination and the piece it captures (or
//! nothing, for a queen promotion that captures nothing): 768 × 6 scores.
//!
//! When a move causes a beta cutoff, the captures and queen promotions searched before it at
//! that node get a penalty, whether the move that cut off was a capture or quiet; if it was a
//! capture or queen promotion itself, it gets a bonus. The bonus, the gravity and the halving
//! between searches are history's.
//!
//! A capture's ordering key is then [`capture_value`], what it wins in material by static
//! exchange evaluation's piece values, times [`CAPTURE_VALUE_SCALE`], plus its capture history
//! score. The attacker only breaks ties, so among captures of the same piece, history decides
//! as soon as it tells them apart.
//! A pawn is worth a fifth of [`HISTORY_MAX`] and a queen 1.76 times it, so a capture of a pawn
//! with the best history there is comes before a capture of a rook with none, but never before
//! a capture of a queen. The split by static exchange evaluation stays: a losing capture still
//! goes after the quiet moves however good its history.
//!
//! The Chess Programming Wiki credits capture history to Stefan Geschwentner (2016). This form,
//! the victim's value plus capture history with no attacker term, is Stockfish's (GPL-3.0), in
//! about its proportions (see [`CAPTURE_VALUE_SCALE`]); the code is cb2026's own.
//!
//! **The first version** kept MVV-LVA's attacker term: its key was the MVV-LVA key times 1,024
//! plus capture history, so history moved a capture two victim ranks at most. It took 11.5%
//! off the bench and measured weaker: −19.2 ± 11.9 Elo after 1,561 games at 8+0.08
//! (cjus/chessengine#115).
//!
//! **Off by default.** The second version measured weaker too: −6.8 ± 7.8 Elo after 3,512
//! games at 8+0.08 (cjus/chessengine#129), though it took 12.1% off the bench. The switch and
//! the table stay, for another try.
//!
//! ## The order, and staged picking
//!
//! 1. The transposition table move.
//! 2. Captures and queen promotions, by MVV-LVA or capture history (from stage 14, only those
//!    that don't lose material by static exchange evaluation).
//! 3. The two killers, then the countermove.
//! 4. The other quiet moves, by history plus continuation history.
//! 5. Captures that lose material (stage 14), in the same order as in 2.
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
//! [History Heuristic](https://www.chessprogramming.org/History_Heuristic) (its section
//! "Capture History").

use cb2026_chess::{Color, Move, MoveKind, PieceType, Position};

use crate::search::{MAX_PLY, Techniques};
use crate::see::{SEE_VALUES, see};

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

// ANCHOR: capture_value
/// What `mv`, a capture or queen promotion in `pos`, wins in material if nothing is taken back,
/// by static exchange evaluation's piece values: the victim's value, plus a queen less a pawn
/// for a queen promotion. Capture history's ordering starts from this.
pub fn capture_value(pos: &Position, mv: Move) -> i32 {
    let value = |pt: PieceType| SEE_VALUES[pt.index()];
    let mut gain = 0;
    if mv.is_capture() {
        let victim = match mv.kind() {
            MoveKind::EnPassant => PieceType::Pawn,
            _ => pos
                .piece_at(mv.to())
                .map_or(PieceType::Pawn, |p| p.piece_type),
        };
        gain += value(victim);
    }
    if mv.promotion() == Some(PieceType::Queen) {
        gain += value(PieceType::Queen) - value(PieceType::Pawn);
    }
    gain
}
// ANCHOR_END: capture_value

/// Whether the quiescence search looks at `mv`: captures and queen promotions (but not
/// under-promotions, which almost never matter).
pub fn is_noisy(mv: Move) -> bool {
    match mv.promotion() {
        Some(piece) => piece == PieceType::Queen,
        None => mv.is_capture(),
    }
}

/// Ordering keys: higher is searched first. The bands keep the stages apart; a quiet move's
/// score, the sum of three history scores, stays within ±3 × [`HISTORY_MAX`], below every
/// band, and a capture's key (at most 1,700 × [`CAPTURE_VALUE_SCALE`] + [`HISTORY_MAX`], about
/// 71,000, and at least 100 × [`CAPTURE_VALUE_SCALE`] − [`HISTORY_MAX`], about −13,000) stays
/// within 100,000 of its band's.
const TT_MOVE_KEY: i32 = 3_000_000;
const NOISY_KEY: i32 = 2_000_000;
const KILLER_KEYS: [i32; 2] = [1_000_000, 900_000];
const COUNTERMOVE_KEY: i32 = 800_000;
const BAD_CAPTURE_KEY: i32 = -1_000_000;
/// The largest a history score can get (in either direction).
pub const HISTORY_MAX: i32 = 16_384;
/// With capture history on, a capture's [`capture_value`] (in centipawns) is multiplied by this
/// before its capture history score is added. A pawn is then worth 3,200, a fifth of
/// [`HISTORY_MAX`], and a queen 28,800, 1.76 times it. Stockfish 16 (GPL-3.0) adds 7 times the
/// victim's middlegame value to a capture history limited to 10,692, which puts a queen at 1.66
/// times the limit.
pub const CAPTURE_VALUE_SCALE: i32 = HISTORY_MAX / 512;
/// How many earlier moves continuation history looks back at: one ply (the opponent's last
/// move) and two (the side to move's own move before that).
pub const CONTINUATION_PLIES: usize = 2;
/// The number of (piece, destination) pairs: 12 pieces × 64 squares.
const PIECE_TO_COUNT: usize = 12 * 64;
/// What a capture or queen promotion can take, for capture history: a pawn, knight, bishop,
/// rook or queen (by piece type index, 0 to 4), or nothing ([`NO_VICTIM`]).
const VICTIMS: usize = 6;
/// The victim slot of a queen promotion that captures nothing. A king is never captured, so
/// the king's index is free for it.
const NO_VICTIM: usize = 5;

// ANCHOR: piece_to
/// A move as continuation history sees it: the piece that moved and where it went.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PieceTo {
    /// The piece, `color × 6 + piece type`.
    piece: usize,
    /// The destination square.
    to: usize,
}

impl PieceTo {
    /// `mv`, about to be played in `pos`. `None` if no piece stands on its from-square, which
    /// a legal move never does.
    pub fn new(pos: &Position, mv: Move) -> Option<PieceTo> {
        let piece = pos.piece_at(mv.from())?;
        Some(PieceTo {
            piece: piece.color.index() * 6 + piece.piece_type.index(),
            to: mv.to().index(),
        })
    }

    /// A number from 0 to 767, for indexing.
    fn index(self) -> usize {
        self.piece * 64 + self.to
    }
}

/// The moves that led to a node, for the heuristics that look back at them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Previous {
    /// The move that led here, for the countermove (`None` at the root and after a null move).
    pub mv: Option<Move>,
    /// The last moves by piece and destination, most recent first, for continuation history.
    pub moved: [Option<PieceTo>; CONTINUATION_PLIES],
}
// ANCHOR_END: piece_to

// ANCHOR: tables
/// What the search has learned about which moves cause cutoffs: killers per ply, countermoves,
/// history and continuation history for quiet moves, and capture history.
#[derive(Clone, Debug)]
pub struct OrderingTables {
    /// Two killer moves per ply.
    killers: Vec<[Option<Move>; 2]>,
    /// The quiet move that last refuted a move, by the piece that moved and its destination.
    countermoves: [[Option<Move>; 64]; 12],
    /// Cutoff history by side, from-square and to-square.
    history: Box<[[[i32; 64]; 64]; 2]>,
    /// Continuation history by plies back, the earlier move and the move: one flat table of
    /// `CONTINUATION_PLIES × 768 × 768` scores (see [`continuation_index`]).
    continuation: Vec<i32>,
    /// Capture history by the piece that moves, its destination and its victim: `768 × 6`
    /// scores (see [`capture_index`]).
    captures: Vec<i32>,
}
// ANCHOR_END: tables

impl Default for OrderingTables {
    fn default() -> OrderingTables {
        OrderingTables {
            killers: vec![[None; 2]; MAX_PLY as usize + 1],
            countermoves: [[None; 64]; 12],
            history: Box::new([[[0; 64]; 64]; 2]),
            // A flat `Vec`, not nested arrays in a `Box`: those can be built on the stack
            // first, and at 4.7 MB they would overflow a search thread's.
            continuation: vec![0; CONTINUATION_PLIES * PIECE_TO_COUNT * PIECE_TO_COUNT],
            captures: vec![0; PIECE_TO_COUNT * VICTIMS],
        }
    }
}

/// Where the score of `mv` after `earlier`, `back` plies before it, is in the continuation
/// table (`back` is 0 for one ply back).
fn continuation_index(back: usize, earlier: PieceTo, mv: PieceTo) -> usize {
    (back * PIECE_TO_COUNT + earlier.index()) * PIECE_TO_COUNT + mv.index()
}

// ANCHOR: capture_index
/// Where `mv`, a capture or queen promotion in `pos`, is in the capture history table: by the
/// piece that moves, its destination and what it captures. `None` if no piece stands on its
/// from-square, which a legal move never does.
fn capture_index(pos: &Position, mv: Move) -> Option<usize> {
    let victim = match mv.kind() {
        MoveKind::EnPassant => PieceType::Pawn.index(),
        _ => pos
            .piece_at(mv.to())
            .map_or(NO_VICTIM, |p| p.piece_type.index()),
    };
    Some(PieceTo::new(pos, mv)?.index() * VICTIMS + victim)
}
// ANCHOR_END: capture_index

/// The countermove table's index for the piece standing on `mv`'s destination.
fn piece_slot(pos: &Position, mv: Move) -> Option<usize> {
    pos.piece_at(mv.to())
        .map(|p| p.color.index() * 6 + p.piece_type.index())
}

// ANCHOR: gravity
/// The history bonus for a cutoff with `depth` plies left (and the penalty for the moves that
/// failed before it): `depth²`, since cutoffs deep in the tree save the most work.
fn cutoff_bonus(depth: u32) -> i32 {
    (depth * depth).min(HISTORY_MAX as u32) as i32
}

/// Adds `change` to a history score with gravity: the nearer the cap, the smaller the change,
/// so scores stay within ±[`HISTORY_MAX`].
fn gravity(entry: &mut i32, change: i32) {
    *entry += change - *entry * change.abs() / HISTORY_MAX;
}
// ANCHOR_END: gravity

impl OrderingTables {
    /// Forgets everything (a new game).
    pub fn clear(&mut self) {
        *self = OrderingTables::default();
    }

    /// Prepares for a new search: killers are about the old search's plies, so they go; the
    /// history tables are halved, so they still help but new results soon dominate.
    pub fn new_search(&mut self) {
        self.killers.fill([None; 2]);
        for value in self.history.iter_mut().flatten().flatten() {
            *value /= 2;
        }
        for value in self.continuation.iter_mut().chain(&mut self.captures) {
            *value /= 2;
        }
    }

    /// The history score of `mv` for `side`.
    pub fn history(&self, side: Color, mv: Move) -> i32 {
        self.history[side.index()][mv.from().index()][mv.to().index()]
    }

    /// The continuation history score of `mv` in `pos`: its scores after each of the moves
    /// in `previous`, added up.
    pub fn continuation(&self, pos: &Position, mv: Move, previous: &Previous) -> i32 {
        let Some(this) = PieceTo::new(pos, mv) else {
            return 0;
        };
        previous
            .moved
            .iter()
            .enumerate()
            .filter_map(|(back, earlier)| {
                Some(self.continuation[continuation_index(back, (*earlier)?, this)])
            })
            .sum()
    }

    /// The score that orders the quiet moves: the history score of `mv` in `pos`, plus its
    /// continuation history score after `previous`, each if switched on. Late move reductions
    /// use it too, to reduce a move with a good score less.
    pub fn quiet_score(
        &self,
        pos: &Position,
        mv: Move,
        previous: &Previous,
        techniques: &Techniques,
    ) -> i32 {
        let mut score = 0;
        if techniques.history {
            score += self.history(pos.side_to_move(), mv);
        }
        if techniques.continuation_history {
            score += self.continuation(pos, mv, previous);
        }
        score
    }

    /// The capture history score of `mv`, a capture or queen promotion in `pos`.
    pub fn capture_history(&self, pos: &Position, mv: Move) -> i32 {
        capture_index(pos, mv).map_or(0, |index| self.captures[index])
    }

    // ANCHOR: noisy_score
    /// The score that orders `mv`, a capture or queen promotion in `pos`, among the others:
    /// its MVV-LVA key, or with capture history on, the material it wins scaled up plus its
    /// capture history score, less the attacker's rank (pawn 1 … king 6) to break ties.
    pub fn noisy_score(&self, pos: &Position, mv: Move, techniques: &Techniques) -> i32 {
        if techniques.capture_history {
            // The attacker's rank is smaller than any history update from depth 3 on (a bonus
            // of 9), so it only decides between captures that history can't tell apart, at
            // first all of them: then the smaller attacker goes first, as in MVV-LVA.
            let attacker = pos
                .piece_at(mv.from())
                .map_or(1, |p| p.piece_type.index() as i32 + 1);
            capture_value(pos, mv) * CAPTURE_VALUE_SCALE + self.capture_history(pos, mv) - attacker
        } else {
            mvv_lva(pos, mv)
        }
    }
    // ANCHOR_END: noisy_score

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
    /// after the quiet moves in `tried` failed to. `previous` are the moves that led here.
    pub fn record_cutoff(
        &mut self,
        pos: &Position,
        best: Move,
        tried: &[Move],
        ply: u32,
        depth: u32,
        previous: &Previous,
    ) {
        let killers = &mut self.killers[ply as usize];
        if killers[0] != Some(best) {
            killers[1] = killers[0];
            killers[0] = Some(best);
        }
        if let Some(prev) = previous.mv
            && let Some(slot) = piece_slot(pos, prev)
        {
            self.countermoves[slot][prev.to().index()] = Some(best);
        }
        let side = pos.side_to_move().index();
        let bonus = cutoff_bonus(depth);
        let mut update = |mv: Move, change: i32| {
            gravity(
                &mut self.history[side][mv.from().index()][mv.to().index()],
                change,
            );
            // Continuation history: the same change, after each earlier move.
            let Some(this) = PieceTo::new(pos, mv) else {
                return;
            };
            for (back, earlier) in previous.moved.iter().enumerate() {
                if let Some(earlier) = *earlier {
                    let index = continuation_index(back, earlier, this);
                    gravity(&mut self.continuation[index], change);
                }
            }
        };
        update(best, bonus);
        for &mv in tried {
            update(mv, -bonus);
        }
    }
    // ANCHOR_END: update

    // ANCHOR: capture_update
    /// Records a beta cutoff with `depth` left in capture history: `best` is the move that
    /// caused it if that was a capture or queen promotion (`None` if it was quiet), and `tried`
    /// are the captures and queen promotions searched before it, which failed to.
    pub fn record_captures(
        &mut self,
        pos: &Position,
        best: Option<Move>,
        tried: &[Move],
        depth: u32,
    ) {
        let bonus = cutoff_bonus(depth);
        let mut update = |mv: Move, change: i32| {
            if let Some(index) = capture_index(pos, mv) {
                gravity(&mut self.captures[index], change);
            }
        };
        if let Some(best) = best {
            update(best, bonus);
        }
        for &mv in tried {
            update(mv, -bonus);
        }
    }
    // ANCHOR_END: capture_update
}

// ANCHOR: picker
/// Hands out moves best first, choosing each one only when it is asked for.
pub struct MovePicker {
    moves: Vec<(Move, i32)>,
    next: usize,
}

impl MovePicker {
    /// Orders `moves` in `pos` for the search at `ply`. `tt_move` comes first; `previous` are
    /// the moves that led here (for the countermove and continuation history).
    pub fn new(
        pos: &Position,
        moves: &[Move],
        tt_move: Option<Move>,
        ply: u32,
        previous: &Previous,
        tables: &OrderingTables,
        techniques: &Techniques,
    ) -> MovePicker {
        let killers = if techniques.killers {
            tables.killers(ply)
        } else {
            [None; 2]
        };
        let countermove = if techniques.countermoves {
            tables.countermove(pos, previous.mv)
        } else {
            None
        };
        let key = |mv: Move| {
            if Some(mv) == tt_move {
                TT_MOVE_KEY
            } else if techniques.mvv_lva && is_noisy(mv) {
                let score = tables.noisy_score(pos, mv, techniques);
                // Captures that lose material in the exchange go after the quiet moves.
                if techniques.see_ordering && see(pos, mv) < 0 {
                    BAD_CAPTURE_KEY + score
                } else {
                    NOISY_KEY + score
                }
            } else if Some(mv) == killers[0] {
                KILLER_KEYS[0]
            } else if Some(mv) == killers[1] {
                KILLER_KEYS[1]
            } else if Some(mv) == countermove {
                COUNTERMOVE_KEY
            } else {
                tables.quiet_score(pos, mv, previous, techniques)
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
        tables.record_cutoff(&pos, mv("d1d3"), &[], 9, 10, &Previous::default());
        tables.record_cutoff(&pos, mv("d1d2"), &[], 3, 1, &Previous::default());
        let picked: Vec<String> = MovePicker::new(
            &pos,
            &moves,
            Some(mv("e1f2")),
            3,
            &Previous::default(),
            &tables,
            // History on, whatever the default.
            &Techniques {
                history: true,
                ..Techniques::default()
            },
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
        tables.record_cutoff(&pos, mv("g1f3"), &[], 2, 3, &Previous::default());
        tables.record_cutoff(&pos, mv("b1c3"), &[], 2, 3, &Previous::default());
        tables.record_cutoff(&pos, mv("b1c3"), &[], 2, 3, &Previous::default());
        assert_eq!(tables.killers(2), [Some(mv("b1c3")), Some(mv("g1f3"))]);
        for _ in 0..1000 {
            tables.record_cutoff(&pos, mv("e2e4"), &[mv("a2a3")], 5, 40, &Previous::default());
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
        let previous = Previous {
            mv: Some(e4),
            moved: [None; CONTINUATION_PLIES],
        };
        tables.record_cutoff(&pos, reply, &[], 1, 2, &previous);
        assert_eq!(tables.countermove(&pos, Some(e4)), Some(reply));
        assert_eq!(tables.countermove(&pos, None), None);
    }

    #[test]
    fn continuation_history_remembers_replies_one_and_two_plies_on() {
        // 1. e4 c5: after e4 (two plies back) and c5 (one ply back), Nf3 refutes a2-a3.
        let mut pos = Position::startpos();
        let mut moved = Vec::new();
        for uci in ["e2e4", "c7c5"] {
            let mv = pos.parse_uci_move(uci).unwrap();
            moved.push(PieceTo::new(&pos, mv));
            pos.make_move(mv);
        }
        let previous = Previous {
            mv: None,
            moved: [moved[1], moved[0]],
        };
        let nf3 = pos.parse_uci_move("g1f3").unwrap();
        let a3 = pos.parse_uci_move("a2a3").unwrap();
        let mut tables = OrderingTables::default();
        tables.record_cutoff(&pos, nf3, &[a3], 2, 4, &previous);
        // 16 (depth²) in each table, less nothing: the entries started at 0.
        assert_eq!(tables.continuation(&pos, nf3, &previous), 2 * 16);
        assert_eq!(tables.continuation(&pos, a3, &previous), -2 * 16);
        // Each table on its own: after c5 alone, and after e4 alone.
        let one_ply = Previous {
            mv: None,
            moved: [moved[1], None],
        };
        let two_plies = Previous {
            mv: None,
            moved: [None, moved[0]],
        };
        assert_eq!(tables.continuation(&pos, nf3, &one_ply), 16);
        assert_eq!(tables.continuation(&pos, nf3, &two_plies), 16);
        // Nothing is known after other moves, or with no earlier moves.
        let after_e4_one_ply_back = Previous {
            mv: None,
            moved: [moved[0], None],
        };
        assert_eq!(tables.continuation(&pos, nf3, &after_e4_one_ply_back), 0);
        assert_eq!(tables.continuation(&pos, nf3, &Previous::default()), 0);
        // Halved for a new search.
        tables.new_search();
        assert_eq!(tables.continuation(&pos, nf3, &previous), 2 * 8);
    }

    #[test]
    fn continuation_history_stays_bounded() {
        let mut pos = Position::startpos();
        let e4 = pos.parse_uci_move("e2e4").unwrap();
        let previous = Previous {
            mv: None,
            moved: [None, PieceTo::new(&pos, e4)],
        };
        pos.make_move(e4);
        let e5 = pos.parse_uci_move("e7e5").unwrap();
        let a6 = pos.parse_uci_move("a7a6").unwrap();
        let mut tables = OrderingTables::default();
        for _ in 0..1000 {
            tables.record_cutoff(&pos, e5, &[a6], 5, 40, &previous);
        }
        let good = tables.continuation(&pos, e5, &previous);
        let bad = tables.continuation(&pos, a6, &previous);
        assert!(
            (HISTORY_MAX / 2 + 1..=HISTORY_MAX).contains(&good),
            "{good}"
        );
        assert!((-HISTORY_MAX..-HISTORY_MAX / 2).contains(&bad), "{bad}");
    }

    #[test]
    fn picker_orders_quiet_moves_by_history_plus_continuation_history() {
        // After 1. e4, history likes ...a6 a little more than ...e5, but continuation history
        // likes ...e5 after e4.
        let mut pos = Position::startpos();
        let e4 = pos.parse_uci_move("e2e4").unwrap();
        let previous = Previous {
            mv: None,
            moved: [PieceTo::new(&pos, e4), None],
        };
        pos.make_move(e4);
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let mut tables = OrderingTables::default();
        // a6: history 9. e5: history 4 + 4 = 8, continuation history 8.
        tables.record_cutoff(&pos, mv("a7a6"), &[], 9, 3, &Previous::default());
        tables.record_cutoff(&pos, mv("e7e5"), &[], 9, 2, &previous);
        tables.record_cutoff(&pos, mv("e7e5"), &[], 9, 2, &previous);
        let moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        let first = |techniques: Techniques| {
            let mut picker =
                MovePicker::new(&pos, &moves, None, 3, &previous, &tables, &techniques);
            pos.to_san(picker.next().unwrap())
        };
        // History on, whatever the default.
        let both = Techniques {
            history: true,
            continuation_history: true,
            ..Techniques::default()
        };
        let without = Techniques {
            continuation_history: false,
            ..both
        };
        assert_eq!(first(without), "a6");
        assert_eq!(first(both), "e5");
    }

    #[test]
    fn the_quiet_score_adds_what_is_switched_on() {
        // After 1. e4, ...e5 caused a cutoff at depth 2: history 4, continuation history 4.
        let mut pos = Position::startpos();
        let e4 = pos.parse_uci_move("e2e4").unwrap();
        let previous = Previous {
            mv: None,
            moved: [PieceTo::new(&pos, e4), None],
        };
        pos.make_move(e4);
        let e5 = pos.parse_uci_move("e7e5").unwrap();
        let mut tables = OrderingTables::default();
        tables.record_cutoff(&pos, e5, &[], 9, 2, &previous);
        let score = |history: bool, continuation_history: bool| {
            let techniques = Techniques {
                history,
                continuation_history,
                ..Techniques::default()
            };
            tables.quiet_score(&pos, e5, &previous, &techniques)
        };
        assert_eq!(score(true, true), 8);
        assert_eq!(score(true, false), 4);
        assert_eq!(score(false, true), 4);
        assert_eq!(score(false, false), 0);
    }

    /// White can take the knight on d5 with the pawn or the rook, or the pawn on b5 with the
    /// other rook.
    const CAPTURES: &str = "4k3/8/8/1p1n4/4P3/8/8/1R1RK3 w - - 0 1";

    #[test]
    fn capture_history_learns_by_piece_destination_and_victim() {
        let pos = Position::from_fen(CAPTURES).unwrap();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let mut tables = OrderingTables::default();
        // Rxd5 cut off at depth 10 after exd5 failed to.
        tables.record_captures(&pos, Some(mv("d1d5")), &[mv("e4d5")], 10);
        assert_eq!(tables.capture_history(&pos, mv("d1d5")), 100);
        assert_eq!(tables.capture_history(&pos, mv("e4d5")), -100);
        assert_eq!(tables.capture_history(&pos, mv("b1b5")), 0);
        // A quiet move cut off: the captures tried before it get the penalty only.
        tables.record_captures(&pos, None, &[mv("b1b5")], 10);
        assert_eq!(tables.capture_history(&pos, mv("b1b5")), -100);
        // The same rook move taking a bishop instead is a different capture.
        let bishop = Position::from_fen("4k3/8/8/1p1b4/4P3/8/8/1R1RK3 w - - 0 1").unwrap();
        let rook_takes_bishop = bishop.parse_uci_move("d1d5").unwrap();
        assert_eq!(tables.capture_history(&bishop, rook_takes_bishop), 0);
        // Halved for a new search.
        tables.new_search();
        assert_eq!(tables.capture_history(&pos, mv("d1d5")), 50);
    }

    #[test]
    fn queen_promotions_are_kept_apart_by_what_they_take() {
        let pos = Position::from_fen("r3k3/1P6/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let mut tables = OrderingTables::default();
        tables.record_captures(&pos, Some(mv("b7b8q")), &[], 10);
        assert_eq!(tables.capture_history(&pos, mv("b7b8q")), 100);
        assert_eq!(tables.capture_history(&pos, mv("b7a8q")), 0);
        // En passant takes a pawn.
        let ep = Position::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 2").unwrap();
        let mut tables = OrderingTables::default();
        tables.record_captures(&ep, Some(ep.parse_uci_move("e5d6").unwrap()), &[], 4);
        let index = capture_index(&ep, ep.parse_uci_move("e5d6").unwrap()).unwrap();
        assert_eq!(index % VICTIMS, PieceType::Pawn.index());
        assert_eq!(tables.captures[index], 16);
    }

    #[test]
    fn capture_history_reorders_captures_within_limits() {
        let pos = Position::from_fen(CAPTURES).unwrap();
        let moves: Vec<Move> = pos.legal_moves().iter().copied().collect();
        let mv = |uci: &str| pos.parse_uci_move(uci).unwrap();
        let captures = |tables: &OrderingTables, techniques: Techniques| -> Vec<String> {
            MovePicker::new(
                &pos,
                &moves,
                None,
                0,
                &Previous::default(),
                tables,
                &techniques,
            )
            .take(3)
            .map(|m| pos.to_san(m))
            .collect()
        };
        let on = Techniques {
            capture_history: true,
            ..Techniques::default()
        };
        let off = Techniques {
            capture_history: false,
            ..on
        };
        let mut tables = OrderingTables::default();
        // With no history, the knight's captures first, the smaller attacker breaking the tie.
        assert_eq!(captures(&tables, on), ["exd5", "Rxd5", "Rxb5"]);
        // Rxd5 cuts off where exd5 didn't: among captures of the knight, history decides as
        // soon as it tells them apart, here after one cutoff at depth 3.
        tables.record_captures(&pos, Some(mv("d1d5")), &[mv("e4d5")], 3);
        assert_eq!(captures(&tables, on), ["Rxd5", "exd5", "Rxb5"]);
        // Off, MVV-LVA: the smaller attacker first.
        assert_eq!(captures(&tables, off), ["exd5", "Rxd5", "Rxb5"]);
        // A capture of a pawn with the best history there is comes before captures of the
        // knight without any.
        let mut tables = OrderingTables::default();
        for _ in 0..1000 {
            tables.record_captures(&pos, Some(mv("b1b5")), &[], 40);
        }
        let best = tables.capture_history(&pos, mv("b1b5"));
        assert!(
            (HISTORY_MAX * 15 / 16..=HISTORY_MAX).contains(&best),
            "{best}"
        );
        assert_eq!(captures(&tables, on), ["Rxb5", "exd5", "Rxd5"]);
        // With that history it would come before a capture of a rook with none, but not
        // before a capture of a queen.
        let pawn_capture = tables.noisy_score(&pos, mv("b1b5"), &on);
        assert!(pawn_capture > SEE_VALUES[PieceType::Rook.index()] * CAPTURE_VALUE_SCALE);
        assert!(pawn_capture < SEE_VALUES[PieceType::Queen.index()] * CAPTURE_VALUE_SCALE);
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
