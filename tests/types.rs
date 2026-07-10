//! P1-01 — core types vocabulary.

mod common;

use common::sq;
use openchess::types::score::{mate_in, mated_in};
use openchess::types::zobrist;
use openchess::{Bitboard, CastlingRights, Color, Move, Piece, PieceType, Square};

#[test]
fn color_opposite() {
    assert_eq!(!Color::White, Color::Black);
    assert_eq!(!Color::Black, Color::White);
}

#[test]
fn piece_color_and_type() {
    let p = Piece::new(Color::White, PieceType::Knight);
    assert_eq!(p.color(), Some(Color::White));
    assert_eq!(p.piece_type(), Some(PieceType::Knight));
    assert_eq!(p.to_char(), 'N');
}

#[test]
fn square_round_trip_all() {
    for sq in Square::all() {
        assert_eq!(Square::new(sq.index()).unwrap(), sq);
        let via = Square::from_file_rank(sq.file(), sq.rank()).unwrap();
        assert_eq!(via, sq);
    }
}

#[test]
fn square_e4_and_indexing() {
    let e4 = sq("e4");
    assert_eq!(e4.file(), 4);
    assert_eq!(e4.rank(), 3);
    assert_eq!(Square::H1.index(), 7);
    assert_eq!(Square::A8.index(), 56);
}

#[test]
fn bitboard_set_clear_contains() {
    let mut bb = Bitboard::EMPTY;
    let e4 = sq("e4");
    bb.set(e4);
    assert!(bb.contains(e4));
    assert_eq!(bb.count(), 1);
    bb.clear(e4);
    assert!(bb.is_empty());
}

#[test]
fn bitboard_lsb_pop() {
    let mut bb = Bitboard::from_square(Square::A1) | Bitboard::from_square(Square::H8);
    assert_eq!(bb.lsb(), Some(Square::A1));
    assert_eq!(bb.pop_lsb(), Some(Square::A1));
    assert_eq!(bb.lsb(), Some(Square::H8));
}

#[test]
fn bitboard_square_round_trip() {
    for sq in Square::all() {
        assert!(Bitboard::from_square(sq).contains(sq));
        assert_eq!(Bitboard::from_square(sq).lsb(), Some(sq));
    }
}

#[test]
fn move_encode_decode() {
    let m = Move::new(sq("e2"), sq("e4"));
    assert_eq!(m.from(), sq("e2"));
    assert_eq!(m.to(), sq("e4"));
    assert!(!m.is_promotion());
    assert_eq!(m.to_string(), "e2e4");
}

#[test]
fn move_promotion_encode() {
    let m = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);
    assert!(m.is_promotion());
    assert_eq!(m.promotion_piece(), Some(PieceType::Queen));
    assert_eq!(m.to_string(), "a7a8q");
}

#[test]
fn castling_rights_ops() {
    let mut cr = CastlingRights::WHITE_KING | CastlingRights::BLACK_QUEEN;
    assert!(cr.contains(CastlingRights::WHITE_KING));
    cr.remove(CastlingRights::WHITE_KING);
    assert!(!cr.contains(CastlingRights::WHITE_KING));
    assert!(cr.contains(CastlingRights::BLACK_QUEEN));
}

#[test]
fn mate_scores_order() {
    assert!(mate_in(0) > mate_in(5));
    assert!(mated_in(0) < mated_in(5));
}

#[test]
fn zobrist_keys_are_stable_and_distinct() {
    zobrist::initialize();
    assert_ne!(
        zobrist::piece_key(Piece::WhitePawn, Square::A1),
        zobrist::piece_key(Piece::BlackPawn, Square::A1)
    );
    assert_ne!(
        zobrist::castling_key(CastlingRights::NONE),
        zobrist::castling_key(CastlingRights::ALL)
    );
    assert_ne!(zobrist::ep_key(0), zobrist::ep_key(7));
    assert_ne!(zobrist::side_key(), 0);
}
