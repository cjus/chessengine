//! `engine/tree` and `engine/treeNode` (spec 7.8): cb2026's search trees, recorded, kept for a
//! while, and read a node at a time.
//!
//! A tree request searches its position with [`Engine::search_tree`], which records the last
//! iteration's decisions near the root, folds the log into nodes ([`FoldedTree`]), keeps it
//! under a handle, and answers with the search's result, the root and its first children.
//! `engine/treeNode` then reads any node by the moves that lead to it.
//!
//! ## How it works
//!
//! - **Each tree has an engine of its own** ([`TreeSetup::engine`]): a fresh one, with its own
//!   [`TREE_HASH_MB`] table and one thread, configured as every search starts (the switches at
//!   their defaults), with the main engine's evaluation, network and tablebases. So a tree is
//!   its own deterministic search, the same tree each time it is asked for, and it runs while
//!   a search runs: the main engine belongs to that search's thread.
//! - **It is recorded on a thread of its own,** as a search is, so the command loop goes on
//!   answering `ping`, `engine/stop` and cancels. One tree at a time (`maxConcurrentTrees` 1):
//!   another that comes while one is recorded is `Busy`, unless that one is ending. A cancel
//!   stops its search, and it answers `Cancelled` and keeps nothing. `engine/stop` doesn't
//!   apply: a tree is wanted whole or not at all. The node's `maxTimeMs` bounds it, as it does
//!   every search, and a tree that reaches it is `LimitExceeded`.
//! - **The trees are kept in a [`TreeStore`]**, shared by the recording thread, which adds a
//!   tree just before it answers, and the command loop, which reads them. A tree unused for
//!   [`TREE_EXPIRY_MS`] expires; past [`MAX_TREES_KEPT`] trees, or [`MAX_TREE_ENTRIES_KEPT`]
//!   nodes and decisions in all, the tree unused longest goes first. A handle names one tree
//!   and is never given twice, so an expired handle is `Expired`, never someone else's tree.
//! - **A tree is kept folded, as the engine's values,** and turned into CNP's terms when a node
//!   is read: a few hundred bytes a node instead of the log's every event.
//!
//! ## Decisions in CNP's terms
//!
//! [`decision`] says each of the engine's notes in alpha-beta's terms (spec 7.8), naming the
//! switch behind it ([`cb2026_shell::SWITCHES`]) as its `technique`:
//!
//! | The engine's note | CNP's decision | Technique |
//! | --- | --- | --- |
//! | `BetaCutoff(m, i)` | `cutoff`, `move` m, `order` i | `alphabeta` |
//! | `TtCutoff` | `table` | `tt` |
//! | `Tablebase` | `tablebase` | |
//! | `NullMoveCutoff` | `nullMove` | `nullmove` |
//! | `ReverseFutility`, `Razoring`, `MultiCut` | `nodePruned` | `rfp`, `razoring`, `multicut` |
//! | `ProbCutCutoff(m)` | `nodePruned`, `move` m | `probcut` |
//! | `LateMovePruning(m)`, `HistoryPruning(m)`, `Futility(m)`, `SeePruning(m)`, `DeltaPruning(m)` | `movePruned`, `move` m | `lmp`, `histprune`, `futility`, `seeprune`, `delta` |
//! | `Reduced(_, n)` | `reduced`, `plies` n | `lmr` |
//! | `NegativeExtension(_)` | `reduced`, `plies` [`SearchParams::negative_extension_plies`] | `negext` |
//! | `SingularExtension(_)`, `DoubleExtension(_)` | `extended`, `plies` 1 | `singular`, `doubleext` |
//! | `ReductionResearch(_)` | `research`, `reason` `depth` | `lmr` |
//! | `PvsResearch(_)` | `research`, `reason` `window` | `pvs` |
//! | Each search of the root before its last ([`FoldedTree::root_searches`]) | `research`, `reason` `window`, at the root | `aspiration` |
//!
//! The rest are attempts that decided nothing (a table entry too shallow, a null move or
//! ProbCut tried, a singular extension's search started) or the brackets of a node's search,
//! which become its window and score; the fold leaves them out.
//!
//! ## Pitfalls
//!
//! - **Mates are counted from the node,** not from the root as the search counts them
//!   ([`to_score_at`]), so a node's `{"mate": 1}` is a mate in one from that node.
//! - **A node's bound is read against the window of its last search,** the one that gave its
//!   score. Its first window would misread a reduced move searched again: its null window's
//!   edge is not the full window's.
//! - **The root's window is its last aspiration window,** which cb2026 widens until the
//!   score falls inside it, so the root's score is exact. Its earlier searches show as its
//!   `research` decisions, and every node below is its last search (see [`FoldedTree`]).
//! - **A recording past [`TREE_MAX_EVENTS`] stops,** and the tree is `truncated`: the later
//!   root moves are then missing, not just deep nodes.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};
use cb2026_engine::eval::EvalParams;
use cb2026_engine::search::{INFINITY, SearchParams, Techniques, to_score_at};
use cb2026_engine::stats::{Note, SearchTree};
use cb2026_engine::tablebase::{Tablebase, TablebaseParams};
use cb2026_engine::tree::FoldedTree;
use cb2026_engine::{Engine, Network, SearchInfo};
use cb2026_shell::SWITCHES;
use cnp::engine::{
    Bound, Decision, DecisionKind, Recorded, ResearchReason, Tree, TreeNode, TreeNodeParams,
    TreeNodeResult, TreeParams, TreeResult,
};
use cnp::{Id, Response, ResponseError, code};

use crate::{MAX_TIME_MS, Output, WATCHDOG_POLL, millis, moves, read_position, score, send};

/// The deepest tree the node records (`maxTreeDepth`). On an M1 Max a depth-16 tree took
/// 0.5–1.1 s and a depth-24 one 11–15 s, in an opening and a middlegame: 24 is about as long as
/// a player would wait, and a guard against a tree that would take hours.
pub const MAX_TREE_DEPTH: u32 = 24;

/// The most plies from the root a tree records (`maxTreePlies`). Each ply more multiplies the
/// nodes by the replies searched at each, three to five: a depth-16 tree held 560–900 nodes
/// at three plies and 6,300–9,300, with 16,000–23,000 decisions, at six.
pub const MAX_TREE_PLIES: u32 = 6;

/// The most events one recording keeps, the search's every decision near the root, `Enter` and
/// `Exit` included ([`Engine::search_tree`]'s `max_events`). Past it the tree is truncated.
/// Depth-14 trees of six plies recorded up to 218,000; an event takes about 70 bytes until the
/// log is folded.
pub const TREE_MAX_EVENTS: usize = 500_000;

/// How long a tree is kept with no request for it, in milliseconds (the result's `expiresMs`):
/// long enough to look round a position, short enough that trees don't pile up.
pub const TREE_EXPIRY_MS: u64 = 5 * 60 * 1000;

/// The most trees kept at once. Past it, the tree unused longest goes.
pub const MAX_TREES_KEPT: usize = 8;

/// The most nodes and decisions the kept trees may hold together, about 25 bytes each. Past
/// it, the trees unused longest go, but never the newest.
pub const MAX_TREE_ENTRIES_KEPT: usize = 2_000_000;

/// The hash table of a tree's engine, in megabytes: the engine's default size, plenty for
/// the second or so a tree's search takes on one thread.
pub const TREE_HASH_MB: usize = cb2026_engine::tt::DEFAULT_HASH_MB;

/// What a tree's engine is made from: the main engine's evaluation, network and tablebases, as
/// they came configured, and its switches at their defaults, as every search starts.
pub(crate) struct TreeSetup {
    params: EvalParams,
    search_params: SearchParams,
    techniques: Techniques,
    network: Option<Arc<Network>>,
    tablebase: Option<Arc<dyn Tablebase>>,
    tablebase_params: TablebaseParams,
}

impl TreeSetup {
    /// The setup of trees searched as `engine` searches, with the switches at `defaults` (in
    /// [`SWITCHES`]' order).
    pub(crate) fn new(engine: &Engine, defaults: &[bool]) -> TreeSetup {
        let mut template = Engine::new();
        template.techniques = engine.techniques;
        for (switch, &default) in SWITCHES.iter().zip(defaults) {
            *(switch.flag)(&mut template) = default;
        }
        TreeSetup {
            params: engine.params.clone(),
            search_params: engine.search_params,
            techniques: template.techniques,
            network: engine.network.clone(),
            tablebase: engine.tablebase.clone(),
            tablebase_params: engine.tablebase_params,
        }
    }

    /// A fresh engine for one tree: a cleared table of its own, one thread.
    fn engine(&self) -> Engine {
        let mut engine = Engine::new();
        engine.set_hash_size(TREE_HASH_MB);
        engine.params = self.params.clone();
        engine.search_params = self.search_params;
        engine.techniques = self.techniques;
        engine.network = self.network.clone();
        engine.tablebase = self.tablebase.clone();
        engine.tablebase_params = self.tablebase_params;
        engine.threads = 1;
        engine
    }
}

/// A tree request, checked and turned into the engine's terms.
pub(crate) struct PreparedTree {
    position: Position,
    history: Vec<u64>,
    depth: u32,
    plies: u32,
    limit: Option<u32>,
    chess960: bool,
}

/// Checks a tree request: its depth and plies within the node's limits, and its position.
pub(crate) fn prepare(params: &TreeParams) -> Result<PreparedTree, ResponseError> {
    let invalid = |message: String| ResponseError::new(code::INVALID_PARAMS, message);
    let too_far = |message: String| ResponseError::new(code::LIMIT_EXCEEDED, message);
    let plies = params.recorded_plies();
    match params.depth {
        0 => return Err(invalid("a tree's depth is at least 1".to_string())),
        depth if depth > MAX_TREE_DEPTH => {
            return Err(too_far(format!(
                "a tree's depth is at most {MAX_TREE_DEPTH}"
            )));
        }
        _ => {}
    }
    match plies {
        0 => return Err(invalid("a tree records at least 1 ply".to_string())),
        plies if plies > MAX_TREE_PLIES => {
            return Err(too_far(format!(
                "a tree records at most {MAX_TREE_PLIES} plies"
            )));
        }
        _ => {}
    }
    let (position, history) = read_position(&params.position)?;
    Ok(PreparedTree {
        position,
        history,
        depth: params.depth,
        plies,
        limit: params.limit,
        chess960: params.position.chess960,
    })
}

/// A tree being recorded on its own thread.
pub(crate) struct Recording {
    /// Its request's id.
    pub(crate) id: Id,
    /// Tells its search to stop (a cancel, `shutdown`, or the watchdog).
    pub(crate) stop: Arc<AtomicBool>,
    /// Set by a cancel or `shutdown`: the answer is `Cancelled`, and nothing is kept.
    pub(crate) cancelled: Arc<AtomicBool>,
    /// Set just before it answers: from then on it only has to end.
    answered: Arc<AtomicBool>,
    /// The thread.
    handle: JoinHandle<()>,
}

impl Recording {
    /// Whether it has finished, answered or been told to stop: it has at most a few
    /// milliseconds left.
    pub(crate) fn ending(&self) -> bool {
        self.handle.is_finished()
            || self.answered.load(Ordering::Acquire)
            || self.stop.load(Ordering::Relaxed)
    }

    /// Waits for its thread to end. If it failed before it answered, answers for it, so the
    /// request still gets its one answer.
    pub(crate) fn join(self, output: &Output) {
        if self.handle.join().is_err() && !self.answered.load(Ordering::Acquire) {
            let error = ResponseError::new(code::INTERNAL_ERROR, "the tree's search failed");
            send(output, Response::error(Some(self.id), error).into());
        }
    }
}

/// Starts recording a tree on its own thread, which answers the request.
pub(crate) fn start(
    setup: &TreeSetup,
    prepared: PreparedTree,
    id: Id,
    output: &Output,
    trees: &Arc<Mutex<TreeStore>>,
) -> Recording {
    // Taken here, when the request has been read: durations count from then (spec 1).
    let start = Instant::now();
    let engine = setup.engine();
    let stop = Arc::new(AtomicBool::new(false));
    let cancelled = Arc::new(AtomicBool::new(false));
    let answered = Arc::new(AtomicBool::new(false));
    let handle = {
        let (stop, cancelled, answered) = (stop.clone(), cancelled.clone(), answered.clone());
        let (id, output, trees) = (id.clone(), output.clone(), trees.clone());
        thread::spawn(move || {
            let flags = RecordingFlags {
                stop,
                cancelled,
                answered,
            };
            record(engine, prepared, id, start, &flags, &output, &trees);
        })
    };
    Recording {
        id,
        stop,
        cancelled,
        answered,
        handle,
    }
}

/// The flags a recording thread shares with the command loop.
struct RecordingFlags {
    stop: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
}

/// Records one tree, keeps it, and answers its request.
fn record(
    mut engine: Engine,
    prepared: PreparedTree,
    id: Id,
    start: Instant,
    flags: &RecordingFlags,
    output: &Output,
    trees: &Mutex<TreeStore>,
) {
    let finished = Arc::new(AtomicBool::new(false));
    // The node's maxTimeMs bounds a tree's search as it does every search.
    let watchdog = {
        let (stop, finished) = (flags.stop.clone(), finished.clone());
        thread::spawn(move || {
            let deadline = Duration::from_millis(MAX_TIME_MS);
            while !finished.load(Ordering::Relaxed) {
                if start.elapsed() >= deadline {
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
                thread::sleep(WATCHDOG_POLL);
            }
        })
    };
    let plies = prepared.plies as usize;
    // With no legal move there is nothing to search: the tree is its root alone.
    let searched = (!prepared.position.legal_moves().is_empty()).then(|| {
        engine.search_tree(
            &prepared.position,
            &prepared.history,
            prepared.depth,
            plies,
            TREE_MAX_EVENTS,
            &flags.stop,
        )
    });
    finished.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    // Checked last, so a cancel that arrived at any time is honoured.
    let response = if flags.cancelled.load(Ordering::Relaxed) {
        Response::error(Some(id), ResponseError::new(code::CANCELLED, "cancelled"))
    } else if flags.stop.load(Ordering::Relaxed) {
        let message = format!("the tree's search took longer than maxTimeMs ({MAX_TIME_MS} ms)");
        Response::error(Some(id), ResponseError::new(code::LIMIT_EXCEEDED, message))
    } else {
        let (folded, info) = match searched {
            Some((log, info)) => (FoldedTree::fold(&log), Some(info)),
            None => (FoldedTree::fold(&SearchTree::new(plies, 0)), None),
        };
        let kept = KeptTree {
            handle: String::new(),
            root: prepared.position,
            chess960: prepared.chess960,
            depth: info.as_ref().map_or(0, |info| info.depth),
            score: info.as_ref().map(|info| score(info.score)),
            negative_extension_plies: engine.search_params.negative_extension_plies,
            tree: folded,
            last_used: Instant::now(),
        };
        let mut store = lock(trees);
        let kept = store.keep(kept);
        let result = tree_result(kept, info.as_ref(), millis(start.elapsed()), prepared.limit);
        drop(store);
        Response::ok::<Tree>(id, &result)
    };
    // Before the answer goes: once the client has it, it may ask for the next tree at once.
    flags.answered.store(true, Ordering::Release);
    send(output, response.into());
}

/// One kept tree.
pub(crate) struct KeptTree {
    /// Its handle.
    handle: String,
    /// The position searched, with the moves before it played.
    root: Position,
    /// Whether moves are written as Chess960's castling.
    chess960: bool,
    /// The depth searched (0 with no legal move).
    depth: u32,
    /// The search's score, exact (none with no legal move).
    score: Option<cnp::engine::Score>,
    /// What a negative extension takes off, for its `reduced` decision.
    negative_extension_plies: u32,
    /// The tree.
    tree: FoldedTree,
    /// When a request last used it.
    last_used: Instant,
}

impl KeptTree {
    /// How many nodes and decisions it holds.
    fn entries(&self) -> usize {
        self.tree.nodes.len() + self.tree.decisions()
    }
}

/// The trees kept for `engine/treeNode`, oldest use first.
#[derive(Default)]
pub(crate) struct TreeStore {
    /// How many handles have been given: the next is one more.
    given: u64,
    /// The trees, the one unused longest first.
    trees: Vec<KeptTree>,
}

impl TreeStore {
    /// Drops the trees unused for [`TREE_EXPIRY_MS`].
    fn expire(&mut self) {
        let expiry = Duration::from_millis(TREE_EXPIRY_MS);
        self.trees.retain(|tree| tree.last_used.elapsed() < expiry);
    }

    /// Keeps `tree` under a new handle, making room by dropping the trees unused longest, and
    /// returns it.
    fn keep(&mut self, mut tree: KeptTree) -> &KeptTree {
        self.expire();
        self.given += 1;
        tree.handle = format!("t{}", self.given);
        self.trees.push(tree);
        let mut entries: usize = self.trees.iter().map(KeptTree::entries).sum();
        while self.trees.len() > 1
            && (self.trees.len() > MAX_TREES_KEPT || entries > MAX_TREE_ENTRIES_KEPT)
        {
            entries -= self.trees.remove(0).entries();
        }
        self.trees.last().expect("just kept")
    }

    /// The tree `handle` names, used now, if it is still kept.
    fn used(&mut self, handle: &str) -> Option<&KeptTree> {
        self.expire();
        let at = self.trees.iter().position(|tree| tree.handle == handle)?;
        let mut tree = self.trees.remove(at);
        tree.last_used = Instant::now();
        self.trees.push(tree);
        self.trees.last()
    }

    /// Forgets every tree (`shutdown`).
    pub(crate) fn clear(&mut self) {
        self.trees.clear();
    }
}

/// The store, even if a recording thread failed while holding it: its trees are whole, since a
/// tree is added in one push.
pub(crate) fn lock(trees: &Mutex<TreeStore>) -> MutexGuard<'_, TreeStore> {
    trees
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// `engine/treeNode`: the node `params.path` leads to in its tree, and a page of its children.
pub(crate) fn node_page(
    trees: &Mutex<TreeStore>,
    params: &TreeNodeParams,
) -> Result<TreeNodeResult, ResponseError> {
    let mut store = lock(trees);
    let kept = store.used(&params.tree).ok_or_else(|| {
        ResponseError::new(
            code::EXPIRED,
            format!("no tree {}: it has expired; ask for it again", params.tree),
        )
    })?;
    let mut position = kept.root.clone();
    let mut at = FoldedTree::ROOT;
    for (n, text) in params.path.iter().enumerate() {
        let child = position.parse_uci_move(text).and_then(|mv| {
            position.make_move(mv);
            kept.tree.nodes[at]
                .children
                .iter()
                .copied()
                .find(|&child| kept.tree.nodes[child].mv == Some(mv))
        });
        at = child.ok_or_else(|| {
            ResponseError::new(
                code::INVALID_PARAMS,
                format!("the tree has no node at {}", params.path[..=n].join(" ")),
            )
        })?;
    }
    Ok(TreeNodeResult {
        node: tree_node(kept, at),
        children: page(kept, at, params.offset.unwrap_or(0), params.limit),
    })
}

/// The tree's answer: its handle, the search's result, and the root with its first children.
fn tree_result(
    kept: &KeptTree,
    info: Option<&SearchInfo>,
    time_ms: u64,
    limit: Option<u32>,
) -> TreeResult {
    let chess960 = kept.chess960;
    TreeResult {
        tree: kept.handle.clone(),
        expires_ms: TREE_EXPIRY_MS,
        depth: kept.depth,
        sel_depth: info.map_or(0, |info| info.seldepth),
        nodes: info.map_or(0, |info| info.nodes),
        time_ms,
        best_move: info
            .and_then(|info| info.pv.first())
            .map(|mv| mv.to_uci(chess960)),
        score: kept.score,
        pv: info.map_or_else(Vec::new, |info| moves(&info.pv, chess960)),
        recorded: Recorded {
            plies: kept.tree.max_ply as u32,
            nodes: kept.tree.nodes.len() as u64,
            decisions: (kept.tree.decisions() + kept.tree.root_searches.saturating_sub(1)) as u64,
            truncated: kept.tree.truncated,
        },
        root: tree_node(kept, FoldedTree::ROOT),
        children: page(kept, FoldedTree::ROOT, 0, limit),
    }
}

/// The children of node `at`, from `offset`, at most `limit` of them.
fn page(kept: &KeptTree, at: usize, offset: u32, limit: Option<u32>) -> Vec<TreeNode> {
    let children = &kept.tree.nodes[at].children;
    let limit = limit.map_or(usize::MAX, |limit| limit as usize);
    children
        .iter()
        .skip(offset as usize)
        .take(limit)
        .map(|&child| tree_node(kept, child))
        .collect()
}

/// Node `at` in CNP's terms: its window and score from its side to move, mates counted from
/// it, the score bounded by the window of the search that gave it.
fn tree_node(kept: &KeptTree, at: usize) -> TreeNode {
    let node = &kept.tree.nodes[at];
    let chess960 = kept.chess960;
    let ply = node.ply as u32;
    let edge = |value: i32| (value.abs() < INFINITY).then(|| score(to_score_at(value, ply)));
    let (depth, alpha, beta, node_score) = match node.window {
        // A root with no legal move, never searched: no window, no score.
        None => (kept.depth, None, None, kept.score),
        Some(window) => {
            let bounded = node.score.map(|value| {
                let bound = if value <= window.alpha {
                    Bound::Upper
                } else if value >= window.beta {
                    Bound::Lower
                } else {
                    Bound::Exact
                };
                cnp::engine::Score {
                    bound,
                    ..score(to_score_at(value, ply))
                }
            });
            (window.depth, edge(window.alpha), edge(window.beta), bounded)
        }
    };
    TreeNode {
        mv: node.mv.map(|mv| mv.to_uci(chess960)),
        depth,
        alpha,
        beta,
        score: node_score,
        child_count: node.children.len() as u32,
        subtree: node.subtree as u64,
        decisions: aspiration_researches(kept, at)
            .chain(
                node.notes
                    .iter()
                    .filter_map(|&note| decision(note, chess960, kept.negative_extension_plies)),
            )
            .collect(),
    }
}

/// At the root, a `research` for each of its searches before the last: an aspiration window
/// the score fell outside, widened and searched again.
fn aspiration_researches(kept: &KeptTree, at: usize) -> impl Iterator<Item = Decision> {
    let earlier = if at == FoldedTree::ROOT {
        kept.tree.root_searches.saturating_sub(1)
    } else {
        0
    };
    std::iter::repeat_n(
        Decision::new(DecisionKind::Research {
            reason: ResearchReason::Window,
        })
        .by("aspiration"),
        earlier,
    )
}

/// One of the engine's notes as a CNP decision (the table in the module's docs). `None` for a
/// note that decides nothing, which the fold has already left out.
pub(crate) fn decision(
    note: Note,
    chess960: bool,
    negative_extension_plies: u32,
) -> Option<Decision> {
    let text = |mv: Move| mv.to_uci(chess960);
    let pruned = |mv: Move| DecisionKind::MovePruned { mv: text(mv) };
    let (kind, technique) = match note {
        Note::BetaCutoff(mv, order) => (
            DecisionKind::Cutoff {
                mv: text(mv),
                order,
            },
            Some("alphabeta"),
        ),
        Note::TtCutoff => (DecisionKind::Table, Some("tt")),
        Note::Tablebase => (DecisionKind::Tablebase, None),
        Note::NullMoveCutoff => (DecisionKind::NullMove, Some("nullmove")),
        Note::ReverseFutility => (DecisionKind::NodePruned { mv: None }, Some("rfp")),
        Note::Razoring => (DecisionKind::NodePruned { mv: None }, Some("razoring")),
        Note::MultiCut => (DecisionKind::NodePruned { mv: None }, Some("multicut")),
        Note::ProbCutCutoff(mv) => (
            DecisionKind::NodePruned { mv: Some(text(mv)) },
            Some("probcut"),
        ),
        Note::LateMovePruning(mv) => (pruned(mv), Some("lmp")),
        Note::HistoryPruning(mv) => (pruned(mv), Some("histprune")),
        Note::Futility(mv) => (pruned(mv), Some("futility")),
        Note::SeePruning(mv) => (pruned(mv), Some("seeprune")),
        Note::DeltaPruning(mv) => (pruned(mv), Some("delta")),
        Note::Reduced(_, plies) => (DecisionKind::Reduced { plies }, Some("lmr")),
        Note::NegativeExtension(_) => (
            DecisionKind::Reduced {
                plies: negative_extension_plies,
            },
            Some("negext"),
        ),
        Note::SingularExtension(_) => (DecisionKind::Extended { plies: 1 }, Some("singular")),
        Note::DoubleExtension(_) => (DecisionKind::Extended { plies: 1 }, Some("doubleext")),
        Note::ReductionResearch(_) => (
            DecisionKind::Research {
                reason: ResearchReason::Depth,
            },
            Some("lmr"),
        ),
        Note::PvsResearch(_) => (
            DecisionKind::Research {
                reason: ResearchReason::Window,
            },
            Some("pvs"),
        ),
        Note::Enter { .. }
        | Note::Exit { .. }
        | Note::Quiescence
        | Note::TtHit
        | Note::NullMove
        | Note::ProbCut
        | Note::SingularSearch(_) => return None,
    };
    let decision = Decision::new(kind);
    Some(match technique {
        Some(id) => decision.by(id),
        None => decision,
    })
}

#[cfg(test)]
mod tests {
    use cb2026_engine::search::MATE;
    use cb2026_engine::tree::{FoldedNode, Window};
    use cnp::engine::ScoreValue;

    use super::*;

    /// Every note there is, with a move where it takes one.
    fn every_note(mv: Move) -> Vec<Note> {
        vec![
            Note::Enter {
                depth: 1,
                alpha: 0,
                beta: 1,
            },
            Note::Exit { score: 0 },
            Note::Quiescence,
            Note::TtHit,
            Note::TtCutoff,
            Note::Tablebase,
            Note::ReverseFutility,
            Note::Razoring,
            Note::NullMove,
            Note::NullMoveCutoff,
            Note::ProbCut,
            Note::ProbCutCutoff(mv),
            Note::LateMovePruning(mv),
            Note::HistoryPruning(mv),
            Note::Futility(mv),
            Note::SeePruning(mv),
            Note::DeltaPruning(mv),
            Note::Reduced(mv, 2),
            Note::ReductionResearch(mv),
            Note::PvsResearch(mv),
            Note::BetaCutoff(mv, 3),
            Note::SingularSearch(mv),
            Note::SingularExtension(mv),
            Note::DoubleExtension(mv),
            Note::MultiCut,
            Note::NegativeExtension(mv),
        ]
    }

    #[test]
    fn every_decision_names_a_switch_cb2026_has() {
        let mv = Position::startpos().parse_uci_move("e2e4").unwrap();
        let mut decided = 0;
        for note in every_note(mv) {
            let Some(decision) = decision(note, false, 1) else {
                continue;
            };
            decided += 1;
            if let Some(id) = &decision.technique {
                assert!(SWITCHES.iter().any(|s| s.name == id), "{id} for {note:?}");
            }
        }
        // Every note but the brackets and the five attempts is a decision.
        assert_eq!(decided, every_note(mv).len() - 7);
        assert!(SWITCHES.iter().any(|s| s.name == "aspiration"));
        // Their shapes, for the ones that carry something.
        let cutoff = decision(Note::BetaCutoff(mv, 3), false, 1).unwrap();
        assert_eq!(
            cutoff.kind,
            DecisionKind::Cutoff {
                mv: "e2e4".to_string(),
                order: 3
            }
        );
        let shallower = decision(Note::NegativeExtension(mv), false, 2).unwrap();
        assert_eq!(shallower.kind, DecisionKind::Reduced { plies: 2 });
        let probcut = decision(Note::ProbCutCutoff(mv), false, 1).unwrap();
        assert_eq!(
            probcut.kind,
            DecisionKind::NodePruned {
                mv: Some("e2e4".to_string())
            }
        );
    }

    /// A kept tree of the start position: the root and one child, `e2e4`, with `window` and
    /// `score` as the search's values.
    fn kept(window: Window, score: i32) -> KeptTree {
        let e4 = Position::startpos().parse_uci_move("e2e4").unwrap();
        KeptTree {
            handle: "t1".to_string(),
            root: Position::startpos(),
            chess960: false,
            depth: 3,
            score: Some(cnp::engine::Score::cp(10)),
            negative_extension_plies: 1,
            tree: FoldedTree {
                nodes: vec![
                    FoldedNode {
                        children: vec![1],
                        subtree: 1,
                        ..FoldedNode::default()
                    },
                    FoldedNode {
                        mv: Some(e4),
                        parent: Some(0),
                        ply: 1,
                        window: Some(window),
                        score: Some(score),
                        notes: vec![Note::Reduced(e4, 1)],
                        ..FoldedNode::default()
                    },
                ],
                root_searches: 3,
                max_ply: 1,
                truncated: false,
            },
            last_used: Instant::now(),
        }
    }

    #[test]
    fn a_node_s_score_is_bounded_by_its_window_and_mates_count_from_it() {
        let window = Window {
            depth: 2,
            alpha: -11,
            beta: -10,
        };
        let bound = |score| tree_node(&kept(window, score), 1).score.unwrap().bound;
        assert_eq!(bound(-10), Bound::Lower);
        assert_eq!(bound(-11), Bound::Upper);
        let open = Window {
            depth: 2,
            alpha: -INFINITY,
            beta: INFINITY,
        };
        let node = tree_node(&kept(open, -12), 1);
        assert_eq!(node.mv.as_deref(), Some("e2e4"));
        assert_eq!((node.alpha, node.beta), (None, None));
        assert_eq!(node.score, Some(cnp::engine::Score::cp(-12)));
        assert_eq!(
            node.decisions,
            vec![Decision::new(DecisionKind::Reduced { plies: 1 }).by("lmr")]
        );
        // A mate four plies from the root: at the node one ply down, its side mates in two
        // moves (three plies); two plies down, the other side is mated in one.
        let mated = tree_node(&kept(open, MATE - 4), 1).score.unwrap();
        assert_eq!(mated.value, ScoreValue::Mate(2));
        assert_eq!(to_score_at(-(MATE - 4), 2), cb2026_engine::Score::Mate(-1));
        // The root: its result, and a research for each of its searches before the last.
        let root = tree_node(&kept(open, 0), FoldedTree::ROOT);
        assert_eq!(
            (root.mv, root.depth, root.child_count, root.subtree),
            (None, 3, 1, 1)
        );
        let research = Decision::new(DecisionKind::Research {
            reason: ResearchReason::Window,
        })
        .by("aspiration");
        assert_eq!(root.decisions, vec![research.clone(), research]);
    }

    #[test]
    fn the_store_drops_expired_trees_then_the_ones_unused_longest() {
        let open = Window {
            depth: 2,
            alpha: -INFINITY,
            beta: INFINITY,
        };
        let mut store = TreeStore::default();
        let handles: Vec<String> = (0..MAX_TREES_KEPT)
            .map(|_| store.keep(kept(open, 0)).handle.clone())
            .collect();
        assert_eq!(handles[0], "t1");
        // Using the first makes the second the one unused longest, which the next tree drops.
        assert!(store.used("t1").is_some());
        let newest = store.keep(kept(open, 0)).handle.clone();
        assert_eq!(newest, format!("t{}", MAX_TREES_KEPT + 1));
        assert!(store.used("t2").is_none());
        assert!(store.used("t1").is_some() && store.used("t3").is_some());
        // A tree unused for the expiry is gone, and its handle never comes back.
        let expiry = Duration::from_millis(TREE_EXPIRY_MS);
        for tree in &mut store.trees {
            tree.last_used = Instant::now().checked_sub(expiry).unwrap();
        }
        assert!(store.used("t1").is_none());
        assert!(store.trees.is_empty());
        assert_eq!(
            store.keep(kept(open, 0)).handle,
            format!("t{}", MAX_TREES_KEPT + 2)
        );
    }
}
