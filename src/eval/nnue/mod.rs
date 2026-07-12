//! Incremental NNUE feature transformer, forward head, and load/embed (P6-05/P6-06).
//!
//! Search uses [`evaluate_with_state`] once a network is attached to [`NnueState`].

mod accumulator;
mod features;
mod forward;
mod network;

pub use accumulator::{Accumulator, NnueState};
pub use features::{feature_index, orient_sq, FEATURE_COUNT, L1_SIZE};
pub use forward::evaluate_raw;
pub use network::{Network, L2_SIZE, L3_SIZE};

use crate::board::Board;
use crate::eval::corrections;
use crate::types::Value;

/// Full NNUE eval: incremental FT state → dense forward → P6-07 corrections.
pub fn evaluate(board: &Board, net: &Network) -> Value {
    let mut acc = Accumulator::default();
    acc.refresh(board, net);
    let raw = evaluate_raw(&acc, board.side_to_move(), net);
    corrections::apply(board, raw, 0, 0)
}

/// NNUE eval using an already-synced [`NnueState`] (no refresh).
pub fn evaluate_with_state(
    board: &Board,
    state: &NnueState,
    net: &Network,
    optimism: Value,
    corr_hist: Value,
) -> Value {
    let raw = evaluate_raw(&state.accumulator, board.side_to_move(), net);
    corrections::apply(board, raw, optimism, corr_hist)
}

/// Raw network output before corrections (debug / acceptance tests).
pub fn evaluate_raw_board(board: &Board, net: &Network) -> Value {
    let mut acc = Accumulator::default();
    acc.refresh(board, net);
    evaluate_raw(&acc, board.side_to_move(), net)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        crate::lookup::initialize();
    }

    #[test]
    fn startpos_nnue_finite_stable_and_differs_from_raw_under_corrections() {
        init();
        let board = Board::startpos();
        let net = Network::embedded_shared();
        let a = evaluate(&board, &net);
        let b = evaluate(&board, &net);
        assert_eq!(a, b);
        assert!(a.abs() < EVAL_SAFE, "nnue score not finite/sane: {a}");

        // Use an imbalanced position so dampening has a non-zero score to shrink.
        let mut imbalanced = Board::startpos();
        imbalanced.remove_piece(crate::types::Square::from_str("d1").unwrap());
        imbalanced.rehash();
        let at_zero = {
            let mut b0 = imbalanced.clone();
            b0.set_halfmove_clock(0);
            evaluate(&b0, &net)
        };
        imbalanced.set_halfmove_clock(80);
        let corrected = evaluate(&imbalanced, &net);
        assert_ne!(
            at_zero, corrected,
            "corrections should change eval when halfmove clock rises"
        );
        assert!(
            corrected.abs() < at_zero.abs() || at_zero == 0,
            "50-move dampening should shrink magnitude"
        );
    }

    const EVAL_SAFE: Value = 50_000;
}

#[cfg(test)]
use std::str::FromStr;
