//! Piece-square tables with MG/EG tapering (P6-03).
//!
//! Tables are from White's perspective with a1 = index 0 (LERF). Black
//! looks up the same values via rank-flip (`sq ^ 56`). Kings are omitted
//! (zero contribution). Game phase from non-pawn material interpolates
//! midgame and endgame PSTs: `score = (mg * phase + eg * (24 - phase)) / 24`.

use crate::board::Board;
use crate::types::{Color, PieceType, Square, Value};

/// Maximum game phase (full non-pawn material of both sides).
pub const PHASE_MAX: i32 = 24;

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

/// Endgame PST for pawns — PeSTO-style advancement bonus, scaled to MG magnitude.
#[rustfmt::skip]
const PAWN_EG: [Value; 64] = [
    // rank 1
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 2
    10, 10, 10, 10, 10, 10, 10, 10,
    // rank 3
    10, 10, 20, 30, 30, 20, 10, 10,
    // rank 4
    20, 20, 30, 40, 40, 30, 20, 20,
    // rank 5
    30, 30, 40, 50, 50, 40, 30, 30,
    // rank 6
    50, 50, 60, 70, 70, 60, 50, 50,
    // rank 7
    80, 80, 80, 80, 80, 80, 80, 80,
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

/// Endgame PST for knights — still prefer center, flatter than MG.
#[rustfmt::skip]
const KNIGHT_EG: [Value; 64] = [
    // rank 1
   -40,-30,-20,-20,-20,-20,-30,-40,
    // rank 2
   -30,-15,  0,  5,  5,  0,-15,-30,
    // rank 3
   -20,  0, 10, 12, 12, 10,  0,-20,
    // rank 4
   -20,  5, 12, 15, 15, 12,  5,-20,
    // rank 5
   -20,  5, 12, 15, 15, 12,  5,-20,
    // rank 6
   -20,  0, 10, 12, 12, 10,  0,-20,
    // rank 7
   -30,-15,  0,  5,  5,  0,-15,-30,
    // rank 8
   -40,-30,-20,-20,-20,-20,-30,-40,
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

/// Endgame PST for bishops — long diagonals / activity matter more.
#[rustfmt::skip]
const BISHOP_EG: [Value; 64] = [
    // rank 1
   -15,-10,-10, -5, -5,-10,-10,-15,
    // rank 2
   -10,  0,  5,  5,  5,  5,  0,-10,
    // rank 3
   -10,  5, 10, 10, 10, 10,  5,-10,
    // rank 4
    -5,  5, 10, 15, 15, 10,  5, -5,
    // rank 5
    -5,  5, 10, 15, 15, 10,  5, -5,
    // rank 6
   -10,  5, 10, 10, 10, 10,  5,-10,
    // rank 7
   -10,  0,  5,  5,  5,  5,  0,-10,
    // rank 8
   -15,-10,-10, -5, -5,-10,-10,-15,
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

/// Endgame PST for rooks — 7th rank and open files remain strong.
#[rustfmt::skip]
const ROOK_EG: [Value; 64] = [
    // rank 1
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 2
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 3
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 4
     0,  0,  0,  0,  0,  0,  0,  0,
    // rank 5
     5,  5,  5,  5,  5,  5,  5,  5,
    // rank 6
    10, 10, 10, 10, 10, 10, 10, 10,
    // rank 7
    25, 25, 25, 25, 25, 25, 25, 25,
    // rank 8
    15, 15, 15, 15, 15, 15, 15, 15,
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

/// Endgame PST for queens — centralization rewarded more than in MG.
#[rustfmt::skip]
const QUEEN_EG: [Value; 64] = [
    // rank 1
   -20,-10,-10, -5, -5,-10,-10,-20,
    // rank 2
   -10,  0,  5,  5,  5,  5,  0,-10,
    // rank 3
   -10,  5, 10, 10, 10, 10,  5,-10,
    // rank 4
    -5,  5, 10, 15, 15, 10,  5, -5,
    // rank 5
    -5,  5, 10, 15, 15, 10,  5, -5,
    // rank 6
   -10,  5, 10, 10, 10, 10,  5,-10,
    // rank 7
   -10,  0,  5,  5,  5,  5,  0,-10,
    // rank 8
   -20,-10,-10, -5, -5,-10,-10,-20,
];

#[inline]
fn table_mg(pt: PieceType) -> Option<&'static [Value; 64]> {
    match pt {
        PieceType::Pawn => Some(&PAWN_MG),
        PieceType::Knight => Some(&KNIGHT_MG),
        PieceType::Bishop => Some(&BISHOP_MG),
        PieceType::Rook => Some(&ROOK_MG),
        PieceType::Queen => Some(&QUEEN_MG),
        PieceType::King => None,
    }
}

#[inline]
fn table_eg(pt: PieceType) -> Option<&'static [Value; 64]> {
    match pt {
        PieceType::Pawn => Some(&PAWN_EG),
        PieceType::Knight => Some(&KNIGHT_EG),
        PieceType::Bishop => Some(&BISHOP_EG),
        PieceType::Rook => Some(&ROOK_EG),
        PieceType::Queen => Some(&QUEEN_EG),
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

/// Phase weight for non-pawn material (pawns and kings contribute 0).
#[inline]
pub fn phase_weight(pt: PieceType) -> i32 {
    match pt {
        PieceType::Pawn | PieceType::King => 0,
        PieceType::Knight | PieceType::Bishop => 1,
        PieceType::Rook => 2,
        PieceType::Queen => 4,
    }
}

/// Game phase from non-pawn material: 0 = pure endgame, [`PHASE_MAX`] = midgame.
pub fn game_phase(board: &Board) -> i32 {
    let mut phase = 0;
    for &pt in &[
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        phase += phase_weight(pt) * board.pieces(pt).count() as i32;
    }
    phase.min(PHASE_MAX)
}

/// Midgame PST bonus for a single piece on `sq`.
#[inline]
pub fn pst_value_mg(pt: PieceType, sq: Square, color: Color) -> Value {
    match table_mg(pt) {
        Some(table) => table[pst_index(sq, color)],
        None => 0,
    }
}

/// Endgame PST bonus for a single piece on `sq`.
#[inline]
pub fn pst_value_eg(pt: PieceType, sq: Square, color: Color) -> Value {
    match table_eg(pt) {
        Some(table) => table[pst_index(sq, color)],
        None => 0,
    }
}

/// Interpolate MG/EG scores by game phase (P6-03).
#[inline]
pub fn taper(mg: Value, eg: Value, phase: i32) -> Value {
    let phase = phase.clamp(0, PHASE_MAX);
    (mg * phase + eg * (PHASE_MAX - phase)) / PHASE_MAX
}

fn accumulate_pst(board: &Board, value_fn: fn(PieceType, Square, Color) -> Value) -> Value {
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
            score += value_fn(pt, sq, Color::White);
        }
        for sq in (bb & board.pieces_color(Color::Black)).squares() {
            score -= value_fn(pt, sq, Color::Black);
        }
    }

    score
}

/// White-minus-Black midgame PST sum (kings skipped).
pub fn pst_midgame(board: &Board) -> Value {
    accumulate_pst(board, pst_value_mg)
}

/// White-minus-Black endgame PST sum (kings skipped).
pub fn pst_endgame(board: &Board) -> Value {
    accumulate_pst(board, pst_value_eg)
}

/// White-minus-Black tapered PST sum (P6-03).
pub fn pst_tapered(board: &Board) -> Value {
    let mg = pst_midgame(board);
    let eg = pst_endgame(board);
    taper(mg, eg, game_phase(board))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Piece;
    use std::str::FromStr;

    #[test]
    fn knight_center_better_than_rim() {
        let center = pst_value_mg(PieceType::Knight, Square::E5, Color::White);
        let rim = pst_value_mg(PieceType::Knight, Square::A1, Color::White);
        assert!(
            center > rim,
            "knight on e5 ({center}) should beat rim a1 ({rim})"
        );
    }

    #[test]
    fn black_uses_rank_flip() {
        // White knight on e4 == Black knight on e5 (mirrored ranks).
        let white = pst_value_mg(
            PieceType::Knight,
            Square::from_file_rank(4, 3).unwrap(),
            Color::White,
        );
        let black = pst_value_mg(
            PieceType::Knight,
            Square::from_file_rank(4, 4).unwrap(),
            Color::Black,
        );
        assert_eq!(white, black);
    }

    #[test]
    fn startpos_phase_is_max() {
        assert_eq!(game_phase(&Board::startpos()), PHASE_MAX);
    }

    #[test]
    fn kp_vs_k_phase_is_zero() {
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        board.put_piece(Piece::WhitePawn, Square::from_str("e5").unwrap());
        board.rehash();
        assert_eq!(game_phase(&board), 0);
    }

    #[test]
    fn mg_eg_endpoints_differ_sensibly() {
        // MG knight-center style differs from EG on the same square.
        let e5 = Square::E5;
        assert_ne!(
            pst_value_mg(PieceType::Knight, e5, Color::White),
            pst_value_eg(PieceType::Knight, e5, Color::White)
        );

        // Advanced pawns are worth more in the endgame than midgame.
        let e7 = Square::from_file_rank(4, 6).unwrap();
        assert!(
            pst_value_eg(PieceType::Pawn, e7, Color::White)
                > pst_value_mg(PieceType::Pawn, e7, Color::White),
            "e7 pawn should score higher in EG than MG"
        );
    }

    #[test]
    fn taper_endpoints() {
        assert_eq!(taper(100, 0, PHASE_MAX), 100);
        assert_eq!(taper(100, 0, 0), 0);
        assert_eq!(taper(100, 0, 12), 50);
    }
}
