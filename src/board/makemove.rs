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
/// Both methods default to no-ops; implementors override only what they need.
/// Hooks fire *after* the board has been fully updated for the move.
pub trait BoardObserver {
    fn on_make(&mut self, board: &Board, m: Move) {
        let _ = (board, m);
    }

    fn on_unmake(&mut self, board: &Board, m: Move) {
        let _ = (board, m);
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

    /// Apply `m` to the board, pushing a [`StateInfo`] snapshot for [`Self::unmake`].
    pub fn make(&mut self, m: Move) {
        self.make_observed(m, None);
    }

    /// Like [`Self::make`], additionally notifying `observer` once the move has
    /// been fully applied.
    pub fn make_observed(&mut self, m: Move, observer: Option<&mut dyn BoardObserver>) {
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
                state.captured = self.remove_piece(cap_sq);
                self.key ^= zobrist::piece_key(state.captured, cap_sq);
                self.relocate_piece(from, to);
                self.key ^= zobrist::piece_key(moving_piece, from);
                self.key ^= zobrist::piece_key(moving_piece, to);
            }
            flags::CASTLING => {
                self.relocate_piece(from, to);
                self.key ^= zobrist::piece_key(moving_piece, from);
                self.key ^= zobrist::piece_key(moving_piece, to);

                let (rook_from, rook_to) = castling_rook_squares(us, to);
                let rook = Piece::new(us, PieceType::Rook);
                self.relocate_piece(rook_from, rook_to);
                self.key ^= zobrist::piece_key(rook, rook_from);
                self.key ^= zobrist::piece_key(rook, rook_to);
            }
            flags::PROMOTION_KNIGHT
            | flags::PROMOTION_BISHOP
            | flags::PROMOTION_ROOK
            | flags::PROMOTION_QUEEN => {
                state.captured = self.piece_on(to);
                if !state.captured.is_empty() {
                    self.key ^= zobrist::piece_key(state.captured, to);
                }
                self.remove_piece(from);
                self.key ^= zobrist::piece_key(moving_piece, from);
                let promo = m
                    .promotion_piece()
                    .expect("promotion move carries a promotion piece");
                let promo_piece = Piece::new(us, promo);
                self.put_piece(promo_piece, to);
                self.key ^= zobrist::piece_key(promo_piece, to);
            }
            _ => {
                state.captured = self.piece_on(to);
                if !state.captured.is_empty() {
                    self.remove_piece(to);
                    self.key ^= zobrist::piece_key(state.captured, to);
                }
                self.relocate_piece(from, to);
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

    /// Like [`Self::unmake`], additionally notifying `observer` once the move
    /// has been fully reversed.
    pub fn unmake_observed(&mut self, m: Move, observer: Option<&mut dyn BoardObserver>) {
        let state = self
            .history
            .pop()
            .expect("unmake called with an empty state stack");

        let from = m.from();
        let to = m.to();
        let us = !self.side_to_move;

        match m.flags() {
            flags::EN_PASSANT => {
                self.relocate_piece(to, from);
                let cap_sq = Square::from_file_rank(to.file(), from.rank())
                    .expect("en passant capture square is on the board");
                self.put_piece(state.captured, cap_sq);
            }
            flags::CASTLING => {
                let (rook_from, rook_to) = castling_rook_squares(us, to);
                self.relocate_piece(rook_to, rook_from);
                self.relocate_piece(to, from);
            }
            flags::PROMOTION_KNIGHT
            | flags::PROMOTION_BISHOP
            | flags::PROMOTION_ROOK
            | flags::PROMOTION_QUEEN => {
                self.remove_piece(to);
                self.put_piece(Piece::new(us, PieceType::Pawn), from);
                if !state.captured.is_empty() {
                    self.put_piece(state.captured, to);
                }
            }
            _ => {
                self.relocate_piece(to, from);
                if !state.captured.is_empty() {
                    self.put_piece(state.captured, to);
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
}
