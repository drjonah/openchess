//! Soft/hard time management (P7-02) with adaptive soft scaling (P7-04).
//!
//! Soft bound ends iterative deepening between iterations without aborting the
//! current search. Hard bound sets the stop flag so the search aborts promptly.
//!
//! Adaptive TM scales the soft (optimum) bound from root stability: best-move
//! changes and falling eval spend more; stable / rising eval spend less.
//! Hard is never scaled.

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

// --- Adaptive soft scale constants (P7-04). Starter values — tune later. ---

/// Base falling-eval factor when the score is flat (`score_drop == 0`).
pub const FALLING_EVAL_OFFSET: f64 = 1.0;
/// Extra soft scale per centipawn of eval drop vs the previous ID iteration.
pub const FALLING_EVAL_PER_CP: f64 = 0.005;
pub const FALLING_EVAL_MIN: f64 = 0.6;
pub const FALLING_EVAL_MAX: f64 = 1.7;

/// Base best-move instability when the root PV has not changed.
pub const BM_INSTABILITY_BASE: f64 = 1.0;
/// Extra soft scale per best-move change across completed ID iterations.
pub const BM_INSTABILITY_PER_CHANGE: f64 = 0.5;
pub const BM_INSTABILITY_MAX: f64 = 3.0;

/// Clamp on the product `falling_eval * best_move_instability`.
pub const SCALE_MIN: f64 = 0.5;
pub const SCALE_MAX: f64 = 5.0;

/// Soft-limit scale from root stability (Stockfish-style structure).
///
/// - `best_move_changes`: how often PV[0] changed across completed ID iterations
/// - `score_drop`: `previous_score - current_score` (positive when eval is falling)
///
/// ```text
/// falling = clamp(1.0 + 0.005 * score_drop, 0.6, 1.7)
/// instability = min(1.0 + 0.5 * best_move_changes, 3.0)
/// scale = clamp(falling * instability, 0.5, 5.0)
/// soft_effective = min(soft_base * scale, hard)
/// ```
pub fn soft_scale(best_move_changes: u32, score_drop: i32) -> f64 {
    let falling = (FALLING_EVAL_OFFSET + FALLING_EVAL_PER_CP * f64::from(score_drop))
        .clamp(FALLING_EVAL_MIN, FALLING_EVAL_MAX);
    let instability =
        (BM_INSTABILITY_BASE + BM_INSTABILITY_PER_CHANGE * f64::from(best_move_changes))
            .min(BM_INSTABILITY_MAX);
    (falling * instability).clamp(SCALE_MIN, SCALE_MAX)
}

/// Soft and hard think-time bounds for one `go`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TimeBudget {
    /// Prefer stopping after this elapsed time (between ID iterations).
    /// Base optimum before adaptive scaling: ≈ remaining/20 + inc/2.
    pub soft: Duration,
    /// Must abort by this elapsed time (sets stop). Never scaled.
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

    /// Soft bound after adaptive stability scaling, still clamped to hard.
    pub fn scaled_soft(self, best_move_changes: u32, score_drop: i32) -> Duration {
        let scale = soft_scale(best_move_changes, score_drop);
        let ms = (self.soft.as_secs_f64() * 1000.0 * scale).round().max(0.0) as u64;
        Duration::from_millis(ms).min(self.hard)
    }

    /// Soft stop using the base (unscaled) optimum.
    #[inline]
    pub fn soft_exceeded(self, elapsed: Duration) -> bool {
        elapsed >= self.soft
    }

    /// Soft stop using stability-scaled optimum (P7-04).
    #[inline]
    pub fn soft_exceeded_scaled(
        self,
        elapsed: Duration,
        best_move_changes: u32,
        score_drop: i32,
    ) -> bool {
        elapsed >= self.scaled_soft(best_move_changes, score_drop)
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

    #[test]
    fn soft_scale_is_one_when_stable() {
        let s = soft_scale(0, 0);
        assert!((s - 1.0).abs() < 1e-9, "stable root scale={s}");
    }

    #[test]
    fn volatile_roots_consume_more_optimum_than_stable() {
        let budget = TimeBudget {
            soft: Duration::from_millis(1_000),
            hard: Duration::from_millis(10_000),
        };

        // Dead-drawn / stable: no PV flips, flat or rising eval.
        let stable = budget.scaled_soft(0, 0);
        let rising = budget.scaled_soft(0, -80);
        // Volatile: several best-move changes + falling eval.
        let volatile = budget.scaled_soft(4, 120);

        assert!(
            rising < stable,
            "rising eval should shrink soft: rising={rising:?} stable={stable:?}"
        );
        assert!(
            volatile > stable,
            "volatile root should get more soft than stable: volatile={volatile:?} stable={stable:?}"
        );
        assert!(
            volatile <= budget.hard,
            "scaled soft must never exceed hard: volatile={volatile:?} hard={:?}",
            budget.hard
        );

        // Explicit scale ordering for logging / acceptance.
        let scale_stable = soft_scale(0, 0);
        let scale_volatile = soft_scale(4, 120);
        assert!(
            scale_volatile > scale_stable,
            "scale_volatile={scale_volatile} scale_stable={scale_stable}"
        );
    }

    #[test]
    fn hard_limit_unaffected_by_stability() {
        let budget = TimeBudget {
            soft: Duration::from_millis(1_000),
            hard: Duration::from_millis(2_500),
        };
        let elapsed_under = Duration::from_millis(2_499);
        let elapsed_over = Duration::from_millis(2_500);

        // Hard checks ignore best-move / eval signals.
        assert!(!budget.hard_exceeded(elapsed_under));
        assert!(budget.hard_exceeded(elapsed_over));
        assert_eq!(budget.hard, Duration::from_millis(2_500));

        // Soft may clamp to hard under max scale; hard field stays fixed.
        let scaled = budget.scaled_soft(10, 500);
        assert_eq!(scaled, budget.hard);
        assert_eq!(budget.hard, Duration::from_millis(2_500));
        assert!(budget.hard_exceeded(budget.hard));
        // Soft stop at hard must not imply hard changed — abort path still uses hard.
        assert!(budget.soft_exceeded_scaled(budget.hard, 10, 500));
    }

    #[test]
    fn soft_exceeded_scaled_uses_stability() {
        let budget = TimeBudget {
            soft: Duration::from_millis(1_000),
            hard: Duration::from_millis(10_000),
        };
        let elapsed = Duration::from_millis(1_100);

        // Stable: soft ≈ 1000 → exceeded.
        assert!(budget.soft_exceeded_scaled(elapsed, 0, 0));
        // Volatile: soft scaled well above 1100 → not yet exceeded.
        assert!(!budget.soft_exceeded_scaled(elapsed, 4, 100));
    }
}
