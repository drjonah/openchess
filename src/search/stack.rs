//! Per-ply search stack and PV helpers (P2-04).

use crate::types::{Move, Value};

pub const MAX_PLY: usize = 128;
pub const MAX_MOVES: usize = 256;

/// Per-ply search state.
#[derive(Clone, Debug)]
pub struct Stack {
    pub static_eval: Value,
    pub move_count: i32,
    pub current_move: Move,
    /// Killer move slots (filled by P3-03 later).
    pub killers: [Move; 2],
    /// PV line starting at this ply.
    pub pv: Vec<Move>,
}

impl Default for Stack {
    fn default() -> Self {
        Self {
            static_eval: 0,
            move_count: 0,
            current_move: Move::NONE,
            killers: [Move::NONE; 2],
            pv: Vec::new(),
        }
    }
}

impl Stack {
    pub fn clear_pv(&mut self) {
        self.pv.clear();
    }

    /// Set PV to `mv` followed by the child ply's PV.
    pub fn update_pv(&mut self, mv: Move, child: &Stack) {
        self.pv.clear();
        self.pv.push(mv);
        self.pv.extend_from_slice(&child.pv);
    }
}

/// Root move with score and PV from the last completed iteration.
#[derive(Clone, Debug)]
pub struct RootMove {
    pub mv: Move,
    pub score: Value,
    pub pv: Vec<Move>,
}

impl RootMove {
    pub fn new(mv: Move) -> Self {
        Self {
            mv,
            score: crate::types::score::VALUE_NONE,
            pv: vec![mv],
        }
    }
}

/// Format a PV as a space-separated UCI move string.
pub fn format_pv(pv: &[Move]) -> String {
    pv.iter()
        .map(|m| m.to_string())
        .collect::<Vec<_>>()
        .join(" ")
}
