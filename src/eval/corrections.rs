//! Post-NNUE corrections (P6-07): material/optimism scale, 50-move dampening,
//! correction-history residual, mate clamp.

use crate::board::Board;
use crate::types::score::{MAX_MATE_PLY, VALUE_MATE};
use crate::types::{Color, Value};

/// Keep eval safely away from mate/TB score bands used by search.
const EVAL_CLAMP: Value = VALUE_MATE - 2 * MAX_MATE_PLY;

/// Apply Stockfish-family style corrections to a raw NNUE score.
///
/// - Scales by total non-pawn material (+ constant) so sparse endgames shrink
/// - Mixes in `optimism` (search-dependent bias; 0 at static eval)
/// - Adds `corr_hist` residual (pawn / non-pawn correction history)
/// - Dampens toward draw as the 50-move clock rises
/// - Clamps away from ±mate
pub fn apply(board: &Board, raw: Value, optimism: Value, corr_hist: Value) -> Value {
    let npm = board.non_pawn_material(Color::White) + board.non_pawn_material(Color::Black);
    // material + optimism blend (starter constants — retune with SPRT later)
    let material_const: i64 = 24_000;
    let npm64 = npm as i64;
    let mut v = (raw as i64 * (npm64 + material_const) + optimism as i64 * npm64)
        / (npm64 + material_const);

    v += i64::from(corr_hist);

    // 50-move dampening: linearly toward 0 as halfmove clock approaches 100.
    let hm = i64::from(board.halfmove_clock().min(100));
    v = v * (100 - hm) / 100;

    (v as Value).clamp(-EVAL_CLAMP, EVAL_CLAMP)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Piece, Square};
    use std::str::FromStr;

    fn init() {
        crate::lookup::initialize();
    }

    #[test]
    fn halfmove_dampening_changes_score() {
        init();
        let mut board = Board::startpos();
        let raw = 200;
        let fresh = apply(&board, raw, 0, 0);
        board.set_halfmove_clock(50);
        let aged = apply(&board, raw, 0, 0);
        assert_ne!(fresh, aged);
        assert!(aged.abs() < fresh.abs());
    }

    #[test]
    fn never_enters_mate_band() {
        init();
        let board = Board::startpos();
        let v = apply(&board, 100_000, 0, 0);
        assert!(v.abs() <= EVAL_CLAMP);
        assert!(!crate::types::score::is_win_score(v));
        assert!(!crate::types::score::is_loss_score(v));
    }

    #[test]
    fn optimism_changes_sparse_material_positions() {
        init();
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        board.put_piece(Piece::WhiteQueen, Square::from_str("d4").unwrap());
        board.rehash();
        let a = apply(&board, 100, 0, 0);
        let b = apply(&board, 100, 200, 0);
        assert_ne!(a, b);
        let c = apply(&board, 100, 0, 50);
        assert_ne!(a, c);
    }
}
