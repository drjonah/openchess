//! Hand-crafted evaluation bootstrap (material).

use crate::board::Board;
use crate::types::score::piece_value;
use crate::types::{Color, PieceType, Value};

/// Side-to-move relative material evaluation.
///
/// Sums piece values for White minus Black (kings excluded from the material
/// balance), then negates when Black is to move.
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

    let score = white - black;
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
        board.put_piece(Piece::WhiteQueen, Square::from_str("d4").unwrap());
        board.rehash();
        assert_eq!(evaluate(&board), piece_value(PieceType::Queen));
    }
}
