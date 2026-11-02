//! # cb2026-cnp: cb2026 as a Chess Node Protocol node
//!
//! The [Chess Node Protocol](https://github.com/cjus/chess-node-protocol) (CNP) is how
//! Solrac, the 3D board, talks to cb2026: JSON-RPC 2.0 over stdin/stdout, one message per line.
//! Where UCI sends a position and `go`, CNP sends one request that carries everything: the
//! position, its limits, a level, which techniques are on, root moves, MultiPV. Every answer
//! says whether a score is exact, and the engine's techniques and levels can be listed, and a
//! position's evaluation had term by term (`engine/evaluate`). Its `initialize` names the
//! version with the commit it was built from ([`build_version`], `1.0.0+f1638bcb6851`), so a
//! client that keeps what the engine said, as Solrac's game review does, can tell builds apart.
//! It ponders: a search with `"ponder": true` runs on the opponent's time until
//! `engine/ponderHit` (spec 7.7). And it keeps search trees (spec 7.8): `engine/tree` records a
//! search's decisions near the root and `engine/treeNode` reads them a node at a time, so
//! Solrac's Engine Mind can say why cb2026 set a move aside (the `tree` module). And a search
//! with `rootMs` is followed at its root (spec 7.9): `engine/root` says which root move it is
//! on, where its effort has gone, and, at once, when it goes back over something.
//! `cb2026 --cnp` speaks it, and so does a plain `cb2026` whose first input line is JSON.
//!
//! ```text
//! → {"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocol":"1.0","client":{"name":"Solrac","version":"0.2.0"}}}
//! ← {"jsonrpc":"2.0","id":1,"result":{"protocol":"1.0","node":{"name":"cb2026",…},"capabilities":{"engine":{…}},"limits":{…}}}
//! → {"jsonrpc":"2.0","id":2,"method":"engine/search","params":{"position":{"fen":"…","moves":["e2e4"]},"limits":{"depth":12}}}
//! ← {"jsonrpc":"2.0","id":2,"result":{"bestMove":"c7c5","score":{"cp":-24},"pv":["c7c5","g1f3"],"depth":12,…}}
//! ```
//!
//! ## How it works
//!
//! - **The command loop answers at once.** It reads one message at a time and answers it, so
//!   `ping`, `engine/stop` and `$/cancelRequest` act while a search runs.
//! - **A search runs on a thread of its own**, as UCI's does, owning the engine until it ends.
//!   That thread sends the search's progress and its one response. The loop answers every
//!   other request, so each request gets exactly one answer.
//! - **One search at a time** (`maxConcurrentSearches` 1). A search that arrives while another
//!   runs is `Busy`, unless the running one has answered, or has been stopped or cancelled.
//!   Then the node waits for it to wind down, which takes a few milliseconds, and starts the
//!   new one. So a client
//!   can cancel and search again at once, as a takeback does.
//! - **Requests stand alone** (spec, 1). Before each search, every technique, the level and
//!   MultiPV go back to their defaults, and then the request's are applied. The `game` hint
//!   keeps the hash table between searches of one game. A search without one, or from another
//!   game, starts afresh.
//! - **The node's limits bound every search.** An infinite search waits for `engine/stop` or
//!   `$/cancelRequest`, as UCI's waits for `stop`, and a watchdog ends it after `maxTimeMs`.
//!   A search with no limits at all gets [`DEFAULT_SEARCH_MS`].
//! - **A ponder search keeps to no time until its hit.** Its position has the expected reply
//!   played and its limits are the ones for after the hit, the clock included. It searches with
//!   its depth, node and mate limits but no time, and as deep as it can if it has none of those
//!   ([`Limits::pondering`]), and doesn't answer until `engine/ponderHit`, `engine/stop` or a cancel,
//!   even when it has nothing left to search. The hit records its moment, and the watchdog
//!   stops the search once the time the time manager would have given it (`timeMs`, or a share
//!   of the clock) has passed since then: the pondering was a bonus. A depth or node limit it
//!   reached before the hit ends it at once, since it has stopped and is only waiting. It is
//!   the same search throughout, so it keeps its tree, and its answer says how long it pondered
//!   (`ponderMs`). On a miss the client cancels it, and the next search is taken at once, as
//!   after any cancel. UCI's `go ponder` and `ponderhit` work the same way (`cb2026-protocol`).
//! - **Following the root is throttled on its own.** The engine tells the search thread where
//!   its root loop is at every root move and whenever it looks at the clock
//!   ([`cb2026_engine::root`]). The thread passes it on as `engine/root` once `rootMs` has
//!   passed since the last one, and a research (a better move, an aspiration window's failure)
//!   at once, the interval then counting from it. `engine/progress` keeps its own interval, so neither holds the other
//!   back. Only the main thread follows its root, so a move's nodes are that thread's.
//! - **A tree has an engine and a thread of its own** (the `tree` module), so it is answered while a
//!   search runs, and is the same tree each time it is asked for. One at a time
//!   (`maxConcurrentTrees` 1). The trees are kept for [`TREE_EXPIRY_MS`] after their last use,
//!   at most [`MAX_TREES_KEPT`] of them, and `engine/treeNode` is answered in the loop, at once.
//!
//! ## What it offers
//!
//! Every engine capability except `window`: cb2026's search always starts with an open window
//! at the root. Pondering, the search tree and following the root included, the tree to
//! [`MAX_TREE_DEPTH`] and [`MAX_TREE_PLIES`]. None of the other domains: voice, and the games,
//! preferences and backups that Solrac's own node keeps. Its techniques are the shell's switches
//! ([`cb2026_shell::SWITCHES`]), by the names the shell's `set` uses. Its levels are [`RATINGS`],
//! whole numbers only, since a level's settings are whole steps. A rating within [`ELO_RANGE`]
//! picks a level as `UCI_Elo` does.
//! `engine/evaluate` answers with the shell's `eval`: [`eval::trace`]'s eight terms ([`TERMS`]),
//! each blended for the position's phase as the evaluation blends its total. Those are the
//! hand-crafted evaluation's, while the search usually evaluates with the network, so its answer
//! also says the network's score (`searchScore`) and, asked, what each piece is worth to it
//! (`pieces`, [`Engine::piece_values`]). With `quiet` it first plays out the quiescence search's
//! line ([`Engine::quiet_line`]), which is where a search's score comes from when its line ends
//! mid-exchange. An engine of its own answers these ([`EVALUATOR_HASH_MB`]), cleared before each
//! request, so an evaluation is the same whether or not a search is running.
//!
//! ## Pitfalls
//!
//! - **A cancelled search must never answer with a move.** The client has moved on, after a
//!   takeback or a new game, and a late move would be played on the wrong position. The search
//!   thread checks the cancel flag last, just before it answers.
//! - **Deterministic searches at a level aren't deterministic.** A level chooses among its
//!   candidate moves at random, so `deterministic` with `strength` is `Unsupported` rather than
//!   a promise the node can't keep.
//! - **An evaluation is answered while a search runs**, so it can't borrow the engine, which
//!   the search thread owns. The node keeps a copy of the engine's evaluation parameters as it
//!   came configured, which is also what every search starts from: a technique a search
//!   switches off (`pst`, `tapered`) doesn't reach an evaluation.
//! - **An answered search still holds the engine for a moment.** Its thread sends the answer,
//!   then gives the engine back, and a client may send its next search in between (Solrac's
//!   chat does, for a review's two searches). That search must be taken, not refused as
//!   `Busy`: nothing runs any more. So the thread says it has answered just before it sends,
//!   and a new search waits the moment it takes to end.
//! - **A hit can cross with the search's end.** The client may send `engine/ponderHit` just as
//!   a stop, a cancel or the node's `maxTimeMs` ends the search, or twice. Such a hit, or one
//!   for a search that never existed, answers `null` and does nothing; only a hit for a running
//!   search that isn't pondering is `InvalidParams` (spec 7.7).
//! - **A ponder search's time counts from the hit, not from the request.** Taken from the
//!   request, a long ponder would use up the move's time before the move was even known, and
//!   the hit would end the search at once.
//! - **Castling notation follows the position.** cb2026 reads both notations. It writes the
//!   king's two-square move, or king-takes-rook when the position says `chess960`.

#![forbid(unsafe_code)]

mod tree;

use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};
use cb2026_engine::eval::{self, MAX_PHASE};
use cb2026_engine::levels::{ELO_RANGE, FULL_STRENGTH, RATINGS};
use cb2026_engine::search::MAX_PLY;
use cb2026_engine::tablebase::TablebaseLoader;
use cb2026_engine::time;
use cb2026_engine::{
    AUTHOR, Engine, Limits, NAME, RootInfo, RootResearch, Score, SearchInfo, SearchResult,
    build_version,
};
use cb2026_shell::SWITCHES;
use cnp::engine::{
    Bound, EloRange, EngineCapabilities, EngineProgress, Evaluate, EvaluateParams, EvaluateResult,
    Level, Levels, LevelsResult, Line, PieceValue, PonderHit, Progress, Root, RootMove, RootParams,
    Search, SearchParams, SearchStats, Stop, Technique, Techniques, TechniquesResult, Term, Tree,
    TreeNodePage,
};
use cnp::lifecycle::{
    CancelRequest, Capabilities, IncompatibleVersionData, Initialize, InitializeResult,
    Limits as NodeLimits, NodeInfo, PROTOCOL_VERSION, Ping, Shutdown, negotiate,
};
use cnp::stdio::{ReadError, read_message, write_message};
use cnp::{Id, Message, Notification, Request, RequestMethod, Response, ResponseError, code};

pub use tree::{
    MAX_TREE_DEPTH, MAX_TREE_ENTRIES_KEPT, MAX_TREE_PLIES, MAX_TREES_KEPT, TREE_EXPIRY_MS,
    TREE_HASH_MB, TREE_MAX_EVENTS,
};

/// The longest search the node runs, infinite ones included: an hour.
pub const MAX_TIME_MS: u64 = 60 * 60 * 1000;

/// The most lines one search may return (as UCI's `MultiPV` option).
pub const MAX_MULTIPV: u32 = 256;

/// How long a search with no limits at all takes.
pub const DEFAULT_SEARCH_MS: u64 = 1000;

/// What the levels' ratings mean (see [`RATINGS`]): the engine's own scale,
/// [`cb2026_engine::levels::RATING_SCALE`].
pub const RATING_SCALE: &str = cb2026_engine::levels::RATING_SCALE;

/// Where cb2026's source is.
pub const SOURCE: &str = "https://github.com/cjus/chessengine";

/// The hash table of the engine that answers `engine/evaluate`, in megabytes. Its quiescence
/// search never reads the table, so the smallest size does.
pub const EVALUATOR_HASH_MB: usize = 1;

/// The evaluation's terms as `engine/evaluate` names them: each one's id and what it measures,
/// in [`eval::EvalTrace::terms`]' order (whose names are the terms' names).
pub const TERMS: [(&str, &str); 8] = [
    ("material", "What the pieces are worth."),
    (
        "pieceSquares",
        "Where the pieces stand: a bonus or a penalty for each piece on each square.",
    ),
    ("mobility", "Safe squares each piece can reach."),
    ("pawnStructure", "Doubled and isolated pawns."),
    (
        "passedPawns",
        "Pawns that no enemy pawn can stop, worth more the further they have gone.",
    ),
    (
        "pieces",
        "The bishop pair, and rooks on open and half-open files.",
    ),
    (
        "kingSafety",
        "The pawns sheltering the king, and attacks on the squares around it.",
    ),
    ("tempo", "A bonus for the side to move."),
];

/// How often an infinite search that has finished its work looks for `engine/stop`.
const WAIT_POLL: Duration = Duration::from_millis(1);

/// How often the watchdog looks at the clock.
const WATCHDOG_POLL: Duration = Duration::from_millis(10);

/// What the binary says about itself that this crate can't know.
#[derive(Clone, Copy, Debug)]
pub struct Config {
    /// The binary's licence as a whole, an SPDX expression: `MIT OR Apache-2.0`, or
    /// `GPL-3.0-or-later` when it was built with Syzygy probing.
    pub license: &'static str,
}

/// Where the node's messages go. Shared between the command loop and the search thread.
type Output = Arc<Mutex<Box<dyn Write + Send>>>;

/// Writes one message. If the client has gone there is no one left to tell, so errors are
/// ignored: the command loop sees the input end and stops.
fn send(output: &Output, message: Message) {
    let mut out = output
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = write_message(&mut *out, &message);
}

/// Runs a CNP session until `exit` or the end of the input, and says whether the client shut
/// it down first (the exit code: 0 if so, 1 if not; spec 5.3).
///
/// `first_line` is a line already read from the input (the JSON that told the binary which
/// protocol to speak); it is handled first.
pub fn run(
    input: impl BufRead,
    output: Box<dyn Write + Send>,
    first_line: Option<String>,
    engine: Engine,
    config: Config,
) -> io::Result<bool> {
    let first = first_line.unwrap_or_default();
    let mut input = io::Cursor::new(first.into_bytes()).chain(input);
    let mut node = Node::new(Arc::new(Mutex::new(output)), engine, config);
    let max_bytes = usize::try_from(NodeLimits::default().message_bytes()).unwrap_or(usize::MAX);
    loop {
        match read_message(&mut input, max_bytes) {
            Ok(None) => break,
            Ok(Some(Message::Request(request))) => node.request(request),
            Ok(Some(Message::Notification(notification))) => {
                if notification.is::<cnp::lifecycle::Exit>() {
                    break;
                }
                node.notification(&notification);
            }
            // The node sends no requests, so it expects no responses.
            Ok(Some(Message::Response(_))) => {}
            Err(ReadError::Io(e)) => {
                node.finish();
                return Err(e);
            }
            Err(e) => {
                if let Some(response) = e.response() {
                    send(&node.output, response.into());
                }
            }
        }
    }
    node.finish();
    Ok(node.state == State::ShutDown)
}

/// Where the session is in its lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    /// Before `initialize`.
    Starting,
    /// Between `initialize` and `shutdown`.
    Running,
    /// After `shutdown`.
    ShutDown,
}

/// A search running on its own thread.
struct Running {
    /// Its request's id.
    id: Id,
    /// Tells the engine to stop (set by `engine/stop`, a cancel, or the watchdog).
    stop: Arc<AtomicBool>,
    /// Set by `engine/stop`: the answer is marked `stopped`.
    stopped: Arc<AtomicBool>,
    /// Set by a cancel or `shutdown`: the answer is `Cancelled`, never a move.
    cancelled: Arc<AtomicBool>,
    /// Set by the search thread just before it sends its answer: from then on it only has to
    /// give the engine back, so a new search waits for it rather than being `Busy`.
    answered: Arc<AtomicBool>,
    /// For a ponder search, when its `engine/ponderHit` came (set once, by the command loop).
    /// `None` for any other search.
    hit: Option<Arc<OnceLock<Instant>>>,
    /// The thread, which gives the engine back when it ends.
    handle: JoinHandle<Engine>,
}

impl Running {
    /// Whether the search has finished, answered, or been told to stop: it has at most a few
    /// milliseconds left, and no message changes what it answers.
    fn ending(&self) -> bool {
        self.handle.is_finished()
            || self.answered.load(Ordering::Acquire)
            || self.stop.load(Ordering::Relaxed)
    }
}

/// The state of one CNP session.
struct Node {
    output: Output,
    config: Config,
    state: State,
    /// The engine, when no search owns it.
    engine: Option<Engine>,
    /// How the engine opens tablebases (kept to rebuild the engine if a search thread fails).
    tablebase_loader: Option<TablebaseLoader>,
    /// The search threads to use, as the engine came configured.
    threads: usize,
    /// Each switch's default, read once from a new engine (in [`SWITCHES`]' order).
    defaults: Vec<bool>,
    /// The engine that answers `engine/evaluate`, configured as the engine came, with a small
    /// hash table: the main engine may be busy with a search.
    evaluator: Engine,
    /// The running search, if any.
    search: Option<Running>,
    /// The `game` hint of the last search, whose hash table the engine still holds.
    last_game: Option<String>,
    /// What each tree's engine is made from.
    tree_setup: tree::TreeSetup,
    /// The tree being recorded, if any.
    recording: Option<tree::Recording>,
    /// The trees kept for `engine/treeNode`, shared with the recording thread.
    trees: Arc<Mutex<tree::TreeStore>>,
}

/// A search request, checked and turned into the engine's terms.
struct Prepared {
    position: Position,
    history: Vec<u64>,
    limits: Limits,
    multipv: usize,
    level: u32,
    elo: Option<u32>,
    techniques: Vec<(usize, bool)>,
    deterministic: bool,
    progress: Option<Duration>,
    /// How often to send `engine/root`, when the search follows its root (`rootMs`).
    root: Option<Duration>,
    game: Option<String>,
    chess960: bool,
    /// A ponder search: `limits` count from its hit (spec 7.7).
    ponder: bool,
}

impl Node {
    fn new(output: Output, engine: Engine, config: Config) -> Node {
        let mut fresh = Engine::new();
        let defaults: Vec<bool> = SWITCHES.iter().map(|s| *(s.flag)(&mut fresh)).collect();
        let tree_setup = tree::TreeSetup::new(&engine, &defaults);
        let mut evaluator = fresh;
        evaluator.set_hash_size(EVALUATOR_HASH_MB);
        evaluator.params = engine.params.clone();
        evaluator.techniques = engine.techniques;
        evaluator.network = engine.network.clone();
        Node {
            output,
            config,
            state: State::Starting,
            tablebase_loader: engine.tablebase_loader,
            threads: engine.threads,
            evaluator,
            engine: Some(engine),
            defaults,
            search: None,
            last_game: None,
            tree_setup,
            recording: None,
            trees: Arc::default(),
        }
    }

    /// Answers a request (a search is answered by its thread).
    fn request(&mut self, request: Request) {
        let id = request.id.clone();
        let error = |code, message: &str| {
            Response::error(Some(id.clone()), ResponseError::new(code, message))
        };
        let response = match (self.state, request.method.as_str()) {
            (State::Starting, Initialize::NAME) => self.initialize(&request),
            (State::Starting, _) => error(code::NOT_INITIALIZED, "initialize first"),
            (_, Initialize::NAME) => error(code::INVALID_REQUEST, "already initialized"),
            (State::ShutDown, _) => error(code::SHUTTING_DOWN, "shutting down"),
            (State::Running, Ping::NAME) => Response::ok::<Ping>(id, &()),
            (State::Running, Shutdown::NAME) => {
                self.finish();
                self.state = State::ShutDown;
                Response::ok::<Shutdown>(id, &())
            }
            (State::Running, Search::NAME) => match self.start_search(&request) {
                Ok(()) => return,
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, Stop::NAME) => match request.params::<Stop>() {
                Ok(params) => {
                    if let Some(running) = self.search.as_ref().filter(|r| r.id == params.id) {
                        running.stopped.store(true, Ordering::Relaxed);
                        running.stop.store(true, Ordering::Relaxed);
                    }
                    Response::ok::<Stop>(id, &())
                }
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, PonderHit::NAME) => match request
                .params::<PonderHit>()
                .and_then(|params| self.ponder_hit(&params.id))
            {
                Ok(()) => Response::ok::<PonderHit>(id, &()),
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, Techniques::NAME) => {
                Response::ok::<Techniques>(id, &self.techniques())
            }
            (State::Running, Levels::NAME) => Response::ok::<Levels>(id, &levels()),
            (State::Running, Evaluate::NAME) => match request
                .params::<Evaluate>()
                .and_then(|params| evaluate(&params, &mut self.evaluator))
            {
                Ok(result) => Response::ok::<Evaluate>(id, &result),
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, Tree::NAME) => match self.start_tree(&request) {
                Ok(()) => return,
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, TreeNodePage::NAME) => match request
                .params::<TreeNodePage>()
                .and_then(|params| tree::node_page(&self.trees, &params))
            {
                Ok(result) => Response::ok::<TreeNodePage>(id, &result),
                Err(e) => Response::error(Some(id), e),
            },
            (State::Running, method) => error(
                code::METHOD_NOT_FOUND,
                &format!("cb2026 has no method {method}"),
            ),
        };
        send(&self.output, response.into());
    }

    /// Acts on a notification: only `$/cancelRequest` means anything here.
    fn notification(&mut self, notification: &Notification) {
        if self.state != State::Running || !notification.is::<CancelRequest>() {
            return;
        }
        let Ok(params) = notification.params::<CancelRequest>() else {
            return;
        };
        if let Some(running) = self.search.as_ref().filter(|r| r.id == params.id) {
            running.cancelled.store(true, Ordering::Relaxed);
            running.stop.store(true, Ordering::Relaxed);
        }
        if let Some(recording) = self.recording.as_ref().filter(|r| r.id == params.id) {
            recording.cancelled.store(true, Ordering::Relaxed);
            recording.stop.store(true, Ordering::Relaxed);
        }
    }

    fn initialize(&mut self, request: &Request) -> Response {
        let id = request.id.clone();
        let params = match request.params::<Initialize>() {
            Ok(params) => params,
            Err(e) => return Response::error(Some(id), e),
        };
        let Some(protocol) = negotiate(params.protocol, PROTOCOL_VERSION) else {
            let data = IncompatibleVersionData {
                supported: vec![PROTOCOL_VERSION],
            };
            let message = format!("cb2026 speaks CNP {}.x", PROTOCOL_VERSION.major);
            return Response::error(
                Some(id),
                ResponseError::new(code::INCOMPATIBLE_VERSION, message).with_data(&data),
            );
        };
        self.state = State::Running;
        let result = InitializeResult {
            protocol,
            node: NodeInfo {
                name: NAME.to_string(),
                version: build_version(),
                author: Some(AUTHOR.to_string()),
                license: self.config.license.to_string(),
                source: Some(SOURCE.to_string()),
            },
            capabilities: Capabilities {
                engine: Some(EngineCapabilities {
                    multipv: true,
                    window: false,
                    root_moves: true,
                    strength: true,
                    techniques: true,
                    deterministic: true,
                    clock: true,
                    mate: true,
                    chess960: true,
                    progress: true,
                    evaluate: true,
                    quiet: true,
                    pieces: true,
                    ponder: true,
                    tree: true,
                    root: true,
                }),
                voice: None,
                preferences: None,
                games: None,
                backups: None,
            },
            limits: NodeLimits {
                max_depth: Some(MAX_PLY),
                max_nodes: None,
                max_time_ms: Some(MAX_TIME_MS),
                max_multipv: Some(MAX_MULTIPV),
                max_concurrent_searches: Some(1),
                max_message_bytes: None,
                max_tree_depth: Some(MAX_TREE_DEPTH),
                max_tree_plies: Some(MAX_TREE_PLIES),
                max_concurrent_trees: Some(1),
            },
        };
        Response::ok::<Initialize>(id, &result)
    }

    /// Checks a search and starts it on its own thread, or says why not.
    fn start_search(&mut self, request: &Request) -> Result<(), ResponseError> {
        let params = request.params::<Search>()?;
        let prepared = prepare(&params)?;
        if let Some(running) = self.search.take() {
            if running.ending() {
                // Finished, answered and ending, or winding down after a stop or a cancel (a
                // ponder miss's among them): a few milliseconds at most.
                self.reclaim(running);
            } else {
                self.search = Some(running);
                return Err(ResponseError::new(
                    code::BUSY,
                    "cb2026 runs one search at a time",
                ));
            }
        }
        let mut engine = self
            .engine
            .take()
            .expect("the engine is back when no search runs");
        self.configure(&mut engine, &prepared);
        // Taken here, before the thread starts, so a hit can never come before it.
        let start = Instant::now();
        let flags = Flags {
            stop: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(false)),
            hit: prepared.ponder.then(|| Arc::new(OnceLock::new())),
        };
        let id = request.id.clone();
        let running = Running {
            id: id.clone(),
            stop: Arc::clone(&flags.stop),
            stopped: Arc::clone(&flags.stopped),
            cancelled: Arc::clone(&flags.cancelled),
            answered: Arc::clone(&flags.answered),
            hit: flags.hit.clone(),
            handle: {
                let output = Arc::clone(&self.output);
                thread::spawn(move || search_thread(engine, prepared, id, start, flags, output))
            },
        };
        self.search = Some(running);
        Ok(())
    }

    /// `engine/ponderHit`: the move the ponder search `id` expected was played, so from now on
    /// it keeps to its limits (the watchdog times it from this moment). A hit that crosses with
    /// the search's end, or names no running search, or comes twice, does nothing (spec 7.7).
    fn ponder_hit(&self, id: &Id) -> Result<(), ResponseError> {
        let Some(running) = self
            .search
            .as_ref()
            .filter(|running| running.id == *id && !running.ending())
        else {
            return Ok(());
        };
        match &running.hit {
            Some(hit) => {
                // A second hit finds it set and leaves the first moment.
                let _ = hit.set(Instant::now());
                Ok(())
            }
            None => Err(ResponseError::new(
                code::INVALID_PARAMS,
                format!("search {id} isn't a ponder search"),
            )),
        }
    }

    /// Sets the engine up for one search: every option back to its default, then the
    /// request's (requests stand alone).
    fn configure(&mut self, engine: &mut Engine, prepared: &Prepared) {
        for (switch, default) in SWITCHES.iter().zip(&self.defaults) {
            *(switch.flag)(engine) = *default;
        }
        for &(index, on) in &prepared.techniques {
            *(SWITCHES[index].flag)(engine) = on;
        }
        engine.multipv = prepared.multipv;
        engine.skill_level = prepared.level;
        engine.limit_strength = prepared.elo.is_some();
        engine.elo = prepared.elo.unwrap_or(ELO_RANGE.1);
        engine.own_book = false;
        engine.threads = if prepared.deterministic {
            1
        } else {
            self.threads
        };
        let same_game = prepared.game.is_some() && prepared.game == self.last_game;
        if prepared.deterministic || !same_game {
            engine.new_game();
        }
        self.last_game = if prepared.deterministic {
            None
        } else {
            prepared.game.clone()
        };
    }

    /// Takes the engine back from a search thread, waiting for it to end.
    fn reclaim(&mut self, running: Running) {
        match running.handle.join() {
            Ok(engine) => self.engine = Some(engine),
            Err(_) => {
                // The thread failed before it answered: answer for it, and start afresh.
                let error = ResponseError::new(code::INTERNAL_ERROR, "the search failed");
                send(
                    &self.output,
                    Response::error(Some(running.id), error).into(),
                );
                let mut engine = Engine::new();
                engine.tablebase_loader = self.tablebase_loader;
                engine.threads = self.threads;
                self.engine = Some(engine);
            }
        }
    }

    /// Checks a tree request and starts recording it on its own thread, or says why not.
    fn start_tree(&mut self, request: &Request) -> Result<(), ResponseError> {
        let params = request.params::<Tree>()?;
        let prepared = tree::prepare(&params)?;
        if let Some(recording) = self.recording.take() {
            if recording.ending() {
                // Answered and ending, or winding down after a cancel: a few milliseconds.
                recording.join(&self.output);
            } else {
                self.recording = Some(recording);
                return Err(ResponseError::new(
                    code::BUSY,
                    "cb2026 records one tree at a time",
                ));
            }
        }
        self.recording = Some(tree::start(
            &self.tree_setup,
            prepared,
            request.id.clone(),
            &self.output,
            &self.trees,
        ));
        Ok(())
    }

    /// Cancels the running search and the tree being recorded, if any, waits for them to
    /// answer, and forgets the kept trees.
    fn finish(&mut self) {
        if let Some(running) = self.search.take() {
            running.cancelled.store(true, Ordering::Relaxed);
            running.stop.store(true, Ordering::Relaxed);
            self.reclaim(running);
        }
        if let Some(recording) = self.recording.take() {
            recording.cancelled.store(true, Ordering::Relaxed);
            recording.stop.store(true, Ordering::Relaxed);
            recording.join(&self.output);
        }
        tree::lock(&self.trees).clear();
    }

    /// The switchable techniques: the shell's switches, under the names `set` uses.
    fn techniques(&self) -> TechniquesResult {
        TechniquesResult {
            techniques: SWITCHES
                .iter()
                .zip(&self.defaults)
                .map(|(switch, default)| Technique {
                    id: switch.name.to_string(),
                    name: short_name(switch.description).to_string(),
                    description: switch.description.to_string(),
                    default: *default,
                    requires: Vec::new(),
                    link: None,
                })
                .collect(),
        }
    }
}

/// The flags a search thread shares with the command loop.
struct Flags {
    stop: Arc<AtomicBool>,
    stopped: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    answered: Arc<AtomicBool>,
    hit: Option<Arc<OnceLock<Instant>>>,
}

impl Flags {
    /// When the ponder search's hit came, if it has.
    fn hit_at(&self) -> Option<Instant> {
        self.hit.as_ref().and_then(|hit| hit.get().copied())
    }
}

/// Checks a search's params and turns them into the engine's terms.
fn prepare(params: &SearchParams) -> Result<Prepared, ResponseError> {
    let invalid = |message: String| ResponseError::new(code::INVALID_PARAMS, message);
    let unsupported = |message: String| ResponseError::new(code::UNSUPPORTED, message);
    let too_far = |message: String| ResponseError::new(code::LIMIT_EXCEEDED, message);
    let illegal = |message: String| ResponseError::new(code::INVALID_POSITION, message);

    if params.window.is_some() {
        return Err(unsupported(
            "cb2026 doesn't search within a window".to_string(),
        ));
    }
    let multipv = params.multipv.unwrap_or(1);
    if multipv == 0 {
        return Err(invalid("multipv is at least 1".to_string()));
    }
    if multipv > MAX_MULTIPV {
        return Err(too_far(format!("multipv is at most {MAX_MULTIPV}")));
    }
    let (level, elo) = match &params.strength {
        None => (FULL_STRENGTH, None),
        Some(strength) => match (strength.level, strength.elo) {
            (Some(level), None) => {
                if level.fract() != 0.0 || !(0.0..=f64::from(FULL_STRENGTH)).contains(&level) {
                    return Err(invalid(format!(
                        "a level is a whole number from 0 to {FULL_STRENGTH}"
                    )));
                }
                // Checked just above: a whole number from 0 to 20.
                (level as u32, None)
            }
            (None, Some(elo)) => {
                if !(ELO_RANGE.0..=ELO_RANGE.1).contains(&elo) {
                    return Err(invalid(format!(
                        "elo is from {} to {}",
                        ELO_RANGE.0, ELO_RANGE.1
                    )));
                }
                (FULL_STRENGTH, Some(elo))
            }
            (None, None) => (FULL_STRENGTH, None),
            (Some(_), Some(_)) => {
                return Err(invalid(
                    "strength has a level or an elo, not both".to_string(),
                ));
            }
        },
    };
    if params.deterministic && (level != FULL_STRENGTH || elo.is_some()) {
        return Err(unsupported(
            "a level chooses its move at random, so it can't be deterministic".to_string(),
        ));
    }
    let mut techniques = Vec::new();
    for (id, &on) in &params.techniques {
        let Some(index) = SWITCHES.iter().position(|s| s.name == id) else {
            return Err(unsupported(format!(
                "cb2026 has no technique {id:?}; engine/techniques lists them"
            )));
        };
        techniques.push((index, on));
    }

    let (position, history) = read_position(&params.position)?;

    let wanted = &params.limits;
    let mut limits = Limits::default();
    if let Some(depth) = wanted.depth {
        if depth > MAX_PLY {
            return Err(too_far(format!("depth is at most {MAX_PLY}")));
        }
        limits.depth = Some(depth);
    }
    if let Some(ms) = wanted.time_ms {
        if ms > MAX_TIME_MS {
            return Err(too_far(format!("timeMs is at most {MAX_TIME_MS}")));
        }
        limits.movetime = Some(Duration::from_millis(ms));
    }
    limits.nodes = wanted.nodes;
    limits.mate = wanted.mate;
    if let Some(clock) = &wanted.clock {
        limits.time = [
            Some(Duration::from_millis(clock.white.time_ms)),
            Some(Duration::from_millis(clock.black.time_ms)),
        ];
        limits.increment = [
            Some(Duration::from_millis(clock.white.increment_ms)),
            Some(Duration::from_millis(clock.black.increment_ms)),
        ];
        limits.moves_to_go = clock.moves_to_go;
    }
    limits.infinite = wanted.infinite;
    if wanted.is_empty() {
        limits.movetime = Some(Duration::from_millis(DEFAULT_SEARCH_MS));
    }
    for text in &params.root_moves {
        let mv = position
            .parse_uci_move(text)
            .ok_or_else(|| illegal(format!("root move {text} isn't legal")))?;
        limits.search_moves.push(mv);
    }

    Ok(Prepared {
        position,
        history,
        limits,
        // At most 256, checked above.
        multipv: multipv as usize,
        level,
        elo,
        techniques,
        deterministic: params.deterministic,
        progress: params.progress_ms.map(Duration::from_millis),
        root: params.root_ms.map(Duration::from_millis),
        game: params.game.clone(),
        chess960: params.position.chess960,
        ponder: params.ponder,
    })
}

/// Runs one search, sends its progress and its answer, and gives the engine back.
fn search_thread(
    mut engine: Engine,
    prepared: Prepared,
    id: Id,
    start: Instant,
    flags: Flags,
    output: Output,
) -> Engine {
    let finished = Arc::new(AtomicBool::new(false));
    // ANCHOR: ponder
    // A ponder search searches with no time; its time counts from the hit, as long as the time
    // manager would give it then (`timeMs`, or its share of the clock). The tree it built while
    // pondering is kept: it is the same search.
    let (limits, after_hit) = if prepared.ponder {
        let side = prepared.position.side_to_move();
        let think = time::budget(&prepared.limits, side, &engine.time).soft;
        (prepared.limits.pondering(), think)
    } else {
        (prepared.limits.clone(), None)
    };
    // The node's maxTimeMs bounds every search, infinite and ponder ones included, from the
    // request; and a ponder search's time runs out `after_hit`.
    let watchdog = {
        let stop = Arc::clone(&flags.stop);
        let finished = Arc::clone(&finished);
        let hit = flags.hit.clone();
        thread::spawn(move || {
            let deadline = Duration::from_millis(MAX_TIME_MS);
            while !finished.load(Ordering::Relaxed) {
                let out_of_time = after_hit
                    .zip(hit.as_ref().and_then(|hit| hit.get()))
                    .is_some_and(|(think, at)| at.elapsed() >= think);
                if start.elapsed() >= deadline || out_of_time {
                    stop.store(true, Ordering::Relaxed);
                    return;
                }
                thread::sleep(WATCHDOG_POLL);
            }
        })
    };
    // ANCHOR_END: ponder
    let chess960 = prepared.chess960;
    // The latest report of each line, by its MultiPV number.
    let mut lines: BTreeMap<u32, SearchInfo> = BTreeMap::new();
    let mut last_progress: Option<Instant> = None;
    let mut last_root: Option<Instant> = None;
    let result = {
        let mut on_info = |info: &SearchInfo| {
            lines.insert(info.multipv.max(1), info.clone());
            if let Some(every) = prepared.progress
                && last_progress.is_none_or(|at| at.elapsed() >= every)
            {
                send(
                    &output,
                    progress(&id, info, prepared.multipv, chess960).into(),
                );
                last_progress = Some(Instant::now());
            }
        };
        // Following the root (spec 7.9): at most every `rootMs`, on an interval of its own, and
        // a research at once, the interval then counting from it.
        let mut on_root = |info: &RootInfo<'_>| {
            if let Some(every) = prepared.root
                && (info.research.is_some() || last_root.is_none_or(|at| at.elapsed() >= every))
            {
                send(&output, root(&id, info, prepared.multipv, chess960).into());
                last_root = Some(Instant::now());
            }
        };
        let following: &mut dyn FnMut(&RootInfo<'_>) = &mut on_root;
        engine.search_following_root(
            &prepared.position,
            &prepared.history,
            &limits,
            &flags.stop,
            &mut on_info,
            prepared.root.is_some().then_some(following),
        )
    };
    // An infinite search waits for `engine/stop` or a cancel (or the watchdog), and a ponder
    // search for its hit too, even when it has nothing left to search.
    while (prepared.limits.infinite || (prepared.ponder && flags.hit_at().is_none()))
        && !flags.stop.load(Ordering::Relaxed)
    {
        thread::sleep(WAIT_POLL);
    }
    finished.store(true, Ordering::Relaxed);
    let _ = watchdog.join();
    // At least one progress report before the result, for a search that completed a depth
    // (spec 7.2): the last one, if the interval kept it back.
    if prepared.progress.is_some()
        && last_progress.is_none()
        && let Some(info) = lines.get(&1)
    {
        send(
            &output,
            progress(&id, info, prepared.multipv, chess960).into(),
        );
    }
    // Checked last, so a cancel that arrived at any time is honoured.
    let response = if flags.cancelled.load(Ordering::Relaxed) {
        Response::error(Some(id), ResponseError::new(code::CANCELLED, "cancelled"))
    } else {
        let stopped = flags.stopped.load(Ordering::Relaxed);
        let elapsed = start.elapsed();
        // How long it pondered: until the hit, or all of it if none came.
        let pondered = prepared
            .ponder
            .then(|| millis(flags.hit_at().map_or(elapsed, |at| at - start)));
        let answer = answer(&result, &lines, &prepared, elapsed, pondered, stopped);
        Response::ok::<Search>(id, &answer)
    };
    // Before the answer goes: once the client has it, it may send the next search at once.
    flags.answered.store(true, Ordering::Release);
    send(&output, response.into());
    engine
}

/// A search's answer, from the engine's result and the lines it reported.
fn answer(
    result: &SearchResult,
    lines: &BTreeMap<u32, SearchInfo>,
    prepared: &Prepared,
    elapsed: Duration,
    ponder_ms: Option<u64>,
    stopped: bool,
) -> cnp::engine::SearchResult {
    let chess960 = prepared.chess960;
    // The line that starts with the move played: at a level it may not be the first.
    let best = result.best_move.and_then(|mv| {
        lines
            .values()
            .find(|info| info.pv.first() == Some(&mv))
            .or_else(|| lines.get(&1))
    });
    let pv = match (best, result.best_move) {
        (Some(info), Some(mv)) if info.pv.first() == Some(&mv) => moves(&info.pv, chess960),
        (_, Some(mv)) => vec![mv.to_uci(chess960)],
        (_, None) => Vec::new(),
    };
    cnp::engine::SearchResult {
        best_move: result.best_move.map(|mv| mv.to_uci(chess960)),
        ponder: result.ponder_move.map(|mv| mv.to_uci(chess960)),
        score: result.best_move.and(best).map(|info| score(info.score)),
        pv,
        depth: best.map_or(0, |info| info.depth),
        sel_depth: best.map_or(0, |info| info.seldepth),
        nodes: result.stats.nodes + result.stats.qnodes,
        time_ms: millis(elapsed),
        ponder_ms,
        stopped,
        lines: if prepared.multipv > 1 {
            lines
                .values()
                .take(prepared.multipv)
                .map(|info| Line {
                    multipv: info.multipv,
                    score: score(info.score),
                    pv: moves(&info.pv, chess960),
                    depth: info.depth,
                    sel_depth: info.seldepth,
                })
                .collect()
        } else {
            Vec::new()
        },
        stats: best.map(|info| SearchStats {
            hash_full: Some(info.hashfull),
            tb_hits: Some(info.tbhits),
        }),
    }
}

/// A progress notification from one report.
fn progress(id: &Id, info: &SearchInfo, multipv: usize, chess960: bool) -> Notification {
    let ms = millis(info.time);
    let params = Progress {
        id: id.clone(),
        depth: info.depth,
        sel_depth: info.seldepth,
        score: score(info.score),
        pv: moves(&info.pv, chess960),
        multipv: (multipv > 1).then_some(info.multipv),
        nodes: info.nodes,
        nps: Some(info.nodes.saturating_mul(1000) / ms.max(1)),
        time_ms: ms,
        hash_full: Some(info.hashfull),
        tb_hits: Some(info.tbhits),
    };
    Notification::new::<EngineProgress>(&params)
}

/// An `engine/root` notification from where the root loop is.
fn root(id: &Id, info: &RootInfo<'_>, multipv: usize, chess960: bool) -> Notification {
    let research = info.research.map(|research| match research {
        RootResearch::Better(mv) => cnp::engine::RootResearch::Better {
            mv: mv.to_uci(chess960),
        },
        RootResearch::FailHigh(at_least) => cnp::engine::RootResearch::FailHigh {
            score: bounded(at_least, Bound::Lower),
        },
        RootResearch::FailLow(at_most) => cnp::engine::RootResearch::FailLow {
            score: bounded(at_most, Bound::Upper),
        },
    });
    let params = RootParams {
        id: id.clone(),
        depth: info.depth,
        multipv: (multipv > 1).then_some(info.multipv),
        mv: info.current.to_uci(chess960),
        number: info.number,
        of: info.of,
        best: info.best.map(|mv| mv.to_uci(chess960)),
        moves: info
            .moves
            .iter()
            .map(|&(mv, nodes)| RootMove {
                mv: mv.to_uci(chess960),
                nodes,
            })
            .collect(),
        nodes: info.nodes,
        time_ms: millis(info.time),
        research,
    };
    Notification::new::<Root>(&params)
}

/// A request's position: its FEN with its moves played, and the hash of each position before
/// a move, for repetitions. `InvalidPosition` if the FEN or a move isn't legal.
fn read_position(wanted: &cnp::engine::Position) -> Result<(Position, Vec<u64>), ResponseError> {
    let illegal = |message: String| ResponseError::new(code::INVALID_POSITION, message);
    let mut position =
        Position::from_fen(&wanted.fen).map_err(|e| illegal(format!("invalid FEN: {e}")))?;
    let mut history = Vec::with_capacity(wanted.moves.len());
    for text in &wanted.moves {
        let mv = position
            .parse_uci_move(text)
            .ok_or_else(|| illegal(format!("{text} isn't a legal move in its position")))?;
        history.push(position.hash());
        position.make_move(mv);
    }
    Ok((position, history))
}

/// `engine/evaluate`: the position's evaluation, term by term, each term blended for the
/// position's phase as [`eval::trace`] blends the total (the shell's `eval` shows the same).
/// With `quiet`, of the position at the end of the quiescence search's line; the search's own
/// score when it evaluates with the network; and with `pieces`, each piece's value to it.
fn evaluate(
    params: &EvaluateParams,
    evaluator: &mut Engine,
) -> Result<EvaluateResult, ResponseError> {
    let (mut position, history) = read_position(&params.position)?;
    let quiet = params.quiet.then(|| {
        // From a clean slate, so the answer doesn't depend on earlier requests.
        evaluator.new_game();
        let line = evaluator.quiet_line(&position, &history).moves;
        for &mv in &line {
            position.make_move(mv);
        }
        moves(&line, params.position.chess960)
    });
    let mated = position.in_check() && position.legal_moves().is_empty();
    let eval_params = &evaluator.params;
    let trace = eval::trace(&position, eval_params);
    let blend = |(mg, eg): (i32, i32)| {
        if eval_params.tapered {
            (mg * trace.phase + eg * (MAX_PHASE - trace.phase)) / MAX_PHASE
        } else {
            mg
        }
    };
    let terms = trace
        .terms()
        .into_iter()
        .zip(TERMS)
        .map(|((name, [white, black]), (id, description))| Term {
            id: id.to_string(),
            name: name.to_string(),
            description: Some(description.to_string()),
            white: blend(white),
            black: blend(black),
        })
        .collect();
    // Only `quiet` promises a mate (spec 7.6); a plain evaluation stays static, as before.
    let score = if quiet.is_some() && mated {
        cnp::engine::Score::mate(0)
    } else {
        cnp::engine::Score::cp(eval::evaluate(&position, eval_params))
    };
    let search = evaluator.search_evaluation(&position);
    let search_score = (search.network && !mated).then(|| cnp::engine::Score::cp(search.score));
    let pieces = (params.pieces && !mated).then(|| {
        evaluator
            .piece_values(&position)
            .into_iter()
            .map(|piece| PieceValue {
                square: piece.square.to_string(),
                value: piece.value,
            })
            .collect()
    });
    Ok(EvaluateResult {
        score,
        terms,
        quiet,
        search_score,
        pieces,
    })
}

/// The levels and their measured ratings.
fn levels() -> LevelsResult {
    LevelsResult {
        levels: (0..=FULL_STRENGTH)
            .zip(RATINGS)
            .map(|(level, rating)| Level { level, rating })
            .collect(),
        rating_scale: RATING_SCALE.to_string(),
        fractional: false,
        elo: Some(EloRange {
            min: ELO_RANGE.0,
            max: ELO_RANGE.1,
        }),
    }
}

fn score(score: Score) -> cnp::engine::Score {
    match score {
        Score::Centipawns(cp) => cnp::engine::Score::cp(cp),
        Score::Mate(moves) => cnp::engine::Score::mate(moves),
    }
}

/// A score that is only a bound, `bound`.
fn bounded(value: Score, bound: Bound) -> cnp::engine::Score {
    cnp::engine::Score {
        bound,
        ..score(value)
    }
}

fn moves(line: &[Move], chess960: bool) -> Vec<String> {
    line.iter().map(|mv| mv.to_uci(chess960)).collect()
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// A switch's name for people: its description up to any note (`Alpha-beta pruning (off: plain
/// minimax)` is `Alpha-beta pruning`).
fn short_name(description: &str) -> &str {
    let end = description.find([':', '(']).unwrap_or(description.len());
    description[..end].trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cb2026_engine::time::TimeParams;
    use cnp::engine::{
        Clock, PonderHitParams, Position as CnpPosition, SearchLimits, SideClock, StopParams,
        Strength, TreeNodeParams, TreeParams, Window,
    };

    fn search(fen: &str) -> SearchParams {
        SearchParams::new(CnpPosition::fen(fen))
    }

    fn start() -> SearchParams {
        SearchParams::new(CnpPosition::start())
    }

    fn code_of(params: &SearchParams) -> i64 {
        match prepare(params) {
            Ok(_) => 0,
            Err(e) => e.code,
        }
    }

    #[test]
    fn a_search_with_no_limits_gets_the_default_time() {
        let prepared = prepare(&start()).unwrap();
        assert_eq!(
            prepared.limits.movetime,
            Some(Duration::from_millis(DEFAULT_SEARCH_MS))
        );
    }

    #[test]
    fn a_clock_becomes_each_side_s_time_and_increment() {
        let mut params = start();
        params.limits = SearchLimits {
            clock: Some(Clock {
                white: SideClock {
                    time_ms: 60_000,
                    increment_ms: 1_000,
                },
                black: SideClock {
                    time_ms: 50_000,
                    increment_ms: 0,
                },
                moves_to_go: Some(20),
            }),
            ..SearchLimits::default()
        };
        let limits = prepare(&params).unwrap().limits;
        assert_eq!(
            limits.time,
            [Some(Duration::from_secs(60)), Some(Duration::from_secs(50))]
        );
        assert_eq!(limits.increment[0], Some(Duration::from_secs(1)));
        assert_eq!(limits.moves_to_go, Some(20));
        assert_eq!(limits.movetime, None);
    }

    #[test]
    fn moves_are_played_and_remembered_for_repetitions() {
        let mut params = start();
        params.position = CnpPosition::start().with_moves(["g1f3", "g8f6", "f3g1", "f6g8"]);
        let prepared = prepare(&params).unwrap();
        assert_eq!(prepared.history.len(), 4);
        assert_eq!(prepared.position.hash(), Position::startpos().hash());
    }

    #[test]
    fn bad_requests_get_the_spec_s_errors() {
        assert_eq!(code_of(&search("not a fen")), code::INVALID_POSITION);
        let mut illegal = start();
        illegal.position = CnpPosition::start().with_moves(["e2e5"]);
        assert_eq!(code_of(&illegal), code::INVALID_POSITION);
        let mut root = start();
        root.root_moves = vec!["e2e5".to_string()];
        assert_eq!(code_of(&root), code::INVALID_POSITION);
        let mut deep = start();
        deep.limits = SearchLimits::depth(MAX_PLY + 1);
        assert_eq!(code_of(&deep), code::LIMIT_EXCEEDED);
        let mut lines = start();
        lines.multipv = Some(MAX_MULTIPV + 1);
        assert_eq!(code_of(&lines), code::LIMIT_EXCEEDED);
        let mut window = start();
        window.window = Some(Window {
            alpha: -10,
            beta: 10,
        });
        assert_eq!(code_of(&window), code::UNSUPPORTED);
        let mut technique = start();
        technique.techniques.insert("noSuchThing".to_string(), true);
        assert_eq!(code_of(&technique), code::UNSUPPORTED);
    }

    #[test]
    fn strength_is_a_whole_level_or_a_rating_in_range() {
        let with = |level: Option<f64>, elo: Option<u32>| {
            let mut params = start();
            params.strength = Some(Strength { level, elo });
            params
        };
        assert_eq!(prepare(&with(Some(7.0), None)).unwrap().level, 7);
        assert_eq!(prepare(&with(None, Some(1500))).unwrap().elo, Some(1500));
        for bad in [
            with(Some(7.5), None),
            with(Some(21.0), None),
            with(Some(-1.0), None),
            with(None, Some(ELO_RANGE.1 + 1)),
            with(Some(3.0), Some(1500)),
        ] {
            assert_eq!(code_of(&bad), code::INVALID_PARAMS, "{:?}", bad.strength);
        }
        let mut random = with(Some(3.0), None);
        random.deterministic = true;
        assert_eq!(code_of(&random), code::UNSUPPORTED);
    }

    #[test]
    fn techniques_are_the_shell_s_switches_with_their_defaults() {
        let node = Node::new(
            Arc::new(Mutex::new(Box::new(io::sink()))),
            Engine::new(),
            Config {
                license: "MIT OR Apache-2.0",
            },
        );
        let listed = node.techniques().techniques;
        assert_eq!(listed.len(), SWITCHES.len());
        let alpha_beta = listed.iter().find(|t| t.id == "alphabeta").unwrap();
        assert_eq!(alpha_beta.name, "Alpha-beta pruning");
        assert!(alpha_beta.default);
        let mut params = start();
        params.techniques.insert("nullmove".to_string(), false);
        assert_eq!(prepare(&params).unwrap().techniques.len(), 1);
    }

    #[test]
    fn levels_run_from_0_to_full_strength_with_their_ratings() {
        let levels = levels();
        assert_eq!(levels.levels.len() as u32, FULL_STRENGTH + 1);
        assert_eq!(levels.levels[0].rating, RATINGS[0]);
        assert!(!levels.fractional);
    }

    #[test]
    fn an_evaluation_is_the_shell_s_eval_term_by_term() {
        // The Two Knights, as the spec's example has it: the shell's `eval` shows the same.
        let fen = "r1bqkb1r/pppp1ppp/2n2n2/4p3/2B1P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 4 4";
        let params = EvaluateParams::new(CnpPosition::fen(fen));
        let result = evaluate(&params, &mut Engine::new()).unwrap();
        assert_eq!(result.score, cnp::engine::Score::cp(-1));
        let ids: Vec<&str> = result.terms.iter().map(|t| t.id.as_str()).collect();
        assert_eq!(ids, TERMS.map(|(id, _)| id));
        let term = |id: &str| {
            let term = result.terms.iter().find(|t| t.id == id).unwrap();
            (term.name.as_str(), term.white, term.black)
        };
        assert_eq!(term("material"), ("Material", 3940, 3940));
        assert_eq!(term("mobility"), ("Mobility", -69, -72));
        assert_eq!(term("tempo"), ("Tempo", 10, 0));
        // The side to move's terms less the other's are the score, give or take rounding.
        let net: i32 = result.terms.iter().map(|t| t.white - t.black).sum();
        assert!((net - (-1)).abs() <= TERMS.len() as i32, "{net}");
    }

    #[test]
    fn an_evaluation_of_an_illegal_position_is_refused() {
        let params = EvaluateParams::new;
        let illegal = params(CnpPosition::start().with_moves(["e2e5"]));
        let bad_fen = params(CnpPosition::fen("not a fen"));
        for wanted in [illegal, bad_fen] {
            let error = evaluate(&wanted, &mut Engine::new()).unwrap_err();
            assert_eq!(error.code, code::INVALID_POSITION);
        }
        // From Black's side, the score is Black's.
        let after_e4 = params(CnpPosition::start().with_moves(["e2e4"]));
        let result = evaluate(&after_e4, &mut Engine::new()).unwrap();
        let net: i32 = result.terms.iter().map(|t| t.black - t.white).sum();
        let cnp::engine::ScoreValue::Centipawns(cp) = result.score.value else {
            panic!("an evaluation is in centipawns");
        };
        assert!((net - cp).abs() <= TERMS.len() as i32, "{net} against {cp}");
    }

    #[test]
    fn an_evaluation_settles_captures_and_values_pieces_by_the_network() {
        // The spec's example: after 1.e4 d5 2.exd5, Black's queen takes back.
        let mut evaluator = Engine::new();
        let mut params =
            EvaluateParams::new(CnpPosition::start().with_moves(["e2e4", "d7d5", "e4d5"]));
        params.quiet = true;
        params.pieces = true;
        let result = evaluate(&params, &mut evaluator).unwrap();
        assert_eq!(result.quiet, Some(vec!["d8d5".to_string()]));
        // The hand-crafted evaluation and its terms, of the position after 2...Qxd5.
        assert_eq!(result.score, cnp::engine::Score::cp(-17));
        let term = |id: &str| {
            let term = result.terms.iter().find(|t| t.id == id).unwrap();
            (term.white, term.black)
        };
        assert_eq!(term("material"), (3850, 3850));
        assert_eq!(term("mobility"), (-89, -75));
        // The network the search evaluates with sees it otherwise.
        assert_eq!(result.search_score, Some(cnp::engine::Score::cp(34)));
        let pieces = result.pieces.as_ref().unwrap();
        assert_eq!(pieces.len(), 28);
        let value = |square: &str| pieces.iter().find(|p| p.square == square).unwrap().value;
        assert_eq!((value("d1"), value("d5"), value("f1")), (918, 912, 338));
        assert!(pieces.iter().all(|p| p.square != "e1" && p.square != "e8"));
        // The same request gives the same answer, whatever came before.
        let quiet_only = EvaluateParams {
            pieces: false,
            ..params.clone()
        };
        let first = evaluate(&quiet_only, &mut evaluator).unwrap();
        assert_eq!(first.pieces, None);
        assert_eq!(evaluate(&params, &mut evaluator).unwrap(), result);
        // Not asked to settle: the position as it stands, a pawn down for Black, and no line.
        let as_it_stands = EvaluateParams::new(params.position.clone());
        let unsettled = evaluate(&as_it_stands, &mut evaluator).unwrap();
        assert_eq!(unsettled.quiet, None);
        assert!(unsettled.search_score.is_some());
    }

    #[test]
    fn an_evaluation_settled_into_mate_is_mate_in_0_with_no_pieces() {
        let mut params =
            EvaluateParams::new(CnpPosition::start().with_moves(["f2f3", "e7e5", "g2g4", "d8h4"]));
        params.quiet = true;
        params.pieces = true;
        let result = evaluate(&params, &mut Engine::new()).unwrap();
        assert_eq!(result.quiet, Some(vec![]));
        assert_eq!(result.score, cnp::engine::Score::mate(0));
        assert_eq!((result.search_score, result.pieces), (None, None));
        // Without `quiet` the score stays static, as it always was.
        params.quiet = false;
        let plain = evaluate(&params, &mut Engine::new()).unwrap();
        assert!(matches!(
            plain.score.value,
            cnp::engine::ScoreValue::Centipawns(_)
        ));
    }

    #[test]
    fn a_search_that_has_answered_doesn_t_make_the_next_busy() {
        let mut node = Node::new(
            Arc::new(Mutex::new(Box::new(io::sink()))),
            Engine::new(),
            Config {
                license: "MIT OR Apache-2.0",
            },
        );
        // A search that has sent its answer and is still giving the engine back.
        let ending = Duration::from_millis(50);
        node.engine = None;
        node.search = Some(Running {
            id: Id::Number(1),
            stop: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(true)),
            hit: None,
            handle: thread::spawn(move || {
                thread::sleep(ending);
                Engine::new()
            }),
        });
        let mut params = start();
        params.limits = SearchLimits::depth(1);
        let request = Request::new::<Search>(2, &params);
        assert!(node.start_search(&request).is_ok(), "refused as busy");
        node.finish();
        // One that hasn't answered still is busy.
        let (release, wait) = std::sync::mpsc::channel::<()>();
        node.search = Some(Running {
            id: Id::Number(3),
            stop: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(false)),
            hit: None,
            handle: thread::spawn(move || {
                let _ = wait.recv();
                Engine::new()
            }),
        });
        let engine = node.engine.take();
        let busy = node.start_search(&Request::new::<Search>(4, &params));
        assert_eq!(busy.unwrap_err().code, code::BUSY);
        release.send(()).unwrap();
        node.engine = engine;
        node.finish();
    }

    /// Output that a test can read back.
    #[derive(Clone, Default)]
    struct Capture(Arc<Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl Capture {
        /// The response to request `id`, if it has been sent.
        fn response(&self, id: i64) -> Option<Response> {
            let bytes = self.0.lock().unwrap().clone();
            let mut input = io::Cursor::new(bytes);
            while let Ok(Some(message)) = read_message(&mut input, usize::MAX) {
                if let Message::Response(response) = message
                    && response.id == Some(Id::Number(id))
                {
                    return Some(response);
                }
            }
            None
        }

        /// Every message sent so far, in order.
        fn messages(&self) -> Vec<Message> {
            let bytes = self.0.lock().unwrap().clone();
            let mut input = io::Cursor::new(bytes);
            let mut messages = Vec::new();
            while let Ok(Some(message)) = read_message(&mut input, usize::MAX) {
                messages.push(message);
            }
            messages
        }

        /// Waits up to `timeout` for the response to request `id`.
        fn wait_for(&self, id: i64, timeout: Duration) -> Option<Response> {
            let start = Instant::now();
            while start.elapsed() < timeout {
                if let Some(response) = self.response(id) {
                    return Some(response);
                }
                thread::sleep(Duration::from_millis(5));
            }
            None
        }
    }

    /// A node past `initialize`, writing to `capture`.
    fn running_node(capture: &Capture) -> Node {
        let mut node = Node::new(
            Arc::new(Mutex::new(Box::new(capture.clone()))),
            Engine::new(),
            Config {
                license: "MIT OR Apache-2.0",
            },
        );
        node.state = State::Running;
        node
    }

    fn ponder_search(limits: SearchLimits) -> SearchParams {
        let mut params = start();
        params.position = CnpPosition::start().with_moves(["e2e4", "e7e5"]);
        params.limits = limits;
        params.ponder = true;
        params
    }

    #[test]
    fn a_ponder_search_s_time_counts_from_its_hit() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let think = 300;
        let params = ponder_search(SearchLimits {
            time_ms: Some(think),
            ..SearchLimits::default()
        });
        node.request(Request::new::<Search>(1, &params));
        // Well past its time, and no answer: its time hasn't started.
        thread::sleep(Duration::from_millis(2 * think));
        assert!(capture.response(1).is_none(), "answered before its hit");
        let hit = Instant::now();
        node.request(Request::new::<PonderHit>(
            2,
            &PonderHitParams { id: Id::Number(1) },
        ));
        assert_eq!(capture.response(2).unwrap().result::<PonderHit>(), Ok(()));
        let answer = capture
            .wait_for(1, Duration::from_secs(10))
            .expect("an answer after the hit");
        let after_hit = hit.elapsed();
        let result = answer.result::<Search>().unwrap();
        assert!(result.best_move.is_some());
        assert!(!result.stopped);
        let pondered = result
            .ponder_ms
            .expect("a ponder search says how long it pondered");
        assert!(pondered >= 2 * think, "pondered {pondered} ms");
        // Its timeMs from the hit, less the move overhead, give or take the watchdog's poll.
        let overhead = millis(TimeParams::default().move_overhead);
        assert!(
            after_hit >= Duration::from_millis(think - overhead),
            "answered {after_hit:?} after the hit"
        );
        assert!(result.time_ms >= pondered + think - overhead, "{result:?}");
        node.finish();
    }

    #[test]
    fn a_ponder_search_stopped_before_its_hit_pondered_all_its_time() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let params = ponder_search(SearchLimits {
            clock: Some(Clock {
                white: SideClock {
                    time_ms: 60_000,
                    increment_ms: 0,
                },
                black: SideClock {
                    time_ms: 60_000,
                    increment_ms: 0,
                },
                moves_to_go: None,
            }),
            ..SearchLimits::default()
        });
        node.request(Request::new::<Search>(1, &params));
        thread::sleep(Duration::from_millis(100));
        node.request(Request::new::<Stop>(2, &StopParams { id: Id::Number(1) }));
        let result = capture
            .wait_for(1, Duration::from_secs(10))
            .expect("an answer after the stop")
            .result::<Search>()
            .unwrap();
        assert!(result.stopped);
        assert_eq!(result.ponder_ms, Some(result.time_ms));
        // A second hit, for a search that has answered, does nothing.
        node.request(Request::new::<PonderHit>(
            3,
            &PonderHitParams { id: Id::Number(1) },
        ));
        assert_eq!(capture.response(3).unwrap().result::<PonderHit>(), Ok(()));
        node.finish();
    }

    /// A tree request of the start position after `moves`.
    fn tree_of(moves: &[&str], depth: u32, plies: u32) -> TreeParams {
        TreeParams {
            plies: Some(plies),
            ..TreeParams::new(
                CnpPosition::start().with_moves(moves.iter().copied()),
                depth,
            )
        }
    }

    /// The error code of request `id`'s response.
    fn error_code(capture: &Capture, id: i64) -> i64 {
        capture.response(id).unwrap().outcome.unwrap_err().code
    }

    #[test]
    fn a_tree_is_answered_while_a_search_runs_and_read_a_node_at_a_time() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let mut infinite = start();
        infinite.limits = SearchLimits::infinite();
        node.request(Request::new::<Search>(1, &infinite));
        let params = TreeParams {
            limit: Some(3),
            ..tree_of(&["e2e4", "e7e5"], 6, 2)
        };
        node.request(Request::new::<Tree>(2, &params));
        let result = capture
            .wait_for(2, Duration::from_secs(60))
            .expect("the tree answers while the search runs")
            .result::<Tree>()
            .unwrap();
        assert!(capture.response(1).is_none(), "the search still runs");
        assert_eq!((result.depth, result.recorded.plies), (6, 2));
        assert!(!result.recorded.truncated);
        assert_eq!(result.pv.first(), result.best_move.as_ref());
        // The root: every legal move entered, and the result's score, exact.
        assert_eq!(result.root.mv, None);
        assert_eq!(result.root.child_count, 29);
        assert_eq!(result.root.score, result.score);
        assert_eq!(result.root.score.unwrap().bound, cnp::engine::Bound::Exact);
        assert_eq!(result.recorded.nodes, result.root.subtree + 1);
        assert_eq!(result.children.len(), 3);
        // A child's window is from its own side to move, below its parent's turned round.
        let first = &result.children[0];
        assert_eq!(first.depth, 5);
        assert!(first.score.is_some());

        // The first child, read again, with a page of its own children.
        let mv = first.mv.clone().unwrap();
        let page = TreeNodeParams {
            offset: Some(1),
            limit: Some(2),
            ..TreeNodeParams::new(result.tree.clone(), [mv.clone()])
        };
        node.request(Request::new::<TreeNodePage>(3, &page));
        let read = capture
            .response(3)
            .unwrap()
            .result::<TreeNodePage>()
            .unwrap();
        assert_eq!(&read.node, first);
        let all = TreeNodeParams::new(result.tree.clone(), [mv.clone()]);
        node.request(Request::new::<TreeNodePage>(4, &all));
        let every = capture
            .response(4)
            .unwrap()
            .result::<TreeNodePage>()
            .unwrap();
        assert_eq!(every.children.len() as u32, first.child_count);
        let expected: Vec<_> = every.children.iter().skip(1).take(2).cloned().collect();
        assert_eq!(read.children, expected);
        // Past the last child, an empty page; a limit of 0, the node alone.
        let past = TreeNodeParams {
            offset: Some(first.child_count),
            ..all.clone()
        };
        node.request(Request::new::<TreeNodePage>(5, &past));
        let past = capture
            .response(5)
            .unwrap()
            .result::<TreeNodePage>()
            .unwrap();
        assert!(past.children.is_empty());

        // A path the tree doesn't hold, and a handle it never gave.
        let nowhere = TreeNodeParams::new(result.tree.clone(), [mv.as_str(), "e1e8"]);
        node.request(Request::new::<TreeNodePage>(6, &nowhere));
        assert_eq!(error_code(&capture, 6), code::INVALID_PARAMS);
        node.request(Request::new::<TreeNodePage>(
            7,
            &TreeNodeParams::new("t99", [""; 0]),
        ));
        assert_eq!(error_code(&capture, 7), code::EXPIRED);

        // The same request records the same tree: its own engine, cleared, on one thread.
        node.request(Request::new::<Tree>(8, &params));
        let again = capture
            .wait_for(8, Duration::from_secs(60))
            .unwrap()
            .result::<Tree>()
            .unwrap();
        assert_ne!(again.tree, result.tree);
        assert_eq!(
            (again.root, again.children, again.nodes, again.pv),
            (result.root, result.children, result.nodes, result.pv)
        );
        node.request(Request::new::<Stop>(9, &StopParams { id: Id::Number(1) }));
        assert!(capture.wait_for(1, Duration::from_secs(60)).is_some());
        node.finish();
    }

    #[test]
    fn a_cancelled_tree_answers_cancelled_and_the_next_isn_t_busy() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        // Deep enough to be running when the cancel comes, and the next while it runs.
        let deep = tree_of(&[], MAX_TREE_DEPTH, 2);
        node.request(Request::new::<Tree>(1, &deep));
        node.request(Request::new::<Tree>(2, &deep));
        assert_eq!(error_code(&capture, 2), code::BUSY);
        node.notification(&Notification::new::<CancelRequest>(
            &cnp::lifecycle::CancelParams { id: Id::Number(1) },
        ));
        node.request(Request::new::<Tree>(3, &tree_of(&[], 2, 1)));
        assert_eq!(error_code(&capture, 1), code::CANCELLED);
        let next = capture.wait_for(3, Duration::from_secs(60)).unwrap();
        assert!(next.result::<Tree>().is_ok(), "{next:?}");
        node.finish();
        // `shutdown` forgets the trees.
        node.request(Request::new::<TreeNodePage>(
            4,
            &TreeNodeParams::new("t1", [""; 0]),
        ));
        assert_eq!(error_code(&capture, 4), code::EXPIRED);
    }

    #[test]
    fn a_tree_beyond_the_limits_or_of_an_illegal_position_is_refused() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let cases = [
            (tree_of(&[], 0, 2), code::INVALID_PARAMS),
            (tree_of(&[], 4, 0), code::INVALID_PARAMS),
            (tree_of(&[], MAX_TREE_DEPTH + 1, 2), code::LIMIT_EXCEEDED),
            (tree_of(&[], 4, MAX_TREE_PLIES + 1), code::LIMIT_EXCEEDED),
            (tree_of(&["e2e5"], 4, 2), code::INVALID_POSITION),
        ];
        for (id, (params, expected)) in (1..).zip(cases) {
            node.request(Request::new::<Tree>(id, &params));
            assert_eq!(error_code(&capture, id), expected, "{params:?}");
        }
    }

    #[test]
    fn a_tree_of_a_position_with_no_legal_move_is_its_root_alone() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let mated = tree_of(&["f2f3", "e7e5", "g2g4", "d8h4"], 8, 2);
        node.request(Request::new::<Tree>(1, &mated));
        let result = capture
            .wait_for(1, Duration::from_secs(60))
            .unwrap()
            .result::<Tree>()
            .unwrap();
        assert_eq!(
            (result.best_move, result.score, result.depth),
            (None, None, 0)
        );
        assert_eq!((result.root.child_count, result.recorded.nodes), (0, 1));
        assert!(result.children.is_empty());
        node.finish();
    }

    /// The `engine/root` notifications among `messages`, each with its place.
    fn roots(messages: &[Message]) -> Vec<(usize, RootParams)> {
        messages
            .iter()
            .enumerate()
            .filter_map(|(at, message)| match message {
                Message::Notification(notification) if notification.is::<Root>() => {
                    Some((at, notification.params::<Root>().unwrap()))
                }
                _ => None,
            })
            .collect()
    }

    #[test]
    fn following_the_root_is_throttled_and_never_comes_after_the_answer() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let every = 50;
        let mut params = start().root_ms(every);
        params.limits = SearchLimits {
            time_ms: Some(600),
            ..SearchLimits::default()
        };
        params.multipv = Some(2);
        node.request(Request::new::<Search>(1, &params));
        let answer = capture
            .wait_for(1, Duration::from_secs(10))
            .expect("an answer")
            .result::<Search>()
            .unwrap();
        // A moment for anything late to show.
        thread::sleep(Duration::from_millis(100));
        let messages = capture.messages();
        let answered = messages
            .iter()
            .position(|m| matches!(m, Message::Response(r) if r.id == Some(Id::Number(1))))
            .unwrap();
        let roots = roots(&messages);
        assert!(!roots.is_empty(), "no engine/root");
        assert!(
            roots.iter().all(|(at, _)| *at < answered),
            "after the answer"
        );
        // The first, before there is a best move: depth 1's first move.
        let first = &roots[0].1;
        assert_eq!(
            (first.depth, first.number, first.best.as_ref()),
            (1, 1, None)
        );
        let mut previous: Option<u64> = None;
        for (_, root) in &roots {
            assert_eq!(root.id, Id::Number(1));
            assert!(matches!(root.multipv, Some(1 | 2)), "{root:?}");
            assert!((1..=root.of).contains(&root.number), "{root:?}");
            assert!(root.moves.iter().any(|m| m.mv == root.mv), "{root:?}");
            let below: u64 = root.moves.iter().map(|m| m.nodes).sum();
            assert!(below <= root.nodes, "{root:?}");
            match &root.research {
                // At most every `rootMs`, counted from the last of any kind (give or take the
                // millisecond the report takes to reach the throttle).
                None => {
                    if let Some(at) = previous {
                        assert!(root.time_ms + 1 >= at + every, "{at} then {root:?}");
                    }
                }
                Some(cnp::engine::RootResearch::Better { mv }) => assert_eq!(mv, &root.mv),
                Some(cnp::engine::RootResearch::FailHigh { score }) => {
                    assert_eq!(score.bound, Bound::Lower);
                }
                Some(cnp::engine::RootResearch::FailLow { score }) => {
                    assert_eq!(score.bound, Bound::Upper);
                }
                Some(cnp::engine::RootResearch::Unknown) => panic!("{root:?}"),
            }
            previous = Some(root.time_ms);
        }
        // The move played was among those followed.
        let best = answer.best_move.unwrap();
        assert!(roots.last().unwrap().1.moves.iter().any(|m| m.mv == best));
        node.finish();
    }

    #[test]
    fn a_search_that_doesn_t_ask_isn_t_followed() {
        let capture = Capture::default();
        let mut node = running_node(&capture);
        let mut params = start();
        params.limits = SearchLimits::depth(6);
        params.progress_ms = Some(0);
        node.request(Request::new::<Search>(1, &params));
        assert!(capture.wait_for(1, Duration::from_secs(10)).is_some());
        let messages = capture.messages();
        assert!(roots(&messages).is_empty());
        // Its progress still comes.
        assert!(messages.iter().any(|m| matches!(
            m,
            Message::Notification(n) if n.is::<EngineProgress>()
        )));
        node.finish();
    }

    #[test]
    fn a_switch_s_name_is_its_description_without_the_note() {
        assert_eq!(
            short_name("Alpha-beta pruning (off: plain minimax)"),
            "Alpha-beta pruning"
        );
        assert_eq!(
            short_name("Capture history: captures ordered by what they win"),
            "Capture history"
        );
        assert_eq!(short_name("Null-move pruning"), "Null-move pruning");
    }
}
