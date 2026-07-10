//! Compact move encoding (16-bit).

use crate::types::piece::PieceType;
use crate::types::square::Square;
use std::fmt;

/// Move flags packed in the high nibble of a u16 encoding:
/// bits 0..5 from, 6..11 to, 12..15 flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Move(u16);

/// Flag nibble values.
pub mod flags {
    pub const NORMAL: u16 = 0;
    pub const EN_PASSANT: u16 = 1;
    pub const CASTLING: u16 = 2;
    pub const PROMOTION_KNIGHT: u16 = 4;
    pub const PROMOTION_BISHOP: u16 = 5;
    pub const PROMOTION_ROOK: u16 = 6;
    pub const PROMOTION_QUEEN: u16 = 7;
}

impl Move {
    pub const NONE: Move = Move(0);

    #[inline]
    pub const fn new(from: Square, to: Square) -> Self {
        Self::with_flags(from, to, flags::NORMAL)
    }

    #[inline]
    pub const fn with_flags(from: Square, to: Square, flag: u16) -> Self {
        Move((from.index() as u16) | ((to.index() as u16) << 6) | (flag << 12))
    }

    #[inline]
    pub const fn promotion(from: Square, to: Square, promo: PieceType) -> Self {
        let flag = match promo {
            PieceType::Knight => flags::PROMOTION_KNIGHT,
            PieceType::Bishop => flags::PROMOTION_BISHOP,
            PieceType::Rook => flags::PROMOTION_ROOK,
            PieceType::Queen => flags::PROMOTION_QUEEN,
            _ => flags::PROMOTION_QUEEN,
        };
        Self::with_flags(from, to, flag)
    }

    #[inline]
    pub const fn en_passant(from: Square, to: Square) -> Self {
        Self::with_flags(from, to, flags::EN_PASSANT)
    }

    #[inline]
    pub const fn castling(from: Square, to: Square) -> Self {
        Self::with_flags(from, to, flags::CASTLING)
    }

    #[inline]
    pub const fn from(self) -> Square {
        Square::from_index_unchecked((self.0 & 0x3F) as u8)
    }

    #[inline]
    pub const fn to(self) -> Square {
        Square::from_index_unchecked(((self.0 >> 6) & 0x3F) as u8)
    }

    #[inline]
    pub const fn flags(self) -> u16 {
        self.0 >> 12
    }

    #[inline]
    pub const fn is_none(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn is_promotion(self) -> bool {
        matches!(
            self.flags(),
            flags::PROMOTION_KNIGHT
                | flags::PROMOTION_BISHOP
                | flags::PROMOTION_ROOK
                | flags::PROMOTION_QUEEN
        )
    }

    #[inline]
    pub const fn is_en_passant(self) -> bool {
        self.flags() == flags::EN_PASSANT
    }

    #[inline]
    pub const fn is_castling(self) -> bool {
        self.flags() == flags::CASTLING
    }

    #[inline]
    pub const fn promotion_piece(self) -> Option<PieceType> {
        match self.flags() {
            flags::PROMOTION_KNIGHT => Some(PieceType::Knight),
            flags::PROMOTION_BISHOP => Some(PieceType::Bishop),
            flags::PROMOTION_ROOK => Some(PieceType::Rook),
            flags::PROMOTION_QUEEN => Some(PieceType::Queen),
            _ => None,
        }
    }

    #[inline]
    pub const fn raw(self) -> u16 {
        self.0
    }
}

impl fmt::Display for Move {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_none() {
            return write!(f, "0000");
        }
        write!(f, "{}{}", self.from(), self.to())?;
        if let Some(promo) = self.promotion_piece() {
            let c = match promo {
                PieceType::Knight => 'n',
                PieceType::Bishop => 'b',
                PieceType::Rook => 'r',
                PieceType::Queen => 'q',
                _ => 'q',
            };
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

/// Castling rights bitflags: White K/Q, Black K/Q.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct CastlingRights(u8);

impl CastlingRights {
    pub const NONE: CastlingRights = CastlingRights(0);
    pub const WHITE_KING: CastlingRights = CastlingRights(1);
    pub const WHITE_QUEEN: CastlingRights = CastlingRights(2);
    pub const BLACK_KING: CastlingRights = CastlingRights(4);
    pub const BLACK_QUEEN: CastlingRights = CastlingRights(8);
    pub const ALL: CastlingRights = CastlingRights(15);

    #[inline]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn from_bits(bits: u8) -> Self {
        CastlingRights(bits & 15)
    }

    #[inline]
    pub const fn contains(self, other: CastlingRights) -> bool {
        (self.0 & other.0) == other.0
    }

    #[inline]
    pub const fn with(self, other: CastlingRights) -> Self {
        CastlingRights(self.0 | other.0)
    }

    #[inline]
    pub const fn without(self, other: CastlingRights) -> Self {
        CastlingRights(self.0 & !other.0)
    }

    #[inline]
    pub fn remove(&mut self, other: CastlingRights) {
        self.0 &= !other.0;
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl std::ops::BitOr for CastlingRights {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        self.with(rhs)
    }
}

impl std::ops::BitOrAssign for CastlingRights {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
