//! Clustered transposition table (P4).
//!
//! Scores are stored as opaque [`Value`]s. Callers (search) must ply-adjust
//! mate/TB scores via [`crate::types::score::value_to_tt`] /
//! [`crate::types::score::value_from_tt`] around [`TranspositionTable::store`] /
//! [`TranspositionTable::probe`].
//!
//! **Lazy SMP:** [`probe`] / [`store`] take `&self` and are intentionally racy
//! (Stockfish-family). Concurrent writers may tear an entry; that is accepted
//! for NPS. Only documented shared mutable structure besides atomics.

use crate::types::{Key, Move, Value};
use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU8, Ordering};

/// Bound type for a TT entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Bound {
    None = 0,
    Exact = 1,
    Lower = 2,
    Upper = 3,
}

impl Bound {
    #[inline]
    pub const fn is_none(self) -> bool {
        matches!(self, Bound::None)
    }
}

/// Single TT slot.
#[derive(Clone, Copy, Debug)]
pub struct TtEntry {
    pub key: Key,
    pub mv: Move,
    pub score: Value,
    pub depth: i16,
    pub bound: Bound,
    pub age: u8,
}

impl Default for TtEntry {
    fn default() -> Self {
        Self {
            key: 0,
            mv: Move::NONE,
            score: 0,
            depth: -1,
            bound: Bound::None,
            age: 0,
        }
    }
}

const CLUSTER_SIZE: usize = 3;

#[derive(Clone, Copy, Debug, Default)]
struct Cluster {
    entries: [TtEntry; CLUSTER_SIZE],
}

/// Clustered transposition table with depth- and age-aware replacement.
pub struct TranspositionTable {
    clusters: Box<[UnsafeCell<Cluster>]>,
    /// Mask so `key & cluster_mask` indexes a cluster.
    cluster_mask: u64,
    age: AtomicU8,
    /// Number of clusters (power of two).
    cluster_count: usize,
}

// SAFETY: clusters are only accessed via racy probe/store; callers accept tears.
unsafe impl Sync for TranspositionTable {}
unsafe impl Send for TranspositionTable {}

impl TranspositionTable {
    /// Allocate a table of roughly `size_mb` megabytes.
    pub fn new(size_mb: usize) -> Self {
        let bytes = size_mb.max(1).saturating_mul(1024 * 1024);
        let cluster_bytes = std::mem::size_of::<Cluster>().max(1);
        let mut cluster_count = (bytes / cluster_bytes).next_power_of_two();
        if cluster_count == 0 {
            cluster_count = 1;
        }
        cluster_count = cluster_count.max(16);

        let clusters: Vec<UnsafeCell<Cluster>> = (0..cluster_count)
            .map(|_| UnsafeCell::new(Cluster::default()))
            .collect();

        Self {
            clusters: clusters.into_boxed_slice(),
            cluster_mask: (cluster_count as u64) - 1,
            age: AtomicU8::new(0),
            cluster_count,
        }
    }

    /// Clear all entries and reset age.
    pub fn clear(&self) {
        for cluster in self.clusters.iter() {
            // SAFETY: exclusive clear — caller must not search concurrently.
            unsafe {
                *cluster.get() = Cluster::default();
            }
        }
        self.age.store(0, Ordering::Relaxed);
    }

    /// Bump generation age for a new search (wraps at 256).
    pub fn new_search(&self) {
        self.age.fetch_add(1, Ordering::Relaxed);
    }

    #[inline]
    fn cluster_index(&self, key: Key) -> usize {
        (key & self.cluster_mask) as usize
    }

    /// Probe for an entry matching `key`.
    pub fn probe(&self, key: Key) -> Option<TtEntry> {
        // SAFETY: racy read of a Copy cluster; torn reads may miss — acceptable.
        let cluster = unsafe { *self.clusters[self.cluster_index(key)].get() };
        for entry in &cluster.entries {
            if entry.bound != Bound::None && entry.key == key {
                return Some(*entry);
            }
        }
        None
    }

    /// Prefetch hint for a soon-to-be-probed key.
    #[inline]
    pub fn prefetch(&self, key: Key) {
        let idx = self.cluster_index(key);
        let ptr = self.clusters.as_ptr().wrapping_add(idx) as *const u8;
        let _ = ptr;
        #[cfg(target_arch = "x86_64")]
        {
            unsafe {
                core::arch::x86_64::_mm_prefetch::<{ core::arch::x86_64::_MM_HINT_T0 }>(
                    ptr as *const i8,
                );
            }
        }
    }

    /// Store an entry, replacing the least valuable slot in the cluster.
    pub fn store(&self, key: Key, mv: Move, score: Value, depth: i16, bound: Bound) {
        let age = self.age.load(Ordering::Relaxed);
        let idx = self.cluster_index(key);
        // SAFETY: racy write — intentional for Lazy SMP.
        let cluster = unsafe { &mut *self.clusters[idx].get() };

        for entry in &mut cluster.entries {
            if entry.bound != Bound::None && entry.key == key {
                let keep_move = if mv.is_none() { entry.mv } else { mv };
                *entry = TtEntry {
                    key,
                    mv: keep_move,
                    score,
                    depth,
                    bound,
                    age,
                };
                return;
            }
        }

        let mut replace = 0usize;
        let mut worst = i32::MAX;
        for (i, entry) in cluster.entries.iter().enumerate() {
            let quality = if entry.bound == Bound::None {
                i32::MIN
            } else {
                let age_penalty = i32::from(age.wrapping_sub(entry.age)) * 4;
                i32::from(entry.depth) - age_penalty
            };
            if quality < worst {
                worst = quality;
                replace = i;
            }
        }

        cluster.entries[replace] = TtEntry {
            key,
            mv,
            score,
            depth,
            bound,
            age,
        };
    }

    /// Approximate fill in permille (0..=1000), Stockfish-style hashfull.
    pub fn hashfull(&self) -> u32 {
        let age = self.age.load(Ordering::Relaxed);
        let sample = self.cluster_count.min(1000);
        let mut used = 0u32;
        for cluster_cell in self.clusters.iter().take(sample) {
            let cluster = unsafe { *cluster_cell.get() };
            for entry in &cluster.entries {
                if entry.bound != Bound::None && entry.age == age {
                    used += 1;
                }
            }
        }
        (used * 1000) / (sample as u32 * CLUSTER_SIZE as u32)
    }

    pub fn cluster_count(&self) -> usize {
        self.cluster_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Square;

    #[test]
    fn store_then_probe_hits() {
        let tt = TranspositionTable::new(1);
        let key = 0xDEAD_BEEF_CAFE_BABEu64;
        let mv = Move::new(
            Square::from_index_unchecked(12),
            Square::from_index_unchecked(28),
        );
        tt.store(key, mv, 42, 5, Bound::Exact);
        let hit = tt.probe(key).expect("hit");
        assert_eq!(hit.mv, mv);
        assert_eq!(hit.score, 42);
        assert_eq!(hit.depth, 5);
        assert_eq!(hit.bound, Bound::Exact);
    }

    #[test]
    fn wrong_key_misses() {
        let tt = TranspositionTable::new(1);
        tt.store(1, Move::NONE, 0, 1, Bound::Exact);
        assert!(tt.probe(2).is_none());
    }

    #[test]
    fn hashfull_rises_under_fill() {
        let tt = TranspositionTable::new(1);
        tt.new_search();
        let before = tt.hashfull();
        for i in 0..5000u64 {
            tt.store(
                i.wrapping_mul(0x9E37_79B9_7F4A_7C15),
                Move::NONE,
                0,
                3,
                Bound::Lower,
            );
        }
        let after = tt.hashfull();
        assert!(after > before, "hashfull {before} -> {after}");
    }

    #[test]
    fn same_key_updates_in_place() {
        let tt = TranspositionTable::new(1);
        let key = 99u64;
        let mv = Move::new(
            Square::from_index_unchecked(0),
            Square::from_index_unchecked(1),
        );
        tt.store(key, mv, 10, 2, Bound::Upper);
        tt.store(key, Move::NONE, 20, 4, Bound::Exact);
        let hit = tt.probe(key).unwrap();
        assert_eq!(hit.score, 20);
        assert_eq!(hit.depth, 4);
        assert_eq!(hit.bound, Bound::Exact);
        assert_eq!(hit.mv, mv, "should keep prior move when new is NONE");
    }
}
