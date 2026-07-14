//! Quiet-position filter for NNUE training samples.
//!
//! Search-labeled nets train poorly on tactically forced positions. We keep
//! positions that are not in check and have no non-losing capture (SEE ≥ 0).

use crate::board::Board;
use crate::types::Move;

/// Return true when `board` is suitable as a quiet training sample.
pub fn is_quiet_training_position(board: &Board) -> bool {
    if board.in_check() {
        return false;
    }
    let mut captures = Vec::new();
    board.generate_captures(&mut captures);
    for mv in &captures {
        if board.see(*mv) >= 0 {
            return false;
        }
    }
    true
}

/// Best capture SEE among legal captures, or `None` if there are none.
#[allow(dead_code)]
pub fn best_capture_see(board: &Board) -> Option<i32> {
    let mut captures = Vec::new();
    board.generate_captures(&mut captures);
    captures.into_iter().map(|mv: Move| board.see(mv)).max()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn startpos_is_quiet() {
        init();
        assert!(is_quiet_training_position(&Board::startpos()));
    }

    #[test]
    fn check_is_not_quiet() {
        init();
        // Scholar's mate mid-check: 1.e4 e5 2.Qh5 Nc6 3.Bc4 Nf6 4.Qxf7+
        let board = Board::from_fen(
            "r1bqkb1r/pppp1Qpp/2n2n2/4p3/2B1P3/8/PPPP1PPP/RNB1K1NR b KQkq - 0 4",
        )
        .unwrap();
        assert!(board.in_check());
        assert!(!is_quiet_training_position(&board));
    }

    #[test]
    fn winning_capture_available_is_not_quiet() {
        init();
        // After 1.e4 d5 White can take on d5 with SEE ≥ 0.
        let board = Board::from_fen(
            "rnbqkbnr/ppp1pppp/8/3p4/4P3/8/PPPP1PPP/RNBQKBNR w KQkq d6 0 2",
        )
        .unwrap();
        assert!(!is_quiet_training_position(&board));
    }
}
