//! Side to move / piece color.

use std::ops::Not;

/// White or Black.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Color {
    White = 0,
    Black = 1,
}

impl Color {
    pub const COUNT: usize = 2;

    #[inline]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[inline]
    pub const fn from_index(index: usize) -> Self {
        match index {
            0 => Color::White,
            _ => Color::Black,
        }
    }
}

impl Not for Color {
    type Output = Color;

    #[inline]
    fn not(self) -> Color {
        match self {
            Color::White => Color::Black,
            Color::Black => Color::White,
        }
    }
}
