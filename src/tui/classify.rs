//! Move quality classification beyond raw centipawn-loss tiers.

use super::game::{classify_cpl, MoveClass, PlyAnalysis};
use crate::board::Board;
use crate::types::Move;

/// Centipawn score (mover POV) above which a winning line is considered available.
pub const WINNING_CP: i32 = 300;
/// Mover POV eval before the ply above which Brilliant is suppressed.
pub const ALREADY_WINNING_CP: i32 = 600;
/// Mover POV eval after the ply must stay at or above this for Brilliant.
pub const BRILLIANT_MIN_AFTER_CP: i32 = -100;
/// Minimum CPL for the current ply to qualify as a punishable miss (Inaccuracy+).
pub const MISS_MIN_CPL: u32 = 51;
/// Previous-opponent CPL at or above this counts as a bad mistake for Miss chaining.
pub const OPPONENT_BAD_CPL: u32 = 100;
/// Maximum CPL for Brilliant (Best / Excellent tier).
pub const BRILLIANT_MAX_CPL: u32 = 25;

/// Inputs for [`classify_move`], usually built from cached post-game search data.
pub struct ClassifyInput<'a> {
    pub cpl: u32,
    /// White-relative eval before the ply.
    pub prev_eval: i32,
    /// White-relative eval after the ply.
    pub after_eval: i32,
    pub white_moved: bool,
    pub played: Move,
    /// Engine best move from the position before this ply (prior search step).
    pub best_move: Option<Move>,
    pub board_before: &'a Board,
    /// Previous ply was Mistake/Blunder or had CPL ≥ [`OPPONENT_BAD_CPL`].
    pub opponent_was_bad: bool,
    /// The played move is a known opening-book move for `board_before` (OPEN-01).
    pub in_book: bool,
}

/// Classify a ply: Book → Miss → Brilliant → CPL base.
pub fn classify_move(input: ClassifyInput<'_>) -> MoveClass {
    // Opening theory overrides centipawn-loss tiers (OPEN-01).
    if input.in_book {
        return MoveClass::Book;
    }

    let base = classify_cpl(input.cpl);

    if is_miss(&input, base) {
        return MoveClass::Miss;
    }
    if is_brilliant(&input, base) {
        return MoveClass::Brilliant;
    }
    base
}

/// True when the opponent's immediately preceding ply was bad enough to punish.
pub fn opponent_was_bad(prev: Option<&PlyAnalysis>) -> bool {
    match prev {
        Some(a) => {
            matches!(
                a.classification,
                MoveClass::Mistake | MoveClass::Blunder
            ) || a.cpl >= OPPONENT_BAD_CPL
        }
        None => false,
    }
}

fn mover_eval_before(input: &ClassifyInput<'_>) -> i32 {
    if input.white_moved {
        input.prev_eval
    } else {
        -input.prev_eval
    }
}

fn mover_eval_after(input: &ClassifyInput<'_>) -> i32 {
    if input.white_moved {
        input.after_eval
    } else {
        -input.after_eval
    }
}

fn winning_line_existed(input: &ClassifyInput<'_>) -> bool {
    if mover_eval_before(input) > WINNING_CP {
        return true;
    }
    input.best_move.is_some()
}

fn is_miss(input: &ClassifyInput<'_>, base: MoveClass) -> bool {
    if !input.opponent_was_bad {
        return false;
    }
    if input.cpl < MISS_MIN_CPL {
        return false;
    }
    if !matches!(
        base,
        MoveClass::Inaccuracy | MoveClass::Mistake | MoveClass::Blunder
    ) {
        return false;
    }
    if !winning_line_existed(input) {
        return false;
    }
    let Some(best) = input.best_move else {
        return false;
    };
    input.played != best
}

fn is_brilliant(input: &ClassifyInput<'_>, base: MoveClass) -> bool {
    if input.cpl > BRILLIANT_MAX_CPL {
        return false;
    }
    if !matches!(base, MoveClass::Best | MoveClass::Excellent) {
        return false;
    }
    let Some(best) = input.best_move else {
        return false;
    };
    if input.played != best {
        return false;
    }
    if input.board_before.see(input.played) >= 0 {
        return false;
    }
    if mover_eval_before(input) >= ALREADY_WINNING_CP {
        return false;
    }
    mover_eval_after(input) >= BRILLIANT_MIN_AFTER_CP
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::Board;
    use crate::types::{Color, Move, Piece, Square};
    use std::str::FromStr;

    fn sq(name: &str) -> Square {
        Square::from_str(name).unwrap()
    }

    fn queen_sacrifice_board() -> Board {
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteQueen, sq("d1"));
        board.put_piece(Piece::BlackPawn, sq("e6"));
        board.set_side_to_move(Color::White);
        board
    }

    fn miss_input(
        cpl: u32,
        played: Move,
        best: Move,
        prev_eval: i32,
        after_eval: i32,
        white_moved: bool,
        opponent_was_bad: bool,
    ) -> ClassifyInput<'static> {
        // Leaked board for 'static lifetime in tests — board is only read, never mutated.
        let board = Box::leak(Box::new(Board::startpos()));
        ClassifyInput {
            cpl,
            prev_eval,
            after_eval,
            white_moved,
            played,
            best_move: Some(best),
            board_before: board,
            opponent_was_bad,
            in_book: false,
        }
    }

    #[test]
    fn book_move_is_classified_as_book() {
        let e4 = Move::new(sq("e2"), sq("e4"));
        let mut input = miss_input(0, e4, e4, 0, 0, true, false);
        input.in_book = true;
        assert_eq!(classify_move(input), MoveClass::Book);
    }

    #[test]
    fn miss_after_opponent_blunder_and_high_cpl() {
        let e4 = Move::new(sq("e2"), sq("e4"));
        let d4 = Move::new(sq("d2"), sq("d4"));
        let input = miss_input(80, d4, e4, 350, 200, true, true);
        assert_eq!(classify_move(input), MoveClass::Miss);
    }

    #[test]
    fn miss_not_when_played_best_move() {
        let e4 = Move::new(sq("e2"), sq("e4"));
        let input = miss_input(80, e4, e4, 350, 200, true, true);
        assert_eq!(classify_move(input), MoveClass::Inaccuracy);
    }

    #[test]
    fn miss_not_on_first_ply_without_bad_opponent() {
        let e4 = Move::new(sq("e2"), sq("e4"));
        let d4 = Move::new(sq("d2"), sq("d4"));
        let input = miss_input(80, d4, e4, 350, 200, true, false);
        assert_eq!(classify_move(input), MoveClass::Inaccuracy);
    }

    #[test]
    fn miss_not_when_cpl_too_low() {
        let e4 = Move::new(sq("e2"), sq("e4"));
        let d4 = Move::new(sq("d2"), sq("d4"));
        let input = miss_input(30, d4, e4, 350, 320, true, true);
        assert_eq!(classify_move(input), MoveClass::Good);
    }

    #[test]
    fn brilliant_on_sacrifice_best_move() {
        let board = queen_sacrifice_board();
        let qd5 = Move::new(sq("d1"), sq("d5"));
        let input = ClassifyInput {
            cpl: 5,
            prev_eval: 50,
            after_eval: 80,
            white_moved: true,
            played: qd5,
            best_move: Some(qd5),
            board_before: &board,
            opponent_was_bad: false,
            in_book: false,
        };
        assert_eq!(classify_move(input), MoveClass::Brilliant);
    }

    #[test]
    fn brilliant_not_when_already_winning() {
        let board = queen_sacrifice_board();
        let qd5 = Move::new(sq("d1"), sq("d5"));
        let input = ClassifyInput {
            cpl: 0,
            prev_eval: 700,
            after_eval: 720,
            white_moved: true,
            played: qd5,
            best_move: Some(qd5),
            board_before: &board,
            opponent_was_bad: false,
            in_book: false,
        };
        assert_eq!(classify_move(input), MoveClass::Best);
    }

    #[test]
    fn brilliant_not_without_sacrifice() {
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteQueen, sq("d1"));
        board.set_side_to_move(Color::White);
        let qd5 = Move::new(sq("d1"), sq("d5"));
        let input = ClassifyInput {
            cpl: 0,
            prev_eval: 50,
            after_eval: 80,
            white_moved: true,
            played: qd5,
            best_move: Some(qd5),
            board_before: &board,
            opponent_was_bad: false,
            in_book: false,
        };
        assert_eq!(classify_move(input), MoveClass::Best);
    }

    #[test]
    fn opponent_was_bad_from_classification_or_cpl() {
        let blunder = PlyAnalysis {
            eval_cp: 0,
            classification: MoveClass::Blunder,
            cpl: 50,
            best_move: None,
        };
        assert!(opponent_was_bad(Some(&blunder)));

        let high_cpl = PlyAnalysis {
            eval_cp: 0,
            classification: MoveClass::Good,
            cpl: 100,
            best_move: None,
        };
        assert!(opponent_was_bad(Some(&high_cpl)));

        let fine = PlyAnalysis {
            eval_cp: 0,
            classification: MoveClass::Inaccuracy,
            cpl: 60,
            best_move: None,
        };
        assert!(!opponent_was_bad(Some(&fine)));
    }
}
