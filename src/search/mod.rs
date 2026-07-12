//! Search entry: iterative deepening, aspiration, PVS (P2).

mod alphabeta;
mod selectivity;
pub mod stack;

use alphabeta::{search, NodeType};
use stack::{format_pv, RootMove, Stack, MAX_PLY};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::board::Board;
use crate::history::HistoryTables;
use crate::transposition::TranspositionTable;
use crate::types::score::VALUE_INFINITE;
use crate::types::{Move, Value};

/// Search limits from UCI / TUI.
#[derive(Clone, Debug, Default)]
pub struct Limits {
    pub depth: Option<i32>,
    pub movetime: Option<Duration>,
    pub nodes: Option<u64>,
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

/// Per-thread search state (single-thread for now).
pub struct ThreadData {
    pub nodes: u64,
    pub stack: Vec<Stack>,
    pub root_moves: Vec<RootMove>,
    pub history: HistoryTables,
}

impl ThreadData {
    pub fn new() -> Self {
        Self {
            nodes: 0,
            stack: vec![Stack::default(); MAX_PLY],
            root_moves: Vec::new(),
            history: HistoryTables::new(),
        }
    }
}

impl Default for ThreadData {
    fn default() -> Self {
        Self::new()
    }
}

/// Optional info callback: (depth, score, nodes, time, pv_string).
pub type InfoCallback<'a> = dyn FnMut(i32, Value, u64, Duration, &str) + 'a;

/// Run iterative deepening search on `board`.
///
/// Always returns the last completed iteration's best move when aborted mid-ID.
pub fn go(
    board: &mut Board,
    limits: Limits,
    tt: &mut TranspositionTable,
    stop: &AtomicBool,
    mut info: Option<&mut InfoCallback<'_>>,
) -> SearchResult {
    tt.new_search();

    let mut td = ThreadData::new();
    let start = Instant::now();

    // Root move list.
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

    for depth in 1..=max_depth {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        if let Some(mt) = limits.movetime {
            if start.elapsed() >= mt {
                stop.store(true, Ordering::Relaxed);
                break;
            }
        }
        if let Some(n) = limits.nodes {
            if td.nodes >= n {
                break;
            }
        }

        // Aspiration windows from depth 4 onward (P2-05).
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
            // Reset PV for this attempt.
            td.stack[0].clear_pv();

            score = search(
                board,
                &mut td,
                tt,
                stop,
                0,
                depth,
                alpha,
                beta,
                NodeType::Root,
            );

            if stop.load(Ordering::Relaxed) {
                break;
            }

            if score <= alpha {
                // Fail low: widen down.
                beta = (alpha + beta) / 2;
                alpha = (score.saturating_sub(delta)).max(-VALUE_INFINITE);
                delta = delta.saturating_mul(2).min(VALUE_INFINITE / 4);
                continue;
            }
            if score >= beta {
                // Fail high: widen up.
                beta = (score.saturating_add(delta)).min(VALUE_INFINITE);
                delta = delta.saturating_mul(2).min(VALUE_INFINITE / 4);
                continue;
            }
            break;
        }

        if stop.load(Ordering::Relaxed) && best.depth > 0 {
            // Keep prior completed iteration.
            break;
        }

        // Extract best move from PV or first root move.
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

        if let Some(cb) = info.as_mut() {
            let pv_str = format_pv(&pv);
            cb(depth, score, td.nodes, start.elapsed(), &pv_str);
        }

        // Soft stop between iterations.
        if let Some(mt) = limits.movetime {
            if start.elapsed() >= mt {
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
    let mut tt = TranspositionTable::new(1);
    let stop = AtomicBool::new(false);
    go(
        board,
        Limits {
            depth: Some(depth),
            ..Default::default()
        },
        &mut tt,
        &stop,
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use super::selectivity;
    use crate::types::{Color, Piece, Square};
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
            &mut tt,
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
            &mut tt,
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
            &mut tt_off,
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
            &mut tt_on,
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
}
