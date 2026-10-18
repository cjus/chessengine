//! Search statistics and search trees: watching the search make its decisions.
//!
//! Every technique in the search claims to save work. These two tools show where the work
//! goes:
//!
//! - [`SearchStats`] counts what happened during a search: how often the transposition table
//!   answered, how often the first move caused the cutoff (a measure of move ordering), how
//!   often each pruning technique fired, and how often late move reductions had to search
//!   again. The shell's `stats` command shows them after each search.
//! - A [`SearchTree`] records the search's decisions node by node, for a small search: which
//!   moves were searched with which window, which were pruned and why, which were reduced and
//!   which re-searched. The shell's `tree` command prints it.
//!
//! The search reports each decision once, as a [`Note`], and both tools are fed from it, so
//! the search code needs one line per decision. Counting costs a few additions per node;
//! recording a tree is only done when asked, and only for the first few plies.
//!
//! The counts say how often a technique fired, not how many nodes it saved: a pruned move
//! might have needed a single node or a million. To measure savings, search with and without
//! the technique (the shell's `ab` command).
//!
//! See the Chess Programming Wiki, [Search Statistics](https://www.chessprogramming.org/Search_Statistics).

use cb2026_chess::Move;

/// One decision of the search, reported to [`SearchStats`] and, when recording, a
/// [`SearchTree`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Note {
    /// A main-search node is entered, `depth` plies to go, with the window `[alpha, beta]`.
    Enter {
        /// Remaining depth.
        depth: u32,
        /// The window's lower bound.
        alpha: i32,
        /// The window's upper bound.
        beta: i32,
    },
    /// The node returns `score`.
    Exit {
        /// The node's score, from its side to move's point of view.
        score: i32,
    },
    /// A quiescence-search node.
    Quiescence,
    /// The transposition table had an entry for the position.
    TtHit,
    /// The entry was deep enough and its bound settled the node.
    TtCutoff,
    /// The endgame tablebases settled the node.
    Tablebase,
    /// Reverse futility pruning returned the static evaluation.
    ReverseFutility,
    /// Razoring: the quiescence search confirmed the node is hopeless.
    Razoring,
    /// A null move was tried.
    NullMove,
    /// The null move scored at least beta: the node is cut off.
    NullMoveCutoff,
    /// Late move pruning skipped this move.
    LateMovePruning(Move),
    /// Futility pruning skipped this quiet move.
    Futility(Move),
    /// SEE pruning skipped this move (main search or quiescence).
    SeePruning(Move),
    /// Delta pruning skipped this capture (quiescence).
    DeltaPruning(Move),
    /// Late move reductions searched this move this many plies shallower.
    Reduced(Move, u32),
    /// A reduced move beat alpha and is searched again at full depth.
    ReductionResearch(Move),
    /// A null-window move beat alpha and is searched again with the full window (PVS).
    PvsResearch(Move),
    /// This move caused a beta cutoff, as the `index`-th move searched (from 1).
    BetaCutoff(Move, u32),
    /// A singular extension's search, without this hash move, starts at the node.
    SingularSearch(Move),
    /// No other move came close: this hash move is singular, and searched a ply deeper.
    SingularExtension(Move),
    /// No other move came close by a wide margin either: this singular hash move is searched a
    /// second ply deeper.
    DoubleExtension(Move),
    /// Another move beat beta in the singular extension's search: the node is cut off.
    MultiCut,
    /// This hash move isn't singular but beats beta: it is searched shallower.
    NegativeExtension(Move),
}

/// What happened during a search: counts of each decision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SearchStats {
    /// Main-search nodes.
    pub nodes: u64,
    /// Quiescence-search nodes.
    pub qnodes: u64,
    /// Transposition table entries found.
    pub tt_hits: u64,
    /// Nodes settled by the transposition table.
    pub tt_cutoffs: u64,
    /// Nodes settled by the tablebases.
    pub tablebase: u64,
    /// Beta cutoffs.
    pub cutoffs: u64,
    /// Beta cutoffs caused by the first move searched.
    pub first_move_cutoffs: u64,
    /// Null moves tried.
    pub null_moves: u64,
    /// Null moves that cut the node off.
    pub null_move_cutoffs: u64,
    /// Nodes cut off by reverse futility pruning.
    pub reverse_futility: u64,
    /// Nodes cut off by razoring.
    pub razoring: u64,
    /// Moves skipped by late move pruning.
    pub late_move_pruning: u64,
    /// Moves skipped by futility pruning.
    pub futility: u64,
    /// Moves skipped by SEE pruning.
    pub see_pruning: u64,
    /// Captures skipped by delta pruning.
    pub delta_pruning: u64,
    /// Moves searched with a late move reduction.
    pub reduced: u64,
    /// Reduced moves searched again at full depth.
    pub reduction_researches: u64,
    /// PVS re-searches with the full window.
    pub pvs_researches: u64,
    /// Singular extensions' searches (each leaving out a hash move).
    pub singular_searches: u64,
    /// Hash moves found singular and extended.
    pub singular_extensions: u64,
    /// Singular hash moves extended a second ply (also counted in `singular_extensions`).
    pub double_extensions: u64,
    /// Nodes cut off by multi-cut.
    pub multi_cuts: u64,
    /// Hash moves searched shallower by a negative extension.
    pub negative_extensions: u64,
}

impl SearchStats {
    /// Counts one decision.
    pub fn count(&mut self, note: Note) {
        let counter = match note {
            Note::Enter { .. } => &mut self.nodes,
            Note::Quiescence => &mut self.qnodes,
            Note::TtHit => &mut self.tt_hits,
            Note::TtCutoff => &mut self.tt_cutoffs,
            Note::Tablebase => &mut self.tablebase,
            Note::ReverseFutility => &mut self.reverse_futility,
            Note::Razoring => &mut self.razoring,
            Note::NullMove => &mut self.null_moves,
            Note::NullMoveCutoff => &mut self.null_move_cutoffs,
            Note::LateMovePruning(_) => &mut self.late_move_pruning,
            Note::Futility(_) => &mut self.futility,
            Note::SeePruning(_) => &mut self.see_pruning,
            Note::DeltaPruning(_) => &mut self.delta_pruning,
            Note::Reduced(..) => &mut self.reduced,
            Note::ReductionResearch(_) => &mut self.reduction_researches,
            Note::PvsResearch(_) => &mut self.pvs_researches,
            Note::SingularSearch(_) => &mut self.singular_searches,
            Note::SingularExtension(_) => &mut self.singular_extensions,
            Note::DoubleExtension(_) => &mut self.double_extensions,
            Note::MultiCut => &mut self.multi_cuts,
            Note::NegativeExtension(_) => &mut self.negative_extensions,
            Note::BetaCutoff(_, index) => {
                if index == 1 {
                    self.first_move_cutoffs += 1;
                }
                &mut self.cutoffs
            }
            Note::Exit { .. } => return,
        };
        *counter += 1;
    }

    /// Adds another search's counts (helper threads, or several searches).
    pub fn add(&mut self, other: &SearchStats) {
        let SearchStats {
            nodes,
            qnodes,
            tt_hits,
            tt_cutoffs,
            tablebase,
            cutoffs,
            first_move_cutoffs,
            null_moves,
            null_move_cutoffs,
            reverse_futility,
            razoring,
            late_move_pruning,
            futility,
            see_pruning,
            delta_pruning,
            reduced,
            reduction_researches,
            pvs_researches,
            singular_searches,
            singular_extensions,
            double_extensions,
            multi_cuts,
            negative_extensions,
        } = *other;
        self.nodes += nodes;
        self.qnodes += qnodes;
        self.tt_hits += tt_hits;
        self.tt_cutoffs += tt_cutoffs;
        self.tablebase += tablebase;
        self.cutoffs += cutoffs;
        self.first_move_cutoffs += first_move_cutoffs;
        self.null_moves += null_moves;
        self.null_move_cutoffs += null_move_cutoffs;
        self.reverse_futility += reverse_futility;
        self.razoring += razoring;
        self.late_move_pruning += late_move_pruning;
        self.futility += futility;
        self.see_pruning += see_pruning;
        self.delta_pruning += delta_pruning;
        self.reduced += reduced;
        self.reduction_researches += reduction_researches;
        self.pvs_researches += pvs_researches;
        self.singular_searches += singular_searches;
        self.singular_extensions += singular_extensions;
        self.double_extensions += double_extensions;
        self.multi_cuts += multi_cuts;
        self.negative_extensions += negative_extensions;
    }
}

/// One entry of a [`SearchTree`]: a decision, where it was made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeEvent {
    /// The moves from the root to the node (`None` for a null move).
    pub path: Vec<Option<Move>>,
    /// What happened.
    pub note: Note,
}

/// A record of a search's decisions, for nodes up to [`SearchTree::max_ply`] plies from the
/// root, in the order they were made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SearchTree {
    /// Only nodes this close to the root are recorded.
    pub max_ply: usize,
    /// At most this many events are kept (a tree grows fast).
    pub max_events: usize,
    /// The events.
    pub events: Vec<TreeEvent>,
    /// Whether events were dropped because of `max_events`.
    pub truncated: bool,
}

impl SearchTree {
    /// An empty tree that records up to `max_ply` plies from the root.
    pub fn new(max_ply: usize, max_events: usize) -> SearchTree {
        SearchTree {
            max_ply,
            max_events,
            ..SearchTree::default()
        }
    }

    /// Records `note`, made at the node reached by `path`, if it is shallow enough.
    pub fn record(&mut self, path: &[Option<Move>], note: Note) {
        if path.len() > self.max_ply {
            return;
        }
        if self.events.len() >= self.max_events {
            self.truncated = true;
            return;
        }
        self.events.push(TreeEvent {
            path: path.to_vec(),
            note,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb2026_chess::Position;

    #[test]
    fn counts_each_decision() {
        let mut stats = SearchStats::default();
        let mv = Position::startpos().parse_uci_move("e2e4").unwrap();
        for note in [
            Note::Enter {
                depth: 3,
                alpha: -10,
                beta: 10,
            },
            Note::BetaCutoff(mv, 1),
            Note::BetaCutoff(mv, 4),
            Note::Futility(mv),
            Note::SingularSearch(mv),
            Note::SingularSearch(mv),
            Note::SingularExtension(mv),
            Note::DoubleExtension(mv),
            Note::MultiCut,
            Note::NegativeExtension(mv),
            Note::Exit { score: 0 },
        ] {
            stats.count(note);
        }
        assert_eq!(stats.nodes, 1);
        assert_eq!(stats.cutoffs, 2);
        assert_eq!(stats.first_move_cutoffs, 1);
        assert_eq!(stats.futility, 1);
        assert_eq!(stats.singular_searches, 2);
        assert_eq!(stats.singular_extensions, 1);
        assert_eq!(stats.double_extensions, 1);
        assert_eq!(stats.multi_cuts, 1);
        assert_eq!(stats.negative_extensions, 1);
        let mut total = stats;
        total.add(&stats);
        assert_eq!(total.cutoffs, 4);
        assert_eq!(total.singular_searches, 4);
        assert_eq!(total.multi_cuts, 2);
        assert_eq!(total.negative_extensions, 2);
        assert_eq!(total.double_extensions, 2);
    }

    #[test]
    fn trees_keep_shallow_events_up_to_a_limit() {
        let mv = Position::startpos().parse_uci_move("e2e4").unwrap();
        let mut tree = SearchTree::new(1, 2);
        tree.record(&[], Note::TtHit);
        tree.record(&[Some(mv), Some(mv)], Note::TtHit);
        tree.record(&[Some(mv)], Note::TtHit);
        tree.record(&[Some(mv)], Note::TtCutoff);
        assert_eq!(tree.events.len(), 2);
        assert!(tree.truncated);
    }
}
