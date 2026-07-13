//! Fixed-position search bench (P8-00).
//!
//! Runs a deterministic fixed-depth search on a small suite of positions and
//! reports total nodes. Used by UCI `bench` and CI signature tests.

use crate::board::Board;
use crate::lookup;
use crate::search::{self, Limits};
use crate::transposition::TranspositionTable;
use crate::types::Move;
use std::sync::atomic::AtomicBool;
use std::time::{Duration, Instant};

/// Default search depth for UCI `bench` and signature tests.
pub const BENCH_DEPTH: i32 = 8;

/// Bench positions: (id, fen). Empty fen means startpos.
pub const BENCH_POSITIONS: &[(&str, &str)] = &[
    ("startpos", ""),
    (
        "kiwipete",
        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
    ),
    (
        "pos3",
        "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1",
    ),
    (
        "midgame",
        "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10",
    ),
];

/// Per-position bench result.
#[derive(Clone, Debug)]
pub struct BenchPositionResult {
    pub id: String,
    pub nodes: u64,
    pub best_move: Move,
    pub time: Duration,
}

/// Aggregate report from a full bench run.
#[derive(Clone, Debug)]
pub struct BenchReport {
    pub depth: i32,
    pub positions: Vec<BenchPositionResult>,
    pub total_nodes: u64,
    pub total_time: Duration,
}

/// Run fixed-depth searches on all [`BENCH_POSITIONS`].
///
/// Clears TT between positions for determinism. `hash_mb` is the TT size.
pub fn run_bench(depth: i32, hash_mb: usize) -> BenchReport {
    lookup::initialize();
    let depth = depth.clamp(1, 64);
    let stop = AtomicBool::new(false);
    let start = Instant::now();
    let mut positions = Vec::with_capacity(BENCH_POSITIONS.len());
    let mut total_nodes = 0u64;

    for &(id, fen) in BENCH_POSITIONS {
        let mut board = if fen.is_empty() {
            Board::startpos()
        } else {
            Board::from_fen(fen).expect("bench FEN must be valid")
        };
        let tt = TranspositionTable::new(hash_mb);
        let result = search::go(
            &mut board,
            Limits {
                depth: Some(depth),
                ..Default::default()
            },
            &tt,
            &stop,
            None,
        );
        total_nodes += result.nodes;
        positions.push(BenchPositionResult {
            id: id.to_string(),
            nodes: result.nodes,
            best_move: result.best_move,
            time: result.time,
        });
    }

    BenchReport {
        depth,
        positions,
        total_nodes,
        total_time: start.elapsed(),
    }
}

/// Format a [`BenchReport`] for UCI stdout.
pub fn format_bench_report(report: &BenchReport) -> String {
    let mut out = String::new();
    for p in &report.positions {
        out.push_str(&format!(
            "BenchPosition: {} nodes {} bestmove {}\n",
            p.id, p.nodes, p.best_move
        ));
    }
    let nps = if report.total_time.as_millis() > 0 {
        report.total_nodes * 1000 / report.total_time.as_millis() as u64
    } else {
        report.total_nodes
    };
    out.push_str(&format!(
        "BenchSummary: depth {} nodes {} time {} ms nps {}\n",
        report.depth,
        report.total_nodes,
        report.total_time.as_millis(),
        nps
    ));
    out
}

/// Locked node signature at [`BENCH_DEPTH`] with 16 MB TT (update when search changes).
///
/// Captured from a release `bench` run; used as a regression gate.
pub const BENCH_NODE_SIGNATURE: u64 = 115_107;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::selectivity;
    use std::sync::atomic::Ordering;

    /// Hold every selectivity A/B lock and force features on so parallel search
    /// tests cannot race the bench signature.
    fn with_stable_selectivity<R>(f: impl FnOnce() -> R) -> R {
        let _ab = selectivity::ab_test_guard();
        let _nmp = selectivity::NMP_TEST_LOCK.lock().unwrap();
        let _lmr = selectivity::LMR_TEST_LOCK.lock().unwrap();
        let _rfp = selectivity::RFP_TEST_LOCK.lock().unwrap();
        let _razor = selectivity::RAZORING_TEST_LOCK.lock().unwrap();
        let _lmp = selectivity::LMP_TEST_LOCK.lock().unwrap();
        let _fut = selectivity::FUTILITY_TEST_LOCK.lock().unwrap();
        let _hist = selectivity::HISTORY_PRUNE_TEST_LOCK.lock().unwrap();
        let _see = selectivity::SEE_PRUNE_TEST_LOCK.lock().unwrap();
        let _pc = selectivity::PROBCUT_TEST_LOCK.lock().unwrap();
        let _iir = selectivity::IIR_TEST_LOCK.lock().unwrap();
        let _se = selectivity::SINGULAR_TEST_LOCK.lock().unwrap();

        selectivity::NMP_ENABLED.store(true, Ordering::Relaxed);
        selectivity::LMR_ENABLED.store(true, Ordering::Relaxed);
        selectivity::RFP_ENABLED.store(true, Ordering::Relaxed);
        selectivity::RAZORING_ENABLED.store(true, Ordering::Relaxed);
        selectivity::LMP_ENABLED.store(true, Ordering::Relaxed);
        selectivity::FUTILITY_ENABLED.store(true, Ordering::Relaxed);
        selectivity::HISTORY_PRUNE_ENABLED.store(true, Ordering::Relaxed);
        selectivity::SEE_PRUNE_ENABLED.store(true, Ordering::Relaxed);
        selectivity::PROBCUT_ENABLED.store(true, Ordering::Relaxed);
        selectivity::IIR_ENABLED.store(true, Ordering::Relaxed);
        selectivity::SINGULAR_ENABLED.store(true, Ordering::Relaxed);

        f()
    }

    #[test]
    fn bench_runs_and_counts_nodes() {
        let report = with_stable_selectivity(|| run_bench(4, 1));
        assert_eq!(report.positions.len(), BENCH_POSITIONS.len());
        assert!(report.total_nodes > 0);
        for p in &report.positions {
            assert!(p.nodes > 0, "{} should search nodes", p.id);
            assert!(!p.best_move.is_none(), "{} should have a bestmove", p.id);
        }
    }

    #[test]
    fn bench_is_deterministic() {
        let (a, b) = with_stable_selectivity(|| {
            let a = run_bench(5, 4);
            let b = run_bench(5, 4);
            (a, b)
        });
        assert_eq!(a.total_nodes, b.total_nodes);
        for (pa, pb) in a.positions.iter().zip(b.positions.iter()) {
            assert_eq!(pa.nodes, pb.nodes);
            assert_eq!(pa.best_move, pb.best_move);
        }
    }

    #[test]
    fn bench_signature() {
        let report = with_stable_selectivity(|| run_bench(BENCH_DEPTH, 16));
        assert_eq!(
            report.total_nodes, BENCH_NODE_SIGNATURE,
            "bench node signature drift: got {} expected {} — update BENCH_NODE_SIGNATURE if intentional",
            report.total_nodes, BENCH_NODE_SIGNATURE
        );
    }
}
