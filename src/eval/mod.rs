//! Evaluation: NNUE production leaf + HCE fallback (P6).
//!
//! Search uses [`evaluate_nnue_state`] with a synced [`NnueState`]. HCE remains
//! for debug comparisons and tools that lack NNUE state.

mod corrections;
mod hce;
pub mod nnue;
mod pst;

use crate::board::Board;
use crate::eval::nnue::NnueState;
use crate::types::Value;

pub use corrections::apply as apply_nnue_corrections;
pub use nnue::Network;

/// Side-to-move relative HCE (material + tapered PSTs).
#[inline]
pub fn evaluate_hce(board: &Board) -> Value {
    hce::evaluate(board)
}

/// Production search leaf: NNUE + corrections using synced [`NnueState`].
#[inline]
pub fn evaluate_nnue_state(
    board: &Board,
    state: &NnueState,
    optimism: Value,
    corr_hist: Value,
) -> Value {
    nnue::evaluate_with_state(board, state, state.network(), optimism, corr_hist)
}

/// Full refresh NNUE + corrections (UCI `eval`, tests).
#[inline]
pub fn evaluate_nnue(board: &Board, net: &Network) -> Value {
    nnue::evaluate(board, net)
}

/// Default board eval without thread state: HCE (tests / tools that lack NNUE).
#[inline]
pub fn evaluate(board: &Board) -> Value {
    hce::evaluate(board)
}
