//! Midgame piece-square tables (PSTs).
//!
//! Tables are from White's perspective with a1 = index 0 (LERF). Black
//! looks up the same values via rank-flip (`sq ^ 56`). Kings are omitted
//! (zero contribution). No endgame tapering — that is P6-03.

use crate::board::Board;
use crate::types::{Color, PieceType, Square, Value};

/// Midgame PST for pawns (White perspective, a1 = 0).
#[rustfmt::skip]
const PAWN_MG: [Value; 64] = [
    // rank 1
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 2
     5, 10, 10,-20,-20, 10, 10,  5,
    // rank 3
     5, -5,-10,  0,  0,-10, -5,  5,
    // rank 4
     0,  0,  0, 20, 20,  0,  0,  0,
    // rank 5
     5,  5, 10, 25, 25, 10,  5,  5,
    // rank 6
    10, 10, 20, 30, 30, 20, 10, 10,
    // rank 7
    50, 50, 50, 50, 50, 50, 50, 50,
    // rank 8
     0,  0,  0,  0,  0,  0,  0,  0,
];

/// Midgame PST for knights (White perspective, a1 = 0).
#[rustfmt::skip]
const KNIGHT_MG: [Value; 64] = [
    // rank 1
   -50,-40,-30,-30,-30,-30,-40,-50,
    // rank 2
   -40,-20,  0,  5,  5,  0,-20,-40,
    // rank 3
   -30,  5, 10, 15, 15, 10,  5,-30,
    // rank 4
   -30,  0, 15, 20, 20, 15,  0,-30,
    // rank 5
   -30,  5, 15, 20, 20, 15,  5,-30,
    // rank 6
   -30,  0, 10, 15, 15, 10,  0,-30,
    // rank 7
   -40,-20,  0,  0,  0,  0,-20,-40,
    // rank 8
   -50,-40,-30,-30,-30,-30,-40,-50,
];

/// Midgame PST for bishops (White perspective, a1 = 0).
#[rustfmt::skip]
const BISHOP_MG: [Value; 64] = [
    // rank 1
   -20,-10,-10,-10,-10,-10,-10,-20,
    // rank 2
   -10,  0,  0,  0,  0,  0,  0,-10,
    // rank 3
   -10,  0,  5, 10, 10,  5,  0,-10,
    // rank 4
   -10,  5,  5, 10, 10,  5,  5,-10,
    // rank 5
   -10,  0, 10, 10, 10, 10,  0,-10,
    // rank 6
   -10, 10, 10, 10, 10, 10, 10,-10,
    // rank 7
   -10,  5,  0,  0,  0,  0,  5,-10,
    // rank 8
   -20,-10,-10,-10,-10,-10,-10,-20,
];

/// Midgame PST for rooks (White perspective, a1 = 0).
#[rustfmt::skip]
const ROOK_MG: [Value; 64] = [
    // rank 1
     0,  0,  0,  5,  5,  0,  0,  0,
    // rank 2
    -5,  0,  0,  0,  0,  0,  0, -5,
    // rank 3
    -5,  0,  0,  0,  0,  0,  0, -5,
    // rank 4
    -5,  0,  0,  0,  0,  0,  0, -5,
    // rank 5
    -5,  0,  0,  0,  0,  0,  0, -5,
    // rank 6
    -5,  0,  0,  0,  0,  0,  0, -5,
    // rank 7
     5, 10, 10, 10, 10, 10, 10,  5,
    // rank 8
     0,  0,  0,  0,  0,  0,  0,  0,
];

/// Midgame PST for queens (White perspective, a1 = 0).
#[rustfmt::skip]
const QUEEN_MG: [Value; 64] = [
    // rank 1
   -20,-10,-10, -5, -5,-10,-10,-20,
    // rank 2
   -10,  0,  0,  0,  0,  0,  0,-10,
    // rank 3
   -10,  0,  5,  5,  5,  5,  0,-10,
    // rank 4
    -5,  0,  5,  5,  5,  5,  0, -5,
    // rank 5
     0,  0,  5,  5,  5,  5,  0, -5,
    // rank 6
   -10,  5,  5,  5,  5,  5,  0,-10,
    // rank 7
   -10,  0,  5,  0,  0,  0,  0,-10,
    // rank 8
   -20,-10,-10, -5, -5,-10,-10,-20,
];

#[inline]
fn table_for(pt: PieceType) -> Option<&'static [Value; 64]> {
    match pt {
        PieceType::Pawn => Some(&PAWN_MG),
        PieceType::Knight => Some(&KNIGHT_MG),
        PieceType::Bishop => Some(&BISHOP_MG),
        PieceType::Rook => Some(&ROOK_MG),
        PieceType::Queen => Some(&QUEEN_MG),
        PieceType::King => None,
    }
}

/// White-perspective table index for `sq` belonging to `color`.
#[inline]
fn pst_index(sq: Square, color: Color) -> usize {
    match color {
        Color::White => sq.index() as usize,
        Color::Black => (sq.index() ^ 56) as usize,
    }
}

/// Midgame PST bonus for a single piece on `sq`.
#[inline]
pub fn pst_value(pt: PieceType, sq: Square, color: Color) -> Value {
    match table_for(pt) {
        Some(table) => table[pst_index(sq, color)],
        None => 0,
    }
}

/// White-minus-Black midgame PST sum (kings skipped).
pub fn pst_midgame(board: &Board) -> Value {
    let mut score = 0;

    for &pt in &[
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        let bb = board.pieces(pt);

        for sq in (bb & board.pieces_color(Color::White)).squares() {
            score += pst_value(pt, sq, Color::White);
        }
        for sq in (bb & board.pieces_color(Color::Black)).squares() {
            score -= pst_value(pt, sq, Color::Black);
        }
    }

    score
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn knight_center_better_than_rim() {
        let center = pst_value(PieceType::Knight, Square::E5, Color::White);
        let rim = pst_value(PieceType::Knight, Square::A1, Color::White);
        assert!(
            center > rim,
            "knight on e5 ({center}) should beat rim a1 ({rim})"
        );
    }

    #[test]
    fn black_uses_rank_flip() {
        // White knight on e4 == Black knight on e5 (mirrored ranks).
        let white = pst_value(PieceType::Knight, Square::from_file_rank(4, 3).unwrap(), Color::White);
        let black = pst_value(PieceType::Knight, Square::from_file_rank(4, 4).unwrap(), Color::Black);
        assert_eq!(white, black);
    }
}
