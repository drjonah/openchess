//! Static Exchange Evaluation (SEE).
//!
//! Simulates the full capture sequence on a target square, alternating
//! sides and always recapturing with the least valuable attacker, to
//! estimate the material outcome of a capture without playing it out with
//! full move generation / legality checks.

use super::Board;
use crate::lookup;
use crate::types::score::piece_value;
use crate::types::{Bitboard, Color, Move, PieceType, Square, Value};

/// Piece types ordered from least to most valuable, used to pick the next
/// least valuable attacker in the exchange.
const ATTACKER_ORDER: [PieceType; 6] = [
    PieceType::Pawn,
    PieceType::Knight,
    PieceType::Bishop,
    PieceType::Rook,
    PieceType::Queen,
    PieceType::King,
];

impl Board {
    /// Static Exchange Evaluation for move `m`.
    ///
    /// Returns the material balance (in centipawns, from the perspective of
    /// the side to move) of playing out the full capture sequence on `m.to()`
    /// with both sides always recapturing with their least valuable attacker.
    /// Positive means the side to move comes out ahead materially; negative
    /// means the exchange loses material.
    ///
    /// This is the classic "swap algorithm": pin/legality of intermediate
    /// captures is *not* checked (a king "capturing" is allowed to appear in
    /// the swap list, valued high so it is only ever used last). The *first*
    /// move's promotion bonus is modeled (gain includes promo − pawn, and the
    /// piece left on the target is the promoted type); promotions during later
    /// recaptures in the swap are not.
    pub fn see(&self, m: Move) -> Value {
        let from = m.from();
        let to = m.to();
        let us = self.side_to_move();

        let mut attacker_type = self
            .piece_on(from)
            .piece_type()
            .expect("see: no piece on move's `from` square");

        let mut occ = self.occupancy();
        occ.clear(from);

        if m.is_en_passant() {
            let captured_pawn_sq = Square::from_file_rank(to.file(), from.rank())
                .expect("see: en passant capture square on board");
            occ.clear(captured_pawn_sq);
        }

        let initial_captured_value = if m.is_en_passant() {
            piece_value(PieceType::Pawn)
        } else {
            match self.piece_on(to).piece_type() {
                Some(pt) => piece_value(pt),
                None => 0,
            }
        };

        let mut gain = [0 as Value; 32];
        gain[0] = initial_captured_value;
        if let Some(promo) = m.promotion_piece() {
            gain[0] += piece_value(promo) - piece_value(PieceType::Pawn);
            attacker_type = promo;
        }
        let mut depth: usize = 0;
        let mut side = !us;

        while depth + 1 < gain.len() {
            let attackers = self.attackers_to_with_occ(to, occ) & occ & self.pieces_color(side);

            let Some((attacker_sq, pt)) = Self::least_valuable(self, attackers) else {
                break;
            };

            depth += 1;
            gain[depth] = piece_value(attacker_type) - gain[depth - 1];

            occ.clear(attacker_sq);
            attacker_type = pt;
            side = !side;
        }

        while depth > 0 {
            gain[depth - 1] = -(-gain[depth - 1]).max(gain[depth]);
            depth -= 1;
        }

        gain[0]
    }

    /// All pieces of either color attacking `sq` given occupancy `occ`
    /// (rather than the board's actual current occupancy), used to reveal
    /// x-ray attackers as pieces are removed during a simulated exchange.
    fn attackers_to_with_occ(&self, sq: Square, occ: Bitboard) -> Bitboard {
        let mut attackers = Bitboard::EMPTY;
        for color in [Color::White, Color::Black] {
            attackers |= lookup::attackers_to(
                sq,
                occ,
                self.pieces(PieceType::Knight) & self.pieces_color(color),
                self.pieces(PieceType::Bishop) & self.pieces_color(color),
                self.pieces(PieceType::Rook) & self.pieces_color(color),
                self.pieces(PieceType::Queen) & self.pieces_color(color),
                self.pieces(PieceType::King) & self.pieces_color(color),
                self.pieces(PieceType::Pawn) & self.pieces_color(color),
                color,
            );
        }
        attackers
    }

    /// Least valuable piece (and its square) among `attackers`, if any.
    fn least_valuable(&self, attackers: Bitboard) -> Option<(Square, PieceType)> {
        for &pt in &ATTACKER_ORDER {
            let bb = attackers & self.pieces(pt);
            if let Some(sq) = bb.lsb() {
                return Some((sq, pt));
            }
        }
        None
    }
}
