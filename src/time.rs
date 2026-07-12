//! Soft/hard time management (P7-02).
//!
//! Soft bound ends iterative deepening between iterations without aborting the
//! current search. Hard bound sets the stop flag so the search aborts promptly.

use crate::search::Limits;
use crate::types::Color;
use std::time::Duration;

/// Default Move Overhead in milliseconds.
///
/// Matches [`crate::config::EngineConfig`] (`50`), not Stockfish's typical `10`.
/// Prefer the project config default so UCI and TUI stay consistent; UCI also
/// exposes a `Move Overhead` option (P7-03).
pub const DEFAULT_MOVE_OVERHEAD_MS: u64 = 50;

/// Default Move Overhead as a [`Duration`].
pub const DEFAULT_MOVE_OVERHEAD: Duration = Duration::from_millis(DEFAULT_MOVE_OVERHEAD_MS);

/// Soft and hard think-time bounds for one `go`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeBudget {
    /// Prefer stopping after this elapsed time (between ID iterations).
    pub soft: Duration,
    /// Must abort by this elapsed time (sets stop).
    pub hard: Duration,
}

impl TimeBudget {
    /// Derive soft/hard bounds from UCI limits for `stm`.
    ///
    /// Returns `None` when there is no timed limit (`infinite`, depth/nodes only).
    ///
    /// Formulas:
    /// - **movetime:** soft = movetime; hard = movetime − overhead
    /// - **clock:** soft ≈ remaining/20 + inc/2, or remaining/movestogo + inc/2;
    ///   hard ≈ remaining − overhead
    ///
    /// Soft is clamped to hard so soft never exceeds the abort bound.
    pub fn from_limits(limits: &Limits, stm: Color, move_overhead: Duration) -> Option<Self> {
        if limits.infinite {
            return None;
        }

        if let Some(mt) = limits.movetime {
            // soft = hard = movetime, then subtract overhead from hard only.
            return Some(Self {
                soft: mt,
                hard: mt.saturating_sub(move_overhead),
            });
        }

        let remaining = match stm {
            Color::White => limits.wtime?,
            Color::Black => limits.btime?,
        };
        let inc = match stm {
            Color::White => limits.winc.unwrap_or(Duration::ZERO),
            Color::Black => limits.binc.unwrap_or(Duration::ZERO),
        };

        let soft_base = match limits.movestogo.filter(|&n| n > 0) {
            Some(mtg) => remaining / mtg + inc / 2,
            None => remaining / 20 + inc / 2,
        };
        let hard = remaining.saturating_sub(move_overhead);
        let soft = soft_base.min(hard);

        Some(Self { soft, hard })
    }

    #[inline]
    pub fn soft_exceeded(self, elapsed: Duration) -> bool {
        elapsed >= self.soft
    }

    #[inline]
    pub fn hard_exceeded(self, elapsed: Duration) -> bool {
        elapsed >= self.hard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::Limits;

    #[test]
    fn clock_soft_is_remaining_over_20_plus_half_inc() {
        let limits = Limits {
            wtime: Some(Duration::from_millis(10_000)),
            winc: Some(Duration::from_millis(100)),
            ..Default::default()
        };
        let budget =
            TimeBudget::from_limits(&limits, Color::White, Duration::from_millis(50)).unwrap();
        // soft = 10000/20 + 100/2 = 500 + 50 = 550; hard = 10000 - 50 = 9950
        assert_eq!(budget.soft, Duration::from_millis(550));
        assert_eq!(budget.hard, Duration::from_millis(9950));
    }

    #[test]
    fn movestogo_divides_remaining() {
        let limits = Limits {
            btime: Some(Duration::from_millis(9000)),
            binc: Some(Duration::from_millis(0)),
            movestogo: Some(30),
            ..Default::default()
        };
        let budget =
            TimeBudget::from_limits(&limits, Color::Black, Duration::from_millis(50)).unwrap();
        // soft = 9000/30 + 0 = 300; hard = 9000 - 50 = 8950
        assert_eq!(budget.soft, Duration::from_millis(300));
        assert_eq!(budget.hard, Duration::from_millis(8950));
    }

    #[test]
    fn movetime_soft_is_full_hard_minus_overhead() {
        let limits = Limits {
            movetime: Some(Duration::from_millis(500)),
            ..Default::default()
        };
        let overhead = Duration::from_millis(50);
        let budget = TimeBudget::from_limits(&limits, Color::White, overhead).unwrap();
        assert_eq!(budget.soft, Duration::from_millis(500));
        assert_eq!(budget.hard, Duration::from_millis(450));
    }

    #[test]
    fn soft_clamped_when_remaining_near_overhead() {
        let limits = Limits {
            wtime: Some(Duration::from_millis(80)),
            winc: Some(Duration::ZERO),
            ..Default::default()
        };
        let overhead = Duration::from_millis(50);
        let budget = TimeBudget::from_limits(&limits, Color::White, overhead).unwrap();
        // soft_base = 80/20 = 4; hard = 30 → soft stays 4
        assert_eq!(budget.soft, Duration::from_millis(4));
        assert_eq!(budget.hard, Duration::from_millis(30));
    }

    #[test]
    fn infinite_and_depth_only_yield_none() {
        assert!(TimeBudget::from_limits(
            &Limits {
                infinite: true,
                wtime: Some(Duration::from_secs(60)),
                ..Default::default()
            },
            Color::White,
            DEFAULT_MOVE_OVERHEAD,
        )
        .is_none());

        assert!(TimeBudget::from_limits(
            &Limits {
                depth: Some(8),
                ..Default::default()
            },
            Color::White,
            DEFAULT_MOVE_OVERHEAD,
        )
        .is_none());
    }

    #[test]
    fn uses_side_to_move_clock() {
        let limits = Limits {
            wtime: Some(Duration::from_millis(1000)),
            btime: Some(Duration::from_millis(5000)),
            winc: Some(Duration::ZERO),
            binc: Some(Duration::from_millis(200)),
            ..Default::default()
        };
        let white =
            TimeBudget::from_limits(&limits, Color::White, Duration::from_millis(0)).unwrap();
        let black =
            TimeBudget::from_limits(&limits, Color::Black, Duration::from_millis(0)).unwrap();
        assert_eq!(white.soft, Duration::from_millis(50)); // 1000/20
        assert_eq!(black.soft, Duration::from_millis(350)); // 5000/20 + 100
    }
}
