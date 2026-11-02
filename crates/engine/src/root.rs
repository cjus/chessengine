//! Following the root: which root move the search is on, where its effort goes, and when it may
//! be changing its mind, reported as it searches ([`Engine::search_following_root`]).
//!
//! The search reports its lines after each depth ([`crate::SearchInfo`]), which can be seconds
//! apart. In between, a player watching the engine think sees nothing, though the search is
//! busy: trying one root move after another, giving most of its effort to a few, and now and
//! then going back over something. UCI shows the same with `currmove`, `currmovenumber`,
//! `lowerbound` and `upperbound`; CNP's `engine/root` (spec 7.9) is built from [`RootInfo`].
//!
//! ## What is reported
//!
//! - **The move being searched,** its place in the depth's order of root moves, and how many
//!   there are, for the line being searched (MultiPV searches the root once per line).
//! - **The best move so far** for the line: this depth's once a move has raised the score, or
//!   the last completed depth's until then.
//! - **The effort:** the nodes searched below each root move, every depth and line together,
//!   so a reader can see where the search spends its time (usually on the best move, and on
//!   the moves that came close).
//! - **What the search goes back over,** at once ([`RootResearch`]): a later move that scored
//!   above the best on its null-window search (PVS) and is searched again in full, and a depth
//!   whose score fell outside its aspiration window and is searched again wider.
//!
//! ## How it works
//!
//! The main thread's searcher ([`crate::search`]) holds a [`RootFollower`] when the caller
//! follows the root. The root loop tells it when a line and each move start and end; the PVS
//! re-search at the root and the aspiration window's failures tell it what is gone back over;
//! and whenever the search looks at the clock and its stop flag, every thousand nodes or so, it
//! reports where the root is, so a long first move still shows its nodes growing.
//! The caller is called every time and decides what to pass on: CNP's node throttles to its
//! client's `rootMs`, letting research through at once.
//!
//! ## Pitfalls
//!
//! - **Only the main thread follows its root.** Lazy SMP's helpers search the same root in
//!   their own orders (and odd ones a depth ahead), so their moves would make no single story.
//!   A move's nodes are then the main thread's, and show each move's share; the total
//!   ([`RootInfo::nodes`]) counts every thread, as [`crate::SearchInfo::nodes`] does.
//! - **It must change nothing.** The follower reads the search's node count and never touches
//!   the search's state, so a search followed or not makes the same moves in the same order,
//!   node for node. A test checks it.
//! - **It must cost nothing much.** The work is per root move and per look at the clock, never
//!   per node, and nothing is allocated after the root's moves are first seen. Without a
//!   follower the cost is a check of an `Option` at those same places.
//! - **The search's hottest loop barely knows.** The PVS re-search is noticed where it already
//!   happens, behind a check that the node is a root move's, so the inner nodes pay one
//!   comparison on the rare re-search.
//!
//! [`Engine::search_following_root`]: crate::Engine::search_following_root

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use cb2026_chess::Move;

use crate::Score;

/// Where the main thread's root loop is, reported as the search goes
/// ([`crate::Engine::search_following_root`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RootInfo<'a> {
    /// The depth being searched: one more than the last completed.
    pub depth: u32,
    /// Which line is being searched, from 1 (MultiPV).
    pub multipv: u32,
    /// The root move being searched now. After a [`RootResearch::FailHigh`] or
    /// [`RootResearch::FailLow`], the one the depth's next search starts with.
    pub current: Move,
    /// Its place in this search of the root's order of moves for this line, from 1.
    pub number: u32,
    /// How many root moves this line searches.
    pub of: u32,
    /// The best root move so far for this line: this depth's, once a move has raised the score,
    /// or the last completed depth's until then. `None` before there is any.
    pub best: Option<Move>,
    /// Every root move searched so far, with the main thread's nodes below it, every depth and
    /// line together, in the order they were first searched.
    pub moves: &'a [(Move, u64)],
    /// Nodes searched so far, every thread together.
    pub nodes: u64,
    /// Time since the search started.
    pub time: Duration,
    /// What the search is going back over, if it is: reported when it happens.
    pub research: Option<RootResearch>,
}

/// What the search goes back over at its root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RootResearch {
    /// This root move, searched after the best, scored above it on its null-window search
    /// (PVS), and is being searched again with the full window to see whether it is the new
    /// best.
    Better(Move),
    /// The depth's score came out at or above its aspiration window: at least this. The best
    /// move looks better than the last depth said, and the depth is searched again, wider.
    FailHigh(Score),
    /// The depth's score came out at or below its aspiration window: at most this. The depth is
    /// searched again, wider.
    FailLow(Score),
}

/// The main thread's account of its root loop, and whom to tell.
pub(crate) struct RootFollower<'a> {
    /// The caller, told every time; it decides what to pass on.
    report: &'a mut dyn FnMut(&RootInfo<'_>),
    /// When the search started.
    start: Instant,
    /// The helper threads' nodes, for the total.
    helper_nodes: &'a AtomicU64,
    /// How many lines are reported: a level's extra candidate lines are searched, and their
    /// nodes counted, but they aren't reported, as their lines aren't.
    reported: usize,
    /// The depth being searched.
    depth: u32,
    /// The line being searched, from 0.
    line: usize,
    /// The move being searched, and its place in `moves`.
    current: Option<(Move, usize)>,
    /// Its place in the order of root moves, from 1.
    number: u32,
    /// How many root moves the line searches.
    of: u32,
    /// The line's best move so far.
    best: Option<Move>,
    /// Each root move searched so far and the nodes of its finished searches.
    moves: Vec<(Move, u64)>,
    /// The thread's node count when the current move's search began.
    move_start: u64,
}

impl<'a> RootFollower<'a> {
    /// A follower that tells `report`, for a search that started at `start`, the first
    /// `reported` lines.
    pub(crate) fn new(
        report: &'a mut dyn FnMut(&RootInfo<'_>),
        start: Instant,
        helper_nodes: &'a AtomicU64,
        reported: usize,
    ) -> RootFollower<'a> {
        RootFollower {
            report,
            start,
            helper_nodes,
            reported,
            depth: 0,
            line: 0,
            current: None,
            number: 0,
            of: 0,
            best: None,
            moves: Vec::new(),
            move_start: 0,
        }
    }

    /// Line `line` (from 0) is about to be searched to `depth`, over `moves`. `previous` is the
    /// line's best move at the last completed depth, its best until this depth has one, unless
    /// an earlier line has taken it at this depth.
    pub(crate) fn begin_line(
        &mut self,
        depth: u32,
        line: usize,
        moves: &[Move],
        previous: Option<Move>,
    ) {
        self.depth = depth;
        self.line = line;
        self.best = previous.filter(|mv| moves.contains(mv));
        self.current = None;
    }

    /// The root loop starts searching `mv`, the `number`-th of `of`, `nodes` into the search.
    pub(crate) fn begin_move(&mut self, mv: Move, number: u32, of: u32, nodes: u64) {
        let index = self.index(mv);
        self.current = Some((mv, index));
        self.number = number;
        self.of = of;
        self.move_start = nodes;
        self.report(nodes, None);
    }

    /// The current move's search has ended, `nodes` into the search: its nodes are counted, and
    /// `best` says whether it raised the score, as the line's new best.
    pub(crate) fn end_move(&mut self, nodes: u64, best: bool) {
        if let Some((mv, index)) = self.current {
            self.moves[index].1 += nodes - self.move_start;
            self.move_start = nodes;
            if best {
                self.best = Some(mv);
            }
        }
    }

    /// The whole depth is to be searched again, starting with `first` of `of` moves, because
    /// of `research`: reports it, `nodes` into the search.
    pub(crate) fn restart(&mut self, first: Move, of: u32, nodes: u64, research: RootResearch) {
        let index = self.index(first);
        self.current = Some((first, index));
        self.number = 1;
        self.of = of;
        self.move_start = nodes;
        self.report(nodes, Some(research));
    }

    /// Reports where the root is, `nodes` into the search, with the current move's nodes so
    /// far counted in, and `research` if the search is going back over something.
    pub(crate) fn report(&mut self, nodes: u64, research: Option<RootResearch>) {
        let Some((current, index)) = self.current else {
            return;
        };
        if self.line >= self.reported {
            return;
        }
        // The current move's search so far, counted in for this report only, so nothing is
        // allocated.
        let partial = nodes - self.move_start;
        self.moves[index].1 += partial;
        (self.report)(&RootInfo {
            depth: self.depth,
            multipv: self.line as u32 + 1,
            current,
            number: self.number,
            of: self.of,
            best: self.best,
            moves: &self.moves,
            nodes: nodes + self.helper_nodes.load(Ordering::Relaxed),
            time: self.start.elapsed(),
            research,
        });
        self.moves[index].1 -= partial;
    }

    /// `mv`'s place in `moves`, added if it is new. A linear search: a position has a few dozen
    /// moves (218 at most), and it runs once per root move searched.
    fn index(&mut self, mv: Move) -> usize {
        match self.moves.iter().position(|&(m, _)| m == mv) {
            Some(index) => index,
            None => {
                self.moves.push((mv, 0));
                self.moves.len() - 1
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use cb2026_chess::Position;

    use super::*;
    use crate::{Engine, Limits, SearchInfo, SearchResult};

    /// The position after 1.e4 e5 2.Nf3 Nc6, where White has 27 moves.
    const RUY_OR_ITALIAN: &str = "r1bqkbnr/pppp1ppp/2n5/4p3/4P3/5N2/PPPP1PPP/RNBQKB1R w KQkq - 2 3";

    /// What a test keeps of a report: all of it, its moves copied.
    #[derive(Clone, Debug)]
    struct Seen {
        depth: u32,
        multipv: u32,
        current: Move,
        number: u32,
        of: u32,
        best: Option<Move>,
        moves: Vec<(Move, u64)>,
        nodes: u64,
        research: Option<RootResearch>,
    }

    /// Searches `fen` to `depth` for `lines` lines on one thread, following the root if
    /// `follow`: the result, the lines reported, and what the follower heard.
    fn search(
        fen: &str,
        depth: u32,
        lines: usize,
        follow: bool,
    ) -> (SearchResult, Vec<SearchInfo>, Vec<Seen>) {
        let mut engine = Engine::new();
        engine.multipv = lines;
        let stop = AtomicBool::new(false);
        let mut infos = Vec::new();
        let mut seen = Vec::new();
        let mut on_root = |root: &RootInfo<'_>| {
            seen.push(Seen {
                depth: root.depth,
                multipv: root.multipv,
                current: root.current,
                number: root.number,
                of: root.of,
                best: root.best,
                moves: root.moves.to_vec(),
                nodes: root.nodes,
                research: root.research,
            });
        };
        let result = engine.search_following_root(
            &Position::from_fen(fen).unwrap(),
            &[],
            &Limits::depth(depth),
            &stop,
            &mut |info| infos.push(info.clone()),
            follow.then_some(&mut on_root as &mut dyn FnMut(&RootInfo<'_>)),
        );
        (result, infos, seen)
    }

    #[test]
    fn following_the_root_changes_nothing_in_the_search() {
        for lines in [1, 3] {
            let (plain, plain_infos, none) = search(RUY_OR_ITALIAN, 8, lines, false);
            let (followed, followed_infos, seen) = search(RUY_OR_ITALIAN, 8, lines, true);
            assert!(none.is_empty());
            assert!(!seen.is_empty());
            // The same moves and lines, node for node.
            assert_eq!(plain, followed);
            let without_time = |infos: Vec<SearchInfo>| {
                infos
                    .into_iter()
                    .map(|info| (info.depth, info.score, info.nodes, info.pv, info.multipv))
                    .collect::<Vec<_>>()
            };
            assert_eq!(without_time(plain_infos), without_time(followed_infos));
        }
    }

    #[test]
    fn every_root_move_is_followed_with_the_nodes_below_it() {
        let (_, infos, seen) = search(RUY_OR_ITALIAN, 8, 1, true);
        let position = Position::from_fen(RUY_OR_ITALIAN).unwrap();
        let legal = position.legal_moves();
        // The first report is the first move of depth 1, before there is a best move.
        let first = &seen[0];
        assert_eq!((first.depth, first.number, first.best), (1, 1, None));
        assert_eq!(first.of as usize, legal.len());
        let mut depth = 1;
        for report in &seen {
            assert!(report.depth >= depth && report.depth <= 8, "{report:?}");
            depth = report.depth;
            assert_eq!(report.multipv, 1);
            assert_eq!(report.of as usize, legal.len());
            assert!((1..=report.of).contains(&report.number), "{report:?}");
            assert!(legal.contains(&report.current));
            assert!(report.moves.iter().any(|&(mv, _)| mv == report.current));
            // The main thread's nodes below the root moves are some of all the nodes.
            let below: u64 = report.moves.iter().map(|&(_, nodes)| nodes).sum();
            assert!(
                below <= report.nodes,
                "{below} > {}: {report:?}",
                report.nodes
            );
        }
        // Every move was searched, and the nodes only grow.
        let last = seen.last().unwrap();
        assert_eq!(last.moves.len(), legal.len());
        for window in seen.windows(2) {
            assert!(window[0].nodes <= window[1].nodes);
        }
        // Once depth 1 has a best move, every later report has one: the depth's best.
        assert!(
            seen.iter()
                .filter(|r| r.depth > 1)
                .all(|r| r.best.is_some())
        );
        let best_at_8 = infos.last().unwrap().pv[0];
        assert_eq!(last.best, Some(best_at_8));
    }

    #[test]
    fn a_better_move_and_a_window_s_failure_are_heard_when_they_happen() {
        // At depth 8 from the start, the search changes its mind seven times, and its score
        // falls below its window once.
        let (_, _, seen) = search(&Position::startpos().to_fen(), 8, 1, true);
        let better: Vec<&Seen> = seen
            .iter()
            .filter(|r| matches!(r.research, Some(RootResearch::Better(_))))
            .collect();
        assert!(
            !better.is_empty(),
            "no better move in {} reports",
            seen.len()
        );
        for report in &better {
            // A later move than the first, about itself.
            assert!(report.number > 1, "{report:?}");
            assert_eq!(report.research, Some(RootResearch::Better(report.current)));
        }
        let windows: Vec<&Seen> = seen
            .iter()
            .filter(|r| {
                matches!(
                    r.research,
                    Some(RootResearch::FailHigh(_) | RootResearch::FailLow(_))
                )
            })
            .collect();
        assert!(!windows.is_empty(), "no aspiration failure");
        for report in &windows {
            // The depth starts again, with the move that failed high if one did.
            assert_eq!(report.number, 1, "{report:?}");
            if let Some(RootResearch::FailHigh(_)) = report.research {
                assert_eq!(report.best, Some(report.current), "{report:?}");
            }
        }
    }

    #[test]
    fn each_line_of_several_is_followed_over_its_own_moves() {
        let (_, _, seen) = search(&Position::startpos().to_fen(), 5, 3, true);
        for line in 1..=3 {
            let reports: Vec<&Seen> = seen.iter().filter(|r| r.multipv == line).collect();
            assert!(!reports.is_empty(), "line {line}");
            // Each line leaves out the moves of the lines before it.
            assert!(reports.iter().all(|r| r.of == 21 - line), "line {line}");
        }
    }
}
