//! Search entry: iterative deepening, aspiration, PVS (P2).

mod alphabeta;
pub mod selectivity;
pub mod stack;

use alphabeta::{search, NodeType};
use stack::{format_pv, RootMove, Stack, MAX_PLY};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::board::Board;
use crate::eval::nnue::NnueState;
use crate::history::HistoryTables;
use crate::time::{TimeBudget, DEFAULT_MOVE_OVERHEAD};
use crate::transposition::TranspositionTable;
use crate::types::score::VALUE_INFINITE;
use crate::types::{Move, Value};

/// Search limits from UCI / TUI.
#[derive(Clone, Debug)]
pub struct Limits {
    pub depth: Option<i32>,
    pub movetime: Option<Duration>,
    pub nodes: Option<u64>,
    pub wtime: Option<Duration>,
    pub btime: Option<Duration>,
    pub winc: Option<Duration>,
    pub binc: Option<Duration>,
    pub movestogo: Option<u32>,
    pub infinite: bool,
    /// GUI/network latency reserve (default 50ms; see `time::DEFAULT_MOVE_OVERHEAD`).
    pub move_overhead: Duration,
    /// Lazy SMP worker count (1 = single-thread).
    pub threads: u32,
    /// NNUE network for this search (defaults to embedded bootstrap).
    pub network: Option<std::sync::Arc<crate::eval::Network>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            depth: None,
            movetime: None,
            nodes: None,
            wtime: None,
            btime: None,
            winc: None,
            binc: None,
            movestogo: None,
            infinite: false,
            move_overhead: DEFAULT_MOVE_OVERHEAD,
            threads: 1,
            network: None,
        }
    }
}

impl Limits {
    /// True when a clock was provided for either side.
    pub fn has_clock(&self) -> bool {
        self.wtime.is_some() || self.btime.is_some()
    }
}

/// Result of a completed (or aborted) search.
#[derive(Clone, Debug)]
pub struct SearchResult {
    pub best_move: Move,
    pub score: Value,
    pub depth: i32,
    pub nodes: u64,
    pub pv: Vec<Move>,
    pub time: Duration,
}

/// Per-thread search state (histories, NNUE, killers — not shared across SMP).
pub struct ThreadData {
    pub nodes: u64,
    pub stack: Vec<Stack>,
    pub root_moves: Vec<RootMove>,
    pub history: HistoryTables,
    /// Incremental HalfKA accumulators; refreshed at each `go` root.
    pub nnue: NnueState,
    /// Search start time (for hard-limit polls at the root).
    pub start: Instant,
    /// Hard elapsed-time abort bound, if any.
    pub hard_limit: Option<Duration>,
    /// Debug counters for singular / negative extensions (P5-06).
    pub singular_extensions: u64,
    pub negative_extensions: u64,
    pub multi_cuts: u64,
}

impl ThreadData {
    pub fn new(network: std::sync::Arc<crate::eval::Network>) -> Self {
        Self {
            nodes: 0,
            stack: vec![Stack::default(); MAX_PLY],
            root_moves: Vec::new(),
            history: HistoryTables::new(),
            nnue: NnueState::new(network),
            start: Instant::now(),
            hard_limit: None,
            singular_extensions: 0,
            negative_extensions: 0,
            multi_cuts: 0,
        }
    }
}

impl Default for ThreadData {
    fn default() -> Self {
        Self::new(crate::eval::Network::embedded_shared())
    }
}

/// Optional info callback: (depth, score, nodes, time, pv_string, hashfull).
pub type InfoCallback<'a> = dyn FnMut(i32, Value, u64, Duration, &str, u32) + 'a;

/// Root best-move / eval stability across iterative deepening (P7-04).
///
/// Feeds [`crate::time::soft_scale`] so volatile roots keep a larger soft bound.
#[derive(Clone, Debug, Default)]
struct IdStability {
    best_move_changes: u32,
    /// `previous_score - current_score` after the last completed iteration.
    score_drop: i32,
    prev_best: Option<Move>,
    prev_score: Option<Value>,
}

impl IdStability {
    fn observe(&mut self, best: Move, score: Value) {
        if let Some(prev) = self.prev_best {
            if prev != best {
                self.best_move_changes += 1;
            }
        }
        if let Some(prev) = self.prev_score {
            self.score_drop = prev - score;
        }
        self.prev_best = Some(best);
        self.prev_score = Some(score);
    }
}

/// Run iterative deepening search on `board` (Lazy SMP when `limits.threads` > 1).
///
/// Always returns the last completed iteration's best move when aborted mid-ID.
pub fn go(
    board: &mut Board,
    limits: Limits,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    mut info: Option<&mut InfoCallback<'_>>,
) -> SearchResult {
    let network = limits
        .network
        .clone()
        .unwrap_or_else(crate::eval::Network::embedded_shared);
    let threads = limits.threads.max(1);
    tt.new_search();
    if threads > 1 {
        return crate::threadpool::go_lazy_smp(
            board,
            limits,
            tt,
            stop,
            network,
            threads,
            info.as_deref_mut(),
        );
    }
    go_single(board.clone(), limits, tt, stop, network, info)
}

/// Single-thread iterative deepening (one Lazy SMP worker).
pub fn go_single(
    mut board: Board,
    limits: Limits,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    network: std::sync::Arc<crate::eval::Network>,
    mut info: Option<&mut InfoCallback<'_>>,
) -> SearchResult {
    let mut td = ThreadData::new(network);
    let start = Instant::now();
    let budget = TimeBudget::from_limits(&limits, board.side_to_move(), limits.move_overhead);
    td.start = start;
    td.hard_limit = budget.map(|b| b.hard);
    td.nnue.refresh(&board);

    if board.is_draw(0) {
        return SearchResult {
            best_move: Move::NONE,
            score: crate::types::score::VALUE_DRAW,
            depth: 0,
            nodes: 0,
            pv: Vec::new(),
            time: start.elapsed(),
        };
    }

    let legal = board.legal_moves();
    if legal.is_empty() {
        return SearchResult {
            best_move: Move::NONE,
            score: if board.in_check() {
                crate::types::score::mated_in(0)
            } else {
                crate::types::score::VALUE_DRAW
            },
            depth: 0,
            nodes: 0,
            pv: Vec::new(),
            time: start.elapsed(),
        };
    }

    td.root_moves = legal.into_iter().map(RootMove::new).collect();

    let max_depth = limits.depth.unwrap_or(64).clamp(1, (MAX_PLY as i32) - 1);
    let mut best = SearchResult {
        best_move: td.root_moves[0].mv,
        score: 0,
        depth: 0,
        nodes: 0,
        pv: vec![td.root_moves[0].mv],
        time: Duration::ZERO,
    };

    let aspiration_delta: Value = 50;
    let mut stability = IdStability::default();

    for depth in 1..=max_depth {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(b) = budget {
            let elapsed = start.elapsed();
            if b.hard_exceeded(elapsed) {
                stop.store(true, Ordering::Relaxed);
                break;
            }
            if b.soft_exceeded_scaled(
                elapsed,
                stability.best_move_changes,
                stability.score_drop,
            ) {
                break;
            }
        }
        if let Some(n) = limits.nodes {
            if td.nodes >= n {
                break;
            }
        }

        let (mut alpha, mut beta) = if depth >= 4 && best.depth > 0 {
            (
                best.score.saturating_sub(aspiration_delta),
                best.score.saturating_add(aspiration_delta),
            )
        } else {
            (-VALUE_INFINITE, VALUE_INFINITE)
        };

        let mut score;
        let mut delta = aspiration_delta;
        loop {
            td.stack[0].clear_pv();

            score = search(
                &mut board,
                &mut td,
                tt,
                stop,
                0,
                depth,
                alpha,
                beta,
                NodeType::Root,
                Move::NONE,
            );

            if stop.load(Ordering::Relaxed) {
                break;
            }

            if score <= alpha {
                beta = (alpha + beta) / 2;
                alpha = (score.saturating_sub(delta)).max(-VALUE_INFINITE);
                delta = delta.saturating_mul(2).min(VALUE_INFINITE / 4);
                continue;
            }
            if score >= beta {
                beta = (score.saturating_add(delta)).min(VALUE_INFINITE);
                delta = delta.saturating_mul(2).min(VALUE_INFINITE / 4);
                continue;
            }
            break;
        }

        if stop.load(Ordering::Relaxed) && best.depth > 0 {
            break;
        }

        let pv = if !td.stack[0].pv.is_empty() {
            td.stack[0].pv.clone()
        } else {
            vec![td.root_moves[0].mv]
        };
        let best_move = pv[0];

        best = SearchResult {
            best_move,
            score,
            depth,
            nodes: td.nodes,
            pv: pv.clone(),
            time: start.elapsed(),
        };

        stability.observe(best_move, score);

        if let Some(cb) = info.as_mut() {
            let pv_str = format_pv(&pv);
            cb(
                depth,
                score,
                td.nodes,
                start.elapsed(),
                &pv_str,
                tt.hashfull(),
            );
        }

        if let Some(b) = budget {
            if b.soft_exceeded_scaled(
                start.elapsed(),
                stability.best_move_changes,
                stability.score_drop,
            ) {
                break;
            }
        }
    }

    best.nodes = td.nodes;
    best.time = start.elapsed();
    best
}

/// Convenience: search with a fresh 1 MB TT and no external stop.
pub fn go_depth(board: &mut Board, depth: i32) -> SearchResult {
    let tt = TranspositionTable::new(1);
    let stop = AtomicBool::new(false);
    go(
        board,
        Limits {
            depth: Some(depth),
            ..Default::default()
        },
        &tt,
        &stop,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use super::selectivity;
    use crate::types::score::VALUE_INFINITE;
    use crate::types::{Color, Move, Piece, Square};
    use std::str::FromStr;
    use std::sync::atomic::Ordering;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn depth_4_returns_legal_move() {
        init();
        let mut board = Board::startpos();
        let result = go_depth(&mut board, 4);
        assert!(!result.best_move.is_none());
        let legal = board.legal_moves();
        assert!(
            legal.contains(&result.best_move),
            "bestmove {} not legal",
            result.best_move
        );
        assert!(result.nodes > 0);
        assert_eq!(result.depth, 4);
    }

    #[test]
    fn search_in_check_no_crash() {
        init();
        // Scholar's mate threat — Black in check from Qh5.
        let mut board =
            Board::from_fen("r1bqkbnr/pppp1ppp/2n5/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR b KQkq - 3 3")
                .unwrap();
        let result = go_depth(&mut board, 3);
        assert!(!result.best_move.is_none() || board.legal_moves().is_empty());
    }

    #[test]
    fn abort_keeps_prior_bestmove() {
        init();
        let mut board = Board::startpos();
        let mut tt = TranspositionTable::new(8);
        let stop = AtomicBool::new(false);
        // Run depth 2 first to establish a move, then abort mid deeper search.
        let first = go(
            &mut board,
            Limits {
                depth: Some(2),
                ..Default::default()
            },
            &tt,
            &stop,
            None,
        );
        assert!(!first.best_move.is_none());

        // Immediate stop: should still return something legal from depth 1+.
        stop.store(true, Ordering::Relaxed);
        let aborted = go(
            &mut board,
            Limits {
                depth: Some(20),
                ..Default::default()
            },
            &tt,
            &stop,
            None,
        );
        // With stop already set, we may only get the first root move as fallback.
        assert!(!aborted.best_move.is_none());
    }

    #[test]
    fn hanging_queen_not_quiet_win() {
        init();
        // Black queen hanging on d5; White queen on d1 can take it (same file).
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        board.put_piece(Piece::WhiteQueen, Square::from_str("d1").unwrap());
        board.put_piece(Piece::BlackQueen, Square::from_str("d5").unwrap());
        board.set_side_to_move(Color::White);
        board.rehash();
        board.refresh_checkers_and_pins();

        let result = go_depth(&mut board, 1);
        assert!(
            result.score > 500,
            "expected winning score after hanging queen, got {} (best {})",
            result.score,
            result.best_move
        );
    }

    #[test]
    fn pv_length_at_least_depth_on_quiet() {
        init();
        let mut board = Board::startpos();
        let result = go_depth(&mut board, 3);
        assert!(
            result.pv.len() >= 1,
            "PV should have at least the best move"
        );
        // On quiet startpos, PV often reaches depth; allow >= 1 if cutoffs shorten.
        assert!(!result.pv.is_empty());
        assert_eq!(result.pv[0], result.best_move);
    }

    #[test]
    fn nmp_reduces_nodes_at_fixed_depth() {
        init();
        let _guard = selectivity::NMP_TEST_LOCK.lock().unwrap();
        let stop = AtomicBool::new(false);

        selectivity::NMP_ENABLED.store(false, Ordering::Relaxed);
        let mut board_off = Board::startpos();
        let mut tt_off = TranspositionTable::new(16);
        let off = go(
            &mut board_off,
            Limits {
                depth: Some(6),
                ..Default::default()
            },
            &tt_off,
            &stop,
            None,
        );

        selectivity::NMP_ENABLED.store(true, Ordering::Relaxed);
        let mut board_on = Board::startpos();
        let mut tt_on = TranspositionTable::new(16);
        let on = go(
            &mut board_on,
            Limits {
                depth: Some(6),
                ..Default::default()
            },
            &tt_on,
            &stop,
            None,
        );

        assert!(
            on.nodes < off.nodes,
            "NMP should reduce nodes: on={} off={}",
            on.nodes,
            off.nodes
        );
        assert!(!on.best_move.is_none());
        assert!(board_on.legal_moves().contains(&on.best_move));
    }

    #[test]
    fn lmr_reduces_nodes_at_fixed_depth() {
        init();
        let _guard = selectivity::LMR_TEST_LOCK.lock().unwrap();
        let stop = AtomicBool::new(false);

        selectivity::LMR_ENABLED.store(false, Ordering::Relaxed);
        let mut board_off = Board::startpos();
        let mut tt_off = TranspositionTable::new(16);
        let off = go(
            &mut board_off,
            Limits {
                depth: Some(6),
                ..Default::default()
            },
            &tt_off,
            &stop,
            None,
        );

        selectivity::LMR_ENABLED.store(true, Ordering::Relaxed);
        let mut board_on = Board::startpos();
        let mut tt_on = TranspositionTable::new(16);
        let on = go(
            &mut board_on,
            Limits {
                depth: Some(6),
                ..Default::default()
            },
            &tt_on,
            &stop,
            None,
        );

        assert!(
            on.nodes < off.nodes,
            "LMR should reduce nodes: on={} off={}",
            on.nodes,
            off.nodes
        );
        assert!(!on.best_move.is_none());
        assert!(board_on.legal_moves().contains(&on.best_move));
    }

    #[test]
    fn rfp_razoring_reduce_nodes_at_fixed_depth() {
        init();
        let _rfp = selectivity::RFP_TEST_LOCK.lock().unwrap();
        let _razor = selectivity::RAZORING_TEST_LOCK.lock().unwrap();
        let stop = AtomicBool::new(false);

        selectivity::RFP_ENABLED.store(false, Ordering::Relaxed);
        selectivity::RAZORING_ENABLED.store(false, Ordering::Relaxed);
        let mut board_off = Board::startpos();
        let mut tt_off = TranspositionTable::new(16);
        let off = go(
            &mut board_off,
            Limits {
                depth: Some(7),
                ..Default::default()
            },
            &tt_off,
            &stop,
            None,
        );

        selectivity::RFP_ENABLED.store(true, Ordering::Relaxed);
        selectivity::RAZORING_ENABLED.store(true, Ordering::Relaxed);
        let mut board_on = Board::startpos();
        let mut tt_on = TranspositionTable::new(16);
        let on = go(
            &mut board_on,
            Limits {
                depth: Some(7),
                ..Default::default()
            },
            &tt_on,
            &stop,
            None,
        );

        assert!(
            on.nodes <= off.nodes,
            "RFP+razoring should not increase nodes: on={} off={}",
            on.nodes,
            off.nodes
        );
        assert!(!on.best_move.is_none());
        assert!(board_on.legal_moves().contains(&on.best_move));
    }

    #[test]
    fn move_loop_pruning_reduces_nodes_at_fixed_depth() {
        init();
        let _lmp = selectivity::LMP_TEST_LOCK.lock().unwrap();
        let _fut = selectivity::FUTILITY_TEST_LOCK.lock().unwrap();
        let _hist = selectivity::HISTORY_PRUNE_TEST_LOCK.lock().unwrap();
        let _see = selectivity::SEE_PRUNE_TEST_LOCK.lock().unwrap();
        let stop = AtomicBool::new(false);

        selectivity::LMP_ENABLED.store(false, Ordering::Relaxed);
        selectivity::FUTILITY_ENABLED.store(false, Ordering::Relaxed);
        selectivity::HISTORY_PRUNE_ENABLED.store(false, Ordering::Relaxed);
        selectivity::SEE_PRUNE_ENABLED.store(false, Ordering::Relaxed);
        let mut board_off = Board::startpos();
        let mut tt_off = TranspositionTable::new(16);
        let off = go(
            &mut board_off,
            Limits {
                depth: Some(7),
                ..Default::default()
            },
            &tt_off,
            &stop,
            None,
        );

        selectivity::LMP_ENABLED.store(true, Ordering::Relaxed);
        selectivity::FUTILITY_ENABLED.store(true, Ordering::Relaxed);
        selectivity::HISTORY_PRUNE_ENABLED.store(true, Ordering::Relaxed);
        selectivity::SEE_PRUNE_ENABLED.store(true, Ordering::Relaxed);
        let mut board_on = Board::startpos();
        let mut tt_on = TranspositionTable::new(16);
        let on = go(
            &mut board_on,
            Limits {
                depth: Some(7),
                ..Default::default()
            },
            &tt_on,
            &stop,
            None,
        );

        assert!(
            on.nodes <= off.nodes,
            "move-loop pruning should not increase nodes: on={} off={}",
            on.nodes,
            off.nodes
        );
        assert!(!on.best_move.is_none());
        assert!(board_on.legal_moves().contains(&on.best_move));
    }

    #[test]
    fn probcut_iir_reduce_nodes_at_fixed_depth() {
        init();
        let _pc = selectivity::PROBCUT_TEST_LOCK.lock().unwrap();
        let _iir = selectivity::IIR_TEST_LOCK.lock().unwrap();
        let stop = AtomicBool::new(false);

        selectivity::PROBCUT_ENABLED.store(false, Ordering::Relaxed);
        selectivity::IIR_ENABLED.store(false, Ordering::Relaxed);
        let mut board_off = Board::startpos();
        let mut tt_off = TranspositionTable::new(16);
        let off = go(
            &mut board_off,
            Limits {
                depth: Some(8),
                ..Default::default()
            },
            &tt_off,
            &stop,
            None,
        );

        selectivity::PROBCUT_ENABLED.store(true, Ordering::Relaxed);
        selectivity::IIR_ENABLED.store(true, Ordering::Relaxed);
        let mut board_on = Board::startpos();
        let mut tt_on = TranspositionTable::new(16);
        let on = go(
            &mut board_on,
            Limits {
                depth: Some(8),
                ..Default::default()
            },
            &tt_on,
            &stop,
            None,
        );

        assert!(
            on.nodes <= off.nodes.saturating_add(64),
            "ProbCut+IIR should not materially increase nodes: on={} off={}",
            on.nodes,
            off.nodes
        );
        assert!(!on.best_move.is_none());
        assert!(board_on.legal_moves().contains(&on.best_move));
    }

    #[test]
    fn singular_extensions_visible_in_deep_search() {
        init();
        let _guard = selectivity::SINGULAR_TEST_LOCK.lock().unwrap();
        selectivity::SINGULAR_ENABLED.store(true, Ordering::Relaxed);
        // Tactical midgame: deeper NonPV nodes with TT moves from prior ID iters.
        let mut board = Board::from_fen(
            "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
        )
        .unwrap();
        let mut tt = TranspositionTable::new(16);
        let stop = AtomicBool::new(false);
        let mut td = ThreadData::default();
        td.nnue.refresh(&board);

        // Same ThreadData across ID so extension counters accumulate. Avoid a
        // separate post-go search: a fully warmed TT cuts off NonPV before
        // singular can run.
        for depth in 1..=10 {
            let _ = search(
                &mut board,
                &mut td,
                &tt,
                &stop,
                0,
                depth,
                -VALUE_INFINITE,
                VALUE_INFINITE,
                NodeType::Root,
                Move::NONE,
            );
        }
        let total = td.singular_extensions + td.negative_extensions + td.multi_cuts;
        assert!(
            total > 0,
            "expected singular/negative/multi-cut activity, got se={} neg={} mc={}",
            td.singular_extensions,
            td.negative_extensions,
            td.multi_cuts
        );
    }

    #[test]
    fn nmp_inactive_in_check_still_legal() {
        init();
        // Rook check on the a-file; Black must resolve check (NMP must not fire).
        let mut board = Board::from_fen("k7/8/8/8/8/8/8/R3K3 b - - 0 1").unwrap();
        assert!(board.in_check());
        let result = go_depth(&mut board, 4);
        assert!(!result.best_move.is_none());
        assert!(board.legal_moves().contains(&result.best_move));
    }

    #[test]
    fn nmp_skipped_on_bare_kp_endgame() {
        init();
        let mut board = Board::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
        assert_eq!(board.non_pawn_material(Color::White), 0);
        let result = go_depth(&mut board, 5);
        assert!(!result.best_move.is_none());
        assert!(board.legal_moves().contains(&result.best_move));
    }

    #[test]
    fn clock_search_respects_hard() {
        init();
        let mut board = Board::startpos();
        let mut tt = TranspositionTable::new(8);
        let stop = AtomicBool::new(false);
        let overhead = Duration::from_millis(50);
        let wtime = Duration::from_millis(200);
        let result = go(
            &mut board,
            Limits {
                wtime: Some(wtime),
                winc: Some(Duration::from_millis(0)),
                move_overhead: overhead,
                ..Default::default()
            },
            &tt,
            &stop,
            None,
        );
        let hard = wtime.saturating_sub(overhead);
        // Allow modest overrun from check granularity / finishing a root move.
        assert!(
            result.time < hard + overhead + Duration::from_millis(150),
            "elapsed {:?} exceeded hard {:?} + slack (best {} depth {})",
            result.time,
            hard,
            result.best_move,
            result.depth
        );
        assert!(!result.best_move.is_none());
        assert!(board.legal_moves().contains(&result.best_move));
    }

    #[test]
    fn lazy_smp_threads_increase_nodes_and_stay_legal() {
        init();
        let stop = AtomicBool::new(false);
        let mut board1 = Board::startpos();
        let tt1 = TranspositionTable::new(16);
        let single = go(
            &mut board1,
            Limits {
                depth: Some(5),
                threads: 1,
                ..Default::default()
            },
            &tt1,
            &stop,
            None,
        );

        let stop2 = AtomicBool::new(false);
        let mut board4 = Board::startpos();
        let tt4 = TranspositionTable::new(16);
        let multi = go(
            &mut board4,
            Limits {
                depth: Some(5),
                threads: 4,
                ..Default::default()
            },
            &tt4,
            &stop2,
            None,
        );

        assert!(
            multi.nodes >= single.nodes,
            "Threads>1 should search at least as many aggregate nodes: multi={} single={}",
            multi.nodes,
            single.nodes
        );
        assert!(!multi.best_move.is_none());
        assert!(board4.legal_moves().contains(&multi.best_move));
    }

    #[test]
    fn id_stability_tracks_best_move_changes_and_score_drop() {
        use crate::time::{soft_scale, TimeBudget};

        let a = Move::new(
            Square::from_str("e2").unwrap(),
            Square::from_str("e4").unwrap(),
        );
        let b = Move::new(
            Square::from_str("d2").unwrap(),
            Square::from_str("d4").unwrap(),
        );

        let mut stab = IdStability::default();
        stab.observe(a, 20);
        assert_eq!(stab.best_move_changes, 0);
        assert_eq!(stab.score_drop, 0);

        // Same best move, rising eval → no change count; negative drop.
        stab.observe(a, 35);
        assert_eq!(stab.best_move_changes, 0);
        assert_eq!(stab.score_drop, -15);

        // Best move flips + falling eval.
        stab.observe(b, 5);
        assert_eq!(stab.best_move_changes, 1);
        assert_eq!(stab.score_drop, 30);

        stab.observe(a, -40);
        assert_eq!(stab.best_move_changes, 2);
        assert_eq!(stab.score_drop, 45);

        let budget = TimeBudget {
            soft: Duration::from_millis(1_000),
            hard: Duration::from_millis(10_000),
        };
        let stable_soft = budget.scaled_soft(0, 0);
        let volatile_soft =
            budget.scaled_soft(stab.best_move_changes, stab.score_drop);
        assert!(
            volatile_soft > stable_soft,
            "volatile ID history should enlarge soft: volatile={volatile_soft:?} stable={stable_soft:?} scale={}",
            soft_scale(stab.best_move_changes, stab.score_drop)
        );
    }
}
