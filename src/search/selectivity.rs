//! Selective search hooks (P5). Improving flag + null-move pruning are live.

use super::alphabeta::{search, NodeType};
use super::ThreadData;
use crate::board::Board;
use crate::transposition::TranspositionTable;
use crate::types::{Move, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Runtime toggle for null-move pruning (tests flip this for A/B node counts).
pub static NMP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`NMP_ENABLED`].
#[cfg(test)]
pub(super) static NMP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// `true` when static eval is better than ~2 plies ago (and not in check).
#[inline]
pub fn is_improving(static_eval: Value, prev_eval: Value, in_check: bool) -> bool {
    !in_check && static_eval > prev_eval
}

/// Placeholder for static forward pruning (RFP, razoring, …).
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

/// Null-move reduction in plies (starter formula; tune with SPRT later).
#[inline]
pub fn null_move_reduction(depth: i32, improving: bool) -> i32 {
    let mut r = 3 + depth / 4;
    if improving {
        r -= 1;
    }
    r.max(1)
}

/// Attempt null-move pruning on a NonPV node that is not in check.
///
/// Returns `Some(beta)` on a successful fail-high prune.
pub fn try_null_move(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &mut TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    depth: i32,
    beta: Value,
    _static_eval: Value,
    improving: bool,
) -> Option<Value> {
    if !NMP_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    if depth < 3 {
        return None;
    }
    if board.non_pawn_material(board.side_to_move()) == 0 {
        return None;
    }

    debug_assert!(
        !board.in_check(),
        "try_null_move must not run when in check"
    );

    let r = null_move_reduction(depth, improving);
    let null_depth = depth - r - 1;

    td.stack[ply].current_move = Move::NONE;

    board.do_null();
    let score = -search(
        board,
        td,
        tt,
        stop,
        ply + 1,
        null_depth,
        -beta,
        -(beta - 1),
        NodeType::NonPv,
    );
    board.undo_null();

    if score >= beta {
        Some(beta)
    } else {
        None
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use crate::types::Color;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn improving_when_eval_rises() {
        assert!(is_improving(50, 10, false));
        assert!(!is_improving(10, 50, false));
        assert!(!is_improving(50, 50, false));
    }

    #[test]
    fn not_improving_in_check() {
        assert!(!is_improving(100, -100, true));
    }

    #[test]
    fn null_reduction_grows_with_depth() {
        assert!(null_move_reduction(8, false) > null_move_reduction(4, false));
        assert!(null_move_reduction(6, true) < null_move_reduction(6, false));
        assert!(null_move_reduction(1, true) >= 1);
    }

    #[test]
    fn nmp_skipped_when_disabled() {
        init();
        let _guard = NMP_TEST_LOCK.lock().unwrap();
        let prev = NMP_ENABLED.swap(false, Ordering::Relaxed);
        let mut board = Board::startpos();
        let mut td = ThreadData::new();
        let mut tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &mut tt,
            &stop,
            0,
            6,
            50,
            0,
            false
        )
        .is_none());
        NMP_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn nmp_skipped_in_bare_kp() {
        init();
        let mut board = Board::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert_eq!(board.non_pawn_material(Color::White), 0);
        let mut td = ThreadData::new();
        let mut tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &mut tt,
            &stop,
            0,
            6,
            50,
            0,
            false
        )
        .is_none());
    }

    #[test]
    fn nmp_skipped_below_min_depth() {
        init();
        let mut board = Board::startpos();
        let mut td = ThreadData::new();
        let mut tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &mut tt,
            &stop,
            0,
            2,
            50,
            0,
            false
        )
        .is_none());
    }
}
