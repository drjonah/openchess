//! Curated deep opening repertoire (P10-08 / P10-09 / P10-10).
//!
//! Named main lines to ~8–12 plies, keyed by Zobrist hash after replay from the
//! start position. This module is **opt-in**: it is not merged into the shallow
//! default book in [`super::mini`], so SPRT and default play stay unchanged
//! until repertoire is explicitly selected.
//!
//! # Adding an opening
//!
//! 1. Add a [`RepertoireLine`] to [`LINES`] below with:
//!    - **`name`** — human-readable label (used by [`opening_name_after`]).
//!    - **`style`** — [`BookStyle::Solid`] or [`BookStyle::Aggressive`] ([`BookStyle::Mixed`]
//!      includes every line when building a table).
//!    - **`side`** — the hero color this line is repertoire for ([`Color::White`] or
//!      [`Color::Black`]); informational for selection policy, not used when building
//!      the move graph.
//!    - **`moves`** — full UCI sequence from startpos, at least eight plies, every
//!      move legal when replayed in order.
//!    - **`weight`** — relative branch weight (summed when lines transpose to the
//!      same position and recommend the same move).
//! 2. Run `cargo test repertoire` — illegal lines fail in
//!    [`tests::every_line_is_legal_and_deep_enough`].
//! 3. If the line transposes with an existing one, no extra work is needed:
//!    [`table`] merges candidates at the shared Zobrist key via [`super::merge_tables`].

use super::{BookMove, BookTable, merge_tables};
use crate::board::Board;
use crate::types::Color;

/// Repertoire flavour: all lines, solid structures, or sharp/aggressive systems.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BookStyle {
    Mixed,
    Solid,
    Aggressive,
}

impl BookStyle {
    /// Parse `mixed`, `solid`, or `aggressive` (case-insensitive); unknown → [`Mixed`].
    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("solid") {
            BookStyle::Solid
        } else if s.eq_ignore_ascii_case("aggressive") {
            BookStyle::Aggressive
        } else {
            BookStyle::Mixed
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            BookStyle::Mixed => "mixed",
            BookStyle::Solid => "solid",
            BookStyle::Aggressive => "aggressive",
        }
    }
}

/// One named repertoire main line from the start position.
pub struct RepertoireLine {
    pub name: &'static str,
    pub style: BookStyle,
    pub side: Color,
    pub moves: &'static [&'static str],
    pub weight: u32,
}

const LINES: &[RepertoireLine] = &[
    RepertoireLine {
        name: "Ruy Lopez Morphy",
        style: BookStyle::Solid,
        side: Color::White,
        moves: &[
            "e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4", "g8f6", "e1g1", "f8e7",
        ],
        weight: 120,
    },
    RepertoireLine {
        name: "Italian Game",
        style: BookStyle::Aggressive,
        side: Color::White,
        moves: &[
            "e2e4", "e7e5", "g1f3", "b8c6", "f1c4", "g8f6", "d2d3", "f8c5", "c2c3", "d7d6",
        ],
        weight: 85,
    },
    RepertoireLine {
        name: "Queen's Gambit Declined",
        style: BookStyle::Solid,
        side: Color::White,
        moves: &[
            "d2d4", "d7d5", "c2c4", "e7e6", "b1c3", "g8f6", "c1g5", "f8e7", "e2e3", "e8g8",
        ],
        weight: 90,
    },
    RepertoireLine {
        name: "Sicilian Najdorf",
        style: BookStyle::Aggressive,
        side: Color::Black,
        moves: &[
            "e2e4", "c7c5", "g1f3", "d7d6", "d2d4", "c5d4", "f3d4", "g8f6", "b1c3", "a7a6",
            "c1g5", "e7e6",
        ],
        weight: 95,
    },
    RepertoireLine {
        name: "Caro-Kann Classical",
        style: BookStyle::Solid,
        side: Color::Black,
        moves: &[
            "e2e4", "c7c6", "d2d4", "d7d5", "b1c3", "d5e4", "c3e4", "g8f6", "e4f6", "e7f6",
        ],
        weight: 90,
    },
    RepertoireLine {
        name: "King's Indian Defence",
        style: BookStyle::Solid,
        side: Color::Black,
        moves: &[
            "d2d4", "g8f6", "c2c4", "g7g6", "b1c3", "f8g7", "e2e4", "d7d6", "g1f3", "e8g8",
        ],
        weight: 85,
    },
    RepertoireLine {
        name: "Queen's Gambit Declined (Black)",
        style: BookStyle::Solid,
        side: Color::Black,
        moves: &[
            "d2d4", "d7d5", "c2c4", "e7e6", "b1c3", "g8f6", "c1g5", "f8e7", "e2e3", "e8g8",
        ],
        weight: 80,
    },
    // Transposes with Four Knights (Nf6 first) after 1.e4 e5 2.Nf3 Nc6 3.Nc3 Nf6.
    RepertoireLine {
        name: "Four Knights (Nc3)",
        style: BookStyle::Solid,
        side: Color::White,
        moves: &[
            "e2e4", "e7e5", "g1f3", "b8c6", "b1c3", "g8f6", "f1c4", "f8c5",
        ],
        weight: 35,
    },
    RepertoireLine {
        name: "Four Knights (Nf6 first)",
        style: BookStyle::Solid,
        side: Color::White,
        moves: &[
            "e2e4", "e7e5", "g1f3", "g8f6", "b1c3", "b8c6", "f1c4", "f8c5",
        ],
        weight: 35,
    },
];

/// All authored repertoire lines.
pub fn lines() -> &'static [RepertoireLine] {
    LINES
}

fn style_matches(line: &RepertoireLine, style: BookStyle) -> bool {
    matches!(style, BookStyle::Mixed) || line.style == style
}

/// Build a Zobrist-keyed book table for `style`.
///
/// For each matching line, replay from startpos and at every prefix insert the
/// line's *next* move with its weight. Transpositions merge via [`super::merge_tables`].
pub fn table(style: BookStyle) -> BookTable {
    let mut table = BookTable::new();
    for line in lines().iter().filter(|line| style_matches(line, style)) {
        let mut partial = BookTable::new();
        let mut board = Board::startpos();
        for uci in line.moves {
            partial
                .entry(board.key())
                .or_default()
                .push(BookMove::new(*uci, line.weight));
            match board.parse_uci_move(uci) {
                Ok(mv) => board.make(mv),
                Err(_) => {
                    debug_assert!(
                        false,
                        "illegal repertoire move {uci} in line {}",
                        line.name
                    );
                    break;
                }
            }
        }
        merge_tables(&mut table, partial);
    }
    table
}

/// Look up a line by exact `name`.
pub fn line_by_name(name: &str) -> Option<&'static RepertoireLine> {
    lines().iter().find(|line| line.name == name)
}

/// If `moves` is a prefix of (or equal to) a named line and has at least eight
/// plies (or equals the full line), return the most specific matching name.
pub fn opening_name_after(moves: &[&str]) -> Option<&'static str> {
    let mut best: Option<&RepertoireLine> = None;
    for line in lines() {
        if moves.len() > line.moves.len() {
            continue;
        }
        let is_prefix = line.moves[..moves.len()]
            .iter()
            .zip(moves.iter())
            .all(|(authored, played)| *authored == *played);
        if !is_prefix {
            continue;
        }
        if moves.len() < 8 && moves.len() != line.moves.len() {
            continue;
        }
        if best.is_none_or(|prev| line.moves.len() > prev.moves.len()) {
            best = Some(line);
        }
    }
    best.map(|line| line.name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    fn init() {
        lookup::initialize();
    }

    fn best_move_from_table(table: &BookTable, board: &Board) -> Option<String> {
        let candidates = table.get(&board.key())?;
        candidates
            .iter()
            .filter_map(|bm| {
                board
                    .parse_uci_move(&bm.uci)
                    .ok()
                    .map(|mv| (mv, bm.weight))
            })
            .max_by_key(|(_, weight)| *weight)
            .map(|(mv, _)| mv.to_string())
    }

    #[test]
    fn every_line_is_legal_and_deep_enough() {
        init();
        for line in lines() {
            assert!(
                line.moves.len() >= 8,
                "{} has only {} plies",
                line.name,
                line.moves.len()
            );
            assert!(line.weight > 0, "zero weight on {}", line.name);
            let mut board = Board::startpos();
            for uci in line.moves {
                let mv = board
                    .parse_uci_move(uci)
                    .unwrap_or_else(|_| panic!("illegal move {uci} in {}", line.name));
                assert!(
                    board.legal_moves().contains(&mv),
                    "{uci} not legal in {}",
                    line.name
                );
                board.make(mv);
            }
        }
    }

    #[test]
    fn mixed_table_covers_startpos_and_morphy_main_line() {
        init();
        let table = table(BookStyle::Mixed);
        let start = Board::startpos();
        assert!(
            table.contains_key(&start.key()),
            "mixed repertoire missing startpos"
        );

        let morphy = line_by_name("Ruy Lopez Morphy").expect("morphy line");
        let mut board = Board::startpos();
        for expected in morphy.moves {
            let best = best_move_from_table(&table, &board)
                .unwrap_or_else(|| panic!("no book move at tabiya before {expected}"));
            assert_eq!(
                best, *expected,
                "mixed table diverged from Morphy before playing {expected}"
            );
            board.make(board.parse_uci_move(expected).unwrap());
        }
        assert!(morphy.moves.len() >= 8);
    }

    #[test]
    fn validate_repertoire_merges_transposition_candidates() {
        init();
        let table = table(BookStyle::Solid);
        let nc3 = line_by_name("Four Knights (Nc3)").unwrap();
        let nf6 = line_by_name("Four Knights (Nf6 first)").unwrap();

        let mut via_nc3 = Board::startpos();
        for uci in &nc3.moves[..6] {
            via_nc3.make(via_nc3.parse_uci_move(uci).unwrap());
        }
        let mut via_nf6 = Board::startpos();
        for uci in &nf6.moves[..6] {
            via_nf6.make(via_nf6.parse_uci_move(uci).unwrap());
        }
        assert_eq!(
            via_nc3.key(),
            via_nf6.key(),
            "Four Knights transposition keys differ"
        );

        let entry = table
            .get(&via_nc3.key())
            .expect("transposed Four Knights position missing");
        let bc4 = entry
            .iter()
            .find(|bm| bm.uci == "f1c4")
            .expect("Bc4 missing at transposition");
        assert_eq!(
            bc4.weight,
            nc3.weight + nf6.weight,
            "transposed Bc4 weights should merge"
        );
    }

    #[test]
    fn opening_name_after_at_morphy_tabiya() {
        init();
        let morphy = line_by_name("Ruy Lopez Morphy").unwrap();
        let prefix: Vec<&str> = morphy.moves[..8].to_vec();
        assert_eq!(
            opening_name_after(&prefix),
            Some("Ruy Lopez Morphy"),
            "expected Morphy name at eight-ply tabiya"
        );
    }

    #[test]
    fn book_style_parse_and_as_str() {
        assert_eq!(BookStyle::parse("solid"), BookStyle::Solid);
        assert_eq!(BookStyle::parse("AGGRESSIVE"), BookStyle::Aggressive);
        assert_eq!(BookStyle::parse("mixed"), BookStyle::Mixed);
        assert_eq!(BookStyle::parse("unknown"), BookStyle::Mixed);
        assert_eq!(BookStyle::Solid.as_str(), "solid");
    }

    #[test]
    fn line_by_name_finds_authored_lines() {
        assert!(line_by_name("Sicilian Najdorf").is_some());
        assert!(line_by_name("Not an opening").is_none());
    }

    #[test]
    fn solid_table_omits_aggressive_only_lines() {
        init();
        let solid = table(BookStyle::Solid);
        let italian = line_by_name("Italian Game").unwrap();
        let mut board = Board::startpos();
        for uci in &italian.moves[..6] {
            board.make(board.parse_uci_move(uci).unwrap());
        }
        let entry = solid.get(&board.key());
        let has_italian_continuation = entry.is_some_and(|candidates| {
            candidates.iter().any(|bm| bm.uci == italian.moves[6])
        });
        assert!(
            !has_italian_continuation,
            "solid table should not carry Italian-only branches"
        );
    }
}
