//! Drawing boards and bitboards as text.
//!
//! Everything here returns a `String` and touches no terminal state, so it can be tested
//! exactly. Colour is ANSI escape codes, used only when the caller says the terminal
//! supports it.

use cb2026_chess::{Bitboard, CastlingSide, Color, File, Piece, PieceType, Position, Rank, Square};

/// How to draw boards.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Style {
    /// Chess symbols (♔♞...) instead of letters (K n ...).
    pub unicode: bool,
    /// ANSI colours for the squares and pieces.
    pub color: bool,
    /// Draw the board from Black's side (rank 1 at the top).
    pub flipped: bool,
}

impl Default for Style {
    fn default() -> Style {
        Style {
            unicode: true,
            color: false,
            flipped: false,
        }
    }
}

/// ANSI escape sequences used for the coloured board.
mod ansi {
    /// Light squares: a warm beige (256-colour palette).
    pub const LIGHT_SQUARE: &str = "\x1b[48;5;180m";
    /// Dark squares: brown.
    pub const DARK_SQUARE: &str = "\x1b[48;5;137m";
    /// White pieces: bright white, bold.
    pub const WHITE_PIECE: &str = "\x1b[1;97m";
    /// Black pieces: black.
    pub const BLACK_PIECE: &str = "\x1b[30m";
    /// Back to the terminal's defaults.
    pub const RESET: &str = "\x1b[0m";
}

/// The symbol for a piece.
///
/// With colour on, both sides use the solid symbols and the colour tells them apart (outline
/// symbols are hard to see on a coloured square). Without colour, White gets the outline
/// symbols and Black the solid ones.
fn piece_symbol(piece: Piece, style: Style) -> char {
    if !style.unicode {
        return piece.to_fen_char();
    }
    let solid = style.color || piece.color == Color::Black;
    match (piece.piece_type, solid) {
        (PieceType::King, false) => '♔',
        (PieceType::Queen, false) => '♕',
        (PieceType::Rook, false) => '♖',
        (PieceType::Bishop, false) => '♗',
        (PieceType::Knight, false) => '♘',
        (PieceType::Pawn, false) => '♙',
        (PieceType::King, true) => '♚',
        (PieceType::Queen, true) => '♛',
        (PieceType::Rook, true) => '♜',
        (PieceType::Bishop, true) => '♝',
        (PieceType::Knight, true) => '♞',
        (PieceType::Pawn, true) => '♟',
    }
}

/// Ranks from the top of the drawing to the bottom.
fn ranks_top_down(style: Style) -> Vec<Rank> {
    let mut ranks = Rank::ALL.to_vec();
    if !style.flipped {
        ranks.reverse();
    }
    ranks
}

/// Files from left to right.
fn files_left_right(style: Style) -> Vec<File> {
    let mut files = File::ALL.to_vec();
    if style.flipped {
        files.reverse();
    }
    files
}

/// The file letters under the board.
fn file_labels(style: Style) -> String {
    let letters: Vec<String> = files_left_right(style)
        .iter()
        .map(|f| format!(" {} ", f.to_char()))
        .collect();
    format!("   {}", letters.join("")).trim_end().to_string()
}

/// Draws one square: three characters wide, so the board looks square in most fonts.
fn draw_square(
    square: Square,
    content: Option<char>,
    piece_color: Option<Color>,
    style: Style,
) -> String {
    let symbol = content.unwrap_or(if style.unicode { '·' } else { '.' });
    if !style.color {
        return format!(" {symbol} ");
    }
    // a1 is a dark square: a square is light when its file and rank indices differ in parity.
    let light = (square.file().index() + square.rank().index()) % 2 == 1;
    let background = if light {
        ansi::LIGHT_SQUARE
    } else {
        ansi::DARK_SQUARE
    };
    let (foreground, symbol) = match piece_color {
        Some(Color::White) => (ansi::WHITE_PIECE, symbol),
        Some(Color::Black) => (ansi::BLACK_PIECE, symbol),
        None => ("", ' '),
    };
    format!("{background}{foreground} {symbol} {}", ansi::RESET)
}

/// The lines of information shown to the right of the board.
fn side_panel(pos: &Position) -> Vec<String> {
    let castling = {
        let rights = pos.castling();
        let mut s = String::new();
        for color in Color::ALL {
            for side in CastlingSide::ALL {
                if rights.has(color, side) {
                    let c = if side == CastlingSide::KingSide {
                        'k'
                    } else {
                        'q'
                    };
                    s.push(if color == Color::White {
                        c.to_ascii_uppercase()
                    } else {
                        c
                    });
                }
            }
        }
        if s.is_empty() { "-".to_string() } else { s }
    };
    vec![
        format!(
            "{} to move · move {}",
            pos.side_to_move(),
            pos.fullmove_number()
        ),
        format!(
            "Castling: {castling}   En passant: {}",
            pos.en_passant().map_or("-".to_string(), |s| s.to_string())
        ),
        format!("50-move counter: {}/100", pos.halfmove_clock()),
        format!("Hash: {:#018x}", pos.hash()),
        if pos.in_check() {
            "Check!".to_string()
        } else {
            String::new()
        },
    ]
}

/// Draws a position: the board with coordinates, an information panel and the FEN.
pub fn board(pos: &Position, style: Style) -> String {
    let panel = side_panel(pos);
    let mut out = String::new();
    for (row, rank) in ranks_top_down(style).into_iter().enumerate() {
        let mut line = format!(" {} ", rank.to_char());
        for file in files_left_right(style) {
            let square = Square::new(file, rank);
            let piece = pos.piece_at(square);
            line.push_str(&draw_square(
                square,
                piece.map(|p| piece_symbol(p, style)),
                piece.map(|p| p.color),
                style,
            ));
        }
        if let Some(info) = panel.get(row) {
            line.push_str("   ");
            line.push_str(info);
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&file_labels(style));
    out.push('\n');
    out.push_str(&format!("FEN: {}\n", pos.to_fen()));
    out
}

/// Draws a bitboard as a board of members (`1`) and non-members, plus its hex value and size.
pub fn bitboard(bb: Bitboard, style: Style) -> String {
    let mut out = String::new();
    for rank in ranks_top_down(style) {
        let mut line = format!(" {} ", rank.to_char());
        for file in files_left_right(style) {
            let square = Square::new(file, rank);
            let member = bb.contains(square);
            if style.color {
                let content = member.then_some('1');
                out.push_str(&draw_square(
                    square,
                    content,
                    content.map(|_| Color::White),
                    style,
                ));
            } else {
                let mark = if member {
                    '1'
                } else if style.unicode {
                    '·'
                } else {
                    '.'
                };
                line.push_str(&format!(" {mark} "));
            }
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out.push_str(&file_labels(style));
    out.push('\n');
    out.push_str(&format!(
        "{:#018x}  ({} square{})\n",
        bb.0,
        bb.count(),
        if bb.count() == 1 { "" } else { "s" }
    ));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ASCII: Style = Style {
        unicode: false,
        color: false,
        flipped: false,
    };

    #[test]
    fn ascii_start_position() {
        let expected = [
            " 8  r  n  b  q  k  b  n  r    White to move · move 1",
            " 7  p  p  p  p  p  p  p  p    Castling: KQkq   En passant: -",
            " 6  .  .  .  .  .  .  .  .    50-move counter: 0/100",
            " 5  .  .  .  .  .  .  .  .    Hash: 0xd15760e4b44e8f30",
            " 4  .  .  .  .  .  .  .  .",
            " 3  .  .  .  .  .  .  .  .",
            " 2  P  P  P  P  P  P  P  P",
            " 1  R  N  B  Q  K  B  N  R",
            "    a  b  c  d  e  f  g  h",
            "FEN: rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
            "",
        ]
        .join("\n");
        assert_eq!(board(&Position::startpos(), ASCII), expected);
    }

    #[test]
    fn flipped_board_puts_rank_1_on_top() {
        let style = Style {
            flipped: true,
            ..ASCII
        };
        let text = board(&Position::startpos(), style);
        // From Black's side the h-file is on the left, so the king (e1) comes before the queen (d1).
        assert!(text.starts_with(" 1  R  N  B  K  Q  B  N  R    White to move"));
        assert!(text.contains("    h  g  f  e  d  c  b  a\n"));
    }

    #[test]
    fn unicode_symbols() {
        let text = board(&Position::startpos(), Style::default());
        assert!(text.contains(" ♜  ♞  ♝  ♛  ♚  ♝  ♞  ♜ "));
        assert!(text.contains(" ♖  ♘  ♗  ♕  ♔  ♗  ♘  ♖\n"));
    }

    #[test]
    fn colour_uses_escape_codes_and_resets() {
        let style = Style {
            color: true,
            ..Style::default()
        };
        let text = board(&Position::startpos(), style);
        assert!(text.contains(ansi::LIGHT_SQUARE));
        assert!(text.contains(ansi::DARK_SQUARE));
        assert_eq!(
            text.matches(ansi::RESET).count(),
            64,
            "every square resets its colours"
        );
    }

    #[test]
    fn bitboard_view() {
        let text = bitboard(Bitboard::rank(Rank::Second), ASCII);
        assert!(text.contains(" 2  1  1  1  1  1  1  1  1\n"));
        assert!(text.contains(" 3  .  .  .  .  .  .  .  .\n"));
        assert!(text.ends_with("0x000000000000ff00  (8 squares)\n"));
    }
}
