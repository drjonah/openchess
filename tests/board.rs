//! P1-02 / P1-07 — dual representation, checkers, and pins.

mod common;

use common::sq;
use openchess::board::Board;
use openchess::{Bitboard, Color, Move, Piece, PieceType, Square};

#[test]
fn put_piece_updates_mailbox_and_bitboards() {
    let mut board = Board::empty();
    let e4 = sq("e4");
    board.put_piece(Piece::WhiteKnight, e4);

    assert_eq!(board.piece_on(e4), Piece::WhiteKnight);
    assert!(board.pieces(PieceType::Knight).contains(e4));
    assert!(board.pieces_color(Color::White).contains(e4));
    assert!(board.occupancy().contains(e4));
}

#[test]
fn remove_piece_clears_both_views() {
    let mut board = Board::empty();
    let d5 = sq("d5");
    board.put_piece(Piece::BlackBishop, d5);

    let removed = board.remove_piece(d5);
    assert_eq!(removed, Piece::BlackBishop);
    assert_eq!(board.piece_on(d5), Piece::Empty);
    assert!(!board.pieces(PieceType::Bishop).contains(d5));
    assert!(!board.pieces_color(Color::Black).contains(d5));
    assert!(!board.occupancy().contains(d5));
}

#[test]
fn put_piece_replaces_existing() {
    let mut board = Board::empty();
    let c3 = sq("c3");
    board.put_piece(Piece::WhitePawn, c3);
    board.put_piece(Piece::BlackQueen, c3);

    assert_eq!(board.piece_on(c3), Piece::BlackQueen);
    assert!(!board.pieces(PieceType::Pawn).contains(c3));
    assert!(!board.pieces_color(Color::White).contains(c3));
    assert!(board.pieces(PieceType::Queen).contains(c3));
    assert_eq!(board.occupancy().count(), 1);
}

#[test]
fn startpos_metadata_and_kings() {
    let board = Board::startpos();
    assert_eq!(board.occupancy().count(), 32);
    assert_eq!(board.side_to_move(), Color::White);
    assert_eq!(board.ep_square(), None);
    assert_eq!(board.halfmove_clock(), 0);
    assert_eq!(board.fullmove_number(), 1);
    assert_eq!(board.king_sq(Color::White), Square::E1);
    assert_eq!(board.king_sq(Color::Black), Square::E8);
    assert!(board.checkers().is_empty());
    assert!(board.pinned().is_empty());
}

#[test]
fn mailbox_bitboard_consistency() {
    let board = Board::startpos();
    for sq in Square::all() {
        let piece = board.piece_on(sq);
        if piece.is_empty() {
            assert!(!board.occupancy().contains(sq));
        } else {
            let pt = piece.piece_type().unwrap();
            let color = piece.color().unwrap();
            assert!(board.occupancy().contains(sq));
            assert!(board.pieces(pt).contains(sq));
            assert!(board.pieces_color(color).contains(sq));
        }
    }
}

#[test]
fn refresh_finds_checker_on_open_e_file() {
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.refresh_checkers_and_pins();

    assert_eq!(board.checkers(), Bitboard::from_square(Square::E8));
    assert!(board.pinned().is_empty());
}

#[test]
fn refresh_finds_pinned_knight_on_e_file() {
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteKnight, sq("e2"));
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.refresh_checkers_and_pins();

    assert!(board.checkers().is_empty());
    assert_eq!(board.pinned(), Bitboard::from_square(sq("e2")));
    assert_eq!(board.pinners(), Bitboard::from_square(Square::E8));
}

#[test]
fn refresh_ignores_enemy_piece_blocking_its_own_slider() {
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::BlackPawn, sq("e5"));
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.refresh_checkers_and_pins();

    assert!(board.checkers().is_empty());
    assert!(board.pinned().is_empty());
}

#[test]
fn refresh_handles_double_check() {
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackBishop, sq("a5"));
    board.put_piece(Piece::BlackKing, Square::H8);
    board.set_side_to_move(Color::White);
    board.refresh_checkers_and_pins();

    assert_eq!(board.checkers().count(), 2);
    assert!(board.checkers().contains(Square::E8));
    assert!(board.checkers().contains(sq("a5")));
}

#[test]
fn make_refreshes_checkers_for_side_now_to_move() {
    common::init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteQueen, sq("h5"));
    board.put_piece(Piece::BlackKing, Square::E8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();
    assert!(board.checkers().is_empty());

    let m = Move::new(sq("h5"), sq("e5"));
    board.make(m);
    assert_eq!(board.side_to_move(), Color::Black);
    assert_eq!(board.checkers(), Bitboard::from_square(sq("e5")));

    board.unmake(m);
    assert!(board.checkers().is_empty());
}
