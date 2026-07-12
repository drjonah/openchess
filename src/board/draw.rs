//! Draw and game-result detection: threefold repetition, 50-move rule,
//! insufficient material, checkmate, and stalemate.

use super::Board;
use crate::types::{Color, PieceType};

/// How a finished game ended (or [`GameResult::Ongoing`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameResult {
    Ongoing,
    Checkmate { winner: Color },
    Stalemate,
    DrawRepetition,
    DrawFiftyMove,
    DrawInsufficientMaterial,
}

impl GameResult {
    pub const fn is_draw(self) -> bool {
        matches!(
            self,
            Self::Stalemate
                | Self::DrawRepetition
                | Self::DrawFiftyMove
                | Self::DrawInsufficientMaterial
        )
    }

    pub const fn is_over(self) -> bool {
        !matches!(self, Self::Ongoing)
    }

    /// Short status string for the TUI / logs.
    pub fn status_message(self) -> &'static str {
        match self {
            Self::Ongoing => "Game in progress",
            Self::Checkmate {
                winner: Color::White,
            } => "Checkmate — White wins",
            Self::Checkmate {
                winner: Color::Black,
            } => "Checkmate — Black wins",
            Self::Stalemate => "Draw by stalemate",
            Self::DrawRepetition => "Draw by threefold repetition",
            Self::DrawFiftyMove => "Draw by 50-move rule",
            Self::DrawInsufficientMaterial => "Draw by insufficient material",
        }
    }
}

impl Board {
    /// Distance in plies to the most recent prior occurrence of this position's
    /// Zobrist key, looking back at most [`Self::halfmove_clock`] entries.
    ///
    /// `None` if the position has not occurred before in the irreversible window.
    pub fn previous_repetition_distance(&self) -> Option<usize> {
        let key = self.key;
        let end = (self.halfmove_clock as usize).min(self.history.len());
        for (i, state) in self.history.iter().rev().take(end).enumerate() {
            if state.key == key {
                return Some(i + 1);
            }
        }
        None
    }

    /// True when the current position has already occurred twice before
    /// (i.e. this is the third occurrence → threefold repetition).
    pub fn is_threefold_repetition(&self) -> bool {
        let key = self.key;
        let end = (self.halfmove_clock as usize).min(self.history.len());
        let mut count = 1; // current position
        for state in self.history.iter().rev().take(end) {
            if state.key == key {
                count += 1;
                if count >= 3 {
                    return true;
                }
            }
        }
        false
    }

    /// 50-move rule: 100 half-moves without capture or pawn move.
    ///
    /// If the side to move is in check with no legal escapes, that is checkmate,
    /// not a draw — matching FIDE / Stockfish.
    pub fn is_fifty_move_draw(&self) -> bool {
        if self.halfmove_clock < 100 {
            return false;
        }
        if !self.in_check() {
            return true;
        }
        !self.legal_moves().is_empty()
    }

    /// Dead position: neither side can possibly checkmate.
    ///
    /// Covers K vs K, K+minor vs K, and K+B vs K+B with bishops on the same color.
    pub fn is_insufficient_material(&self) -> bool {
        if self.pieces(PieceType::Pawn).any()
            || self.pieces(PieceType::Rook).any()
            || self.pieces(PieceType::Queen).any()
        {
            return false;
        }

        let knights = self.pieces(PieceType::Knight).count();
        let bishops = self.pieces(PieceType::Bishop).count();
        let minors = knights + bishops;

        if minors == 0 {
            return true; // K vs K
        }
        if minors == 1 {
            return true; // K+N vs K or K+B vs K
        }
        // K+B vs K+B, same square color → draw; opposite colors can mate.
        if knights == 0 && bishops == 2 {
            let wb = self.pieces(PieceType::Bishop) & self.pieces_color(Color::White);
            let bb = self.pieces(PieceType::Bishop) & self.pieces_color(Color::Black);
            if wb.count() == 1 && bb.count() == 1 {
                let wsq = wb.lsb().expect("white bishop");
                let bsq = bb.lsb().expect("black bishop");
                let light = |sq: crate::types::Square| (sq.file() + sq.rank()) % 2 == 0;
                return light(wsq) == light(bsq);
            }
        }
        false
    }

    /// Claimable / automatic draw independent of mate (repetition, 50-move, material).
    pub fn is_draw_by_rule(&self) -> bool {
        self.is_threefold_repetition()
            || self.is_fifty_move_draw()
            || self.is_insufficient_material()
    }

    /// Search-time draw probe (Stockfish-style).
    ///
    /// Returns true when:
    /// - the 50-move rule applies, or
    /// - the position repeated once strictly after the root (`distance < ply`), or
    /// - the position repeated twice before/at the root (threefold with current).
    ///
    /// Call with `ply == 0` at the root: only real threefold / 50-move score as draws.
    pub fn is_draw(&self, ply: usize) -> bool {
        if self.is_fifty_move_draw() {
            return true;
        }

        let key = self.key;
        let end = (self.halfmove_clock as usize).min(self.history.len());
        let mut occurrences_before = 0usize;
        for (i, state) in self.history.iter().rev().take(end).enumerate() {
            if state.key != key {
                continue;
            }
            let distance = i + 1;
            occurrences_before += 1;
            // First prior occurrence strictly inside the search tree → draw.
            if occurrences_before == 1 && distance < ply {
                return true;
            }
            // Second prior occurrence → current is the third (threefold).
            if occurrences_before >= 2 {
                return true;
            }
        }
        false
    }

    /// Full game result for the current position (mate, stalemate, draws, or ongoing).
    pub fn game_result(&self) -> GameResult {
        if self.is_threefold_repetition() {
            return GameResult::DrawRepetition;
        }
        if self.is_insufficient_material() {
            return GameResult::DrawInsufficientMaterial;
        }
        if self.is_fifty_move_draw() {
            return GameResult::DrawFiftyMove;
        }

        let legal = self.legal_moves();
        if legal.is_empty() {
            return if self.in_check() {
                GameResult::Checkmate {
                    winner: !self.side_to_move(),
                }
            } else {
                GameResult::Stalemate
            };
        }
        GameResult::Ongoing
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    fn init() {
        lookup::initialize();
    }

    fn play(board: &mut Board, uci: &str) {
        let mv = board.parse_uci_move(uci).expect(uci);
        board.make(mv);
    }

    #[test]
    fn startpos_is_ongoing() {
        init();
        let board = Board::startpos();
        assert_eq!(board.game_result(), GameResult::Ongoing);
        assert!(!board.is_draw(0));
    }

    #[test]
    fn kings_only_is_insufficient_material() {
        init();
        let board = Board::from_fen("8/8/8/8/8/8/8/k3K3 w - - 0 1").unwrap();
        assert!(board.is_insufficient_material());
        assert_eq!(board.game_result(), GameResult::DrawInsufficientMaterial);
    }

    #[test]
    fn threefold_repetition_after_returning_twice() {
        init();
        // Knights shuffle: each full cycle returns to startpos.
        let mut board = Board::startpos();
        let cycle = ["b1c3", "b8c6", "c3b1", "c6b8"];
        // Startpos occurs once. After first cycle → 2nd. After second → 3rd.
        for _ in 0..2 {
            for mv in cycle {
                play(&mut board, mv);
            }
        }
        assert!(
            board.is_threefold_repetition(),
            "startpos should have occurred three times"
        );
        assert_eq!(board.game_result(), GameResult::DrawRepetition);
        assert!(board.is_draw(0));
    }

    #[test]
    fn search_scores_twofold_inside_tree_as_draw() {
        init();
        let mut board = Board::startpos();
        for mv in ["b1c3", "b8c6", "c3b1", "c6b8"] {
            play(&mut board, mv);
        }
        // Back at startpos once (2nd occurrence). At root ply=0 this is NOT yet a draw.
        assert!(!board.is_threefold_repetition());
        assert!(!board.is_draw(0));
        // Distance to prior startpos is 4; at ply 5, distance < ply → draw.
        assert!(board.is_draw(5));
    }

    #[test]
    fn fifty_move_draw_from_fen_clock() {
        init();
        let board = Board::from_fen("8/8/8/8/8/8/4Q3/k3K3 w - - 100 50").unwrap();
        assert!(board.is_fifty_move_draw());
        assert_eq!(board.game_result(), GameResult::DrawFiftyMove);
    }

    #[test]
    fn checkmate_beats_high_halfmove_clock() {
        init();
        let board =
            Board::from_fen("rnb1kbnr/pppp1ppp/8/4p3/6Pq/5P2/PPPPP2P/RNBQKBNR w KQkq - 100 4")
                .unwrap();
        assert!(board.in_check());
        assert!(board.legal_moves().is_empty());
        assert!(!board.is_fifty_move_draw());
        assert_eq!(
            board.game_result(),
            GameResult::Checkmate {
                winner: Color::Black
            }
        );
    }

    #[test]
    fn unmake_restores_repetition_state() {
        init();
        let mut board = Board::startpos();
        let cycle = ["b1c3", "b8c6", "c3b1", "c6b8"];
        let mut stack = Vec::new();
        for _ in 0..2 {
            for uci in cycle {
                let m = board.parse_uci_move(uci).unwrap();
                board.make(m);
                stack.push(m);
            }
        }
        assert!(board.is_threefold_repetition());
        let m = stack.pop().unwrap();
        board.unmake(m);
        assert!(!board.is_threefold_repetition());
    }

    #[test]
    fn same_color_bishops_insufficient() {
        init();
        // Both bishops on dark squares (c1 and a3).
        let board = Board::from_fen("8/8/8/8/8/b7/8/k1B1K3 w - - 0 1").unwrap();
        assert!(board.is_insufficient_material());
    }
}
