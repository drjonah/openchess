//! Zobrist hashing keys.
//!
//! Tables are built once from a fixed-seed [`SplitMix64`] PRNG so that keys
//! are stable across runs (useful for reproducing perft/search results and
//! for transposition table debugging). Callers combine keys by XOR:
//!
//! - one key per (piece, square) pair
//! - one key XORed in whenever Black is to move
//! - one key per castling-rights bitset (16 entries, including "no rights")
//! - one key per en passant file, XORed in only when an EP square is live
//!
//! [`Board::compute_key`](crate::board::Board::compute_key) does a full
//! from-scratch rehash; [`Board::make`](crate::board::Board::make) and
//! [`Board::unmake`](crate::board::Board::unmake) keep the key incrementally
//! in sync with that definition.

use crate::types::moves::CastlingRights;
use crate::types::piece::{Piece, PieceType};
use crate::types::square::Square;
use std::sync::OnceLock;

/// 64-bit position hash key.
pub type Key = u64;

/// Fixed seed so keys (and therefore hashes) are stable across runs.
const SEED: u64 = 0x5EED_C0FF_EE12_3456;

/// Minimal SplitMix64 PRNG, used only to fill the Zobrist tables once.
struct SplitMix64(u64);

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        SplitMix64(seed)
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

/// Number of distinct colored piece kinds (6 piece types × 2 colors).
const PIECE_KINDS: usize = PieceType::COUNT * 2;

struct ZobristTables {
    pieces: [[Key; Square::COUNT]; PIECE_KINDS],
    side: Key,
    castling: [Key; 16],
    ep_file: [Key; 8],
}

static TABLES: OnceLock<ZobristTables> = OnceLock::new();

/// Build the Zobrist tables once. Safe to call multiple times.
pub fn initialize() {
    let _ = TABLES.get_or_init(build_tables);
}

fn tables() -> &'static ZobristTables {
    initialize();
    TABLES.get().expect("zobrist tables initialized")
}

fn build_tables() -> ZobristTables {
    let mut rng = SplitMix64::new(SEED);

    let mut pieces = [[0u64; Square::COUNT]; PIECE_KINDS];
    for kind in pieces.iter_mut() {
        for key in kind.iter_mut() {
            *key = rng.next_u64();
        }
    }

    let side = rng.next_u64();

    let mut castling = [0u64; 16];
    for key in castling.iter_mut() {
        *key = rng.next_u64();
    }

    let mut ep_file = [0u64; 8];
    for key in ep_file.iter_mut() {
        *key = rng.next_u64();
    }

    ZobristTables {
        pieces,
        side,
        castling,
        ep_file,
    }
}

/// Index into the `pieces` table for a colored piece: `color * 6 + piece_type`.
#[inline]
fn piece_index(piece: Piece) -> usize {
    let color = piece
        .color()
        .expect("zobrist::piece_key: Piece::Empty has no key");
    let piece_type = piece.piece_type().expect("non-empty piece has a type");
    color.index() * PieceType::COUNT + piece_type.index()
}

/// Key for a colored piece standing on `sq`. Panics if `piece` is [`Piece::Empty`].
#[inline]
pub fn piece_key(piece: Piece, sq: Square) -> Key {
    tables().pieces[piece_index(piece)][sq.index() as usize]
}

/// Key XORed into the position hash whenever Black is to move.
#[inline]
pub fn side_key() -> Key {
    tables().side
}

/// Key for a full castling-rights bitset (0..16, including [`CastlingRights::NONE`]).
#[inline]
pub fn castling_key(rights: CastlingRights) -> Key {
    tables().castling[rights.bits() as usize]
}

/// Key for an en passant file (0..8). Only XORed in when an EP square is live.
#[inline]
pub fn ep_key(file: u8) -> Key {
    debug_assert!(file < 8, "ep_key: file {file} out of range");
    tables().ep_file[file as usize]
}
