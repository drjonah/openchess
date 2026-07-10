//! P1-03 — attack tables / magic bitboards.

mod common;

use common::{bb, init, sq};
use openchess::lookup::{
    attackers_to, attacks_bb, between, bishop_attacks, initialize, king_attacks, knight_attacks,
    line, pawn_attacks, queen_attacks, rook_attacks,
};
use openchess::{Bitboard, Color, PieceType, Square};

fn ray_attacks(sq: Square, occ: Bitboard, dirs: &[(i8, i8)]) -> Bitboard {
    let mut attacks = Bitboard::EMPTY;
    for &(df, dr) in dirs {
        let mut file = sq.file() as i8 + df;
        let mut rank = sq.rank() as i8 + dr;
        while (0..8).contains(&file) && (0..8).contains(&rank) {
            let target = Square::from_file_rank(file as u8, rank as u8).unwrap();
            attacks.set(target);
            if occ.contains(target) {
                break;
            }
            file += df;
            rank += dr;
        }
    }
    attacks
}

fn rook_attacks_slow(sq: Square, occ: Bitboard) -> Bitboard {
    ray_attacks(sq, occ, &[(0, 1), (0, -1), (1, 0), (-1, 0)])
}

fn bishop_attacks_slow(sq: Square, occ: Bitboard) -> Bitboard {
    ray_attacks(sq, occ, &[(1, 1), (-1, 1), (1, -1), (-1, -1)])
}

#[test]
fn knight_a1_attacks_two_squares() {
    init();
    let attacks = knight_attacks(Square::A1);
    assert_eq!(attacks.count(), 2);
    assert!(attacks.contains(sq("b3")));
    assert!(attacks.contains(sq("c2")));
}

#[test]
fn knight_e4_has_eight_attacks() {
    init();
    assert_eq!(knight_attacks(sq("e4")).count(), 8);
}

#[test]
fn king_e4_has_eight_attacks_a1_has_three() {
    init();
    assert_eq!(king_attacks(sq("e4")).count(), 8);
    assert_eq!(king_attacks(Square::A1).count(), 3);
}

#[test]
fn bishop_a1_empty_long_diagonal() {
    init();
    assert_eq!(
        bishop_attacks(Square::A1, Bitboard::EMPTY),
        bb(&["b2", "c3", "d4", "e5", "f6", "g7", "h8"])
    );
}

#[test]
fn rook_a1_empty_file_and_rank() {
    init();
    let attacks = rook_attacks(Square::A1, Bitboard::EMPTY);
    assert_eq!(attacks.count(), 14);
}

#[test]
fn rook_d4_blocked_on_d6() {
    init();
    let attacks = rook_attacks(sq("d4"), Bitboard::from_square(sq("d6")));
    assert!(attacks.contains(sq("d5")));
    assert!(attacks.contains(sq("d6")));
    assert!(!attacks.contains(sq("d7")));
}

#[test]
fn rook_edge_file_masks_see_interior_blockers() {
    init();
    let attacks_h = rook_attacks(Square::H1, Bitboard::from_square(sq("h4")));
    assert!(attacks_h.contains(sq("h4")));
    assert!(!attacks_h.contains(sq("h5")));

    let attacks_a8 = rook_attacks(Square::A8, Bitboard::from_square(sq("a5")));
    assert!(attacks_a8.contains(sq("a5")));
    assert!(!attacks_a8.contains(sq("a4")));
}

#[test]
fn magic_attacks_match_slow_reference_on_edge_files() {
    init();
    let cases = [
        (Square::A1, Bitboard::from_square(sq("a3"))),
        (Square::A1, Bitboard::from_square(sq("a7"))),
        (Square::H1, Bitboard::from_square(sq("h2"))),
        (Square::H8, Bitboard::from_square(sq("h4"))),
        (Square::A8, Bitboard::from_square(sq("a2"))),
        (
            Square::A1,
            Bitboard::from_square(sq("a3")) | Bitboard::from_square(sq("c1")),
        ),
    ];
    for (sq, occ) in cases {
        assert_eq!(rook_attacks(sq, occ), rook_attacks_slow(sq, occ));
        assert_eq!(bishop_attacks(sq, occ), bishop_attacks_slow(sq, occ));
    }
}

#[test]
fn white_pawn_e4_attacks_d5_f5() {
    init();
    assert_eq!(pawn_attacks(Color::White, sq("e4")), bb(&["d5", "f5"]));
}

#[test]
fn queen_is_bishop_or_rook() {
    init();
    let d4 = sq("d4");
    let occ = Bitboard::from_square(sq("d6")) | Bitboard::from_square(sq("f4"));
    assert_eq!(
        queen_attacks(d4, occ),
        bishop_attacks(d4, occ) | rook_attacks(d4, occ)
    );
}

#[test]
fn attacks_bb_matches_helpers() {
    init();
    let d4 = sq("d4");
    let occ = Bitboard::from_square(sq("d6"));
    assert_eq!(attacks_bb(PieceType::Knight, d4, occ), knight_attacks(d4));
    assert_eq!(attacks_bb(PieceType::Rook, d4, occ), rook_attacks(d4, occ));
}

#[test]
fn attackers_to_finds_slider_and_leaper() {
    init();
    let target = sq("e4");
    let occ = Bitboard::from_square(target)
        | Bitboard::from_square(sq("d5"))
        | Bitboard::from_square(sq("f6"));
    let attackers = attackers_to(
        target,
        occ,
        Bitboard::from_square(sq("f6")),
        Bitboard::from_square(sq("d5")),
        Bitboard::EMPTY,
        Bitboard::EMPTY,
        Bitboard::EMPTY,
        Bitboard::EMPTY,
        Color::White,
    );
    assert!(attackers.contains(sq("d5")));
    assert!(attackers.contains(sq("f6")));
}

#[test]
fn between_and_line_on_diagonal() {
    init();
    assert_eq!(
        line(Square::A1, Square::H8),
        bb(&["a1", "b2", "c3", "d4", "e5", "f6", "g7", "h8"])
    );
    assert_eq!(
        between(Square::A1, Square::H8),
        bb(&["b2", "c3", "d4", "e5", "f6", "g7"])
    );
}

#[test]
fn initialize_is_idempotent() {
    initialize();
    initialize();
    assert_eq!(knight_attacks(sq("e4")).count(), 8);
}
