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

/// Maximum ply distance treated as a mate/TB-range score (matches search stack).
pub const MAX_MATE_PLY: i32 = 128;

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

/// True if `v` is in the winning mate/TB score band.
#[inline]
pub const fn is_win_score(v: Value) -> bool {
    v >= VALUE_MATE - MAX_MATE_PLY
}

/// True if `v` is in the losing mate/TB score band.
#[inline]
pub const fn is_loss_score(v: Value) -> bool {
    v <= -VALUE_MATE + MAX_MATE_PLY
}

/// Convert a search score to a ply-neutral TT score.
///
/// Win scores store `v + ply`; loss scores store `v - ply`; normal scores unchanged.
/// Adjustments move scores toward ±[`VALUE_MATE`] so they stay inside the mate band.
#[inline]
pub const fn value_to_tt(v: Value, ply: i32) -> Value {
    if is_win_score(v) {
        v + ply
    } else if is_loss_score(v) {
        v - ply
    } else {
        v
    }
}

/// Convert a TT score back to a search score at the current ply.
///
/// Win scores probe `v - ply`; loss scores probe `v + ply`; normal scores unchanged.
#[inline]
pub const fn value_from_tt(v: Value, ply: i32) -> Value {
    if is_win_score(v) {
        v - ply
    } else if is_loss_score(v) {
        v + ply
    } else {
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_scores_unchanged() {
        for ply in [0, 3, 10, 50] {
            assert_eq!(value_to_tt(42, ply), 42);
            assert_eq!(value_from_tt(42, ply), 42);
            assert_eq!(value_to_tt(-100, ply), -100);
            assert_eq!(value_from_tt(-100, ply), -100);
            assert_eq!(value_to_tt(0, ply), 0);
            assert_eq!(value_from_tt(0, ply), 0);
        }
    }

    #[test]
    fn win_loss_classification() {
        assert!(is_win_score(mate_in(0)));
        assert!(is_win_score(mate_in(MAX_MATE_PLY)));
        assert!(!is_win_score(mate_in(MAX_MATE_PLY + 1)));
        assert!(is_loss_score(mated_in(0)));
        assert!(is_loss_score(mated_in(MAX_MATE_PLY)));
        assert!(!is_loss_score(mated_in(MAX_MATE_PLY + 1)));
        assert!(!is_win_score(900));
        assert!(!is_loss_score(-900));
    }

    #[test]
    fn mate_roundtrip_same_ply() {
        for ply in [0, 1, 3, 7, 20, 64] {
            let win = mate_in(ply);
            let loss = mated_in(ply);
            assert_eq!(value_from_tt(value_to_tt(win, ply), ply), win);
            assert_eq!(value_from_tt(value_to_tt(loss, ply), ply), loss);
        }
    }

    #[test]
    fn mate_roundtrip_different_store_probe_plies() {
        // Store mate_in(3) as if searched at ply 5; recover at ply 5.
        let win = mate_in(3);
        let stored = value_to_tt(win, 5);
        assert_eq!(value_from_tt(stored, 5), win);

        let loss = mated_in(3);
        let stored_loss = value_to_tt(loss, 5);
        assert_eq!(value_from_tt(stored_loss, 5), loss);
    }

    #[test]
    fn mate_adjust_across_plies() {
        // Mate found at ply 5 should read as mate_in(3) when probed at ply 3.
        let stored = value_to_tt(mate_in(5), 5);
        assert_eq!(value_from_tt(stored, 3), mate_in(3));

        let stored_loss = value_to_tt(mated_in(5), 5);
        assert_eq!(value_from_tt(stored_loss, 3), mated_in(3));
    }

    #[test]
    fn mate_in_three_survives_child_store() {
        // Position scored mate_in(3) at ply 2 still reports mate_in(3) at ply 2.
        let stored = value_to_tt(mate_in(3), 2);
        assert_eq!(value_from_tt(stored, 2), mate_in(3));
        // Same entry at root ply is mate-in-1 from that position.
        assert_eq!(value_from_tt(stored, 0), mate_in(1));
    }
}
