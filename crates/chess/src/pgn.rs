//! Portable Game Notation (PGN): how chess games are stored and exchanged.
//!
//! A PGN game is a list of **tags** (`[White "Carlos"]`), then the **movetext**: the moves in
//! SAN with move numbers, optional comments in braces, and the result:
//!
//! ```text
//! [Event "Casual game"]
//! [White "Carlos"]
//! [Black "cb2026"]
//! [Result "0-1"]
//!
//! 1. f3 e5 2. g4 {a blunder} Qh4# 0-1
//! ```
//!
//! Writing is strict: the seven required tags first (Event, Site, Date, Round, White, Black,
//! Result), lines under 80 characters. Reading is forgiving, because PGN from the wild bends
//! the rules. It skips comments (`{...}` and `; ...`), variations (`(...)`, nested), numeric
//! annotations (`$1`), annotation marks (`!?`) and escape lines (`%`), and accepts move numbers
//! glued to moves (`1.e4`). A game that doesn't start from the initial position has a `FEN`
//! tag. Only the first game in the text is read. Malformed input gives an error, never a panic.
//!
//! See the [PGN standard](https://www.saremba.de/chessgml/standards/pgn/pgn-complete.htm) and the
//! Chess Programming Wiki, [Portable Game Notation](https://www.chessprogramming.org/Portable_Game_Notation).

use crate::moves::Move;
use crate::piece::Color;
use crate::position::Position;

/// The tags every PGN game has, in the order they must come.
const SEVEN_TAG_ROSTER: [&str; 7] = ["Event", "Site", "Date", "Round", "White", "Black", "Result"];

/// Lines in written PGN stay shorter than this.
const LINE_WIDTH: usize = 80;

/// A game: its tags, starting position, moves and their comments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PgnGame {
    /// Tag pairs other than `FEN` and `SetUp` (which come from `start`), in order.
    pub tags: Vec<(String, String)>,
    /// The starting position.
    pub start: Position,
    /// The moves played.
    pub moves: Vec<Move>,
    /// A comment after each move, if any (the same length as `moves`).
    pub comments: Vec<Option<String>>,
    /// The result: `1-0`, `0-1`, `1/2-1/2`, or `*` for a game still going.
    pub result: String,
}

impl PgnGame {
    /// A game from `start` with no moves yet.
    pub fn new(start: Position) -> PgnGame {
        PgnGame {
            tags: Vec::new(),
            start,
            moves: Vec::new(),
            comments: Vec::new(),
            result: "*".to_string(),
        }
    }

    /// The value of tag `name`, if present.
    pub fn tag(&self, name: &str) -> Option<&str> {
        self.tags
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Sets tag `name` to `value`, replacing an earlier value.
    pub fn set_tag(&mut self, name: &str, value: &str) {
        match self.tags.iter_mut().find(|(n, _)| n == name) {
            Some(tag) => tag.1 = value.to_string(),
            None => self.tags.push((name.to_string(), value.to_string())),
        }
    }

    /// The game in PGN.
    pub fn to_pgn(&self) -> String {
        let mut out = String::new();
        let mut tag = |name: &str, value: &str| {
            let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
            out.push_str(&format!("[{name} \"{escaped}\"]\n"));
        };
        for name in SEVEN_TAG_ROSTER {
            let value = match name {
                "Result" => self.result.as_str(),
                "Date" => self.tag(name).unwrap_or("????.??.??"),
                _ => self.tag(name).unwrap_or("?"),
            };
            tag(name, value);
        }
        if self.start != Position::startpos() {
            tag("SetUp", "1");
            tag("FEN", &self.start.to_fen());
        }
        for (name, value) in &self.tags {
            if !SEVEN_TAG_ROSTER.contains(&name.as_str()) && name != "SetUp" && name != "FEN" {
                tag(name, value);
            }
        }
        out.push('\n');

        let mut words = Vec::new();
        let mut pos = self.start.clone();
        for (i, &mv) in self.moves.iter().enumerate() {
            let number = pos.fullmove_number();
            if pos.side_to_move() == Color::White {
                words.push(format!("{number}."));
            } else if i == 0 {
                words.push(format!("{number}..."));
            }
            words.push(pos.to_san(mv));
            if let Some(Some(comment)) = self.comments.get(i) {
                // Braces can't nest in PGN comments.
                let clean = comment.replace(['{', '}'], "");
                words.push(format!("{{{clean}}}"));
            }
            pos.make_move(mv);
        }
        words.push(self.result.clone());
        let mut line = String::new();
        for word in words {
            if !line.is_empty() && line.len() + 1 + word.len() >= LINE_WIDTH {
                out.push_str(&line);
                out.push('\n');
                line.clear();
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(&word);
        }
        out.push_str(&line);
        out.push('\n');
        out
    }

    /// Reads the first game in `text`.
    pub fn parse(text: &str) -> Result<PgnGame, String> {
        let mut tags = Vec::new();
        let mut movetext = String::new();
        let mut in_movetext = false;
        for line in text.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('%') {
                continue;
            }
            if !in_movetext && trimmed.starts_with('[') {
                tags.push(parse_tag(trimmed)?);
                continue;
            }
            if trimmed.is_empty() {
                continue;
            }
            if in_movetext && trimmed.starts_with('[') {
                // The next game's tags: only the first game is read.
                break;
            }
            in_movetext = true;
            movetext.push_str(line);
            movetext.push('\n');
        }
        let start = match tags.iter().find(|(n, _)| n == "FEN") {
            Some((_, fen)) => {
                Position::from_fen(fen).map_err(|e| format!("bad FEN tag \"{fen}\": {e}"))?
            }
            None => Position::startpos(),
        };
        let mut game = PgnGame::new(start);
        game.tags = tags
            .into_iter()
            .filter(|(n, _)| n != "FEN" && n != "SetUp")
            .collect();
        let tag_result = game.tag("Result").map(str::to_string);

        let mut pos = game.start.clone();
        let mut chars = movetext.chars().peekable();
        let mut result = None;
        while let Some(&c) = chars.peek() {
            match c {
                '{' => {
                    chars.next();
                    let comment: String = chars.by_ref().take_while(|&c| c != '}').collect();
                    if let Some(last) = game.comments.last_mut() {
                        let comment = comment.split_whitespace().collect::<Vec<_>>().join(" ");
                        *last = Some(match last.take() {
                            Some(earlier) => format!("{earlier} {comment}"),
                            None => comment,
                        });
                    }
                }
                ';' => {
                    chars.by_ref().take_while(|&c| c != '\n').for_each(drop);
                }
                '(' => {
                    let mut depth = 0;
                    for c in chars.by_ref() {
                        match c {
                            '(' => depth += 1,
                            ')' => depth -= 1,
                            _ => {}
                        }
                        if depth == 0 {
                            break;
                        }
                    }
                }
                // Whitespace, and stray closing brackets (with no opening one), are skipped.
                c if c.is_whitespace() || c == ')' || c == '}' => {
                    chars.next();
                }
                _ => {
                    let mut token = String::new();
                    while let Some(&c) = chars.peek() {
                        if c.is_whitespace() || "{}();".contains(c) {
                            break;
                        }
                        token.push(c);
                        chars.next();
                    }
                    if let Some(r) = ["1-0", "0-1", "1/2-1/2", "*"]
                        .into_iter()
                        .find(|&r| token == r)
                    {
                        result = Some(r.to_string());
                        break;
                    }
                    if token.starts_with('$') {
                        continue;
                    }
                    // A move number, possibly with the move glued on: `12.`, `12...`, `1.e4`.
                    // (Digits alone are a number too; `0-0` is castling.)
                    let digits =
                        token.len() - token.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                    let after = &token[digits..];
                    let san = if digits > 0 && (after.is_empty() || after.starts_with('.')) {
                        after.trim_start_matches('.')
                    } else {
                        token.as_str()
                    };
                    if san.is_empty() || san.chars().all(|c| "!?".contains(c)) {
                        continue;
                    }
                    let mv = pos.parse_san(san).map_err(|e| {
                        format!(
                            "move {}{} {san}: {e:?}",
                            pos.fullmove_number(),
                            if pos.side_to_move() == Color::White {
                                "."
                            } else {
                                "..."
                            }
                        )
                    })?;
                    pos.make_move(mv);
                    game.moves.push(mv);
                    game.comments.push(None);
                }
            }
        }
        game.result = result.or(tag_result).unwrap_or_else(|| "*".to_string());
        Ok(game)
    }
}

/// Reads a tag pair line, `[Name "Value"]`.
fn parse_tag(line: &str) -> Result<(String, String), String> {
    let inner = line
        .strip_prefix('[')
        .and_then(|l| l.strip_suffix(']'))
        .ok_or_else(|| format!("bad tag line: {line}"))?
        .trim();
    let (name, rest) = inner
        .split_once(char::is_whitespace)
        .ok_or_else(|| format!("bad tag line: {line}"))?;
    let rest = rest.trim();
    let quoted = rest
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .ok_or_else(|| format!("tag value not in quotes: {line}"))?;
    let mut value = String::new();
    let mut escaped = false;
    for c in quoted.chars() {
        if escaped || c != '\\' {
            value.push(c);
            escaped = false;
        } else {
            escaped = true;
        }
    }
    Ok((name.to_string(), value))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(moves: &[&str]) -> PgnGame {
        let mut game = PgnGame::new(Position::startpos());
        let mut pos = Position::startpos();
        for uci in moves {
            let mv = pos.parse_uci_move(uci).unwrap();
            pos.make_move(mv);
            game.moves.push(mv);
            game.comments.push(None);
        }
        game
    }

    #[test]
    fn writes_the_seven_tags_and_numbered_moves() {
        let mut game = play(&["f2f3", "e7e5", "g2g4", "d8h4"]);
        game.set_tag("White", "Fool");
        game.set_tag("Black", "cb2026 \"the engine\"");
        game.comments[3] = Some("+M1/1".to_string());
        game.result = "0-1".to_string();
        let pgn = game.to_pgn();
        assert!(pgn.starts_with("[Event \"?\"]\n[Site \"?\"]\n[Date \"????.??.??\"]\n"));
        assert!(
            pgn.contains("[Black \"cb2026 \\\"the engine\\\"\"]"),
            "{pgn}"
        );
        assert!(
            pgn.ends_with("\n1. f3 e5 2. g4 Qh4# {+M1/1} 0-1\n"),
            "{pgn}"
        );
        assert!(!pgn.contains("FEN"));
    }

    #[test]
    fn round_trips() {
        let mut game = play(&["e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4"]);
        game.set_tag("Event", "Test");
        game.comments[1] = Some("the Sicilian".to_string());
        game.result = "*".to_string();
        let back = PgnGame::parse(&game.to_pgn()).unwrap();
        assert_eq!(back.moves, game.moves);
        assert_eq!(back.comments, game.comments);
        assert_eq!(back.tag("Event"), Some("Test"));
        assert_eq!(back.result, "*");
    }

    #[test]
    fn long_games_wrap_lines() {
        let mut moves = Vec::new();
        for _ in 0..10 {
            moves.extend(["g1f3", "g8f6", "f3g1", "f6g8"]);
        }
        let pgn = play(&moves).to_pgn();
        assert!(pgn.lines().all(|l| l.len() < LINE_WIDTH), "{pgn}");
    }

    #[test]
    fn reads_wild_pgn() {
        let text = "\
[Event \"Wild\"]
[FEN \"rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1\"]
[SetUp \"1\"]
% an escape line
1... e5 $1 {open game} 2.Nf3! (2. f4 exf4 (2... d5)) Nc6 ; a rest-of-line comment
3. Bb5 a6!? 1/2-1/2

[Event \"Second game\"]
1. d4 *
";
        assert_eq!(
            PgnGame::parse("1. e4 e5 2. Nf3 Nc6 3. Bc4 Bc5 4. 0-0 *")
                .unwrap()
                .moves
                .len(),
            7
        );
        let game = PgnGame::parse(text).unwrap();
        assert_eq!(game.moves.len(), 5);
        assert_eq!(game.comments[0].as_deref(), Some("open game"));
        assert_eq!(game.result, "1/2-1/2");
        assert_eq!(game.tag("Event"), Some("Wild"));
        assert_eq!(game.tag("SetUp"), None);
        let mut pos = game.start.clone();
        for &mv in &game.moves {
            pos.make_move(mv);
        }
        assert_eq!(
            pos.to_fen(),
            "r1bqkbnr/1ppp1ppp/p1n5/1B2p3/4P3/5N2/PPPP1PPP/RNBQK2R w KQkq - 0 4"
        );
    }

    #[test]
    fn bad_input_is_an_error_not_a_panic() {
        for text in [
            "1. e5",
            "[Event \"unclosed",
            "[FEN \"not a fen\"]\n1. e4",
            "1. e4 e5 2. Qxf7",
            "{{{",
            "((((",
            "1....",
            "[]",
            "[Event]",
        ] {
            let _ = PgnGame::parse(text);
        }
        // Random jumbles of PGN pieces (a cheap fuzz test).
        let pieces = [
            "1.", "e4", "e5", "Nf3", "{", "}", "(", ")", ";", "\n", "[", "]", "\"", "$3", "0-0",
            "O-O-O", "1-0", "*", "...", "é", "12", "=Q", "x", "+", "#", "%", " ", "\\",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..5_000 {
            let mut text = String::new();
            for _ in 0..(seed % 24) {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                text.push_str(pieces[(seed % pieces.len() as u64) as usize]);
            }
            let _ = PgnGame::parse(&text);
        }
        assert!(PgnGame::parse("1. e4 e5 2. Ke3").is_err());
        assert!(PgnGame::parse("[White \"x\"").is_err());
    }
}
