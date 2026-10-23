//! Opening books from people's games, for the `make-book` tool: games in PGN in, a Polyglot
//! book out, for `BookFile` ([`cb2026_engine::book`]).
//!
//! The tool reads the games one at a time and keeps those that pass its filters ([`accepts`]):
//! both players rated at least `min_rating`, one of the chosen speeds, no bots, standard chess
//! from the usual start, and a game that was played out. For every position in the first
//! `plies` plies of each game it counts the move played there, and how it scored for the side
//! that played it. Positions are found by their Polyglot key, so the counts of transpositions
//! add up. A move goes into the book if at least `min_games` games played it and it scored at
//! least `min_score`. Its weight is the points it scored, two for a win and one for a draw,
//! so the engine plays popular moves that do well most often, and a move nobody wins with not
//! at all. PolyGlot's `make-book` (Fabien Letouzey, GPL) weighs moves the same way (the idea
//! only; the code is cb2026's own).
//!
//! ## Where the games come from
//!
//! Lichess publishes every rated game it hosts, a month to a file, at
//! <https://database.lichess.org/>, under the Creative Commons CC0 licence, so a book made from
//! them can be shared under any terms. A month is about 90 million games and 30 GB compressed
//! with zstd. The tool reads the games from standard input, so the file is decompressed on
//! the way in and never stored, and it stops reading when it has used `max_games` games.
//!
//! ## Pitfalls
//!
//! - **Bots.** Lichess's files include the games of BOT accounts, engines that play each other
//!   at ratings up to 3000 and more. In the 150,000 games at the start of September 2026,
//!   bots played most of the games where both sides were rated 2600 or more. A book of their
//!   moves would be an engine book. Bots have the title `BOT`, and their games are skipped.
//! - **Ratings.** Lichess's ratings run higher than FIDE's: 2200 on Lichess is a good club
//!   player, not a master. Both players must be strong, not just one, or the weaker player's
//!   moves come in too.
//! - **Results come from the whole game.** A blitz game is often decided long after the
//!   opening, so a move's score means little until it has been played in many games. That is
//!   what `min_games` is for.
//! - **Memory.** Every position and move seen is counted until the end, when the rare ones are
//!   dropped: about 20 million pairs for a million games of 20 plies, a gigabyte or so.
//!
//! See the Chess Programming Wiki, [Opening Book](https://www.chessprogramming.org/Opening_Book).

use std::collections::HashMap;
use std::io::BufRead;

use cb2026_chess::Position;
use cb2026_chess::pgn::PgnGame;
use cb2026_engine::book::{OpeningBook, encode_move, polyglot_key};

use crate::mistakes::speed;

/// Which games to use and which moves to keep.
#[derive(Clone, Debug, PartialEq)]
pub struct BookSettings {
    /// Both players must be rated at least this (0: ratings aren't checked).
    pub min_rating: u32,
    /// The speeds to use, Lichess's names for a game's estimated length ([`speed`]); empty
    /// for any game, timed or not.
    pub speeds: Vec<String>,
    /// How many plies of each game to count.
    pub plies: usize,
    /// A move goes into the book only if at least this many games played it.
    pub min_games: u32,
    /// A move goes into the book only if it scored at least this, from 0 to 1, for the side
    /// that played it.
    pub min_score: f64,
    /// Stop reading after using this many games.
    pub max_games: usize,
}

impl Default for BookSettings {
    fn default() -> BookSettings {
        BookSettings {
            min_rating: 2200,
            speeds: ["blitz", "rapid", "classical"].map(String::from).to_vec(),
            plies: 20,
            min_games: 20,
            min_score: 0.35,
            max_games: usize::MAX,
        }
    }
}

/// What a move scored, from the side that played it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MoveStats {
    /// Games won after it.
    pub wins: u32,
    /// Games drawn.
    pub draws: u32,
    /// Games lost.
    pub losses: u32,
}

impl MoveStats {
    /// The games that played it.
    pub fn games(&self) -> u32 {
        self.wins + self.draws + self.losses
    }

    /// Its points, two for a win and one for a draw: its weight in the book.
    pub fn points(&self) -> u32 {
        2 * self.wins + self.draws
    }

    /// Its score, from 0 (lost every game) to 1 (won every game).
    pub fn score(&self) -> f64 {
        f64::from(self.points()) / f64::from(2 * self.games().max(1))
    }
}

/// The moves counted so far, by position key and move, and how many games were read.
#[derive(Clone, Debug, Default)]
pub struct BookCounts {
    /// What each move scored, by Polyglot key and Polyglot move.
    pub moves: HashMap<(u64, u16), MoveStats>,
    /// The games read.
    pub games_read: usize,
    /// The games used: they passed the filters and their moves were read.
    pub games_used: usize,
}

impl BookCounts {
    // ANCHOR: count
    /// Counts the first `plies` plies of `game`, which ended `result` ("1-0", "0-1" or
    /// "1/2-1/2").
    pub fn add_game(&mut self, game: &PgnGame, result: &str, plies: usize) {
        // Points for White and for Black: 2 for a win, 1 for a draw, 0 for a loss.
        let points = match result {
            "1-0" => [2, 0],
            "0-1" => [0, 2],
            _ => [1, 1],
        };
        let mut pos = game.start.clone();
        for &mv in game.moves.iter().take(plies) {
            let stats = self
                .moves
                .entry((polyglot_key(&pos), encode_move(mv)))
                .or_default();
            match points[pos.side_to_move().index()] {
                2 => stats.wins += 1,
                1 => stats.draws += 1,
                _ => stats.losses += 1,
            }
            pos.make_move(mv);
        }
        self.games_used += 1;
    }
    // ANCHOR_END: count

    // ANCHOR: build
    /// The book: the moves played in at least `min_games` games that scored at least
    /// `min_score` and won some points, each weighted by its points (scaled down where a
    /// position's points don't fit the format's 16 bits).
    pub fn book(&self, settings: &BookSettings) -> OpeningBook {
        let mut by_position: HashMap<u64, Vec<(u16, u32)>> = HashMap::new();
        for (&(key, mv), stats) in &self.moves {
            if stats.games() >= settings.min_games
                && stats.score() >= settings.min_score
                && stats.points() > 0
            {
                by_position
                    .entry(key)
                    .or_default()
                    .push((mv, stats.points()));
            }
        }
        let mut entries = Vec::new();
        for (key, moves) in by_position {
            let most = moves.iter().map(|&(_, points)| points).max().unwrap_or(0);
            let scale = f64::from(u16::MAX) / f64::from(most.max(u32::from(u16::MAX)));
            for (mv, points) in moves {
                let weight = (f64::from(points) * scale).round().max(1.0) as u16;
                entries.push((key, mv, weight));
            }
        }
        OpeningBook::from_entries(entries)
    }
    // ANCHOR_END: build
}

/// Whether the game with these tags passes the filters, and if it does, its result.
pub fn accepts<'a>(tags: &'a HashMap<String, String>, settings: &BookSettings) -> Option<&'a str> {
    let tag = |name: &str| tags.get(name).map(String::as_str);
    let standard = tag("Variant").is_none_or(|v| v == "Standard") && tag("FEN").is_none();
    let bot = tag("WhiteTitle") == Some("BOT") || tag("BlackTitle") == Some("BOT");
    // Abandoned games, games stopped for cheating and games the server lost aren't played out.
    let played_out = !matches!(
        tag("Termination"),
        Some("Abandoned" | "Rules infraction" | "Unterminated")
    );
    let rated = |name: &str| {
        tag(name)
            .and_then(|r| r.parse::<u32>().ok())
            .is_some_and(|r| r >= settings.min_rating)
    };
    let strong = settings.min_rating == 0 || (rated("WhiteElo") && rated("BlackElo"));
    let speed_ok = settings.speeds.is_empty()
        || tag("TimeControl")
            .and_then(speed)
            .is_some_and(|s| settings.speeds.iter().any(|wanted| wanted == s));
    let result = tag("Result").filter(|r| ["1-0", "0-1", "1/2-1/2"].contains(r))?;
    (standard && !bot && played_out && strong && speed_ok).then_some(result)
}

/// Counts the moves of the games in `pgn` that pass the filters, until `max_games` are used.
/// `progress` is called every million games read.
pub fn count_games(
    mut pgn: impl BufRead,
    settings: &BookSettings,
    progress: &mut dyn FnMut(&BookCounts),
) -> BookCounts {
    let mut counts = BookCounts::default();
    let mut tags = HashMap::new();
    let mut movetext = String::new();
    // Whether the game being read is used, with its result: decided at its first line of moves.
    let mut wanted: Option<Option<String>> = None;
    let mut line = String::new();
    loop {
        line.clear();
        // A read error ends the input like its end does.
        let end = !matches!(pgn.read_line(&mut line), Ok(n) if n > 0);
        let trimmed = line.trim();
        // A game ends at a blank line after its moves, at the next game's tags, or at the end.
        let next_tags = trimmed.starts_with('[') && wanted.is_some();
        if end || next_tags || (trimmed.is_empty() && wanted.is_some()) {
            if wanted.is_some() {
                if let Some(Some(result)) = wanted.take()
                    && let Ok(game) = PgnGame::parse(&movetext)
                {
                    counts.add_game(&game, &result, settings.plies);
                }
                counts.games_read += 1;
                if counts.games_read.is_multiple_of(1_000_000) {
                    progress(&counts);
                }
                tags.clear();
                movetext.clear();
                if counts.games_used >= settings.max_games {
                    break;
                }
            }
            if end {
                break;
            }
            if !next_tags {
                continue;
            }
        }
        if trimmed.is_empty() || trimmed.starts_with('%') {
            continue;
        }
        if wanted.is_none() && trimmed.starts_with('[') {
            if let Some((name, value)) = trimmed
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split_once(' ')
            {
                tags.insert(name.to_string(), value.trim_matches('"').to_string());
            }
            continue;
        }
        let used = wanted.get_or_insert_with(|| accepts(&tags, settings).map(str::to_string));
        if used.is_some() {
            movetext.push_str(&line);
        }
    }
    counts
}

/// The book's moves for `pos` with their share of the weight, heaviest first, in SAN: what the
/// tool prints for the starting position.
pub fn shares(book: &OpeningBook, pos: &Position) -> Vec<(String, f64)> {
    let moves = book.moves(pos);
    let total: f64 = moves.iter().map(|m| f64::from(m.weight)).sum();
    moves
        .iter()
        .map(|m| (pos.to_san(m.mv), f64::from(m.weight) / total.max(1.0)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A game in Lichess's layout: tags, a blank line, the moves on one line, a blank line.
    fn lichess_game(white: u32, black: u32, extra: &str, moves: &str, result: &str) -> String {
        format!(
            "[Event \"Rated Blitz game\"]\n[White \"a\"]\n[Black \"b\"]\n[Result \"{result}\"]\n\
             [WhiteElo \"{white}\"]\n[BlackElo \"{black}\"]\n[TimeControl \"300+0\"]\n{extra}\
             [Termination \"Normal\"]\n\n{moves} {result}\n\n"
        )
    }

    fn settings(min_games: u32) -> BookSettings {
        BookSettings {
            min_games,
            ..BookSettings::default()
        }
    }

    fn book_from(pgn: &str, settings: &BookSettings) -> (OpeningBook, BookCounts) {
        let counts = count_games(pgn.as_bytes(), settings, &mut |_| {});
        (counts.book(settings), counts)
    }

    #[test]
    fn counts_moves_and_weighs_them_by_points() {
        let pgn = [
            lichess_game(
                2300,
                2300,
                "",
                "1. e4 { [%clk 0:05:00] } 1... c5 2. Nf3",
                "1-0",
            ),
            lichess_game(2300, 2300, "", "1. e4 e5 2. Nf3", "1/2-1/2"),
            lichess_game(2300, 2300, "", "1. d4 d5", "0-1"),
        ]
        .concat();
        let (book, counts) = book_from(&pgn, &settings(1));
        assert_eq!((counts.games_read, counts.games_used), (3, 3));
        let start = Position::startpos();
        let weights: Vec<String> = book
            .moves(&start)
            .iter()
            .map(|m| format!("{} {}", m.mv, m.weight))
            .collect();
        // e4 won one game and drew one; d4 lost its only game, so it isn't in the book.
        assert_eq!(weights, ["e2e4 3"]);
        let mut after_e4 = start.clone();
        after_e4.make_move(start.parse_san("e4").unwrap());
        let replies = shares(&book, &after_e4);
        // Black lost after c5 and drew after e5.
        assert_eq!(replies, [("e5".to_string(), 1.0)]);
    }

    #[test]
    fn transpositions_add_up() {
        // The two games reach the same position by different orders, and play e6 there.
        let pgn = [
            lichess_game(2300, 2300, "", "1. Nf3 Nf6 2. d4 e6", "1-0"),
            lichess_game(2300, 2300, "", "1. d4 Nf6 2. Nf3 e6", "1-0"),
        ]
        .concat();
        let (book, _) = book_from(&pgn, &settings(2));
        assert_eq!(book.len(), 0, "e6 lost both times");
        let pgn = pgn.replace("\"1-0\"", "\"0-1\"").replace(" 1-0", " 0-1");
        let (book, _) = book_from(&pgn, &settings(2));
        assert_eq!(book.len(), 1, "e6 won both times, from the same position");
    }

    #[test]
    fn filters_games() {
        let moves = "1. e4 e5";
        let pgn = [
            lichess_game(2300, 2100, "", moves, "1-0"),
            lichess_game(2300, 2300, "[BlackTitle \"BOT\"]\n", moves, "1-0"),
            lichess_game(2300, 2300, "[Variant \"Chess960\"]\n", moves, "1-0"),
            lichess_game(2300, 2300, "", moves, "*"),
            lichess_game(2300, 2300, "", moves, "1-0").replace("300+0", "60+0"),
            lichess_game(2300, 2300, "", moves, "1-0").replace("Normal", "Abandoned"),
            lichess_game(2300, 2300, "", moves, "1-0"),
        ]
        .concat();
        let (_, counts) = book_from(&pgn, &settings(1));
        assert_eq!((counts.games_read, counts.games_used), (7, 1));
        assert_eq!(counts.moves.len(), 2);
    }

    #[test]
    fn reads_moves_over_several_lines_and_stops_at_max_games() {
        let game = "[Result \"1-0\"]\n[WhiteElo \"2400\"]\n[BlackElo \"2400\"]\n\
                    [TimeControl \"600+5\"]\n\n1. e4 e5\n2. Nf3 Nc6\n3. Bb5 1-0\n";
        let pgn = format!("{game}\n{game}{game}");
        let settings = BookSettings {
            plies: 5,
            min_games: 3,
            ..BookSettings::default()
        };
        let (book, counts) = book_from(&pgn, &settings);
        assert_eq!(counts.games_used, 3);
        assert_eq!(book.len(), 3, "White's three moves won three times");
        let settings = BookSettings {
            max_games: 2,
            ..settings
        };
        let (_, counts) = book_from(&pgn, &settings);
        assert_eq!((counts.games_read, counts.games_used), (2, 2));
    }

    #[test]
    fn weights_fit_in_sixteen_bits() {
        let mut counts = BookCounts::default();
        let key = polyglot_key(&Position::startpos());
        let mv = |uci: &str| encode_move(Position::startpos().parse_uci_move(uci).unwrap());
        let big = MoveStats {
            wins: 100_000,
            draws: 0,
            losses: 0,
        };
        let small = MoveStats {
            wins: 10_001,
            draws: 0,
            losses: 0,
        };
        counts.moves.insert((key, mv("e2e4")), big);
        counts.moves.insert((key, mv("d2d4")), small);
        let book = counts.book(&settings(1));
        let weights: Vec<u16> = book
            .moves(&Position::startpos())
            .iter()
            .map(|m| m.weight)
            .collect();
        assert_eq!(weights, [u16::MAX, 6554]);
    }
}
