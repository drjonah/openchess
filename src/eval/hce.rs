//! Hand-crafted evaluation bootstrap (material + midgame PSTs).

use crate::board::Board;
use crate::eval::pst;
use crate::types::score::piece_value;
use crate::types::{Color, PieceType, Value};

/// Side-to-move relative evaluation: material + midgame piece-square tables.
///
/// Sums piece values and PST bonuses for White minus Black (kings excluded
/// from both material and PST), then negates when Black is to move.
pub fn evaluate(board: &Board) -> Value {
    let mut white = 0;
    let mut black = 0;

    for &pt in &[
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        let value = piece_value(pt);
        let bb = board.pieces(pt);
        white += value * (bb & board.pieces_color(Color::White)).count() as Value;
        black += value * (bb & board.pieces_color(Color::Black)).count() as Value;
    }

    let score = white - black + pst::pst_midgame(board);
    if board.side_to_move() == Color::White {
        score
    } else {
        -score
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Piece, Square};
    use std::str::FromStr;

    #[test]
    fn startpos_is_zero() {
        let board = Board::startpos();
        assert_eq!(evaluate(&board), 0);
    }

    #[test]
    fn missing_white_queen_is_negative_for_white() {
        let mut board = Board::startpos();
        board.remove_piece(Square::from_str("d1").unwrap());
        board.rehash();
        let score = evaluate(&board);
        assert!(
            score < -800,
            "expected large negative for White STM, got {score}"
        );
    }

    #[test]
    fn missing_white_queen_positive_for_black_stm() {
        let mut board = Board::startpos();
        board.remove_piece(Square::from_str("d1").unwrap());
        board.set_side_to_move(Color::Black);
        board.rehash();
        let score = evaluate(&board);
        assert!(
            score > 800,
            "expected large positive for Black STM, got {score}"
        );
    }

    #[test]
    fn put_extra_white_piece_positive() {
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        let sq = Square::from_str("d4").unwrap();
        board.put_piece(Piece::WhiteQueen, sq);
        board.rehash();
        let expected =
            piece_value(PieceType::Queen) + pst::pst_value(PieceType::Queen, sq, Color::White);
        assert_eq!(evaluate(&board), expected);
    }

    #[test]
    fn knight_on_center_beats_knight_on_rim() {
        let mut center = Board::empty();
        center.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        center.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        center.put_piece(Piece::WhiteKnight, Square::from_str("e5").unwrap());
        center.rehash();

        let mut rim = Board::empty();
        rim.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        rim.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        rim.put_piece(Piece::WhiteKnight, Square::from_str("a1").unwrap());
        rim.rehash();

        let center_score = evaluate(&center);
        let rim_score = evaluate(&rim);
        assert!(
            center_score > rim_score,
            "knight on e5 ({center_score}) should beat a1 ({rim_score})"
        );
    }
}
