//! Lazy SMP worker pool (P8-01).
//!
//! N workers search the same root independently with a shared TT. Histories,
//! killers, and NNUE accumulators are per-thread. Results are voted by depth.

use crate::board::Board;
use crate::eval::Network;
use crate::search::{self, Limits, SearchResult};
use crate::transposition::TranspositionTable;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

/// Shared node counter across Lazy SMP workers.
pub struct SharedSearchState {
    pub nodes: AtomicU64,
}

impl SharedSearchState {
    pub fn new() -> Self {
        Self {
            nodes: AtomicU64::new(0),
        }
    }
}

impl Default for SharedSearchState {
    fn default() -> Self {
        Self::new()
    }
}

/// Run `threads` independent ID searches; pick the deepest / strongest result.
///
/// Thread 0 runs on the calling thread (so UCI `info` callbacks work). Helpers
/// `threads - 1` run in the background with the same shared TT / stop flag.
pub fn go_lazy_smp(
    board: &Board,
    limits: Limits,
    tt: &TranspositionTable,
    stop: &AtomicBool,
    network: Arc<Network>,
    threads: u32,
    info: Option<&mut search::InfoCallback<'_>>,
) -> SearchResult {
    let threads = threads.max(1) as usize;
    if threads == 1 {
        return search::go_single(board.clone(), limits, tt, stop, network, info);
    }

    let shared = Arc::new(SharedSearchState::new());
    let results: Arc<Mutex<Vec<SearchResult>>> = Arc::new(Mutex::new(Vec::new()));

    // Extend lifetimes for helper threads joined before this function returns.
    let stop_ref: &'static AtomicBool = unsafe { &*(stop as *const AtomicBool) };
    let tt_ref: &'static TranspositionTable =
        unsafe { &*(tt as *const TranspositionTable) };

    let mut handles = Vec::with_capacity(threads - 1);
    for _tid in 1..threads {
        let board_c = board.clone();
        let limits_c = limits.clone();
        let net = Arc::clone(&network);
        let results_c = Arc::clone(&results);
        let shared_c = Arc::clone(&shared);

        handles.push(thread::spawn(move || {
            let result = search::go_single(board_c, limits_c, tt_ref, stop_ref, net, None);
            shared_c.nodes.fetch_add(result.nodes, Ordering::Relaxed);
            if let Ok(mut guard) = results_c.lock() {
                guard.push(result);
            }
        }));
    }

    let mut best = search::go_single(
        board.clone(),
        limits,
        tt,
        stop,
        network,
        info,
    );
    shared.nodes.fetch_add(best.nodes, Ordering::Relaxed);
    // Wind down helpers once the primary thread finishes an iteration loop.
    stop.store(true, Ordering::Relaxed);

    for h in handles {
        let _ = h.join();
    }

    if let Ok(guard) = results.lock() {
        for r in guard.iter() {
            if is_better_result(r, &best) {
                best = r.clone();
            }
        }
    }
    best.nodes = shared.nodes.load(Ordering::Relaxed);
    best
}

fn is_better_result(a: &SearchResult, b: &SearchResult) -> bool {
    if a.best_move.is_none() {
        return false;
    }
    if b.best_move.is_none() {
        return true;
    }
    if a.depth != b.depth {
        return a.depth > b.depth;
    }
    a.score.abs() > b.score.abs()
}
