//! Driving a UCI engine as a separate process: the GUI's side of the protocol.
//!
//! The engine's output is read on a background thread and passed back through a channel, so
//! the caller can wait for a reply with a timeout instead of blocking forever on an engine
//! that has hung.

use std::fmt;
use std::io::{self, BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

/// Something went wrong talking to an engine.
#[derive(Debug)]
pub enum EngineError {
    /// The engine couldn't be started or written to.
    Io(io::Error),
    /// The engine didn't answer in time.
    Timeout(String),
    /// The engine's output ended (it crashed or quit).
    Exited,
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::Io(e) => write!(f, "{e}"),
            EngineError::Timeout(what) => write!(f, "timed out waiting for {what}"),
            EngineError::Exited => write!(f, "the engine exited"),
        }
    }
}

impl std::error::Error for EngineError {}

impl From<io::Error> for EngineError {
    fn from(e: io::Error) -> EngineError {
        EngineError::Io(e)
    }
}

/// How long to wait for answers that should be immediate (`uciok`, `readyok`).
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

/// A running UCI engine.
pub struct UciEngine {
    child: Child,
    stdin: ChildStdin,
    lines: Receiver<String>,
    /// The engine's `id name`, or the path if it didn't send one.
    pub name: String,
}

/// The engine's answer to `go`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BestMove {
    /// The move, in UCI notation.
    pub uci: String,
    /// How long the engine took, measured on our side.
    pub elapsed: Duration,
    /// The last `score ...` the engine reported (like `cp 31` or `mate 3`), if any.
    pub score: Option<String>,
}

impl UciEngine {
    /// Starts the engine at `path`, does the `uci` handshake and sets `options`.
    pub fn start(path: &str, options: &[(String, String)]) -> Result<UciEngine, EngineError> {
        let mut child = Command::new(path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let stdin = child.stdin.take().expect("stdin is piped");
        let stdout = child.stdout.take().expect("stdout is piped");
        let (sender, lines) = mpsc::channel();
        thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        let mut engine = UciEngine {
            child,
            stdin,
            lines,
            name: path.to_string(),
        };
        engine.send("uci")?;
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        loop {
            let line = engine.next_line(deadline, "uciok")?;
            if let Some(name) = line.strip_prefix("id name ") {
                engine.name = name.trim().to_string();
            }
            if line.trim() == "uciok" {
                break;
            }
        }
        for (name, value) in options {
            engine.send(&format!("setoption name {name} value {value}"))?;
        }
        engine.ready()?;
        Ok(engine)
    }

    /// Sends one command line.
    pub fn send(&mut self, line: &str) -> Result<(), EngineError> {
        writeln!(self.stdin, "{line}")?;
        self.stdin.flush()?;
        Ok(())
    }

    /// The next line of output, waiting until `deadline`.
    fn next_line(&mut self, deadline: Instant, waiting_for: &str) -> Result<String, EngineError> {
        let wait = deadline.saturating_duration_since(Instant::now());
        match self.lines.recv_timeout(wait) {
            Ok(line) => Ok(line),
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout(waiting_for.to_string())),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Exited),
        }
    }

    /// Sends `isready` and waits for `readyok`.
    pub fn ready(&mut self) -> Result<(), EngineError> {
        self.send("isready")?;
        let deadline = Instant::now() + HANDSHAKE_TIMEOUT;
        while self.next_line(deadline, "readyok")?.trim() != "readyok" {}
        Ok(())
    }

    /// Tells the engine a new game starts.
    pub fn new_game(&mut self) -> Result<(), EngineError> {
        self.send("ucinewgame")?;
        self.ready()
    }

    /// Sends `position` and `go`, then waits until `timeout` for `bestmove`.
    pub fn go(
        &mut self,
        position: &str,
        go: &str,
        timeout: Duration,
    ) -> Result<BestMove, EngineError> {
        self.send(position)?;
        let start = Instant::now();
        self.send(go)?;
        let deadline = start + timeout;
        let mut score = None;
        loop {
            let line = self.next_line(deadline, "bestmove")?;
            if let Some(rest) = line.strip_prefix("info ") {
                if let Some(at) = rest.find("score ") {
                    let words: Vec<&str> = rest[at + 6..].split_whitespace().take(2).collect();
                    score = Some(words.join(" "));
                }
            } else if let Some(rest) = line.strip_prefix("bestmove ") {
                let uci = rest.split_whitespace().next().unwrap_or("").to_string();
                return Ok(BestMove {
                    uci,
                    elapsed: start.elapsed(),
                    score,
                });
            }
        }
    }
}

impl Drop for UciEngine {
    fn drop(&mut self) {
        // Ask politely, then make sure.
        let _ = self.send("quit");
        thread::sleep(Duration::from_millis(10));
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
