//! Embedded mini opening book for the first plies (P10-01).
//!
//! A weighted first-move table for White plus main replies keyed on White's
//! first move. Positions are keyed by Zobrist hash, computed by replaying the
//! move sequence from the start position, so no FEN or key literals are hard
//! coded. Weights are rough human-frequency proportions (variety over strict
//! theory strength); real selection is weighted-random in [`super::Book::probe`].

use super::{BookMove, BookTable};
use crate::board::Board;

/// `(moves from startpos, [(book move uci, weight)])`.
type Line = (&'static [&'static str], &'static [(&'static str, u32)]);

const LINES: &[Line] = &[
    // White to move (ply 0): the four principled first moves.
    (
        &[],
        &[("e2e4", 40), ("d2d4", 35), ("g1f3", 15), ("c2c4", 10)],
    ),
    // Black replies (ply 1), keyed on White's first move.
    (
        &["e2e4"],
        &[
            ("c7c5", 30), // Sicilian
            ("e7e5", 28), // Open games
            ("e7e6", 14), // French
            ("c7c6", 12), // Caro-Kann
            ("d7d5", 8),  // Scandinavian
            ("g7g6", 8),  // Modern
        ],
    ),
    (
        &["d2d4"],
        &[
            ("g8f6", 40), // Indian defences
            ("d7d5", 35), // Closed / QGD family
            ("e7e6", 15),
            ("g7g6", 10),
        ],
    ),
    (
        &["g1f3"],
        &[
            ("d7d5", 35),
            ("g8f6", 35),
            ("c7c5", 20),
            ("g7g6", 10),
        ],
    ),
    (
        &["c2c4"],
        &[
            ("e7e5", 30), // Reversed Sicilian
            ("g8f6", 30),
            ("c7c5", 20), // Symmetrical English
            ("e7e6", 20),
        ],
    ),
];

/// Build the mini book table. Requires [`crate::lookup::initialize`].
pub fn table() -> BookTable {
    let mut table = BookTable::new();
    for (moves, candidates) in LINES {
        let mut board = Board::startpos();
        let mut ok = true;
        for uci in *moves {
            match board.parse_uci_move(uci) {
                Ok(mv) => board.make(mv),
                Err(_) => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok {
            continue;
        }
        let entry: Vec<BookMove> = candidates
            .iter()
            .map(|(uci, weight)| BookMove::new(*uci, *weight))
            .collect();
        table.insert(board.key(), entry);
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;

    #[test]
    fn table_has_startpos_and_first_move_replies() {
        lookup::initialize();
        let table = table();
        let start = Board::startpos();
        assert!(table.contains_key(&start.key()), "startpos missing");

        // Position after each White first move must have Black replies.
        for uci in ["e2e4", "d2d4", "g1f3", "c2c4"] {
            let mut b = Board::startpos();
            b.make(b.parse_uci_move(uci).unwrap());
            assert!(table.contains_key(&b.key()), "no replies keyed after {uci}");
        }
    }

    #[test]
    fn startpos_never_lists_a_flank_pawn() {
        lookup::initialize();
        let table = table();
        let start = Board::startpos();
        let entry = table.get(&start.key()).unwrap();
        for bm in entry {
            assert!(
                !matches!(bm.uci.as_str(), "a2a3" | "a2a4" | "h2h3" | "h2h4"),
                "mini book lists flank pawn {}",
                bm.uci
            );
        }
    }
}
