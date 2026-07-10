//! Board squares: a1 = 0 … h8 = 63 (little-endian rank-file).

use crate::types::bitboard::Bitboard;
use std::fmt;
use std::str::FromStr;

/// A square on the chessboard (0..63).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Square(u8);

impl Square {
    pub const COUNT: usize = 64;

    pub const A1: Square = Square(0);
    pub const B1: Square = Square(1);
    pub const C1: Square = Square(2);
    pub const D1: Square = Square(3);
    pub const E1: Square = Square(4);
    pub const F1: Square = Square(5);
    pub const G1: Square = Square(6);
    pub const H1: Square = Square(7);
    pub const D5: Square = Square(35);
    pub const E5: Square = Square(36);
    pub const A7: Square = Square(48);
    pub const A8: Square = Square(56);
    pub const B8: Square = Square(57);
    pub const C8: Square = Square(58);
    pub const D8: Square = Square(59);
    pub const E8: Square = Square(60);
    pub const F8: Square = Square(61);
    pub const G8: Square = Square(62);
    pub const H8: Square = Square(63);

    #[inline]
    pub const fn new(index: u8) -> Option<Self> {
        if index < 64 {
            Some(Square(index))
        } else {
            None
        }
    }

    /// # Safety
    /// `index` must be in `0..64`.
    #[inline]
    pub const fn from_index_unchecked(index: u8) -> Self {
        Square(index)
    }

    #[inline]
    pub const fn from_file_rank(file: u8, rank: u8) -> Option<Self> {
        if file < 8 && rank < 8 {
            Some(Square(rank * 8 + file))
        } else {
            None
        }
    }

    #[inline]
    pub const fn index(self) -> u8 {
        self.0
    }

    #[inline]
    pub const fn file(self) -> u8 {
        self.0 & 7
    }

    #[inline]
    pub const fn rank(self) -> u8 {
        self.0 >> 3
    }

    #[inline]
    pub const fn bitboard(self) -> Bitboard {
        Bitboard(1u64 << self.0)
    }

    #[inline]
    pub const fn offset(self, df: i8, dr: i8) -> Option<Self> {
        let file = self.file() as i8 + df;
        let rank = self.rank() as i8 + dr;
        if file >= 0 && file < 8 && rank >= 0 && rank < 8 {
            Some(Square((rank as u8) * 8 + file as u8))
        } else {
            None
        }
    }

    pub fn all() -> impl Iterator<Item = Square> {
        (0u8..64).map(Square)
    }
}

impl fmt::Display for Square {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let file = (b'a' + self.file()) as char;
        let rank = (b'1' + self.rank()) as char;
        write!(f, "{file}{rank}")
    }
}

impl FromStr for Square {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bytes = s.as_bytes();
        if bytes.len() != 2 {
            return Err(());
        }
        let file = bytes[0].wrapping_sub(b'a');
        let rank = bytes[1].wrapping_sub(b'1');
        Self::from_file_rank(file, rank).ok_or(())
    }
}
