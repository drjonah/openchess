//! Generate Standard Algebraic Notation for a legal move.

use crate::board::Board;
use crate::types::{Move, PieceType};

/// Format `mv` as SAN for the position on `board` (before the move is played).
pub fn format_san(board: &Board, mv: Move) -> String {
    let mut san = format_san_body(board, mv);
    let mut after = board.clone();
    after.make(mv);
    if after.in_check() {
        if after.legal_moves().is_empty() {
            san.push('#');
        } else {
            san.push('+');
        }
    }
    san
}

fn format_san_body(board: &Board, mv: Move) -> String {
    if mv.is_castling() {
        return if mv.to().file() >= 6 {
            "O-O".into()
        } else {
            "O-O-O".into()
        };
    }

    let from = mv.from();
    let to = mv.to();
    let piece = board
        .piece_on(from)
        .piece_type()
        .unwrap_or(PieceType::Pawn);
    let capture = !board.piece_on(to).is_empty() || mv.is_en_passant();

    let mut s = String::new();
    if piece == PieceType::Pawn {
        if capture {
            s.push(file_char(from.file()));
            s.push('x');
        }
        s.push_str(&to.to_string());
        if let Some(promo) = mv.promotion_piece() {
            s.push('=');
            s.push(piece_letter(promo));
        }
    } else {
        s.push(piece_letter(piece));
        s.push_str(&disambiguation(board, mv, piece));
        if capture {
            s.push('x');
        }
        s.push_str(&to.to_string());
    }
    s
}

fn disambiguation(board: &Board, mv: Move, piece: PieceType) -> String {
    let from = mv.from();
    let to = mv.to();
    let others: Vec<Move> = board
        .legal_moves()
        .into_iter()
        .filter(|m| {
            *m != mv
                && m.to() == to
                && m.promotion_piece() == mv.promotion_piece()
                && board.piece_on(m.from()).piece_type() == Some(piece)
        })
        .collect();
    if others.is_empty() {
        return String::new();
    }
    let file_unique = others.iter().all(|m| m.from().file() != from.file());
    let rank_unique = others.iter().all(|m| m.from().rank() != from.rank());
    if file_unique {
        file_char(from.file()).to_string()
    } else if rank_unique {
        rank_char(from.rank()).to_string()
    } else {
        from.to_string()
    }
}

fn piece_letter(pt: PieceType) -> char {
    match pt {
        PieceType::Pawn => 'P',
        PieceType::Knight => 'N',
        PieceType::Bishop => 'B',
        PieceType::Rook => 'R',
        PieceType::Queen => 'Q',
        PieceType::King => 'K',
    }
}

fn file_char(file: u8) -> char {
    (b'a' + file) as char
}

fn rank_char(rank: u8) -> char {
    (b'1' + rank) as char
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::session::resolve_player_move;

    fn play(board: &mut Board, san: &str) {
        let mv = resolve_player_move(board, san).unwrap();
        board.make(mv);
    }

    #[test]
    fn pawn_push_and_capture() {
        let mut board = Board::startpos();
        let e4 = resolve_player_move(&board, "e4").unwrap();
        assert_eq!(format_san(&board, e4), "e4");
        board.make(e4);
        play(&mut board, "d5");
        let exd5 = resolve_player_move(&board, "exd5").unwrap();
        assert_eq!(format_san(&board, exd5), "exd5");
    }

    #[test]
    fn knight_and_castling() {
        let mut board = Board::startpos();
        let nf3 = resolve_player_move(&board, "Nf3").unwrap();
        assert_eq!(format_san(&board, nf3), "Nf3");
        board.make(nf3);
        play(&mut board, "Nc6");
        play(&mut board, "g3");
        play(&mut board, "e5");
        play(&mut board, "Bg2");
        play(&mut board, "d6");
        let oo = resolve_player_move(&board, "O-O").unwrap();
        assert_eq!(format_san(&board, oo), "O-O");
    }

    #[test]
    fn check_suffix() {
        // Fool's mate setup: f3 e5 g4 Qh4#
        let mut board = Board::startpos();
        play(&mut board, "f3");
        play(&mut board, "e5");
        play(&mut board, "g4");
        let qh4 = resolve_player_move(&board, "Qh4").unwrap();
        assert_eq!(format_san(&board, qh4), "Qh4#");
    }
}
