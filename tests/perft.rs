//! P1-10 — perft suite (hard gate).

mod common;

use common::{
    init, KIWIPETE_FEN, POS3_FEN, POS4_FEN, POS5_FEN, POS6_FEN, START_FEN,
};
use openchess::board::Board;
use openchess::tools::{perft, perft_divide};

#[test]
fn startpos_perft_shallow() {
    init();
    let mut board = Board::startpos();
    assert_eq!(perft(&mut board, 0), 1);
    assert_eq!(perft(&mut board, 1), 20);
    assert_eq!(perft(&mut board, 2), 400);
    assert_eq!(perft(&mut board, 3), 8_902);
    assert_eq!(perft(&mut board, 4), 197_281);
}

#[test]
fn startpos_perft_5() {
    init();
    let mut board = Board::startpos();
    assert_eq!(perft(&mut board, 5), 4_865_609);
}

/// Hard gate: startpos perft(6). Slow in debug — run with `--release`.
#[test]
#[ignore]
fn startpos_perft_6_gate() {
    init();
    let mut board = Board::startpos();
    assert_eq!(perft(&mut board, 6), 119_060_324);
}

#[test]
fn kiwipete_perft_shallow() {
    init();
    let mut board = Board::from_fen(KIWIPETE_FEN).unwrap();
    assert_eq!(perft(&mut board, 1), 48);
    assert_eq!(perft(&mut board, 2), 2_039);
    assert_eq!(perft(&mut board, 3), 97_862);
}

#[test]
fn kiwipete_perft_4() {
    init();
    let mut board = Board::from_fen(KIWIPETE_FEN).unwrap();
    assert_eq!(perft(&mut board, 4), 4_085_603);
}

/// Hard gate: Kiwipete perft(5). Slow in debug — run with `--release`.
#[test]
#[ignore]
fn kiwipete_perft_5_gate() {
    init();
    let mut board = Board::from_fen(KIWIPETE_FEN).unwrap();
    assert_eq!(perft(&mut board, 5), 193_690_690);
}

#[test]
fn position3_perft() {
    init();
    let mut board = Board::from_fen(POS3_FEN).unwrap();
    assert_eq!(perft(&mut board, 1), 14);
    assert_eq!(perft(&mut board, 2), 191);
    assert_eq!(perft(&mut board, 3), 2_812);
    assert_eq!(perft(&mut board, 4), 43_238);
    assert_eq!(perft(&mut board, 5), 674_624);
}

#[test]
fn position4_perft() {
    init();
    let mut board = Board::from_fen(POS4_FEN).unwrap();
    assert_eq!(perft(&mut board, 1), 6);
    assert_eq!(perft(&mut board, 2), 264);
    assert_eq!(perft(&mut board, 3), 9_467);
    assert_eq!(perft(&mut board, 4), 422_333);
}

#[test]
fn position5_perft() {
    init();
    let mut board = Board::from_fen(POS5_FEN).unwrap();
    assert_eq!(perft(&mut board, 1), 44);
    assert_eq!(perft(&mut board, 2), 1_486);
    assert_eq!(perft(&mut board, 3), 62_379);
    assert_eq!(perft(&mut board, 4), 2_103_487);
}

#[test]
fn position6_perft() {
    init();
    let mut board = Board::from_fen(POS6_FEN).unwrap();
    assert_eq!(perft(&mut board, 1), 46);
    assert_eq!(perft(&mut board, 2), 2_079);
    assert_eq!(perft(&mut board, 3), 89_890);
    assert_eq!(perft(&mut board, 4), 3_894_594);
}

#[test]
fn startpos_fen_matches_hardcoded_perft4() {
    init();
    let mut a = Board::startpos();
    let mut b = Board::from_fen(START_FEN).unwrap();
    assert_eq!(perft(&mut a, 4), perft(&mut b, 4));
}

#[test]
fn divide_sums_to_perft() {
    init();
    let mut board = Board::startpos();
    let div = perft_divide(&mut board, 3);
    let sum: u64 = div.iter().map(|(_, n)| n).sum();
    assert_eq!(sum, 8_902);
}
