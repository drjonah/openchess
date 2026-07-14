//! Polyglot `.bin` opening-book loader (P10-05).
//!
//! Polyglot uses its own 781-entry Zobrist scheme, distinct from
//! [`crate::board::Board::key`]. Entries are 16 bytes (big-endian):
//! `key:u64`, `move:u16`, `weight:u16`, `learn:u32`. Castling is encoded as
//! king-from → rook-from (`e1h1` / `e1a1` / …) and must be mapped to OpenChess
//! king-destination castling moves (`e1g1` / `e1c1` / …).

mod random {
    include!("polyglot_random.rs");
}

use super::{BookMove, BookRng};
use crate::board::Board;
use crate::lookup;
use crate::types::{CastlingRights, Color, Move, Piece, PieceType, Square};
use std::fs;
use std::path::Path;
use std::sync::Arc;

/// One Polyglot book entry (learn field discarded).
#[derive(Clone, Copy, Debug)]
struct Entry {
    key: u64,
    raw_move: u16,
    weight: u16,
}

/// Sorted-by-key Polyglot book, probed with [`polyglot_key`].
#[derive(Clone, Debug)]
pub struct PolyglotBook {
    entries: Arc<[Entry]>,
}

impl PolyglotBook {
    /// Load a `.bin` file. Empty / truncated / unreadable files error out so
    /// callers can fall back to the embedded default book.
    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        Self::from_bytes(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Parse raw Polyglot bytes (public for tests / fixtures).
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        if bytes.is_empty() {
            return Err("empty polyglot book".into());
        }
        if bytes.len() % 16 != 0 {
            return Err(format!(
                "corrupt polyglot book ({} bytes, not a multiple of 16)",
                bytes.len()
            ));
        }
        let mut entries = Vec::with_capacity(bytes.len() / 16);
        for chunk in bytes.chunks_exact(16) {
            let key = u64::from_be_bytes(chunk[0..8].try_into().unwrap());
            let raw_move = u16::from_be_bytes(chunk[8..10].try_into().unwrap());
            let weight = u16::from_be_bytes(chunk[10..12].try_into().unwrap());
            // learn = chunk[12..16] — unused
            if weight > 0 {
                entries.push(Entry {
                    key,
                    raw_move,
                    weight,
                });
            }
        }
        if entries.is_empty() {
            return Err("polyglot book has no positive-weight entries".into());
        }
        // Spec requires ascending key order; sort defensively.
        entries.sort_by_key(|e| e.key);
        Ok(Self {
            entries: entries.into(),
        })
    }

    /// Weighted random legal move for `board`, or `None` on miss.
    pub fn probe(&self, board: &Board, rng: &mut BookRng) -> Option<Move> {
        let resolved = self.resolve_legal(board);
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

    /// Highest-weight legal move (deterministic).
    pub fn best_move(&self, board: &Board) -> Option<Move> {
        self.resolve_legal(board)
            .into_iter()
            .max_by_key(|(_, w)| *w)
            .map(|(mv, _)| mv)
    }

    pub fn is_book_move(&self, board: &Board, mv: Move) -> bool {
        self.resolve_legal(board)
            .into_iter()
            .any(|(candidate, _)| candidate == mv)
    }

    /// Convert to a Zobrist-keyed [`BookTable`] for the given position set by
    /// probing each unique key against… — not used; Polyglot stays native.
    ///
    /// Export candidates for the current position as UCI strings (tests).
    pub fn candidates_uci(&self, board: &Board) -> Vec<BookMove> {
        self.resolve_legal(board)
            .into_iter()
            .map(|(mv, w)| BookMove::new(mv.to_string(), w))
            .collect()
    }

    fn resolve_legal(&self, board: &Board) -> Vec<(Move, u32)> {
        let key = polyglot_key(board);
        let range = self.entries_for(key);
        let mut out = Vec::new();
        for entry in range {
            if let Some(mv) = decode_move(board, entry.raw_move) {
                out.push((mv, u32::from(entry.weight)));
            }
        }
        out
    }

    fn entries_for(&self, key: u64) -> &[Entry] {
        let start = self
            .entries
            .partition_point(|e| e.key < key);
        let end = start
            + self.entries[start..]
                .partition_point(|e| e.key == key);
        &self.entries[start..end]
    }
}

/// Polyglot Zobrist key for `board` (not [`Board::key`]).
pub fn polyglot_key(board: &Board) -> u64 {
    let mut key: u64 = 0;

    for sq in Square::all() {
        let piece = board.piece_on(sq);
        if piece.is_empty() {
            continue;
        }
        let kind = polyglot_piece_kind(piece);
        let idx = 64 * kind + sq.index() as usize;
        key ^= random::RANDOM64[idx];
    }

    let rights = board.castling_rights();
    if rights.contains(CastlingRights::WHITE_KING) {
        key ^= random::RANDOM64[768];
    }
    if rights.contains(CastlingRights::WHITE_QUEEN) {
        key ^= random::RANDOM64[769];
    }
    if rights.contains(CastlingRights::BLACK_KING) {
        key ^= random::RANDOM64[770];
    }
    if rights.contains(CastlingRights::BLACK_QUEEN) {
        key ^= random::RANDOM64[771];
    }

    if let Some(ep) = board.ep_square() {
        if ep_capture_possible(board, ep) {
            key ^= random::RANDOM64[772 + ep.file() as usize];
        }
    }

    // Polyglot XORs the side key when White is to move.
    if board.side_to_move() == Color::White {
        key ^= random::RANDOM64[780];
    }

    key
}

fn polyglot_piece_kind(piece: Piece) -> usize {
    // 0=black pawn, 1=white pawn, 2=black knight, …, 11=white king
    let (color, pt) = match piece {
        Piece::BlackPawn => (0, 0),
        Piece::WhitePawn => (1, 0),
        Piece::BlackKnight => (0, 1),
        Piece::WhiteKnight => (1, 1),
        Piece::BlackBishop => (0, 2),
        Piece::WhiteBishop => (1, 2),
        Piece::BlackRook => (0, 3),
        Piece::WhiteRook => (1, 3),
        Piece::BlackQueen => (0, 4),
        Piece::WhiteQueen => (1, 4),
        Piece::BlackKing => (0, 5),
        Piece::WhiteKing => (1, 5),
        Piece::Empty => unreachable!(),
    };
    2 * pt + color
}

fn ep_capture_possible(board: &Board, ep: Square) -> bool {
    let us = board.side_to_move();
    // Squares from which a pawn of `us` attacks `ep` equal the attack set of
    // an opposite-color pawn sitting on `ep`.
    let sources = lookup::pawn_attacks(!us, ep);
    let our_pawns = board.pieces(PieceType::Pawn) & board.pieces_color(us);
    !(our_pawns & sources).is_empty()
}

/// Decode a Polyglot move encoding into a legal OpenChess [`Move`].
fn decode_move(board: &Board, raw: u16) -> Option<Move> {
    let from_idx = ((raw >> 6) & 0x3F) as u8;
    let to_idx = (raw & 0x3F) as u8;
    let promo_code = (raw >> 12) & 0x7;

    let from = Square::new(from_idx)?;
    let to = Square::new(to_idx)?;

    // Castling: king-from → rook-from → map to king destination.
    let (mapped_to, is_castle) = match (from, to) {
        (Square::E1, Square::H1) => (Square::G1, true),
        (Square::E1, Square::A1) => (Square::C1, true),
        (Square::E8, Square::H8) => (Square::G8, true),
        (Square::E8, Square::A8) => (Square::C8, true),
        _ => (to, false),
    };

    let promo = match promo_code {
        1 => Some(PieceType::Knight),
        2 => Some(PieceType::Bishop),
        3 => Some(PieceType::Rook),
        4 => Some(PieceType::Queen),
        _ => None,
    };

    let legal = board.legal_moves();
    legal.into_iter().find(|mv| {
        if mv.from() != from || mv.to() != mapped_to {
            return false;
        }
        if is_castle {
            return mv.is_castling();
        }
        match promo {
            Some(pt) => mv.promotion_piece() == Some(pt),
            None => !mv.is_promotion(),
        }
    })
}

/// Write a minimal Polyglot book for tests (big-endian entries, sorted).
pub fn write_bytes(entries: &[(u64, u16, u16)]) -> Vec<u8> {
    let mut sorted = entries.to_vec();
    sorted.sort_by_key(|(k, _, _)| *k);
    let mut out = Vec::with_capacity(sorted.len() * 16);
    for (key, mv, weight) in sorted {
        out.extend_from_slice(&key.to_be_bytes());
        out.extend_from_slice(&mv.to_be_bytes());
        out.extend_from_slice(&weight.to_be_bytes());
        out.extend_from_slice(&0u32.to_be_bytes()); // learn
    }
    out
}

/// Encode a non-castling, non-promo UCI-like from/to into Polyglot raw move.
pub fn encode_quiet(from: Square, to: Square) -> u16 {
    ((from.index() as u16) << 6) | (to.index() as u16)
}

/// Encode Polyglot castling (king → rook square).
pub fn encode_castle(from: Square, rook: Square) -> u16 {
    encode_quiet(from, rook)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use std::io::Write;
    use std::str::FromStr;

    fn init() {
        lookup::initialize();
    }

    #[test]
    fn startpos_key_matches_polyglot_spec() {
        init();
        let key = polyglot_key(&Board::startpos());
        assert_eq!(key, 0x463B_9618_1691_FC9C, "got {key:#018X}");
    }

    #[test]
    fn after_e4_key_differs_and_omits_impossible_ep() {
        init();
        let mut board = Board::startpos();
        let before = polyglot_key(&board);
        board.make(board.parse_uci_move("e2e4").unwrap());
        let after = polyglot_key(&board);
        assert_ne!(before, after);
        // 1.e4 sets EP to e3, but no Black pawn can capture there, so Polyglot
        // must not XOR the e-file EP key (spec: only when a capture is possible).
        assert_eq!(board.ep_square().map(|s| s.to_string()).as_deref(), Some("e3"));
        assert!(!ep_capture_possible(&board, board.ep_square().unwrap()));
        // Stable golden for this implementation (startpos key already checked).
        assert_eq!(after, 0x823C_9B50_FD11_4196);
    }

    #[test]
    fn loads_fixture_and_probes_e4() {
        init();
        let start = Board::startpos();
        let key = polyglot_key(&start);
        let e2 = Square::from_str("e2").unwrap();
        let e4 = Square::from_str("e4").unwrap();
        let d2 = Square::from_str("d2").unwrap();
        let d4 = Square::from_str("d4").unwrap();
        let bytes = write_bytes(&[
            (key, encode_quiet(e2, e4), 40),
            (key, encode_quiet(d2, d4), 20),
        ]);

        let dir = std::env::temp_dir().join("openchess_polyglot_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("mini.bin");
        {
            let mut f = fs::File::create(&path).unwrap();
            f.write_all(&bytes).unwrap();
        }

        let book = PolyglotBook::load(&path).unwrap();
        let mut rng = BookRng::from_seed(1);
        let mv = book.probe(&start, &mut rng).expect("fixture hit");
        assert!(matches!(mv.to_string().as_str(), "e2e4" | "d2d4"));

        let best = book.best_move(&start).unwrap();
        assert_eq!(best.to_string(), "e2e4");

        assert!(PolyglotBook::from_bytes(&[1, 2, 3]).is_err());
        assert!(PolyglotBook::load(Path::new("/no/such/book.bin")).is_err());
    }

    #[test]
    fn castling_decode_maps_rook_target() {
        init();
        let fen = "r3k2r/8/8/8/8/8/8/R3K2R w KQkq - 0 1";
        let board = Board::from_fen(fen).unwrap();
        let ks = decode_move(&board, encode_castle(Square::E1, Square::H1)).unwrap();
        assert!(ks.is_castling());
        assert_eq!(ks.to_string(), "e1g1");
        let qs = decode_move(&board, encode_castle(Square::E1, Square::A1)).unwrap();
        assert!(qs.is_castling());
        assert_eq!(qs.to_string(), "e1c1");
    }
}
