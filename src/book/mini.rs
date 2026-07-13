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
    // White's second move after common Black replies (ply 2).
    (
        &["e2e4", "e7e5"],
        &[("g1f3", 75), ("b1c3", 15), ("d2d4", 10)],
    ),
    (
        &["e2e4", "c7c5"],
        &[("g1f3", 80), ("b1c3", 15), ("d2d4", 5)],
    ),
    (
        &["e2e4", "e7e6"],
        &[("d2d4", 90), ("b1c3", 5), ("g1f3", 5)],
    ),
    (
        &["e2e4", "c7c6"],
        &[("d2d4", 90), ("b1c3", 5), ("g1f3", 5)],
    ),
    (
        &["e2e4", "d7d5"],
        &[("e4d5", 70), ("b1c3", 20), ("d2d4", 10)],
    ),
    (
        &["e2e4", "g7g6"],
        &[("d2d4", 75), ("d2d3", 15), ("g1f3", 10)],
    ),
    (
        &["d2d4", "g8f6"],
        &[("c2c4", 55), ("g1f3", 30), ("c2c3", 10), ("b1c3", 5)],
    ),
    (
        &["d2d4", "d7d5"],
        &[("c2c4", 60), ("g1f3", 25), ("c2c3", 10), ("b1c3", 5)],
    ),
    (
        &["d2d4", "e7e6"],
        &[("c2c4", 70), ("g1f3", 20), ("b1c3", 10)],
    ),
    (
        &["d2d4", "g7g6"],
        &[("c2c4", 60), ("g1f3", 30), ("b1c3", 10)],
    ),
    (
        &["g1f3", "d7d5"],
        &[("d2d4", 45), ("c2c4", 40), ("g2g3", 10), ("b1c3", 5)],
    ),
    (
        &["g1f3", "g8f6"],
        &[("c2c4", 55), ("d2d4", 30), ("b1c3", 10), ("g2g3", 5)],
    ),
    (
        &["c2c4", "e7e5"],
        &[("b1c3", 45), ("g1f3", 35), ("d2d4", 15), ("g2g3", 5)],
    ),
    (
        &["c2c4", "g8f6"],
        &[("b1c3", 45), ("g1f3", 35), ("e2e4", 15), ("g2g3", 5)],
    ),
    (
        &["c2c4", "c7c5"],
        &[("g1f3", 55), ("d2d4", 30), ("b1c3", 10), ("e2e4", 5)],
    ),
    // Black's second move after principled White second moves (ply 3).
    (
        &["e2e4", "e7e5", "g1f3"],
        &[("b8c6", 80), ("g8f6", 15), ("d7d6", 5)],
    ),
    (
        &["e2e4", "c7c5", "g1f3"],
        &[("d7d6", 55), ("b8c6", 30), ("e7e6", 10), ("g7g6", 5)],
    ),
    (
        &["e2e4", "e7e6", "d2d4"],
        &[("d7d5", 90), ("c7c5", 5), ("g8f6", 5)],
    ),
    (
        &["e2e4", "c7c6", "d2d4"],
        &[("d7d5", 90), ("g8f6", 5), ("e7e6", 5)],
    ),
    (
        &["d2d4", "g8f6", "c2c4"],
        &[("e7e6", 45), ("g7g6", 35), ("d7d5", 15), ("c7c5", 5)],
    ),
    (
        &["d2d4", "d7d5", "c2c4"],
        &[("e7e6", 50), ("c7c6", 25), ("d5c4", 15), ("g8f6", 10)],
    ),
    // White's third move in the main open-game and Sicilian lines (ply 4).
    (
        &["e2e4", "e7e5", "g1f3", "b8c6"],
        &[("f1b5", 45), ("f1c4", 35), ("d2d4", 15), ("b1c3", 5)],
    ),
    (
        &["e2e4", "c7c5", "g1f3", "d7d6"],
        &[("d2d4", 60), ("b1c3", 25), ("c2c3", 15)],
    ),
    (
        &["e2e4", "e7e6", "d2d4", "d7d5"],
        &[("b1c3", 50), ("e4e5", 30), ("g1f3", 15), ("c2c3", 5)],
    ),
    (
        &["d2d4", "g8f6", "c2c4", "e7e6"],
        &[("b1c3", 45), ("g1f3", 35), ("a2a3", 10), ("f2f3", 10)],
    ),
    // Black's third move in the Ruy Lopez and Open Sicilian (ply 5).
    (
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5"],
        &[("a7a6", 85), ("g8f6", 10), ("f8c5", 5)],
    ),
    (
        &["e2e4", "c7c5", "g1f3", "d7d6", "d2d4"],
        &[("c5d4", 90), ("g8f6", 5), ("b8c6", 5)],
    ),
    // White's fourth move in the Morphy Ruy Lopez (ply 6).
    (
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6"],
        &[("b5a4", 90), ("b5c6", 5), ("b5d3", 5)],
    ),
    // Black's fourth move vs the Morphy Defence (ply 7).
    (
        &["e2e4", "e7e5", "g1f3", "b8c6", "f1b5", "a7a6", "b5a4"],
        &[("g8f6", 85), ("b7b5", 10), ("f8e7", 5)],
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

    #[test]
    fn authored_lines_are_legal() {
        lookup::initialize();
        for (moves, candidates) in LINES {
            let mut board = Board::startpos();
            for uci in *moves {
                let mv = board
                    .parse_uci_move(uci)
                    .unwrap_or_else(|_| panic!("illegal prefix move {uci}"));
                board.make(mv);
            }
            let legal = board.legal_moves();
            for (uci, weight) in *candidates {
                assert!(*weight > 0, "zero weight for {uci}");
                let mv = board
                    .parse_uci_move(uci)
                    .unwrap_or_else(|_| panic!("illegal book move {uci}"));
                assert!(legal.contains(&mv), "book move {uci} not legal");
            }
        }
    }

    #[test]
    fn open_game_has_white_third_move() {
        lookup::initialize();
        let table = table();
        let mut board = Board::startpos();
        for uci in ["e2e4", "e7e5", "g1f3", "b8c6"] {
            board.make(board.parse_uci_move(uci).unwrap());
        }
        let entry = table.get(&board.key()).expect("1.e4 e5 2.Nf3 Nc6 missing");
        let ucis: Vec<&str> = entry.iter().map(|bm| bm.uci.as_str()).collect();
        assert!(ucis.contains(&"f1b5"), "Ruy Lopez missing from mini book");
        assert!(ucis.contains(&"f1c4"), "Italian missing from mini book");
    }
}
