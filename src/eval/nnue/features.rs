//! HalfKA-style sparse feature indices for the incremental feature transformer.
//!
//! For each perspective (White / Black king), active features are every piece
//! *except* that perspective's own king, indexed by the oriented king square
//! and the oriented piece square. Black's perspective mirrors ranks (`sq ^ 56`)
//! so both sides share one weight table layout.

use crate::types::{Color, Piece, PieceType, Square};

/// Accumulator width (per perspective). Phase-C bootstrap size (was 64 stub).
pub const L1_SIZE: usize = 256;

/// Feature dimensionality: 64 king squares × 12 piece slots × 64 piece squares.
pub const FEATURE_COUNT: usize = 64 * 12 * 64;

/// Orient a square into the perspective's coordinate frame (rank-flip for Black).
#[inline]
pub fn orient_sq(perspective: Color, sq: Square) -> usize {
    let idx = sq.index() as usize;
    if perspective == Color::White {
        idx
    } else {
        idx ^ 56
    }
}

/// Piece slot relative to `perspective`: our pieces `0..6`, enemy `6..12`.
///
/// Slot order matches [`PieceType`] indices (Pawn..King). The perspective's
/// own king is never a feature (its square indexes the HalfKA axis).
#[inline]
pub fn oriented_piece_slot(perspective: Color, piece: Piece) -> usize {
    let pt = piece.piece_type().expect("empty piece has no slot").index();
    let ours = piece.color().expect("empty piece has no color") == perspective;
    if ours { pt } else { 6 + pt }
}

/// HalfKA feature index for `piece` on `sq` under `perspective`'s king.
///
/// Returns `None` when `piece` is empty or is the perspective's own king.
#[inline]
pub fn feature_index(
    perspective: Color,
    king: Square,
    piece: Piece,
    sq: Square,
) -> Option<usize> {
    if piece.is_empty() {
        return None;
    }
    if piece.piece_type() == Some(PieceType::King) && piece.color() == Some(perspective) {
        return None;
    }
    let king_o = orient_sq(perspective, king);
    let sq_o = orient_sq(perspective, sq);
    let slot = oriented_piece_slot(perspective, piece);
    Some(king_o * (12 * 64) + slot * 64 + sq_o)
}

/// Decode slot from a HalfKA feature index (for bootstrap weight init).
#[inline]
pub fn feature_slot(feature: usize) -> usize {
    (feature / 64) % 12
}

/// Decode oriented piece square from a HalfKA feature index.
#[inline]
pub fn feature_sq(feature: usize) -> usize {
    feature % 64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn own_king_is_not_a_feature() {
        assert!(feature_index(
            Color::White,
            Square::E1,
            Piece::WhiteKing,
            Square::E1
        )
        .is_none());
        assert!(feature_index(
            Color::Black,
            Square::E8,
            Piece::BlackKing,
            Square::E8
        )
        .is_none());
    }

    #[test]
    fn enemy_king_is_a_feature() {
        let idx = feature_index(
            Color::White,
            Square::E1,
            Piece::BlackKing,
            Square::E8,
        )
        .unwrap();
        assert!(idx < FEATURE_COUNT);
    }

    #[test]
    fn black_perspective_mirrors_ranks() {
        let e2 = Square::from_file_rank(4, 1).unwrap();
        let e7 = Square::from_file_rank(4, 6).unwrap();
        let white_idx = feature_index(
            Color::White,
            Square::E1,
            Piece::WhitePawn,
            e2,
        )
        .unwrap();
        let black_idx = feature_index(
            Color::Black,
            Square::E8,
            Piece::BlackPawn,
            e7,
        )
        .unwrap();
        assert_eq!(white_idx, black_idx);
    }
}
