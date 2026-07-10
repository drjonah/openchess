//! Move generation.
//!
//! Pseudo-legal candidates are generated with simple occupancy masks (own
//! pieces can't be captured, castling paths must be empty and unattacked),
//! then filtered to strictly legal moves by playing each one on a scratch
//! board and checking whether the moving side's king ends up attacked.

use super::Board;
use crate::lookup;
use crate::types::{Bitboard, CastlingRights, Color, Move, PieceType, Square};

/// Back rank a pawn of `color` promotes on.
fn promotion_rank(color: Color) -> u8 {
    match color {
        Color::White => 7,
        Color::Black => 0,
    }
}

/// Rank a pawn of `color` starts on, from which a double push is legal.
fn pawn_start_rank(color: Color) -> u8 {
    match color {
        Color::White => 1,
        Color::Black => 6,
    }
}

/// Push a pawn move to `from`->`to`, expanding it into all four promotion
/// moves if `to` lands on the back rank.
fn push_pawn_move(moves: &mut Vec<Move>, from: Square, to: Square, promo_rank: u8) {
    if to.rank() == promo_rank {
        for piece_type in [
            PieceType::Queen,
            PieceType::Rook,
            PieceType::Bishop,
            PieceType::Knight,
        ] {
            moves.push(Move::promotion(from, to, piece_type));
        }
    } else {
        moves.push(Move::new(from, to));
    }
}

impl Board {
    /// Whether the side to move's king is currently attacked.
    pub fn in_check(&self) -> bool {
        let us = self.side_to_move();
        self.is_square_attacked(self.king_sq(us), !us)
    }

    /// All strictly legal moves in the current position.
    pub fn legal_moves(&self) -> Vec<Move> {
        let mut moves = Vec::new();
        self.generate_legal(&mut moves);
        moves
    }

    /// Append every strictly legal move for the side to move.
    pub fn generate_legal(&self, moves: &mut Vec<Move>) {
        let mut pseudo = Vec::new();
        self.generate_pseudo_captures(&mut pseudo);
        self.generate_pseudo_quiets(&mut pseudo);
        self.filter_legal(&pseudo, moves);
    }

    /// Append legal noisy moves: captures, en passant, and capturing promotions.
    pub fn generate_captures(&self, moves: &mut Vec<Move>) {
        let mut pseudo = Vec::new();
        self.generate_pseudo_captures(&mut pseudo);
        self.filter_legal(&pseudo, moves);
    }

    /// Append legal quiet moves: pushes, non-capturing promotions, and castling.
    pub fn generate_quiets(&self, moves: &mut Vec<Move>) {
        let mut pseudo = Vec::new();
        self.generate_pseudo_quiets(&mut pseudo);
        self.filter_legal(&pseudo, moves);
    }

    /// Legal evasions when in check. Equivalent to [`Self::generate_legal`];
    /// provided as a documented entry point for callers that already know
    /// they're in check (e.g. search).
    pub fn generate_evasions(&self, moves: &mut Vec<Move>) {
        debug_assert!(
            self.in_check(),
            "generate_evasions called outside of check"
        );
        self.generate_legal(moves);
    }

    /// Filter pseudo-legal `candidates` down to moves that don't leave the
    /// moving side's king attacked, appending survivors to `out`.
    fn filter_legal(&self, candidates: &[Move], out: &mut Vec<Move>) {
        let us = self.side_to_move();
        let mut scratch = self.clone();
        for &m in candidates {
            scratch.make(m);
            if !scratch.is_square_attacked(scratch.king_sq(us), !us) {
                out.push(m);
            }
            scratch.unmake(m);
        }
    }

    fn generate_pseudo_captures(&self, moves: &mut Vec<Move>) {
        let us = self.side_to_move();
        // Never generate captures of the enemy king (would leave an illegal position).
        let enemy = self.pieces_color(!us) & !self.pieces(PieceType::King);

        self.generate_pawn_captures(us, moves);
        self.generate_piece_moves(us, enemy, moves);
    }

    fn generate_pseudo_quiets(&self, moves: &mut Vec<Move>) {
        let us = self.side_to_move();
        let empty = !self.occupancy();

        self.generate_pawn_quiets(us, moves);
        self.generate_piece_moves(us, empty, moves);
        self.generate_castling(us, moves);
    }

    /// Knight/bishop/rook/queen/king moves landing on any square in `target`.
    fn generate_piece_moves(&self, us: Color, target: Bitboard, moves: &mut Vec<Move>) {
        let occ = self.occupancy();
        for piece_type in [
            PieceType::Knight,
            PieceType::Bishop,
            PieceType::Rook,
            PieceType::Queen,
            PieceType::King,
        ] {
            for from in (self.pieces(piece_type) & self.pieces_color(us)).squares() {
                let attacks = lookup::attacks_bb(piece_type, from, occ) & target;
                for to in attacks.squares() {
                    moves.push(Move::new(from, to));
                }
            }
        }
    }

    fn generate_pawn_captures(&self, us: Color, moves: &mut Vec<Move>) {
        let enemy = self.pieces_color(!us) & !self.pieces(PieceType::King);
        let promo_rank = promotion_rank(us);
        let ep_square = self.ep_square();

        for from in (self.pieces(PieceType::Pawn) & self.pieces_color(us)).squares() {
            let attacks = lookup::pawn_attacks(us, from);
            for to in (attacks & enemy).squares() {
                push_pawn_move(moves, from, to, promo_rank);
            }
            if let Some(ep) = ep_square {
                if attacks.contains(ep) {
                    moves.push(Move::en_passant(from, ep));
                }
            }
        }
    }

    fn generate_pawn_quiets(&self, us: Color, moves: &mut Vec<Move>) {
        let empty = !self.occupancy();
        let promo_rank = promotion_rank(us);
        let start_rank = pawn_start_rank(us);
        let dr: i8 = if us == Color::White { 1 } else { -1 };

        for from in (self.pieces(PieceType::Pawn) & self.pieces_color(us)).squares() {
            let Some(one) = from.offset(0, dr) else {
                continue;
            };
            if !empty.contains(one) {
                continue;
            }
            push_pawn_move(moves, from, one, promo_rank);

            if from.rank() == start_rank {
                if let Some(two) = one.offset(0, dr) {
                    if empty.contains(two) {
                        moves.push(Move::new(from, two));
                    }
                }
            }
        }
    }

    fn generate_castling(&self, us: Color, moves: &mut Vec<Move>) {
        let (king_home, king_side, queen_side, rook_king_home, rook_queen_home) = match us {
            Color::White => (Square::E1, Square::G1, Square::C1, Square::H1, Square::A1),
            Color::Black => (Square::E8, Square::G8, Square::C8, Square::H8, Square::A8),
        };
        if self.king_sq(us) != king_home || self.in_check() {
            return;
        }

        let them = !us;
        let occ = self.occupancy();
        let rights = self.castling_rights();
        let rook = crate::types::Piece::new(us, PieceType::Rook);

        let (king_right, queen_right, king_path, queen_path): (_, _, &[Square], &[Square]) =
            match us {
                Color::White => (
                    CastlingRights::WHITE_KING,
                    CastlingRights::WHITE_QUEEN,
                    &[Square::F1, Square::G1],
                    &[Square::B1, Square::C1, Square::D1],
                ),
                Color::Black => (
                    CastlingRights::BLACK_KING,
                    CastlingRights::BLACK_QUEEN,
                    &[Square::F8, Square::G8],
                    &[Square::B8, Square::C8, Square::D8],
                ),
            };

        if rights.contains(king_right)
            && self.piece_on(rook_king_home) == rook
            && king_path.iter().all(|&sq| !occ.contains(sq))
            && king_path.iter().all(|&sq| !self.is_square_attacked(sq, them))
        {
            moves.push(Move::castling(king_home, king_side));
        }

        if rights.contains(queen_right)
            && self.piece_on(rook_queen_home) == rook
            && queen_path.iter().all(|&sq| !occ.contains(sq))
        {
            // The king only passes through its two nearest squares; the far
            // corner (b-file) just needs to be empty for the rook to slide.
            let safe = queen_path[1..]
                .iter()
                .all(|&sq| !self.is_square_attacked(sq, them));
            if safe {
                moves.push(Move::castling(king_home, queen_side));
            }
        }
    }
}
