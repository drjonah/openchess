//! Scalar NNUE forward: ClippedReLU FT → L2 → L3 → cp (P6-06).

use super::accumulator::Accumulator;
use super::features::L1_SIZE;
use super::network::Network;
use crate::types::{Color, Value};

/// Clipped ReLU used as FT activation (Stockfish-family style, 0..127).
#[inline]
fn crelu(x: i16) -> i32 {
    (x as i32).clamp(0, 127)
}

/// Dense matmul: `out[j] = bias[j] + sum_i in[i] * w[j * in_dim + i]` with i8 weights.
fn affine_crelu(input: &[i32], weights: &[i8], bias: &[i32], out_dim: usize) -> Vec<i32> {
    let in_dim = input.len();
    debug_assert_eq!(weights.len(), out_dim * in_dim);
    debug_assert_eq!(bias.len(), out_dim);
    let mut out = vec![0i32; out_dim];
    for j in 0..out_dim {
        let mut sum = bias[j];
        let row = &weights[j * in_dim..(j + 1) * in_dim];
        for i in 0..in_dim {
            sum += input[i] * row[i] as i32;
        }
        // Keep activations in a sane int range before the next layer.
        out[j] = sum.clamp(0, 127);
    }
    out
}

/// Evaluate the dense head from a dual accumulator (STM-relative).
///
/// Returns a raw network score in centipawns (after `scale`), **before**
/// post-NNUE corrections (P6-07).
pub fn evaluate_raw(acc: &Accumulator, stm: Color, net: &Network) -> Value {
    assert_eq!(net.l1, L1_SIZE, "network L1 must match accumulator width");

    let stm_acc = acc.perspective(stm);
    let nstm_acc = acc.perspective(!stm);

    let mut input = vec![0i32; 2 * net.l1];
    for i in 0..net.l1 {
        input[i] = crelu(stm_acc[i]);
        input[net.l1 + i] = crelu(nstm_acc[i]);
    }

    let h1 = affine_crelu(&input, &net.fc1_w, &net.fc1_b, net.l2);
    let h2 = affine_crelu(&h1, &net.fc2_w, &net.fc2_b, net.l3);

    let mut sum = net.out_b;
    for i in 0..net.l3 {
        sum += h2[i] * net.out_w[i] as i32;
    }
    sum / net.scale
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::eval::nnue::{Network, NnueState};

    fn init() {
        crate::lookup::initialize();
    }

    #[test]
    fn startpos_raw_is_finite_and_stable() {
        init();
        let board = Board::startpos();
        let mut state = NnueState::default();
        state.refresh(&board);
        let net = Network::embedded_shared();
        let a = evaluate_raw(&state.accumulator, board.side_to_move(), &net);
        let b = evaluate_raw(&state.accumulator, board.side_to_move(), &net);
        assert_eq!(a, b);
        assert!(a.abs() < 50_000, "raw score blew up: {a}");
    }
}
