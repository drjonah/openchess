//! Scored move list and staged MovePicker (P3).

use crate::board::Board;
use crate::history::HistoryTables;
use crate::types::score::piece_value;
use crate::types::{Color, Move, PieceType, Value};

/// Killer bonuses — above butterfly history range so killers sort first among quiets.
pub const KILLER0_BONUS: i32 = 9000;
pub const KILLER1_BONUS: i32 = 8000;

/// Scored move list with pick-best selection (no full sort required).
#[derive(Clone, Debug, Default)]
pub struct MoveList {
    moves: Vec<(Move, i32)>,
}

impl MoveList {
    pub fn new() -> Self {
        Self { moves: Vec::new() }
    }

    pub fn with_capacity(cap: usize) -> Self {
        Self {
            moves: Vec::with_capacity(cap),
        }
    }

    pub fn clear(&mut self) {
        self.moves.clear();
    }

    pub fn len(&self) -> usize {
        self.moves.len()
    }

    pub fn is_empty(&self) -> bool {
        self.moves.is_empty()
    }

    pub fn push(&mut self, mv: Move, score: i32) {
        self.moves.push((mv, score));
    }

    /// Remove and return the highest-scored remaining move.
    pub fn pick_best(&mut self) -> Option<Move> {
        if self.moves.is_empty() {
            return None;
        }
        let mut best_i = 0;
        let mut best_s = self.moves[0].1;
        for i in 1..self.moves.len() {
            if self.moves[i].1 > best_s {
                best_s = self.moves[i].1;
                best_i = i;
            }
        }
        Some(self.moves.swap_remove(best_i).0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (Move, i32)> + '_ {
        self.moves.iter().copied()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    TtMove,
    GoodNoisy,
    Quiets,
    BadNoisy,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerKind {
    Main,
    Qsearch,
    Evasion,
}

/// Staged move picker: TT → good noisy (SEE≥0) → quiets → bad noisy.
pub struct MovePicker {
    tt_move: Move,
    stage: Stage,
    good: MoveList,
    quiets: MoveList,
    bad: MoveList,
    kind: PickerKind,
}

impl MovePicker {
    /// Main search picker over legal moves.
    pub fn new(
        board: &Board,
        tt_move: Option<Move>,
        history: &HistoryTables,
        killers: &[Move; 2],
    ) -> Self {
        Self::build(
            board,
            tt_move.unwrap_or(Move::NONE),
            PickerKind::Main,
            Some((history, killers)),
        )
    }

    /// Quiescence picker: captures / promotions only, SEE-ordered.
    pub fn qsearch(board: &Board, tt_move: Option<Move>) -> Self {
        Self::build(
            board,
            tt_move.unwrap_or(Move::NONE),
            PickerKind::Qsearch,
            None,
        )
    }

    /// Check-evasion picker: legal evasions staged like main search.
    pub fn evasion(
        board: &Board,
        tt_move: Option<Move>,
        history: &HistoryTables,
        killers: &[Move; 2],
    ) -> Self {
        debug_assert!(
            board.in_check(),
            "MovePicker::evasion called outside of check"
        );
        Self::build(
            board,
            tt_move.unwrap_or(Move::NONE),
            PickerKind::Evasion,
            Some((history, killers)),
        )
    }

    fn build(
        board: &Board,
        tt_move: Move,
        kind: PickerKind,
        quiet_ctx: Option<(&HistoryTables, &[Move; 2])>,
    ) -> Self {
        let mut good = MoveList::with_capacity(32);
        let mut quiets = MoveList::with_capacity(48);
        let mut bad = MoveList::with_capacity(16);

        let mut tt_ok = false;
        let stm = board.side_to_move();

        match kind {
            PickerKind::Qsearch => {
                let mut caps = Vec::new();
                board.generate_captures(&mut caps);
                for mv in caps {
                    if note_tt(&mut tt_ok, tt_move, mv) {
                        continue;
                    }
                    push_noisy(board, mv, &mut good, &mut bad);
                }
            }
            PickerKind::Main => {
                let (history, killers) = quiet_ctx.expect("main picker needs history/killers");
                let mut caps = Vec::new();
                let mut qs = Vec::new();
                board.generate_captures(&mut caps);
                board.generate_quiets(&mut qs);

                for mv in caps {
                    if note_tt(&mut tt_ok, tt_move, mv) {
                        continue;
                    }
                    push_noisy(board, mv, &mut good, &mut bad);
                }
                for mv in qs {
                    if note_tt(&mut tt_ok, tt_move, mv) {
                        continue;
                    }
                    quiets.push(mv, quiet_score(stm, mv, history, killers));
                }
            }
            PickerKind::Evasion => {
                let (history, killers) = quiet_ctx.expect("evasion picker needs history/killers");
                let mut evasions = Vec::new();
                board.generate_evasions(&mut evasions);
                for mv in evasions {
                    if note_tt(&mut tt_ok, tt_move, mv) {
                        continue;
                    }
                    if is_noisy(board, mv) {
                        push_noisy(board, mv, &mut good, &mut bad);
                    } else {
                        quiets.push(mv, quiet_score(stm, mv, history, killers));
                    }
                }
            }
        }

        let stage = if tt_ok {
            Stage::TtMove
        } else {
            Stage::GoodNoisy
        };

        Self {
            tt_move: if tt_ok { tt_move } else { Move::NONE },
            stage,
            good,
            quiets,
            bad,
            kind,
        }
    }

    /// Next move in stage order, or `None` when exhausted.
    pub fn next(&mut self) -> Option<Move> {
        loop {
            match self.stage {
                Stage::TtMove => {
                    self.stage = Stage::GoodNoisy;
                    if !self.tt_move.is_none() {
                        return Some(self.tt_move);
                    }
                }
                Stage::GoodNoisy => {
                    if let Some(mv) = self.good.pick_best() {
                        return Some(mv);
                    }
                    self.stage = if self.kind == PickerKind::Qsearch {
                        Stage::BadNoisy
                    } else {
                        Stage::Quiets
                    };
                }
                Stage::Quiets => {
                    if let Some(mv) = self.quiets.pick_best() {
                        return Some(mv);
                    }
                    self.stage = Stage::BadNoisy;
                }
                Stage::BadNoisy => {
                    if let Some(mv) = self.bad.pick_best() {
                        return Some(mv);
                    }
                    self.stage = Stage::Done;
                }
                Stage::Done => return None,
            }
        }
    }
}

fn note_tt(tt_ok: &mut bool, tt_move: Move, mv: Move) -> bool {
    if !tt_move.is_none() && mv == tt_move {
        *tt_ok = true;
        true
    } else {
        false
    }
}

/// Capture, en passant, or promotion.
pub fn is_noisy(board: &Board, mv: Move) -> bool {
    mv.is_en_passant() || mv.is_promotion() || !board.piece_on(mv.to()).is_empty()
}

/// Quiet = not noisy (castling counts as quiet).
#[inline]
pub fn is_quiet(board: &Board, mv: Move) -> bool {
    !is_noisy(board, mv)
}

fn quiet_score(stm: Color, mv: Move, history: &HistoryTables, killers: &[Move; 2]) -> i32 {
    if mv == killers[0] {
        KILLER0_BONUS
    } else if mv == killers[1] {
        KILLER1_BONUS
    } else {
        history.get(stm, mv)
    }
}

fn push_noisy(board: &Board, mv: Move, good: &mut MoveList, bad: &mut MoveList) {
    let see = board.see(mv);
    let score = mvv_lva(board, mv) + see;
    if see >= 0 {
        good.push(mv, score);
    } else {
        bad.push(mv, score);
    }
}

/// MVV-LVA style score: victim value * 16 - attacker value.
fn mvv_lva(board: &Board, mv: Move) -> i32 {
    let victim = if mv.is_en_passant() {
        piece_value(PieceType::Pawn)
    } else {
        board
            .piece_on(mv.to())
            .piece_type()
            .map(piece_value)
            .unwrap_or(0)
    };
    let attacker = board
        .piece_on(mv.from())
        .piece_type()
        .map(piece_value)
        .unwrap_or(0);
    let promo_bonus: Value = mv
        .promotion_piece()
        .map(|pt| piece_value(pt) - piece_value(PieceType::Pawn))
        .unwrap_or(0);
    (victim + promo_bonus) * 16 - attacker
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::history_bonus;
    use crate::lookup;
    use crate::types::{Piece, Square};
    use std::str::FromStr;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn pick_best_returns_highest_first() {
        let mut list = MoveList::new();
        let a = Move::new(
            Square::from_index_unchecked(0),
            Square::from_index_unchecked(1),
        );
        let b = Move::new(
            Square::from_index_unchecked(2),
            Square::from_index_unchecked(3),
        );
        let c = Move::new(
            Square::from_index_unchecked(4),
            Square::from_index_unchecked(5),
        );
        list.push(a, 10);
        list.push(b, 50);
        list.push(c, 20);
        assert_eq!(list.pick_best(), Some(b));
        assert_eq!(list.pick_best(), Some(c));
        assert_eq!(list.pick_best(), Some(a));
        assert_eq!(list.pick_best(), None);
    }

    #[test]
    fn good_captures_before_losing() {
        init();
        // Queen on c4: take hanging a4 (good) or e4 defended by d5 (SEE-losing).
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("e8").unwrap());
        board.put_piece(Piece::WhiteQueen, Square::from_str("c4").unwrap());
        board.put_piece(Piece::BlackPawn, Square::from_str("a4").unwrap());
        board.put_piece(Piece::BlackPawn, Square::from_str("e4").unwrap());
        board.put_piece(Piece::BlackPawn, Square::from_str("d5").unwrap());
        board.set_side_to_move(Color::White);
        board.rehash();
        board.refresh_checkers_and_pins();

        let take_hanging = Move::new(
            Square::from_str("c4").unwrap(),
            Square::from_str("a4").unwrap(),
        );
        let take_defended = Move::new(
            Square::from_str("c4").unwrap(),
            Square::from_str("e4").unwrap(),
        );
        assert!(board.see(take_hanging) >= 0);
        assert!(board.see(take_defended) < 0);

        let history = HistoryTables::new();
        let killers = [Move::NONE; 2];
        let mut picker = MovePicker::new(&board, None, &history, &killers);
        let mut order = Vec::new();
        while let Some(mv) = picker.next() {
            order.push(mv);
        }
        let hi = order.iter().position(|m| *m == take_hanging);
        let di = order.iter().position(|m| *m == take_defended);
        assert!(
            hi.is_some() && di.is_some() && hi.unwrap() < di.unwrap(),
            "good capture before bad: {order:?}"
        );
    }

    #[test]
    fn tt_move_comes_first() {
        init();
        let board = Board::startpos();
        let e2e4 = Move::new(
            Square::from_str("e2").unwrap(),
            Square::from_str("e4").unwrap(),
        );
        let history = HistoryTables::new();
        let killers = [Move::NONE; 2];
        let mut picker = MovePicker::new(&board, Some(e2e4), &history, &killers);
        assert_eq!(picker.next(), Some(e2e4));
    }

    #[test]
    fn evasion_capture_before_quiet_king_move() {
        init();
        // White king checked by black rook on e2; queen can capture it, or king can flee.
        let mut board = Board::empty();
        board.put_piece(Piece::WhiteKing, Square::from_str("e1").unwrap());
        board.put_piece(Piece::WhiteQueen, Square::from_str("d1").unwrap());
        board.put_piece(Piece::BlackKing, Square::from_str("a8").unwrap());
        board.put_piece(Piece::BlackRook, Square::from_str("e2").unwrap());
        board.set_side_to_move(Color::White);
        board.rehash();
        board.refresh_checkers_and_pins();
        assert!(board.in_check());

        let capture_rook = Move::new(
            Square::from_str("d1").unwrap(),
            Square::from_str("e2").unwrap(),
        );
        let king_flee = Move::new(
            Square::from_str("e1").unwrap(),
            Square::from_str("f1").unwrap(),
        );

        let history = HistoryTables::new();
        let killers = [Move::NONE; 2];
        let mut picker = MovePicker::evasion(&board, None, &history, &killers);
        let mut order = Vec::new();
        while let Some(mv) = picker.next() {
            order.push(mv);
        }
        let ci = order.iter().position(|m| *m == capture_rook);
        let ki = order.iter().position(|m| *m == king_flee);
        assert!(
            ci.is_some() && ki.is_some() && ci.unwrap() < ki.unwrap(),
            "capture evasion before quiet king move: {order:?}"
        );
    }

    #[test]
    fn history_orders_quiets() {
        init();
        let board = Board::startpos();
        let e2e4 = Move::new(
            Square::from_str("e2").unwrap(),
            Square::from_str("e4").unwrap(),
        );
        let d2d4 = Move::new(
            Square::from_str("d2").unwrap(),
            Square::from_str("d4").unwrap(),
        );

        let mut history = HistoryTables::new();
        history.update(Color::White, e2e4, history_bonus(5));
        let killers = [Move::NONE; 2];

        let mut picker = MovePicker::new(&board, None, &history, &killers);
        // Skip captures (none at startpos) — first quiet should be e2e4.
        let first_quiet = picker.next();
        assert_eq!(first_quiet, Some(e2e4));

        // d2d4 should appear later than e2e4.
        let mut found_d2d4 = false;
        while let Some(mv) = picker.next() {
            if mv == d2d4 {
                found_d2d4 = true;
                break;
            }
        }
        assert!(found_d2d4, "d2d4 should still be generated");
    }

    #[test]
    fn killer_ranks_above_history_quiet() {
        init();
        let board = Board::startpos();
        let e2e4 = Move::new(
            Square::from_str("e2").unwrap(),
            Square::from_str("e4").unwrap(),
        );
        let d2d4 = Move::new(
            Square::from_str("d2").unwrap(),
            Square::from_str("d4").unwrap(),
        );

        let mut history = HistoryTables::new();
        // Give d2d4 a strong history score, but e2e4 is killer 0.
        history.update(Color::White, d2d4, history_bonus(8));
        let killers = [e2e4, Move::NONE];

        let mut picker = MovePicker::new(&board, None, &history, &killers);
        assert_eq!(picker.next(), Some(e2e4));
    }
}
