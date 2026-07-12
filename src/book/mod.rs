//! Opening book: pre-search move selection (P10).
//!
//! Before the search runs, [`Book::probe`] returns a weighted book move for the
//! current position, or `None` to fall through to search. All candidate moves
//! are validated against legal move generation, so a stale or malformed book can
//! never produce an illegal move.
//!
//! Backends:
//! - [`mini`] — a tiny embedded first-move table (P10-01), always available.
//! - [`epd`] — the embedded `testing/books/openings.epd` set, expanded into a
//!   Zobrist-keyed move graph (P10-03).
//!
//! The default book merges the mini table with the EPD graph. A `BookFile` path
//! (Polyglot `.bin` — P10-05) may extend this later; unknown/broken files fall
//! back to the embedded default so play never breaks.

pub mod epd;
pub mod mini;

use crate::board::Board;
use crate::types::{Key, Move};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

/// A single weighted book candidate, stored as a UCI string and resolved (and
/// legality-checked) against the live position at probe time.
#[derive(Clone, Debug)]
pub struct BookMove {
    pub uci: String,
    pub weight: u32,
}

impl BookMove {
    pub fn new(uci: impl Into<String>, weight: u32) -> Self {
        Self {
            uci: uci.into(),
            weight,
        }
    }
}

/// Zobrist-keyed table of weighted book moves.
pub type BookTable = HashMap<Key, Vec<BookMove>>;

/// Persisted book settings (serde; lives inside [`crate::config::Config`]).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BookConfig {
    /// Master switch. `OwnBook false` (SPRT / analysis) disables all probing.
    pub enabled: bool,
    /// Stop probing once this many plies (half-moves) have been played.
    pub max_plies: u32,
    /// Optional Polyglot `.bin` path; `None` uses the embedded default book.
    pub file: Option<PathBuf>,
}

impl Default for BookConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_plies: 16,
            file: None,
        }
    }
}

impl BookConfig {
    /// Clamp `max_plies` to a sane range (0 disables via ply check).
    pub fn clamp(&mut self) {
        self.max_plies = self.max_plies.min(60);
    }
}

/// A tiny deterministic-by-seed PRNG (SplitMix64) for weighted book selection.
///
/// Kept in-tree to avoid a `rand` dependency. Seed explicitly with
/// [`BookRng::from_seed`] for reproducible tests.
#[derive(Clone, Debug)]
pub struct BookRng(u64);

impl BookRng {
    pub fn from_seed(seed: u64) -> Self {
        Self(seed)
    }

    /// Seed from the wall clock (best-effort entropy for varied bot play).
    pub fn from_entropy() -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self(nanos ^ 0xD1B5_4A32_D192_ED03)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform value in `0..n` (`n` must be non-zero).
    pub fn below(&mut self, n: u64) -> u64 {
        debug_assert!(n > 0);
        self.next_u64() % n
    }
}

impl Default for BookRng {
    fn default() -> Self {
        Self::from_entropy()
    }
}

/// An opening book ready for probing.
#[derive(Clone)]
pub struct Book {
    config: BookConfig,
    table: Arc<BookTable>,
}

impl Book {
    /// Build a book from settings.
    ///
    /// Disabled config yields an empty book. A configured `file` is loaded and,
    /// on any error, falls back to the embedded default book. Requires
    /// [`crate::lookup::initialize`] to have run (move generation is used to
    /// build and validate entries).
    pub fn from_config(config: &BookConfig) -> Self {
        let table = if !config.enabled {
            Arc::new(BookTable::new())
        } else if let Some(path) = &config.file {
            load_file(path).unwrap_or_else(|_| default_table())
        } else {
            default_table()
        };
        Self {
            config: config.clone(),
            table,
        }
    }

    /// An always-enabled book backed by the embedded default table.
    pub fn embedded() -> Self {
        Self {
            config: BookConfig::default(),
            table: default_table(),
        }
    }

    /// A permanently disabled book (probe always returns `None`).
    pub fn disabled() -> Self {
        Self {
            config: BookConfig {
                enabled: false,
                ..BookConfig::default()
            },
            table: Arc::new(BookTable::new()),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn max_plies(&self) -> u32 {
        self.config.max_plies
    }

    /// Probe for a weighted book move at `board`, having already played `ply`
    /// half-moves. Returns `None` when disabled, past `max_plies`, on a book
    /// miss, or when no candidate is currently legal.
    pub fn probe(&self, board: &Board, ply: u32, rng: &mut BookRng) -> Option<Move> {
        if !self.config.enabled || ply >= self.config.max_plies {
            return None;
        }
        let candidates = self.table.get(&board.key())?;
        let resolved = self.resolve_legal(board, candidates);
        if resolved.is_empty() {
            return None;
        }
        let total: u64 = resolved.iter().map(|(_, w)| u64::from(*w)).sum();
        if total == 0 {
            return None;
        }
        let mut pick = rng.below(total);
        for (mv, weight) in &resolved {
            let w = u64::from(*weight);
            if pick < w {
                return Some(*mv);
            }
            pick -= w;
        }
        Some(resolved[0].0)
    }

    /// Highest-weight legal book move (deterministic), ignoring ply/enabled.
    ///
    /// Useful for tests and tooling; play uses [`Self::probe`].
    pub fn best_move(&self, board: &Board) -> Option<Move> {
        let candidates = self.table.get(&board.key())?;
        self.resolve_legal(board, candidates)
            .into_iter()
            .max_by_key(|(_, w)| *w)
            .map(|(mv, _)| mv)
    }

    /// Whether `mv` is a listed book move for `board` (for move classification,
    /// OPEN-01). Ignores the enabled flag and ply so post-game analysis can tag
    /// theory even when the bot did not use the book.
    pub fn is_book_move(&self, board: &Board, mv: Move) -> bool {
        self.table.get(&board.key()).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|bm| board.parse_uci_move(&bm.uci).ok() == Some(mv))
        })
    }

    /// Resolve candidates to `(legal Move, weight)` pairs, dropping unresolved,
    /// illegal, or zero-weight entries.
    fn resolve_legal(&self, board: &Board, candidates: &[BookMove]) -> Vec<(Move, u32)> {
        candidates
            .iter()
            .filter(|bm| bm.weight > 0)
            .filter_map(|bm| board.parse_uci_move(&bm.uci).ok().map(|mv| (mv, bm.weight)))
            .collect()
    }
}

/// Merge `src` into `dst`, summing weights for duplicate moves per position.
pub(crate) fn merge_tables(dst: &mut BookTable, src: BookTable) {
    for (key, moves) in src {
        let entry = dst.entry(key).or_default();
        for bm in moves {
            match entry.iter_mut().find(|existing| existing.uci == bm.uci) {
                Some(existing) => existing.weight = existing.weight.saturating_add(bm.weight),
                None => entry.push(bm),
            }
        }
    }
}

/// Build the embedded default table (mini first-move table + EPD graph).
///
/// Built fresh per call (cheap, and only on bot moves) rather than cached, so a
/// build attempted before [`crate::lookup::initialize`] can never poison a
/// shared table for the rest of the process.
fn default_table() -> Arc<BookTable> {
    let mut table = mini::table();
    merge_tables(&mut table, epd::embedded_table());
    Arc::new(table)
}

/// Load a book file by extension. Only Polyglot `.bin` is a placeholder here
/// (P10-05); everything else errors so the caller falls back to the default.
fn load_file(path: &std::path::Path) -> Result<Arc<BookTable>, String> {
    Err(format!(
        "book file loading not yet implemented for {} (P10-05)",
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn rng_is_deterministic_for_seed() {
        let mut a = BookRng::from_seed(42);
        let mut b = BookRng::from_seed(42);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn disabled_book_never_probes() {
        init();
        let book = Book::disabled();
        let board = Board::startpos();
        let mut rng = BookRng::from_seed(1);
        assert!(book.probe(&board, 0, &mut rng).is_none());
    }

    #[test]
    fn startpos_probe_is_legal_and_not_a_flank_pawn() {
        init();
        let book = Book::embedded();
        let board = Board::startpos();
        let legal = board.legal_moves();
        // Every seed must yield a legal, sensible first move.
        for seed in 0..64u64 {
            let mut rng = BookRng::from_seed(seed);
            let mv = book.probe(&board, 0, &mut rng).expect("startpos is in book");
            assert!(legal.contains(&mv), "book move {mv} not legal");
            assert!(
                !matches!(
                    mv.to_string().as_str(),
                    "a2a3" | "a2a4" | "h2h3" | "h2h4"
                ),
                "book returned a flank pawn push: {mv}"
            );
        }
    }

    #[test]
    fn past_max_plies_returns_none() {
        init();
        let book = Book::embedded();
        let board = Board::startpos();
        let mut rng = BookRng::from_seed(1);
        assert!(book.probe(&board, book.max_plies(), &mut rng).is_none());
    }

    #[test]
    fn black_has_a_reply_after_e4() {
        init();
        let book = Book::embedded();
        let mut board = Board::startpos();
        board.make(board.parse_uci_move("e2e4").unwrap());
        let mut rng = BookRng::from_seed(7);
        let reply = book.probe(&board, 1, &mut rng).expect("e4 replies in book");
        assert!(board.legal_moves().contains(&reply));
    }

    #[test]
    fn weighted_selection_respects_weights() {
        // A 90/10 split should favour the heavy move across many seeds.
        init();
        let book = Book::embedded();
        let board = Board::startpos();
        let e4 = board.parse_uci_move("e2e4").unwrap();
        let mut e4_count = 0;
        for seed in 0..400u64 {
            let mut rng = BookRng::from_seed(seed);
            if book.probe(&board, 0, &mut rng) == Some(e4) {
                e4_count += 1;
            }
        }
        // e4 carries the largest default weight; it should appear often but
        // not always (variety), confirming weighting works.
        assert!(e4_count > 40, "e4 chosen too rarely: {e4_count}/400");
        assert!(e4_count < 400, "no variety in book selection");
    }
}
