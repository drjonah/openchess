//! P1-05 / P1-07 — legal move generation and evasion hand cases.

mod common;

use common::{init, sq};
use openchess::board::Board;
use openchess::{CastlingRights, Color, Move, Piece, PieceType, Square};

#[test]
fn startpos_has_twenty_legal_moves() {
    init();
    assert_eq!(Board::startpos().legal_moves().len(), 20);
}

#[test]
fn after_e4_black_has_twenty_legal_moves() {
    init();
    let mut board = Board::startpos();
    board.make(Move::new(sq("e2"), sq("e4")));
    assert_eq!(board.legal_moves().len(), 20);
}

#[test]
fn after_e4_e5_nf3_black_has_sensible_moves() {
    init();
    let mut board = Board::startpos();
    board.make(Move::new(sq("e2"), sq("e4")));
    board.make(Move::new(sq("e7"), sq("e5")));
    board.make(Move::new(sq("g1"), sq("f3")));

    let moves = board.legal_moves();
    assert!(moves.contains(&Move::new(sq("b8"), sq("c6"))));
    assert!(moves.contains(&Move::new(sq("g8"), sq("f6"))));
    assert!(moves.contains(&Move::new(sq("d7"), sq("d6"))));
}

#[test]
fn king_in_check_only_generates_evasions() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();

    assert!(board.in_check());
    let moves = board.legal_moves();
    assert!(!moves.is_empty());
    for m in &moves {
        assert_eq!(m.from(), Square::E1);
        assert_ne!(m.to().file(), Square::E1.file());
    }
}

#[test]
fn castling_available_when_legal() {
    init();
    let board = Board::from_fen("r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1").unwrap();
    let moves = board.legal_moves();
    assert!(moves.contains(&Move::castling(Square::E1, Square::G1)));
    assert!(moves.contains(&Move::castling(Square::E1, Square::C1)));
}

#[test]
fn castling_blocked_when_king_passes_through_attacked_square() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteRook, Square::H1);
    board.put_piece(Piece::BlackKing, Square::E8);
    board.put_piece(Piece::BlackRook, Square::F8);
    board.set_side_to_move(Color::White);
    board.set_castling_rights(CastlingRights::ALL);
    board.rehash();
    board.refresh_checkers_and_pins();

    let moves = board.legal_moves();
    assert!(!moves.contains(&Move::castling(Square::E1, Square::G1)));
}

#[test]
fn captures_and_quiets_partition_legal_moves() {
    init();
    let board = Board::startpos();
    let mut captures = Vec::new();
    let mut quiets = Vec::new();
    board.generate_captures(&mut captures);
    board.generate_quiets(&mut quiets);
    assert!(captures.is_empty());
    assert_eq!(quiets.len(), 20);
}

#[test]
fn en_passant_capture_is_generated() {
    init();
    let board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert!(board
        .legal_moves()
        .contains(&Move::en_passant(sq("e5"), sq("d6"))));
}

#[test]
fn promotion_generates_all_four_pieces() {
    init();
    let board = Board::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let moves = board.legal_moves();
    for pt in [
        PieceType::Queen,
        PieceType::Rook,
        PieceType::Bishop,
        PieceType::Knight,
    ] {
        assert!(moves.contains(&Move::promotion(sq("a7"), sq("a8"), pt)));
    }
}

#[test]
fn pinned_knight_has_no_legal_moves() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteKnight, sq("e2"));
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();

    assert!(board.pinned().contains(sq("e2")));
    assert!(!board.legal_moves().iter().any(|m| m.from() == sq("e2")));
}

#[test]
fn pinned_rook_may_move_along_pin_ray() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteRook, sq("e4"));
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackKing, Square::A8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();

    let from_e4: Vec<_> = board
        .legal_moves()
        .into_iter()
        .filter(|m| m.from() == sq("e4"))
        .collect();
    assert!(!from_e4.is_empty());
    for m in &from_e4 {
        assert_eq!(m.to().file(), Square::E1.file());
    }
    assert!(from_e4.iter().any(|m| m.to() == Square::E8));
}

#[test]
fn single_check_can_be_resolved_by_capturing_checker() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteQueen, Square::D1);
    board.put_piece(Piece::BlackKnight, sq("f3"));
    board.put_piece(Piece::BlackKing, Square::E8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();

    assert!(board.checkers().contains(sq("f3")));
    assert!(board
        .legal_moves()
        .contains(&Move::new(Square::D1, sq("f3"))));
}

#[test]
fn double_check_only_king_moves() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteKing, Square::E1);
    board.put_piece(Piece::WhiteQueen, Square::D1);
    board.put_piece(Piece::BlackRook, Square::E8);
    board.put_piece(Piece::BlackBishop, sq("a5"));
    board.put_piece(Piece::BlackKing, Square::H8);
    board.set_side_to_move(Color::White);
    board.rehash();
    board.refresh_checkers_and_pins();

    assert_eq!(board.checkers().count(), 2);
    let moves = board.legal_moves();
    assert!(!moves.is_empty());
    for m in &moves {
        assert_eq!(m.from(), Square::E1);
    }
}

#[test]
fn no_legal_move_leaves_own_king_in_check() {
    init();
    let mut board = Board::startpos();
    for _ in 0..4 {
        let moves = board.legal_moves();
        if moves.is_empty() {
            break;
        }
        let m = moves[0];
        board.make(m);
        let mover = !board.side_to_move();
        assert!(!board.is_square_attacked(board.king_sq(mover), board.side_to_move()));
    }
}
