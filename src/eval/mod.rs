//! Evaluation: HCE bootstrap, later NNUE.

mod hce;

use crate::board::Board;
use crate::types::Value;

/// Side-to-move relative evaluation of `board`.
#[inline]
pub fn evaluate(board: &Board) -> Value {
    hce::evaluate(board)
}
