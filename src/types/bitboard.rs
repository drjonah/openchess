//! Bitboard: 64-bit occupancy set (a1 = bit 0).

use crate::types::square::Square;
use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, BitXor, BitXorAssign, Not};

/// Set of squares packed into a `u64`.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Bitboard(pub u64);

impl Bitboard {
    pub const EMPTY: Bitboard = Bitboard(0);
    pub const ALL: Bitboard = Bitboard(u64::MAX);

    pub const FILE_A: Bitboard = Bitboard(0x0101_0101_0101_0101);
    pub const FILE_H: Bitboard = Bitboard(0x8080_8080_8080_8080);
    pub const RANK_1: Bitboard = Bitboard(0x0000_0000_0000_00FF);
    pub const RANK_8: Bitboard = Bitboard(0xFF00_0000_0000_0000);

    #[inline]
    pub const fn new(bits: u64) -> Self {
        Bitboard(bits)
    }

    #[inline]
    pub const fn from_square(sq: Square) -> Self {
        Bitboard(1u64 << sq.index())
    }

    #[inline]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    #[inline]
    pub const fn any(self) -> bool {
        self.0 != 0
    }

    #[inline]
    pub const fn contains(self, sq: Square) -> bool {
        (self.0 & (1u64 << sq.index())) != 0
    }

    #[inline]
    pub const fn with(self, sq: Square) -> Self {
        Bitboard(self.0 | (1u64 << sq.index()))
    }

    #[inline]
    pub const fn without(self, sq: Square) -> Self {
        Bitboard(self.0 & !(1u64 << sq.index()))
    }

    #[inline]
    pub fn set(&mut self, sq: Square) {
        self.0 |= 1u64 << sq.index();
    }

    #[inline]
    pub fn clear(&mut self, sq: Square) {
        self.0 &= !(1u64 << sq.index());
    }

    #[inline]
    pub const fn count(self) -> u32 {
        self.0.count_ones()
    }

    #[inline]
    pub const fn lsb(self) -> Option<Square> {
        if self.0 == 0 {
            None
        } else {
            Some(Square::from_index_unchecked(self.0.trailing_zeros() as u8))
        }
    }

    #[inline]
    pub const fn msb(self) -> Option<Square> {
        if self.0 == 0 {
            None
        } else {
            Some(Square::from_index_unchecked(63 - self.0.leading_zeros() as u8))
        }
    }

    /// Pop least-significant bit and return its square.
    #[inline]
    pub fn pop_lsb(&mut self) -> Option<Square> {
        let sq = self.lsb()?;
        self.0 &= self.0 - 1;
        Some(sq)
    }

    #[inline]
    pub const fn north(self) -> Bitboard {
        Bitboard(self.0 << 8)
    }

    #[inline]
    pub const fn south(self) -> Bitboard {
        Bitboard(self.0 >> 8)
    }

    #[inline]
    pub const fn east(self) -> Bitboard {
        Bitboard((self.0 << 1) & !Self::FILE_A.0)
    }

    #[inline]
    pub const fn west(self) -> Bitboard {
        Bitboard((self.0 >> 1) & !Self::FILE_H.0)
    }

    #[inline]
    pub const fn north_east(self) -> Bitboard {
        Bitboard((self.0 << 9) & !Self::FILE_A.0)
    }

    #[inline]
    pub const fn north_west(self) -> Bitboard {
        Bitboard((self.0 << 7) & !Self::FILE_H.0)
    }

    #[inline]
    pub const fn south_east(self) -> Bitboard {
        Bitboard((self.0 >> 7) & !Self::FILE_A.0)
    }

    #[inline]
    pub const fn south_west(self) -> Bitboard {
        Bitboard((self.0 >> 9) & !Self::FILE_H.0)
    }

    pub fn squares(self) -> BitboardIter {
        BitboardIter { bits: self.0 }
    }
}

pub struct BitboardIter {
    bits: u64,
}

impl Iterator for BitboardIter {
    type Item = Square;

    #[inline]
    fn next(&mut self) -> Option<Square> {
        if self.bits == 0 {
            None
        } else {
            let sq = Square::from_index_unchecked(self.bits.trailing_zeros() as u8);
            self.bits &= self.bits - 1;
            Some(sq)
        }
    }
}

impl Not for Bitboard {
    type Output = Bitboard;
    #[inline]
    fn not(self) -> Bitboard {
        Bitboard(!self.0)
    }
}

macro_rules! bit_ops {
    ($($trait:ident, $method:ident, $assign_trait:ident, $assign_method:ident);+ $(;)?) => {
        $(
            impl $trait for Bitboard {
                type Output = Bitboard;
                #[inline]
                fn $method(self, rhs: Bitboard) -> Bitboard {
                    Bitboard(self.0.$method(rhs.0))
                }
            }
            impl $assign_trait for Bitboard {
                #[inline]
                fn $assign_method(&mut self, rhs: Bitboard) {
                    self.0.$assign_method(rhs.0);
                }
            }
        )+
    };
}

bit_ops! {
    BitAnd, bitand, BitAndAssign, bitand_assign;
    BitOr, bitor, BitOrAssign, bitor_assign;
    BitXor, bitxor, BitXorAssign, bitxor_assign;
}

impl fmt::Debug for Bitboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Bitboard(0x{:016X})", self.0)
    }
}

impl fmt::Display for Bitboard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..8).rev() {
            for file in 0..8 {
                let sq = Square::from_file_rank(file, rank).unwrap();
                write!(f, "{}", if self.contains(sq) { '1' } else { '.' })?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
