//! Selective search hooks (P5). Currently no-ops.

use crate::board::Board;
use crate::types::Value;

/// Placeholder for forward pruning (NMP, RFP, razoring, …).
/// Returns `Some(score)` to cut the node early.
#[inline]
pub fn forward_prune(
    _board: &Board,
    _depth: i32,
    _alpha: Value,
    _beta: Value,
    _static_eval: Value,
    _improving: bool,
) -> Option<Value> {
    None
}

/// Placeholder for late-move / futility / history move-loop pruning.
#[inline]
pub fn should_prune_move(_move_count: i32, _depth: i32, _is_quiet: bool) -> bool {
    false
}

/// Placeholder LMR reduction (plies to subtract). Zero until P5-02.
#[inline]
pub fn late_move_reduction(_depth: i32, _move_count: i32) -> i32 {
    0
}
