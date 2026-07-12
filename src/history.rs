//! Quiet butterfly history tables (P3-03).
//!
//! Indexed by `[Color][from][to]`. Continuation / capture history lands in P3-04.

use crate::types::{Color, Move};

/// Soft clamp for history entries (Stockfish-family gravity scale).
pub const HISTORY_MAX: i32 = 16384;

/// Butterfly history: side × from-square × to-square.
#[derive(Clone, Debug)]
pub struct HistoryTables {
    /// `[color][from][to]`
    butterfly: [[[i16; 64]; 64]; Color::COUNT],
}

impl Default for HistoryTables {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryTables {
    pub fn new() -> Self {
        Self {
            butterfly: [[[0; 64]; 64]; Color::COUNT],
        }
    }

    pub fn clear(&mut self) {
        self.butterfly = [[[0; 64]; 64]; Color::COUNT];
    }

    #[inline]
    pub fn get(&self, color: Color, mv: Move) -> i32 {
        self.butterfly[color.index()][mv.from().index() as usize][mv.to().index() as usize] as i32
    }

    /// Gravity update: `entry += bonus - entry * |bonus| / HISTORY_MAX`.
    #[inline]
    pub fn update(&mut self, color: Color, mv: Move, bonus: i32) {
        let bonus = bonus.clamp(-HISTORY_MAX, HISTORY_MAX);
        let entry =
            &mut self.butterfly[color.index()][mv.from().index() as usize][mv.to().index() as usize];
        let cur = *entry as i32;
        let next = cur + bonus - cur * bonus.abs() / HISTORY_MAX;
        *entry = next.clamp(-HISTORY_MAX, HISTORY_MAX) as i16;
    }
}

/// Depth-squared history bonus (clamped).
#[inline]
pub fn history_bonus(depth: i32) -> i32 {
    let d = depth.max(0);
    (d * d).min(HISTORY_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    fn e2e4() -> Move {
        Move::new(
            Square::from_index_unchecked(12), // e2
            Square::from_index_unchecked(28), // e4
        )
    }

    fn d2d4() -> Move {
        Move::new(
            Square::from_index_unchecked(11), // d2
            Square::from_index_unchecked(27), // d4
        )
    }

    #[test]
    fn update_raises_score() {
        let mut h = HistoryTables::new();
        assert_eq!(h.get(Color::White, e2e4()), 0);
        h.update(Color::White, e2e4(), history_bonus(4));
        assert!(h.get(Color::White, e2e4()) > 0);
        assert_eq!(h.get(Color::White, d2d4()), 0);
        assert_eq!(h.get(Color::Black, e2e4()), 0);
    }

    #[test]
    fn repeated_updates_stay_clamped() {
        let mut h = HistoryTables::new();
        for _ in 0..100 {
            h.update(Color::White, e2e4(), HISTORY_MAX);
        }
        let v = h.get(Color::White, e2e4());
        assert!(v <= HISTORY_MAX);
        assert!(v > 0);
    }

    #[test]
    fn malus_lowers_score() {
        let mut h = HistoryTables::new();
        h.update(Color::White, e2e4(), history_bonus(6));
        let before = h.get(Color::White, e2e4());
        h.update(Color::White, e2e4(), -history_bonus(4));
        assert!(h.get(Color::White, e2e4()) < before);
    }

    #[test]
    fn updated_quiet_ranks_above_untouched() {
        let mut h = HistoryTables::new();
        h.update(Color::White, e2e4(), history_bonus(5));
        assert!(h.get(Color::White, e2e4()) > h.get(Color::White, d2d4()));
    }
}
