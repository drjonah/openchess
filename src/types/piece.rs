//! Piece types and colored pieces.

use crate::types::color::Color;
use std::fmt;

/// Uncolored piece kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum PieceType {
    Pawn = 0,
    Knight = 1,
    Bishop = 2,
    Rook = 3,
    Queen = 4,
    King = 5,
}

impl PieceType {
    pub const COUNT: usize = 6;

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(PieceType::Pawn),
            1 => Some(PieceType::Knight),
            2 => Some(PieceType::Bishop),
            3 => Some(PieceType::Rook),
            4 => Some(PieceType::Queen),
            5 => Some(PieceType::King),
            _ => None,
        }
    }

    pub const ALL: [PieceType; 6] = [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
        PieceType::King,
    ];
}

/// Colored piece, or empty for mailbox slots.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Piece {
    Empty,
    WhitePawn,
    WhiteKnight,
    WhiteBishop,
    WhiteRook,
    WhiteQueen,
    WhiteKing,
    BlackPawn,
    BlackKnight,
    BlackBishop,
    BlackRook,
    BlackQueen,
    BlackKing,
}

impl Piece {
    #[inline]
    pub const fn new(color: Color, piece_type: PieceType) -> Self {
        match (color, piece_type) {
            (Color::White, PieceType::Pawn) => Piece::WhitePawn,
            (Color::White, PieceType::Knight) => Piece::WhiteKnight,
            (Color::White, PieceType::Bishop) => Piece::WhiteBishop,
            (Color::White, PieceType::Rook) => Piece::WhiteRook,
            (Color::White, PieceType::Queen) => Piece::WhiteQueen,
            (Color::White, PieceType::King) => Piece::WhiteKing,
            (Color::Black, PieceType::Pawn) => Piece::BlackPawn,
            (Color::Black, PieceType::Knight) => Piece::BlackKnight,
            (Color::Black, PieceType::Bishop) => Piece::BlackBishop,
            (Color::Black, PieceType::Rook) => Piece::BlackRook,
            (Color::Black, PieceType::Queen) => Piece::BlackQueen,
            (Color::Black, PieceType::King) => Piece::BlackKing,
        }
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        matches!(self, Piece::Empty)
    }

    #[inline]
    pub const fn color(self) -> Option<Color> {
        match self {
            Piece::Empty => None,
            Piece::WhitePawn
            | Piece::WhiteKnight
            | Piece::WhiteBishop
            | Piece::WhiteRook
            | Piece::WhiteQueen
            | Piece::WhiteKing => Some(Color::White),
            Piece::BlackPawn
            | Piece::BlackKnight
            | Piece::BlackBishop
            | Piece::BlackRook
            | Piece::BlackQueen
            | Piece::BlackKing => Some(Color::Black),
        }
    }

    #[inline]
    pub const fn piece_type(self) -> Option<PieceType> {
        match self {
            Piece::Empty => None,
            Piece::WhitePawn | Piece::BlackPawn => Some(PieceType::Pawn),
            Piece::WhiteKnight | Piece::BlackKnight => Some(PieceType::Knight),
            Piece::WhiteBishop | Piece::BlackBishop => Some(PieceType::Bishop),
            Piece::WhiteRook | Piece::BlackRook => Some(PieceType::Rook),
            Piece::WhiteQueen | Piece::BlackQueen => Some(PieceType::Queen),
            Piece::WhiteKing | Piece::BlackKing => Some(PieceType::King),
        }
    }

    /// FEN character for this piece.
    pub const fn to_char(self) -> char {
        match self {
            Piece::Empty => '.',
            Piece::WhitePawn => 'P',
            Piece::WhiteKnight => 'N',
            Piece::WhiteBishop => 'B',
            Piece::WhiteRook => 'R',
            Piece::WhiteQueen => 'Q',
            Piece::WhiteKing => 'K',
            Piece::BlackPawn => 'p',
            Piece::BlackKnight => 'n',
            Piece::BlackBishop => 'b',
            Piece::BlackRook => 'r',
            Piece::BlackQueen => 'q',
            Piece::BlackKing => 'k',
        }
    }

    pub const fn from_char(c: char) -> Option<Self> {
        match c {
            'P' => Some(Piece::WhitePawn),
            'N' => Some(Piece::WhiteKnight),
            'B' => Some(Piece::WhiteBishop),
            'R' => Some(Piece::WhiteRook),
            'Q' => Some(Piece::WhiteQueen),
            'K' => Some(Piece::WhiteKing),
            'p' => Some(Piece::BlackPawn),
            'n' => Some(Piece::BlackKnight),
            'b' => Some(Piece::BlackBishop),
            'r' => Some(Piece::BlackRook),
            'q' => Some(Piece::BlackQueen),
            'k' => Some(Piece::BlackKing),
            _ => None,
        }
    }
}

impl fmt::Display for Piece {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_char())
    }
}
