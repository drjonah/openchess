//! Alpha-beta / PVS search and quiescence (P2-01, P2-03, P2-06).

use super::selectivity;
use super::stack::{Stack, MAX_PLY};
use super::ThreadData;
use crate::board::Board;
use crate::eval;
use crate::history::{capture_key, history_bonus, ContSlot, CONT_PLIES};
use crate::movepick::{is_quiet, HistoryContext, MovePicker};
use crate::transposition::{Bound, TranspositionTable};
use crate::types::score::{
    mated_in, mate_in, value_from_tt, value_to_tt, VALUE_DRAW, VALUE_INFINITE, VALUE_MATE,
};
use crate::types::{Move, Value};
use std::sync::atomic::{AtomicBool, Ordering};

/// Node type specialization for PVS.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NodeType {
    Root,
    Pv,
    NonPv,
}

const MAX_QUIETS_TRACKED: usize = 64;
const MAX_CAPTURES_TRACKED: usize = 32;

/// Fail-soft alpha-beta / PVS search.
pub fn search(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &mut TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    depth: i32,
    mut alpha: Value,
    beta: Value,
    node: NodeType,
) -> Value {
    let is_root = node == NodeType::Root;
    let is_pv = node != NodeType::NonPv;
    let in_check = board.in_check();

    td.nodes += 1;

    // Abort check (skip at root so we always finish a move if possible).
    if !is_root && (stop.load(Ordering::Relaxed) || ply >= MAX_PLY - 1) {
        return eval::evaluate(board);
    }

    // Mate distance pruning.
    alpha = alpha.max(mated_in(ply as i32));
    let beta = beta.min(mate_in(ply as i32 + 1));
    if alpha >= beta {
        return alpha;
    }

    // Drop into quiescence at leaves.
    if depth <= 0 {
        return qsearch(board, td, tt, stop, ply, alpha, beta, 0);
    }

    // Ensure stack slot exists.
    while td.stack.len() <= ply + 1 {
        td.stack.push(Stack::default());
    }
    td.stack[ply].clear_pv();
    td.stack[ply].move_count = 0;
    // Null-move children inherit a sentinel continuation context.
    if ply > 0 && td.stack[ply - 1].current_move.is_none() {
        td.stack[ply].cont_slot = ContSlot::NONE;
    }

    let key = board.key();

    // TT probe.
    let mut tt_move = Move::NONE;
    let mut tt_hit = false;
    if let Some(entry) = tt.probe(key) {
        tt_hit = true;
        tt_move = entry.mv;
        if !is_pv && entry.depth as i32 >= depth {
            let score = value_from_tt(entry.score, ply as i32);
            match entry.bound {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }
    }

    // Static eval (skip when in check).
    let static_eval = if in_check {
        -VALUE_INFINITE
    } else {
        eval::evaluate(board)
    };
    td.stack[ply].static_eval = static_eval;

    let prev_eval = if ply >= 2 {
        td.stack[ply - 2].static_eval
    } else {
        static_eval
    };
    let improving = ply >= 2 && selectivity::is_improving(static_eval, prev_eval, in_check);

    // P5 forward pruning: NMP (live) + static hooks (RFP later).
    if node == NodeType::NonPv && !in_check {
        if let Some(score) = selectivity::try_null_move(
            board,
            td,
            tt,
            stop,
            ply,
            depth,
            beta,
            static_eval,
            improving,
        ) {
            return score;
        }
        if let Some(score) =
            selectivity::forward_prune(board, depth, alpha, beta, static_eval, improving)
        {
            return score;
        }
    }

    let mut best_score = -VALUE_INFINITE;
    let mut best_move = Move::NONE;
    let mut move_count = 0i32;
    let old_alpha = alpha;
    let stm = board.side_to_move();

    let killers = td.stack[ply].killers;
    let mut picker = {
        let mut stack_cont = [ContSlot::NONE; MAX_PLY];
        for (i, s) in td.stack.iter().enumerate().take(MAX_PLY) {
            stack_cont[i] = s.cont_slot;
        }
        let hctx = HistoryContext::new(&td.history, &killers, &stack_cont, ply, stm);
        MovePicker::new(
            board,
            if tt_move.is_none() {
                None
            } else {
                Some(tt_move)
            },
            &hctx,
        )
    };

    let mut quiets_searched = [Move::NONE; MAX_QUIETS_TRACKED];
    let mut quiet_count = 0usize;
    let mut captures_searched = [Move::NONE; MAX_CAPTURES_TRACKED];
    let mut capture_count = 0usize;

    while let Some(mv) = picker.next() {
        // Root hard-abort poll: stop between root moves once the hard bound is hit.
        if is_root {
            if let Some(hard) = td.hard_limit {
                if td.start.elapsed() >= hard {
                    stop.store(true, Ordering::Relaxed);
                }
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
        }

        move_count += 1;
        td.stack[ply].move_count = move_count;
        td.stack[ply].current_move = mv;

        let quiet = is_quiet(board, mv);

        if selectivity::should_prune_move(move_count, depth, quiet) {
            continue;
        }

        let moving_piece = board.piece_on(mv.from());
        board.make(mv);
        td.stack[ply + 1].cont_slot = ContSlot::new(moving_piece, mv.to());
        tt.prefetch(board.key());

        let mut score: Value;
        let gives_check = board.in_check();
        let new_depth = depth - 1 + if gives_check { 0 } else { 0 }; // extension hook later

        let reduction = if move_count > 1 && depth >= 3 {
            selectivity::late_move_reduction(
                depth,
                move_count,
                quiet,
                improving,
                in_check,
                gives_check,
            )
        } else {
            0
        };

        if move_count == 1 {
            // First move: full window.
            let child = if is_pv { NodeType::Pv } else { NodeType::NonPv };
            score = -search(
                board,
                td,
                tt,
                stop,
                ply + 1,
                new_depth,
                -beta,
                -alpha,
                child,
            );
        } else {
            // PVS: null-window scout, then re-search on fail-high.
            let reduced = (new_depth - reduction).max(0);
            score = -search(
                board,
                td,
                tt,
                stop,
                ply + 1,
                reduced,
                -(alpha + 1),
                -alpha,
                NodeType::NonPv,
            );
            if score > alpha && (reduction > 0 || is_pv) {
                let child = if is_pv { NodeType::Pv } else { NodeType::NonPv };
                score = -search(
                    board,
                    td,
                    tt,
                    stop,
                    ply + 1,
                    new_depth,
                    -beta,
                    -alpha,
                    child,
                );
            }
        }

        board.unmake(mv);

        if stop.load(Ordering::Relaxed) {
            if is_root {
                break;
            }
            return best_score.max(alpha);
        }

        if score > best_score {
            best_score = score;
            best_move = mv;
            if is_pv {
                // Child PV is at ply+1.
                let child_pv = td.stack[ply + 1].pv.clone();
                td.stack[ply].pv.clear();
                td.stack[ply].pv.push(mv);
                td.stack[ply].pv.extend(child_pv);
            }
            if score > alpha {
                alpha = score;
                if alpha >= beta {
                    if quiet {
                        update_quiet_stats(
                            td,
                            board,
                            ply,
                            stm,
                            mv,
                            depth,
                            &quiets_searched[..quiet_count],
                        );
                    } else {
                        update_capture_stats(
                            td,
                            board,
                            mv,
                            depth,
                            &captures_searched[..capture_count],
                        );
                    }
                    break;
                }
            }
        }

        if quiet {
            if quiet_count < MAX_QUIETS_TRACKED {
                quiets_searched[quiet_count] = mv;
                quiet_count += 1;
            }
        } else if capture_count < MAX_CAPTURES_TRACKED {
            captures_searched[capture_count] = mv;
            capture_count += 1;
        }
    }

    // Terminal node: checkmate or stalemate.
    if move_count == 0 {
        return if in_check {
            mated_in(ply as i32)
        } else {
            VALUE_DRAW
        };
    }

    // TT store.
    let bound = if best_score >= beta {
        Bound::Lower
    } else if best_score > old_alpha {
        Bound::Exact
    } else {
        Bound::Upper
    };
    let _ = tt_hit; // reserved for IIR (P5)
    tt.store(
        key,
        best_move,
        value_to_tt(best_score, ply as i32),
        depth as i16,
        bound,
    );

    best_score
}

/// Update killers + butterfly + continuation history on a quiet beta cutoff.
fn update_quiet_stats(
    td: &mut ThreadData,
    board: &Board,
    ply: usize,
    stm: crate::types::Color,
    mv: Move,
    depth: i32,
    previous_quiets: &[Move],
) {
    let killers = &mut td.stack[ply].killers;
    if killers[0] != mv {
        killers[1] = killers[0];
        killers[0] = mv;
    }

    let bonus = history_bonus(depth);
    let cont = cont_slots_for_ply(&td.stack, ply);
    let piece = board.piece_on(mv.from());

    td.history.update(stm, mv, bonus);
    td.history.update_continuation(&cont, piece, mv.to(), bonus);
    for &q in previous_quiets {
        if q != mv {
            let qp = board.piece_on(q.from());
            td.history.update(stm, q, -bonus);
            td.history.update_continuation(&cont, qp, q.to(), -bonus);
        }
    }
}

/// Update capture history on a noisy beta cutoff.
fn update_capture_stats(
    td: &mut ThreadData,
    board: &Board,
    mv: Move,
    depth: i32,
    previous_captures: &[Move],
) {
    let bonus = history_bonus(depth);
    if let Some((piece, to, captured)) = capture_key(board, mv) {
        td.history.capture_update(piece, to, captured, bonus);
    }
    for &c in previous_captures {
        if c != mv {
            if let Some((piece, to, captured)) = capture_key(board, c) {
                td.history.capture_update(piece, to, captured, -bonus);
            }
        }
    }
}

/// Continuation slots at offsets 1/2/4/6 for the current ply.
#[inline]
fn cont_slots_for_ply(stack: &[Stack], ply: usize) -> [ContSlot; 4] {
    let mut out = [ContSlot::NONE; 4];
    for (i, &d) in CONT_PLIES.iter().enumerate() {
        if ply + 1 >= d {
            let idx = ply + 1 - d;
            if idx < stack.len() {
                out[i] = stack[idx].cont_slot;
            }
        }
    }
    out
}

/// Quiescence search: stand-pat + captures (P2-03).
///
/// `checks` counts consecutive check-evasions already searched (capped).
pub fn qsearch(
    board: &mut Board,
    td: &mut ThreadData,
    tt: &mut TranspositionTable,
    stop: &AtomicBool,
    ply: usize,
    mut alpha: Value,
    beta: Value,
    checks: u8,
) -> Value {
    td.nodes += 1;

    const MAX_QS_CHECKS: u8 = 2;
    if stop.load(Ordering::Relaxed) || ply >= MAX_PLY - 1 {
        return eval::evaluate(board);
    }

    while td.stack.len() <= ply + 1 {
        td.stack.push(Stack::default());
    }
    td.stack[ply].clear_pv();

    let in_check = board.in_check();
    let key = board.key();
    let old_alpha = alpha;

    // TT probe (NonPV-style cutoffs; depth >= 0 entries are usable in QS).
    let mut tt_move = Move::NONE;
    if let Some(entry) = tt.probe(key) {
        tt_move = entry.mv;
        if entry.depth >= 0 {
            let score = value_from_tt(entry.score, ply as i32);
            match entry.bound {
                Bound::Exact => return score,
                Bound::Lower if score >= beta => return score,
                Bound::Upper if score <= alpha => return score,
                _ => {}
            }
        }
    }

    let mut best_score = if in_check {
        -VALUE_INFINITE
    } else {
        let stand_pat = eval::evaluate(board);
        if stand_pat >= beta {
            tt.store(
                key,
                Move::NONE,
                value_to_tt(stand_pat, ply as i32),
                0,
                Bound::Lower,
            );
            return stand_pat;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }
        stand_pat
    };

    if in_check && checks >= MAX_QS_CHECKS {
        return eval::evaluate(board);
    }

    const DELTA_MARGIN: Value = 900;

    let tt_move_opt = if tt_move.is_none() {
        None
    } else {
        Some(tt_move)
    };
    let killers = td.stack[ply].killers;
    let mut picker = if in_check {
        let mut stack_cont = [ContSlot::NONE; MAX_PLY];
        for (i, s) in td.stack.iter().enumerate().take(MAX_PLY) {
            stack_cont[i] = s.cont_slot;
        }
        let hctx =
            HistoryContext::new(&td.history, &killers, &stack_cont, ply, board.side_to_move());
        MovePicker::evasion(board, tt_move_opt, &hctx)
    } else {
        MovePicker::qsearch(board, tt_move_opt)
    };

    let mut move_count = 0i32;
    let mut best_move = Move::NONE;
    while let Some(mv) = picker.next() {
        move_count += 1;

        if !in_check && board.see(mv) < 0 {
            continue;
        }

        if !in_check {
            let capture_value = capture_value(board, mv);
            if best_score + capture_value + DELTA_MARGIN < alpha {
                continue;
            }
        }

        board.make(mv);
        tt.prefetch(board.key());
        let next_checks = if in_check { checks + 1 } else { 0 };
        let score = -qsearch(board, td, tt, stop, ply + 1, -beta, -alpha, next_checks);
        board.unmake(mv);

        if stop.load(Ordering::Relaxed) {
            return best_score;
        }

        if score > best_score {
            best_score = score;
            best_move = mv;
            if score > alpha {
                alpha = score;
                td.stack[ply].pv.clear();
                td.stack[ply].pv.push(mv);
                let child = td.stack[ply + 1].pv.clone();
                td.stack[ply].pv.extend(child);
                if alpha >= beta {
                    break;
                }
            }
        }
    }

    if in_check && move_count == 0 {
        return mated_in(ply as i32);
    }

    let bound = if best_score >= beta {
        Bound::Lower
    } else if best_score > old_alpha {
        Bound::Exact
    } else {
        Bound::Upper
    };
    tt.store(
        key,
        best_move,
        value_to_tt(best_score, ply as i32),
        0,
        bound,
    );

    best_score
}

fn capture_value(board: &Board, mv: Move) -> Value {
    use crate::types::score::piece_value;
    use crate::types::PieceType;
    if mv.is_en_passant() {
        return piece_value(PieceType::Pawn);
    }
    let mut v = board
        .piece_on(mv.to())
        .piece_type()
        .map(piece_value)
        .unwrap_or(0);
    if let Some(promo) = mv.promotion_piece() {
        v += piece_value(promo) - piece_value(PieceType::Pawn);
    }
    v
}

/// Suppress unused import warning for VALUE_MATE in mate helpers path.
#[allow(dead_code)]
fn _mate_range() -> Value {
    VALUE_MATE
}
