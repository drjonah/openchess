//! Session: real [`Board`] + play modes + engine search.
//!
//! Player input accepts one move: short algebraic (`e4`, `Nf3`, `O-O`) or UCI (`e2e4`).

use super::classify::{classify_move, opponent_was_bad, ClassifyInput};
use super::game::{
    cpl_from_eval_swing, AnalyzedGame, GameHeaders, PlyAnalysis, PlyRecord,
};
use super::san::format_san;
use crate::board::{Board, GameResult};
use crate::book::{Book, BookRng};
use crate::search::{Limits, SearchResult};
use crate::session::LiveSearch;
use crate::types::{Color, Move, Piece, PieceType, Square};
use std::str::FromStr;
use std::time::{Duration, Instant};

// Re-export for callers that historically imported these from `tui::session`.
pub use crate::session::{GoLimits, PlayMode, SearchInfo, stm_score_to_white};

/// Sequential post-game analysis of an imported transcript.
struct PostGameState {
    /// `0` = evaluate start position (CPL baseline); `1..=n` = position after that many plies.
    next_step: usize,
    total_plies: usize,
    limits: GoLimits,
    /// White-relative eval of the previous position (for CPL).
    prev_eval: Option<i32>,
    /// Best move from the most recent completed search (position before the next ply).
    pending_best_move: Option<Move>,
}

pub struct EngineSession {
    board: Board,
    /// Moves applied this game (for undo via [`Board::unmake`]).
    move_stack: Vec<Move>,
    last_move: Option<Move>,
    mode: Option<PlayMode>,
    flipped: bool,
    info: SearchInfo,
    go_started: Option<Instant>,
    pending_limits: Option<GoLimits>,
    apply_on_finish: bool,
    /// Side to move when the current/last bot search started (for White-relative conversion).
    search_stm: Option<Color>,
    /// White-relative live eval for the eval bar (`Some(0)` at startpos).
    live_eval_cp: Option<i32>,
    /// Position changed since `live_eval_cp` was computed for the current board.
    live_eval_stale: bool,
    status: String,
    /// Live or imported game transcript for the move panel.
    analyzed: Option<AnalyzedGame>,
    /// True after [`Self::load_analyzed_game`] until a fresh game/FEN is loaded.
    imported_game: bool,
    /// User forced the eval bar on (also shown automatically for imported games).
    eval_bar_forced: bool,
    /// Active bot / analyze search, if any.
    live: Option<LiveSearch>,
    /// Separate background eval-bar search (does not block bot moves).
    eval_live: Option<LiveSearch>,
    /// Side to move when the eval search started.
    eval_stm: Option<Color>,
    /// Position key (`move_stack.len()`) the eval search was started for.
    eval_position_key: Option<u64>,
    /// Active post-game analysis pass over an imported game.
    post_game: Option<PostGameState>,
    /// Dedicated search worker for post-game analysis.
    analysis_live: Option<LiveSearch>,
    /// Side to move for the active post-game analysis search.
    analysis_stm: Option<Color>,
    /// Limits used when starting post-game analysis on import.
    analysis_limits: GoLimits,
    /// After a PvB→BvB takeover (`x`), this color keeps shared `bot.*` strength
    /// instead of switching to per-side Bot vs Bot settings.
    bvb_shared_side: Option<Color>,
    /// Opening book built from [`BookConfig`] (P10-02 / TUI-04).
    book: Book,
    /// PRNG for weighted book selection (varied bot play).
    book_rng: BookRng,
    /// Opening-phase search floor depth (0 = off); applied on book miss (P10-04).
    opening_floor_depth: i32,
    /// Private TT size (MB) for interactive searches (from `engine.hash_mb`).
    hash_mb: usize,
}

impl EngineSession {
    pub fn new() -> Self {
        Self::new_with_config(&crate::config::Config::default())
    }

    /// Seed flip / eval bar from user config; mode stays unset until the user picks one.
    pub fn new_with_config(config: &crate::config::Config) -> Self {
        Self {
            board: Board::startpos(),
            move_stack: Vec::new(),
            last_move: None,
            mode: None,
            flipped: config.tui.flip_board,
            info: SearchInfo::default(),
            go_started: None,
            pending_limits: None,
            apply_on_finish: true,
            search_stm: None,
            live_eval_cp: Some(0),
            live_eval_stale: false,
            status: "Choose a game mode to start".into(),
            analyzed: Some(empty_analyzed(Board::startpos().to_fen())),
            imported_game: false,
            eval_bar_forced: config.tui.show_eval_bar,
            live: None,
            eval_live: None,
            eval_stm: None,
            eval_position_key: None,
            post_game: None,
            analysis_live: None,
            analysis_stm: None,
            analysis_limits: config.analysis_go_limits(),
            bvb_shared_side: None,
            book: Book::from_config(&config.book),
            book_rng: BookRng::from_entropy(),
            opening_floor_depth: config.engine.opening_floor_depth as i32,
            hash_mb: config.engine.hash_mb.max(1) as usize,
        }
    }

    /// Apply common TUI fields from config without resetting the board.
    pub fn apply_tui_config(&mut self, config: &crate::config::Config) {
        if let Some(current) = self.mode {
            let mode = config.tui.default_mode.to_play_mode();
            if current != mode {
                self.set_mode(mode);
            }
        }
        self.flipped = config.tui.flip_board;
        self.eval_bar_forced = config.tui.show_eval_bar;
        self.analysis_limits = config.analysis_go_limits();
        self.book = Book::from_config(&config.book);
        self.opening_floor_depth = config.engine.opening_floor_depth as i32;
        self.hash_mb = config.engine.hash_mb.max(1) as usize;
    }

    pub fn set_flipped(&mut self, flipped: bool) {
        self.flipped = flipped;
    }

    pub fn set_eval_bar_forced(&mut self, on: bool) {
        if on && !self.eval_bar_forced {
            self.live_eval_stale = true;
        }
        self.eval_bar_forced = on;
    }

    pub fn eval_bar_forced(&self) -> bool {
        self.eval_bar_forced
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn status(&self) -> &str {
        &self.status
    }

    pub fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
    }

    pub fn piece_on(&self, sq: Square) -> Piece {
        self.board.piece_on(sq)
    }

    pub fn side_to_move(&self) -> Color {
        self.board.side_to_move()
    }

    pub fn fullmove_number(&self) -> u16 {
        self.board.fullmove_number()
    }

    pub fn last_move(&self) -> Option<Move> {
        self.last_move
    }

    pub fn flipped(&self) -> bool {
        self.flipped
    }

    pub fn toggle_flip(&mut self) {
        self.flipped = !self.flipped;
    }

    /// Show eval bar when browsing an imported game, or when the user toggled it on.
    pub fn show_eval_bar(&self) -> bool {
        self.eval_bar_forced || self.imported_game
    }

    /// True while the current transcript came from an import.
    pub fn imported_game(&self) -> bool {
        self.imported_game
    }

    pub fn toggle_eval_bar(&mut self) {
        if self.imported_game {
            self.status = "Eval bar stays on while browsing an imported game".into();
            return;
        }
        self.eval_bar_forced = !self.eval_bar_forced;
        if self.eval_bar_forced {
            // Re-evaluate the current live position when the bar is shown.
            self.live_eval_stale = true;
            self.status = "Eval bar on (v to hide)".into();
        } else {
            self.status = "Eval bar off (v to show)".into();
        }
    }

    /// Position needs a live-eval search for the eval bar.
    pub fn live_eval_stale(&self) -> bool {
        self.live_eval_stale
    }

    pub fn mode(&self) -> Option<PlayMode> {
        self.mode
    }

    pub fn mode_title(&self) -> &'static str {
        self.mode.map(PlayMode::title).unwrap_or("Choose a mode")
    }

    /// True when the user must pick a play mode before starting.
    pub fn needs_mode_picker(&self) -> bool {
        self.mode.is_none() && !self.imported_game
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.stop_thinking_quiet();
        self.stop_eval_quiet();
        self.stop_post_game_quiet();
        self.bvb_shared_side = None;
        self.mode = Some(mode);
        self.flipped = matches!(
            mode,
            PlayMode::PlayerVsBot {
                human: Color::Black
            }
        );
        self.info.bestmove = None;
        self.status = format!("{} — {}", mode.title(), mode.blurb());
    }

    /// Hand the human's seat to a bot (`x`): switch to Bot vs Bot.
    ///
    /// If coming from Player vs Bot, the side that was already the engine keeps
    /// the shared PvB strength; only the taking-over color uses its Bot vs Bot
    /// per-side settings.
    pub fn take_over_with_bot(&mut self) {
        let keep_shared = match self.mode {
            Some(PlayMode::PlayerVsBot { human }) => Some(!human),
            _ => None,
        };
        self.set_mode(PlayMode::BotVsBot);
        self.bvb_shared_side = keep_shared;
        self.status = match keep_shared {
            Some(Color::White) => {
                "Bot vs Bot — White keeps PvB strength · Black uses side settings".into()
            }
            Some(Color::Black) => {
                "Bot vs Bot — Black keeps PvB strength · White uses side settings".into()
            }
            None => format!(
                "{} — {}",
                PlayMode::BotVsBot.title(),
                PlayMode::BotVsBot.blurb()
            ),
        };
    }

    /// Color that still uses shared PvB `bot.*` limits after a takeover, if any.
    pub fn bvb_shared_side(&self) -> Option<Color> {
        self.bvb_shared_side
    }

    /// Search limits for the side about to move, honoring takeover overrides.
    pub fn play_go_limits(&self, config: &crate::config::Config) -> GoLimits {
        match self.mode {
            Some(PlayMode::BotVsBot) => {
                let stm = self.board.side_to_move();
                if self.bvb_shared_side == Some(stm) {
                    config.go_limits()
                } else {
                    config.side_go_limits(stm)
                }
            }
            _ => config.go_limits(),
        }
    }

    pub fn info(&self) -> &SearchInfo {
        &self.info
    }

    pub fn is_thinking(&self) -> bool {
        self.info.thinking
    }

    pub fn is_human_turn(&self) -> bool {
        match self.mode {
            None => false,
            Some(PlayMode::PlayerVsPlayer | PlayMode::Analyze) => true,
            Some(PlayMode::PlayerVsBot { human }) => self.board.side_to_move() == human,
            Some(PlayMode::BotVsBot) => false,
        }
    }

    pub fn engine_should_auto_move(&self) -> bool {
        if self.is_game_over() {
            return false;
        }
        match self.mode {
            None => false,
            Some(PlayMode::PlayerVsPlayer | PlayMode::Analyze) => false,
            Some(PlayMode::PlayerVsBot { human }) => self.board.side_to_move() != human,
            Some(PlayMode::BotVsBot) => true,
        }
    }

    /// Current game result (mate, draw-by-rule, or ongoing).
    pub fn game_result(&self) -> GameResult {
        self.board.game_result()
    }

    pub fn is_game_over(&self) -> bool {
        self.game_result().is_over()
    }

    pub fn search_applies_move(&self) -> bool {
        !matches!(self.mode, Some(PlayMode::Analyze))
    }

    pub fn analyzed(&self) -> Option<&AnalyzedGame> {
        self.analyzed.as_ref()
    }

    /// White-relative eval for the eval bar (`None` = no analysis yet).
    ///
    /// Prefers a live eval-worker score, then bot-search score while thinking,
    /// then the cached `live_eval_cp`.
    pub fn current_eval(&self) -> Option<i32> {
        if let Some(ev) = self.analyzed.as_ref().and_then(|g| g.current_eval()) {
            return Some(ev);
        }
        if let Some(score) = self.live_eval_worker_score() {
            return Some(score);
        }
        if self.info.thinking {
            if let Some(stm) = self.search_stm {
                if self.info.depth > 0 || self.info.nodes > 0 {
                    return Some(stm_score_to_white(self.info.score_cp, stm));
                }
            }
        }
        self.live_eval_cp
    }

    /// Live score from the dedicated eval worker, if it has reported depth/nodes.
    fn live_eval_worker_score(&self) -> Option<i32> {
        let stm = self.eval_stm?;
        let live = self.eval_live.as_ref()?;
        let snap = live.snapshot_live();
        if snap.depth > 0 || snap.nodes > 0 {
            Some(stm_score_to_white(snap.score_cp, stm))
        } else {
            None
        }
    }

    fn position_key(&self) -> u64 {
        self.move_stack.len() as u64
    }

    /// PV / best-move hints are Analyze-only (never during live play vs bot).
    pub fn show_engine_hints(&self) -> bool {
        matches!(self.mode, Some(PlayMode::Analyze))
    }

    pub fn new_game(&mut self) {
        self.stop_thinking_quiet();
        self.stop_eval_quiet();
        self.stop_post_game_quiet();
        self.board = Board::startpos();
        self.move_stack.clear();
        self.last_move = None;
        self.mode = None;
        self.analyzed = Some(empty_analyzed(self.board.to_fen()));
        self.imported_game = false;
        self.bvb_shared_side = None;
        self.info = SearchInfo::default();
        self.live_eval_cp = Some(0);
        self.live_eval_stale = false;
        self.search_stm = None;
        self.status = "Choose a game mode to start".into();
    }

    pub fn load_fen(&mut self, fen: &str) -> Result<(), String> {
        self.stop_thinking_quiet();
        self.stop_eval_quiet();
        self.stop_post_game_quiet();
        self.board = Board::from_fen(fen).map_err(|e| e.to_string())?;
        self.move_stack.clear();
        self.last_move = None;
        self.analyzed = Some(empty_analyzed(self.board.to_fen()));
        // Bare FEN is a study position: enter Analyze so the mode picker closes.
        self.mode = Some(PlayMode::Analyze);
        self.imported_game = true;
        self.info = SearchInfo::default();
        self.live_eval_cp = None;
        self.live_eval_stale = true;
        self.search_stm = None;
        self.status = "Loaded FEN · Analyze · G for hints".into();
        Ok(())
    }

    /// Load a browseable game, jump to the final ply, switch to Analyze, and start post-game analysis.
    pub fn load_analyzed_game(&mut self, mut game: AnalyzedGame) -> Result<(), String> {
        self.stop_thinking_quiet();
        self.stop_eval_quiet();
        self.stop_post_game_quiet();
        game.cursor = game.plies.len();
        self.sync_from_game(&game)?;
        let n = game.ply_count();
        for ply in &mut game.plies {
            ply.analysis = None;
        }
        self.analyzed = Some(game);
        self.imported_game = true;
        self.mode = Some(PlayMode::Analyze);
        self.info = SearchInfo::default();
        self.live_eval_cp = None;
        self.live_eval_stale = false;
        if n > 0 {
            self.start_post_game_analysis(self.analysis_limits);
        } else {
            self.status = format!("Imported {n} plies · ←/→ step · Analyze");
        }
        Ok(())
    }

    /// Begin (or restart) sequential engine analysis of the loaded game.
    pub fn start_post_game_analysis(&mut self, limits: GoLimits) {
        self.stop_post_game_quiet();
        let Some(game) = self.analyzed.as_mut() else {
            return;
        };
        let total = game.ply_count();
        if total == 0 {
            return;
        }
        for ply in &mut game.plies {
            ply.analysis = None;
        }
        self.post_game = Some(PostGameState {
            next_step: 0,
            total_plies: total,
            limits,
            prev_eval: None,
            pending_best_move: None,
        });
        self.status = format!("Analyzing 0/{total}… · ←/→ step");
        // First search starts on the next `poll()` tick so load stays responsive.
    }

    pub fn is_post_game_analyzing(&self) -> bool {
        self.post_game.is_some()
    }

    /// Cancel an in-progress post-game analysis pass.
    pub fn cancel_post_game_analysis(&mut self) {
        self.stop_post_game_quiet();
    }

    pub fn apply_move_list(&mut self, list: &str) -> Result<(), String> {
        for (i, tok) in list.split_whitespace().enumerate() {
            self.play_text(tok)
                .map_err(|e| format!("move {} ({tok}): {e}", i + 1))?;
        }
        self.status = format!("Applied moves · {}", self.mode_title());
        Ok(())
    }

    /// Build plies by resolving SAN/UCI tokens from a start FEN (does not mutate session).
    pub fn resolve_move_tokens(
        start_fen: &str,
        tokens: &[String],
    ) -> Result<Vec<PlyRecord>, String> {
        let mut board = if start_fen.is_empty() || start_fen == "startpos" {
            Board::startpos()
        } else {
            Board::from_fen(start_fen).map_err(|e| e.to_string())?
        };
        let mut plies = Vec::with_capacity(tokens.len());
        for (i, tok) in tokens.iter().enumerate() {
            let mv = resolve_player_move(&board, tok)
                .map_err(|e| format!("move {} ({tok}): {e}", i + 1))?;
            plies.push(PlyRecord::new(mv, tok.clone()));
            board.make(mv);
        }
        Ok(plies)
    }

    pub fn goto_ply(&mut self, ply: usize) -> bool {
        let Some(game) = self.analyzed.as_ref() else {
            self.status = "No imported game to browse".into();
            return false;
        };
        let max = game.ply_count();
        let target = ply.min(max);
        let mut game = self.analyzed.take().unwrap();
        game.cursor = target;
        match self.sync_from_game(&game) {
            Ok(()) => {
                self.analyzed = Some(game);
                self.stop_thinking_quiet();
                self.stop_eval_quiet();
                self.status = format!("Ply {}/{}", target, max);
                true
            }
            Err(e) => {
                self.analyzed = Some(game);
                self.status = e;
                false
            }
        }
    }

    pub fn step_back(&mut self) -> bool {
        let cursor = self.analyzed.as_ref().map(|g| g.cursor).unwrap_or(0);
        if cursor == 0 {
            if self.analyzed.is_some() {
                self.status = "At start of game".into();
            }
            return false;
        }
        self.goto_ply(cursor - 1)
    }

    pub fn step_forward(&mut self) -> bool {
        let Some(game) = self.analyzed.as_ref() else {
            self.status = "No imported game to browse".into();
            return false;
        };
        if game.cursor >= game.ply_count() {
            self.status = "At end of game".into();
            return false;
        }
        let next = game.cursor + 1;
        self.goto_ply(next)
    }

    pub fn goto_start(&mut self) -> bool {
        self.goto_ply(0)
    }

    pub fn goto_end(&mut self) -> bool {
        let max = self.analyzed.as_ref().map(|g| g.ply_count()).unwrap_or(0);
        self.goto_ply(max)
    }

    fn sync_from_game(&mut self, game: &AnalyzedGame) -> Result<(), String> {
        let mut board = Board::from_fen(&game.start_fen).map_err(|e| e.to_string())?;
        self.move_stack.clear();
        for ply in game.plies.iter().take(game.cursor) {
            board.make(ply.mv);
            self.move_stack.push(ply.mv);
        }
        self.board = board;
        self.last_move = game.last_move_at_cursor();
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        if let Some(game) = self.analyzed.as_ref() {
            if game.cursor < game.plies.len() {
                // Mid-browse: step the cursor without deleting plies.
                return self.step_back();
            }
            if game.plies.is_empty() {
                self.status = "Nothing to undo".into();
                return false;
            }
            let mut game = self.analyzed.take().unwrap();
            game.plies.pop();
            game.cursor = game.plies.len();
            return match self.sync_from_game(&game) {
                Ok(()) => {
                    self.analyzed = Some(game);
                    self.stop_thinking_quiet();
                    self.stop_eval_quiet();
                    self.live_eval_stale = true;
                    if self.move_stack.is_empty() {
                        self.live_eval_cp = Some(0);
                        self.live_eval_stale = false;
                    }
                    self.status = "Undid last move".into();
                    true
                }
                Err(e) => {
                    self.analyzed = Some(game);
                    self.status = e;
                    false
                }
            };
        }
        if let Some(m) = self.move_stack.pop() {
            self.board.unmake(m);
            self.last_move = self.move_stack.last().copied();
            self.stop_thinking_quiet();
            self.stop_eval_quiet();
            self.live_eval_stale = true;
            if self.move_stack.is_empty() {
                self.live_eval_cp = Some(0);
                self.live_eval_stale = false;
            }
            self.status = "Undid last move".into();
            true
        } else {
            self.status = "Nothing to undo".into();
            false
        }
    }

    fn stop_thinking_quiet(&mut self) {
        if let Some(mut live) = self.live.take() {
            live.shutdown();
        }
        self.info.thinking = false;
        self.go_started = None;
        self.pending_limits = None;
        self.search_stm = None;
    }

    fn stop_eval_quiet(&mut self) {
        if let Some(mut live) = self.eval_live.take() {
            live.shutdown();
        }
        self.eval_stm = None;
        self.eval_position_key = None;
    }

    fn stop_post_game_quiet(&mut self) {
        if let Some(mut live) = self.analysis_live.take() {
            live.shutdown();
        }
        self.analysis_stm = None;
        self.post_game = None;
    }

    /// Build a board at the position after `ply_count` plies of the analyzed game.
    fn board_after_plies(game: &AnalyzedGame, ply_count: usize) -> Result<Board, String> {
        let mut board = Board::from_fen(&game.start_fen).map_err(|e| e.to_string())?;
        for ply in game.plies.iter().take(ply_count) {
            board.make(ply.mv);
        }
        Ok(board)
    }

    fn kick_post_game_search(&mut self) {
        if self.analysis_live.is_some() {
            return;
        }
        let Some(state) = self.post_game.as_ref() else {
            return;
        };
        let step = state.next_step;
        let total = state.total_plies;
        let limits = state.limits;
        if step > total {
            self.post_game = None;
            self.status = format!("Analysis complete · {total} plies · ←/→ step");
            return;
        }

        let Some(game) = self.analyzed.as_ref() else {
            self.post_game = None;
            return;
        };
        let board = match Self::board_after_plies(game, step) {
            Ok(b) => b,
            Err(e) => {
                self.post_game = None;
                self.status = format!("Analysis failed: {e}");
                return;
            }
        };
        self.analysis_stm = Some(board.side_to_move());
        let search_limits = limits.to_search_limits();
        self.analysis_live = Some(self.spawn_search(board, search_limits));
        if step == 0 {
            self.status = format!("Analyzing 0/{total}… · ←/→ step");
        } else {
            self.status = format!("Analyzing {step}/{total}… · ←/→ step");
        }
    }

    /// Play a single player move: SAN (`e4`, `Nf3`, `O-O`) or UCI (`e2e4`).
    pub fn play_text(&mut self, text: &str) -> Result<(), String> {
        if self.mode.is_none() {
            return Err("Choose a game mode first".into());
        }
        let mv = resolve_player_move(&self.board, text)?;
        self.play_move(mv)
    }

    pub fn play_move(&mut self, mv: Move) -> Result<(), String> {
        if self.is_game_over() {
            return Err(self.game_result().status_message().into());
        }
        self.ensure_analyzed();
        let san = format_san(&self.board, mv);
        if let Some(game) = self.analyzed.as_mut() {
            if game.cursor < game.plies.len() {
                game.plies.truncate(game.cursor);
            }
            game.plies.push(PlyRecord::new(mv, san));
            game.cursor = game.plies.len();
        }
        // Legality already checked for player input; bot picks from legal_moves.
        self.board.make(mv);
        self.move_stack.push(mv);
        self.last_move = Some(mv);
        self.stop_eval_quiet();
        self.live_eval_stale = true;
        let result = self.board.game_result();
        self.status = if result.is_over() {
            result.status_message().into()
        } else {
            format!("Played {mv}")
        };
        Ok(())
    }

    fn ensure_analyzed(&mut self) {
        if self.analyzed.is_none() {
            self.analyzed = Some(empty_analyzed(self.board.to_fen()));
        }
    }

    pub fn go(&mut self, limits: GoLimits) {
        if self.mode.is_none() {
            self.status = "Choose a game mode first".into();
            return;
        }
        self.start_bot_search(limits);
    }

    /// Background search that only updates live eval (does not move or block bot search).
    pub fn go_eval(&mut self, limits: GoLimits) {
        let key = self.position_key();
        if self.eval_live.is_some() {
            if self.eval_position_key == Some(key) {
                // Already evaluating this position.
                return;
            }
            // Stale eval for a previous position — restart.
            self.stop_eval_quiet();
        }
        self.start_eval_search(limits);
    }

    fn spawn_search(&self, board: Board, search_limits: Limits) -> LiveSearch {
        LiveSearch::spawn(board, search_limits, self.hash_mb)
    }

    /// Probe the opening book before searching. On a hit, play the move
    /// immediately and report it; returns `true` when the book handled the move.
    ///
    /// Only used when the bot actually plays (never in Analyze), so book moves
    /// do not hijack manual analysis (TUI-04).
    fn try_book_move(&mut self) -> bool {
        if !self.search_applies_move() {
            return false;
        }
        let ply = self.move_stack.len() as u32;
        let Some(mv) = self.book.probe(&self.board, ply, &mut self.book_rng) else {
            return false;
        };
        self.search_stm = Some(self.board.side_to_move());
        self.info = SearchInfo::default();
        match self.play_move(mv) {
            Ok(()) => {
                self.info.bestmove = None;
                if !self.is_game_over() {
                    self.status = format!("Book: {mv}");
                }
            }
            Err(e) => {
                self.status = format!("Book move failed: {e}");
            }
        }
        true
    }

    fn start_bot_search(&mut self, limits: GoLimits) {
        if self.info.thinking {
            self.status = "Already thinking".into();
            return;
        }
        let result = self.board.game_result();
        if result.is_over() {
            self.status = result.status_message().into();
            return;
        }
        if self.try_book_move() {
            return;
        }
        self.apply_on_finish = self.search_applies_move();
        self.search_stm = Some(self.board.side_to_move());
        self.info = SearchInfo {
            thinking: true,
            depth: 0,
            score_cp: 0,
            nodes: 0,
            time: Duration::ZERO,
            pv: String::new(),
            bestmove: None,
        };
        self.go_started = Some(Instant::now());
        self.pending_limits = Some(limits);
        self.status = if self.apply_on_finish {
            "Bot thinking…".into()
        } else {
            "Analyzing (will not move)…".into()
        };

        let mut search_limits = limits.to_search_limits();
        // Opening-phase floor: on a book miss early in the game, ensure the bot
        // searches at least to the floor depth before the time abort (P10-04).
        if self.opening_floor_depth > 0
            && (self.move_stack.len() as u32) <= crate::search::OPENING_PHASE_PLIES
        {
            search_limits.min_opening_depth = Some(self.opening_floor_depth);
        }
        self.live = Some(self.spawn_search(self.board.clone(), search_limits));
    }

    fn start_eval_search(&mut self, limits: GoLimits) {
        self.eval_stm = Some(self.board.side_to_move());
        self.eval_position_key = Some(self.position_key());
        let search_limits = limits.to_search_limits();
        self.eval_live = Some(self.spawn_search(self.board.clone(), search_limits));
    }

    pub fn stop(&mut self) {
        if self.info.thinking {
            if let Some(live) = &self.live {
                live.request_stop();
            }
            self.finish_search(true);
        }
    }

    pub fn poll(&mut self) {
        self.poll_bot();
        self.poll_eval();
        self.poll_post_game();
    }

    fn poll_bot(&mut self) {
        if !self.info.thinking {
            return;
        }
        let Some(started) = self.go_started else {
            return;
        };
        self.info.time = started.elapsed();

        if let Some(live) = self.live.as_ref() {
            let snap = live.snapshot_live();
            if snap.depth > 0 || snap.nodes > 0 || !snap.pv.is_empty() {
                self.info.depth = snap.depth;
                self.info.score_cp = snap.score_cp;
                self.info.nodes = snap.nodes;
                // Keep the wall-clock live timer (info.time) set above; never
                // stream the PV during bot play.
                self.info.pv = if self.show_engine_hints() {
                    snap.pv
                } else {
                    String::new()
                };
            }
        }

        let ready = self.live.as_ref().map(LiveSearch::is_ready).unwrap_or(false);
        if ready {
            self.finish_search(false);
        }
    }

    fn poll_eval(&mut self) {
        if self.eval_live.is_none() {
            return;
        }

        let ready = self
            .eval_live
            .as_ref()
            .map(LiveSearch::is_ready)
            .unwrap_or(false);

        if !ready {
            return;
        }

        let key = self.eval_position_key;
        let stm = self.eval_stm;
        let result = self.eval_live.take().unwrap().take_result();

        self.eval_stm = None;
        self.eval_position_key = None;

        // Drop stale results if the position changed (should be rare after cancel).
        if key != Some(self.position_key()) {
            return;
        }

        if let (Some(result), Some(stm)) = (result, stm) {
            self.live_eval_cp = Some(stm_score_to_white(result.score, stm));
            self.live_eval_stale = false;
        }
    }

    fn poll_post_game(&mut self) {
        if self.post_game.is_none() {
            return;
        }

        if self.analysis_live.is_none() {
            self.kick_post_game_search();
            return;
        }

        let ready = self
            .analysis_live
            .as_ref()
            .map(LiveSearch::is_ready)
            .unwrap_or(false);

        if !ready {
            return;
        }

        let stm = self.analysis_stm;
        let result = self.analysis_live.take().unwrap().take_result();
        self.analysis_stm = None;

        let Some(result) = result else {
            self.post_game = None;
            self.status = "Analysis search failed".into();
            return;
        };
        let Some(stm) = stm else {
            self.post_game = None;
            self.status = "Analysis search failed".into();
            return;
        };

        let eval_cp = stm_score_to_white(result.score, stm);
        let cached_best = (!result.best_move.is_none()).then_some(result.best_move);
        let (next_step, total) = {
            let Some(state) = self.post_game.as_mut() else {
                return;
            };
            let step = state.next_step;
            let total = state.total_plies;

            if step == 0 {
                state.prev_eval = Some(eval_cp);
                state.pending_best_move = cached_best;
                state.next_step = 1;
            } else {
                let ply_idx = step - 1;
                let prev = state.prev_eval.unwrap_or(eval_cp);
                // After White moves, Black is to move (and vice versa).
                let white_moved = stm == Color::Black;
                let cpl = cpl_from_eval_swing(prev, eval_cp, white_moved);
                let best_move = state.pending_best_move;
                if let Some(game) = self.analyzed.as_mut() {
                    let played = game.plies.get(ply_idx).map(|p| p.mv);
                    let prev_analysis = ply_idx
                        .checked_sub(1)
                        .and_then(|i| game.plies.get(i))
                        .and_then(|p| p.analysis.as_ref());
                    let opponent_bad = opponent_was_bad(prev_analysis);
                    if let (Some(played), Ok(board_before)) = (
                        played,
                        Self::board_after_plies(game, ply_idx),
                    ) {
                        // Tag opening theory regardless of the OwnBook flag so
                        // book moves show the BK glyph (OPEN-01).
                        let in_book = Book::embedded().is_book_move(&board_before, played);
                        let classification = classify_move(ClassifyInput {
                            cpl,
                            prev_eval: prev,
                            after_eval: eval_cp,
                            white_moved,
                            played,
                            best_move,
                            board_before: &board_before,
                            opponent_was_bad: opponent_bad,
                            in_book,
                        });
                        if let Some(ply) = game.plies.get_mut(ply_idx) {
                            ply.analysis = Some(PlyAnalysis {
                                eval_cp,
                                classification,
                                cpl,
                                best_move,
                            });
                        }
                    }
                }
                state.prev_eval = Some(eval_cp);
                state.pending_best_move = cached_best;
                state.next_step = step + 1;
            }
            (state.next_step, total)
        };

        if next_step > total {
            self.post_game = None;
            self.status = format!("Analysis complete · {total} plies · ←/→ step");
        } else {
            let done = next_step.saturating_sub(1).min(total);
            self.status = format!("Analyzing {done}/{total}… · ←/→ step");
            self.kick_post_game_search();
        }
    }

    fn finish_search(&mut self, stopped: bool) {
        if let Some(mut live) = self.live.take() {
            let result = live.take_result();
            self.apply_search_result(result, stopped);
        } else {
            self.info.thinking = false;
            self.go_started = None;
            self.pending_limits = None;
            self.status = "No legal moves".into();
        }
    }

    fn apply_search_result(&mut self, result: Option<SearchResult>, stopped: bool) {
        self.info.thinking = false;
        self.go_started = None;
        self.pending_limits = None;
        let apply = self.apply_on_finish;

        let Some(result) = result else {
            self.search_stm = None;
            self.status = "Search failed".into();
            return;
        };

        if let Some(stm) = self.search_stm.take() {
            self.live_eval_cp = Some(stm_score_to_white(result.score, stm));
        }

        self.info.depth = result.depth.max(0) as u32;
        self.info.score_cp = result.score;
        self.info.nodes = result.nodes;
        self.info.time = result.time;
        // Keep PV only when Analyze hints are allowed (not bot play).
        if self.show_engine_hints() && !apply {
            self.info.pv = result
                .pv
                .iter()
                .map(|m| m.to_string())
                .collect::<Vec<_>>()
                .join(" ");
        } else {
            self.info.pv.clear();
        }

        if result.best_move.is_none() {
            self.live_eval_stale = false;
            let ended = self.board.game_result();
            self.status = if ended.is_over() {
                ended.status_message().into()
            } else {
                "No legal moves".into()
            };
            return;
        }

        let mv = result.best_move;
        if apply {
            // Bot already plays this move — don't leave it as a board/panel hint.
            self.info.bestmove = None;
            match self.play_move(mv) {
                Ok(()) => {
                    // play_move already sets draw/mate status when the game ended.
                    if !self.is_game_over() {
                        self.status = if stopped {
                            format!("Stopped → played {mv}")
                        } else {
                            format!("Bot plays {mv}")
                        };
                    }
                }
                Err(e) => {
                    self.live_eval_stale = false;
                    self.status = format!("Engine move failed: {e}");
                }
            }
        } else {
            self.info.bestmove = Some(mv.to_string());
            self.live_eval_stale = false;
            self.status = format!("Best move: {mv} (board unchanged)");
        }
    }
}

impl Default for EngineSession {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_analyzed(start_fen: String) -> AnalyzedGame {
    AnalyzedGame::new(start_fen, Vec::new(), GameHeaders::default())
}

/// Resolve one player move: UCI or simple SAN against legal moves.
pub fn resolve_player_move(board: &Board, text: &str) -> Result<Move, String> {
    let raw = text.trim();
    if raw.is_empty() {
        return Err("enter one move (e.g. e4 or e2e4)".into());
    }
    let s = raw
        .trim_end_matches(|c| matches!(c, '+' | '#' | '!' | '?'))
        .to_string();

    // Prefer real UCI resolution (correct flags).
    if (s.len() == 4 || s.len() == 5) && board.parse_uci_move(&s).is_ok() {
        return board.parse_uci_move(&s).map_err(|e| e.to_string());
    }
    if let Ok(m) = board.parse_uci_move(&s.to_ascii_lowercase()) {
        return Ok(m);
    }

    resolve_san(board, &s)
}

fn resolve_san(board: &Board, san: &str) -> Result<Move, String> {
    let s = san.trim();
    let lower = s.to_ascii_lowercase();
    let legal = board.legal_moves();
    if legal.is_empty() {
        return Err("no legal moves".into());
    }

    // Castling
    if matches!(lower.as_str(), "o-o" | "0-0" | "oo") {
        return legal
            .iter()
            .copied()
            .find(|m| m.is_castling() && m.to().file() == 6)
            .ok_or_else(|| "O-O is not legal here".into());
    }
    if matches!(lower.as_str(), "o-o-o" | "0-0-0" | "ooo") {
        return legal
            .iter()
            .copied()
            .find(|m| m.is_castling() && m.to().file() == 2)
            .ok_or_else(|| "O-O-O is not legal here".into());
    }

    let mut rest = s.trim_end_matches(|c| matches!(c, '+' | '#'));
    // Promotion: e8=Q / e8Q / e7e8q already handled as UCI
    let mut promo: Option<PieceType> = None;
    if let Some(idx) = rest.rfind('=') {
        let (left, right) = rest.split_at(idx);
        promo = parse_promo_char(right.chars().nth(1).unwrap_or(' '))?;
        rest = left;
    } else if rest.len() >= 2 {
        let last = rest.chars().last().unwrap();
        if matches!(last, 'Q' | 'R' | 'B' | 'N' | 'q' | 'r' | 'b' | 'n')
            && rest.chars().nth(rest.len().saturating_sub(2)) != Some('x')
        {
            // e8Q style — only if preceding looks like a square
            if rest.len() >= 3 {
                let maybe_sq = &rest[rest.len() - 3..rest.len() - 1];
                if Square::from_str(maybe_sq).is_ok() {
                    promo = parse_promo_char(last)?;
                    rest = &rest[..rest.len() - 1];
                }
            }
        }
    }

    let capture = rest.contains('x');
    let body = rest.replace('x', "");

    let (piece, disambig, to_str) = parse_san_body(&body)?;
    let to = Square::from_str(to_str).map_err(|_| format!("bad destination: {to_str}"))?;

    let mut matches: Vec<Move> = legal
        .into_iter()
        .filter(|m| m.to() == to)
        .filter(|m| m.promotion_piece() == promo)
        .filter(|m| {
            let moving = board.piece_on(m.from());
            moving.piece_type() == Some(piece)
        })
        .filter(|m| {
            if !capture {
                return true;
            }
            // Capture SAN: destination occupied or EP
            !board.piece_on(m.to()).is_empty() || m.is_en_passant()
        })
        .filter(|m| match disambig {
            Disambig::None => true,
            Disambig::File(f) => m.from().file() == f,
            Disambig::Rank(r) => m.from().rank() == r,
            Disambig::Square(sq) => m.from() == sq,
        })
        .collect();

    // Pawn pushes: SAN "e4" has no capture; also allow non-capture filter looseness
    if !capture && piece == PieceType::Pawn {
        matches.retain(|m| board.piece_on(m.to()).is_empty() && !m.is_en_passant());
    }

    match matches.len() {
        1 => Ok(matches[0]),
        0 => Err(format!("no legal move matches '{san}' — try e4 or e2e4")),
        _ => Err(format!(
            "ambiguous move '{san}' — add file/rank (Nbd2) or use UCI"
        )),
    }
}

enum Disambig {
    None,
    File(u8),
    Rank(u8),
    Square(Square),
}

fn parse_promo_char(c: char) -> Result<Option<PieceType>, String> {
    Ok(Some(match c {
        'Q' | 'q' => PieceType::Queen,
        'R' | 'r' => PieceType::Rook,
        'B' | 'b' => PieceType::Bishop,
        'N' | 'n' => PieceType::Knight,
        _ => return Err(format!("bad promotion piece: {c}")),
    }))
}

/// Parse SAN body without `x` / promo into (piece, disambiguation, to_square_str).
fn parse_san_body(body: &str) -> Result<(PieceType, Disambig, &str), String> {
    if body.len() < 2 {
        return Err(format!("bad move: {body}"));
    }
    let bytes = body.as_bytes();

    // Pawn: e4 / de5 / d5 (after x stripped: de5)
    let first = body.chars().next().unwrap();
    if first.is_ascii_lowercase() && first.is_ascii_alphabetic() {
        // Entire thing ends with destination square
        if body.len() == 2 {
            return Ok((PieceType::Pawn, Disambig::None, body));
        }
        if body.len() == 3 {
            // file + to square: de5
            let file = (bytes[0] - b'a') as u8;
            if file < 8 {
                return Ok((PieceType::Pawn, Disambig::File(file), &body[1..]));
            }
        }
        return Err(format!("bad pawn move: {body}"));
    }

    // Piece move: Nf3, Nbd2, N1d2, Ng1f3
    let piece = match first {
        'K' => PieceType::King,
        'Q' => PieceType::Queen,
        'R' => PieceType::Rook,
        'B' => PieceType::Bishop,
        'N' => PieceType::Knight,
        _ => return Err(format!("unknown piece in '{body}' (use Nf3, not nf3)")),
    };
    let rest = &body[1..];
    if rest.len() < 2 {
        return Err(format!("bad move: {body}"));
    }
    let to = &rest[rest.len() - 2..];
    let mid = &rest[..rest.len() - 2];
    let dis = if mid.is_empty() {
        Disambig::None
    } else if mid.len() == 1 {
        let c = mid.as_bytes()[0];
        if (b'a'..=b'h').contains(&c) {
            Disambig::File(c - b'a')
        } else if (b'1'..=b'8').contains(&c) {
            Disambig::Rank(c - b'1')
        } else {
            return Err(format!("bad disambiguation in '{body}'"));
        }
    } else if mid.len() == 2 {
        Disambig::Square(Square::from_str(mid).map_err(|_| format!("bad from-square in '{body}'"))?)
    } else {
        return Err(format!("bad move: {body}"));
    };
    Ok((piece, dis, to))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_starts_without_mode() {
        let s = EngineSession::new();
        assert_eq!(s.mode(), None);
        assert!(s.needs_mode_picker());
    }

    #[test]
    fn new_game_clears_mode() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e4").unwrap();
        s.new_game();
        assert_eq!(s.mode(), None);
        assert!(s.needs_mode_picker());
        assert!(s.move_stack.is_empty());
    }

    #[test]
    fn play_text_requires_mode() {
        let mut s = EngineSession::new();
        assert_eq!(
            s.play_text("e4").unwrap_err(),
            "Choose a game mode first"
        );
    }

    #[test]
    fn new_game_live_eval_starts_at_zero() {
        let s = EngineSession::new();
        assert_eq!(s.current_eval(), Some(0));
        assert!(!s.live_eval_stale());
    }

    #[test]
    fn engine_hints_only_in_analyze_mode() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsBot {
            human: Color::White,
        });
        assert!(!s.show_engine_hints());
        s.set_mode(PlayMode::BotVsBot);
        assert!(!s.show_engine_hints());
        s.set_mode(PlayMode::PlayerVsPlayer);
        assert!(!s.show_engine_hints());
        s.set_mode(PlayMode::Analyze);
        assert!(s.show_engine_hints());
    }

    #[test]
    fn play_move_marks_live_eval_stale() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e4").unwrap();
        assert!(s.live_eval_stale());
    }

    #[test]
    fn eval_search_does_not_block_bot_go() {
        let mut s = EngineSession::new();
        // Disable the book so this exercises the search-threading path.
        s.book = Book::disabled();
        s.set_mode(PlayMode::PlayerVsBot {
            human: Color::White,
        });
        s.play_text("e4").unwrap();
        assert!(s.engine_should_auto_move());

        // Long eval search on its own thread.
        s.go_eval(GoLimits {
            depth: Some(64),
            movetime: Some(Duration::from_millis(5_000)),
        });
        assert!(s.eval_live.is_some());
        assert!(!s.is_thinking());

        // Bot search must still be able to start.
        s.go(GoLimits {
            depth: Some(1),
            movetime: Some(Duration::from_millis(50)),
        });
        assert!(s.is_thinking());
        assert!(s.live.is_some());
        assert!(s.eval_live.is_some());

        s.stop_thinking_quiet();
        s.stop_eval_quiet();
    }

    #[test]
    fn bot_plays_book_move_instantly() {
        crate::lookup::initialize();
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::BotVsBot);
        assert_eq!(s.move_stack.len(), 0);

        // Book is enabled by default; the first BvB move should come from book
        // with no background search spawned.
        s.go(GoLimits {
            depth: Some(20),
            movetime: Some(Duration::from_millis(5_000)),
        });
        assert!(!s.is_thinking(), "book move should not spawn a search");
        assert!(s.live.is_none());
        assert_eq!(s.move_stack.len(), 1, "book move should be applied");
        assert!(s.status().starts_with("Book:"), "status: {}", s.status());
        let played = s.last_move().unwrap();
        assert!(!matches!(
            played.to_string().as_str(),
            "a2a3" | "a2a4" | "h2h3" | "h2h4"
        ));
    }

    #[test]
    fn disabled_book_falls_through_to_search() {
        crate::lookup::initialize();
        let mut s = EngineSession::new();
        s.book = Book::disabled();
        s.set_mode(PlayMode::BotVsBot);
        s.go(GoLimits {
            depth: Some(1),
            movetime: Some(Duration::from_millis(200)),
        });
        // No book: a real search is spawned (thinking) and no move applied yet.
        assert!(s.is_thinking());
        assert!(s.live.is_some());
        assert_eq!(s.move_stack.len(), 0);
        s.stop_thinking_quiet();
    }

    #[test]
    fn analyze_mode_never_uses_book() {
        crate::lookup::initialize();
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::Analyze);
        s.go(GoLimits {
            depth: Some(1),
            movetime: Some(Duration::from_millis(200)),
        });
        // Analyze must search (for hints), never auto-play a book move.
        assert!(s.is_thinking());
        assert_eq!(s.move_stack.len(), 0);
        s.stop_thinking_quiet();
    }

    #[test]
    fn play_move_cancels_eval_so_stale_result_is_dropped() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.set_eval_bar_forced(true);

        s.go_eval(GoLimits {
            depth: Some(64),
            movetime: Some(Duration::from_millis(5_000)),
        });
        assert!(s.eval_live.is_some());
        let before = s.live_eval_cp;

        // Moving cancels the in-flight eval; a later poll must not overwrite.
        s.play_text("e4").unwrap();
        assert!(s.eval_live.is_none());
        assert!(s.live_eval_stale());

        s.poll();
        // Cached eval is unchanged by a cancelled worker (stale flag stays set
        // until a fresh eval for the new position finishes).
        assert_eq!(s.live_eval_cp, before);
        assert!(s.live_eval_stale());

        s.stop_eval_quiet();
    }

    #[test]
    fn go_eval_skips_when_already_running_for_same_position() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);

        s.go_eval(GoLimits {
            depth: Some(64),
            movetime: Some(Duration::from_millis(5_000)),
        });
        let key = s.eval_position_key;
        assert!(key.is_some());

        s.go_eval(GoLimits {
            depth: Some(1),
            movetime: Some(Duration::from_millis(50)),
        });
        // Still the original long search — not restarted.
        assert_eq!(s.eval_position_key, key);

        s.stop_eval_quiet();
    }

    #[test]
    fn live_play_appends_to_move_panel() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e4").unwrap();
        s.play_text("e5").unwrap();
        let game = s.analyzed().unwrap();
        assert_eq!(game.plies.len(), 2);
        assert_eq!(game.cursor, 2);
        assert_eq!(game.plies[0].san, "e4");
        assert_eq!(game.plies[1].san, "e5");
    }

    #[test]
    fn undo_at_tip_truncates_transcript() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e4").unwrap();
        s.play_text("e5").unwrap();
        assert!(s.undo());
        let game = s.analyzed().unwrap();
        assert_eq!(game.plies.len(), 1);
        assert_eq!(game.cursor, 1);
        assert_eq!(game.plies[0].san, "e4");
        assert_eq!(s.side_to_move(), Color::Black);
    }

    #[test]
    fn accepts_short_pawn_move() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e4").unwrap();
        assert_eq!(s.side_to_move(), Color::Black);
        assert_eq!(
            s.piece_on(Square::from_str("e4").unwrap()),
            Piece::WhitePawn
        );
    }

    #[test]
    fn accepts_uci_still() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsPlayer);
        s.play_text("e2e4").unwrap();
        assert_eq!(s.side_to_move(), Color::Black);
    }

    #[test]
    fn vs_bot_one_move_then_bot_turn() {
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsBot {
            human: Color::White,
        });
        s.play_text("e4").unwrap();
        assert!(s.engine_should_auto_move());
        assert!(!s.is_human_turn());
    }

    #[test]
    fn threefold_repetition_ends_game_and_stops_bots() {
        crate::lookup::initialize();
        let mut s = EngineSession::new();
        s.set_mode(PlayMode::BotVsBot);
        let cycle = ["b1c3", "b8c6", "c3b1", "c6b8"];
        for _ in 0..2 {
            for mv in cycle {
                s.play_text(mv).unwrap();
            }
        }
        assert!(s.is_game_over());
        assert_eq!(s.game_result(), GameResult::DrawRepetition);
        assert!(!s.engine_should_auto_move());
        assert!(s.status().contains("repetition"));
        assert!(s.play_text("g1f3").is_err());
    }

    #[test]
    fn browse_imported_game_steps() {
        crate::lookup::initialize();
        let tokens = ["e4", "e5", "Nf3", "Nc6"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let plies = EngineSession::resolve_move_tokens(fen, &tokens).unwrap();
        let game = super::super::game::AnalyzedGame::new(
            fen.into(),
            plies,
            super::super::game::GameHeaders::default(),
        );
        let mut s = EngineSession::new();
        // Shallow limits so the background pass stays cheap if it starts.
        s.analysis_limits = GoLimits {
            depth: Some(1),
            movetime: Some(Duration::from_millis(50)),
        };
        s.load_analyzed_game(game).unwrap();
        assert_eq!(s.analyzed().unwrap().cursor, 4);
        assert_eq!(s.mode(), Some(PlayMode::Analyze));
        assert!(s.is_post_game_analyzing());
        // Cancel so browsing assertions are not blocked by the worker.
        s.cancel_post_game_analysis();
        // Evals were cleared / never finished.
        assert!(s.current_eval().is_none());

        assert!(s.step_back());
        assert_eq!(s.analyzed().unwrap().cursor, 3);
        assert!(s.goto_start());
        assert_eq!(s.analyzed().unwrap().cursor, 0);
        assert!(s.last_move().is_none());
        assert!(s.step_forward());
        assert_eq!(s.analyzed().unwrap().cursor, 1);
        assert!(s.goto_end());
        assert_eq!(s.analyzed().unwrap().cursor, 4);
    }

    #[test]
    fn takeover_from_pvb_white_keeps_black_shared() {
        let mut cfg = crate::config::Config::default();
        cfg.bot.depth = 8;
        cfg.bot.movetime_ms = 450;
        cfg.bot.white.depth = 12;
        cfg.bot.white.movetime_ms = 5000;
        cfg.bot.black.depth = 2;
        cfg.bot.black.movetime_ms = 100;

        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsBot {
            human: Color::White,
        });
        s.take_over_with_bot();
        assert_eq!(s.mode(), Some(PlayMode::BotVsBot));
        assert_eq!(s.bvb_shared_side(), Some(Color::Black));

        // White (taking over) uses Bot vs Bot White settings.
        assert_eq!(s.side_to_move(), Color::White);
        let white = s.play_go_limits(&cfg);
        assert_eq!(white.depth, Some(12));
        assert_eq!(white.movetime, Some(Duration::from_millis(5000)));

        s.play_text("e4").unwrap();
        assert_eq!(s.side_to_move(), Color::Black);
        // Black (former opponent bot) keeps shared PvB strength, not bot.black.
        let black = s.play_go_limits(&cfg);
        assert_eq!(black.depth, Some(8));
        assert_eq!(black.movetime, Some(Duration::from_millis(450)));
    }

    #[test]
    fn takeover_from_pvb_black_keeps_white_shared() {
        let mut cfg = crate::config::Config::default();
        cfg.bot.depth = 8;
        cfg.bot.white.depth = 12;
        cfg.bot.black.depth = 2;

        let mut s = EngineSession::new();
        s.set_mode(PlayMode::PlayerVsBot {
            human: Color::Black,
        });
        s.take_over_with_bot();
        assert_eq!(s.bvb_shared_side(), Some(Color::White));

        let white = s.play_go_limits(&cfg);
        assert_eq!(white.depth, Some(8));

        s.play_text("e4").unwrap();
        let black = s.play_go_limits(&cfg);
        assert_eq!(black.depth, Some(2));
    }

    #[test]
    fn fresh_bot_vs_bot_uses_both_side_settings() {
        let mut cfg = crate::config::Config::default();
        cfg.bot.depth = 8;
        cfg.bot.white.depth = 12;
        cfg.bot.black.depth = 2;

        let mut s = EngineSession::new();
        s.set_mode(PlayMode::BotVsBot);
        assert_eq!(s.bvb_shared_side(), None);
        assert_eq!(s.play_go_limits(&cfg).depth, Some(12));
        s.play_text("e4").unwrap();
        assert_eq!(s.play_go_limits(&cfg).depth, Some(2));
    }

    #[test]
    fn post_game_analysis_fills_plies_and_eval() {
        crate::lookup::initialize();
        let tokens = ["e4", "e5"]
            .into_iter()
            .map(String::from)
            .collect::<Vec<_>>();
        let fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
        let plies = EngineSession::resolve_move_tokens(fen, &tokens).unwrap();
        let game = super::super::game::AnalyzedGame::new(
            fen.into(),
            plies,
            super::super::game::GameHeaders::default(),
        );
        let mut s = EngineSession::new();
        s.analysis_limits = GoLimits {
            depth: Some(1),
            movetime: None,
        };
        s.load_analyzed_game(game).unwrap();
        assert!(s.is_post_game_analyzing());

        let deadline = Instant::now() + Duration::from_secs(30);
        while s.is_post_game_analyzing() && Instant::now() < deadline {
            s.poll();
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(
            !s.is_post_game_analyzing(),
            "post-game analysis should finish"
        );

        let game = s.analyzed().unwrap();
        assert_eq!(game.ply_count(), 2);
        for ply in &game.plies {
            let analysis = ply.analysis.as_ref().expect("every ply should have analysis");
            assert!(
                !analysis.classification.glyph().is_empty(),
                "classification should be assigned"
            );
            assert!(
                analysis.best_move.is_some(),
                "cached engine best move should be stored"
            );
        }

        s.goto_end();
        assert!(s.current_eval().is_some());
        s.goto_ply(1);
        assert_eq!(
            s.current_eval(),
            s.analyzed().unwrap().plies[0]
                .analysis
                .as_ref()
                .map(|a| a.eval_cp)
        );
    }
}
