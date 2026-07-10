//! P1-09 — FEN and UCI move parsing.

mod common;

use common::{KIWIPETE_FEN, START_FEN};
use openchess::board::Board;
use openchess::{CastlingRights, Color, Piece, PieceType, Square};
use std::str::FromStr;

#[test]
fn round_trips_startpos_fen() {
    let board = Board::from_fen(START_FEN).unwrap();
    assert_eq!(board.to_fen(), START_FEN);
}

#[test]
fn from_fen_startpos_matches_hardcoded_startpos() {
    let parsed = Board::from_fen(START_FEN).unwrap();
    let hardcoded = Board::startpos();
    for sq in Square::all() {
        assert_eq!(parsed.piece_on(sq), hardcoded.piece_on(sq), "mismatch at {sq}");
    }
    assert_eq!(parsed.side_to_move(), hardcoded.side_to_move());
    assert_eq!(parsed.castling_rights(), hardcoded.castling_rights());
    assert_eq!(parsed.ep_square(), hardcoded.ep_square());
    assert_eq!(parsed.key(), hardcoded.key());
}

#[test]
fn parse_uci_move_accepts_legal_opening() {
    let board = Board::from_fen(START_FEN).unwrap();
    let m = board.parse_uci_move("e2e4").unwrap();
    assert_eq!(m.from(), Square::from_str("e2").unwrap());
    assert_eq!(m.to(), Square::from_str("e4").unwrap());
}

#[test]
fn parse_uci_move_rejects_illegal_move() {
    let board = Board::from_fen(START_FEN).unwrap();
    assert!(board.parse_uci_move("e2e5").is_err());
}

#[test]
fn parse_uci_move_rejects_malformed_strings() {
    let board = Board::from_fen(START_FEN).unwrap();
    assert!(board.parse_uci_move("").is_err());
    assert!(board.parse_uci_move("e2").is_err());
    assert!(board.parse_uci_move("z9z8").is_err());
}

#[test]
fn parses_kiwipete_fen() {
    let board = Board::from_fen(KIWIPETE_FEN).unwrap();
    assert_eq!(board.to_fen(), KIWIPETE_FEN);
    assert_eq!(board.side_to_move(), Color::White);
    assert_eq!(board.castling_rights(), CastlingRights::ALL);
    assert_eq!(
        board.piece_on(Square::from_str("e5").unwrap()),
        Piece::WhiteKnight
    );
}

#[test]
fn parse_uci_move_resolves_castling_flag() {
    let board = Board::from_fen(KIWIPETE_FEN).unwrap();
    assert!(board.parse_uci_move("e1g1").unwrap().is_castling());
}

#[test]
fn parse_uci_move_resolves_promotion() {
    let board = Board::from_fen("8/P7/8/8/4k3/8/8/4K3 w - - 0 1").unwrap();
    let m = board.parse_uci_move("a7a8q").unwrap();
    assert_eq!(m.promotion_piece(), Some(PieceType::Queen));
}

#[test]
fn parse_uci_move_resolves_en_passant() {
    let board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    assert!(board.parse_uci_move("e5d6").unwrap().is_en_passant());
}

#[test]
fn from_fen_rejects_bad_inputs() {
    assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR").is_err());
    assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR x KQkq - 0 1").is_err());
    assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkqx - 0 1").is_err());
    assert!(Board::from_fen("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq z9 0 1").is_err());
}

#[test]
fn to_fen_defaults_castling_dash_when_none() {
    let board = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    assert_eq!(board.to_fen(), "4k3/8/8/8/8/8/8/4K3 w - - 0 1");
}
