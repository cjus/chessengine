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
//!
//! ## What it offers
//!
//! Every engine capability except `window`: cb2026's search always starts with an open window
//! at the root. Its techniques are the shell's switches ([`cb2026_shell::SWITCHES`]), by the
//! names the shell's `set` uses. Its levels are [`RATINGS`], whole numbers only, since a level's
//! settings are whole steps. A rating within [`ELO_RANGE`] picks a level as `UCI_Elo` does.
//! `engine/evaluate` answers with the shell's `eval`: [`eval::trace`]'s eight terms ([`TERMS`]),
//! each blended for the position's phase as the evaluation blends its total.
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
//! - **Castling notation follows the position.** cb2026 reads both notations. It writes the
//!   king's two-square move, or king-takes-rook when the position says `chess960`.

#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use cb2026_chess::{Move, Position};
use cb2026_engine::eval::{self, EvalParams, MAX_PHASE};
use cb2026_engine::levels::{ELO_RANGE, FULL_STRENGTH, RATINGS};
use cb2026_engine::search::MAX_PLY;
use cb2026_engine::tablebase::TablebaseLoader;
use cb2026_engine::{AUTHOR, Engine, Limits, NAME, Score, SearchInfo, SearchResult, build_version};
use cb2026_shell::SWITCHES;
use cnp::engine::{
    EloRange, EngineCapabilities, EngineProgress, Evaluate, EvaluateParams, EvaluateResult, Level,
    Levels, LevelsResult, Line, Progress, Search, SearchParams, SearchStats, Stop, Technique,
    Techniques, TechniquesResult, Term,
};
use cnp::lifecycle::{
    CancelRequest, Capabilities, IncompatibleVersionData, Initialize, InitializeResult,
    Limits as NodeLimits, NodeInfo, PROTOCOL_VERSION, Ping, Shutdown, negotiate,
};
use cnp::stdio::{ReadError, read_message, write_message};
use cnp::{Id, Message, Notification, Request, RequestMethod, Response, ResponseError, code};

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
    /// The thread, which gives the engine back when it ends.
    handle: JoinHandle<Engine>,
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
    /// The evaluation's parameters as the engine came configured, for `engine/evaluate` while a
    /// search owns the engine.
    eval_params: EvalParams,
    /// The running search, if any.
    search: Option<Running>,
    /// The `game` hint of the last search, whose hash table the engine still holds.
    last_game: Option<String>,
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
    game: Option<String>,
    chess960: bool,
}

impl Node {
    fn new(output: Output, engine: Engine, config: Config) -> Node {
        let mut fresh = Engine::new();
        let defaults = SWITCHES.iter().map(|s| *(s.flag)(&mut fresh)).collect();
        Node {
            output,
            config,
            state: State::Starting,
            tablebase_loader: engine.tablebase_loader,
            threads: engine.threads,
            eval_params: engine.params.clone(),
            engine: Some(engine),
            defaults,
            search: None,
            last_game: None,
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
            (State::Running, Techniques::NAME) => {
                Response::ok::<Techniques>(id, &self.techniques())
            }
            (State::Running, Levels::NAME) => Response::ok::<Levels>(id, &levels()),
            (State::Running, Evaluate::NAME) => match request
                .params::<Evaluate>()
                .and_then(|params| evaluate(&params, &self.eval_params))
            {
                Ok(result) => Response::ok::<Evaluate>(id, &result),
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
                }),
                voice: None,
            },
            limits: NodeLimits {
                max_depth: Some(MAX_PLY),
                max_nodes: None,
                max_time_ms: Some(MAX_TIME_MS),
                max_multipv: Some(MAX_MULTIPV),
                max_concurrent_searches: Some(1),
                max_message_bytes: None,
            },
        };
        Response::ok::<Initialize>(id, &result)
    }

    /// Checks a search and starts it on its own thread, or says why not.
    fn start_search(&mut self, request: &Request) -> Result<(), ResponseError> {
        let params = request.params::<Search>()?;
        let prepared = prepare(&params)?;
        if let Some(running) = self.search.take() {
            if running.handle.is_finished()
                || running.answered.load(Ordering::Acquire)
                || running.stop.load(Ordering::Relaxed)
            {
                // Finished, answered and ending, or winding down after a stop or a cancel: a
                // few milliseconds at most.
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
        let flags = Flags {
            stop: Arc::new(AtomicBool::new(false)),
            stopped: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            answered: Arc::new(AtomicBool::new(false)),
        };
        let id = request.id.clone();
        let running = Running {
            id: id.clone(),
            stop: Arc::clone(&flags.stop),
            stopped: Arc::clone(&flags.stopped),
            cancelled: Arc::clone(&flags.cancelled),
            answered: Arc::clone(&flags.answered),
            handle: {
                let output = Arc::clone(&self.output);
                thread::spawn(move || search_thread(engine, prepared, id, flags, output))
            },
        };
        self.search = Some(running);
        Ok(())
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

    /// Cancels the running search, if any, and waits for it to answer.
    fn finish(&mut self) {
        if let Some(running) = self.search.take() {
            running.cancelled.store(true, Ordering::Relaxed);
            running.stop.store(true, Ordering::Relaxed);
            self.reclaim(running);
        }
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
        game: params.game.clone(),
        chess960: params.position.chess960,
    })
}

/// Runs one search, sends its progress and its answer, and gives the engine back.
fn search_thread(
    mut engine: Engine,
    prepared: Prepared,
    id: Id,
    flags: Flags,
    output: Output,
) -> Engine {
    let start = Instant::now();
    let finished = Arc::new(AtomicBool::new(false));
    // The node's maxTimeMs bounds every search, infinite ones included.
    let watchdog = {
        let stop = Arc::clone(&flags.stop);
        let finished = Arc::clone(&finished);
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
    let chess960 = prepared.chess960;
    // The latest report of each line, by its MultiPV number.
    let mut lines: BTreeMap<u32, SearchInfo> = BTreeMap::new();
    let mut last_progress: Option<Instant> = None;
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
        engine.search(
            &prepared.position,
            &prepared.history,
            &prepared.limits,
            &flags.stop,
            &mut on_info,
        )
    };
    // An infinite search waits for `engine/stop` or a cancel (or the watchdog).
    while prepared.limits.infinite && !flags.stop.load(Ordering::Relaxed) {
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
        let answer = answer(&result, &lines, &prepared, start.elapsed(), stopped);
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
fn evaluate(
    params: &EvaluateParams,
    eval_params: &EvalParams,
) -> Result<EvaluateResult, ResponseError> {
    let (position, _) = read_position(&params.position)?;
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
    Ok(EvaluateResult {
        score: cnp::engine::Score::cp(eval::evaluate(&position, eval_params)),
        terms,
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
    use cnp::engine::{Clock, Position as CnpPosition, SearchLimits, SideClock, Strength, Window};

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
        let params = EvaluateParams {
            position: CnpPosition::fen(fen),
        };
        let result = evaluate(&params, &EvalParams::default()).unwrap();
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
        let params = |position| EvaluateParams { position };
        let illegal = params(CnpPosition::start().with_moves(["e2e5"]));
        let bad_fen = params(CnpPosition::fen("not a fen"));
        for wanted in [illegal, bad_fen] {
            let error = evaluate(&wanted, &EvalParams::default()).unwrap_err();
            assert_eq!(error.code, code::INVALID_POSITION);
        }
        // From Black's side, the score is Black's.
        let after_e4 = params(CnpPosition::start().with_moves(["e2e4"]));
        let result = evaluate(&after_e4, &EvalParams::default()).unwrap();
        let net: i32 = result.terms.iter().map(|t| t.black - t.white).sum();
        let cnp::engine::ScoreValue::Centipawns(cp) = result.score.value else {
            panic!("an evaluation is in centipawns");
        };
        assert!((net - cp).abs() <= TERMS.len() as i32, "{net} against {cp}");
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
