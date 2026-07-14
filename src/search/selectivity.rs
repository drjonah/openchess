//! Selective search hooks (P5).
//!
//! Live: improving, NMP, LMR, RFP, razoring, LMP/futility/history/SEE prune,
//! ProbCut, IIR, singular/multi-cut/negative extensions.

use super::alphabeta::{qsearch, search, NodeType};
use super::ThreadData;
use crate::board::Board;
use crate::transposition::TranspositionTable;
use crate::types::{Move, Value};
use crate::types::score::VALUE_INFINITE;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::LazyLock;
#[cfg(test)]
use std::sync::Mutex;

/// Runtime toggle for null-move pruning (tests flip this for A/B node counts).
pub static NMP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`NMP_ENABLED`].
#[cfg(test)]
pub static NMP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Global serialization for *all* selectivity A/B tests.
///
/// Every search reads every toggle, but individual A/B tests only lock their own
/// feature lock. Without a shared gate, e.g. the LMR test can flip `LMR_ENABLED`
/// while the NMP test is mid-search, corrupting its node counts. All
/// toggle-manipulating tests (and the bench signature) acquire this first so at
/// most one runs at a time. Poison-tolerant via [`ab_test_guard`] so one genuine
/// failure does not cascade into unrelated `PoisonError`s.
#[cfg(test)]
pub static SELECTIVITY_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the global [`SELECTIVITY_TEST_LOCK`], tolerating poisoning.
#[cfg(test)]
pub fn ab_test_guard() -> std::sync::MutexGuard<'static, ()> {
    SELECTIVITY_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Runtime toggle for late-move reductions (tests flip this for A/B node counts).
pub static LMR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`LMR_ENABLED`].
#[cfg(test)]
pub static LMR_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for reverse futility pruning.
pub static RFP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`RFP_ENABLED`].
#[cfg(test)]
pub static RFP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for razoring.
pub static RAZORING_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`RAZORING_ENABLED`].
#[cfg(test)]
pub static RAZORING_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for late-move pruning.
pub static LMP_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`LMP_ENABLED`].
#[cfg(test)]
pub static LMP_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for futility pruning in the move loop.
pub static FUTILITY_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`FUTILITY_ENABLED`].
#[cfg(test)]
pub static FUTILITY_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for history-based quiet pruning.
pub static HISTORY_PRUNE_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`HISTORY_PRUNE_ENABLED`].
#[cfg(test)]
pub static HISTORY_PRUNE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for SEE pruning of losing captures in main search.
pub static SEE_PRUNE_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`SEE_PRUNE_ENABLED`].
#[cfg(test)]
pub static SEE_PRUNE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for ProbCut.
pub static PROBCUT_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`PROBCUT_ENABLED`].
#[cfg(test)]
pub static PROBCUT_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Runtime toggle for Internal Iterative Reduction.
pub static IIR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`IIR_ENABLED`].
#[cfg(test)]
pub static IIR_TEST_LOCK: Mutex<()> = Mutex::new(());

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

/// Max depth for reverse futility pruning (starter; tune with SPRT later).
const RFP_MAX_DEPTH: i32 = 6;
/// Per-ply RFP margin (cp).
const RFP_MARGIN: Value = 75;

/// Max depth for razoring.
const RAZOR_MAX_DEPTH: i32 = 3;
/// Base razoring margin (cp).
const RAZOR_MARGIN: Value = 300;

/// `true` when static eval is better than ~2 plies ago (and not in check).
#[inline]
pub fn is_improving(static_eval: Value, prev_eval: Value, in_check: bool) -> bool {
    !in_check && static_eval > prev_eval
}

/// Reverse futility pruning: high eval at low depth → fail high.
///
/// Returns `Some(static_eval)` when the node can be pruned. Caller must already
/// gate on NonPV and not-in-check.
#[inline]
pub fn try_rfp(
    depth: i32,
    beta: Value,
    static_eval: Value,
    improving: bool,
) -> Option<Value> {
    if !RFP_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    if depth <= 0 || depth > RFP_MAX_DEPTH {
        return None;
    }
    let mut margin = RFP_MARGIN * depth;
    if improving {
        margin -= RFP_MARGIN;
    }
    if static_eval - margin >= beta {
        Some(static_eval)
    } else {
        None
    }
}

/// Razoring: very low eval at shallow depth → drop to qsearch.
///
/// Returns `Some(score)` from quiescence when razoring triggers.
pub fn try_razoring(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    depth: i32,
    alpha: Value,
    beta: Value,
    static_eval: Value,
) -> Option<Value> {
    if !RAZORING_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    if depth <= 0 || depth > RAZOR_MAX_DEPTH {
        return None;
    }
    let margin = RAZOR_MARGIN * depth;
    if static_eval + margin > alpha {
        return None;
    }
    let score = qsearch(board, td, tt, stop, ply, alpha, beta, 0);
    Some(score)
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
    tt: &TranspositionTable,
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

    // Null move flips side-to-move only — no piece deltas — so the NNUE
    // accumulator stays valid without an observer (see NnueState tests).
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
        Move::NONE,
    );
    board.undo_null();

    if score >= beta {
        Some(beta)
    } else {
        None
    }
}

/// Max depth for move-loop futility pruning.
const FUTILITY_MAX_DEPTH: i32 = 4;
/// Per-ply futility margin (cp).
const FUTILITY_MARGIN: Value = 100;

/// History score below which late quiets may be pruned.
const HISTORY_PRUNE_THRESHOLD: i32 = -4000;

/// SEE threshold for skipping losing captures near the leaves (NonPV).
const SEE_PRUNE_DEPTH: i32 = 4;

/// Context for move-loop pruning decisions (LMP / futility / history / SEE).
#[derive(Clone, Copy, Debug)]
pub struct MovePruneCtx {
    pub move_count: i32,
    pub depth: i32,
    pub quiet: bool,
    pub is_pv: bool,
    pub in_check: bool,
    pub improving: bool,
    pub static_eval: Value,
    pub alpha: Value,
    pub hist_score: i32,
    pub see_score: Value,
}

/// Late-move / futility / history / SEE move-loop pruning.
///
/// Returns `true` when the move should be skipped. Never prunes the first move,
/// PV nodes, or positions in check.
#[inline]
pub fn should_prune_move(ctx: MovePruneCtx) -> bool {
    if ctx.is_pv || ctx.in_check || ctx.move_count <= 1 {
        return false;
    }

    if ctx.quiet {
        if LMP_ENABLED.load(Ordering::Relaxed) {
            // Improving positions keep more moves; threshold grows with depth².
            let base = if ctx.improving { 3 + ctx.depth } else { 2 + ctx.depth };
            let limit = base + ctx.depth * ctx.depth / 2;
            if ctx.move_count > limit {
                return true;
            }
        }

        if FUTILITY_ENABLED.load(Ordering::Relaxed)
            && ctx.depth <= FUTILITY_MAX_DEPTH
            && ctx.static_eval + FUTILITY_MARGIN * ctx.depth <= ctx.alpha
        {
            return true;
        }

        if HISTORY_PRUNE_ENABLED.load(Ordering::Relaxed)
            && ctx.depth <= 4
            && ctx.move_count > 3
            && ctx.hist_score < HISTORY_PRUNE_THRESHOLD
        {
            return true;
        }
    } else if SEE_PRUNE_ENABLED.load(Ordering::Relaxed)
        && ctx.depth <= SEE_PRUNE_DEPTH
        && ctx.see_score < 0
    {
        return true;
    }

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

/// History-aware LMR tweak: increase reduction for low-history quiets.
#[inline]
pub fn lmr_history_adjustment(hist_score: i32) -> i32 {
    if hist_score < -2000 {
        1
    } else if hist_score > 2000 {
        -1
    } else {
        0
    }
}

/// Min depth for ProbCut.
const PROBCUT_MIN_DEPTH: i32 = 5;
/// ProbCut shallow reduction from current depth.
const PROBCUT_REDUCTION: i32 = 4;
/// ProbCut beta margin (cp).
const PROBCUT_MARGIN: Value = 100;
/// Max captures to try in ProbCut.
const PROBCUT_MOVES: i32 = 3;

/// ProbCut: shallow capture search to prove a fail-high.
///
/// Returns `Some(beta)` when a capture proves a cutoff at reduced depth.
pub fn try_probcut(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    depth: i32,
    beta: Value,
    static_eval: Value,
    tt_move: Move,
) -> Option<Value> {
    use crate::movepick::{is_quiet, HistoryContext, MovePicker};
    use crate::history::ContSlot;
    use super::stack::MAX_PLY;

    if !PROBCUT_ENABLED.load(Ordering::Relaxed) {
        return None;
    }
    if depth < PROBCUT_MIN_DEPTH {
        return None;
    }
    // Only attempt when static eval already looks like a cut.
    if static_eval < beta {
        return None;
    }

    let pc_beta = beta + PROBCUT_MARGIN;
    let pc_depth = (depth - PROBCUT_REDUCTION).max(1);

    let killers = td.stack[ply].killers;
    let mut stack_cont = [ContSlot::NONE; MAX_PLY];
    for (i, s) in td.stack.iter().enumerate().take(MAX_PLY) {
        stack_cont[i] = s.cont_slot;
    }
    let hctx = HistoryContext::new(
        &td.history,
        &killers,
        &stack_cont,
        ply,
        board.side_to_move(),
    );
    let mut picker = MovePicker::new(
        board,
        if tt_move.is_none() {
            None
        } else {
            Some(tt_move)
        },
        &hctx,
    );

    let mut tried = 0i32;
    while let Some(mv) = picker.next() {
        if is_quiet(board, mv) {
            continue;
        }
        if board.see(mv) < 0 {
            continue;
        }
        tried += 1;
        if tried > PROBCUT_MOVES {
            break;
        }

        let moving_piece = board.piece_on(mv.from());
        board.make_observed(mv, Some(&mut td.nnue));
        td.stack[ply + 1].cont_slot = ContSlot::new(moving_piece, mv.to());
        let score = -search(
            board,
            td,
            tt,
            stop,
            ply + 1,
            pc_depth - 1,
            -pc_beta,
            -(pc_beta - 1),
            NodeType::NonPv,
            Move::NONE,
        );
        board.unmake_observed(mv, Some(&mut td.nnue));

        if stop.load(Ordering::Relaxed) {
            return None;
        }
        if score >= pc_beta {
            return Some(beta);
        }
    }
    None
}

/// Internal Iterative Reduction: reduce depth when there is no TT move.
///
/// Returns the (possibly reduced) depth to use for the move loop.
#[inline]
pub fn apply_iir(depth: i32, tt_hit: bool, is_pv: bool) -> i32 {
    if !IIR_ENABLED.load(Ordering::Relaxed) {
        return depth;
    }
    // PV nodes keep full depth for accuracy; NonPV without TT move reduce by 1.
    if !is_pv && !tt_hit && depth >= 6 {
        depth - 1
    } else {
        depth
    }
}

/// Runtime toggle for singular extensions.
pub static SINGULAR_ENABLED: AtomicBool = AtomicBool::new(true);

/// Serializes tests that flip [`SINGULAR_ENABLED`].
#[cfg(test)]
pub static SINGULAR_TEST_LOCK: Mutex<()> = Mutex::new(());

/// Min depth for singular extension attempts.
const SINGULAR_MIN_DEPTH: i32 = 8;
/// Margin subtracted from TT value for the singular beta.
const SINGULAR_MARGIN: Value = 64;

/// Outcome of a singular-extension probe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SingularResult {
    /// No singular logic applied.
    None,
    /// TT move is singular — extend by this many plies (1).
    Extend(i32),
    /// TT move is not singular — reduce its depth.
    Negative,
    /// Multiple moves beat singular beta — prune the node with this score.
    MultiCut(Value),
}

/// Singular extension / multi-cut / negative extension probe.
///
/// Runs a reduced search excluding `tt_move`. Caller applies the result to
/// the TT move's extension and may return early on multi-cut.
pub fn try_singular(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    depth: i32,
    beta: Value,
    tt_move: Move,
    tt_value: Value,
    tt_depth: i32,
    in_check: bool,
    is_pv: bool,
) -> SingularResult {
    if !SINGULAR_ENABLED.load(Ordering::Relaxed) {
        return SingularResult::None;
    }
    if is_pv || in_check || tt_move.is_none() {
        return SingularResult::None;
    }
    if depth < SINGULAR_MIN_DEPTH || tt_depth < depth - 3 {
        return SingularResult::None;
    }
    // Avoid singular around mate scores.
    if tt_value.abs() >= VALUE_INFINITE / 4 {
        return SingularResult::None;
    }

    let se_beta = tt_value - SINGULAR_MARGIN;
    let se_depth = ((depth - 1) / 2).max(1);

    // Ensure stack room for the verification search.
    while td.stack.len() <= ply + 1 {
        td.stack.push(super::stack::Stack::default());
    }

    let score = search(
        board,
        td,
        tt,
        stop,
        ply,
        se_depth,
        se_beta - 1,
        se_beta,
        NodeType::NonPv,
        tt_move,
    );

    if stop.load(Ordering::Relaxed) {
        return SingularResult::None;
    }

    if score < se_beta {
        SingularResult::Extend(1)
    } else if score >= beta {
        // Multi-cut: even without the TT move we fail high.
        SingularResult::MultiCut(score)
    } else {
        SingularResult::Negative
    }
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
        let _ab = ab_test_guard();
        let _guard = NMP_TEST_LOCK.lock().unwrap();
        let prev = NMP_ENABLED.swap(false, Ordering::Relaxed);
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &tt,
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
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &tt,
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
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_null_move(
            &mut board,
            &mut td,
            &tt,
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
    fn rfp_cuts_when_eval_far_above_beta() {
        // depth 4, margin = 75*4 = 300; eval 500 - 300 >= beta 100 → cut
        assert_eq!(try_rfp(4, 100, 500, false), Some(500));
    }

    #[test]
    fn rfp_skipped_when_improving_tightens_margin() {
        // depth 4, improving: margin = 300-75 = 225; 300-225 = 75 < beta 100 → no cut
        assert!(try_rfp(4, 100, 300, true).is_none());
        assert!(try_rfp(4, 100, 300, false).is_none());
        assert_eq!(try_rfp(4, 100, 400, false), Some(400));
    }

    #[test]
    fn rfp_skipped_above_max_depth() {
        assert!(try_rfp(7, 0, 10_000, false).is_none());
    }

    #[test]
    fn rfp_disabled_returns_none() {
        let _ab = ab_test_guard();
        let _guard = RFP_TEST_LOCK.lock().unwrap();
        let prev = RFP_ENABLED.swap(false, Ordering::Relaxed);
        assert!(try_rfp(4, 0, 10_000, false).is_none());
        RFP_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn razoring_skipped_when_eval_not_low() {
        init();
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_razoring(
            &mut board,
            &mut td,
            &tt,
            &stop,
            0,
            2,
            -50,
            50,
            0
        )
        .is_none());
    }

    #[test]
    fn razoring_triggers_on_very_low_eval() {
        init();
        let _ab = ab_test_guard();
        let _guard = RAZORING_TEST_LOCK.lock().unwrap();
        RAZORING_ENABLED.store(true, Ordering::Relaxed);
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        let score = try_razoring(
            &mut board,
            &mut td,
            &tt,
            &stop,
            0,
            2,
            0,
            100,
            -1000,
        );
        assert!(score.is_some());
    }

    #[test]
    fn razoring_disabled_returns_none() {
        init();
        let _ab = ab_test_guard();
        let _guard = RAZORING_TEST_LOCK.lock().unwrap();
        let prev = RAZORING_ENABLED.swap(false, Ordering::Relaxed);
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        assert!(try_razoring(
            &mut board,
            &mut td,
            &tt,
            &stop,
            0,
            2,
            0,
            100,
            -1000
        )
        .is_none());
        RAZORING_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn singular_disabled_returns_none() {
        init();
        let _ab = ab_test_guard();
        let _guard = SINGULAR_TEST_LOCK.lock().unwrap();
        let prev = SINGULAR_ENABLED.swap(false, Ordering::Relaxed);
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        let result = try_singular(
            &mut board,
            &mut td,
            &tt,
            &stop,
            0,
            10,
            50,
            Move::NONE,
            0,
            10,
            false,
            false,
        );
        assert_eq!(result, SingularResult::None);
        SINGULAR_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn singular_skipped_shallow_or_pv() {
        init();
        let mut board = Board::startpos();
        let mut td = ThreadData::default();
        let tt = TranspositionTable::new(1);
        let stop = AtomicBool::new(false);
        // Shallow
        assert_eq!(
            try_singular(
                &mut board,
                &mut td,
                &tt,
                &stop,
                0,
                4,
                50,
                Move::NONE,
                0,
                4,
                false,
                false
            ),
            SingularResult::None
        );
        // PV
        assert_eq!(
            try_singular(
                &mut board,
                &mut td,
                &tt,
                &stop,
                0,
                10,
                50,
                Move::NONE,
                0,
                10,
                false,
                true
            ),
            SingularResult::None
        );
    }

    #[test]
    fn iir_reduces_nonpv_without_tt() {
        assert_eq!(apply_iir(8, false, false), 7);
        assert_eq!(apply_iir(8, true, false), 8);
        assert_eq!(apply_iir(8, false, true), 8);
        assert_eq!(apply_iir(5, false, false), 5);
    }

    #[test]
    fn iir_disabled_keeps_depth() {
        let _ab = ab_test_guard();
        let _guard = IIR_TEST_LOCK.lock().unwrap();
        let prev = IIR_ENABLED.swap(false, Ordering::Relaxed);
        assert_eq!(apply_iir(8, false, false), 8);
        IIR_ENABLED.store(prev, Ordering::Relaxed);
    }

    #[test]
    fn lmp_prunes_late_quiets() {
        let ctx = MovePruneCtx {
            move_count: 40,
            depth: 4,
            quiet: true,
            is_pv: false,
            in_check: false,
            improving: false,
            static_eval: 0,
            alpha: 0,
            hist_score: 0,
            see_score: 0,
        };
        assert!(should_prune_move(ctx));
    }

    #[test]
    fn lmp_skips_pv_and_first_move() {
        let mut ctx = MovePruneCtx {
            move_count: 40,
            depth: 4,
            quiet: true,
            is_pv: true,
            in_check: false,
            improving: false,
            static_eval: 0,
            alpha: 0,
            hist_score: 0,
            see_score: 0,
        };
        assert!(!should_prune_move(ctx));
        ctx.is_pv = false;
        ctx.move_count = 1;
        assert!(!should_prune_move(ctx));
    }

    #[test]
    fn futility_prunes_low_eval_quiets() {
        let ctx = MovePruneCtx {
            move_count: 3,
            depth: 2,
            quiet: true,
            is_pv: false,
            in_check: false,
            improving: false,
            static_eval: -500,
            alpha: 0,
            hist_score: 0,
            see_score: 0,
        };
        assert!(should_prune_move(ctx));
    }

    #[test]
    fn see_prunes_losing_captures() {
        let ctx = MovePruneCtx {
            move_count: 2,
            depth: 3,
            quiet: false,
            is_pv: false,
            in_check: false,
            improving: false,
            static_eval: 0,
            alpha: 0,
            hist_score: 0,
            see_score: -200,
        };
        assert!(should_prune_move(ctx));
    }

    #[test]
    fn lmr_guards_skip_reduction() {
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
        let r = late_move_reduction(3, 60, true, false, false, false);
        assert!(r <= 3 - 2);
        assert!(r >= 0);
    }

    #[test]
    fn lmr_disabled_returns_zero() {
        let _ab = ab_test_guard();
        let _guard = LMR_TEST_LOCK.lock().unwrap();
        let prev = LMR_ENABLED.swap(false, Ordering::Relaxed);
        assert_eq!(late_move_reduction(8, 12, true, false, false, false), 0);
        LMR_ENABLED.store(prev, Ordering::Relaxed);
    }
}
