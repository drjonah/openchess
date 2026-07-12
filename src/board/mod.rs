//! Board position: dual bitboard + mailbox representation.

mod makemove;
mod movegen;
mod parser;
mod see;

use crate::types::score::piece_value;
use crate::types::{
    zobrist, Bitboard, CastlingRights, Color, Key, Piece, PieceType, Square, Value,
};
use std::fmt;

pub use makemove::{BoardObserver, StateInfo};
pub use parser::{FenError, ParseMoveError};

/// Chess position with bitboards, mailbox, and game state.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Board {
    pieces: [Bitboard; PieceType::COUNT],
    pieces_by_color: [Bitboard; Color::COUNT],
    occupancy: Bitboard,
    mailbox: [Piece; Square::COUNT],
    side_to_move: Color,
    castling: CastlingRights,
    ep_square: Option<Square>,
    halfmove_clock: u16,
    fullmove_number: u16,
    key: Key,
    checkers: Bitboard,
    pinned: Bitboard,
    /// Enemy sliders currently pinning a piece in [`Self::pinned`] to our king.
    pinners: Bitboard,
    /// Undo history pushed by [`Board::make`] and popped by [`Board::unmake`].
    history: Vec<StateInfo>,
}

impl Board {
    /// Empty board with default game state.
    pub const fn empty() -> Self {
        Board {
            pieces: [Bitboard::EMPTY; PieceType::COUNT],
            pieces_by_color: [Bitboard::EMPTY; Color::COUNT],
            occupancy: Bitboard::EMPTY,
            mailbox: [Piece::Empty; Square::COUNT],
            side_to_move: Color::White,
            castling: CastlingRights::NONE,
            ep_square: None,
            halfmove_clock: 0,
            fullmove_number: 1,
            key: 0,
            checkers: Bitboard::EMPTY,
            pinned: Bitboard::EMPTY,
            pinners: Bitboard::EMPTY,
            history: Vec::new(),
        }
    }

    /// Alias for [`Self::empty`].
    pub const fn new() -> Self {
        Self::empty()
    }

    /// Standard starting position (hardcoded; no FEN parser).
    pub fn startpos() -> Self {
        let mut board = Self::empty();
        board.castling = CastlingRights::ALL;
        board.side_to_move = Color::White;
        board.halfmove_clock = 0;
        board.fullmove_number = 1;

        let back_rank = [
            Piece::WhiteRook,
            Piece::WhiteKnight,
            Piece::WhiteBishop,
            Piece::WhiteQueen,
            Piece::WhiteKing,
            Piece::WhiteBishop,
            Piece::WhiteKnight,
            Piece::WhiteRook,
        ];

        for file in 0..8 {
            let sq = Square::from_file_rank(file, 0).unwrap();
            board.put_piece(back_rank[file as usize], sq);

            let pawn_sq = Square::from_file_rank(file, 1).unwrap();
            board.put_piece(Piece::WhitePawn, pawn_sq);

            let black_pawn_sq = Square::from_file_rank(file, 6).unwrap();
            board.put_piece(Piece::BlackPawn, black_pawn_sq);

            let black_back_sq = Square::from_file_rank(file, 7).unwrap();
            board.put_piece(
                Piece::new(
                    Color::Black,
                    back_rank[file as usize].piece_type().unwrap(),
                ),
                black_back_sq,
            );
        }

        board.key = board.compute_key();
        // Pin/check refresh lands with P1-07; keep startpos buildable until then.
        board.checkers = Bitboard::EMPTY;
        board.pinned = Bitboard::EMPTY;
        board.pinners = Bitboard::EMPTY;
        board
    }

    /// Recompute the Zobrist key from scratch (pieces, side to move,
    /// castling rights, en passant file).
    ///
    /// [`Self::make`]/[`Self::unmake`] keep [`Self::key`] incrementally in
    /// sync with this definition; this is mainly for construction and tests.
    /// [`Self::put_piece`]/[`Self::remove_piece`] deliberately do *not* touch
    /// the key — callers that mutate pieces directly must [`Self::rehash`].
    pub fn compute_key(&self) -> Key {
        let mut key: Key = 0;

        for sq in Square::all() {
            let piece = self.piece_on(sq);
            if !piece.is_empty() {
                key ^= zobrist::piece_key(piece, sq);
            }
        }

        if self.side_to_move == Color::Black {
            key ^= zobrist::side_key();
        }

        key ^= zobrist::castling_key(self.castling);

        if let Some(ep) = self.ep_square {
            key ^= zobrist::ep_key(ep.file());
        }

        key
    }

    /// Store a freshly computed Zobrist key (for positions built via
    /// [`Self::put_piece`] / [`Self::remove_piece`] rather than [`Self::make`]).
    pub fn rehash(&mut self) {
        self.key = self.compute_key();
    }

    pub const fn side_to_move(&self) -> Color {
        self.side_to_move
    }

    pub const fn castling_rights(&self) -> CastlingRights {
        self.castling
    }

    pub const fn ep_square(&self) -> Option<Square> {
        self.ep_square
    }

    pub const fn halfmove_clock(&self) -> u16 {
        self.halfmove_clock
    }

    pub const fn fullmove_number(&self) -> u16 {
        self.fullmove_number
    }

    pub const fn key(&self) -> Key {
        self.key
    }

    pub const fn checkers(&self) -> Bitboard {
        self.checkers
    }

    pub const fn pinned(&self) -> Bitboard {
        self.pinned
    }

    /// Enemy sliders pinning a piece in [`Self::pinned`] to our king.
    pub const fn pinners(&self) -> Bitboard {
        self.pinners
    }

    /// Number of half-moves currently pushed on the undo stack.
    pub fn history_len(&self) -> usize {
        self.history.len()
    }

    pub fn set_side_to_move(&mut self, color: Color) {
        self.side_to_move = color;
    }

    pub fn set_castling_rights(&mut self, rights: CastlingRights) {
        self.castling = rights;
    }

    pub fn set_ep_square(&mut self, sq: Option<Square>) {
        self.ep_square = sq;
    }

    pub fn set_halfmove_clock(&mut self, n: u16) {
        self.halfmove_clock = n;
    }

    pub fn set_fullmove_number(&mut self, n: u16) {
        self.fullmove_number = n;
    }

    #[inline]
    pub fn piece_on(&self, sq: Square) -> Piece {
        self.mailbox[sq.index() as usize]
    }

    /// Place a piece on `sq`, updating bitboards and mailbox.
    ///
    /// If `sq` is occupied, the existing piece is removed first. Does *not*
    /// update [`Self::key`]; callers that build positions this way should
    /// call [`Self::compute_key`] afterwards. [`Self::make`]/[`Self::unmake`]
    /// update the key incrementally themselves.
    pub fn put_piece(&mut self, piece: Piece, sq: Square) {
        debug_assert!(!piece.is_empty(), "put_piece called with Piece::Empty");

        if !self.piece_on(sq).is_empty() {
            self.remove_piece(sq);
        }

        let piece_type = piece.piece_type().unwrap();
        let color = piece.color().unwrap();

        self.pieces[piece_type.index()].set(sq);
        self.pieces_by_color[color.index()].set(sq);
        self.occupancy.set(sq);
        self.mailbox[sq.index() as usize] = piece;
    }

    /// Remove the piece on `sq`, updating bitboards and mailbox.
    ///
    /// Does *not* update [`Self::key`]; see [`Self::put_piece`].
    pub fn remove_piece(&mut self, sq: Square) -> Piece {
        let piece = self.piece_on(sq);
        if piece.is_empty() {
            return Piece::Empty;
        }

        let piece_type = piece.piece_type().unwrap();
        let color = piece.color().unwrap();

        self.pieces[piece_type.index()].clear(sq);
        self.pieces_by_color[color.index()].clear(sq);
        self.occupancy.clear(sq);
        self.mailbox[sq.index() as usize] = Piece::Empty;

        piece
    }

    #[inline]
    pub const fn pieces(&self, piece_type: PieceType) -> Bitboard {
        self.pieces[piece_type.index()]
    }

    #[inline]
    pub const fn pieces_color(&self, color: Color) -> Bitboard {
        self.pieces_by_color[color.index()]
    }

    #[inline]
    pub const fn occupancy(&self) -> Bitboard {
        self.occupancy
    }

    /// King square for `color` (least significant bit of the king bitboard).
    pub fn king_sq(&self, color: Color) -> Square {
        (self.pieces(PieceType::King) & self.pieces_color(color))
            .lsb()
            .expect("king not found")
    }

    /// Total material for `color` (all pieces except kings), in centipawns.
    pub fn material(&self, color: Color) -> Value {
        let us = self.pieces_color(color);
        let mut sum = 0;
        for &pt in &[
            PieceType::Pawn,
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
        ] {
            sum += piece_value(pt) * (self.pieces(pt) & us).count() as Value;
        }
        sum
    }

    /// White material minus Black material, in centipawns.
    pub fn material_balance(&self) -> Value {
        self.material(Color::White) - self.material(Color::Black)
    }

    /// Non-pawn material for `color` (knights + bishops + rooks + queens).
    ///
    /// Used as a simple zugzwang gate for null-move pruning.
    pub fn non_pawn_material(&self, color: Color) -> Value {
        let us = self.pieces_color(color);
        let mut sum = 0;
        for &pt in &[
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
        ] {
            sum += piece_value(pt) * (self.pieces(pt) & us).count() as Value;
        }
        sum
    }

    /// Whether `sq` is attacked by any piece of color `by`.
    ///
    /// Useful for later castling/check-safety checks; does not depend on movegen.
    pub fn is_square_attacked(&self, sq: Square, by: Color) -> bool {
        let occ = self.occupancy();
        crate::lookup::attackers_to(
            sq,
            occ,
            self.pieces(PieceType::Knight) & self.pieces_color(by),
            self.pieces(PieceType::Bishop) & self.pieces_color(by),
            self.pieces(PieceType::Rook) & self.pieces_color(by),
            self.pieces(PieceType::Queen) & self.pieces_color(by),
            self.pieces(PieceType::King) & self.pieces_color(by),
            self.pieces(PieceType::Pawn) & self.pieces_color(by),
            by,
        )
        .any()
    }

    /// Recompute [`Self::checkers`], [`Self::pinned`], and [`Self::pinners`]
    /// for the side to move's king.
    ///
    /// `checkers` is every enemy piece currently attacking our king.
    /// `pinned` is every one of our pieces that, if it moved off the
    /// king/enemy-slider line, would expose our king to check; `pinners` is
    /// the matching enemy slider on the far end of each pin.
    ///
    /// Must be called after any change to piece placement (construction,
    /// [`Self::make`]) that isn't already covered by [`Self::unmake`]
    /// restoring a [`StateInfo`] snapshot.
    ///
    /// No-ops (clearing all three fields) if the side to move has no king on
    /// the board; several unit tests build minimal boards without both
    /// kings to exercise unrelated mechanics.
    pub fn refresh_checkers_and_pins(&mut self) {
        let us = self.side_to_move;
        let them = !us;

        let Some(king_sq) = (self.pieces(PieceType::King) & self.pieces_color(us)).lsb() else {
            self.checkers = Bitboard::EMPTY;
            self.pinned = Bitboard::EMPTY;
            self.pinners = Bitboard::EMPTY;
            return;
        };
        let occ = self.occupancy();

        self.checkers = crate::lookup::attackers_to(
            king_sq,
            occ,
            self.pieces(PieceType::Knight) & self.pieces_color(them),
            self.pieces(PieceType::Bishop) & self.pieces_color(them),
            self.pieces(PieceType::Rook) & self.pieces_color(them),
            self.pieces(PieceType::Queen) & self.pieces_color(them),
            self.pieces(PieceType::King) & self.pieces_color(them),
            self.pieces(PieceType::Pawn) & self.pieces_color(them),
            them,
        );

        let enemy_bishops = self.pieces(PieceType::Bishop) & self.pieces_color(them);
        let enemy_rooks = self.pieces(PieceType::Rook) & self.pieces_color(them);
        let enemy_queens = self.pieces(PieceType::Queen) & self.pieces_color(them);

        // Enemy sliders that would attack the king on an otherwise-empty
        // board along their piece type's rays; only these can possibly pin.
        let snipers = (crate::lookup::rook_attacks(king_sq, Bitboard::EMPTY)
            & (enemy_rooks | enemy_queens))
            | (crate::lookup::bishop_attacks(king_sq, Bitboard::EMPTY)
                & (enemy_bishops | enemy_queens));

        let mut pinned = Bitboard::EMPTY;
        let mut pinners = Bitboard::EMPTY;
        for sniper_sq in snipers.squares() {
            let blockers = crate::lookup::between(king_sq, sniper_sq) & occ;
            if blockers.count() == 1 && (blockers & self.pieces_color(us)).any() {
                pinned |= blockers;
                pinners |= Bitboard::from_square(sniper_sq);
            }
        }

        self.pinned = pinned;
        self.pinners = pinners;
    }
}

impl fmt::Debug for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Board")
            .field("side_to_move", &self.side_to_move)
            .field("castling", &self.castling)
            .field("ep_square", &self.ep_square)
            .field("halfmove_clock", &self.halfmove_clock)
            .field("fullmove_number", &self.fullmove_number)
            .field("key", &self.key)
            .finish()
    }
}

impl fmt::Display for Board {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for rank in (0..8).rev() {
            for file in 0..8 {
                let sq = Square::from_file_rank(file, rank).unwrap();
                write!(f, "{}", self.piece_on(sq).to_char())?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod material_tests {
    use super::*;
    use crate::types::score::{BISHOP_VALUE, PAWN_VALUE, QUEEN_VALUE, ROOK_VALUE};

    #[test]
    fn startpos_material_is_balanced() {
        let board = Board::startpos();
        assert_eq!(board.material(Color::White), board.material(Color::Black));
        assert_eq!(board.material_balance(), 0);
    }

    #[test]
    fn kings_only_material_is_zero() {
        let board = Board::from_fen("8/8/8/8/8/8/8/k2K4 w - - 0 1").unwrap();
        assert_eq!(board.material(Color::White), 0);
        assert_eq!(board.material(Color::Black), 0);
        assert_eq!(board.material_balance(), 0);
    }

    #[test]
    fn extra_queen_shifts_balance() {
        let board = Board::from_fen("Q7/8/8/8/8/8/8/k2K4 w - - 0 1").unwrap();
        assert_eq!(board.material_balance(), QUEEN_VALUE);
    }

    #[test]
    fn missing_rook_shifts_balance_negative() {
        let board = Board::from_fen("8/8/8/8/8/8/8/k2KR3 w - - 0 1").unwrap();
        assert_eq!(board.material(Color::White), ROOK_VALUE);
        assert_eq!(board.material_balance(), ROOK_VALUE);
    }

    #[test]
    fn pawn_up_one() {
        let board = Board::from_fen("rnbqkbnr/ppp1pppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1").unwrap();
        assert_eq!(board.material(Color::White), board.material(Color::Black) + PAWN_VALUE);
        assert_eq!(board.material_balance(), PAWN_VALUE);
    }

    #[test]
    fn bishop_vs_knight_advantage() {
        let board = Board::from_fen("K1B4k/8/8/8/8/8/8/n7 w - - 0 1").unwrap();
        assert_eq!(
            board.material_balance(),
            BISHOP_VALUE - crate::types::score::KNIGHT_VALUE
        );
    }
}
