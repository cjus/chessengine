//! Search trees folded into their nodes: the positions the search reached near the root, how it
//! searched each, and what it decided there.
//!
//! A [`SearchTree`] is a log: one [`Note`] after another, each made at the node its path leads
//! to. That is what the search can write cheaply, one line per decision, but not what a reader
//! wants, which is the tree: each root move with its window, its score and the reasons the
//! search gave, then its replies the same way. [`FoldedTree::fold`] turns the log into that
//! tree, once, so that a node can be read without going through the log again (CNP's
//! `engine/treeNode` reads a big tree a node at a time).
//!
//! ## How the log folds
//!
//! - **`Enter` and `Exit` pair up like brackets,** one pair for each search of a node, with its
//!   window and remaining depth, then its score. A recorded tree brackets the root's searches
//!   too.
//! - **A node is shown as its last search,** the one whose score counted: its window, its
//!   score, its children and its decisions are that search's. A node can be searched more
//!   than once: a late move reduced, then searched again at full depth, then again with the
//!   full window; and the whole root again on each aspiration re-search. Mixed together, those
//!   searches would repeat each decision and keep replies the deciding search never looked at,
//!   with scores from another question; a score's bound would be read against the wrong
//!   window. The earlier searches show as the decisions that led to the last: `Reduced`,
//!   `ReductionResearch` and `PvsResearch` for a node, and [`FoldedTree::root_searches`] for
//!   the root.
//! - **Each decision goes to the node it is about.** Most are made at the node they concern: a
//!   cutoff, a table hit, null-move or reverse futility pruning, a move pruned (which has no
//!   node of its own, so it stays with its parent, naming the move). A reduction and its
//!   re-searches are noted with the move already played, at the child's path, and the singular,
//!   double and negative extensions at the parent, before its moves, naming the hash move. Both
//!   say how the parent searched that child, so they go to the child, before its own
//!   decisions. A decision's move, when it has one, is always a move from its own node: the
//!   reply that refuted it, the move left out, the capture that proved ProbCut.
//! - **Verification searches are left out.** A null move's search (its path has a null move)
//!   and a singular extension's search (the node entered again while it is open, without its
//!   hash move, at a lower depth and a lowered window) are the search checking something, not
//!   lines of play. What they found shows as the decisions they led to (`NullMoveCutoff`,
//!   `SingularExtension`, `MultiCut`).
//! - **Attempts that decided nothing are left out:** `TtHit` (an entry too shallow to use),
//!   `NullMove`, `ProbCut` and `SingularSearch` (a check started; the decision follows if it
//!   succeeded), and `Quiescence`.
//!
//! ## Pitfalls
//!
//! - **Scores are the search's own values**, from each node's side to move, with mates counted
//!   from the root ([`crate::search::to_score_at`] counts them from the node).
//! - **A truncated log ends mid-search** ([`SearchTree::truncated`]): some nodes have no score,
//!   and the later part of the search, the later root moves included, is missing. A node then
//!   shows its last search that finished, if it has one.
//! - **A node's score comes from the whole search,** below the recorded plies too. The tree
//!   shows the first plies' reasons, not every reason.
//!
//! See the Chess Programming Wiki, [Search Tree](https://www.chessprogramming.org/Search_Tree)
//! and [Node Types](https://www.chessprogramming.org/Node_Types).

use cb2026_chess::Move;

use crate::stats::{Note, SearchTree};

/// The window and remaining depth a node was searched with, as the search's values from the
/// node's side to move.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Window {
    /// The depth still to go when the node was entered, in plies.
    pub depth: u32,
    /// The window's lower edge ([`crate::search::INFINITY`] negated when it has none).
    pub alpha: i32,
    /// The window's upper edge ([`crate::search::INFINITY`] when it has none).
    pub beta: i32,
}

/// One node of a [`FoldedTree`]: its last search.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoldedNode {
    /// The move that reaches it from its parent; `None` at the root.
    pub mv: Option<Move>,
    /// Its parent's index; `None` at the root.
    pub parent: Option<usize>,
    /// How many plies from the root it is.
    pub ply: usize,
    /// The window of its last search. `None` for a root the log doesn't bracket.
    pub window: Option<Window>,
    /// Its last search's score, from its side to move. `None` if the log ended inside it, or
    /// for a root the log doesn't bracket.
    pub score: Option<i32>,
    /// Its children's indices: the nodes its last search entered, in the order it first
    /// entered each.
    pub children: Vec<usize>,
    /// How many nodes the tree holds below it.
    pub subtree: usize,
    /// What the search decided about it, in order: how its parent searched it (reduced,
    /// extended, searched again), then what its last search decided at it (see the module's
    /// docs for which notes these are).
    pub notes: Vec<Note>,
}

/// A [`SearchTree`] folded into its nodes (see the module's docs).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FoldedTree {
    /// The nodes, the root first; a node's parent always comes before it.
    pub nodes: Vec<FoldedNode>,
    /// How many times the root was searched (1, and one more for each aspiration re-search);
    /// 0 for a log that doesn't bracket the root's searches.
    pub root_searches: usize,
    /// How many plies from the root were recorded.
    pub max_ply: usize,
    /// Whether the log was cut short ([`SearchTree::truncated`]).
    pub truncated: bool,
}

/// One search of one node, from its `Enter` to its `Exit`.
#[derive(Default)]
struct Search {
    /// The move into the node; `None` at the root.
    mv: Option<Move>,
    /// Its window; `None` for the root of a log that doesn't bracket it.
    window: Option<Window>,
    /// Its score, once it has exited.
    score: Option<i32>,
    /// What it decided at the node.
    notes: Vec<Note>,
    /// How it searched its children, by the move to each: reductions, extensions, re-searches.
    how: Vec<(Move, Note)>,
    /// Its children's searches, in order.
    children: Vec<usize>,
}

/// A bracket opened by an `Enter`, closed by its `Exit`.
struct Frame {
    /// Its search, or `None` inside a verification search.
    search: Option<usize>,
    /// Its distance from the root.
    ply: usize,
}

impl FoldedTree {
    /// The root's index.
    pub const ROOT: usize = 0;

    /// Folds a recorded log into its nodes.
    pub fn fold(tree: &SearchTree) -> FoldedTree {
        // The searches, the first being the root as the log has it outside any bracket.
        let mut searches = vec![Search::default()];
        let mut roots: Vec<usize> = Vec::new();
        let mut open: Vec<Frame> = Vec::new();
        // Open brackets of verification searches: while there are any, nothing is kept.
        let mut verifying = 0usize;
        for event in &tree.events {
            // A null move's search, and everything below it.
            let Some(path) = event.path.iter().copied().collect::<Option<Vec<Move>>>() else {
                continue;
            };
            // The search the event happens in, and its distance from the root.
            let (top, top_ply) = match open.last() {
                Some(frame) => (frame.search, frame.ply),
                None => (Some(0), 0),
            };
            match event.note {
                Note::Enter { depth, alpha, beta } => {
                    // A bracket inside a verification search, or the singular extension's
                    // search, which enters the open node again: skipped. So is one whose parent
                    // isn't open, which a well-formed log never has.
                    let parent = match (top, path.len()) {
                        (_, 0) if open.is_empty() => None,
                        (Some(parent), len) if verifying == 0 && len == top_ply + 1 => Some(parent),
                        _ => {
                            verifying += 1;
                            open.push(Frame {
                                search: None,
                                ply: path.len(),
                            });
                            continue;
                        }
                    };
                    let search = searches.len();
                    searches.push(Search {
                        mv: path.last().copied(),
                        window: Some(Window { depth, alpha, beta }),
                        ..Search::default()
                    });
                    match parent {
                        Some(parent) => searches[parent].children.push(search),
                        None => roots.push(search),
                    }
                    open.push(Frame {
                        search: Some(search),
                        ply: path.len(),
                    });
                }
                Note::Exit { score } => match open.pop() {
                    Some(Frame {
                        search: Some(search),
                        ..
                    }) => searches[search].score = Some(score),
                    Some(Frame { search: None, .. }) => verifying -= 1,
                    None => {}
                },
                _ if verifying > 0 => {}
                Note::TtHit
                | Note::NullMove
                | Note::ProbCut
                | Note::SingularSearch(_)
                | Note::Quiescence => {}
                note => {
                    let Some(top) = top else { continue };
                    match note {
                        // Noted at the child's path, with its move already played.
                        Note::Reduced(mv, _)
                        | Note::ReductionResearch(mv)
                        | Note::PvsResearch(mv) => {
                            if path.len() == top_ply + 1 {
                                searches[top].how.push((mv, note));
                            }
                        }
                        // Noted at the parent's, naming the hash move.
                        Note::SingularExtension(mv)
                        | Note::DoubleExtension(mv)
                        | Note::NegativeExtension(mv) => searches[top].how.push((mv, note)),
                        _ => {
                            if path.len() == top_ply {
                                searches[top].notes.push(note);
                            }
                        }
                    }
                }
            }
        }
        let root = Self::last_of(&searches, &roots).unwrap_or(0);
        let mut folded = FoldedTree {
            nodes: vec![FoldedNode {
                window: searches[root].window,
                score: searches[root].score,
                notes: searches[root].notes.clone(),
                ..FoldedNode::default()
            }],
            root_searches: roots.len(),
            max_ply: tree.max_ply,
            truncated: tree.truncated,
        };
        folded.grow(Self::ROOT, root, &searches);
        // Children come after their parents, so one pass from the end sums every subtree.
        for i in (1..folded.nodes.len()).rev() {
            let below = folded.nodes[i].subtree + 1;
            if let Some(parent) = folded.nodes[i].parent {
                folded.nodes[parent].subtree += below;
            }
        }
        folded
    }

    /// The last of `candidates` that finished, or else the last one.
    fn last_of(searches: &[Search], candidates: &[usize]) -> Option<usize> {
        candidates
            .iter()
            .rev()
            .find(|&&search| searches[search].score.is_some())
            .or(candidates.last())
            .copied()
    }

    /// Adds node `node`'s children, as its search `search` entered them, and theirs below.
    fn grow(&mut self, node: usize, search: usize, searches: &[Search]) {
        let parent = &searches[search];
        let mut moves: Vec<Move> = Vec::new();
        for &child in &parent.children {
            if let Some(mv) = searches[child].mv
                && !moves.contains(&mv)
            {
                moves.push(mv);
            }
        }
        for mv in moves {
            let tries: Vec<usize> = parent
                .children
                .iter()
                .copied()
                .filter(|&child| searches[child].mv == Some(mv))
                .collect();
            let Some(last) = Self::last_of(searches, &tries) else {
                continue;
            };
            let how = parent
                .how
                .iter()
                .filter(|&&(about, _)| about == mv)
                .map(|&(_, note)| note);
            let child = self.nodes.len();
            self.nodes.push(FoldedNode {
                mv: Some(mv),
                parent: Some(node),
                ply: self.nodes[node].ply + 1,
                window: searches[last].window,
                score: searches[last].score,
                notes: how.chain(searches[last].notes.iter().copied()).collect(),
                ..FoldedNode::default()
            });
            self.nodes[node].children.push(child);
            self.grow(child, last, searches);
        }
    }

    /// The root.
    pub fn root(&self) -> &FoldedNode {
        &self.nodes[Self::ROOT]
    }

    /// The index of the node `path` leads to from the root, if the tree holds it.
    pub fn find(&self, path: &[Move]) -> Option<usize> {
        path.iter().try_fold(Self::ROOT, |node, &mv| {
            self.nodes[node]
                .children
                .iter()
                .copied()
                .find(|&child| self.nodes[child].mv == Some(mv))
        })
    }

    /// How many decisions the tree holds, at every node together.
    pub fn decisions(&self) -> usize {
        self.nodes.iter().map(|node| node.notes.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicBool;

    use cb2026_chess::Position;

    use super::*;
    use crate::Engine;
    use crate::stats::TreeEvent;

    /// The moves `uci` names, played one after another from the start.
    fn line(uci: &[&str]) -> Vec<Move> {
        let mut pos = Position::startpos();
        uci.iter()
            .map(|text| {
                let mv = pos.parse_uci_move(text).expect("a legal move");
                pos.make_move(mv);
                mv
            })
            .collect()
    }

    fn at(path: &[Move], note: Note) -> TreeEvent {
        TreeEvent {
            path: path.iter().copied().map(Some).collect(),
            note,
        }
    }

    fn enter(depth: u32, alpha: i32, beta: i32) -> Note {
        Note::Enter { depth, alpha, beta }
    }

    #[test]
    fn a_node_is_its_last_search_after_the_notes_on_how_it_was_searched() {
        let moves = line(&["e2e4", "e7e5", "g1f3"]);
        let (e4, e5, nf3) = (&moves[..1], &moves[..2], moves[2]);
        let d4 = line(&["d2d4"]);
        let (d4_d5, d4_nf6) = (line(&["d2d4", "d7d5"]), line(&["d2d4", "g8f6"]));
        let a6 = line(&["e2e4", "a7a6"])[1];
        let null_below_e4 = TreeEvent {
            path: vec![Some(moves[0]), None],
            note: enter(1, -10, -9),
        };
        let log = SearchTree {
            max_ply: 2,
            max_events: 100,
            truncated: false,
            events: vec![
                // A first search of the root, which an aspiration re-search replaces.
                at(&[], enter(3, -5, 5)),
                at(e4, enter(2, -5, 5)),
                at(e4, Note::Exit { score: -50 }),
                at(&[], Note::Exit { score: 50 }),
                // The second.
                at(&[], enter(3, -100, 100)),
                at(e4, enter(2, -100, 100)),
                at(e4, Note::TtHit),
                // The singular extension's search: e4's node again, then a reply inside it.
                at(e4, Note::SingularSearch(moves[1])),
                at(e4, enter(1, -40, -39)),
                at(e5, enter(0, 39, 40)),
                at(e5, Note::Exit { score: 50 }),
                at(e4, Note::Exit { score: -50 }),
                at(e4, Note::SingularExtension(moves[1])),
                at(e5, enter(2, -100, 100)),
                // An extension of a move beyond the recorded plies goes nowhere.
                at(e5, Note::SingularExtension(nf3)),
                at(e5, Note::Exit { score: 20 }),
                // A null move's search is left out.
                at(e4, Note::NullMove),
                null_below_e4.clone(),
                TreeEvent {
                    note: Note::Exit { score: 9 },
                    ..null_below_e4
                },
                // A pruned reply stays with e4, naming it.
                at(e4, Note::Futility(a6)),
                at(e4, Note::Exit { score: -20 }),
                // d4: reduced, refuted by d5; searched again with the full window, by Nf6.
                at(&d4, Note::Reduced(d4[0], 2)),
                at(&d4, enter(1, -21, -20)),
                at(&d4_d5, enter(0, 20, 21)),
                at(&d4_d5, Note::Exit { score: 25 }),
                at(&d4, Note::BetaCutoff(d4_d5[1], 1)),
                at(&d4, Note::Exit { score: -25 }),
                at(&d4, Note::PvsResearch(d4[0])),
                at(&d4, enter(2, -100, -20)),
                at(&d4_nf6, enter(1, 20, 100)),
                at(&d4_nf6, Note::Exit { score: 30 }),
                at(&d4, Note::BetaCutoff(d4_nf6[1], 1)),
                at(&d4, Note::Exit { score: -30 }),
                at(&[], Note::Exit { score: 20 }),
            ],
        };
        let tree = FoldedTree::fold(&log);
        let root = tree.root();
        assert_eq!(tree.root_searches, 2);
        assert_eq!(
            (root.window, root.score),
            (
                Some(Window {
                    depth: 3,
                    alpha: -100,
                    beta: 100
                }),
                Some(20)
            )
        );
        assert_eq!(root.children.len(), 2);
        assert_eq!(root.subtree, 4);
        assert_eq!(tree.find(&[]), Some(FoldedTree::ROOT));

        let e4_node = &tree.nodes[tree.find(e4).unwrap()];
        assert_eq!(e4_node.score, Some(-20));
        assert_eq!(
            e4_node.window,
            Some(Window {
                depth: 2,
                alpha: -100,
                beta: 100
            })
        );
        assert_eq!(e4_node.notes, vec![Note::Futility(a6)]);
        assert_eq!((e4_node.children.len(), e4_node.subtree), (1, 1));

        // The reply is its real search, not the singular search's, and was extended.
        let e5_node = &tree.nodes[tree.find(e5).unwrap()];
        assert_eq!(e5_node.score, Some(20));
        assert_eq!(e5_node.window.unwrap().depth, 2);
        assert_eq!(e5_node.notes, vec![Note::SingularExtension(moves[1])]);
        assert_eq!(tree.find(&moves), None);

        // d4 is its last search, with the reply that refuted it there; how it came to be
        // searched that way comes first.
        let d4_node = &tree.nodes[tree.find(&d4).unwrap()];
        assert_eq!(d4_node.score, Some(-30));
        assert_eq!(
            d4_node.window,
            Some(Window {
                depth: 2,
                alpha: -100,
                beta: -20
            })
        );
        assert_eq!(
            d4_node.notes,
            vec![
                Note::Reduced(d4[0], 2),
                Note::PvsResearch(d4[0]),
                Note::BetaCutoff(d4_nf6[1], 1)
            ]
        );
        assert_eq!(tree.find(&d4_d5), None);
        assert!(tree.find(&d4_nf6).is_some());
        assert_eq!(tree.decisions(), 5);
    }

    #[test]
    fn a_node_the_log_ends_inside_has_its_window_and_no_score() {
        let e4 = line(&["e2e4"]);
        let log = SearchTree {
            max_ply: 1,
            max_events: 1,
            truncated: true,
            events: vec![at(&e4, enter(2, -5, 5))],
        };
        let tree = FoldedTree::fold(&log);
        assert!(tree.truncated);
        let node = &tree.nodes[tree.find(&e4).unwrap()];
        assert_eq!(node.window.map(|w| w.depth), Some(2));
        assert_eq!(node.score, None);
    }

    /// A real search's tree: every decision's move is a move from its own node, and the notes
    /// about how a node was searched name the move that leads to it.
    #[test]
    fn a_real_tree_gives_each_decision_to_the_node_it_is_about() {
        // An open middlegame, deep enough for reductions, re-searches and pruning.
        let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/3P1N2/PPP2PPP/RNBQK2R b KQkq - 0 4";
        let root = Position::from_fen(fen).unwrap();
        let mut engine = Engine::new();
        let stop = AtomicBool::new(false);
        let (log, info) = engine.search_tree(&root, &[], 8, 3, 1_000_000, &stop);
        assert!(!log.truncated);
        let tree = FoldedTree::fold(&log);

        // Every root move was searched, each a child of the root, and the best is one of them.
        // The root is bracketed, and its last search's score is the result's.
        let root_node = tree.root();
        assert!(tree.root_searches >= 1);
        assert_eq!(
            root_node.score.map(crate::search::to_score),
            Some(info.score)
        );
        assert_eq!(root_node.children.len(), root.legal_moves().len());
        assert!(tree.find(&info.pv[..1]).is_some());
        assert_eq!(root_node.subtree, tree.nodes.len() - 1);

        let mut kinds = std::collections::HashSet::new();
        for (i, node) in tree.nodes.iter().enumerate().skip(1) {
            let mut path = Vec::new();
            let mut at = Some(i);
            while let Some(n) = at.filter(|&n| n != FoldedTree::ROOT) {
                path.push(tree.nodes[n].mv.unwrap());
                at = tree.nodes[n].parent;
            }
            path.reverse();
            let mut pos = root.clone();
            let mut parent = root.clone();
            for &mv in &path {
                parent = pos.clone();
                pos.make_move(mv);
            }
            assert!(parent.legal_moves().contains(&node.mv.unwrap()));
            assert!(node.ply <= 3 && node.window.is_some() && node.score.is_some());
            for note in &node.notes {
                kinds.insert(std::mem::discriminant(note));
                match *note {
                    Note::BetaCutoff(mv, _)
                    | Note::ProbCutCutoff(mv)
                    | Note::LateMovePruning(mv)
                    | Note::HistoryPruning(mv)
                    | Note::Futility(mv)
                    | Note::SeePruning(mv) => {
                        assert!(pos.legal_moves().contains(&mv), "{note:?} at {path:?}");
                    }
                    Note::Reduced(mv, _)
                    | Note::ReductionResearch(mv)
                    | Note::PvsResearch(mv)
                    | Note::SingularExtension(mv)
                    | Note::DoubleExtension(mv)
                    | Note::NegativeExtension(mv) => {
                        assert_eq!(Some(mv), node.mv, "{note:?} at {path:?}");
                    }
                    Note::Enter { .. }
                    | Note::Exit { .. }
                    | Note::TtHit
                    | Note::NullMove
                    | Note::ProbCut
                    | Note::SingularSearch(_)
                    | Note::Quiescence => panic!("{note:?} isn't a decision"),
                    _ => {}
                }
            }
        }
        // Enough happened to test the rules on: cutoffs, reductions and re-searches at least.
        assert!(kinds.len() >= 4, "only {} kinds of decision", kinds.len());
    }
}
