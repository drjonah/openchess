//! Clustered transposition table (P4).
//!
//! Mate scores are stored as-is for now; ply adjustment lands with P4-03.

use crate::types::{Key, Move, Value};

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
    clusters: Vec<Cluster>,
    /// Mask so `key & cluster_mask` indexes a cluster.
    cluster_mask: u64,
    age: u8,
    /// Number of clusters (power of two).
    cluster_count: usize,
}

impl TranspositionTable {
    /// Allocate a table of roughly `size_mb` megabytes.
    pub fn new(size_mb: usize) -> Self {
        let bytes = size_mb.max(1).saturating_mul(1024 * 1024);
        let cluster_bytes = std::mem::size_of::<Cluster>().max(1);
        let mut cluster_count = (bytes / cluster_bytes).next_power_of_two();
        if cluster_count == 0 {
            cluster_count = 1;
        }
        // Keep a usable minimum even for tiny requests.
        cluster_count = cluster_count.max(16);

        Self {
            clusters: vec![Cluster::default(); cluster_count],
            cluster_mask: (cluster_count as u64) - 1,
            age: 0,
            cluster_count,
        }
    }

    /// Clear all entries and reset age.
    pub fn clear(&mut self) {
        for cluster in &mut self.clusters {
            *cluster = Cluster::default();
        }
        self.age = 0;
    }

    /// Bump generation age for a new search (wraps at 256).
    pub fn new_search(&mut self) {
        self.age = self.age.wrapping_add(1);
    }

    #[inline]
    fn cluster_index(&self, key: Key) -> usize {
        (key & self.cluster_mask) as usize
    }

    /// Probe for an entry matching `key`.
    pub fn probe(&self, key: Key) -> Option<TtEntry> {
        let cluster = &self.clusters[self.cluster_index(key)];
        for entry in &cluster.entries {
            if entry.bound != Bound::None && entry.key == key {
                return Some(*entry);
            }
        }
        None
    }

    /// Store an entry, replacing the least valuable slot in the cluster.
    pub fn store(&mut self, key: Key, mv: Move, score: Value, depth: i16, bound: Bound) {
        let age = self.age;
        let idx = self.cluster_index(key);
        let cluster = &mut self.clusters[idx];

        // Prefer overwriting the same key if present.
        for entry in &mut cluster.entries {
            if entry.bound != Bound::None && entry.key == key {
                // Keep the existing move if the new store has none.
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

        // Otherwise replace the worst entry (empty, stale age, or shallow depth).
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
        let sample = self.cluster_count.min(1000);
        let mut used = 0u32;
        for cluster in self.clusters.iter().take(sample) {
            for entry in &cluster.entries {
                if entry.bound != Bound::None && entry.age == self.age {
                    used += 1;
                }
            }
        }
        // used / (sample * CLUSTER_SIZE) * 1000
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
        let mut tt = TranspositionTable::new(1);
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
        let mut tt = TranspositionTable::new(1);
        tt.store(1, Move::NONE, 0, 1, Bound::Exact);
        assert!(tt.probe(2).is_none());
    }

    #[test]
    fn hashfull_rises_under_fill() {
        let mut tt = TranspositionTable::new(1);
        tt.new_search();
        let before = tt.hashfull();
        for i in 0..5000u64 {
            tt.store(i.wrapping_mul(0x9E37_79B9_7F4A_7C15), Move::NONE, 0, 3, Bound::Lower);
        }
        let after = tt.hashfull();
        assert!(after > before, "hashfull {before} -> {after}");
    }

    #[test]
    fn same_key_updates_in_place() {
        let mut tt = TranspositionTable::new(1);
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
