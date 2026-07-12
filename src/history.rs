//! History tables for move ordering (P3-03 / P3-04) and eval corrections (P6-07).
//!
//! - Butterfly quiet history: `[Color][from][to]`
//! - Capture history: `[piece][to][captured_type]`
//! - Continuation history: `[prev_piece][prev_to][piece][to]`
//! - Pawn history: `[pawn_key][piece][to]` for quiet ordering
//! - Correction history: pawn / non-pawn residuals vs static eval

use crate::board::Board;
use crate::types::{Color, Move, Piece, PieceType, Square, Value};

/// Soft clamp for history entries (Stockfish-family gravity scale).
pub const HISTORY_MAX: i32 = 16384;

/// Number of non-empty piece slots (`WhitePawn`..=`BlackKing`).
pub const PIECE_SLOTS: usize = 12;

/// Sentinel piece slot for null / root continuation context.
pub const CONT_NONE_PIECE: u8 = PIECE_SLOTS as u8;

/// Continuation context for a prior move (`prev_piece`, `prev_to`).
///
/// Stored on [`crate::search::stack::Stack`] at `ply+1` after a real make;
/// sentinel after null moves / at root.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContSlot {
    pub prev_piece: u8,
    pub prev_to: u8,
}

impl ContSlot {
    pub const NONE: Self = Self {
        prev_piece: CONT_NONE_PIECE,
        prev_to: 0,
    };

    #[inline]
    pub fn new(piece: Piece, to: Square) -> Self {
        Self {
            prev_piece: piece.slot_index() as u8,
            prev_to: to.index(),
        }
    }

    #[inline]
    pub fn is_valid(self) -> bool {
        (self.prev_piece as usize) < PIECE_SLOTS
    }
}

impl Default for ContSlot {
    fn default() -> Self {
        Self::NONE
    }
}

/// Ply offsets used for continuation history (counter / follow-up / deeper).
pub const CONT_PLIES: [usize; 4] = [1, 2, 4, 6];

const CONT_LEN: usize = PIECE_SLOTS * 64 * PIECE_SLOTS * 64;

/// Buckets for pawn-keyed tables (pawn history + pawn correction).
const PAWN_BUCKETS: usize = 16_384;
const PAWN_MASK: u64 = (PAWN_BUCKETS as u64) - 1;

const CORR_MAX: i32 = 1024;

/// Butterfly + capture + continuation + pawn + correction history tables.
#[derive(Clone, Debug)]
pub struct HistoryTables {
    /// `[color][from][to]`
    butterfly: [[[i16; 64]; 64]; Color::COUNT],
    /// `[piece_slot][to][captured_type]`
    capture: [[[i16; PieceType::COUNT]; 64]; PIECE_SLOTS],
    /// Flat `[prev_piece][prev_to][piece][to]` (heap-backed to avoid stack blowups).
    continuation: Vec<i16>,
    /// `[pawn_bucket][piece_slot][to]` — quiet moves keyed by pawn structure.
    pawn_history: Vec<i16>,
    /// Pawn-structure correction residual (STM-relative after sign).
    pawn_corr: Vec<i16>,
    /// Non-pawn material/placement correction residual per side.
    nonpawn_corr: [Vec<i16>; Color::COUNT],
}

impl Default for HistoryTables {
    fn default() -> Self {
        Self::new()
    }
}

impl HistoryTables {
    pub fn new() -> Self {
        Self {
            butterfly: [[[0; 64]; 64]; Color::COUNT],
            capture: [[[0; PieceType::COUNT]; 64]; PIECE_SLOTS],
            continuation: vec![0; CONT_LEN],
            pawn_history: vec![0; PAWN_BUCKETS * PIECE_SLOTS * 64],
            pawn_corr: vec![0; PAWN_BUCKETS],
            nonpawn_corr: [vec![0; PAWN_BUCKETS], vec![0; PAWN_BUCKETS]],
        }
    }

    pub fn clear(&mut self) {
        self.butterfly = [[[0; 64]; 64]; Color::COUNT];
        self.capture = [[[0; PieceType::COUNT]; 64]; PIECE_SLOTS];
        self.continuation.fill(0);
        self.pawn_history.fill(0);
        self.pawn_corr.fill(0);
        self.nonpawn_corr[0].fill(0);
        self.nonpawn_corr[1].fill(0);
    }

    // --- Butterfly ---

    #[inline]
    pub fn get(&self, color: Color, mv: Move) -> i32 {
        self.butterfly[color.index()][mv.from().index() as usize][mv.to().index() as usize] as i32
    }

    /// Gravity update: `entry += bonus - entry * |bonus| / HISTORY_MAX`.
    #[inline]
    pub fn update(&mut self, color: Color, mv: Move, bonus: i32) {
        let entry =
            &mut self.butterfly[color.index()][mv.from().index() as usize][mv.to().index() as usize];
        apply_gravity(entry, bonus);
    }

    // --- Capture ---

    #[inline]
    pub fn capture_get(&self, piece: Piece, to: Square, captured: PieceType) -> i32 {
        self.capture[piece.slot_index()][to.index() as usize][captured.index()] as i32
    }

    #[inline]
    pub fn capture_update(&mut self, piece: Piece, to: Square, captured: PieceType, bonus: i32) {
        let entry = &mut self.capture[piece.slot_index()][to.index() as usize][captured.index()];
        apply_gravity(entry, bonus);
    }

    // --- Continuation ---

    #[inline]
    pub fn cont_get(&self, slot: ContSlot, piece: Piece, to: Square) -> i32 {
        if !slot.is_valid() {
            return 0;
        }
        self.continuation[cont_index(
            slot.prev_piece as usize,
            slot.prev_to as usize,
            piece.slot_index(),
            to.index() as usize,
        )] as i32
    }

    #[inline]
    pub fn cont_update(&mut self, slot: ContSlot, piece: Piece, to: Square, bonus: i32) {
        if !slot.is_valid() {
            return;
        }
        let entry = &mut self.continuation[cont_index(
            slot.prev_piece as usize,
            slot.prev_to as usize,
            piece.slot_index(),
            to.index() as usize,
        )];
        apply_gravity(entry, bonus);
    }

    /// Sum continuation history at offsets 1, 2, 4, 6.
    ///
    /// `cont_slots[i]` must be the slot for offset `CONT_PLIES[i]` (see
    /// [`continuation_slots`]).
    #[inline]
    pub fn continuation_score(&self, cont_slots: &[ContSlot; 4], piece: Piece, to: Square) -> i32 {
        let mut sum = 0;
        for &slot in cont_slots {
            sum += self.cont_get(slot, piece, to);
        }
        sum
    }

    /// Update continuation at offsets 1, 2, 4, 6 for `(piece, to)`.
    #[inline]
    pub fn update_continuation(
        &mut self,
        cont_slots: &[ContSlot; 4],
        piece: Piece,
        to: Square,
        bonus: i32,
    ) {
        for &slot in cont_slots {
            self.cont_update(slot, piece, to, bonus);
        }
    }

    // --- Pawn history (P3-04) ---

    #[inline]
    pub fn pawn_get(&self, board: &Board, piece: Piece, to: Square) -> i32 {
        let idx = pawn_hist_index(pawn_structure_key(board), piece.slot_index(), to.index() as usize);
        self.pawn_history[idx] as i32
    }

    #[inline]
    pub fn pawn_update(&mut self, board: &Board, piece: Piece, to: Square, bonus: i32) {
        let idx = pawn_hist_index(pawn_structure_key(board), piece.slot_index(), to.index() as usize);
        apply_gravity(&mut self.pawn_history[idx], bonus);
    }

    // --- Correction history (P6-07) ---

    /// STM-relative correction residual from pawn + non-pawn tables.
    #[inline]
    pub fn correction_score(&self, board: &Board) -> Value {
        let stm = board.side_to_move();
        let pk = pawn_structure_key(board);
        let npk = nonpawn_structure_key(board, stm);
        let pawn = self.pawn_corr[(pk & PAWN_MASK) as usize] as i32;
        let nonpawn = self.nonpawn_corr[stm.index()][(npk & PAWN_MASK) as usize] as i32;
        // Blend; keep small so raw NNUE dominates.
        ((pawn + nonpawn) / 2) as Value
    }

    /// Gravity-update correction tables toward `diff = search_score - static_eval`.
    #[inline]
    pub fn update_correction(&mut self, board: &Board, diff: Value, depth: i32) {
        let bonus = (diff.clamp(-CORR_MAX, CORR_MAX) * depth.max(1) / 8).clamp(-CORR_MAX, CORR_MAX);
        let stm = board.side_to_move();
        let pk = (pawn_structure_key(board) & PAWN_MASK) as usize;
        let npk = (nonpawn_structure_key(board, stm) & PAWN_MASK) as usize;
        apply_corr_gravity(&mut self.pawn_corr[pk], bonus);
        apply_corr_gravity(&mut self.nonpawn_corr[stm.index()][npk], bonus);
    }

    // --- Stable read API (P5 LMR / LMP) ---

    /// Butterfly + continuation + pawn history for a quiet move.
    #[inline]
    pub fn quiet_score(
        &self,
        color: Color,
        mv: Move,
        piece: Piece,
        cont_slots: &[ContSlot; 4],
    ) -> i32 {
        self.get(color, mv) + self.continuation_score(cont_slots, piece, mv.to())
    }

    /// Quiet score including pawn-structure history when a board is available.
    #[inline]
    pub fn quiet_score_with_pawns(
        &self,
        color: Color,
        board: &Board,
        mv: Move,
        piece: Piece,
        cont_slots: &[ContSlot; 4],
    ) -> i32 {
        self.quiet_score(color, mv, piece, cont_slots) + self.pawn_get(board, piece, mv.to())
    }

    /// Capture-history term for a noisy move (0 if not a capture / EP).
    #[inline]
    pub fn capture_score(&self, board: &Board, mv: Move) -> i32 {
        match capture_key(board, mv) {
            Some((piece, to, captured)) => self.capture_get(piece, to, captured),
            None => 0,
        }
    }

    /// Combined history signal for LMR / history pruning.
    ///
    /// Quiet: `2 * butterfly + cont(1) + cont(2)`. Capture: capture history only.
    #[inline]
    pub fn stat_score(
        &self,
        color: Color,
        board: &Board,
        mv: Move,
        quiet: bool,
        cont_slots: &[ContSlot; 4],
    ) -> i32 {
        if quiet {
            let piece = board.piece_on(mv.from());
            2 * self.get(color, mv)
                + self.cont_get(cont_slots[0], piece, mv.to())
                + self.cont_get(cont_slots[1], piece, mv.to())
        } else {
            self.capture_score(board, mv)
        }
    }
}

/// Build the four continuation slots for the current search ply.
///
/// With `cont_slot` written to `stack[ply+1]` after make, the 1-ply slot for
/// the current node lives at `stack[ply]` (and 2/4/6 at `ply-1` / `ply-3` / `ply-5`).
#[inline]
pub fn continuation_slots(stack_cont: &[ContSlot], ply: usize) -> [ContSlot; 4] {
    let mut out = [ContSlot::NONE; 4];
    for (i, &d) in CONT_PLIES.iter().enumerate() {
        // ply+1-d: 1→ply, 2→ply-1, 4→ply-3, 6→ply-5
        if ply + 1 >= d {
            let idx = ply + 1 - d;
            if idx < stack_cont.len() {
                out[i] = stack_cont[idx];
            }
        }
    }
    out
}

/// `(moved_piece, to, captured_type)` for captures / EP; `None` for quiet promotions.
#[inline]
pub fn capture_key(board: &Board, mv: Move) -> Option<(Piece, Square, PieceType)> {
    let piece = board.piece_on(mv.from());
    if piece.is_empty() {
        return None;
    }
    let captured = if mv.is_en_passant() {
        PieceType::Pawn
    } else {
        board.piece_on(mv.to()).piece_type()?
    };
    Some((piece, mv.to(), captured))
}

/// Zobrist-like mix of both sides' pawn bitboards.
#[inline]
pub fn pawn_structure_key(board: &Board) -> u64 {
    let pawns = board.pieces(PieceType::Pawn);
    let wp = (pawns & board.pieces_color(Color::White)).0;
    let bp = (pawns & board.pieces_color(Color::Black)).0;
    wp.wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ bp.wrapping_mul(0xBF58_476D_1CE4_E5B9)
}

/// Mix of STM non-pawn piece placement (excl. kings lightly).
#[inline]
pub fn nonpawn_structure_key(board: &Board, color: Color) -> u64 {
    let mut k = 0u64;
    for &pt in &[
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ] {
        let bb = (board.pieces(pt) & board.pieces_color(color)).0;
        k ^= bb.wrapping_mul(0xD6E8_FEB8_6659_FD93u64.wrapping_add(pt.index() as u64));
    }
    k
}

#[inline]
fn pawn_hist_index(pawn_key: u64, piece: usize, to: usize) -> usize {
    let bucket = (pawn_key & PAWN_MASK) as usize;
    (bucket * PIECE_SLOTS + piece) * 64 + to
}

#[inline]
fn cont_index(prev_piece: usize, prev_to: usize, piece: usize, to: usize) -> usize {
    (((prev_piece * 64) + prev_to) * PIECE_SLOTS + piece) * 64 + to
}

/// Gravity update shared by all history tables.
#[inline]
fn apply_gravity(entry: &mut i16, bonus: i32) {
    let bonus = bonus.clamp(-HISTORY_MAX, HISTORY_MAX);
    let cur = *entry as i32;
    let next = cur + bonus - cur * bonus.abs() / HISTORY_MAX;
    *entry = next.clamp(-HISTORY_MAX, HISTORY_MAX) as i16;
}

#[inline]
fn apply_corr_gravity(entry: &mut i16, bonus: i32) {
    let bonus = bonus.clamp(-CORR_MAX, CORR_MAX);
    let cur = *entry as i32;
    let next = cur + bonus - cur * bonus.abs() / CORR_MAX;
    *entry = next.clamp(-CORR_MAX, CORR_MAX) as i16;
}

/// Depth-squared history bonus (clamped).
#[inline]
pub fn history_bonus(depth: i32) -> i32 {
    let d = depth.max(0);
    (d * d).min(HISTORY_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use crate::types::Square;

    fn e2e4() -> Move {
        Move::new(
            Square::from_index_unchecked(12), // e2
            Square::from_index_unchecked(28), // e4
        )
    }

    fn d2d4() -> Move {
        Move::new(
            Square::from_index_unchecked(11), // d2
            Square::from_index_unchecked(27), // d4
        )
    }

    #[test]
    fn pawn_history_orders_quiet_under_same_structure() {
        lookup::initialize();
        let mut h = HistoryTables::new();
        let board = Board::startpos();
        let e2e4 = e2e4();
        let d2d4 = d2d4();
        h.pawn_update(&board, Piece::WhitePawn, e2e4.to(), history_bonus(5));
        assert!(
            h.pawn_get(&board, Piece::WhitePawn, e2e4.to())
                > h.pawn_get(&board, Piece::WhitePawn, d2d4.to())
        );
    }

    #[test]
    fn correction_history_moves_score() {
        lookup::initialize();
        let mut h = HistoryTables::new();
        let board = Board::startpos();
        assert_eq!(h.correction_score(&board), 0);
        h.update_correction(&board, 200, 6);
        assert_ne!(h.correction_score(&board), 0);
    }

    #[test]
    fn update_raises_score() {
        let mut h = HistoryTables::new();
        assert_eq!(h.get(Color::White, e2e4()), 0);
        h.update(Color::White, e2e4(), history_bonus(4));
        assert!(h.get(Color::White, e2e4()) > 0);
        assert_eq!(h.get(Color::White, d2d4()), 0);
        assert_eq!(h.get(Color::Black, e2e4()), 0);
    }

    #[test]
    fn repeated_updates_stay_clamped() {
        let mut h = HistoryTables::new();
        for _ in 0..100 {
            h.update(Color::White, e2e4(), HISTORY_MAX);
        }
        let v = h.get(Color::White, e2e4());
        assert!(v <= HISTORY_MAX);
        assert!(v > 0);
    }

    #[test]
    fn malus_lowers_score() {
        let mut h = HistoryTables::new();
        h.update(Color::White, e2e4(), history_bonus(6));
        let before = h.get(Color::White, e2e4());
        h.update(Color::White, e2e4(), -history_bonus(4));
        assert!(h.get(Color::White, e2e4()) < before);
    }

    #[test]
    fn updated_quiet_ranks_above_untouched() {
        let mut h = HistoryTables::new();
        h.update(Color::White, e2e4(), history_bonus(5));
        assert!(h.get(Color::White, e2e4()) > h.get(Color::White, d2d4()));
    }

    #[test]
    fn capture_updates_do_not_overflow() {
        let mut h = HistoryTables::new();
        let pc = Piece::WhiteQueen;
        let to = Square::from_index_unchecked(28);
        for _ in 0..100 {
            h.capture_update(pc, to, PieceType::Pawn, HISTORY_MAX);
        }
        let v = h.capture_get(pc, to, PieceType::Pawn);
        assert!(v <= HISTORY_MAX);
        assert!(v > 0);
    }

    #[test]
    fn continuation_updates_do_not_overflow() {
        let mut h = HistoryTables::new();
        let slot = ContSlot::new(Piece::WhitePawn, Square::from_index_unchecked(28));
        let pc = Piece::BlackPawn;
        let to = Square::from_index_unchecked(27);
        for _ in 0..100 {
            h.cont_update(slot, pc, to, HISTORY_MAX);
        }
        let v = h.cont_get(slot, pc, to);
        assert!(v <= HISTORY_MAX);
        assert!(v > 0);
    }

    #[test]
    fn capture_history_orders_equal_victims() {
        lookup::initialize();
        let mut h = HistoryTables::new();
        // Two queen×pawn captures: boost NxP-style via QxP on e4 over QxP on a4.
        let q = Piece::WhiteQueen;
        let e4 = Square::from_index_unchecked(28);
        let a4 = Square::from_index_unchecked(24);
        h.capture_update(q, e4, PieceType::Pawn, history_bonus(6));
        assert!(h.capture_get(q, e4, PieceType::Pawn) > h.capture_get(q, a4, PieceType::Pawn));
    }

    #[test]
    fn continuation_orders_follow_up_quiet() {
        let mut h = HistoryTables::new();
        let prev = ContSlot::new(Piece::WhitePawn, Square::from_index_unchecked(28)); // e4
        let d7d5 = Move::new(
            Square::from_index_unchecked(51), // d7
            Square::from_index_unchecked(35), // d5
        );
        let e7e5 = Move::new(
            Square::from_index_unchecked(52), // e7
            Square::from_index_unchecked(36), // e5
        );
        let slots = [prev, ContSlot::NONE, ContSlot::NONE, ContSlot::NONE];
        h.cont_update(prev, Piece::BlackPawn, d7d5.to(), history_bonus(5));
        let s_d = h.quiet_score(Color::Black, d7d5, Piece::BlackPawn, &slots);
        let s_e = h.quiet_score(Color::Black, e7e5, Piece::BlackPawn, &slots);
        assert!(s_d > s_e);
    }

    #[test]
    fn continuation_slots_map_ply_offsets() {
        let mut slots = vec![ContSlot::NONE; 8];
        slots[3] = ContSlot::new(Piece::WhiteKnight, Square::from_index_unchecked(18));
        slots[4] = ContSlot::new(Piece::BlackPawn, Square::from_index_unchecked(35));
        // At ply 4: d=1 → idx 4, d=2 → idx 3
        let c = continuation_slots(&slots, 4);
        assert_eq!(c[0], slots[4]);
        assert_eq!(c[1], slots[3]);
        assert!(!c[2].is_valid()); // ply+1-4 = 1, still NONE
    }

    #[test]
    fn stat_score_quiet_uses_butterfly_and_cont() {
        lookup::initialize();
        let mut h = HistoryTables::new();
        let board = Board::startpos();
        let mv = e2e4();
        h.update(Color::White, mv, 100);
        let slot = ContSlot::new(Piece::BlackKnight, Square::from_index_unchecked(0));
        let slots = [slot, ContSlot::NONE, ContSlot::NONE, ContSlot::NONE];
        h.cont_update(slot, Piece::WhitePawn, mv.to(), 50);
        let s = h.stat_score(Color::White, &board, mv, true, &slots);
        assert_eq!(s, 2 * 100 + 50);
    }
}
