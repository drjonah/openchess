//! Evaluation / mate score helpers (centipawn-like `i32`).

/// Side-to-move relative score in centipawns (approx).
pub type Value = i32;

/// Alias used interchangeably with [`Value`].
pub type Score = Value;

pub const VALUE_ZERO: Value = 0;
pub const VALUE_DRAW: Value = 0;
pub const VALUE_MATE: Value = 32_000;
pub const VALUE_MATED: Value = -VALUE_MATE;
pub const VALUE_INFINITE: Value = 32_001;
pub const VALUE_NONE: Value = 32_002;

/// Material values used by SEE and bootstrap HCE.
pub const PAWN_VALUE: Value = 100;
pub const KNIGHT_VALUE: Value = 320;
pub const BISHOP_VALUE: Value = 330;
pub const ROOK_VALUE: Value = 500;
pub const QUEEN_VALUE: Value = 900;

use crate::types::piece::PieceType;

#[inline]
pub const fn piece_value(pt: PieceType) -> Value {
    match pt {
        PieceType::Pawn => PAWN_VALUE,
        PieceType::Knight => KNIGHT_VALUE,
        PieceType::Bishop => BISHOP_VALUE,
        PieceType::Rook => ROOK_VALUE,
        PieceType::Queen => QUEEN_VALUE,
        PieceType::King => 20_000,
    }
}

/// Mate score at a given ply distance (winning for side to move).
#[inline]
pub const fn mate_in(ply: i32) -> Value {
    VALUE_MATE - ply
}

/// Mated score at a given ply distance.
#[inline]
pub const fn mated_in(ply: i32) -> Value {
    -VALUE_MATE + ply
}
