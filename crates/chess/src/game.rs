//! A game of chess: where it started, the moves played, and whether it is over.
//!
//! [`Game`] is the record a program plays a game on: the shell, the match tool and Solrac Chess
//! AI all keep their games in one. It holds the starting position, every move played with its
//! SAN, its comment and the position before it, and the current position. Each move is checked
//! against the legal moves before it is played, so a `Game` can never hold an illegal move.
//!
//! ## When is a game over?
//!
//! After every move (and every takeback) the game looks at the new position:
//!
//! - **Checkmate**: no legal moves, in check.
//! - **Stalemate**: no legal moves, not in check.
//! - **The fifty-move rule**: 100 half-moves without a capture or a pawn move.
//! - **Insufficient material**: neither side can possibly mate
//!   ([`Position::is_insufficient_material`]).
//! - **Threefold repetition**: the position has occurred twice before, with the same side to
//!   move, castling rights and en passant square (compared by hash key).
//!
//! Checkmate is checked first, so a mate on the hundredth half-move is still a mate.
//!
//! A game can also end off the board, when a player resigns ([`Game::resign`]), the players
//! agree a draw ([`Game::agree_draw`]), a flag falls ([`Game::time_out`]), or the result
//! comes from somewhere else, like an arbiter or a PGN record ([`Game::declare`]). The clocks
//! themselves are the program's: a `Game` doesn't know about time. Taking a move back withdraws
//! an ending like this, as it reopens a game ended on the board.
//!
//! ## Pitfalls
//!
//! - **Claims.** Under the FIDE Laws, threefold repetition and the fifty-move rule end the game
//!   only when a player claims them; the game ends by itself at fivefold repetition and
//!   seventy-five moves. Like most programs, `Game` ends the game as soon as a draw could be
//!   claimed, because an engine would claim it anyway.
//! - **Repetitions by hash key.** The en passant square is part of the key only when an enemy
//!   pawn stands next to the pawn that moved two squares ([`makemove`](crate::makemove)), so
//!   a position where that capture would be illegal (the capturing pawn is pinned) differs
//!   from the same position without it. FIDE counts the two as the same, so in that rare case
//!   a repetition goes unseen until the position comes up again.
//! - **Dead positions.** Insufficient material covers the dead positions that come from the
//!   material alone (king against king, king and one minor piece against king, bishops all on
//!   one colour). Positions dead for other reasons, such as locked pawn chains, play on.
//! - **Time against a lone king.** Under the FIDE Laws (6.9), a player whose flag falls only
//!   draws if the opponent couldn't mate by any series of legal moves. [`Game::time_out`]
//!   asks [`Position::has_mating_material`], which sees the material that can't mate (a lone
//!   king, a lone knight against queens, bishops of one colour against no pawns, knights or
//!   bishops of the other), but not a position dead for another reason, such as locked pawns:
//!   there the opponent still wins.
//!
//! ## Further reading
//!
//! - FIDE, [Laws of Chess](https://handbook.fide.com/chapter/E012023), articles 5 (the
//!   completion of the game), 6.9 (time) and 9 (the drawn game).
//! - Chess Programming Wiki, [Repetitions](https://www.chessprogramming.org/Repetitions).
//!
//! This type started in Solrac Chess AI's core crate, and moved here so that one set of rules
//! decides when every game is over.

use std::fmt;

use crate::fen::FenError;
use crate::moves::Move;
use crate::pgn::PgnGame;
use crate::piece::Color;
use crate::position::Position;

/// Half-moves without a capture or a pawn move after which the game is drawn (the fifty-move
/// rule, counted in half-moves).
pub const FIFTY_MOVE_HALFMOVES: u32 = 100;

/// How many times a position must have occurred before, for the next occurrence to be a
/// threefold repetition.
pub const REPETITIONS_BEFORE_DRAW: usize = 2;

/// A move of the game, with what a board, a move list and a PGN record need to show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayedMove {
    /// The move.
    pub mv: Move,
    /// The move in Standard Algebraic Notation, with `+` or `#`.
    pub san: String,
    /// The position before the move.
    pub before: Position,
    /// A comment written after the move in PGN, like an engine's score and depth.
    pub comment: Option<String>,
}

/// How a game ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// The side to move is checkmated.
    Checkmate {
        /// Who delivered mate.
        winner: Color,
    },
    /// The side to move has no legal moves and is not in check.
    Stalemate,
    /// The same position for the third time.
    Repetition,
    /// Fifty moves each without a capture or a pawn move.
    FiftyMoves,
    /// Neither side has enough material to mate.
    InsufficientMaterial,
    /// A player resigned.
    Resignation {
        /// The player who didn't resign.
        winner: Color,
    },
    /// The players agreed a draw.
    DrawAgreed,
    /// A player ran out of time.
    Timeout {
        /// The player whose flag didn't fall.
        winner: Color,
    },
    /// A player ran out of time, but the opponent couldn't have mated, so the game is a draw
    /// (FIDE Laws 6.9).
    TimeoutDraw {
        /// The player whose flag fell.
        flagged: Color,
    },
    /// The result was decided off the board for a reason the game doesn't know: by an arbiter,
    /// or in a PGN record whose moves don't end the game.
    Declared {
        /// The winner, or `None` for a draw.
        winner: Option<Color>,
    },
}

impl Outcome {
    /// The winner, or `None` for a draw.
    pub fn winner(self) -> Option<Color> {
        match self {
            Outcome::Checkmate { winner }
            | Outcome::Resignation { winner }
            | Outcome::Timeout { winner } => Some(winner),
            Outcome::Declared { winner } => winner,
            Outcome::Stalemate
            | Outcome::Repetition
            | Outcome::FiftyMoves
            | Outcome::InsufficientMaterial
            | Outcome::DrawAgreed
            | Outcome::TimeoutDraw { .. } => None,
        }
    }

    /// The result as PGN writes it: `1-0`, `0-1` or `1/2-1/2`.
    pub fn result(self) -> &'static str {
        match self.winner() {
            Some(Color::White) => "1-0",
            Some(Color::Black) => "0-1",
            None => "1/2-1/2",
        }
    }
}

impl fmt::Display for Outcome {
    /// A sentence for the player, like "Checkmate. White wins."
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Outcome::Checkmate { winner } => write!(f, "Checkmate. {winner} wins."),
            Outcome::Stalemate => write!(f, "Stalemate. The game is a draw."),
            Outcome::Repetition => write!(f, "Draw by threefold repetition."),
            Outcome::FiftyMoves => write!(f, "Draw by the fifty-move rule."),
            Outcome::InsufficientMaterial => {
                write!(f, "Draw. Neither side has enough material to mate.")
            }
            Outcome::Resignation { winner } => write!(f, "{} resigned. {winner} wins.", !*winner),
            Outcome::DrawAgreed => write!(f, "Draw agreed."),
            Outcome::Timeout { winner } => write!(f, "{} lost on time. {winner} wins.", !*winner),
            Outcome::TimeoutDraw { flagged } => write!(
                f,
                "{flagged} ran out of time, but {} couldn't mate. The game is a draw.",
                !*flagged
            ),
            Outcome::Declared { .. } => write!(f, "The game ended {}.", self.result()),
        }
    }
}

/// Why a move or a game couldn't be accepted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameError {
    /// The move isn't legal in the current position.
    IllegalMove(Move),
    /// The game is already over.
    GameOver(Outcome),
    /// A FEN string couldn't be read.
    Fen(FenError),
}

impl fmt::Display for GameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GameError::IllegalMove(mv) => write!(f, "{mv} is not a legal move here"),
            GameError::GameOver(outcome) => write!(f, "the game is over: {outcome}"),
            GameError::Fen(error) => write!(f, "can't read the FEN: {error}"),
        }
    }
}

impl std::error::Error for GameError {}

/// A game of chess (see the [module docs](self)).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Game {
    start: Position,
    position: Position,
    moves: Vec<PlayedMove>,
    /// An ending off the board (resignation, agreement, time, a declared result); everything
    /// else is worked out from the position.
    ended: Option<Outcome>,
    /// The outcome, recomputed after every change.
    outcome: Option<Outcome>,
}

impl Default for Game {
    fn default() -> Game {
        Game::from_position(Position::startpos())
    }
}

impl Game {
    /// A new game from the initial position.
    pub fn new() -> Game {
        Game::default()
    }

    /// A new game from `start`.
    pub fn from_position(start: Position) -> Game {
        let mut game = Game {
            position: start.clone(),
            start,
            moves: Vec::new(),
            ended: None,
            outcome: None,
        };
        game.outcome = game.compute_outcome();
        game
    }

    /// A new game from a FEN string.
    pub fn from_fen(fen: &str) -> Result<Game, GameError> {
        Position::from_fen(fen)
            .map(Game::from_position)
            .map_err(GameError::Fen)
    }

    /// Replays a PGN game, with its comments. When the moves end the game, the result is theirs
    /// and the record's result tag is ignored. When they don't, a `1-0`, `0-1` or `1/2-1/2` tag
    /// ends the game with [`Outcome::Declared`] (the game was resigned, agreed or lost on
    /// time), and `*` leaves it going.
    pub fn from_pgn(pgn: &PgnGame) -> Result<Game, GameError> {
        let mut game = Game::from_position(pgn.start.clone());
        for (i, &mv) in pgn.moves.iter().enumerate() {
            let comment = pgn.comments.get(i).cloned().flatten();
            game.play_with_comment(mv, comment)?;
        }
        let declared = match pgn.result.as_str() {
            "1-0" => Some(Some(Color::White)),
            "0-1" => Some(Some(Color::Black)),
            "1/2-1/2" => Some(None),
            _ => None,
        };
        if let Some(winner) = declared {
            game.declare(winner);
        }
        Ok(game)
    }

    /// The position the game started from.
    pub fn start(&self) -> &Position {
        &self.start
    }

    /// The position now.
    pub fn position(&self) -> &Position {
        &self.position
    }

    /// The moves played, in order.
    pub fn moves(&self) -> &[PlayedMove] {
        &self.moves
    }

    /// The last move played, if any.
    pub fn last_move(&self) -> Option<&PlayedMove> {
        self.moves.last()
    }

    /// Whether the game is over, and how.
    pub fn outcome(&self) -> Option<Outcome> {
        self.outcome
    }

    /// The hash keys of every position before the current one, oldest first. This is what
    /// `cb2026-engine`'s `Engine::search` wants as its `history`, for spotting repetitions.
    pub fn history_hashes(&self) -> Vec<u64> {
        self.moves
            .iter()
            .map(|played| played.before.hash())
            .collect()
    }

    /// Plays `mv`, if it is legal and the game isn't over.
    pub fn play(&mut self, mv: Move) -> Result<&PlayedMove, GameError> {
        self.play_with_comment(mv, None)
    }

    /// Plays `mv` with a comment for PGN, if it is legal and the game isn't over.
    pub fn play_with_comment(
        &mut self,
        mv: Move,
        comment: Option<String>,
    ) -> Result<&PlayedMove, GameError> {
        if let Some(outcome) = self.outcome {
            return Err(GameError::GameOver(outcome));
        }
        if !self.position.legal_moves().contains(&mv) {
            return Err(GameError::IllegalMove(mv));
        }
        let san = self.position.to_san(mv);
        let before = self.position.clone();
        self.position.make_move(mv);
        self.moves.push(PlayedMove {
            mv,
            san,
            before,
            comment,
        });
        self.outcome = self.compute_outcome();
        Ok(self.moves.last().expect("a move was just pushed"))
    }

    /// Takes back the last move and returns it. Also withdraws an ending off the board
    /// (a resignation, an agreed draw, a loss on time, a declared result).
    pub fn undo(&mut self) -> Option<PlayedMove> {
        self.ended = None;
        let played = self.moves.pop();
        if let Some(played) = &played {
            self.position = played.before.clone();
        }
        self.outcome = self.compute_outcome();
        played
    }

    /// `loser` resigns. Does nothing if the game is already over.
    pub fn resign(&mut self, loser: Color) {
        self.end(Outcome::Resignation { winner: !loser });
    }

    /// The players agree a draw. Does nothing if the game is already over.
    pub fn agree_draw(&mut self) {
        self.end(Outcome::DrawAgreed);
    }

    /// `side`'s flag fell. The opponent wins if it has the material to mate
    /// ([`Position::has_mating_material`]); otherwise the game is a draw (FIDE Laws 6.9, and
    /// the module docs' pitfalls). Does nothing if the game is already over.
    pub fn time_out(&mut self, side: Color) {
        let opponent = !side;
        self.end(if self.position.has_mating_material(opponent) {
            Outcome::Timeout { winner: opponent }
        } else {
            Outcome::TimeoutDraw { flagged: side }
        });
    }

    /// Ends the game with a result decided off the board: `winner`, or `None` for a draw.
    /// Does nothing if the game is already over.
    pub fn declare(&mut self, winner: Option<Color>) {
        self.end(Outcome::Declared { winner });
    }

    /// The game as a PGN record, with its comments and result. Tags (players, event) are the
    /// caller's.
    pub fn to_pgn(&self) -> PgnGame {
        let mut pgn = PgnGame::new(self.start.clone());
        pgn.moves = self.moves.iter().map(|played| played.mv).collect();
        pgn.comments = self
            .moves
            .iter()
            .map(|played| played.comment.clone())
            .collect();
        pgn.result = self
            .outcome
            .map_or("*".to_string(), |outcome| outcome.result().to_string());
        pgn
    }

    /// The moves in pairs for a move list: the move number, White's move and Black's move.
    /// A game that starts with Black to move has no White move in its first pair.
    pub fn numbered_moves(&self) -> Vec<(u32, Option<&str>, Option<&str>)> {
        let mut rows: Vec<(u32, Option<&str>, Option<&str>)> = Vec::new();
        for played in &self.moves {
            let number = played.before.fullmove_number();
            match played.before.side_to_move() {
                Color::White => rows.push((number, Some(&played.san), None)),
                Color::Black => match rows.last_mut() {
                    Some(row) if row.0 == number && row.2.is_none() => row.2 = Some(&played.san),
                    _ => rows.push((number, None, Some(&played.san))),
                },
            }
        }
        rows
    }

    /// Ends the game off the board with `outcome`, unless it is already over.
    fn end(&mut self, outcome: Outcome) {
        if self.outcome.is_none() {
            self.ended = Some(outcome);
            self.outcome = self.compute_outcome();
        }
    }

    fn compute_outcome(&self) -> Option<Outcome> {
        if self.ended.is_some() {
            return self.ended;
        }
        let pos = &self.position;
        if pos.legal_moves().is_empty() {
            return Some(if pos.in_check() {
                Outcome::Checkmate {
                    winner: !pos.side_to_move(),
                }
            } else {
                Outcome::Stalemate
            });
        }
        if pos.halfmove_clock() >= FIFTY_MOVE_HALFMOVES {
            return Some(Outcome::FiftyMoves);
        }
        if pos.is_insufficient_material() {
            return Some(Outcome::InsufficientMaterial);
        }
        let earlier = self
            .moves
            .iter()
            .filter(|played| played.before.hash() == pos.hash())
            .count();
        if earlier >= REPETITIONS_BEFORE_DRAW {
            return Some(Outcome::Repetition);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::moves::MoveKind;

    fn play_san(game: &mut Game, moves: &str) {
        for san in moves.split_whitespace() {
            let mv = game.position().parse_san(san).expect("test move parses");
            game.play(mv).expect("test move is legal");
        }
    }

    #[test]
    fn fools_mate_is_checkmate() {
        let mut game = Game::new();
        play_san(&mut game, "f3 e5 g4 Qh4#");
        assert_eq!(
            game.outcome(),
            Some(Outcome::Checkmate {
                winner: Color::Black
            })
        );
        assert_eq!(game.outcome().unwrap().result(), "0-1");
        assert_eq!(game.last_move().unwrap().san, "Qh4#");
    }

    #[test]
    fn no_moves_after_the_game_is_over() {
        let mut game = Game::new();
        play_san(&mut game, "f3 e5 g4 Qh4#");
        let any = game.start().legal_moves()[0];
        assert!(matches!(game.play(any), Err(GameError::GameOver(_))));
    }

    #[test]
    fn illegal_moves_are_refused() {
        let mut game = Game::new();
        let e2e5 = Move::new(
            "e2".parse().unwrap(),
            "e5".parse().unwrap(),
            MoveKind::Quiet,
        );
        assert_eq!(game.play(e2e5), Err(GameError::IllegalMove(e2e5)));
        assert!(game.moves().is_empty());
    }

    #[test]
    fn stalemate() {
        let mut game = Game::from_fen("k7/8/1QK5/8/8/8/8/8 w - - 0 1").unwrap();
        assert_eq!(game.outcome(), None);
        play_san(&mut game, "Qc7");
        assert_eq!(game.outcome(), Some(Outcome::Stalemate));
        assert_eq!(game.outcome().unwrap().result(), "1/2-1/2");
    }

    #[test]
    fn threefold_repetition() {
        let mut game = Game::new();
        play_san(&mut game, "Nf3 Nf6 Ng1 Ng8 Nf3 Nf6 Ng1");
        assert_eq!(game.outcome(), None);
        play_san(&mut game, "Ng8");
        assert_eq!(game.outcome(), Some(Outcome::Repetition));
    }

    #[test]
    fn fifty_move_rule() {
        let mut game = Game::from_fen("k7/8/8/8/8/8/1R6/K7 w - - 99 80").unwrap();
        assert_eq!(game.outcome(), None);
        play_san(&mut game, "Rc2");
        assert_eq!(game.outcome(), Some(Outcome::FiftyMoves));
    }

    #[test]
    fn mate_on_the_hundredth_half_move_is_still_mate() {
        let mut game = Game::from_fen("6k1/5ppp/8/8/8/8/8/4R1K1 w - - 99 80").unwrap();
        play_san(&mut game, "Re8#");
        assert_eq!(
            game.outcome(),
            Some(Outcome::Checkmate {
                winner: Color::White
            })
        );
    }

    #[test]
    fn insufficient_material_after_the_last_capture() {
        let mut game = Game::from_fen("k7/8/8/8/8/8/1n6/KR6 w - - 0 1").unwrap();
        play_san(&mut game, "Rxb2");
        assert_eq!(game.outcome(), None, "king and rook can mate");
        let mut game = Game::from_fen("k7/8/8/8/8/8/1r6/KN6 w - - 0 1").unwrap();
        play_san(&mut game, "Kxb2");
        assert_eq!(game.outcome(), Some(Outcome::InsufficientMaterial));
    }

    #[test]
    fn undo_restores_the_position_and_reopens_the_game() {
        let mut game = Game::new();
        play_san(&mut game, "f3 e5 g4 Qh4#");
        let undone = game.undo().unwrap();
        assert_eq!(undone.san, "Qh4#");
        assert_eq!(game.outcome(), None);
        assert_eq!(game.position(), &undone.before);
        while game.undo().is_some() {}
        assert_eq!(game.position(), &Position::startpos());
    }

    #[test]
    fn resignation_and_takeback() {
        let mut game = Game::new();
        game.resign(Color::White);
        assert_eq!(
            game.outcome(),
            Some(Outcome::Resignation {
                winner: Color::Black
            })
        );
        assert_eq!(
            game.outcome().unwrap().to_string(),
            "White resigned. Black wins."
        );
        game.undo();
        assert_eq!(game.outcome(), None);
    }

    #[test]
    fn endings_off_the_board() {
        let mut game = Game::new();
        play_san(&mut game, "e4");
        game.agree_draw();
        assert_eq!(game.outcome(), Some(Outcome::DrawAgreed));
        assert_eq!(game.to_pgn().result, "1/2-1/2");
        // Taking the move back withdraws the agreement, and the move.
        game.undo();
        assert!(game.moves().is_empty());
        game.time_out(Color::White);
        assert_eq!(game.outcome().unwrap().result(), "0-1");
        assert_eq!(
            game.outcome().unwrap().to_string(),
            "White lost on time. Black wins."
        );
        // A game already over stays as it ended.
        game.resign(Color::Black);
        game.declare(None);
        assert_eq!(
            game.outcome(),
            Some(Outcome::Timeout {
                winner: Color::Black
            })
        );
    }

    #[test]
    fn a_flag_falling_is_a_draw_when_the_opponent_cannot_mate() {
        // White has a queen, Black only a knight: White can mate, Black can't.
        let start = "k7/8/8/8/8/8/8/2nQK3 w - - 0 1";
        let mut game = Game::from_fen(start).unwrap();
        game.time_out(Color::White);
        assert_eq!(
            game.outcome(),
            Some(Outcome::TimeoutDraw {
                flagged: Color::White
            })
        );
        assert_eq!(game.outcome().unwrap().result(), "1/2-1/2");
        assert_eq!(
            game.outcome().unwrap().to_string(),
            "White ran out of time, but Black couldn't mate. The game is a draw."
        );
        let mut game = Game::from_fen(start).unwrap();
        game.time_out(Color::Black);
        assert_eq!(
            game.outcome(),
            Some(Outcome::Timeout {
                winner: Color::White
            })
        );
    }

    #[test]
    fn a_game_over_on_the_board_cannot_be_resigned() {
        let mut game = Game::new();
        play_san(&mut game, "f3 e5 g4 Qh4#");
        game.resign(Color::Black);
        assert_eq!(game.outcome().unwrap().winner(), Some(Color::Black));
    }

    #[test]
    fn history_hashes_are_the_positions_before_each_move() {
        let mut game = Game::new();
        play_san(&mut game, "e4 e5");
        let hashes = game.history_hashes();
        assert_eq!(hashes.len(), 2);
        assert_eq!(hashes[0], Position::startpos().hash());
        assert!(!hashes.contains(&game.position().hash()));
    }

    #[test]
    fn pgn_round_trip() {
        let mut game = Game::new();
        play_san(&mut game, "e4 e5 Nf3 Nc6");
        let bb5 = game.position().parse_san("Bb5").unwrap();
        game.play_with_comment(bb5, Some("+0.35/18".to_string()))
            .unwrap();
        play_san(&mut game, "a6");
        let text = game.to_pgn().to_pgn();
        assert!(
            // A comment between White's move and Black's: Black's is numbered again.
            text.contains("1. e4 e5 2. Nf3 Nc6 3. Bb5 {+0.35/18} 3... a6 *"),
            "{text}"
        );
        let again = Game::from_pgn(&PgnGame::parse(&text).unwrap()).unwrap();
        assert_eq!(again, game);
    }

    #[test]
    fn a_pgn_result_the_moves_dont_explain_is_declared() {
        let mut pgn = Game::new().to_pgn();
        pgn.moves = vec![Position::startpos().parse_san("e4").unwrap()];
        pgn.comments = vec![None];
        pgn.result = "0-1".to_string();
        let game = Game::from_pgn(&pgn).unwrap();
        assert_eq!(
            game.outcome(),
            Some(Outcome::Declared {
                winner: Some(Color::Black)
            })
        );
        assert_eq!(game.outcome().unwrap().to_string(), "The game ended 0-1.");
        // A record's result can't overrule the board.
        let mut mated = Game::new();
        play_san(&mut mated, "f3 e5 g4 Qh4#");
        let mut pgn = mated.to_pgn();
        pgn.result = "1-0".to_string();
        assert_eq!(Game::from_pgn(&pgn).unwrap().outcome(), mated.outcome());
    }

    #[test]
    fn numbered_moves_pair_white_and_black() {
        let mut game = Game::new();
        play_san(&mut game, "e4 e5 Nf3");
        assert_eq!(
            game.numbered_moves(),
            vec![(1, Some("e4"), Some("e5")), (2, Some("Nf3"), None)]
        );
        let mut game =
            Game::from_fen("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1").unwrap();
        play_san(&mut game, "e5 Nf3");
        assert_eq!(
            game.numbered_moves(),
            vec![(1, None, Some("e5")), (2, Some("Nf3"), None)]
        );
    }
}
