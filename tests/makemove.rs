//! P1-04 / P1-06 — make/unmake and incremental Zobrist.

mod common;

use common::{init, lcg_next, mv, sq};
use openchess::board::Board;
use openchess::{CastlingRights, Color, Move, Piece, PieceType, Square};

#[test]
fn make_unmake_quiet_restores_startpos() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();

    board.make(mv("e2", "e4"));
    assert_ne!(board, snapshot);
    assert_eq!(board.side_to_move(), Color::Black);
    assert_eq!(board.piece_on(sq("e4")), Piece::WhitePawn);
    assert_eq!(board.ep_square(), Some(sq("e3")));
    assert_eq!(board.key(), board.compute_key());

    board.unmake(mv("e2", "e4"));
    assert_eq!(board, snapshot);
    assert_eq!(board.key(), snapshot.key());
    assert_eq!(board.history_len(), 0);
}

#[test]
fn capture_sequence_unwinds_to_startpos() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();
    let moves = [mv("e2", "e4"), mv("d7", "d5"), mv("e4", "d5")];
    for m in moves {
        board.make(m);
        assert_eq!(board.key(), board.compute_key());
    }
    for m in moves.iter().rev() {
        board.unmake(*m);
    }
    assert_eq!(board, snapshot);
}

#[test]
fn castling_moves_king_and_rook_and_unmakes() {
    init();
    let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
    let snapshot = board.clone();
    board.make(Move::castling(Square::E1, Square::G1));

    assert_eq!(board.piece_on(Square::G1), Piece::WhiteKing);
    assert_eq!(board.piece_on(Square::F1), Piece::WhiteRook);
    assert!(board.piece_on(Square::E1).is_empty());
    assert!(board.piece_on(Square::H1).is_empty());
    assert_eq!(board.key(), board.compute_key());

    board.unmake(Move::castling(Square::E1, Square::G1));
    assert_eq!(board, snapshot);
}

#[test]
fn queenside_castling_black_round_trips() {
    init();
    let mut board = Board::from_fen("r3k3/8/8/8/8/8/8/4K3 b q - 0 1").unwrap();
    let snapshot = board.clone();
    board.make(Move::castling(Square::E8, Square::C8));

    assert_eq!(board.piece_on(Square::C8), Piece::BlackKing);
    assert_eq!(board.piece_on(Square::D8), Piece::BlackRook);
    assert_eq!(board.key(), board.compute_key());

    board.unmake(Move::castling(Square::E8, Square::C8));
    assert_eq!(board, snapshot);
}

#[test]
fn en_passant_capture_and_unmake() {
    init();
    let mut board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
    let snapshot = board.clone();
    let m = Move::en_passant(sq("e5"), sq("d6"));
    board.make(m);

    assert_eq!(board.piece_on(sq("d6")), Piece::WhitePawn);
    assert!(board.piece_on(sq("d5")).is_empty());
    assert_eq!(board.key(), board.compute_key());

    board.unmake(m);
    assert_eq!(board, snapshot);
}

#[test]
fn promotion_replaces_pawn_and_unmakes() {
    init();
    let mut board = Board::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    let snapshot = board.clone();
    let m = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);
    board.make(m);

    assert_eq!(board.piece_on(sq("a8")), Piece::WhiteQueen);
    assert_eq!(board.key(), board.compute_key());

    board.unmake(m);
    assert_eq!(board, snapshot);
}

#[test]
fn promotion_with_capture_restores_captured_piece() {
    init();
    let mut board = Board::from_fen("r3k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
    // Give black queenside castling so capturing a8 clears a right.
    board.set_castling_rights(CastlingRights::BLACK_QUEEN);
    board.rehash();
    let snapshot = board.clone();

    let m = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);
    board.make(m);
    assert_eq!(board.piece_on(sq("a8")), Piece::WhiteQueen);
    assert!(!board.castling_rights().contains(CastlingRights::BLACK_QUEEN));
    assert_eq!(board.key(), board.compute_key());

    board.unmake(m);
    assert_eq!(board, snapshot);
}

#[test]
fn scripted_walk_round_trips_to_startpos() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();
    let moves = [
        mv("e2", "e4"),
        mv("e7", "e5"),
        mv("g1", "f3"),
        mv("b8", "c6"),
        mv("f1", "b5"),
        mv("g8", "f6"),
        mv("b1", "c3"),
        mv("f8", "b4"),
        mv("d2", "d3"),
        mv("d7", "d6"),
    ];
    for m in moves {
        board.make(m);
        assert_eq!(board.key(), board.compute_key());
    }
    for m in moves.iter().rev() {
        board.unmake(*m);
    }
    assert_eq!(board, snapshot);
}

#[test]
fn startpos_key_matches_rehash() {
    let board = Board::startpos();
    assert_eq!(board.key(), board.compute_key());
    assert_ne!(board.key(), 0);
}

#[test]
fn random_walk_make_unmake_restores_board_and_key() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();
    let mut rng = 0xC0FFEE_u64;
    let mut path = Vec::new();

    for _ in 0..200 {
        let moves = board.legal_moves();
        if moves.is_empty() {
            break;
        }
        let idx = (lcg_next(&mut rng) as usize) % moves.len();
        let m = moves[idx];
        board.make(m);
        assert_eq!(board.key(), board.compute_key(), "key desynced after {m}");
        path.push(m);
    }

    assert!(!path.is_empty());
    for m in path.iter().rev() {
        board.unmake(*m);
    }
    assert_eq!(board, snapshot);
    assert_eq!(board.history_len(), 0);
}

#[test]
fn incremental_key_matches_rehash_after_thousand_random_plies() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();
    let mut rng = 0xDEAD_BEEF_u64;
    let mut path = Vec::new();
    let mut made = 0u32;
    let mut forbidden: Option<Move> = None;

    while made < 1000 {
        let moves = board.legal_moves();
        let candidates: Vec<Move> = moves
            .into_iter()
            .filter(|m| Some(*m) != forbidden)
            .collect();
        forbidden = None;

        if candidates.is_empty() {
            let Some(last) = path.pop() else { break };
            board.unmake(last);
            forbidden = Some(last);
            continue;
        }

        let idx = (lcg_next(&mut rng) as usize) % candidates.len();
        let m = candidates[idx];
        board.make(m);
        assert_eq!(
            board.key(),
            board.compute_key(),
            "key desynced at ply {made} after {m}"
        );
        path.push(m);
        made += 1;
    }

    assert_eq!(made, 1000, "expected 1000 random plies");
    for m in path.iter().rev() {
        board.unmake(*m);
        assert_eq!(board.key(), board.compute_key());
    }
    assert_eq!(board, snapshot);
    assert_eq!(board.history_len(), 0);
}

#[test]
fn do_null_undo_null_round_trips_startpos() {
    init();
    let snapshot = Board::startpos();
    let mut board = Board::startpos();

    board.do_null();
    assert_eq!(board.side_to_move(), Color::Black);
    assert_eq!(board.halfmove_clock(), 1);
    assert!(board.ep_square().is_none());
    assert_eq!(board.key(), board.compute_key());
    assert_eq!(board.history_len(), 1);

    board.undo_null();
    assert_eq!(board, snapshot);
    assert_eq!(board.key(), snapshot.key());
    assert_eq!(board.history_len(), 0);
}

#[test]
fn do_null_clears_ep_and_restores_on_undo() {
    init();
    let mut board = Board::startpos();
    board.make(mv("e2", "e4"));
    assert_eq!(board.ep_square(), Some(sq("e3")));
    let snapshot = board.clone();

    board.do_null();
    assert!(board.ep_square().is_none());
    assert_eq!(board.side_to_move(), Color::White);
    assert_eq!(board.key(), board.compute_key());

    board.undo_null();
    assert_eq!(board, snapshot);
    assert_eq!(board.ep_square(), Some(sq("e3")));
}

#[test]
fn non_pawn_material_startpos_and_bare_kp() {
    init();
    let start = Board::startpos();
    assert!(start.non_pawn_material(Color::White) > 0);
    assert!(start.non_pawn_material(Color::Black) > 0);

    let kp = Board::from_fen("4k3/8/8/8/8/8/4P3/4K3 w - - 0 1").unwrap();
    assert_eq!(kp.non_pawn_material(Color::White), 0);
    assert_eq!(kp.non_pawn_material(Color::Black), 0);
}
