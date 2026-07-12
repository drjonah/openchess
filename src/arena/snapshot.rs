//! Read-only live views of a game slot (P11-03).
//!
//! A [`GameSnapshot`] is cloneable and self-contained so an inspector can
//! render one slot while other slots keep searching, without holding any lock.

use crate::board::{Board, GameResult};
use crate::types::{Color, PieceType};

use super::slot::{GameSlot, Outcome, SlotStatus};

/// Per-side material: piece counts and centipawn totals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialBalance {
    pub white_cp: i32,
    pub black_cp: i32,
    /// White minus Black, in centipawns.
    pub balance_cp: i32,
    /// Counts in [P, N, B, R, Q] order.
    pub white_counts: [u32; 5],
    pub black_counts: [u32; 5],
}

impl MaterialBalance {
    /// Count material on `board` (kings excluded from the centipawn totals).
    pub fn of(board: &Board) -> Self {
        let white_cp = board.material(Color::White);
        let black_cp = board.material(Color::Black);
        Self {
            white_cp,
            black_cp,
            balance_cp: white_cp - black_cp,
            white_counts: counts(board, Color::White),
            black_counts: counts(board, Color::Black),
        }
    }
}

fn counts(board: &Board, color: Color) -> [u32; 5] {
    let us = board.pieces_color(color);
    let mut out = [0u32; 5];
    for (i, pt) in [
        PieceType::Pawn,
        PieceType::Knight,
        PieceType::Bishop,
        PieceType::Rook,
        PieceType::Queen,
    ]
    .into_iter()
    .enumerate()
    {
        out[i] = (board.pieces(pt) & us).count() as u32;
    }
    out
}

/// A cloneable read-only view of one slot at a moment in time.
#[derive(Clone, Debug)]
pub struct GameSnapshot {
    pub id: usize,
    pub fen: String,
    pub ply: usize,
    pub side_to_move: Color,
    /// SAN tokens for the move panel.
    pub transcript: Vec<String>,
    pub last_move: Option<String>,
    pub status: SlotStatus,
    pub result: GameResult,
    pub outcome: Outcome,
    pub profile: Option<String>,
    /// White-relative eval centipawns from the last completed search.
    pub eval_white_cp: Option<i32>,
    pub material: MaterialBalance,
    /// Live search depth (0 when idle/finished).
    pub depth: u32,
    pub nodes: u64,
    pub thinking: bool,
}

impl GameSnapshot {
    /// Build a snapshot from a live slot (no search is started).
    pub fn of(slot: &GameSlot) -> Self {
        let info = slot.last_info();
        Self {
            id: slot.id,
            fen: slot.board().to_fen(),
            ply: slot.ply_count(),
            side_to_move: slot.side_to_move(),
            transcript: slot.transcript().iter().map(|p| p.san.clone()).collect(),
            last_move: slot.last_move().map(|m| m.to_string()),
            status: slot.status(),
            result: slot.result(),
            outcome: slot.outcome(),
            profile: slot.profile.clone(),
            eval_white_cp: slot.eval_white_cp(),
            material: MaterialBalance::of(slot.board()),
            depth: info.depth,
            nodes: info.nodes,
            thinking: info.thinking,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::SideStrength;

    #[test]
    fn startpos_material_is_balanced() {
        crate::lookup::initialize();
        let mb = MaterialBalance::of(&Board::startpos());
        assert_eq!(mb.balance_cp, 0);
        // [P, N, B, R, Q]
        assert_eq!(mb.white_counts, [8, 2, 2, 2, 1]);
        assert_eq!(mb.black_counts, [8, 2, 2, 2, 1]);
    }

    #[test]
    fn material_after_queen_capture() {
        crate::lookup::initialize();
        // White is up a full queen (no black queen on the board).
        let fen = "rnb1kbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let board = Board::from_fen(fen).unwrap();
        let mb = MaterialBalance::of(&board);
        assert_eq!(mb.balance_cp, crate::types::score::QUEEN_VALUE);
        assert_eq!(mb.black_counts[4], 0);
        assert_eq!(mb.white_counts[4], 1);
    }

    #[test]
    fn snapshot_reads_slot_without_search() {
        crate::lookup::initialize();
        let slot = GameSlot::new(
            7,
            SideStrength {
                depth: 1,
                movetime_ms: 0,
            },
            SideStrength {
                depth: 1,
                movetime_ms: 0,
            },
            40,
        );
        let snap = GameSnapshot::of(&slot);
        assert_eq!(snap.id, 7);
        assert_eq!(snap.ply, 0);
        assert_eq!(snap.side_to_move, Color::White);
        assert!(snap.transcript.is_empty());
        assert_eq!(snap.material.balance_cp, 0);
        assert!(!snap.thinking);
    }
}
