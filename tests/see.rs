//! P1-08 — Static Exchange Evaluation.

mod common;

use common::{init, sq};
use openchess::board::Board;
use openchess::types::score::{BISHOP_VALUE, KNIGHT_VALUE, PAWN_VALUE, QUEEN_VALUE, ROOK_VALUE};
use openchess::types::PieceType;
use openchess::{Color, Move, Piece};

#[test]
fn winning_pawn_capture_unprotected() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhitePawn, sq("e4"));
    board.put_piece(Piece::BlackPawn, sq("d5"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("e4"), sq("d5"))), PAWN_VALUE);
}

#[test]
fn winning_queen_capture_unprotected_pawn() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("d1"));
    board.put_piece(Piece::BlackPawn, sq("d5"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("d1"), sq("d5"))), PAWN_VALUE);
}

#[test]
fn losing_queen_capture_defended_by_pawn() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("d1"));
    board.put_piece(Piece::BlackPawn, sq("d5"));
    board.put_piece(Piece::BlackPawn, sq("e6"));
    board.set_side_to_move(Color::White);
    assert_eq!(
        board.see(Move::new(sq("d1"), sq("d5"))),
        PAWN_VALUE - QUEEN_VALUE
    );
}

#[test]
fn equal_trade_rook_takes_rook_defended_by_rook() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteRook, sq("d1"));
    board.put_piece(Piece::BlackRook, sq("d5"));
    board.put_piece(Piece::BlackRook, sq("d8"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("d1"), sq("d5"))), 0);
}

#[test]
fn equal_trade_rook_takes_unprotected_rook() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteRook, sq("d1"));
    board.put_piece(Piece::BlackRook, sq("d5"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("d1"), sq("d5"))), ROOK_VALUE);
}

#[test]
fn xray_queen_behind_rook_continues_exchange() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteRook, sq("a2"));
    board.put_piece(Piece::WhiteQueen, sq("a1"));
    board.put_piece(Piece::BlackRook, sq("a5"));
    board.put_piece(Piece::BlackRook, sq("a8"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("a2"), sq("a5"))), ROOK_VALUE);
}

#[test]
fn bishop_capture_defended_by_pawn_loses_material() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteBishop, sq("b3"));
    board.put_piece(Piece::BlackPawn, sq("d5"));
    board.put_piece(Piece::BlackPawn, sq("e6"));
    board.set_side_to_move(Color::White);
    assert_eq!(
        board.see(Move::new(sq("b3"), sq("d5"))),
        PAWN_VALUE - BISHOP_VALUE
    );
}

#[test]
fn quiet_move_into_attacked_empty_square_is_negative() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("d1"));
    board.put_piece(Piece::BlackPawn, sq("e6"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("d1"), sq("d5"))), -QUEEN_VALUE);
}

#[test]
fn quiet_move_into_safe_empty_square_is_zero() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("d1"));
    board.set_side_to_move(Color::White);
    assert_eq!(board.see(Move::new(sq("d1"), sq("d5"))), 0);
}

#[test]
fn en_passant_capture_gains_pawn() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhitePawn, sq("e5"));
    board.put_piece(Piece::BlackPawn, sq("d5"));
    board.set_side_to_move(Color::White);
    assert_eq!(
        board.see(Move::en_passant(sq("e5"), sq("d6"))),
        PAWN_VALUE
    );
}

#[test]
fn undefended_capture_promotion_is_strongly_positive() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhitePawn, sq("a7"));
    board.put_piece(Piece::BlackKnight, sq("b8"));
    board.set_side_to_move(Color::White);
    let promo = Move::promotion(sq("a7"), sq("b8"), PieceType::Queen);
    // Capture knight + (queen − pawn) with no recapture.
    assert_eq!(board.see(promo), KNIGHT_VALUE + QUEEN_VALUE - PAWN_VALUE);
}

#[test]
fn capture_promotion_into_defended_square_reflects_recapture() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhitePawn, sq("a7"));
    board.put_piece(Piece::BlackKnight, sq("b8"));
    // Black rook defends b8; after promo-capture, black takes the queen.
    board.put_piece(Piece::BlackRook, sq("b1"));
    board.set_side_to_move(Color::White);
    let promo = Move::promotion(sq("a7"), sq("b8"), PieceType::Queen);
    // Gain knight + (Q−P), then lose the queen on the recapture → knight − pawn.
    assert_eq!(board.see(promo), KNIGHT_VALUE - PAWN_VALUE);
}

/// S2-02: opponent pawn recaptures with promotion — exchange is losing.
///
/// Qxb1, then axb1=Q. Without promo modeling SEE would be knight − queen;
/// with promo the (Q − P) bonus makes it more negative.
#[test]
fn losing_recapture_promotion_is_negative() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("b8"));
    board.put_piece(Piece::BlackKnight, sq("b1"));
    board.put_piece(Piece::BlackPawn, sq("a2"));
    board.set_side_to_move(Color::White);
    let see = board.see(Move::new(sq("b8"), sq("b1")));
    assert_eq!(
        see,
        KNIGHT_VALUE - QUEEN_VALUE - (QUEEN_VALUE - PAWN_VALUE)
    );
    assert!(see < 0);
}

/// S2-02: our pawn can promo-recapture, flipping an otherwise losing capture.
///
/// Qxc8 (rook), Ba6xc8 (bishop), b7xc8=Q. Without promo SEE is negative; with
/// queen promotion on the second recapture, STM stands at +rook (winning).
#[test]
fn winning_recapture_promotion_is_positive() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::WhiteQueen, sq("c3"));
    board.put_piece(Piece::WhitePawn, sq("b7"));
    board.put_piece(Piece::BlackRook, sq("c8"));
    board.put_piece(Piece::BlackBishop, sq("a6"));
    board.set_side_to_move(Color::White);
    let see = board.see(Move::new(sq("c3"), sq("c8")));
    assert_eq!(see, ROOK_VALUE);
    assert!(see > 0);
}

/// S2-02: black-to-move, white pawn promo-recaptures on rank 8.
#[test]
fn black_to_move_loses_to_white_promo_recapture() {
    init();
    let mut board = Board::empty();
    board.put_piece(Piece::BlackQueen, sq("b1"));
    board.put_piece(Piece::WhiteKnight, sq("b8"));
    board.put_piece(Piece::WhitePawn, sq("a7"));
    board.set_side_to_move(Color::Black);
    let see = board.see(Move::new(sq("b1"), sq("b8")));
    assert_eq!(
        see,
        KNIGHT_VALUE - QUEEN_VALUE - (QUEEN_VALUE - PAWN_VALUE)
    );
    assert!(see < 0);
}
