//! Concurrent arena runner: many isolated slots advanced by a fair scheduler
//! (P11-01).
//!
//! Two concurrency modes share one code path:
//! - **Serial** (`max_concurrent == 1`, default): one search in flight, slots
//!   advanced round-robin so none starves.
//! - **Bounded parallel** (`max_concurrent > 1`): up to `K` single-threaded
//!   searches at once. Searches never use Lazy SMP — the arena scales by
//!   running more games, not by threading each one.

use std::time::Duration;

use crate::config::SideStrength;
use crate::types::Color;

use super::profile::ArenaProfile;
use super::slot::{GameSlot, SlotEvent};
use super::snapshot::GameSnapshot;

/// Default per-slot ply cap before a game is adjudicated a draw.
pub const DEFAULT_PLY_LIMIT: usize = 400;
/// Default private TT size per search (MB).
pub const DEFAULT_HASH_MB: usize = 8;

/// Startup configuration for an [`Arena`].
#[derive(Clone, Debug)]
pub struct ArenaConfig {
    pub games: usize,
    pub white: SideStrength,
    pub black: SideStrength,
    pub ply_limit: usize,
    pub concurrency: usize,
    pub hash_mb: usize,
    /// Optional named profiles assigned round-robin across slots.
    pub profiles: Vec<ArenaProfile>,
    /// Swap colors on odd slots so each profile plays both sides.
    pub alternate_colors: bool,
}

impl Default for ArenaConfig {
    fn default() -> Self {
        Self {
            games: 1,
            white: SideStrength::default(),
            black: SideStrength::default(),
            ply_limit: DEFAULT_PLY_LIMIT,
            concurrency: 1,
            hash_mb: DEFAULT_HASH_MB,
            profiles: Vec::new(),
            alternate_colors: true,
        }
    }
}

/// A set of concurrent Bot-vs-Bot game slots and a fair scheduler.
pub struct Arena {
    slots: Vec<GameSlot>,
    max_concurrent: usize,
    hash_mb: usize,
    /// Rotating index so the top-up scan is round-robin (fairness).
    scan_start: usize,
}

impl Arena {
    /// Build `games` slots that share one White/Black strength.
    pub fn new(games: usize, white: SideStrength, black: SideStrength, ply_limit: usize) -> Self {
        Self::from_config(&ArenaConfig {
            games,
            white,
            black,
            ply_limit,
            ..ArenaConfig::default()
        })
    }

    /// Build an arena from a full configuration.
    pub fn from_config(config: &ArenaConfig) -> Self {
        let games = config.games.max(1);
        let ply_limit = config.ply_limit.max(1);
        let mut slots = Vec::with_capacity(games);

        for i in 0..games {
            let (mut white, mut black, profile) = if config.profiles.is_empty() {
                (config.white.clone(), config.black.clone(), None)
            } else {
                let p = &config.profiles[i % config.profiles.len()];
                (p.white.clone(), p.black.clone(), Some(p.name.clone()))
            };
            // Swap colors on odd slots so each profile plays both sides.
            if config.alternate_colors && !config.profiles.is_empty() && i % 2 == 1 {
                std::mem::swap(&mut white, &mut black);
            }
            let mut slot = GameSlot::new(i, white, black, ply_limit);
            slot.profile = profile;
            slots.push(slot);
        }

        Self {
            slots,
            max_concurrent: config.concurrency.max(1),
            hash_mb: config.hash_mb.max(1),
            scan_start: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }

    pub fn slots(&self) -> &[GameSlot] {
        &self.slots
    }

    pub fn slot(&self, id: usize) -> Option<&GameSlot> {
        self.slots.get(id)
    }

    pub fn slot_mut(&mut self, id: usize) -> Option<&mut GameSlot> {
        self.slots.get_mut(id)
    }

    /// Cloneable read-only views of every slot.
    pub fn snapshots(&self) -> Vec<GameSnapshot> {
        self.slots.iter().map(GameSnapshot::of).collect()
    }

    pub fn thinking_count(&self) -> usize {
        self.slots.iter().filter(|s| s.is_thinking()).count()
    }

    pub fn any_runnable(&self) -> bool {
        self.slots.iter().any(GameSlot::is_runnable)
    }

    pub fn all_finished(&self) -> bool {
        self.slots.iter().all(GameSlot::is_finished)
    }

    /// Advance the arena one step: apply finished searches, then top up new
    /// searches up to the concurrency cap. Non-blocking.
    pub fn tick(&mut self) -> Vec<SlotEvent> {
        let mut events = Vec::new();
        for slot in &mut self.slots {
            if slot.is_thinking() {
                events.extend(slot.poll());
            }
        }

        let mut capacity = self.max_concurrent.saturating_sub(self.thinking_count());
        if capacity > 0 && !self.slots.is_empty() {
            let n = self.slots.len();
            for k in 0..n {
                if capacity == 0 {
                    break;
                }
                let idx = (self.scan_start + k) % n;
                if self.slots[idx].is_runnable() {
                    self.slots[idx].begin_search(self.hash_mb);
                    self.scan_start = (idx + 1) % n;
                    capacity -= 1;
                }
            }
        }
        events
    }

    /// Drive the arena to completion, forwarding each event to `on_event`.
    ///
    /// Returns when all slots are finished, or when no slot can make progress
    /// (e.g. every remaining slot is paused).
    pub fn run_to_completion(&mut self, on_event: &mut dyn FnMut(&SlotEvent)) {
        loop {
            let events = self.tick();
            for e in &events {
                on_event(e);
            }
            if self.all_finished() {
                break;
            }
            if self.thinking_count() == 0 && !self.any_runnable() {
                // No progress possible (all paused/aborted).
                break;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    // --- Per-slot control (P11-05 / P11-06) ---------------------------------

    /// Apply a strength change to one side of a slot (takes effect next move).
    pub fn set_slot_strength(&mut self, id: usize, color: Color, strength: SideStrength) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.set_strength(color, strength);
        }
    }

    /// Mirror a strength change to a given side of every slot.
    pub fn set_all_strength(&mut self, color: Color, strength: SideStrength) {
        for slot in &mut self.slots {
            slot.set_strength(color, strength.clone());
        }
    }

    pub fn pause_slot(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.pause();
        }
    }

    pub fn resume_slot(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.resume();
        }
    }

    pub fn restart_slot(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.restart();
        }
    }

    pub fn abort_slot(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.abort();
        }
    }

    /// Request one manual move on a paused slot.
    pub fn step_slot(&mut self, id: usize) {
        if let Some(slot) = self.slots.get_mut(id) {
            slot.request_step();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::slot::Outcome;
    use crate::board::GameResult;

    fn strength(depth: u32) -> SideStrength {
        SideStrength {
            depth,
            movetime_ms: 0,
        }
    }

    #[test]
    fn four_independent_games_play_to_completion() {
        crate::lookup::initialize();
        let mut arena = Arena::new(4, strength(1), strength(1), 80);
        assert_eq!(arena.len(), 4);
        arena.run_to_completion(&mut |_| {});

        assert!(arena.all_finished());
        for slot in arena.slots() {
            assert!(slot.is_finished());
            // Every game reached a legal terminal state or was adjudicated.
            let ok = matches!(
                slot.result(),
                GameResult::Checkmate { .. }
                    | GameResult::Stalemate
                    | GameResult::DrawRepetition
                    | GameResult::DrawFiftyMove
                    | GameResult::DrawInsufficientMaterial
            ) || slot.finish_reason() == Some(super::super::slot::FinishReason::PlyCap);
            assert!(ok, "slot {} ended with {:?}", slot.id, slot.result());
            assert!(slot.ply_count() <= 80);
            assert_ne!(slot.outcome(), Outcome::Unfinished);
        }
    }

    #[test]
    fn serial_scheduler_runs_one_search_at_a_time() {
        crate::lookup::initialize();
        // movetime gives searches enough duration to observe concurrency.
        let s = SideStrength {
            depth: 30,
            movetime_ms: 60,
        };
        let mut arena = Arena::new(4, s.clone(), s, 80);
        // First tick starts exactly one search (serial default).
        let _ = arena.tick();
        assert_eq!(arena.thinking_count(), 1);
    }

    #[test]
    fn bounded_parallel_starts_multiple_searches() {
        crate::lookup::initialize();
        let s = SideStrength {
            depth: 30,
            movetime_ms: 60,
        };
        let config = ArenaConfig {
            games: 4,
            white: s.clone(),
            black: s,
            ply_limit: 80,
            concurrency: 3,
            ..ArenaConfig::default()
        };
        let mut arena = Arena::from_config(&config);
        let _ = arena.tick();
        assert_eq!(arena.thinking_count(), 3);
        // Clean shutdown of in-flight searches.
        for id in 0..arena.len() {
            arena.abort_slot(id);
        }
    }

    #[test]
    fn fair_scheduler_advances_all_slots() {
        crate::lookup::initialize();
        let mut arena = Arena::new(3, strength(1), strength(1), 60);
        arena.run_to_completion(&mut |_| {});
        for slot in arena.slots() {
            assert!(slot.ply_count() >= 1, "slot {} never moved", slot.id);
        }
    }

    #[test]
    fn profiles_assigned_round_robin_with_color_swap() {
        crate::lookup::initialize();
        let config = ArenaConfig {
            games: 2,
            profiles: vec![super::super::profile::ArenaProfile {
                name: "asym".into(),
                white: strength(9),
                black: strength(2),
            }],
            alternate_colors: true,
            ply_limit: 40,
            ..ArenaConfig::default()
        };
        let arena = Arena::from_config(&config);
        // Slot 0: strong White; slot 1: colors swapped → strong Black.
        assert_eq!(arena.slot(0).unwrap().white.depth, 9);
        assert_eq!(arena.slot(0).unwrap().black.depth, 2);
        assert_eq!(arena.slot(1).unwrap().white.depth, 2);
        assert_eq!(arena.slot(1).unwrap().black.depth, 9);
        assert_eq!(arena.slot(0).unwrap().profile.as_deref(), Some("asym"));
    }
}
