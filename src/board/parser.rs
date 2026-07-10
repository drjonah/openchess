//! FEN position parsing/serialization and UCI-style move parsing.
//!
//! FEN has six whitespace-separated fields: piece placement, side to move,
//! castling rights, en passant target, halfmove clock, fullmove number. The
//! last two are optional on input (default to `0` and `1`) but always
//! written by [`Board::to_fen`].
//!
//! UCI move strings (`e2e4`, `a7a8q`) carry no legality or move-flag
//! information by themselves; [`Board::parse_uci_move`] resolves one against
//! [`Board::legal_moves`] so the returned [`Move`] carries the correct
//! castling/en-passant/promotion flags.

use super::Board;
use crate::types::{CastlingRights, Color, Move, Piece, PieceType, Square};
use std::fmt;
use std::str::FromStr;

/// Error returned by [`Board::from_fen`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FenError {
    /// Wrong number of whitespace-separated fields (expected 4..=6).
    InvalidFieldCount(usize),
    /// Piece placement field is malformed (wrong rank count, bad run length,
    /// or an unrecognized piece character).
    InvalidPlacement(String),
    /// Side-to-move field is neither `w` nor `b`.
    InvalidSideToMove(String),
    /// Castling field is not `-` or a subset of `KQkq`.
    InvalidCastling(String),
    /// En passant field is not `-` or a valid square.
    InvalidEnPassant(String),
    /// Halfmove clock field did not parse as an integer.
    InvalidHalfmoveClock(String),
    /// Fullmove number field did not parse as an integer.
    InvalidFullmoveNumber(String),
}

impl fmt::Display for FenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FenError::InvalidFieldCount(n) => {
                write!(f, "expected 4 to 6 FEN fields, got {n}")
            }
            FenError::InvalidPlacement(s) => write!(f, "invalid piece placement: '{s}'"),
            FenError::InvalidSideToMove(s) => write!(f, "invalid side to move: '{s}'"),
            FenError::InvalidCastling(s) => write!(f, "invalid castling rights: '{s}'"),
            FenError::InvalidEnPassant(s) => write!(f, "invalid en passant square: '{s}'"),
            FenError::InvalidHalfmoveClock(s) => write!(f, "invalid halfmove clock: '{s}'"),
            FenError::InvalidFullmoveNumber(s) => write!(f, "invalid fullmove number: '{s}'"),
        }
    }
}

impl std::error::Error for FenError {}

/// Error returned by [`Board::parse_uci_move`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseMoveError {
    /// Not 4 or 5 characters (`e2e4` or `a7a8q`).
    InvalidFormat(String),
    /// The from/to portion isn't a valid square.
    InvalidSquare(String),
    /// The trailing promotion character isn't one of `nbrq`.
    InvalidPromotion(char),
    /// Well-formed but not a legal move in the current position.
    IllegalMove(String),
}

impl fmt::Display for ParseMoveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseMoveError::InvalidFormat(s) => write!(f, "invalid UCI move format: '{s}'"),
            ParseMoveError::InvalidSquare(s) => write!(f, "invalid square: '{s}'"),
            ParseMoveError::InvalidPromotion(c) => write!(f, "invalid promotion piece: '{c}'"),
            ParseMoveError::IllegalMove(s) => write!(f, "illegal move: '{s}'"),
        }
    }
}

impl std::error::Error for ParseMoveError {}

/// Piece placement ranks run top (rank 8) to bottom (rank 1) in FEN, but
/// [`Square::from_file_rank`] indexes rank 0 as rank 1.
fn parse_placement(board: &mut Board, placement: &str) -> Result<(), FenError> {
    let invalid = || FenError::InvalidPlacement(placement.to_string());

    let ranks: Vec<&str> = placement.split('/').collect();
    if ranks.len() != 8 {
        return Err(invalid());
    }

    for (rank_from_top, rank_str) in ranks.iter().enumerate() {
        let rank = 7 - rank_from_top as u8;
        let mut file: u8 = 0;

        for c in rank_str.chars() {
            if let Some(skip) = c.to_digit(10) {
                if skip == 0 || file + skip as u8 > 8 {
                    return Err(invalid());
                }
                file += skip as u8;
            } else {
                if file >= 8 {
                    return Err(invalid());
                }
                let piece = Piece::from_char(c).ok_or_else(invalid)?;
                let sq = Square::from_file_rank(file, rank).expect("file/rank in 0..8");
                board.put_piece(piece, sq);
                file += 1;
            }
        }

        if file != 8 {
            return Err(invalid());
        }
    }

    Ok(())
}

fn parse_castling(s: &str) -> Result<CastlingRights, FenError> {
    if s == "-" {
        return Ok(CastlingRights::NONE);
    }

    let mut rights = CastlingRights::NONE;
    for c in s.chars() {
        rights |= match c {
            'K' => CastlingRights::WHITE_KING,
            'Q' => CastlingRights::WHITE_QUEEN,
            'k' => CastlingRights::BLACK_KING,
            'q' => CastlingRights::BLACK_QUEEN,
            _ => return Err(FenError::InvalidCastling(s.to_string())),
        };
    }
    Ok(rights)
}

fn parse_ep_square(s: &str) -> Result<Option<Square>, FenError> {
    if s == "-" {
        return Ok(None);
    }
    Square::from_str(s)
        .map(Some)
        .map_err(|_| FenError::InvalidEnPassant(s.to_string()))
}

impl Board {
    /// Parse a FEN string into a [`Board`].
    ///
    /// The halfmove clock and fullmove number fields are optional and
    /// default to `0` and `1` respectively; all other fields are required.
    pub fn from_fen(fen: &str) -> Result<Board, FenError> {
        let fields: Vec<&str> = fen.split_whitespace().collect();
        if !(4..=6).contains(&fields.len()) {
            return Err(FenError::InvalidFieldCount(fields.len()));
        }

        let mut board = Board::empty();
        parse_placement(&mut board, fields[0])?;

        board.side_to_move = match fields[1] {
            "w" => Color::White,
            "b" => Color::Black,
            other => return Err(FenError::InvalidSideToMove(other.to_string())),
        };

        board.castling = parse_castling(fields[2])?;
        board.ep_square = parse_ep_square(fields[3])?;

        board.halfmove_clock = match fields.get(4) {
            Some(s) => s
                .parse()
                .map_err(|_| FenError::InvalidHalfmoveClock(s.to_string()))?,
            None => 0,
        };

        board.fullmove_number = match fields.get(5) {
            Some(s) => s
                .parse()
                .map_err(|_| FenError::InvalidFullmoveNumber(s.to_string()))?,
            None => 1,
        };

        board.key = board.compute_key();
        board.refresh_checkers_and_pins();
        Ok(board)
    }

    /// Serialize this position to a 6-field FEN string.
    pub fn to_fen(&self) -> String {
        let mut placement = String::new();
        for rank_from_top in 0..8u8 {
            let rank = 7 - rank_from_top;
            let mut empty_run = 0u8;

            for file in 0..8 {
                let sq = Square::from_file_rank(file, rank).expect("file/rank in 0..8");
                let piece = self.piece_on(sq);
                if piece.is_empty() {
                    empty_run += 1;
                    continue;
                }
                if empty_run > 0 {
                    placement.push_str(&empty_run.to_string());
                    empty_run = 0;
                }
                placement.push(piece.to_char());
            }

            if empty_run > 0 {
                placement.push_str(&empty_run.to_string());
            }
            if rank_from_top != 7 {
                placement.push('/');
            }
        }

        let side = match self.side_to_move {
            Color::White => "w",
            Color::Black => "b",
        };

        let mut castling = String::new();
        if self.castling.contains(CastlingRights::WHITE_KING) {
            castling.push('K');
        }
        if self.castling.contains(CastlingRights::WHITE_QUEEN) {
            castling.push('Q');
        }
        if self.castling.contains(CastlingRights::BLACK_KING) {
            castling.push('k');
        }
        if self.castling.contains(CastlingRights::BLACK_QUEEN) {
            castling.push('q');
        }
        if castling.is_empty() {
            castling.push('-');
        }

        let ep = match self.ep_square {
            Some(sq) => sq.to_string(),
            None => "-".to_string(),
        };

        format!(
            "{placement} {side} {castling} {ep} {} {}",
            self.halfmove_clock, self.fullmove_number
        )
    }

    /// Parse a UCI move string (`e2e4`, `a7a8q`) against this position.
    ///
    /// The from/to squares (and promotion piece, if any) are matched against
    /// [`Self::legal_moves`], so the returned [`Move`] carries the correct
    /// castling/en-passant/promotion flags. Returns an error for malformed
    /// input or a move that isn't legal here.
    pub fn parse_uci_move(&self, uci: &str) -> Result<Move, ParseMoveError> {
        if uci.len() != 4 && uci.len() != 5 {
            return Err(ParseMoveError::InvalidFormat(uci.to_string()));
        }
        if !uci.is_ascii() {
            return Err(ParseMoveError::InvalidFormat(uci.to_string()));
        }

        let from = Square::from_str(&uci[0..2])
            .map_err(|_| ParseMoveError::InvalidSquare(uci[0..2].to_string()))?;
        let to = Square::from_str(&uci[2..4])
            .map_err(|_| ParseMoveError::InvalidSquare(uci[2..4].to_string()))?;

        let promo = match uci.as_bytes().get(4) {
            Some(&b) => Some(match b as char {
                'n' => PieceType::Knight,
                'b' => PieceType::Bishop,
                'r' => PieceType::Rook,
                'q' => PieceType::Queen,
                c => return Err(ParseMoveError::InvalidPromotion(c)),
            }),
            None => None,
        };

        self.legal_moves()
            .into_iter()
            .find(|m| m.from() == from && m.to() == to && m.promotion_piece() == promo)
            .ok_or_else(|| ParseMoveError::IllegalMove(uci.to_string()))
    }
}
