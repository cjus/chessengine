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
//! the rules. It skips escape lines (`%`), accepts move numbers glued to moves (`1.e4`), and
//! drops a variation it can't read rather than the game. A game that doesn't start from the
//! initial position has a `FEN` tag. Only the first game in the text is read. Malformed input
//! gives an error, never a panic.
//!
//! ## Annotations
//!
//! A game read keeps everything annotated in it, and writes it back:
//!
//! - **Comments**, in braces or after `;` to the end of the line. A comment after a move is
//!   that move's ([`PgnGame::comments`]); one before a line's first move is the line's own
//!   ([`PgnGame::intro`], [`Line::intro`]). Commands inside comments (`[%clk 0:04:52]`,
//!   `[%eval 0.31]`, `[%cal Ge2e4]`) are kept as part of the text, and [`commands`] reads
//!   them out of it.
//! - **Numeric annotation glyphs** (NAGs) after a move: `$1` to `$255`, and the symbols that
//!   stand for the common ones, glued to the move or apart: `!`, `?`, `!!`, `??`, `!?`, `?!`
//!   for the move, and `=`, `∞`, `⩲` (or `+=`), `±` (or `+/-`), `+-` and the rest for the
//!   position ([`glyph`], [`nag_of`]). They are written back as `$n`, as the standard's export
//!   format asks.
//! - **Variations** (`(...)`, nested): lines that could have been played instead of a move,
//!   from the position before it ([`Annotations::variations`]). Each is a [`Line`], with
//!   comments, glyphs and variations of its own.
//!
//! [`crate::game::Game`] keeps only the comment after each move: a game read into one and
//! written back loses the rest. Keep the [`PgnGame`] to keep them.
//!
//! **Pitfalls.** A variation's move numbers repeat its parent's (`2. f4` after `2. Nf3`), and
//! a Black move after a comment or a variation needs its number again (`2... Nc6`), or a
//! reader can't tell whose move it is. Variations nest without limit in the standard; reading
//! stops recursing at [`MAX_VARIATION_DEPTH`] and skips anything deeper, so a hostile file
//! can't exhaust the stack.
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

/// How deep variations are read inside one another; deeper ones are skipped.
pub const MAX_VARIATION_DEPTH: usize = 64;

/// The symbols that stand for numeric annotation glyphs, as the standard numbers them, the
/// one shown for each first. The rest are other spellings in use: ASCII for the Unicode ones,
/// and the minus sign or a hyphen.
const GLYPHS: [(&str, u8); 37] = [
    ("!", 1),
    ("?", 2),
    ("!!", 3),
    ("??", 4),
    ("!?", 5),
    ("?!", 6),
    ("□", 7),
    ("=", 10),
    ("∞", 13),
    ("⩲", 14),
    ("⩱", 15),
    ("±", 16),
    ("∓", 17),
    ("+−", 18),
    ("−+", 19),
    ("⨀", 22),
    ("⨀", 23),
    ("⟳", 32),
    ("⟳", 33),
    ("↑", 36),
    ("↑", 37),
    ("→", 40),
    ("→", 41),
    ("⇆", 132),
    ("⇆", 133),
    ("∆", 140),
    ("N", 146),
    ("+=", 14),
    ("=+", 15),
    ("+/-", 16),
    ("-/+", 17),
    ("+-", 18),
    ("-+", 19),
    ("+/=", 14),
    ("=/+", 15),
    ("~", 13),
    ("Δ", 140),
];

/// The symbol shown for numeric annotation glyph `nag` (`$16` is `±`), if it has one.
pub fn glyph(nag: u8) -> Option<&'static str> {
    GLYPHS
        .iter()
        .find(|&&(_, n)| n == nag)
        .map(|&(symbol, _)| symbol)
}

/// The numeric annotation glyph that `symbol` stands for (`±` and `+/-` are `$16`).
pub fn nag_of(symbol: &str) -> Option<u8> {
    GLYPHS
        .iter()
        .find(|&&(s, _)| s == symbol)
        .map(|&(_, nag)| nag)
}

/// What is annotated on a move besides the comment after it: its glyphs, and the lines that
/// could have been played instead.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Annotations {
    /// Numeric annotation glyphs, in order: `$1` (`!`) to `$6` (`?!`) judge the move, `$10`
    /// to `$19` the position, and the standard names the rest up to `$139`.
    pub nags: Vec<u8>,
    /// Lines that could have been played instead of the move, each from the position before
    /// it.
    pub variations: Vec<Line>,
}

impl Annotations {
    /// Whether nothing is annotated.
    pub fn is_empty(&self) -> bool {
        self.nags.is_empty() && self.variations.is_empty()
    }
}

/// A variation: moves from a position, annotated as a game's moves are.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Line {
    /// A comment before its first move.
    pub intro: Option<String>,
    /// Its moves.
    pub moves: Vec<Move>,
    /// A comment after each move, if any. Shorter than `moves` means none after the rest.
    pub comments: Vec<Option<String>>,
    /// Each move's glyphs and variations. Shorter than `moves` means none for the rest.
    pub annotations: Vec<Annotations>,
}

/// A game: its tags, starting position, moves and their comments.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PgnGame {
    /// Tag pairs other than `FEN` and `SetUp` (which come from `start`), in order.
    pub tags: Vec<(String, String)>,
    /// The starting position.
    pub start: Position,
    /// The moves played.
    pub moves: Vec<Move>,
    /// A comment after each move, if any. Shorter than `moves` means none after the rest.
    pub comments: Vec<Option<String>>,
    /// A comment before the first move: about the game.
    pub intro: Option<String>,
    /// Each move's glyphs and variations ([module docs](self#annotations)). Shorter than
    /// `moves` means none for the rest.
    pub annotations: Vec<Annotations>,
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
            intro: None,
            annotations: Vec::new(),
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
        write_line(
            &mut words,
            &self.start,
            self.intro.as_deref(),
            &self.moves,
            &self.comments,
            &self.annotations,
        );
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

        let mut reader = Reader {
            tokens: tokens(&movetext),
            at: 0,
        };
        let (line, result) = reader.line(&game.start, 0)?;
        game.intro = line.intro;
        game.moves = line.moves;
        game.comments = line.comments;
        game.annotations = line.annotations;
        game.result = result.or(tag_result).unwrap_or_else(|| "*".to_string());
        Ok(game)
    }
}

/// Writes a line's movetext as words: its intro, then each move with its number, glyphs,
/// comment and variations.
fn write_line(
    words: &mut Vec<String>,
    start: &Position,
    intro: Option<&str>,
    moves: &[Move],
    comments: &[Option<String>],
    annotations: &[Annotations],
) {
    // Black's moves are numbered at a line's start, and after a comment or a variation.
    let mut number_black = true;
    if let Some(intro) = intro {
        write_comment(words, intro);
    }
    let mut pos = start.clone();
    for (i, &mv) in moves.iter().enumerate() {
        let number = pos.fullmove_number();
        if pos.side_to_move() == Color::White {
            words.push(format!("{number}."));
        } else if number_black {
            words.push(format!("{number}..."));
        }
        words.push(pos.to_san(mv));
        number_black = false;
        let notes = annotations.get(i);
        for nag in notes.map_or(&[][..], |notes| &notes.nags) {
            words.push(format!("${nag}"));
        }
        if let Some(Some(comment)) = comments.get(i) {
            write_comment(words, comment);
            number_black = true;
        }
        for variation in notes.map_or(&[][..], |notes| &notes.variations) {
            let mut inner = Vec::new();
            write_line(
                &mut inner,
                &pos,
                variation.intro.as_deref(),
                &variation.moves,
                &variation.comments,
                &variation.annotations,
            );
            match inner.as_mut_slice() {
                [] => continue,
                [only] => *only = format!("({only})"),
                [first, .., last] => {
                    first.insert(0, '(');
                    last.push(')');
                }
            }
            words.extend(inner);
            number_black = true;
        }
        pos.make_move(mv);
    }
}

/// Writes a comment in braces, as words so that it wraps with the rest. Braces can't nest in
/// PGN comments, so any inside it are left out.
fn write_comment(words: &mut Vec<String>, comment: &str) {
    let clean = comment.replace(['{', '}'], "");
    let parts: Vec<&str> = clean.split_whitespace().collect();
    match parts.as_slice() {
        [] => words.push("{}".to_string()),
        [only] => words.push(format!("{{{only}}}")),
        [first, middle @ .., last] => {
            words.push(format!("{{{first}"));
            words.extend(middle.iter().map(|word| word.to_string()));
            words.push(format!("{last}}}"));
        }
    }
}

/// A piece of movetext.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    /// A comment, `{...}` or `; ...`, its whitespace collapsed.
    Comment(String),
    /// `(`: a variation starts.
    Open,
    /// `)`: it ends.
    Close,
    /// `$n`.
    Nag(u8),
    /// Anything else: a move, a move number, a glyph or a result.
    Word(String),
}

/// Splits movetext into its pieces.
fn tokens(movetext: &str) -> Vec<Token> {
    let collapse = |text: String| text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut tokens = Vec::new();
    let mut chars = movetext.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            '{' => {
                chars.next();
                let comment: String = chars.by_ref().take_while(|&c| c != '}').collect();
                tokens.push(Token::Comment(collapse(comment)));
            }
            ';' => {
                chars.next();
                let comment: String = chars.by_ref().take_while(|&c| c != '\n').collect();
                tokens.push(Token::Comment(collapse(comment)));
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            // Whitespace, and a stray closing brace, are skipped.
            c if c.is_whitespace() || c == '}' => {
                chars.next();
            }
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || "{}();".contains(c) {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                match word.strip_prefix('$').map(str::parse::<u8>) {
                    Some(Ok(nag)) => tokens.push(Token::Nag(nag)),
                    // `$` with no number it can read: skipped, as before glyphs were kept.
                    Some(Err(_)) => {}
                    None => tokens.push(Token::Word(word)),
                }
            }
        }
    }
    tokens
}

/// Reads movetext's [`Token`]s into lines, from the first unread one.
struct Reader {
    tokens: Vec<Token>,
    at: usize,
}

impl Reader {
    /// Reads a line from `start`, `depth` variations deep (0 for the game's moves), up to its
    /// closing `)`, or for the game's moves up to the result or the end. Returns the line and
    /// the result, if one was read.
    fn line(&mut self, start: &Position, depth: usize) -> Result<(Line, Option<String>), String> {
        let mut line = Line::default();
        let mut pos = start.clone();
        // The position before the last move read: its variations start there.
        let mut before = start.clone();
        while let Some(token) = self.tokens.get(self.at).cloned() {
            self.at += 1;
            match token {
                Token::Comment(comment) => {
                    let slot = match line.comments.last_mut() {
                        Some(slot) => slot,
                        None => &mut line.intro,
                    };
                    *slot = Some(match slot.take() {
                        Some(earlier) => format!("{earlier} {comment}"),
                        None => comment,
                    });
                }
                Token::Nag(nag) => {
                    if let Some(notes) = line.annotations.last_mut() {
                        notes.nags.push(nag);
                    }
                }
                Token::Open => {
                    let opened = self.at;
                    let read = (depth < MAX_VARIATION_DEPTH && !line.moves.is_empty())
                        .then(|| self.line(&before, depth + 1));
                    match read {
                        Some(Ok((variation, _))) => {
                            if let Some(notes) = line.annotations.last_mut() {
                                notes.variations.push(variation);
                            }
                        }
                        // A variation it can't read, or before any move, or too deep, is
                        // dropped, not the game.
                        _ => {
                            self.at = opened;
                            self.skip_variation();
                        }
                    }
                }
                Token::Close => {
                    if depth > 0 {
                        return Ok((line, None));
                    }
                    // A stray `)` in the game's moves is skipped.
                }
                Token::Word(word) => {
                    if ["1-0", "0-1", "1/2-1/2", "*"].contains(&word.as_str()) {
                        if depth == 0 {
                            return Ok((line, Some(word)));
                        }
                        continue;
                    }
                    if let Some(nag) = nag_of(&word) {
                        if let Some(notes) = line.annotations.last_mut() {
                            notes.nags.push(nag);
                        }
                        continue;
                    }
                    // A move number, possibly with the move glued on: `12.`, `12...`, `1.e4`.
                    // (Digits alone are a number too; `0-0` is castling.)
                    let digits =
                        word.len() - word.trim_start_matches(|c: char| c.is_ascii_digit()).len();
                    let after = &word[digits..];
                    let san = if digits > 0 && (after.is_empty() || after.starts_with('.')) {
                        after.trim_start_matches('.')
                    } else {
                        word.as_str()
                    };
                    // A glyph glued to the move: `Nf3!?`.
                    let bare = san.trim_end_matches(['!', '?']);
                    let suffix = nag_of(&san[bare.len()..]);
                    if bare.is_empty() {
                        continue;
                    }
                    let mv = pos.parse_san(bare).map_err(|e| {
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
                    before = pos.clone();
                    pos.make_move(mv);
                    line.moves.push(mv);
                    line.comments.push(None);
                    line.annotations.push(Annotations {
                        nags: suffix.into_iter().collect(),
                        variations: Vec::new(),
                    });
                }
            }
        }
        Ok((line, None))
    }

    /// Skips the rest of a variation whose `(` was just read, to its `)` or the end.
    fn skip_variation(&mut self) {
        let mut depth = 1;
        while let Some(token) = self.tokens.get(self.at) {
            self.at += 1;
            match token {
                Token::Open => depth += 1,
                Token::Close => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                return;
            }
        }
    }
}

/// A command embedded in a comment, as the PGN Specification Supplement (Hartwig and
/// Langvad, 2001) writes them and lichess and chess.com export them: `[%clk 0:04:52]` is
/// the command `clk` with the value `0:04:52`. [`Command::clock`] and [`Command::evaluation`]
/// read the two most common; the rest (`%cal` and `%csl` for arrows and squares, `%emt`,
/// engines' own) are left to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    /// Its name, without the `%`: `clk`.
    pub name: String,
    /// Everything after the name, trimmed: `0:04:52`.
    pub value: String,
}

/// An evaluation from a `%eval` command, from White's side.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evaluation {
    /// In hundredths of a pawn: `[%eval -1.25]` is -125.
    Centipawns(i32),
    /// Mate in this many moves, by White if it's positive: `[%eval #-3]` is -3.
    Mate(i32),
}

/// The largest evaluation [`Command::evaluation`] reads, in pawns: anything larger isn't
/// one, and wouldn't fit a whole number of centipawns.
const MAX_EVALUATION_PAWNS: f64 = 100_000.0;

impl Command {
    /// The time on the clock that a `%clk` command says, `h:mm:ss` with an optional
    /// fraction of a second (`0:04:52.3`), or `mm:ss`; `None` for any other command or value.
    pub fn clock(&self) -> Option<std::time::Duration> {
        if self.name != "clk" {
            return None;
        }
        let parts: Vec<&str> = self.value.split(':').collect();
        let (hours, minutes, seconds) = match parts[..] {
            [hours, minutes, seconds] => (Some(hours.parse::<u64>().ok()?), minutes, seconds),
            [minutes, seconds] => (None, minutes, seconds),
            _ => return None,
        };
        let minutes: u64 = minutes.parse().ok()?;
        let seconds: f64 = seconds.parse().ok()?;
        // Without hours, the minutes may run past an hour (`90:00`).
        if (hours.is_some() && minutes >= 60) || !(0.0..60.0).contains(&seconds) {
            return None;
        }
        let hours = hours.unwrap_or(0);
        let whole = hours.checked_mul(3600)?.checked_add(minutes * 60)?;
        Some(std::time::Duration::from_secs(whole) + std::time::Duration::from_secs_f64(seconds))
    }

    /// The evaluation that a `%eval` command says: pawns (`0.31`, `-1.2`) or a mate
    /// (`#3`, `#-2`), optionally followed by the depth (`0.31,20`), which is dropped. `None`
    /// for any other command or value.
    pub fn evaluation(&self) -> Option<Evaluation> {
        if self.name != "eval" {
            return None;
        }
        let value = self.value.split(',').next()?.trim();
        if let Some(mate) = value.strip_prefix('#') {
            return mate.parse().ok().map(Evaluation::Mate);
        }
        let pawns: f64 = value.parse().ok()?;
        (pawns.is_finite() && pawns.abs() <= MAX_EVALUATION_PAWNS)
            .then(|| Evaluation::Centipawns((pawns * 100.0).round() as i32))
    }
}

/// A comment's text without its commands, and the commands, in order:
/// `"[%clk 0:04:52] a good try"` is `"a good try"` and the command `clk`. Text that only
/// looks like the start of one (`[%` with no `]`, or no name) is left as text. The text's
/// spaces are collapsed and trimmed, as a comment read from PGN's are.
pub fn commands(comment: &str) -> (String, Vec<Command>) {
    let mut text = String::new();
    let mut commands = Vec::new();
    let mut rest = comment;
    while let Some(start) = rest.find("[%") {
        let inside = &rest[start + 2..];
        let Some(end) = inside.find(']') else {
            break;
        };
        let body = &inside[..end];
        let (name, value) = body.split_once(char::is_whitespace).unwrap_or((body, ""));
        let named = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
        if !named {
            // Not a command: keep its `[%` as text and look past it.
            text.push_str(&rest[..start + 2]);
            rest = inside;
            continue;
        }
        text.push_str(&rest[..start]);
        text.push(' ');
        commands.push(Command {
            name: name.to_string(),
            value: value.trim().to_string(),
        });
        rest = &inside[end + 1..];
    }
    text.push_str(rest);
    let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
    (text, commands)
}

/// Splits a PGN file into its games' texts (a game starts with a tag line after the previous
/// game's moves).
pub fn split_games(text: &str) -> Vec<&str> {
    let mut games = Vec::new();
    let mut start = 0;
    let mut seen_moves = false;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && seen_moves {
            games.push(&text[start..offset]);
            start = offset;
            seen_moves = false;
        } else if !trimmed.is_empty() && !trimmed.starts_with('[') && !trimmed.starts_with('%') {
            seen_moves = true;
        }
        offset += line.len();
    }
    if !text[start..].trim().is_empty() {
        games.push(&text[start..]);
    }
    games
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

    /// The SAN of `line`'s moves, played from `start`.
    fn sans(start: &Position, line: &Line) -> Vec<String> {
        let mut pos = start.clone();
        line.moves
            .iter()
            .map(|&mv| {
                let san = pos.to_san(mv);
                pos.make_move(mv);
                san
            })
            .collect()
    }

    /// `game`'s moves and annotations, as a [`Line`].
    fn main_line(game: &PgnGame) -> Line {
        Line {
            intro: game.intro.clone(),
            moves: game.moves.clone(),
            comments: game.comments.clone(),
            annotations: game.annotations.clone(),
        }
    }

    const ANNOTATED: &str = "\
[Event \"Annotated\"]

{A game to learn from} 1. e4 $1 {the best by test} e5!? (1... c5 {the Sicilian} 2. Nf3
(2. c3 d5) d6 ; a rest-of-line comment
) 2. Nf3 +/- Nc6 $14 $32 {[%clk 0:04:52] [%eval 0.31]} 3. Bb5 a6 *
";

    #[test]
    fn reads_comments_glyphs_and_variations() {
        let game = PgnGame::parse(ANNOTATED).unwrap();
        let start = Position::startpos();
        assert_eq!(game.intro.as_deref(), Some("A game to learn from"));
        assert_eq!(
            sans(&start, &main_line(&game)),
            ["e4", "e5", "Nf3", "Nc6", "Bb5", "a6"]
        );
        assert_eq!(game.comments[0].as_deref(), Some("the best by test"));
        assert_eq!(game.annotations[0].nags, [1]);
        assert_eq!(game.annotations[1].nags, [5]);
        assert_eq!(game.annotations[2].nags, [16]);
        assert_eq!(game.annotations[3].nags, [14, 32]);
        assert_eq!(
            game.comments[3].as_deref(),
            Some("[%clk 0:04:52] [%eval 0.31]")
        );

        // 1... c5 instead of 1... e5, from the position after 1. e4.
        let [sicilian] = game.annotations[1].variations.as_slice() else {
            panic!("{:?}", game.annotations[1]);
        };
        let mut after_e4 = start.clone();
        after_e4.make_move(game.moves[0]);
        assert_eq!(sans(&after_e4, sicilian), ["c5", "Nf3", "d6"]);
        assert_eq!(sicilian.comments[0].as_deref(), Some("the Sicilian"));
        assert_eq!(
            sicilian.comments[2].as_deref(),
            Some("a rest-of-line comment")
        );
        // 2. c3 inside it, instead of 2. Nf3.
        let [alapin] = sicilian.annotations[1].variations.as_slice() else {
            panic!("{:?}", sicilian.annotations[1]);
        };
        let mut after_c5 = after_e4.clone();
        after_c5.make_move(sicilian.moves[0]);
        assert_eq!(sans(&after_c5, alapin), ["c3", "d5"]);
    }

    #[test]
    fn annotations_are_written_back() {
        let game = PgnGame::parse(ANNOTATED).unwrap();
        let pgn = game.to_pgn();
        assert!(pgn.lines().all(|l| l.len() < LINE_WIDTH), "{pgn}");
        let movetext = pgn.split("\n\n").nth(1).unwrap().replace('\n', " ");
        assert_eq!(
            movetext.trim_end(),
            "{A game to learn from} 1. e4 $1 {the best by test} 1... e5 $5 (1... c5 \
             {the Sicilian} 2. Nf3 (2. c3 d5) 2... d6 {a rest-of-line comment}) 2. Nf3 $16 \
             Nc6 $14 $32 {[%clk 0:04:52] [%eval 0.31]} 3. Bb5 a6 *"
        );
        let back = PgnGame::parse(&pgn).unwrap();
        assert_eq!(main_line(&back), main_line(&game));
        assert_eq!(back.tag("Event"), Some("Annotated"));
    }

    #[test]
    fn a_variation_it_cannot_read_is_dropped_not_the_game() {
        // Kxe8 is illegal there; the variation goes, the game stays.
        let game =
            PgnGame::parse("1. e4 (1. Kxe8 {nonsense} (1. d4)) e5 2. Nf3 (2. Qh5) *").unwrap();
        assert_eq!(game.moves.len(), 3);
        assert!(game.annotations[0].variations.is_empty());
        assert_eq!(game.annotations[2].variations.len(), 1);
        // A variation before any move, and a stray `)`, are skipped.
        let game = PgnGame::parse("(1. d4 d5) 1. e4 ) e5 *").unwrap();
        assert_eq!(game.moves.len(), 2);
        assert!(game.annotations.iter().all(Annotations::is_empty));
    }

    #[test]
    fn variations_nested_too_deep_are_skipped_without_overflowing_the_stack() {
        let depth = 10_000;
        let text = format!(
            "1. e4 {} {} e5 *",
            "(1. d4 ".repeat(depth),
            ")".repeat(depth)
        );
        let game = PgnGame::parse(&text).unwrap();
        assert_eq!(game.moves.len(), 2);
        let mut nested = 0;
        let mut variations = &game.annotations[0].variations;
        while let Some(variation) = variations.first() {
            nested += 1;
            variations = &variation.annotations[0].variations;
        }
        assert_eq!(nested, MAX_VARIATION_DEPTH);
    }

    #[test]
    fn glyphs_and_their_numbers() {
        assert_eq!(glyph(1), Some("!"));
        assert_eq!(glyph(6), Some("?!"));
        assert_eq!(glyph(16), Some("±"));
        assert_eq!(glyph(14), Some("⩲"));
        assert_eq!(glyph(200), None);
        // A symbol for both sides (`⨀`, zugzwang, is $22 for White and $23 for Black) reads
        // as White's.
        for (symbol, nag) in GLYPHS {
            assert!(
                nag_of(symbol).is_some_and(|read| read == nag || read + 1 == nag),
                "{symbol}"
            );
            assert!(glyph(nag).is_some(), "{nag}");
        }
        assert_eq!(nag_of("⨀"), Some(22));
        assert_eq!(nag_of("+/-"), Some(16));
        assert_eq!(nag_of("e4"), None);
    }

    #[test]
    fn splits_files_into_games() {
        let text = "[Event \"a\"]\n\n1. e4 *\n\n[Event \"b\"]\n[Round \"2\"]\n1. d4 d5\n2. c4 *\n";
        let games = split_games(text);
        assert_eq!(games.len(), 2);
        assert_eq!(PgnGame::parse(games[1]).unwrap().moves.len(), 3);
        assert!(split_games("").is_empty());
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

    #[test]
    fn commands_are_read_out_of_a_comment() {
        let (text, found) = commands("[%eval 0.17] [%clk 0:04:52]  a good try [%cal Ge2e4,Rd1d8]");
        assert_eq!(text, "a good try");
        let names: Vec<&str> = found.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["eval", "clk", "cal"]);
        assert_eq!(found[2].value, "Ge2e4,Rd1d8");
        assert_eq!(found[0].evaluation(), Some(Evaluation::Centipawns(17)));
        assert_eq!(found[0].clock(), None);
        assert_eq!(
            found[1].clock(),
            Some(std::time::Duration::from_secs(4 * 60 + 52))
        );
        // Text between commands stays, with its spaces collapsed; nothing but commands is
        // no text.
        assert_eq!(commands("before [%clk 1:00:00] after").0, "before after");
        assert_eq!(commands("[%clk 1:00:00]").0, "");
        // What only looks like a command is text.
        for text in ["[% clk 1:00]", "[%clk 1:00", "50 [%] off", "a [%?] b"] {
            let (kept, found) = commands(text);
            assert!(found.is_empty(), "{text}");
            assert_eq!(kept, text.split_whitespace().collect::<Vec<_>>().join(" "));
        }
        // A broken one doesn't hide a good one after it.
        let (text, found) = commands("[%] [%clk 0:01:00] x");
        assert_eq!((text.as_str(), found.len()), ("[%] x", 1));
    }

    #[test]
    fn clocks_and_evaluations_read_as_the_supplement_writes_them() {
        let command = |name: &str, value: &str| Command {
            name: name.into(),
            value: value.into(),
        };
        let clock = |value| command("clk", value).clock();
        let ms = std::time::Duration::from_millis;
        assert_eq!(clock("1:02:03"), Some(ms(3_723_000)));
        assert_eq!(clock("0:00:09.5"), Some(ms(9_500)));
        assert_eq!(clock("90:00"), Some(ms(5_400_000)));
        for bad in ["", "1:60:00", "0:00:60", "a:00:00", "1:2:3:4", "-1:00"] {
            assert_eq!(clock(bad), None, "{bad}");
        }
        assert_eq!(command("emt", "0:00:03").clock(), None);

        let eval = |value| command("eval", value).evaluation();
        assert_eq!(eval("0.31"), Some(Evaluation::Centipawns(31)));
        assert_eq!(eval("-1.25,24"), Some(Evaluation::Centipawns(-125)));
        assert_eq!(eval("#3"), Some(Evaluation::Mate(3)));
        assert_eq!(eval("#-2"), Some(Evaluation::Mate(-2)));
        for bad in ["", "#", "inf", "NaN", "1e9", "#x"] {
            assert_eq!(eval(bad), None, "{bad}");
        }
        assert_eq!(command("clk", "0.31").evaluation(), None);
    }
}
