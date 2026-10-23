//! XBoard / CECP: the Chess Engine Communication Protocol, version 2.
//!
//! An older protocol than UCI, still used by XBoard, WinBoard and some other programs. It works
//! differently in one important way: **the engine keeps track of the game**. A UCI GUI sends the
//! whole position before every search; an XBoard GUI sends one move at a time (`usermove e7e5`),
//! and the engine decides by itself when it is its turn to move. So the engine keeps the
//! position, knows which side it plays (`new`, `force`, `go`, `playother`), and keeps the clock
//! settings (`level`, `st`, `sd`, `time`, `otim`).
//!
//! A short session (`>` is the GUI, `<` the engine):
//!
//! ```text
//! > xboard
//! > protover 2
//! < feature done=0
//! < feature myname="cb2026 0.1.0" setboard=1 usermove=1 ping=1 san=0 ...
//! < feature done=1
//! > new
//! > level 0 5 3
//! > post
//! > usermove e2e4
//! < 1 30 0 20 e7e5
//! < move e7e5
//! ```
//!
//! Thinking output (`post`) is `depth score time nodes pv`, with the score in centipawns from
//! the engine's side and the time in centiseconds. Mate scores are shown as 100000 plus the
//! number of moves to mate (negative when being mated), the usual convention. With MultiPV,
//! each depth's lines are sent together, best last: XBoard sorts them by score to show them, but
//! takes the last one as the engine's score and PV.
//!
//! See the CECP specification by Tim Mann and H.G. Muller,
//! <https://www.gnu.org/software/xboard/engine-intf.html>, and the Chess Programming Wiki,
//! [Chess Engine Communication Protocol](https://www.chessprogramming.org/Chess_Engine_Communication_Protocol).

use std::cell::{Cell, RefCell};
use std::io::{self, BufRead, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use cb2026_chess::{Color, Move, MoveKind, Position};
use cb2026_engine::levels::FULL_STRENGTH;
use cb2026_engine::time::{self, TimeParams};
use cb2026_engine::{Engine, Limits, MAX_THREADS, Score, SearchInfo, id_name};

type Output = Arc<Mutex<Box<dyn Write + Send>>>;

fn send(output: &Output, line: &str) {
    let mut out = output.lock().unwrap_or_else(|e| e.into_inner());
    let _ = writeln!(out, "{line}");
    let _ = out.flush();
}

/// The time control set by `level`, `st` or `sd`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TimeControl {
    /// `level MPS BASE INC`: moves per session (0 for the whole game), and an increment.
    Clock {
        moves_per_session: u32,
        increment: Duration,
    },
    /// `st N`: exactly N seconds per move.
    MoveTime(Duration),
}

/// What the search thread and the session share.
#[derive(Debug)]
struct Shared {
    /// The moves the thread has played, in order. Each is written here *before* `move ...` is
    /// sent, so the session can play it on its own board before it reads the GUI's reply.
    played: Vec<Move>,
    /// While pondering: the opponent's move we expect.
    pondering: Option<Move>,
    /// The stop flag of the search running now. Each search gets a fresh one, so a timer set
    /// for one search can never stop the next.
    stop: Arc<AtomicBool>,
}

/// A search thread started by the session: it owns the engine until it finishes.
///
/// A game search plays its move and then, with pondering on (`hard`), carries on: it plays
/// the expected reply on its own board and searches the position after it while the opponent
/// thinks. If the opponent plays that move (a *ponder hit*) the same search goes on against the
/// clock; any other move ends the thread and a new search starts.
struct RunningSearch {
    shared: Arc<Mutex<Shared>>,
    /// Set when the session wants the thread to end without playing another move.
    quit: Arc<AtomicBool>,
    handle: JoinHandle<Engine>,
    /// How many of `Shared::played` the session has played on its board.
    collected: usize,
    /// Whether this is an analysis search (it never plays a move).
    analysis: bool,
    /// The engine's time settings, for a ponder hit (the engine is busy in the thread).
    time_params: TimeParams,
}

/// Locks `shared`, even if a thread panicked while holding it.
fn lock(shared: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(|e| e.into_inner())
}

/// The state of one XBoard session.
struct Session {
    output: Output,
    engine: Option<Engine>,
    search: Option<RunningSearch>,
    position: Position,
    /// The positions before `position` and the moves that led from each, for `undo`.
    history: Vec<(Position, Move)>,
    /// The side the engine plays, or `None` in force mode.
    engine_side: Option<Color>,
    time_control: TimeControl,
    max_depth: Option<u32>,
    /// The engine's and the opponent's clocks (`time`, `otim`).
    engine_time: Option<Duration>,
    opponent_time: Option<Duration>,
    post: bool,
    /// Pondering (`hard`): thinking on the opponent's time.
    ponder: bool,
    analyzing: bool,
    chess960: bool,
}

/// Runs an XBoard session until `quit` or the end of input. `first_line`, if given, was
/// already read (it is usually `xboard`).
pub fn run<R: BufRead>(
    input: R,
    output: Box<dyn Write + Send>,
    first_line: Option<String>,
    engine: Engine,
) -> io::Result<()> {
    let mut session = Session::new(output, engine);
    for line in first_line.into_iter().map(Ok).chain(input.lines()) {
        if !session.handle(line?.trim()) {
            break;
        }
    }
    session.stop_search();
    Ok(())
}

impl Session {
    fn new(output: Box<dyn Write + Send>, engine: Engine) -> Session {
        Session {
            output: Arc::new(Mutex::new(output)),
            engine: Some(engine),
            search: None,
            position: Position::startpos(),
            history: Vec::new(),
            engine_side: Some(Color::Black),
            time_control: TimeControl::Clock {
                moves_per_session: 0,
                increment: Duration::ZERO,
            },
            max_depth: None,
            engine_time: None,
            opponent_time: None,
            post: false,
            ponder: false,
            analyzing: false,
            chess960: false,
        }
    }

    /// Handles one command; `false` means quit.
    fn handle(&mut self, line: &str) -> bool {
        self.collect_finished_search();
        let mut words = line.splitn(2, ' ');
        let Some(command) = words.next().filter(|c| !c.is_empty()) else {
            return true;
        };
        let args = words.next().unwrap_or("").trim();
        match command {
            "xboard" | "accepted" | "rejected" | "random" | "computer" | "name" | "rating"
            | "ics" | "." | "bk" | "hint" => {}
            "hard" => self.ponder = true,
            "easy" => {
                self.ponder = false;
                if self.pondering().is_some() {
                    self.stop_search();
                }
            }
            "protover" => self.features(),
            "new" => {
                self.stop_search();
                self.set_position(Position::startpos());
                self.engine_side = Some(Color::Black);
                self.max_depth = None;
                self.chess960 = false;
                if let Some(engine) = self.engine.as_mut() {
                    engine.new_game();
                }
            }
            "variant" => self.chess960 = args == "fischerandom",
            "setboard" => match Position::from_fen(args) {
                Ok(position) => {
                    self.stop_search();
                    self.set_position(position);
                }
                Err(e) => send(
                    &self.output,
                    &format!("tellusererror Illegal position: {e}"),
                ),
            },
            "force" => {
                self.stop_search();
                self.engine_side = None;
            }
            "go" => {
                self.stop_search();
                self.engine_side = Some(self.position.side_to_move());
                self.think();
            }
            "playother" => {
                self.stop_search();
                self.engine_side = Some(!self.position.side_to_move());
            }
            "usermove" => self.user_move(args),
            "?" => {
                // Move now: stop thinking and play the best move so far (not while pondering or
                // analysing, when it isn't our move).
                if let Some(search) = &self.search
                    && !search.analysis
                {
                    let shared = lock(&search.shared);
                    if shared.pondering.is_none() {
                        shared.stop.store(true, Ordering::Relaxed);
                    }
                }
            }
            "level" => self.level(args),
            "st" => {
                if let Ok(seconds) = args.parse::<f64>() {
                    self.time_control =
                        TimeControl::MoveTime(Duration::from_secs_f64(seconds.max(0.0)));
                }
            }
            "sd" => self.max_depth = args.parse().ok(),
            "time" => self.engine_time = centiseconds(args),
            "otim" => self.opponent_time = centiseconds(args),
            "post" => self.post = true,
            "nopost" => self.post = false,
            "analyze" => {
                self.stop_search();
                self.analyzing = true;
                self.engine_side = None;
                self.analyze();
            }
            "exit" => {
                self.stop_search();
                self.analyzing = false;
            }
            "undo" => self.take_back(1),
            "remove" => self.take_back(2),
            "result" => {
                self.stop_search();
                self.engine_side = None;
            }
            "ping" => send(&self.output, &format!("pong {args}")),
            "memory" => {
                if let (Ok(mb), Some(engine)) = (args.parse::<usize>(), self.engine.as_mut()) {
                    engine.set_hash_size(mb.max(1));
                }
            }
            "cores" => {
                if let (Ok(n), Some(engine)) = (args.parse::<usize>(), self.engine.as_mut()) {
                    engine.threads = n.clamp(1, MAX_THREADS);
                }
            }
            "egtpath" => {
                let path = args.strip_prefix("syzygy").unwrap_or(args).trim();
                if let Some(engine) = self.engine.as_mut()
                    && let Err(e) = engine.load_tablebases(path)
                {
                    send(
                        &self.output,
                        &format!("telluser Tablebases not loaded: {e}"),
                    );
                }
            }
            "option" => self.option(args),
            "quit" => return false,
            other => {
                // A bare move is a move too (for GUIs that don't send `usermove`).
                if self.parse_move(other).is_some() {
                    self.user_move(other);
                } else {
                    send(&self.output, &format!("Error (unknown command): {other}"));
                }
            }
        }
        true
    }

    /// The features we support, in answer to `protover 2`.
    fn features(&self) {
        send(&self.output, "feature done=0");
        send(
            &self.output,
            &format!(
                "feature myname=\"{}\" setboard=1 usermove=1 ping=1 san=0 analyze=1 colors=0 \
                 time=1 draw=0 sigint=0 sigterm=0 reuse=1 memory=1 smp=1 egt=\"syzygy\" \
                 variants=\"normal,fischerandom\"",
                id_name()
            ),
        );
        send(
            &self.output,
            &format!("feature option=\"Skill Level -spin {FULL_STRENGTH} 0 {FULL_STRENGTH}\""),
        );
        send(&self.output, "feature option=\"MultiPV -spin 1 1 256\"");
        send(&self.output, "feature done=1");
    }

    /// `option NAME=VALUE`, for the options announced in [`Session::features`].
    fn option(&mut self, args: &str) {
        let Some((name, value)) = args.split_once('=') else {
            return;
        };
        let Some(engine) = self.engine.as_mut() else {
            return;
        };
        match (name, value.parse::<u32>()) {
            ("Skill Level", Ok(level)) => engine.skill_level = level.min(FULL_STRENGTH),
            ("MultiPV", Ok(n)) => engine.multipv = n.clamp(1, 256) as usize,
            _ => send(&self.output, &format!("Error (unknown option): {args}")),
        }
    }

    /// `level MPS BASE INC`: BASE is minutes, or minutes:seconds.
    fn level(&mut self, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        if let [mps, _base, inc] = parts[..] {
            self.time_control = TimeControl::Clock {
                moves_per_session: mps.parse().unwrap_or(0),
                increment: Duration::from_secs_f64(inc.parse::<f64>().unwrap_or(0.0).max(0.0)),
            };
        }
    }

    fn set_position(&mut self, position: Position) {
        self.position = position;
        self.history.clear();
        if self.analyzing {
            self.analyze();
        }
    }

    /// Reads a move as XBoard sends it: coordinates (`e2e4`, `e7e8q`), or `O-O`/`O-O-O`
    /// (how XBoard writes castling in Chess960).
    fn parse_move(&self, text: &str) -> Option<Move> {
        self.position.parse_uci_move(text).or_else(|| match text {
            "O-O" | "O-O-O" | "0-0" | "0-0-0" => {
                self.position.parse_san(&text.replace('0', "O")).ok()
            }
            _ => None,
        })
    }

    /// `usermove`: the opponent's move. Then it may be our turn.
    fn user_move(&mut self, text: &str) {
        let Some(mv) = self.parse_move(text) else {
            send(&self.output, &format!("Illegal move: {text}"));
            return;
        };
        if self.pondering() == Some(mv) {
            self.ponder_hit(mv);
            return;
        }
        let analyzing = self.analyzing;
        self.stop_search();
        self.play(mv);
        if analyzing {
            self.analyze();
        } else if self.engine_side == Some(self.position.side_to_move()) {
            self.think();
        }
    }

    fn play(&mut self, mv: Move) {
        let before = self.position.clone();
        self.position.make_move(mv);
        self.history.push((before, mv));
    }

    /// `undo` / `remove`: take back plies.
    fn take_back(&mut self, plies: usize) {
        let analyzing = self.analyzing;
        self.stop_search();
        for _ in 0..plies {
            if let Some((before, _)) = self.history.pop() {
                self.position = before;
            }
        }
        if analyzing {
            self.analyze();
        }
    }

    /// The hash keys of the game so far, for repetition detection.
    fn history_keys(&self) -> Vec<u64> {
        self.history.iter().map(|(pos, _)| pos.hash()).collect()
    }

    /// The search limits for the engine's move from the clock settings.
    fn limits(&self) -> Limits {
        let mut limits = Limits {
            depth: self.max_depth,
            ..Limits::default()
        };
        let us = self.position.side_to_move();
        match self.time_control {
            TimeControl::MoveTime(time) => limits.movetime = Some(time),
            TimeControl::Clock {
                moves_per_session,
                increment,
            } => {
                limits.time[us.index()] = self.engine_time;
                limits.time[(!us).index()] = self.opponent_time;
                limits.increment = [Some(increment); 2];
                if moves_per_session > 0 {
                    // Moves left until the next time control, counting our moves only.
                    let played = (self.history.len() / 2) as u32;
                    limits.moves_to_go = Some(moves_per_session - played % moves_per_session);
                }
            }
        }
        if limits.depth.is_none() && limits.movetime.is_none() && limits.time[us.index()].is_none()
        {
            // No clock received yet: think for a second.
            limits.movetime = Some(Duration::from_secs(1));
        }
        limits
    }

    /// Starts thinking about our move.
    fn think(&mut self) {
        let limits = self.limits();
        self.start_search(limits, false);
    }

    /// Starts (or restarts) analysis of the current position.
    fn analyze(&mut self) {
        let limits = Limits {
            infinite: true,
            ..Limits::default()
        };
        self.start_search(limits, true);
    }

    fn start_search(&mut self, limits: Limits, analysis: bool) {
        if self.position.legal_moves().is_empty() {
            return;
        }
        let Some(mut engine) = self.engine.take() else {
            return;
        };
        let time_params = engine.time.clone();
        let shared = Arc::new(Mutex::new(Shared {
            played: Vec::new(),
            pondering: None,
            stop: Arc::new(AtomicBool::new(false)),
        }));
        let quit = Arc::new(AtomicBool::new(false));
        let (thread_shared, thread_quit) = (Arc::clone(&shared), Arc::clone(&quit));
        let output = Arc::clone(&self.output);
        let mut position = self.position.clone();
        let mut keys = self.history_keys();
        let mut limits = limits;
        let post = self.post || analysis;
        let ponder = self.ponder && !analysis;
        let chess960 = self.chess960;
        let multipv = engine.multipv.max(1);
        let handle = thread::spawn(move || {
            // MultiPV: the engine reports a depth's lines together, best first. XBoard sorts
            // each depth's lines by score for display, but keeps the last one it reads as the
            // engine's score and PV (for the PGN, the evaluation graph and adjudication), so a
            // depth's lines are held until the last arrives, then sent best last. If fewer
            // arrive (a search stopped mid-depth), the next depth or the search's end sends them.
            let pending = RefCell::new(Vec::<String>::new());
            let expected = Cell::new(1);
            let flush = || {
                for line in pending.borrow_mut().drain(..).rev() {
                    send(&output, &line);
                }
            };
            let mut on_info = |info: &SearchInfo| {
                if !post {
                    return;
                }
                if info.multipv == 1 {
                    flush();
                }
                pending.borrow_mut().push(thinking_line(info, chess960));
                if pending.borrow().len() >= expected.get() {
                    flush();
                }
            };
            // ANCHOR: ponder
            loop {
                expected.set(multipv.min(position.legal_moves().len()).max(1));
                let stop = Arc::clone(&lock(&thread_shared).stop);
                let result = engine.search(&position, &keys, &limits, &stop, &mut on_info);
                flush();
                // Analysis and pondering wait to be stopped, or for the ponder hit.
                while (analysis || lock(&thread_shared).pondering.is_some())
                    && !stop.load(Ordering::Relaxed)
                {
                    thread::sleep(Duration::from_millis(1));
                }
                let mut shared = lock(&thread_shared);
                // Stopped by the session, during analysis, or a ponder miss: no move.
                if analysis || shared.pondering.is_some() || thread_quit.load(Ordering::SeqCst) {
                    break;
                }
                let Some(mv) = result.best_move else {
                    break;
                };
                // Record the move before announcing it (see `Shared::played`).
                shared.played.push(mv);
                send(&output, &format!("move {}", move_text(mv, chess960)));
                // Ponder on the expected reply, if there is one and the game goes on.
                let Some(reply) = result.ponder_move.filter(|_| ponder) else {
                    break;
                };
                keys.push(position.hash());
                position.make_move(mv);
                keys.push(position.hash());
                position.make_move(reply);
                if position.legal_moves().is_empty() {
                    break;
                }
                shared.pondering = Some(reply);
                shared.stop = Arc::new(AtomicBool::new(false));
                limits = Limits {
                    infinite: true,
                    ..Limits::default()
                };
            }
            // ANCHOR_END: ponder
            engine
        });
        self.search = Some(RunningSearch {
            shared,
            quit,
            handle,
            collected: 0,
            analysis,
            time_params,
        });
    }

    /// The opponent's move we are pondering on, if we are.
    fn pondering(&self) -> Option<Move> {
        let search = self.search.as_ref()?;
        lock(&search.shared).pondering
    }

    /// The opponent played the move we were pondering on: the ponder search goes on, now
    /// against our clock. It stops once the time it would have been given has passed, counting
    /// from now, so the pondering time is a bonus.
    fn ponder_hit(&mut self, mv: Move) {
        self.play(mv);
        let limits = self.limits();
        let side = self.position.side_to_move();
        let Some(search) = self.search.as_ref() else {
            return;
        };
        let mut shared = lock(&search.shared);
        shared.pondering = None;
        let stop = Arc::clone(&shared.stop);
        match time::budget(&limits, side, &search.time_params).soft {
            Some(time) => {
                thread::spawn(move || {
                    thread::sleep(time);
                    stop.store(true, Ordering::Relaxed);
                });
            }
            // A depth limit and no clock: the ponder search has gone deep enough.
            None => stop.store(true, Ordering::Relaxed),
        }
    }

    /// Plays the moves our search thread has made since we last looked, and takes the engine
    /// back if the thread has finished.
    fn collect_finished_search(&mut self) {
        let Some(search) = self.search.as_mut() else {
            return;
        };
        let new: Vec<Move> = lock(&search.shared).played[search.collected..].to_vec();
        search.collected += new.len();
        let finished = search.handle.is_finished();
        for mv in new {
            self.play(mv);
        }
        if finished {
            self.stop_search();
        }
    }

    /// Ends the search thread (if any) without letting it play a new move, takes the engine
    /// back, and plays any move it had already announced.
    fn stop_search(&mut self) {
        let Some(search) = self.search.take() else {
            return;
        };
        search.quit.store(true, Ordering::SeqCst);
        lock(&search.shared).stop.store(true, Ordering::Relaxed);
        match search.handle.join() {
            Ok(engine) => self.engine = Some(engine),
            Err(_) => {
                send(&self.output, "telluser the search thread failed");
                self.engine = Some(Engine::new());
            }
        }
        let played = lock(&search.shared).played[search.collected..].to_vec();
        for mv in played {
            self.play(mv);
        }
    }
}

/// A move as XBoard writes it: coordinates (`e2e4`, `e7e8q`, castling as the king's move
/// `e1g1`), except castling in Chess960, which is `O-O` or `O-O-O`.
fn move_text(mv: Move, chess960: bool) -> String {
    if chess960 && mv.kind() == MoveKind::Castle {
        let short = mv.to().file() > mv.from().file();
        return if short { "O-O" } else { "O-O-O" }.to_string();
    }
    mv.to_uci(false)
}

/// `time` and `otim` arguments: centiseconds.
fn centiseconds(text: &str) -> Option<Duration> {
    text.trim()
        .parse::<i64>()
        .ok()
        .map(|cs| Duration::from_millis((cs.max(0) * 10) as u64))
}

// ANCHOR: thinking
/// A thinking line: `depth score time nodes pv`, score in centipawns (mates as 100000 + moves),
/// time in centiseconds, the PV in coordinate notation.
fn thinking_line(info: &SearchInfo, chess960: bool) -> String {
    let score = match info.score {
        Score::Centipawns(cp) => cp,
        Score::Mate(moves) if moves > 0 => 100_000 + moves,
        Score::Mate(moves) => -100_000 + moves,
    };
    let pv: Vec<String> = info.pv.iter().map(|&mv| move_text(mv, chess960)).collect();
    format!(
        "{} {score} {} {} {}",
        info.depth,
        info.time.as_millis() / 10,
        info.nodes,
        pv.join(" ")
    )
}
// ANCHOR_END: thinking

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);

    impl Write for Captured {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    /// Runs a script, waiting `pause` milliseconds after each line so searches can finish.
    fn session(script: &[&str], pause: u64) -> Vec<String> {
        let captured = Captured::default();
        let mut session = Session::new(Box::new(captured.clone()), Engine::new());
        for line in script {
            if !session.handle(line) {
                break;
            }
            thread::sleep(Duration::from_millis(pause));
        }
        session.stop_search();
        let bytes = captured.0.lock().unwrap().clone();
        String::from_utf8(bytes)
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn moves(lines: &[String]) -> Vec<String> {
        lines
            .iter()
            .filter_map(|l| l.strip_prefix("move ").map(str::to_string))
            .collect()
    }

    #[test]
    fn announces_features() {
        let lines = session(&["xboard", "protover 2"], 0);
        assert_eq!(lines.first().unwrap(), "feature done=0");
        assert_eq!(lines.last().unwrap(), "feature done=1");
        assert!(lines[1].contains("setboard=1 usermove=1 ping=1"));
        assert!(lines[1].contains("variants=\"normal,fischerandom\""));
    }

    #[test]
    fn plays_black_after_new_and_answers_each_move() {
        let lines = session(
            &[
                "new",
                "sd 3",
                "post",
                "usermove e2e4",
                "usermove d2d4",
                "ping 7",
            ],
            400,
        );
        let played = moves(&lines);
        assert_eq!(played.len(), 2, "{lines:?}");
        assert!(lines.iter().any(|l| l == "pong 7"));
        // Thinking lines: depth score time nodes pv.
        let thinking = lines.iter().find(|l| l.starts_with("3 ")).unwrap();
        assert!(thinking.split_whitespace().count() >= 5, "{thinking}");
    }

    #[test]
    fn force_go_and_undo() {
        let lines = session(
            &[
                "new",
                "force",
                "usermove e2e4",
                "usermove e7e5",
                "st 0.2",
                "go",
                "force",
                "undo",
                "undo",
                "ping 1",
            ],
            400,
        );
        // In force mode it doesn't reply; `go` makes it play White's third ply.
        assert_eq!(moves(&lines).len(), 1, "{lines:?}");
        assert!(lines.iter().any(|l| l == "pong 1"));
    }

    #[test]
    fn illegal_moves_and_unknown_commands_are_reported() {
        let lines = session(&["new", "force", "usermove e2e5", "frobnicate"], 0);
        assert!(lines.iter().any(|l| l == "Illegal move: e2e5"));
        assert!(
            lines
                .iter()
                .any(|l| l == "Error (unknown command): frobnicate")
        );
    }

    #[test]
    fn setboard_and_mate_in_one() {
        let lines = session(
            &[
                "new",
                "force",
                "setboard 6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1",
                "sd 3",
                "go",
            ],
            400,
        );
        assert_eq!(moves(&lines), ["e1e8"]);
    }

    #[test]
    fn multipv_sends_each_depths_lines_best_last() {
        let lines = session(
            &["new", "force", "option MultiPV=3", "post", "sd 5", "go"],
            1000,
        );
        let played = moves(&lines);
        assert_eq!(played.len(), 1, "{lines:?}");
        // Thinking lines (depth score time nodes pv), grouped by depth in the order sent.
        let mut depths: Vec<(u32, Vec<(i32, String)>)> = Vec::new();
        for line in &lines {
            let words: Vec<&str> = line.split_whitespace().collect();
            let (Some(depth), Some(score)) = (
                words.first().and_then(|w| w.parse::<u32>().ok()),
                words.get(1).and_then(|w| w.parse::<i32>().ok()),
            ) else {
                continue;
            };
            let first_move = words.get(4).copied().unwrap_or("").to_string();
            match depths.last_mut() {
                Some((d, group)) if *d == depth => group.push((score, first_move)),
                _ => depths.push((depth, vec![(score, first_move)])),
            }
        }
        assert_eq!(depths.len(), 5, "{lines:?}");
        for (depth, group) in &depths {
            assert_eq!(group.len(), 3, "depth {depth}: {lines:?}");
            let best = group.iter().map(|(score, _)| *score).max().unwrap();
            assert_eq!(group.last().unwrap().0, best, "depth {depth}: {lines:?}");
        }
        // XBoard takes the last line as the engine's PV: it starts with the move played.
        assert_eq!(depths.last().unwrap().1.last().unwrap().1, played[0]);
    }

    #[test]
    fn analysis_reports_and_follows_the_moves() {
        let lines = session(&["new", "post", "analyze", "usermove e2e4", "exit"], 300);
        // Thinking lines, and no move played.
        assert!(lines.iter().any(|l| l.starts_with("1 ")), "{lines:?}");
        assert!(moves(&lines).is_empty());
    }

    /// Polls `condition` until it holds, for up to five seconds (tests run on busy machines).
    fn wait_until(mut condition: impl FnMut() -> bool) -> bool {
        let start = std::time::Instant::now();
        while start.elapsed() < Duration::from_secs(5) {
            if condition() {
                return true;
            }
            thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn ponders_on_the_expected_reply() {
        let captured = Captured::default();
        let mut session = Session::new(Box::new(captured.clone()), Engine::new());
        let played = || {
            let text = String::from_utf8(captured.0.lock().unwrap().clone()).unwrap();
            text.lines().filter(|l| l.starts_with("move ")).count()
        };
        for line in ["new", "hard", "st 0.3", "usermove e2e4"] {
            session.handle(line);
        }
        assert!(wait_until(|| played() == 1));
        session.handle("ping 1");
        // After moving it ponders, and doesn't move again by itself.
        let expected = session.pondering().expect("pondering after its move");
        thread::sleep(Duration::from_millis(700));
        assert_eq!(played(), 1);
        // A ponder hit: the same search answers within its time.
        session.handle(&format!("usermove {}", expected.to_uci(false)));
        assert!(wait_until(|| played() == 2));
        session.handle("ping 2");
        assert_eq!(session.history.len(), 4);
        // A miss: any other move ends the ponder search and starts a new one. Not a king move:
        // the first one generated can walk into a mate (1. e4 d5 2. exd5 Qxd5 3. Ke2 Qe4#), and
        // a finished game has nothing to ponder on.
        let expected = session.pondering().expect("pondering again");
        let king = session
            .position
            .king_square(session.position.side_to_move());
        let other = session
            .position
            .legal_moves()
            .iter()
            .copied()
            .find(|&mv| mv != expected && mv.from() != king)
            .unwrap();
        session.handle(&format!("usermove {}", other.to_uci(false)));
        assert!(wait_until(|| played() == 3));
        session.handle("ping 3");
        assert_eq!(session.history.len(), 6);
        // `easy` stops pondering.
        assert!(session.pondering().is_some());
        session.handle("easy");
        assert_eq!(session.pondering(), None);
        assert_eq!(played(), 3);
    }

    #[test]
    fn chess960_castling_is_written_o_o() {
        // White can only castle short and it mates nothing else: just check the notation.
        let lines = session(
            &[
                "new",
                "variant fischerandom",
                "force",
                "setboard 4k3/8/8/8/8/8/8/4K2R w K - 0 1",
                "usermove O-O",
                "ping 2",
            ],
            100,
        );
        assert!(!lines.iter().any(|l| l.starts_with("Illegal")), "{lines:?}");
        assert!(lines.iter().any(|l| l == "pong 2"));
    }
}
