//! Perft — recursive legal-move node counter (P1 correctness gate).

use crate::board::Board;

/// Count leaf nodes at `depth` by expanding all legal moves.
///
/// `depth == 0` returns 1 (the current position is a leaf).
pub fn perft(board: &mut Board, depth: u32) -> u64 {
    if depth == 0 {
        return 1;
    }

    let moves = board.legal_moves();
    if depth == 1 {
        return moves.len() as u64;
    }

    let mut nodes = 0u64;
    for m in moves {
        board.make(m);
        nodes += perft(board, depth - 1);
        board.unmake(m);
    }
    nodes
}

/// Divide perft: for each root legal move, report child node count at `depth - 1`.
pub fn perft_divide(board: &mut Board, depth: u32) -> Vec<(crate::types::Move, u64)> {
    let moves = board.legal_moves();
    let mut out = Vec::with_capacity(moves.len());
    for m in moves {
        board.make(m);
        let nodes = if depth <= 1 {
            1
        } else {
            perft(board, depth - 1)
        };
        board.unmake(m);
        out.push((m, nodes));
    }
    out
}
