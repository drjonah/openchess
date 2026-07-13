//! One arena game slot: an isolated [`Board`], transcript, per-side strength,
//! and an optional background search job.
//!
//! Each slot owns its own board and, while searching, a private
//! transposition table via [`crate::session::LiveSearch`] — no state is shared
//! between slots (P11-01).

use crate::board::{Board, GameResult};
use crate::book::{Book, BookRng};
use crate::config::SideStrength;
use crate::search::{Limits, SearchResult};
use crate::session::{LiveSearch, SearchInfo, stm_score_to_white};
use crate::tui::game::PlyRecord;
use crate::tui::san::format_san;
use crate::types::{Color, Move};
use std::time::Duration;

/// Scheduler state of a slot (distinct from the chess [`GameResult`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotStatus {
    /// Runnable and waiting for the scheduler to start its next move.
    Idle,
    /// A background search is in flight.
    Thinking,
    /// User-paused: the scheduler skips this slot.
    Paused,
    /// Game over (natural result, ply-cap adjudication, or abort).
    Finished,
}

/// Why a slot stopped playing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishReason {
    /// Mate / stalemate / draw by rule.
    Natural,
    /// Hit the per-slot ply limit and was adjudicated a draw.
    PlyCap,
    /// Aborted by the user (or a failed search); no decisive result.
    Aborted,
}

/// White-relative outcome of a (possibly adjudicated) game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    WhiteWin,
    BlackWin,
    Draw,
    Unfinished,
}

impl Outcome {
    /// PGN-style result tag.
    pub fn result_tag(self) -> &'static str {
        match self {
            Outcome::WhiteWin => "1-0",
            Outcome::BlackWin => "0-1",
            Outcome::Draw => "1/2-1/2",
            Outcome::Unfinished => "*",
        }
    }
}

/// An event produced when a slot advances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SlotEvent {
    /// A move was applied to the slot's board.
    Move {
        slot: usize,
        ply: usize,
        uci: String,
        /// White-relative centipawn eval from the search that chose the move.
        eval_cp: Option<i32>,
    },
    /// The slot finished (result decided or adjudicated).
    Finish {
        slot: usize,
        outcome: Outcome,
        plies: usize,
    },
}

/// One concurrent Bot-vs-Bot game.
pub struct GameSlot {
    pub id: usize,
    board: Board,
    start_fen: String,
    transcript: Vec<PlyRecord>,
    /// White search limits for this slot.
    pub white: SideStrength,
    /// Black search limits for this slot.
    pub black: SideStrength,
    /// Name of the applied [`crate::arena::ArenaProfile`], if any.
    pub profile: Option<String>,
    status: SlotStatus,
    finish_reason: Option<FinishReason>,
    job: Option<LiveSearch>,
    /// Side to move when the current search started.
    search_stm: Option<Color>,
    last_info: SearchInfo,
    last_move: Option<Move>,
    result: GameResult,
    /// Hard cap on plies before a game is adjudicated a draw.
    ply_limit: usize,
    /// Most recent completed White-relative eval (centipawns).
    eval_white_cp: Option<i32>,
    /// Opening book (same default as interactive play — P10 / TUI-04).
    book: Book,
    book_rng: BookRng,
    /// When set, the slot re-pauses after applying one move (manual step).
    step_once: bool,
}

/// Build search [`Limits`] from a per-side strength (single-threaded).
fn strength_limits(s: &SideStrength, ply: u32) -> Limits {
    let mut limits = Limits {
        threads: 1,
        depth: Some(s.depth as i32),
        ..Default::default()
    };
    if s.movetime_ms > 0 {
        limits.movetime = Some(Duration::from_millis(s.movetime_ms));
        if ply <= crate::search::OPENING_PHASE_PLIES {
            limits.min_opening_depth = Some(crate::search::MIN_OPENING_DEPTH);
        }
    }
    limits
}

impl GameSlot {
    /// New slot starting from the initial position.
    pub fn new(id: usize, white: SideStrength, black: SideStrength, ply_limit: usize) -> Self {
        let board = Board::startpos();
        Self::from_board(id, board, white, black, ply_limit)
    }

    /// New slot starting from a FEN position.
    pub fn from_fen(
        id: usize,
        fen: &str,
        white: SideStrength,
        black: SideStrength,
        ply_limit: usize,
    ) -> Result<Self, String> {
        let board = Board::from_fen(fen).map_err(|e| e.to_string())?;
        Ok(Self::from_board(id, board, white, black, ply_limit))
    }

    fn from_board(
        id: usize,
        board: Board,
        white: SideStrength,
        black: SideStrength,
        ply_limit: usize,
    ) -> Self {
        let start_fen = board.to_fen();
        let result = board.game_result();
        let mut slot = Self {
            id,
            board,
            start_fen,
            transcript: Vec::new(),
            white,
            black,
            profile: None,
            status: SlotStatus::Idle,
            finish_reason: None,
            job: None,
            search_stm: None,
            last_info: SearchInfo::default(),
            last_move: None,
            result,
            ply_limit: ply_limit.max(1),
            eval_white_cp: None,
            book: Book::embedded(),
            book_rng: BookRng::from_entropy(),
            step_once: false,
        };
        if result.is_over() {
            slot.finish(FinishReason::Natural);
        }
        slot
    }

    pub fn status(&self) -> SlotStatus {
        self.status
    }

    pub fn result(&self) -> GameResult {
        self.result
    }

    pub fn finish_reason(&self) -> Option<FinishReason> {
        self.finish_reason
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn start_fen(&self) -> &str {
        &self.start_fen
    }

    pub fn transcript(&self) -> &[PlyRecord] {
        &self.transcript
    }

    pub fn ply_count(&self) -> usize {
        self.transcript.len()
    }

    pub fn last_move(&self) -> Option<Move> {
        self.last_move
    }

    pub fn last_info(&self) -> &SearchInfo {
        &self.last_info
    }

    pub fn side_to_move(&self) -> Color {
        self.board.side_to_move()
    }

    pub fn eval_white_cp(&self) -> Option<i32> {
        self.eval_white_cp
    }

    pub fn is_finished(&self) -> bool {
        self.status == SlotStatus::Finished
    }

    pub fn is_thinking(&self) -> bool {
        self.status == SlotStatus::Thinking
    }

    pub fn is_paused(&self) -> bool {
        self.status == SlotStatus::Paused
    }

    /// Ready for the scheduler to start a new search on.
    pub fn is_runnable(&self) -> bool {
        self.status == SlotStatus::Idle && self.job.is_none() && !self.board.game_result().is_over()
    }

    /// Strength for the given color.
    pub fn strength(&self, color: Color) -> &SideStrength {
        match color {
            Color::White => &self.white,
            Color::Black => &self.black,
        }
    }

    /// Replace the strength for one color (takes effect on that side's next move).
    pub fn set_strength(&mut self, color: Color, strength: SideStrength) {
        match color {
            Color::White => self.white = strength,
            Color::Black => self.black = strength,
        }
    }

    /// Replace the opening book (e.g. [`Book::disabled()`] for search-only slots).
    pub fn set_book(&mut self, book: Book) {
        self.book = book;
    }

    /// White-relative outcome (adjudicates ply-cap as a draw).
    pub fn outcome(&self) -> Outcome {
        match self.result {
            GameResult::Checkmate {
                winner: Color::White,
            } => Outcome::WhiteWin,
            GameResult::Checkmate {
                winner: Color::Black,
            } => Outcome::BlackWin,
            _ if self.result.is_draw() => Outcome::Draw,
            _ => match self.finish_reason {
                Some(FinishReason::PlyCap) => Outcome::Draw,
                _ => Outcome::Unfinished,
            },
        }
    }

    /// Start a background search for the side to move, or play a book move
    /// instantly when the opening book hits (no-op if not runnable).
    ///
    /// Returns move/finish events when a book move was applied synchronously.
    pub fn begin_search(&mut self, hash_mb: usize) -> Vec<SlotEvent> {
        if self.job.is_some() || self.is_finished() || self.status == SlotStatus::Paused {
            return Vec::new();
        }
        if self.board.game_result().is_over() {
            self.finish(FinishReason::Natural);
            return Vec::new();
        }

        let ply = self.transcript.len() as u32;
        if let Some(mv) = self.book.probe(&self.board, ply, &mut self.book_rng) {
            if !mv.is_none() {
                return self.apply_played_move(mv, None);
            }
        }

        let stm = self.board.side_to_move();
        let limits = strength_limits(self.strength(stm), ply);
        self.last_info = SearchInfo {
            thinking: true,
            ..SearchInfo::default()
        };
        self.search_stm = Some(stm);
        self.job = Some(LiveSearch::spawn(self.board.clone(), limits, hash_mb));
        self.status = SlotStatus::Thinking;
        Vec::new()
    }

    /// Poll the in-flight search; apply the move and produce events when ready.
    pub fn poll(&mut self) -> Vec<SlotEvent> {
        let Some(job) = self.job.as_ref() else {
            return Vec::new();
        };

        let live = job.snapshot_live();
        self.last_info.depth = live.depth;
        self.last_info.score_cp = live.score_cp;
        self.last_info.nodes = live.nodes;
        self.last_info.time = live.time;
        self.last_info.pv = live.pv;
        self.last_info.thinking = true;

        if !job.is_ready() {
            return Vec::new();
        }

        let mut job = self.job.take().unwrap();
        let stm = self.search_stm.take().unwrap_or(Color::White);
        let result = job.take_result();
        self.apply_result(stm, result)
    }

    fn apply_result(&mut self, stm: Color, result: Option<SearchResult>) -> Vec<SlotEvent> {
        self.last_info.thinking = false;

        let Some(result) = result else {
            self.finish(FinishReason::Aborted);
            return vec![SlotEvent::Finish {
                slot: self.id,
                outcome: self.outcome(),
                plies: self.transcript.len(),
            }];
        };

        self.eval_white_cp = Some(stm_score_to_white(result.score, stm));
        self.last_info.depth = result.depth.max(0) as u32;
        self.last_info.score_cp = result.score;
        self.last_info.nodes = result.nodes;
        self.last_info.time = result.time;

        if result.best_move.is_none() {
            self.finish(FinishReason::Natural);
            return vec![SlotEvent::Finish {
                slot: self.id,
                outcome: self.outcome(),
                plies: self.transcript.len(),
            }];
        }

        self.apply_played_move(result.best_move, self.eval_white_cp)
    }

    fn apply_played_move(&mut self, mv: Move, eval_cp: Option<i32>) -> Vec<SlotEvent> {
        self.last_info.thinking = false;

        let san = format_san(&self.board, mv);
        self.board.make(mv);
        self.transcript.push(PlyRecord::new(mv, san));
        self.last_move = Some(mv);

        let ply = self.transcript.len();
        let mut events = vec![SlotEvent::Move {
            slot: self.id,
            ply,
            uci: mv.to_string(),
            eval_cp,
        }];

        self.result = self.board.game_result();
        if self.result.is_over() {
            self.finish(FinishReason::Natural);
            events.push(self.finish_event());
        } else if self.transcript.len() >= self.ply_limit {
            self.finish(FinishReason::PlyCap);
            events.push(self.finish_event());
        } else if self.step_once {
            self.step_once = false;
            self.status = SlotStatus::Paused;
        } else {
            self.status = SlotStatus::Idle;
        }
        events
    }

    fn finish_event(&self) -> SlotEvent {
        SlotEvent::Finish {
            slot: self.id,
            outcome: self.outcome(),
            plies: self.transcript.len(),
        }
    }

    fn finish(&mut self, reason: FinishReason) {
        self.abort_job();
        self.finish_reason = Some(reason);
        self.status = SlotStatus::Finished;
        self.last_info.thinking = false;
    }

    fn abort_job(&mut self) {
        if let Some(mut job) = self.job.take() {
            job.shutdown();
        }
        self.search_stm = None;
        self.last_info.thinking = false;
    }

    /// Pause the slot (discards any in-flight search).
    pub fn pause(&mut self) {
        if self.is_finished() {
            return;
        }
        self.abort_job();
        self.step_once = false;
        self.status = SlotStatus::Paused;
    }

    /// Resume a paused slot.
    pub fn resume(&mut self) {
        if self.status == SlotStatus::Paused {
            self.status = SlotStatus::Idle;
        }
    }

    /// Advance exactly one move while paused (scheduler starts the search).
    pub fn request_step(&mut self) {
        if self.status == SlotStatus::Paused && !self.board.game_result().is_over() {
            self.step_once = true;
            self.status = SlotStatus::Idle;
        }
    }

    /// True when a single manual step is pending.
    pub fn step_pending(&self) -> bool {
        self.step_once
    }

    /// Restart the slot from its starting position, keeping strengths.
    pub fn restart(&mut self) {
        self.abort_job();
        self.board = Board::from_fen(&self.start_fen).unwrap_or_else(|_| Board::startpos());
        self.transcript.clear();
        self.last_move = None;
        self.result = self.board.game_result();
        self.finish_reason = None;
        self.eval_white_cp = None;
        self.last_info = SearchInfo::default();
        self.step_once = false;
        self.status = if self.result.is_over() {
            SlotStatus::Finished
        } else {
            SlotStatus::Idle
        };
    }

    /// Abort the slot: stop searching and mark finished with no result.
    pub fn abort(&mut self) {
        if self.is_finished() {
            return;
        }
        self.finish(FinishReason::Aborted);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strength(depth: u32) -> SideStrength {
        SideStrength {
            depth,
            movetime_ms: 0,
        }
    }

    #[test]
    fn new_slot_starts_idle_and_runnable() {
        crate::lookup::initialize();
        let slot = GameSlot::new(0, strength(1), strength(1), 40);
        assert_eq!(slot.status(), SlotStatus::Idle);
        assert!(slot.is_runnable());
        assert_eq!(slot.ply_count(), 0);
        assert_eq!(slot.outcome(), Outcome::Unfinished);
    }

    #[test]
    fn first_move_comes_from_book_without_search() {
        crate::lookup::initialize();
        let mut slot = GameSlot::new(0, strength(1), strength(1), 40);
        let events = slot.begin_search(1);
        assert!(!slot.is_thinking(), "book move should not spawn search");
        assert_eq!(slot.ply_count(), 1, "book move should be applied");
        assert_eq!(events.len(), 1);
        assert!(matches!(events[0], SlotEvent::Move { ply: 1, .. }));
        let uci = slot.last_move().unwrap().to_string();
        assert!(
            !matches!(uci.as_str(), "a2a3" | "a2a4" | "h2h3" | "h2h4"),
            "unexpected flank pawn: {uci}"
        );
    }

    #[test]
    fn pause_blocks_scheduling_then_resume_restores() {
        crate::lookup::initialize();
        let mut slot = GameSlot::new(0, strength(1), strength(1), 40);
        slot.pause();
        assert!(slot.is_paused());
        assert!(!slot.is_runnable());
        // begin_search must not start on a paused slot.
        slot.begin_search(1);
        assert!(!slot.is_thinking());
        slot.resume();
        assert!(slot.is_runnable());
    }

    #[test]
    fn restart_clears_transcript() {
        crate::lookup::initialize();
        let mut slot = GameSlot::new(0, strength(1), strength(1), 40);
        // Play one move synchronously.
        slot.begin_search(1);
        loop {
            let events = slot.poll();
            if !events.is_empty() || !slot.is_thinking() {
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(slot.ply_count() >= 1);
        slot.restart();
        assert_eq!(slot.ply_count(), 0);
        assert_eq!(slot.status(), SlotStatus::Idle);
        assert!(slot.last_move().is_none());
    }

    #[test]
    fn abort_marks_unfinished() {
        crate::lookup::initialize();
        let mut slot = GameSlot::new(0, strength(1), strength(1), 40);
        slot.abort();
        assert!(slot.is_finished());
        assert_eq!(slot.outcome(), Outcome::Unfinished);
    }

    #[test]
    fn ply_cap_adjudicates_draw() {
        crate::lookup::initialize();
        // Cap of 2 plies forces adjudication before a natural result.
        let mut slot = GameSlot::new(0, strength(1), strength(1), 2);
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        while !slot.is_finished() && std::time::Instant::now() < deadline {
            if !slot.is_thinking() {
                slot.begin_search(1);
            }
            let _ = slot.poll();
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(slot.is_finished());
        assert_eq!(slot.ply_count(), 2);
        assert_eq!(slot.finish_reason(), Some(FinishReason::PlyCap));
        assert_eq!(slot.outcome(), Outcome::Draw);
    }

    #[test]
    fn set_strength_takes_effect() {
        crate::lookup::initialize();
        let mut slot = GameSlot::new(0, strength(1), strength(1), 40);
        slot.set_strength(Color::Black, strength(9));
        assert_eq!(slot.strength(Color::Black).depth, 9);
        assert_eq!(slot.strength(Color::White).depth, 1);
    }
}
