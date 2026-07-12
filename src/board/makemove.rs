//! Move application and reversal: make/unmake with an explicit state stack.
//!
//! [`Board::make`] mutates the board in place and pushes a [`StateInfo`]
//! snapshot of everything that can't be recovered by reversing the piece
//! movement alone (castling rights, EP square, halfmove clock, captured
//! piece, key). [`Board::unmake`] pops that snapshot and undoes the move,
//! given the *same* [`Move`] that was passed to `make`.

use super::Board;
use crate::types::moves::flags;
use crate::types::{zobrist, Bitboard, CastlingRights, Color, Key, Move, Piece, PieceType, Square};

/// Snapshot of irreversible board state, pushed before each [`Board::make`]
/// and popped by the matching [`Board::unmake`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StateInfo {
    pub castling: CastlingRights,
    pub ep_square: Option<Square>,
    pub halfmove_clock: u16,
    pub fullmove_number: u16,
    /// Piece captured by the move, or [`Piece::Empty`] for a quiet move.
    pub captured: Piece,
    pub key: Key,
    pub checkers: Bitboard,
    pub pinned: Bitboard,
    pub pinners: Bitboard,
}

/// Observer hook surface for incremental evaluation (e.g. NNUE accumulators).
///
/// All methods default to no-ops; implementors override only what they need.
///
/// # Hook ordering
///
/// During [`Board::make_observed`]:
/// - [`Self::on_remove`] / [`Self::on_add`] fire as each piece placement changes
///   (the board may be mid-update; do not read global consistency from it here).
/// - [`Self::on_make`] fires once at the end, when the board is fully consistent.
///
/// During [`Board::unmake_observed`]:
/// - Piece deltas fire as the move is reversed (same mid-update caveat).
/// - [`Self::on_unmake`] fires once at the end, when the prior position is restored.
///
/// A relocate is reported as remove-from then add-to. Captures emit an extra
/// remove for the captured piece. Castling emits king and rook relocations.
/// Promotions remove the pawn and add the promoted piece (plus remove the
/// captured piece when promoting with capture). En passant removes the
/// captured pawn on its rank, not on the destination square.
pub trait BoardObserver {
    fn on_make(&mut self, board: &Board, m: Move) {
        let _ = (board, m);
    }

    fn on_unmake(&mut self, board: &Board, m: Move) {
        let _ = (board, m);
    }

    fn on_add(&mut self, piece: Piece, sq: Square) {
        let _ = (piece, sq);
    }

    fn on_remove(&mut self, piece: Piece, sq: Square) {
        let _ = (piece, sq);
    }
}

fn observe_add(observer: &mut Option<&mut dyn BoardObserver>, piece: Piece, sq: Square) {
    if piece.is_empty() {
        return;
    }
    if let Some(obs) = observer.as_mut() {
        obs.on_add(piece, sq);
    }
}

fn observe_remove(observer: &mut Option<&mut dyn BoardObserver>, piece: Piece, sq: Square) {
    if piece.is_empty() {
        return;
    }
    if let Some(obs) = observer.as_mut() {
        obs.on_remove(piece, sq);
    }
}

/// Rook `(from, to)` squares for a castling move, given the side to move and
/// the king's destination square.
fn castling_rook_squares(color: Color, king_to: Square) -> (Square, Square) {
    if king_to == Square::G1 {
        (Square::H1, Square::F1)
    } else if king_to == Square::C1 {
        (Square::A1, Square::D1)
    } else if king_to == Square::G8 {
        (Square::H8, Square::F8)
    } else if king_to == Square::C8 {
        (Square::A8, Square::D8)
    } else {
        panic!("invalid castling king destination for {color:?}: {king_to}")
    }
}

/// The castling right lost when a king or rook leaves, or a rook is captured
/// on, one of the four home corner squares.
fn castling_right_for_square(sq: Square) -> Option<CastlingRights> {
    if sq == Square::A1 {
        Some(CastlingRights::WHITE_QUEEN)
    } else if sq == Square::H1 {
        Some(CastlingRights::WHITE_KING)
    } else if sq == Square::A8 {
        Some(CastlingRights::BLACK_QUEEN)
    } else if sq == Square::H8 {
        Some(CastlingRights::BLACK_KING)
    } else {
        None
    }
}

/// Both castling rights belonging to `color`.
fn king_rights(color: Color) -> CastlingRights {
    match color {
        Color::White => CastlingRights::WHITE_KING.with(CastlingRights::WHITE_QUEEN),
        Color::Black => CastlingRights::BLACK_KING.with(CastlingRights::BLACK_QUEEN),
    }
}

impl Board {
    /// Relocate a piece from `from` to `to`. `to` must already be empty;
    /// callers are responsible for removing any captured piece first.
    fn relocate_piece(&mut self, from: Square, to: Square) {
        let piece = self.piece_on(from);
        debug_assert!(!piece.is_empty(), "relocate_piece: {from} is empty");
        debug_assert!(
            self.piece_on(to).is_empty(),
            "relocate_piece: destination {to} is occupied"
        );

        let piece_type = piece.piece_type().unwrap();
        let color = piece.color().unwrap();

        self.pieces[piece_type.index()].clear(from);
        self.pieces[piece_type.index()].set(to);
        self.pieces_by_color[color.index()].clear(from);
        self.pieces_by_color[color.index()].set(to);
        self.occupancy.clear(from);
        self.occupancy.set(to);
        self.mailbox[from.index() as usize] = Piece::Empty;
        self.mailbox[to.index() as usize] = piece;
    }

    /// Like [`Self::relocate_piece`], notifying `observer` of the remove/add pair.
    fn relocate_piece_observed(
        &mut self,
        from: Square,
        to: Square,
        observer: &mut Option<&mut dyn BoardObserver>,
    ) {
        let piece = self.piece_on(from);
        observe_remove(observer, piece, from);
        self.relocate_piece(from, to);
        observe_add(observer, piece, to);
    }

    /// Like [`Self::remove_piece`], notifying `observer` before clearing the square.
    fn remove_piece_observed(
        &mut self,
        sq: Square,
        observer: &mut Option<&mut dyn BoardObserver>,
    ) -> Piece {
        let piece = self.piece_on(sq);
        observe_remove(observer, piece, sq);
        self.remove_piece(sq)
    }

    /// Like [`Self::put_piece`], notifying remove of any occupant then add of `piece`.
    fn put_piece_observed(
        &mut self,
        piece: Piece,
        sq: Square,
        observer: &mut Option<&mut dyn BoardObserver>,
    ) {
        let existing = self.piece_on(sq);
        observe_remove(observer, existing, sq);
        self.put_piece(piece, sq);
        observe_add(observer, piece, sq);
    }

    /// Apply `m` to the board, pushing a [`StateInfo`] snapshot for [`Self::unmake`].
    pub fn make(&mut self, m: Move) {
        self.make_observed(m, None);
    }

    /// Like [`Self::make`], additionally notifying `observer` of piece deltas and
    /// then [`BoardObserver::on_make`] once the move has been fully applied.
    pub fn make_observed(&mut self, m: Move, mut observer: Option<&mut dyn BoardObserver>) {
        let from = m.from();
        let to = m.to();
        let us = self.side_to_move;

        let moving_piece = self.piece_on(from);
        debug_assert!(!moving_piece.is_empty(), "make: no piece on {from}");
        let moving_type = moving_piece.piece_type().unwrap();

        let mut state = StateInfo {
            castling: self.castling,
            ep_square: self.ep_square,
            halfmove_clock: self.halfmove_clock,
            fullmove_number: self.fullmove_number,
            captured: Piece::Empty,
            key: self.key,
            checkers: self.checkers,
            pinned: self.pinned,
            pinners: self.pinners,
        };

        match m.flags() {
            flags::EN_PASSANT => {
                let cap_sq = Square::from_file_rank(to.file(), from.rank())
                    .expect("en passant capture square is on the board");
                state.captured = self.remove_piece_observed(cap_sq, &mut observer);
                self.key ^= zobrist::piece_key(state.captured, cap_sq);
                self.relocate_piece_observed(from, to, &mut observer);
                self.key ^= zobrist::piece_key(moving_piece, from);
                self.key ^= zobrist::piece_key(moving_piece, to);
            }
            flags::CASTLING => {
                self.relocate_piece_observed(from, to, &mut observer);
                self.key ^= zobrist::piece_key(moving_piece, from);
                self.key ^= zobrist::piece_key(moving_piece, to);

                let (rook_from, rook_to) = castling_rook_squares(us, to);
                let rook = Piece::new(us, PieceType::Rook);
                self.relocate_piece_observed(rook_from, rook_to, &mut observer);
                self.key ^= zobrist::piece_key(rook, rook_from);
                self.key ^= zobrist::piece_key(rook, rook_to);
            }
            flags::PROMOTION_KNIGHT
            | flags::PROMOTION_BISHOP
            | flags::PROMOTION_ROOK
            | flags::PROMOTION_QUEEN => {
                state.captured = self.piece_on(to);
                if !state.captured.is_empty() {
                    // Cleared by put_piece below; notify remove now so the
                    // delta is visible before the promo piece is added.
                    observe_remove(&mut observer, state.captured, to);
                    self.key ^= zobrist::piece_key(state.captured, to);
                }
                self.remove_piece_observed(from, &mut observer);
                self.key ^= zobrist::piece_key(moving_piece, from);
                let promo = m
                    .promotion_piece()
                    .expect("promotion move carries a promotion piece");
                let promo_piece = Piece::new(us, promo);
                self.put_piece(promo_piece, to);
                observe_add(&mut observer, promo_piece, to);
                self.key ^= zobrist::piece_key(promo_piece, to);
            }
            _ => {
                state.captured = self.piece_on(to);
                if !state.captured.is_empty() {
                    self.remove_piece_observed(to, &mut observer);
                    self.key ^= zobrist::piece_key(state.captured, to);
                }
                self.relocate_piece_observed(from, to, &mut observer);
                self.key ^= zobrist::piece_key(moving_piece, from);
                self.key ^= zobrist::piece_key(moving_piece, to);
            }
        }

        // Castling rights: king or rook leaving home, or an enemy rook
        // captured on its home square.
        if moving_type == PieceType::King {
            self.castling.remove(king_rights(us));
        } else if moving_type == PieceType::Rook {
            if let Some(right) = castling_right_for_square(from) {
                self.castling.remove(right);
            }
        }
        if state.captured.piece_type() == Some(PieceType::Rook) {
            if let Some(right) = castling_right_for_square(to) {
                self.castling.remove(right);
            }
        }
        if self.castling != state.castling {
            self.key ^= zobrist::castling_key(state.castling);
            self.key ^= zobrist::castling_key(self.castling);
        }

        // EP square is only live for the move right after a pawn double push.
        if let Some(old_ep) = state.ep_square {
            self.key ^= zobrist::ep_key(old_ep.file());
        }
        self.ep_square = if moving_type == PieceType::Pawn
            && (to.rank() as i8 - from.rank() as i8).abs() == 2
        {
            Square::from_file_rank(from.file(), (from.rank() + to.rank()) / 2)
        } else {
            None
        };
        if let Some(new_ep) = self.ep_square {
            self.key ^= zobrist::ep_key(new_ep.file());
        }

        self.halfmove_clock = if moving_type == PieceType::Pawn || !state.captured.is_empty() {
            0
        } else {
            self.halfmove_clock + 1
        };

        if us == Color::Black {
            self.fullmove_number += 1;
        }

        self.side_to_move = !us;
        self.key ^= zobrist::side_key();
        self.refresh_checkers_and_pins();
        self.history.push(state);

        if let Some(observer) = observer {
            observer.on_make(self, m);
        }
    }

    /// Reverse the effect of the most recent [`Self::make`] call, which must
    /// have been given the same move `m`.
    pub fn unmake(&mut self, m: Move) {
        self.unmake_observed(m, None);
    }

    /// Like [`Self::unmake`], additionally notifying `observer` of piece deltas and
    /// then [`BoardObserver::on_unmake`] once the move has been fully reversed.
    pub fn unmake_observed(&mut self, m: Move, mut observer: Option<&mut dyn BoardObserver>) {
        let state = self
            .history
            .pop()
            .expect("unmake called with an empty state stack");

        let from = m.from();
        let to = m.to();
        let us = !self.side_to_move;

        match m.flags() {
            flags::EN_PASSANT => {
                self.relocate_piece_observed(to, from, &mut observer);
                let cap_sq = Square::from_file_rank(to.file(), from.rank())
                    .expect("en passant capture square is on the board");
                self.put_piece_observed(state.captured, cap_sq, &mut observer);
            }
            flags::CASTLING => {
                let (rook_from, rook_to) = castling_rook_squares(us, to);
                self.relocate_piece_observed(rook_to, rook_from, &mut observer);
                self.relocate_piece_observed(to, from, &mut observer);
            }
            flags::PROMOTION_KNIGHT
            | flags::PROMOTION_BISHOP
            | flags::PROMOTION_ROOK
            | flags::PROMOTION_QUEEN => {
                self.remove_piece_observed(to, &mut observer);
                self.put_piece_observed(Piece::new(us, PieceType::Pawn), from, &mut observer);
                if !state.captured.is_empty() {
                    self.put_piece_observed(state.captured, to, &mut observer);
                }
            }
            _ => {
                self.relocate_piece_observed(to, from, &mut observer);
                if !state.captured.is_empty() {
                    self.put_piece_observed(state.captured, to, &mut observer);
                }
            }
        }

        self.castling = state.castling;
        self.ep_square = state.ep_square;
        self.halfmove_clock = state.halfmove_clock;
        self.fullmove_number = state.fullmove_number;
        self.key = state.key;
        self.checkers = state.checkers;
        self.pinned = state.pinned;
        self.pinners = state.pinners;
        self.side_to_move = us;

        if let Some(observer) = observer {
            observer.on_unmake(self, m);
        }
    }

    /// Pass the turn without moving a piece (null move for NMP).
    ///
    /// Pushes a [`StateInfo`] snapshot; reverse with [`Self::undo_null`].
    /// Clears en passant, increments the halfmove clock, flips the side to
    /// move, and refreshes checkers/pins. Does not touch pieces or castling.
    pub fn do_null(&mut self) {
        let us = self.side_to_move;

        let state = StateInfo {
            castling: self.castling,
            ep_square: self.ep_square,
            halfmove_clock: self.halfmove_clock,
            fullmove_number: self.fullmove_number,
            captured: Piece::Empty,
            key: self.key,
            checkers: self.checkers,
            pinned: self.pinned,
            pinners: self.pinners,
        };

        if let Some(old_ep) = self.ep_square {
            self.key ^= zobrist::ep_key(old_ep.file());
            self.ep_square = None;
        }

        self.halfmove_clock += 1;

        if us == Color::Black {
            self.fullmove_number += 1;
        }

        self.side_to_move = !us;
        self.key ^= zobrist::side_key();
        self.refresh_checkers_and_pins();
        self.history.push(state);
    }

    /// Reverse the most recent [`Self::do_null`].
    pub fn undo_null(&mut self) {
        let state = self
            .history
            .pop()
            .expect("undo_null called with an empty state stack");

        self.castling = state.castling;
        self.ep_square = state.ep_square;
        self.halfmove_clock = state.halfmove_clock;
        self.fullmove_number = state.fullmove_number;
        self.key = state.key;
        self.checkers = state.checkers;
        self.pinned = state.pinned;
        self.pinners = state.pinners;
        self.side_to_move = !self.side_to_move;
    }
}

#[cfg(test)]
mod observer_tests {
    use super::*;
    use crate::types::CastlingRights;
    use std::str::FromStr;

    #[derive(Default)]
    struct MockObserver {
        adds: Vec<(Piece, Square)>,
        removes: Vec<(Piece, Square)>,
        makes: u32,
        unmakes: u32,
    }

    impl BoardObserver for MockObserver {
        fn on_make(&mut self, _board: &Board, _m: Move) {
            self.makes += 1;
        }

        fn on_unmake(&mut self, _board: &Board, _m: Move) {
            self.unmakes += 1;
        }

        fn on_add(&mut self, piece: Piece, sq: Square) {
            self.adds.push((piece, sq));
        }

        fn on_remove(&mut self, piece: Piece, sq: Square) {
            self.removes.push((piece, sq));
        }
    }

    fn sq(name: &str) -> Square {
        Square::from_str(name).unwrap()
    }

    fn sort_deltas(deltas: &[(Piece, Square)]) -> Vec<(Piece, Square)> {
        let mut out = deltas.to_vec();
        out.sort_by_key(|(piece, square)| (piece.slot_index(), square.index()));
        out
    }

    fn assert_multiset_eq(actual: &[(Piece, Square)], expected: &[(Piece, Square)]) {
        assert_eq!(
            sort_deltas(actual),
            sort_deltas(expected),
            "delta multiset mismatch\n actual: {actual:?}\n expected: {expected:?}"
        );
    }

    fn init() {
        crate::lookup::initialize();
    }

    #[test]
    fn quiet_move_deltas() {
        init();
        let mut board = Board::startpos();
        let mut obs = MockObserver::default();
        let m = Move::new(sq("e2"), sq("e4"));

        board.make_observed(m, Some(&mut obs));
        assert_eq!(obs.makes, 1);
        assert_multiset_eq(&obs.removes, &[(Piece::WhitePawn, sq("e2"))]);
        assert_multiset_eq(&obs.adds, &[(Piece::WhitePawn, sq("e4"))]);

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_eq!(obs.unmakes, 1);
        assert_multiset_eq(&obs.removes, &[(Piece::WhitePawn, sq("e4"))]);
        assert_multiset_eq(&obs.adds, &[(Piece::WhitePawn, sq("e2"))]);
    }

    #[test]
    fn capture_deltas() {
        init();
        let mut board =
            Board::from_fen("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let mut obs = MockObserver::default();
        let m = Move::new(sq("e4"), sq("d5"));

        board.make_observed(m, Some(&mut obs));
        assert_multiset_eq(
            &obs.removes,
            &[(Piece::BlackPawn, sq("d5")), (Piece::WhitePawn, sq("e4"))],
        );
        assert_multiset_eq(&obs.adds, &[(Piece::WhitePawn, sq("d5"))]);

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_multiset_eq(&obs.removes, &[(Piece::WhitePawn, sq("d5"))]);
        assert_multiset_eq(
            &obs.adds,
            &[(Piece::WhitePawn, sq("e4")), (Piece::BlackPawn, sq("d5"))],
        );
    }

    #[test]
    fn castling_deltas_king_and_rook() {
        init();
        let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mut obs = MockObserver::default();
        let m = Move::castling(Square::E1, Square::G1);

        board.make_observed(m, Some(&mut obs));
        assert_multiset_eq(
            &obs.removes,
            &[
                (Piece::WhiteKing, Square::E1),
                (Piece::WhiteRook, Square::H1),
            ],
        );
        assert_multiset_eq(
            &obs.adds,
            &[
                (Piece::WhiteKing, Square::G1),
                (Piece::WhiteRook, Square::F1),
            ],
        );

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_multiset_eq(
            &obs.removes,
            &[
                (Piece::WhiteKing, Square::G1),
                (Piece::WhiteRook, Square::F1),
            ],
        );
        assert_multiset_eq(
            &obs.adds,
            &[
                (Piece::WhiteKing, Square::E1),
                (Piece::WhiteRook, Square::H1),
            ],
        );
    }

    #[test]
    fn en_passant_deltas() {
        init();
        let mut board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let mut obs = MockObserver::default();
        let m = Move::en_passant(sq("e5"), sq("d6"));

        board.make_observed(m, Some(&mut obs));
        assert_multiset_eq(
            &obs.removes,
            &[(Piece::BlackPawn, sq("d5")), (Piece::WhitePawn, sq("e5"))],
        );
        assert_multiset_eq(&obs.adds, &[(Piece::WhitePawn, sq("d6"))]);

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_multiset_eq(&obs.removes, &[(Piece::WhitePawn, sq("d6"))]);
        assert_multiset_eq(
            &obs.adds,
            &[(Piece::WhitePawn, sq("e5")), (Piece::BlackPawn, sq("d5"))],
        );
    }

    #[test]
    fn quiet_promotion_deltas() {
        init();
        let mut board = Board::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut obs = MockObserver::default();
        let m = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);

        board.make_observed(m, Some(&mut obs));
        assert_multiset_eq(&obs.removes, &[(Piece::WhitePawn, sq("a7"))]);
        assert_multiset_eq(&obs.adds, &[(Piece::WhiteQueen, sq("a8"))]);

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_multiset_eq(&obs.removes, &[(Piece::WhiteQueen, sq("a8"))]);
        assert_multiset_eq(&obs.adds, &[(Piece::WhitePawn, sq("a7"))]);
    }

    #[test]
    fn capture_promotion_deltas() {
        init();
        let mut board = Board::from_fen("r3k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        board.set_castling_rights(CastlingRights::BLACK_QUEEN);
        board.rehash();
        let mut obs = MockObserver::default();
        let m = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);

        board.make_observed(m, Some(&mut obs));
        assert_multiset_eq(
            &obs.removes,
            &[(Piece::BlackRook, sq("a8")), (Piece::WhitePawn, sq("a7"))],
        );
        assert_multiset_eq(&obs.adds, &[(Piece::WhiteQueen, sq("a8"))]);

        obs.adds.clear();
        obs.removes.clear();
        board.unmake_observed(m, Some(&mut obs));
        assert_multiset_eq(&obs.removes, &[(Piece::WhiteQueen, sq("a8"))]);
        assert_multiset_eq(
            &obs.adds,
            &[(Piece::WhitePawn, sq("a7")), (Piece::BlackRook, sq("a8"))],
        );
    }

    #[test]
    fn random_walk_make_unmake_deltas_cancel() {
        init();
        let mut board = Board::startpos();
        let mut obs = MockObserver::default();
        let mut rng = 0xC0FFEE_u64;
        let mut path = Vec::new();

        for _ in 0..64 {
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let m = moves[(rng as usize) % moves.len()];
            board.make_observed(m, Some(&mut obs));
            path.push(m);
        }

        assert!(!path.is_empty());
        for m in path.iter().rev() {
            board.unmake_observed(*m, Some(&mut obs));
        }

        assert_eq!(obs.makes, path.len() as u32);
        assert_eq!(obs.unmakes, path.len() as u32);
        // Every piece that was removed was eventually added back (and vice versa).
        assert_multiset_eq(&obs.adds, &obs.removes);
    }
}
