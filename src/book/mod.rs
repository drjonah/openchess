//! Opening book: pre-search move selection (P10).
//!
//! Before the search runs, [`Book::probe`] returns a weighted book move for the
//! current position, or `None` to fall through to search. All candidate moves
//! are validated against legal move generation, so a stale or malformed book can
//! never produce an illegal move.
//!
//! Backends:
//! - [`mini`] — shallow embedded first-move table (P10-01), always in the default.
//! - [`epd`] — embedded `testing/books/openings.epd` move graph (P10-03).
//! - [`polyglot`] — optional Polyglot `.bin` file via `BookFile` (P10-05).
//! - [`repertoire`] — opt-in deep named lines (P10-08..10); off by default.

pub mod epd;
pub mod mini;
pub mod polyglot;
pub mod repertoire;

use crate::board::Board;
use crate::types::{Key, Move};
use polyglot::PolyglotBook;
use repertoire::BookStyle;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

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

/// Zobrist-keyed table of weighted book moves ([`Board::key`], not Polyglot).
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
    /// Opt-in deep curated repertoire (P10-08). Off by default so SPRT/default
    /// play stay on the shallow mini+EPD book.
    pub repertoire: bool,
    /// Repertoire flavour: `mixed`, `solid`, or `aggressive` (P10-10).
    pub style: String,
}

impl Default for BookConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_plies: 16,
            file: None,
            repertoire: false,
            style: BookStyle::Mixed.as_str().into(),
        }
    }
}

impl BookConfig {
    /// Clamp `max_plies` and normalise style.
    pub fn clamp(&mut self) {
        self.max_plies = self.max_plies.min(60);
        self.style = BookStyle::parse(&self.style).as_str().into();
    }

    pub fn book_style(&self) -> BookStyle {
        BookStyle::parse(&self.style)
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

/// Soft anti-repetition across games (P10-10).
///
/// Remembers recent root book moves and dampens their weights on the next probe
/// from the same position so the bot does not repeat the identical opening every
/// game. A fixed [`BookRng`] seed still reproduces a full game when this state
/// is fresh / unused.
#[derive(Clone, Debug, Default)]
pub struct VarietyState {
    recent: VecDeque<String>,
}

impl VarietyState {
    const CAP: usize = 3;

    pub fn record(&mut self, mv: Move) {
        self.recent.push_back(mv.to_string());
        while self.recent.len() > Self::CAP {
            self.recent.pop_front();
        }
    }

    fn dampen(&self, candidates: &mut [(Move, u32)]) {
        for (mv, weight) in candidates.iter_mut() {
            if self.recent.iter().any(|uci| uci == &mv.to_string()) {
                *weight = (*weight / 4).max(1);
            }
        }
    }
}

#[derive(Clone)]
enum Backend {
    Table(Arc<BookTable>),
    Polyglot(PolyglotBook),
}

/// An opening book ready for probing.
#[derive(Clone)]
pub struct Book {
    config: BookConfig,
    backend: Backend,
}

impl std::fmt::Debug for Book {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Book")
            .field("enabled", &self.config.enabled)
            .field("max_plies", &self.config.max_plies)
            .field("repertoire", &self.config.repertoire)
            .field("style", &self.config.style)
            .field("file", &self.config.file)
            .field(
                "backend",
                &match &self.backend {
                    Backend::Table(_) => "table",
                    Backend::Polyglot(_) => "polyglot",
                },
            )
            .finish()
    }
}

impl Book {
    /// Build a book from settings.
    ///
    /// Disabled config yields an empty book. A configured `file` loads a Polyglot
    /// `.bin` and, on any error, falls back to the embedded default. With
    /// `repertoire: true` and no file, the deep repertoire is merged into the
    /// shallow default. Requires [`crate::lookup::initialize`].
    pub fn from_config(config: &BookConfig) -> Self {
        let mut config = config.clone();
        config.clamp();
        if !config.enabled {
            return Self {
                config,
                backend: Backend::Table(Arc::new(BookTable::new())),
            };
        }
        if let Some(path) = &config.file {
            match PolyglotBook::load(path) {
                Ok(pg) => {
                    return Self {
                        config,
                        backend: Backend::Polyglot(pg),
                    };
                }
                Err(e) => {
                    eprintln!("info string book file load failed ({e}); using embedded book");
                }
            }
        }
        let table = if config.repertoire {
            repertoire_table(config.book_style())
        } else {
            default_table()
        };
        Self {
            config,
            backend: Backend::Table(table),
        }
    }

    /// An always-enabled book backed by the embedded default table.
    pub fn embedded() -> Self {
        Self {
            config: BookConfig::default(),
            backend: Backend::Table(default_table()),
        }
    }

    /// Embedded default merged with the deep repertoire for `style`.
    pub fn with_repertoire(style: BookStyle) -> Self {
        Self {
            config: BookConfig {
                repertoire: true,
                style: style.as_str().into(),
                ..BookConfig::default()
            },
            backend: Backend::Table(repertoire_table(style)),
        }
    }

    /// A permanently disabled book (probe always returns `None`).
    pub fn disabled() -> Self {
        Self {
            config: BookConfig {
                enabled: false,
                ..BookConfig::default()
            },
            backend: Backend::Table(Arc::new(BookTable::new())),
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    pub fn max_plies(&self) -> u32 {
        self.config.max_plies
    }

    pub fn config(&self) -> &BookConfig {
        &self.config
    }

    /// Probe for a weighted book move at `board`, having already played `ply`
    /// half-moves. Returns `None` when disabled, past `max_plies`, on a book
    /// miss, or when no candidate is currently legal.
    pub fn probe(&self, board: &Board, ply: u32, rng: &mut BookRng) -> Option<Move> {
        self.probe_varied(board, ply, rng, None)
    }

    /// Like [`Self::probe`], optionally dampening recently played root moves
    /// (P10-10 anti-repetition). Records the chosen move into `variety` when set.
    pub fn probe_varied(
        &self,
        board: &Board,
        ply: u32,
        rng: &mut BookRng,
        variety: Option<&mut VarietyState>,
    ) -> Option<Move> {
        if !self.config.enabled || ply >= self.config.max_plies {
            return None;
        }
        let mut resolved = self.resolve_legal(board);
        if resolved.is_empty() {
            return None;
        }
        if let Some(v) = variety.as_ref() {
            v.dampen(&mut resolved);
        }
        let total: u64 = resolved.iter().map(|(_, w)| u64::from(*w)).sum();
        if total == 0 {
            return None;
        }
        let mut pick = rng.below(total);
        let mut chosen = resolved[0].0;
        for (mv, weight) in &resolved {
            let w = u64::from(*weight);
            if pick < w {
                chosen = *mv;
                break;
            }
            pick -= w;
        }
        if let Some(v) = variety {
            if ply == 0 {
                v.record(chosen);
            }
        }
        Some(chosen)
    }

    /// Highest-weight legal book move (deterministic), ignoring ply/enabled.
    pub fn best_move(&self, board: &Board) -> Option<Move> {
        self.resolve_legal(board)
            .into_iter()
            .max_by_key(|(_, w)| *w)
            .map(|(mv, _)| mv)
    }

    /// Whether `mv` is a listed book move for `board` (OPEN-01 classification).
    pub fn is_book_move(&self, board: &Board, mv: Move) -> bool {
        match &self.backend {
            Backend::Table(table) => table.get(&board.key()).is_some_and(|candidates| {
                candidates
                    .iter()
                    .any(|bm| board.parse_uci_move(&bm.uci).ok() == Some(mv))
            }),
            Backend::Polyglot(pg) => pg.is_book_move(board, mv),
        }
    }

    fn resolve_legal(&self, board: &Board) -> Vec<(Move, u32)> {
        match &self.backend {
            Backend::Table(table) => table
                .get(&board.key())
                .map(|candidates| {
                    candidates
                        .iter()
                        .filter(|bm| bm.weight > 0)
                        .filter_map(|bm| {
                            board.parse_uci_move(&bm.uci).ok().map(|mv| (mv, bm.weight))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            Backend::Polyglot(pg) => {
                // Re-resolve via best-effort probe helpers.
                pg.candidates_uci(board)
                    .into_iter()
                    .filter_map(|bm| board.parse_uci_move(&bm.uci).ok().map(|mv| (mv, bm.weight)))
                    .collect()
            }
        }
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

/// Lazily built embedded default table (mini + EPD graph).
static DEFAULT_TABLE: OnceLock<Arc<BookTable>> = OnceLock::new();

fn default_table() -> Arc<BookTable> {
    Arc::clone(DEFAULT_TABLE.get_or_init(|| {
        let mut table = mini::table();
        merge_tables(&mut table, epd::embedded_table());
        Arc::new(table)
    }))
}

fn repertoire_table(style: BookStyle) -> Arc<BookTable> {
    let mut table = (*default_table()).clone();
    merge_tables(&mut table, repertoire::table(style));
    Arc::new(table)
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
        assert!(e4_count > 40, "e4 chosen too rarely: {e4_count}/400");
        assert!(e4_count < 400, "no variety in book selection");
    }

    #[test]
    fn morphy_defence_stays_in_book_through_six_plies() {
        init();
        let book = Book::embedded();
        let line = [
            "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6",
        ];
        let mut board = Board::startpos();
        for (ply, uci) in line.iter().enumerate() {
            let mv = board.parse_uci_move(uci).unwrap();
            board.make(mv);
            if ply + 1 < line.len() {
                assert!(
                    book.best_move(&board).is_some(),
                    "book miss after ply {} ({uci})",
                    ply + 1
                );
            }
        }
    }

    #[test]
    fn repertoire_follows_named_line_eight_plies() {
        init();
        let book = Book::with_repertoire(BookStyle::Solid);
        let line = repertoire::line_by_name("Ruy Lopez Morphy").expect("line");
        assert!(line.moves.len() >= 8);
        let mut board = Board::startpos();
        for uci in &line.moves[..8] {
            assert!(
                book.is_book_move(&board, board.parse_uci_move(uci).unwrap())
                    || book.best_move(&board).is_some(),
                "expected book coverage before {uci}"
            );
            let mv = board.parse_uci_move(uci).unwrap();
            board.make(mv);
        }
        assert_eq!(
            repertoire::opening_name_after(&line.moves[..8]),
            Some("Ruy Lopez Morphy")
        );
    }

    #[test]
    fn variety_damps_recent_root_moves_but_seed_reproduces() {
        init();
        let book = Book::embedded();
        let board = Board::startpos();
        let mut variety = VarietyState::default();
        let mut rng_a = BookRng::from_seed(99);
        let first = book
            .probe_varied(&board, 0, &mut rng_a, Some(&mut variety))
            .unwrap();
        // Fresh variety + same seed → same first pick.
        let mut variety2 = VarietyState::default();
        let mut rng_b = BookRng::from_seed(99);
        let again = book
            .probe_varied(&board, 0, &mut rng_b, Some(&mut variety2))
            .unwrap();
        assert_eq!(first, again);

        // After recording, the same seed path may still pick it (weights only
        // dampened), but over many seeds the recorded move should appear less.
        let recorded = first.to_string();
        let mut hits_fresh = 0;
        let mut hits_damped = 0;
        for seed in 0..200u64 {
            let mut rng = BookRng::from_seed(seed);
            if book.probe(&board, 0, &mut rng).unwrap().to_string() == recorded {
                hits_fresh += 1;
            }
            let mut rng = BookRng::from_seed(seed);
            let mut v = VarietyState::default();
            v.record(first);
            if book
                .probe_varied(&board, 0, &mut rng, Some(&mut v))
                .unwrap()
                .to_string()
                == recorded
            {
                hits_damped += 1;
            }
        }
        assert!(
            hits_damped < hits_fresh,
            "anti-rep should reduce repeats: damped={hits_damped} fresh={hits_fresh}"
        );
    }

    #[test]
    fn polyglot_file_loads_via_from_config() {
        init();
        use std::io::Write;
        let start = Board::startpos();
        let key = polyglot::polyglot_key(&start);
        let e2 = "e2".parse().unwrap();
        let e4 = "e4".parse().unwrap();
        let bytes = polyglot::write_bytes(&[(key, polyglot::encode_quiet(e2, e4), 50)]);
        let dir = std::env::temp_dir().join("openchess_book_cfg");
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join("t.bin");
        std::fs::File::create(&path)
            .unwrap()
            .write_all(&bytes)
            .unwrap();

        let book = Book::from_config(&BookConfig {
            file: Some(path),
            ..BookConfig::default()
        });
        assert_eq!(book.best_move(&start).unwrap().to_string(), "e2e4");

        let book = Book::from_config(&BookConfig {
            file: Some(PathBuf::from("/no/such/openchess_book.bin")),
            ..BookConfig::default()
        });
        assert!(book.best_move(&start).is_some());
    }
}
