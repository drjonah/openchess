//! Dual White/Black HalfKA accumulators with incremental BoardObserver updates.
//!
//! Piece deltas update both perspectives when possible. Moving a perspective's
//! own king marks that perspective dirty; [`BoardObserver::on_make`] /
//! [`BoardObserver::on_unmake`] then rebuild it from the consistent board.
//! Null moves change no pieces — leave the accumulator untouched.

use super::features::{feature_index, L1_SIZE};
use super::network::Network;
use crate::board::{Board, BoardObserver};
use crate::types::{Color, Move, Piece, PieceType, Square};
use std::sync::Arc;

/// Dual-perspective feature-transformer state.
#[derive(Clone, Debug)]
pub struct Accumulator {
    /// `values[Color::index()]` — White / Black perspective activations.
    pub values: [[i16; L1_SIZE]; Color::COUNT],
    /// King squares last used for each perspective (after last refresh/update).
    pub kings: [Square; Color::COUNT],
}

impl Default for Accumulator {
    fn default() -> Self {
        Self {
            values: [[0; L1_SIZE]; Color::COUNT],
            kings: [Square::E1, Square::E8],
        }
    }
}

impl Accumulator {
    /// Rebuild both perspectives from `board` using `net` FT weights.
    pub fn refresh(&mut self, board: &Board, net: &Network) {
        for &color in &[Color::White, Color::Black] {
            self.refresh_perspective(board, color, net);
        }
    }

    /// Rebuild one perspective from scratch (bias + all active features).
    pub fn refresh_perspective(&mut self, board: &Board, perspective: Color, net: &Network) {
        debug_assert_eq!(net.l1, L1_SIZE);
        let king = board.king_sq(perspective);
        self.kings[perspective.index()] = king;
        let acc = &mut self.values[perspective.index()];
        for i in 0..L1_SIZE {
            acc[i] = net.ft_bias[i];
        }

        for sq in board.occupancy().squares() {
            let piece = board.piece_on(sq);
            if let Some(feat) = feature_index(perspective, king, piece, sq) {
                apply_feature(acc, net, feat, 1);
            }
        }
    }

    #[inline]
    pub fn perspective(&self, color: Color) -> &[i16; L1_SIZE] {
        &self.values[color.index()]
    }
}

/// Search-thread NNUE state: live accumulator + dirty flags + FT weights.
#[derive(Clone, Debug)]
pub struct NnueState {
    pub accumulator: Accumulator,
    /// Perspectives that must full-refresh at the end of the current make/unmake.
    refresh: [bool; Color::COUNT],
    /// Shared network (FT columns for incremental updates).
    net: Arc<Network>,
}

impl Default for NnueState {
    fn default() -> Self {
        Self::new(Network::embedded_shared())
    }
}

impl NnueState {
    pub fn new(net: Arc<Network>) -> Self {
        Self {
            accumulator: Accumulator::default(),
            refresh: [false; Color::COUNT],
            net,
        }
    }

    #[inline]
    pub fn network(&self) -> &Network {
        &self.net
    }

    #[inline]
    pub fn network_arc(&self) -> Arc<Network> {
        Arc::clone(&self.net)
    }

    /// Full dual refresh; clears dirty flags. Call at search root.
    pub fn refresh(&mut self, board: &Board) {
        self.accumulator.refresh(board, &self.net);
        self.refresh = [false; Color::COUNT];
    }

    #[inline]
    fn add_piece(&mut self, perspective: Color, piece: Piece, sq: Square) {
        if self.refresh[perspective.index()] {
            return;
        }
        let king = self.accumulator.kings[perspective.index()];
        if let Some(feat) = feature_index(perspective, king, piece, sq) {
            apply_feature(
                &mut self.accumulator.values[perspective.index()],
                &self.net,
                feat,
                1,
            );
        }
    }

    #[inline]
    fn remove_piece(&mut self, perspective: Color, piece: Piece, sq: Square) {
        if self.refresh[perspective.index()] {
            return;
        }
        let king = self.accumulator.kings[perspective.index()];
        if let Some(feat) = feature_index(perspective, king, piece, sq) {
            apply_feature(
                &mut self.accumulator.values[perspective.index()],
                &self.net,
                feat,
                -1,
            );
        }
    }

    fn finish_update(&mut self, board: &Board) {
        for &color in &[Color::White, Color::Black] {
            if self.refresh[color.index()] {
                self.accumulator
                    .refresh_perspective(board, color, &self.net);
                self.refresh[color.index()] = false;
            } else {
                self.accumulator.kings[color.index()] = board.king_sq(color);
            }
        }
    }
}

impl BoardObserver for NnueState {
    fn on_make(&mut self, board: &Board, _m: Move) {
        self.finish_update(board);
    }

    fn on_unmake(&mut self, board: &Board, _m: Move) {
        self.finish_update(board);
    }

    fn on_add(&mut self, piece: Piece, sq: Square) {
        if piece.is_empty() {
            return;
        }
        if piece.piece_type() == Some(PieceType::King) {
            let color = piece.color().unwrap();
            self.refresh[color.index()] = true;
            self.add_piece(!color, piece, sq);
        } else {
            self.add_piece(Color::White, piece, sq);
            self.add_piece(Color::Black, piece, sq);
        }
    }

    fn on_remove(&mut self, piece: Piece, sq: Square) {
        if piece.is_empty() {
            return;
        }
        if piece.piece_type() == Some(PieceType::King) {
            let color = piece.color().unwrap();
            self.refresh[color.index()] = true;
            self.remove_piece(!color, piece, sq);
        } else {
            self.remove_piece(Color::White, piece, sq);
            self.remove_piece(Color::Black, piece, sq);
        }
    }
}

#[inline]
fn apply_feature(acc: &mut [i16; L1_SIZE], net: &Network, feature: usize, sign: i16) {
    let col = net.ft_column(feature);
    for i in 0..L1_SIZE {
        acc[i] = acc[i].wrapping_add(col[i].wrapping_mul(sign));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::moves::flags;
    use std::str::FromStr;

    fn init() {
        crate::lookup::initialize();
    }

    fn sq(name: &str) -> Square {
        Square::from_str(name).unwrap()
    }

    fn assert_acc_eq(a: &Accumulator, b: &Accumulator) {
        assert_eq!(a.kings, b.kings, "king squares differ");
        assert_eq!(a.values[0], b.values[0], "white perspective mismatch");
        assert_eq!(a.values[1], b.values[1], "black perspective mismatch");
    }

    fn refresh_copy(board: &Board, net: &Network) -> Accumulator {
        let mut acc = Accumulator::default();
        acc.refresh(board, net);
        acc
    }

    fn fresh_state() -> NnueState {
        NnueState::new(Network::embedded_shared())
    }

    #[test]
    fn quiet_move_incremental_matches_refresh() {
        init();
        let mut board = Board::startpos();
        let mut nnue = fresh_state();
        nnue.refresh(&board);

        let m = Move::new(sq("e2"), sq("e4"));
        board.make_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));

        board.unmake_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
    }

    #[test]
    fn capture_incremental_matches_refresh() {
        init();
        let mut board = Board::from_fen("4k3/8/8/3p4/4P3/8/8/4K3 w - - 0 1").unwrap();
        let mut nnue = fresh_state();
        nnue.refresh(&board);

        let m = Move::new(sq("e4"), sq("d5"));
        board.make_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.unmake_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
    }

    #[test]
    fn king_move_triggers_refresh_and_matches() {
        init();
        let mut board = Board::from_fen("4k3/8/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        let mut nnue = fresh_state();
        nnue.refresh(&board);

        let m = Move::new(sq("e1"), sq("e2"));
        board.make_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.unmake_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
    }

    #[test]
    fn castling_incremental_matches_refresh() {
        init();
        let mut board = Board::from_fen("4k3/8/8/8/8/8/8/R3K2R w KQ - 0 1").unwrap();
        let mut nnue = fresh_state();
        nnue.refresh(&board);

        let m = Move::castling(Square::E1, Square::G1);
        assert_eq!(m.flags(), flags::CASTLING);
        board.make_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.unmake_observed(m, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
    }

    #[test]
    fn en_passant_and_promotion_match_refresh() {
        init();
        let mut board = Board::from_fen("4k3/8/8/3pP3/8/8/8/4K3 w - d6 0 1").unwrap();
        let mut nnue = fresh_state();
        nnue.refresh(&board);
        let ep = Move::en_passant(sq("e5"), sq("d6"));
        board.make_observed(ep, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.unmake_observed(ep, Some(&mut nnue));

        let mut board = Board::from_fen("4k3/P7/8/8/8/8/8/4K3 w - - 0 1").unwrap();
        nnue.refresh(&board);
        let promo = Move::promotion(sq("a7"), sq("a8"), PieceType::Queen);
        board.make_observed(promo, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.unmake_observed(promo, Some(&mut nnue));
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
    }

    #[test]
    fn null_move_leaves_accumulator_unchanged() {
        init();
        let mut board = Board::startpos();
        let mut nnue = fresh_state();
        nnue.refresh(&board);
        let before = nnue.accumulator.clone();

        board.do_null();
        assert_acc_eq(&nnue.accumulator, &before);
        assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        board.undo_null();
        assert_acc_eq(&nnue.accumulator, &before);
    }

    #[test]
    fn random_walk_incremental_equals_refresh() {
        init();
        let mut board = Board::startpos();
        let mut nnue = fresh_state();
        nnue.refresh(&board);
        let mut rng = 0xDEAD_BEEF_u64;
        let mut path = Vec::new();

        for _ in 0..64 {
            assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
            let moves = board.legal_moves();
            if moves.is_empty() {
                break;
            }
            rng = rng.wrapping_mul(6364136223846793005).wrapping_add(1);
            let m = moves[(rng as usize) % moves.len()];
            board.make_observed(m, Some(&mut nnue));
            path.push(m);
        }

        assert!(!path.is_empty());
        for m in path.iter().rev() {
            board.unmake_observed(*m, Some(&mut nnue));
            assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
        }
        assert_eq!(board.to_fen(), Board::startpos().to_fen());
    }

    #[test]
    fn random_games_incremental_equals_refresh() {
        init();
        let mut rng = 0xC0FFEE_u64;

        for game in 0..4 {
            let mut board = Board::startpos();
            let mut nnue = fresh_state();
            nnue.refresh(&board);

            for _ply in 0..48 {
                assert_acc_eq(&nnue.accumulator, &refresh_copy(&board, nnue.network()));
                let moves = board.legal_moves();
                if moves.is_empty() {
                    break;
                }
                rng = rng
                    .wrapping_mul(6364136223846793005)
                    .wrapping_add(game + 1);
                let m = moves[(rng as usize) % moves.len()];
                board.make_observed(m, Some(&mut nnue));
            }
        }
    }
}
