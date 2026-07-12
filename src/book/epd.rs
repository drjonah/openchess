//! EPD-keyed opening book derived from `testing/books/openings.epd` (P10-03).
//!
//! The EPD file lists named opening positions (aligned with the P8-03 SPRT
//! suite) but carries no explicit book moves. We reconstruct a move graph: for
//! every listed position, any single legal move that reaches another listed
//! position becomes a weighted book edge. This links, e.g., the start position
//! to `1.e4`/`1.d4`/`1.Nf3`, and `1.e4` to the Sicilian / French / Caro-Kann
//! positions, without hand-writing keys.

use super::{BookMove, BookTable};
use crate::board::Board;
use crate::types::Key;
use std::collections::HashSet;

/// Embedded copy of the SPRT smoke book.
const OPENINGS_EPD: &str = include_str!("../../testing/books/openings.epd");

/// Weight for a derived EPD edge. Low, so mini-book human weights dominate where
/// both books cover the same position.
const EDGE_WEIGHT: u32 = 10;

/// Build the EPD-derived book table. Requires [`crate::lookup::initialize`].
pub fn embedded_table() -> BookTable {
    build_from_epd(OPENINGS_EPD)
}

fn build_from_epd(text: &str) -> BookTable {
    let boards: Vec<Board> = text.lines().filter_map(parse_epd_line).collect();
    let keys: HashSet<Key> = boards.iter().map(Board::key).collect();

    let mut table = BookTable::new();
    for board in &boards {
        for mv in board.legal_moves() {
            let mut child = board.clone();
            child.make(mv);
            if keys.contains(&child.key()) {
                table
                    .entry(board.key())
                    .or_default()
                    .push(BookMove::new(mv.to_string(), EDGE_WEIGHT));
            }
        }
    }
    table
}

/// Parse one EPD line into a [`Board`], honoring the non-standard `hmvc` /
/// `fmvn` opcodes used by `openings.epd`. Returns `None` for blank/invalid rows.
fn parse_epd_line(line: &str) -> Option<Board> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    let mut segments = line.split(';');
    let first = segments.next()?.trim();

    let mut toks = first.split_whitespace();
    let placement = toks.next()?;
    let stm = toks.next()?;
    let castle = toks.next()?;
    let ep = toks.next()?;

    // Trailing "hmvc N" opcode in the first segment.
    let mut halfmove = "0";
    while let Some(t) = toks.next() {
        if t == "hmvc" {
            if let Some(v) = toks.next() {
                halfmove = v;
            }
        }
    }

    // "fmvn N" opcode in a later segment.
    let mut fullmove = "1";
    for seg in segments {
        if let Some(rest) = seg.trim().strip_prefix("fmvn") {
            let v = rest.trim();
            if !v.is_empty() {
                fullmove = v;
            }
        }
    }

    let fen = format!("{placement} {stm} {castle} {ep} {halfmove} {fullmove}");
    Board::from_fen(&fen).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    #[test]
    fn parses_startpos_line() {
        lookup::initialize();
        let board = parse_epd_line(
            "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - hmvc 0; fmvn 1; id \"startpos\";",
        )
        .unwrap();
        assert_eq!(board.key(), Board::startpos().key());
    }

    #[test]
    fn startpos_links_to_main_first_moves() {
        lookup::initialize();
        let table = embedded_table();
        let start = Board::startpos();
        let edges = table.get(&start.key()).expect("startpos has EPD edges");
        let ucis: Vec<&str> = edges.iter().map(|bm| bm.uci.as_str()).collect();
        // The EPD set contains the e4/d4/Nf3 one-move positions.
        for expected in ["e2e4", "d2d4", "g1f3"] {
            assert!(ucis.contains(&expected), "missing EPD edge {expected}");
        }
    }

    #[test]
    fn e4_links_to_sicilian() {
        lookup::initialize();
        let table = embedded_table();
        let mut b = Board::startpos();
        b.make(b.parse_uci_move("e2e4").unwrap());
        let edges = table.get(&b.key()).expect("1.e4 has EPD edges");
        let ucis: Vec<&str> = edges.iter().map(|bm| bm.uci.as_str()).collect();
        // 1.e4 c5 (Sicilian) is in the suite.
        assert!(ucis.contains(&"c7c5"), "no Sicilian edge from 1.e4");
    }
}
