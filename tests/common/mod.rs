//! Shared helpers for P1 integration tests.

#![allow(dead_code)]

use openchess::lookup;
use openchess::{Bitboard, Move, Square};
use std::str::FromStr;

pub const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
pub const KIWIPETE_FEN: &str =
    "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1";
pub const POS3_FEN: &str = "8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1";
pub const POS4_FEN: &str = "r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1";
pub const POS5_FEN: &str = "rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8";
pub const POS6_FEN: &str =
    "r4rk1/1pp1qppp/p1np1n2/2b1p1B1/2B1P1b1/P1NP1N2/1PP1QPPP/R4RK1 w - - 0 10";

pub fn init() {
    lookup::initialize();
}

pub fn sq(name: &str) -> Square {
    Square::from_str(name).unwrap()
}

pub fn mv(from: &str, to: &str) -> Move {
    Move::new(sq(from), sq(to))
}

pub fn bb(names: &[&str]) -> Bitboard {
    names
        .iter()
        .fold(Bitboard::EMPTY, |acc, name| acc.with(sq(name)))
}

/// Tiny deterministic LCG so we don't pull in `rand`.
pub fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1);
    *state
}
