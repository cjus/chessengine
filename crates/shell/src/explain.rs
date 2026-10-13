//! The shell's "look inside" commands: what each technique is worth and why the engine
//! chose its move.
//!
//! - `techniques` lists every switchable technique and evaluation term; `set <name> on|off`
//!   switches one, and `ab <name> [depth]` searches with and without it and compares.
//! - `stats` shows what the last search did (see `cb2026_engine::stats`).
//! - `tree <depth> [plies]` prints the search tree's first plies: windows, scores, cutoffs,
//!   and the moves pruned or reduced, with the reason.
//! - `explain <move>` searches a move and compares it with the engine's choice.
//! - `eval diff <move>` shows which evaluation terms a move changes.
//! - `see <move>` shows the static exchange evaluation of a move.

use std::sync::atomic::AtomicBool;
use std::time::Instant;

use cb2026_chess::{Move, Position};
use cb2026_engine::search::{INFINITY, to_score};
use cb2026_engine::stats::{Note, SearchStats, SearchTree};
use cb2026_engine::{Engine, Limits, SearchInfo, eval, levels, see};

use crate::{Response, Shell, parse_move, pawns, pv_san, score_text};

/// A technique or evaluation term that can be switched on and off: its name in `set`, what
/// it is, and where its switch lives in the engine.
pub(crate) struct Switch {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) flag: fn(&mut Engine) -> &mut bool,
}

/// Every switch, in the order the stages added them.
pub(crate) const SWITCHES: &[Switch] = &[
    Switch {
        name: "alphabeta",
        description: "Alpha-beta pruning (off: plain minimax)",
        flag: |e| &mut e.techniques.alpha_beta,
    },
    Switch {
        name: "quiescence",
        description: "Quiescence search",
        flag: |e| &mut e.techniques.quiescence,
    },
    Switch {
        name: "mvvlva",
        description: "MVV-LVA capture ordering",
        flag: |e| &mut e.techniques.mvv_lva,
    },
    Switch {
        name: "tt",
        description: "Transposition table",
        flag: |e| &mut e.techniques.tt,
    },
    Switch {
        name: "pst",
        description: "Piece-square tables",
        flag: |e| &mut e.params.use_pst,
    },
    Switch {
        name: "tapered",
        description: "Tapered evaluation (middlegame/endgame blend)",
        flag: |e| &mut e.params.tapered,
    },
    Switch {
        name: "killers",
        description: "Killer moves",
        flag: |e| &mut e.techniques.killers,
    },
    Switch {
        name: "countermoves",
        description: "Countermoves",
        flag: |e| &mut e.techniques.countermoves,
    },
    Switch {
        name: "history",
        description: "History heuristic",
        flag: |e| &mut e.techniques.history,
    },
    Switch {
        name: "conthist",
        description: "Continuation history",
        flag: |e| &mut e.techniques.continuation_history,
    },
    Switch {
        name: "pvs",
        description: "Principal variation search",
        flag: |e| &mut e.techniques.pvs,
    },
    Switch {
        name: "aspiration",
        description: "Aspiration windows",
        flag: |e| &mut e.techniques.aspiration,
    },
    Switch {
        name: "nullmove",
        description: "Null-move pruning",
        flag: |e| &mut e.techniques.null_move,
    },
    Switch {
        name: "lmr",
        description: "Late move reductions",
        flag: |e| &mut e.techniques.lmr,
    },
    Switch {
        name: "lmp",
        description: "Late move pruning",
        flag: |e| &mut e.techniques.late_move_pruning,
    },
    Switch {
        name: "rfp",
        description: "Reverse futility pruning",
        flag: |e| &mut e.techniques.reverse_futility,
    },
    Switch {
        name: "razoring",
        description: "Razoring",
        flag: |e| &mut e.techniques.razoring,
    },
    Switch {
        name: "futility",
        description: "Futility pruning",
        flag: |e| &mut e.techniques.futility,
    },
    Switch {
        name: "seeprune",
        description: "SEE pruning",
        flag: |e| &mut e.techniques.see_pruning,
    },
    Switch {
        name: "delta",
        description: "Delta pruning (quiescence)",
        flag: |e| &mut e.techniques.delta_pruning,
    },
    Switch {
        name: "seeorder",
        description: "Losing captures (by SEE) after quiet moves",
        flag: |e| &mut e.techniques.see_ordering,
    },
    Switch {
        name: "checkext",
        description: "Check extensions (off by default)",
        flag: |e| &mut e.techniques.check_extension,
    },
    Switch {
        name: "iir",
        description: "Internal iterative reductions",
        flag: |e| &mut e.techniques.iir,
    },
    Switch {
        name: "improving",
        description: "Adjust pruning when improving (off by default)",
        flag: |e| &mut e.techniques.improving,
    },
    Switch {
        name: "mobility",
        description: "Evaluation: mobility",
        flag: |e| &mut e.params.terms.use_mobility,
    },
    Switch {
        name: "pawns",
        description: "Evaluation: pawn structure and passed pawns",
        flag: |e| &mut e.params.terms.use_pawns,
    },
    Switch {
        name: "pieces",
        description: "Evaluation: bishop pair and rook files",
        flag: |e| &mut e.params.terms.use_pieces,
    },
    Switch {
        name: "kingsafety",
        description: "Evaluation: king safety",
        flag: |e| &mut e.params.terms.use_king_safety,
    },
    Switch {
        name: "tempo",
        description: "Evaluation: tempo",
        flag: |e| &mut e.params.terms.use_tempo,
    },
];

/// The switch called `name`.
pub(crate) fn switch(name: &str) -> Option<&'static Switch> {
    SWITCHES.iter().find(|s| s.name == name)
}

/// Depth for `ab` when none is given.
const AB_DEFAULT_DEPTH: u32 = 8;
/// Plies of the tree `tree` shows when not told.
const TREE_DEFAULT_PLIES: usize = 2;
/// Decisions a recorded tree keeps at most.
const TREE_MAX_EVENTS: usize = 20_000;
/// Lines `tree` prints at most.
const TREE_MAX_LINES: usize = 400;

impl Shell {
    /// `techniques`: every switch and whether it's on.
    pub(crate) fn techniques(&mut self) -> Response {
        let mut out = String::from(
            "Techniques and evaluation terms (switch with `set <name> on|off`, compare with `ab <name>`):\n",
        );
        for s in SWITCHES {
            let on = *(s.flag)(&mut self.engine);
            out.push_str(&format!(
                "  {:<13}{:<5}{}\n",
                s.name,
                if on { "on" } else { "off" },
                s.description
            ));
        }
        Response::text(out.trim_end())
    }

    /// An engine for experiments: a copy of ours at full strength, one line, one thread, no
    /// book, and an empty transposition table (so earlier searches don't skew a comparison).
    pub(crate) fn lab_engine(&self) -> Engine {
        let mut engine = self.engine.clone();
        engine.skill_level = levels::FULL_STRENGTH;
        engine.limit_strength = false;
        engine.multipv = 1;
        engine.threads = 1;
        engine.own_book = false;
        engine.new_game();
        engine
    }

    fn history_hashes(&self) -> Vec<u64> {
        self.history.iter().map(|p| p.before.hash()).collect()
    }

    /// Searches the current position with `engine`, returning the last main-line report and
    /// the move chosen.
    pub(crate) fn lab_search(
        &self,
        engine: &mut Engine,
        limits: &Limits,
    ) -> (Option<SearchInfo>, Option<Move>, SearchStats) {
        let stop = AtomicBool::new(false);
        let mut last = None;
        let result = engine.search(
            &self.position,
            &self.history_hashes(),
            limits,
            &stop,
            &mut |info: &SearchInfo| {
                if info.multipv == 1 {
                    last = Some(info.clone());
                }
            },
        );
        (last, result.best_move, result.stats)
    }

    /// `ab <name> [depth]`: searches with the technique on and off and compares.
    pub(crate) fn ab(&mut self, args: &str) -> Response {
        let mut words = args.split_whitespace();
        let usage = "Usage: ab <technique> [depth]   (`techniques` lists them)";
        let Some(s) = words.next().and_then(switch) else {
            return Response::text(usage);
        };
        let depth = match words.next().map(str::parse::<u32>) {
            None => AB_DEFAULT_DEPTH,
            Some(Ok(d)) if (1..=20).contains(&d) => d,
            _ => return Response::text(usage),
        };
        if self.position.legal_moves().is_empty() {
            return Response::text("No legal moves.");
        }
        let mut out = format!(
            "{} ({}), depth {depth}, one thread, empty hash table:\n  {:<5}{:>12}{:>10}  {:<8}{:>7}\n",
            s.description, s.name, "", "nodes", "time", "move", "score"
        );
        let mut nodes = [0u64; 2];
        for (i, on) in [true, false].into_iter().enumerate() {
            let mut engine = self.lab_engine();
            *(s.flag)(&mut engine) = on;
            let start = Instant::now();
            let (info, mv, stats) = self.lab_search(&mut engine, &Limits::depth(depth));
            let time = start.elapsed();
            nodes[i] = stats.nodes + stats.qnodes;
            out.push_str(&format!(
                "  {:<5}{:>12}{:>8.2} s  {:<8}{:>7}\n",
                if on { "on" } else { "off" },
                nodes[i],
                time.as_secs_f64(),
                mv.map_or("-".to_string(), |mv| self.position.to_san(mv)),
                info.map_or("-".to_string(), |i| score_text(i.score)),
            ));
        }
        let ratio = nodes[1] as f64 / nodes[0].max(1) as f64;
        out.push_str(&format!(
            "Switched off, the search needs {ratio:.2}× the nodes. (One position says little: try several, and `bench`.)"
        ));
        Response::text(out)
    }

    /// `stats`: what the last search did.
    pub(crate) fn stats(&self) -> Response {
        let Some(s) = self.last_stats else {
            return Response::text("No search yet: `go`, `explain` or a move against the engine.");
        };
        let percent = |part: u64, whole: u64| 100.0 * part as f64 / whole.max(1) as f64;
        let all = s.nodes + s.qnodes;
        let mut out = format!(
            "The last search (all threads): {all} nodes, {} in the main search, {} in the quiescence search ({:.0}%)\n",
            s.nodes,
            s.qnodes,
            percent(s.qnodes, all)
        );
        out.push_str(&format!(
            "  Transposition table: an entry at {:.1}% of main-search nodes, settled {:.1}%\n",
            percent(s.tt_hits, s.nodes),
            percent(s.tt_cutoffs, s.nodes)
        ));
        out.push_str(&format!(
            "  Beta cutoffs: {}, {:.1}% by the first move searched (move ordering at work)\n",
            s.cutoffs,
            percent(s.first_move_cutoffs, s.cutoffs)
        ));
        out.push_str(&format!(
            "  Null moves: {} tried, {} cut off ({:.0}%)\n",
            s.null_moves,
            s.null_move_cutoffs,
            percent(s.null_move_cutoffs, s.null_moves)
        ));
        out.push_str(&format!(
            "  Nodes cut off: reverse futility {}, razoring {}, tablebases {}\n",
            s.reverse_futility, s.razoring, s.tablebase
        ));
        out.push_str(&format!(
            "  Moves skipped: late move pruning {}, futility {}, SEE {}, delta {}\n",
            s.late_move_pruning, s.futility, s.see_pruning, s.delta_pruning
        ));
        out.push_str(&format!(
            "  Late move reductions: {} moves reduced, {} searched again ({:.1}%)\n",
            s.reduced,
            s.reduction_researches,
            percent(s.reduction_researches, s.reduced)
        ));
        out.push_str(&format!(
            "  PVS: {} re-searches with the full window\n",
            s.pvs_researches
        ));
        out.push_str("Counts say how often a technique fired, not how many nodes it saved: `ab <name>` measures that.");
        Response::text(out)
    }

    /// `explain <move>`: searches the move and compares it with the engine's choice.
    pub(crate) fn explain(&mut self, args: &str) -> Response {
        let Some(mv) = parse_move(&self.position, args) else {
            return Response::text("Usage: explain <move>   (a legal move, like Nf3 or g1f3)");
        };
        let san = self.position.to_san(mv);
        let limits = self.default_limits();
        let mut engine = self.lab_engine();
        let (best, best_move, mut stats) = self.lab_search(&mut engine, &limits);
        let only = Limits {
            search_moves: vec![mv],
            ..limits
        };
        let (mine, _, mine_stats) = self.lab_search(&mut engine, &only);
        stats.add(&mine_stats);
        self.last_stats = Some(stats);
        let (Some(best), Some(best_move), Some(mine)) = (best, best_move, mine) else {
            return Response::text("The search found nothing to compare.");
        };
        let line = |info: &SearchInfo| {
            format!(
                "{} (depth {}): {}",
                score_text(info.score),
                info.depth,
                pv_san(&self.position, &info.pv)
            )
        };
        let mut out = format!("Engine's choice {}\n", line(&best));
        out.push_str(&format!("Your move {san} {}\n", line(&mine)));
        if best_move == mv {
            out.push_str(&format!("{san} is the move the engine would play."));
        } else {
            match (best.score, mine.score) {
                (cb2026_engine::Score::Centipawns(a), cb2026_engine::Score::Centipawns(b)) => {
                    out.push_str(&format!(
                        "{san} is {:.2} pawns worse than {}.",
                        f64::from(a - b) / 100.0,
                        self.position.to_san(best_move)
                    ));
                }
                _ => out.push_str(&format!(
                    "{san} scores {}, {} scores {}.",
                    score_text(mine.score),
                    self.position.to_san(best_move),
                    score_text(best.score)
                )),
            }
            if mine.pv.len() > 1 {
                let mut after = self.position.clone();
                after.make_move(mv);
                out.push_str(&format!(
                    " The engine's answer to it: {}",
                    pv_san(&after, &mine.pv[1..])
                ));
            }
        }
        Response::text(out)
    }

    /// `eval diff <move>`: which evaluation terms the move changes, from White's side.
    pub(crate) fn eval_diff(&self, args: &str) -> Response {
        let Some(mv) = parse_move(&self.position, args) else {
            return Response::text("Usage: eval diff <move>");
        };
        let params = &self.engine.params;
        let mut after = self.position.clone();
        after.make_move(mv);
        let (t0, t1) = (
            eval::trace(&self.position, params),
            eval::trace(&after, params),
        );
        let blend = |phase: i32, (mg, eg): (i32, i32)| {
            if params.tapered {
                (mg * phase + eg * (eval::MAX_PHASE - phase)) / eval::MAX_PHASE
            } else {
                mg
            }
        };
        let mut out = format!(
            "What {} changes, in pawns, from White's point of view:\n  {:<16}{:>9}{:>9}{:>9}\n",
            self.position.to_san(mv),
            "",
            "Before",
            "After",
            "Change"
        );
        for ((name, [w0, b0]), (_, [w1, b1])) in t0.terms().into_iter().zip(t1.terms()) {
            let before = blend(t0.phase, (w0.0 - b0.0, w0.1 - b0.1));
            let now = blend(t1.phase, (w1.0 - b1.0, w1.1 - b1.1));
            if before != now {
                out.push_str(&format!(
                    "  {name:<16}{:>9}{:>9}{:>9}\n",
                    pawns(before),
                    pawns(now),
                    pawns(now - before)
                ));
            }
        }
        out.push_str(&format!(
            "  {:<16}{:>9}{:>9}{:>9}",
            "Total",
            pawns(t0.total),
            pawns(t1.total),
            pawns(t1.total - t0.total)
        ));
        Response::text(out)
    }

    /// `see <move>`: the static exchange evaluation.
    pub(crate) fn see_move(&self, args: &str) -> Response {
        let Some(mv) = parse_move(&self.position, args) else {
            return Response::text("Usage: see <move>");
        };
        let value = see::see(&self.position, mv);
        let verdict = match value {
            v if v > 0 => "wins material",
            0 => "breaks even",
            _ => "loses material",
        };
        Response::text(format!(
            "SEE of {}: {} ({verdict} once the captures on {} are played out, cheapest piece first)",
            self.position.to_san(mv),
            pawns(value),
            mv.to()
        ))
    }

    /// `tree <depth> [plies]`: the search tree's first plies.
    pub(crate) fn tree(&mut self, args: &str) -> Response {
        let usage = "Usage: tree <depth 1-10> [plies 1-4]";
        let mut words = args.split_whitespace().map(str::parse::<usize>);
        let depth = match words.next() {
            Some(Ok(d)) if (1..=10).contains(&d) => d as u32,
            _ => return Response::text(usage),
        };
        let plies = match words.next() {
            None => TREE_DEFAULT_PLIES,
            Some(Ok(p)) if (1..=4).contains(&p) => p,
            _ => return Response::text(usage),
        };
        if self.position.legal_moves().is_empty() {
            return Response::text("No legal moves.");
        }
        let mut engine = self.lab_engine();
        let (tree, info) = engine.search_tree(
            &self.position,
            &self.history_hashes(),
            depth,
            plies,
            TREE_MAX_EVENTS,
        );
        let mut out = format!(
            "Search tree of the depth-{depth} iteration (the earlier ones ran first), {plies} plies shown.\n\
             Windows and scores are from the side to move's point of view at each node.\n\n"
        );
        out.push_str(&render_tree(&self.position, &tree));
        out.push_str(&format!(
            "\nResult: {} (depth {depth}, {} nodes): {}",
            score_text(info.score),
            info.nodes,
            pv_san(&self.position, &info.pv)
        ));
        Response::text(out)
    }
}

/// A search value for people: pawns, a mate, or ±∞.
fn value_text(value: i32) -> String {
    if value >= INFINITY {
        "+inf".to_string()
    } else if value <= -INFINITY {
        "-inf".to_string()
    } else {
        score_text(to_score(value))
    }
}

/// The position after `path` (moves and null moves) from `root`, and the last move's SAN.
fn replay(root: &Position, path: &[Option<Move>]) -> (Position, String) {
    let mut pos = root.clone();
    let mut san = String::new();
    for step in path {
        match step {
            Some(mv) => {
                san = pos.to_san(*mv);
                pos.make_move(*mv);
            }
            None => {
                san = "(pass)".to_string();
                pos.make_null_move();
            }
        }
    }
    (pos, san)
}

/// The tree as indented lines: one per node searched (with its window, what happened at it
/// and its score) and one per move pruned.
fn render_tree(root: &Position, tree: &SearchTree) -> String {
    let mut lines: Vec<String> = Vec::new();
    // Nodes not yet closed: (path length, line).
    let mut open: Vec<(usize, usize)> = Vec::new();
    // Tags for the next node entered at each path length (reductions and re-searches, which
    // the search notes with the move already on the path).
    let mut pending: Vec<(usize, String)> = Vec::new();
    let indent = |len: usize| "  ".repeat(len.saturating_sub(1));
    for event in &tree.events {
        let len = event.path.len();
        let node_line = |open: &Vec<(usize, usize)>| {
            open.iter()
                .rev()
                .find(|&&(l, _)| l == len)
                .map(|&(_, line)| line)
        };
        let san_of = |mv: Move| replay(root, &event.path).0.to_san(mv);
        let tag = match event.note {
            Note::Enter { depth, alpha, beta } => {
                if len == 0 {
                    continue;
                }
                let (_, san) = replay(root, &event.path);
                let mut line = format!(
                    "{}{san}  depth {depth} [{}, {}]",
                    indent(len),
                    value_text(alpha),
                    value_text(beta)
                );
                for (_, tag) in pending.iter().filter(|(l, _)| *l == len) {
                    line.push_str(&format!(", {tag}"));
                }
                pending.retain(|(l, _)| *l != len);
                lines.push(line);
                open.push((len, lines.len() - 1));
                None
            }
            Note::Exit { score } => {
                while open.last().is_some_and(|&(l, _)| l > len) {
                    open.pop();
                }
                if let Some(&(l, line)) = open.last()
                    && l == len
                {
                    lines[line].push_str(&format!(" -> {}", value_text(score)));
                    open.pop();
                }
                None
            }
            Note::TtCutoff => Some("transposition table cutoff".to_string()),
            Note::Tablebase => Some("tablebase result".to_string()),
            Note::ReverseFutility => Some("reverse futility cutoff".to_string()),
            Note::Razoring => Some("razored".to_string()),
            Note::NullMoveCutoff => Some("null move cutoff".to_string()),
            Note::BetaCutoff(mv, index) => {
                Some(format!("beta cutoff by {} (move {index})", san_of(mv)))
            }
            Note::LateMovePruning(mv) | Note::Futility(mv) | Note::SeePruning(mv) => {
                let why = match event.note {
                    Note::LateMovePruning(_) => "late move pruning",
                    Note::Futility(_) => "futility",
                    _ => "SEE",
                };
                lines.push(format!("{}{}  pruned ({why})", indent(len + 1), san_of(mv)));
                None
            }
            Note::Reduced(_, by) => {
                pending.push((len, format!("reduced {by}")));
                None
            }
            Note::ReductionResearch(_) => {
                pending.push((len, "re-search at full depth".to_string()));
                None
            }
            Note::PvsResearch(_) => {
                pending.push((len, "re-search with full window".to_string()));
                None
            }
            Note::TtHit | Note::NullMove | Note::Quiescence | Note::DeltaPruning(_) => None,
        };
        if let Some(tag) = tag
            && let Some(line) = node_line(&open)
        {
            lines[line].push_str(&format!(" ({tag})"));
        }
    }
    let total = lines.len();
    let mut out: Vec<String> = lines.into_iter().take(TREE_MAX_LINES).collect();
    if total > TREE_MAX_LINES {
        out.push(format!(
            "... {} more lines (try fewer plies or a smaller depth)",
            total - TREE_MAX_LINES
        ));
    }
    if tree.truncated {
        out.push(format!(
            "... the tree had more than {TREE_MAX_EVENTS} decisions; the rest weren't recorded"
        ));
    }
    out.join("\n")
}
