//! Browseable game transcript with optional per-ply analysis slots.

use crate::types::Move;

/// Move quality classification (filled by a future analysis pass).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveClass {
    Book,
    Best,
    Excellent,
    Good,
    Inaccuracy,
    Mistake,
    Blunder,
    Brilliant,
    Miss,
}

impl MoveClass {
    /// Short glyph for the move list.
    pub fn glyph(self) -> &'static str {
        match self {
            Self::Book => "BK",
            Self::Best => "*",
            Self::Excellent => "+",
            Self::Good => "=",
            Self::Inaccuracy => "?!",
            Self::Mistake => "?",
            Self::Blunder => "??",
            Self::Brilliant => "!!",
            Self::Miss => "X",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Book => "Book",
            Self::Best => "Best",
            Self::Excellent => "Excellent",
            Self::Good => "Good",
            Self::Inaccuracy => "Inaccuracy",
            Self::Mistake => "Mistake",
            Self::Blunder => "Blunder",
            Self::Brilliant => "Brilliant",
            Self::Miss => "Miss",
        }
    }
}

/// Per-ply engine analysis (left blank until analysis is implemented).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlyAnalysis {
    /// White-relative centipawns after this ply (mate via `types::score` conventions).
    pub eval_cp: i32,
    pub classification: MoveClass,
    pub cpl: u32,
}

/// One half-move in the game.
#[derive(Clone, Debug)]
pub struct PlyRecord {
    pub mv: Move,
    pub san: String,
    pub analysis: Option<PlyAnalysis>,
}

impl PlyRecord {
    pub fn new(mv: Move, san: impl Into<String>) -> Self {
        Self {
            mv,
            san: san.into(),
            analysis: None,
        }
    }
}

/// Optional PGN header metadata for the move-list panel.
#[derive(Clone, Debug, Default)]
pub struct GameHeaders {
    pub white: Option<String>,
    pub black: Option<String>,
    pub result: Option<String>,
}

/// Imported game with a ply cursor for browsing.
#[derive(Clone, Debug)]
pub struct AnalyzedGame {
    pub start_fen: String,
    pub plies: Vec<PlyRecord>,
    /// `0` = start position; `N` = after `N` plies.
    pub cursor: usize,
    pub headers: GameHeaders,
}

impl AnalyzedGame {
    pub fn new(start_fen: String, plies: Vec<PlyRecord>, headers: GameHeaders) -> Self {
        let cursor = plies.len();
        Self {
            start_fen,
            plies,
            cursor,
            headers,
        }
    }

    pub fn ply_count(&self) -> usize {
        self.plies.len()
    }

    /// Eval after the current cursor ply, if analysis exists.
    /// At cursor 0 (start), returns `None` (no ply yet).
    pub fn current_eval(&self) -> Option<i32> {
        if self.cursor == 0 {
            return None;
        }
        self.plies
            .get(self.cursor - 1)
            .and_then(|p| p.analysis.as_ref().map(|a| a.eval_cp))
    }

    pub fn last_move_at_cursor(&self) -> Option<Move> {
        if self.cursor == 0 {
            None
        } else {
            self.plies.get(self.cursor - 1).map(|p| p.mv)
        }
    }
}
