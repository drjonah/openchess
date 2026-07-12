//! Selective search hooks (P5). Improving flag, NMP, and LMR are live.

use super::alphabeta::{search, NodeType};
use super::ThreadData;
use crate::board::Board;
use crate::transposition::TranspositionTable;
use crate::types::{Move, Value};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::Mutex;

/// Runtime toggle for null-move pruning (tests flip this for A/B node counts).
pub static NMP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`NMP_ENABLED`].
#[cfg(test)]
pub(super) static NMP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for late-move reductions (tests flip this for A/B node counts).
pub static LMR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`LMR_ENABLED`].
#[cfg(test)]
pub(super) static LMR_TEST_LOCK: Mutex<()> = Mutex::new(());

const LMR_MAX: usize = 64;
const LMR_SCALE: f64 = 2.75;
const LMR_BASE: f64 = 0.5;

/// Precomputed reductions: `floor(BASE + ln(d) * ln(m) / SCALE)`.
static LMR_TABLE: LazyLock<[[i32; LMR_MAX]; LMR_MAX]> = LazyLock::new(|| {
    let mut table = [[0i32; LMR_MAX]; LMR_MAX];
    for d in 1..LMR_MAX {
        for m in 1..LMR_MAX {
            table[d][m] =
                (LMR_BASE + (d as f64).ln() * (m as f64).ln() / LMR_SCALE).floor() as i32;
        }
    }
    table
});

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

/// Late-move reduction in plies (log-depth × log-move table; tune with SPRT later).
///
/// Returns `0` when LMR does not apply (captures, checks, early moves, shallow depth).
#[inline]
pub fn late_move_reduction(
    depth: i32,
    move_count: i32,
    quiet: bool,
    improving: bool,
    in_check: bool,
    gives_check: bool,
) -> i32 {
    if !LMR_ENABLED.load(Ordering::Relaxed) {
        return 0;
    }
    if !quiet || in_check || gives_check || depth < 3 || move_count <= 1 {
        return 0;
    }

    let d = (depth as usize).min(LMR_MAX - 1);
    let m = (move_count as usize).min(LMR_MAX - 1);
    let mut r = LMR_TABLE[d][m];

    if improving {
        r = (r - 1).max(0);
    }

    r.min(depth - 2).max(0)
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

    #[test]
    fn lmr_guards_skip_reduction() {
        // Captures / checks / shallow / first move → 0.
        assert_eq!(late_move_reduction(6, 4, false, false, false, false), 0);
        assert_eq!(late_move_reduction(6, 4, true, false, true, false), 0);
        assert_eq!(late_move_reduction(6, 4, true, false, false, true), 0);
        assert_eq!(late_move_reduction(2, 4, true, false, false, false), 0);
        assert_eq!(late_move_reduction(6, 1, true, false, false, false), 0);
    }

    #[test]
    fn lmr_formula_grows_with_depth_and_moves() {
        let r_shallow = late_move_reduction(3, 2, true, false, false, false);
        let r_deeper = late_move_reduction(8, 2, true, false, false, false);
        let r_later = late_move_reduction(8, 16, true, false, false, false);
        assert!(r_deeper >= r_shallow);
        assert!(r_later >= r_deeper);
        // Table lookup matches floor(BASE + ln(d)*ln(m)/SCALE), then clamp.
        let expected = (LMR_BASE + 8f64.ln() * 16f64.ln() / LMR_SCALE).floor() as i32;
        assert_eq!(r_later, expected.min(8 - 2));
    }

    #[test]
    fn lmr_improving_reduces_reduction() {
        let base = late_move_reduction(8, 12, true, false, false, false);
        let improved = late_move_reduction(8, 12, true, true, false, false);
        if base > 0 {
            assert_eq!(improved, (base - 1).max(0));
        } else {
            assert_eq!(improved, 0);
        }
    }

    #[test]
    fn lmr_clamped_to_depth_minus_two() {
        // Large move index → table value can exceed depth-2 at shallow depths.
        let r = late_move_reduction(3, 60, true, false, false, false);
        assert!(r <= 3 - 2);
        assert!(r >= 0);
    }

    #[test]
    fn lmr_disabled_returns_zero() {
        let _guard = LMR_TEST_LOCK.lock().unwrap();
        let prev = LMR_ENABLED.swap(false, Ordering::Relaxed);
        assert_eq!(late_move_reduction(8, 12, true, false, false, false), 0);
        LMR_ENABLED.store(prev, Ordering::Relaxed);
    }
}
