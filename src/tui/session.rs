//! Session: real [`Board`] + play modes + engine search.
//!
//! Player input accepts one move: short algebraic (`e4`, `Nf3`, `O-O`) or UCI (`e2e4`).

use super::game::{AnalyzedGame, PlyRecord};
use crate::board::Board;
use crate::search::{self, Limits, SearchResult};
use crate::transposition::TranspositionTable;
use crate::types::{Color, Move, Piece, PieceType, Square};
use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default)]
pub struct GoLimits {
    pub depth: Option<u32>,
    pub movetime: Option<Duration>,
}

#[derive(Clone, Debug, Default)]
pub struct SearchInfo {
    pub depth: u32,
    pub score_cp: i32,
    pub nodes: u64,
    pub time: Duration,
    pub pv: String,
    pub thinking: bool,
    pub bestmove: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    PlayerVsPlayer,
    PlayerVsBot { human: Color },
    BotVsBot,
    Analyze,
}

impl PlayMode {
    pub fn title(self) -> &'static str {
        match self {
            PlayMode::PlayerVsPlayer => "Player vs Player",
            PlayMode::PlayerVsBot {
                human: Color::White,
            } => "Player vs Bot (you White)",
            PlayMode::PlayerVsBot {
                human: Color::Black,
            } => "Player vs Bot (you Black)",
            PlayMode::BotVsBot => "Bot vs Bot",
            PlayMode::Analyze => "Analyze (hint only)",
        }
    }

    pub fn blurb(self) -> &'static str {
        match self {
            PlayMode::PlayerVsPlayer => "You move both colors — one move at a time",
            PlayMode::PlayerVsBot {
                human: Color::White,
            } => "Enter your move; bot replies",
            PlayMode::PlayerVsBot {
                human: Color::Black,
            } => "Enter your move; bot replies",
            PlayMode::BotVsBot => "Engine plays both sides",
            PlayMode::Analyze => "g = best move (board unchanged)",
        }
    }
}

/// Background search job started by [`EngineSession::go`].
struct LiveSearch {
    stop: Arc<AtomicBool>,
    result: Arc<Mutex<Option<SearchResult>>>,
    handle: Option<JoinHandle<()>>,
}

pub struct EngineSession {
    board: Board,
    /// Moves applied this game (for undo via [`Board::unmake`]).
    move_stack: Vec<Move>,
    last_move: Option<Move>,
    mode: PlayMode,
    flipped: bool,
    info: SearchInfo,
    go_started: Option<Instant>,
    pending_limits: Option<GoLimits>,
    apply_on_finish: bool,
    status: String,
    /// Imported game for ply-by-ply browse (Analyze mode).
    analyzed: Option<AnalyzedGame>,
    /// User forced the eval bar on (also shown automatically while browsing an imported game).
    eval_bar_forced: bool,
    /// Active engine search, if any.
    live: Option<LiveSearch>,
}

impl EngineSession {
    pub fn new() -> Self {
        Self::new_with_config(&crate::config::Config::default())
    }

    /// Seed mode / flip / eval bar from user config.
    pub fn new_with_config(config: &crate::config::Config) -> Self {
        let mode = config.tui.default_mode.to_play_mode();
        Self {
            board: Board::startpos(),
            move_stack: Vec::new(),
            last_move: None,
            mode,
            flipped: config.tui.flip_board,
            info: SearchInfo::default(),
            go_started: None,
            pending_limits: None,
            apply_on_finish: true,
            status: format!(
                "{} — {} · , settings · ? help",
                mode.title(),
                mode.blurb()
            ),
            analyzed: None,
            eval_bar_forced: config.tui.show_eval_bar,
            live: None,
        }
    }

    /// Apply common TUI fields from config without resetting the board.
    pub fn apply_tui_config(&mut self, config: &crate::config::Config) {
        let mode = config.tui.default_mode.to_play_mode();
        if self.mode != mode {
            self.set_mode(mode);
        }
        self.flipped = config.tui.flip_board;
        self.eval_bar_forced = config.tui.show_eval_bar;
    }

    pub fn set_flipped(&mut self, flipped: bool) {
        self.flipped = flipped;
    }

    pub fn set_eval_bar_forced(&mut self, on: bool) {
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
        self.eval_bar_forced || self.analyzed.is_some()
    }

    pub fn toggle_eval_bar(&mut self) {
        if self.analyzed.is_some() {
            self.status = "Eval bar stays on while browsing an imported game".into();
            return;
        }
        self.eval_bar_forced = !self.eval_bar_forced;
        self.status = if self.eval_bar_forced {
            "Eval bar on (v to hide)".into()
        } else {
            "Eval bar off (v to show)".into()
        };
    }

    pub fn mode(&self) -> PlayMode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: PlayMode) {
        self.stop_thinking_quiet();
        self.mode = mode;
        self.flipped = matches!(
            mode,
            PlayMode::PlayerVsBot {
                human: Color::Black
            }
        );
        self.info.bestmove = None;
        self.status = format!("{} — {}", mode.title(), mode.blurb());
    }

    pub fn info(&self) -> &SearchInfo {
        &self.info
    }

    pub fn is_thinking(&self) -> bool {
        self.info.thinking
    }

    pub fn is_human_turn(&self) -> bool {
        match self.mode {
            PlayMode::PlayerVsPlayer | PlayMode::Analyze => true,
            PlayMode::PlayerVsBot { human } => self.board.side_to_move() == human,
            PlayMode::BotVsBot => false,
        }
    }

    pub fn engine_should_auto_move(&self) -> bool {
        match self.mode {
            PlayMode::PlayerVsPlayer | PlayMode::Analyze => false,
            PlayMode::PlayerVsBot { human } => self.board.side_to_move() != human,
            PlayMode::BotVsBot => true,
        }
    }

    pub fn search_applies_move(&self) -> bool {
        !matches!(self.mode, PlayMode::Analyze)
    }

    pub fn analyzed(&self) -> Option<&AnalyzedGame> {
        self.analyzed.as_ref()
    }

    /// White-relative eval for the eval bar (`None` = no analysis yet).
    pub fn current_eval(&self) -> Option<i32> {
        self.analyzed.as_ref().and_then(|g| g.current_eval())
    }

    pub fn new_game(&mut self) {
        self.stop_thinking_quiet();
        self.analyzed = None;
        self.board = Board::startpos();
        self.move_stack.clear();
        self.last_move = None;
        self.info = SearchInfo::default();
        self.status = format!("New game · {}", self.mode.title());
    }

    pub fn load_fen(&mut self, fen: &str) -> Result<(), String> {
        self.stop_thinking_quiet();
        self.analyzed = None;
        self.board = Board::from_fen(fen).map_err(|e| e.to_string())?;
        self.move_stack.clear();
        self.last_move = None;
        self.info = SearchInfo::default();
        self.status = format!("Loaded FEN · {}", self.mode.title());
        Ok(())
    }

    /// Load a browseable game, jump to the final ply, switch to Analyze.
    pub fn load_analyzed_game(&mut self, mut game: AnalyzedGame) -> Result<(), String> {
        self.stop_thinking_quiet();
        game.cursor = game.plies.len();
        self.sync_from_game(&game)?;
        let n = game.ply_count();
        self.analyzed = Some(game);
        self.mode = PlayMode::Analyze;
        self.info = SearchInfo::default();
        self.status = format!("Imported {n} plies · ←/→ step · Analyze");
        Ok(())
    }

    pub fn apply_move_list(&mut self, list: &str) -> Result<(), String> {
        for (i, tok) in list.split_whitespace().enumerate() {
            self.play_text(tok)
                .map_err(|e| format!("move {} ({tok}): {e}", i + 1))?;
        }
        self.status = format!("Applied moves · {}", self.mode.title());
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
        if self.analyzed.is_some() {
            return self.step_back();
        }
        if let Some(m) = self.move_stack.pop() {
            self.board.unmake(m);
            self.last_move = self.move_stack.last().copied();
            self.stop_thinking_quiet();
            self.status = "Undid last move".into();
            true
        } else {
            self.status = "Nothing to undo".into();
            false
        }
    }

    fn stop_thinking_quiet(&mut self) {
        if let Some(mut live) = self.live.take() {
            live.stop.store(true, Ordering::Relaxed);
            if let Some(handle) = live.handle.take() {
                let _ = handle.join();
            }
        }
        self.info.thinking = false;
        self.go_started = None;
        self.pending_limits = None;
    }

    /// Play a single player move: SAN (`e4`, `Nf3`, `O-O`) or UCI (`e2e4`).
    pub fn play_text(&mut self, text: &str) -> Result<(), String> {
        let mv = resolve_player_move(&self.board, text)?;
        self.play_move(mv)
    }

    pub fn play_move(&mut self, mv: Move) -> Result<(), String> {
        // Playing leaves browse mode; imported transcript is cleared.
        self.analyzed = None;
        // Legality already checked for player input; bot picks from legal_moves.
        self.board.make(mv);
        self.move_stack.push(mv);
        self.last_move = Some(mv);
        self.status = format!("Played {mv}");
        Ok(())
    }

    pub fn go(&mut self, limits: GoLimits) {
        if self.info.thinking {
            self.status = "Already thinking".into();
            return;
        }
        self.apply_on_finish = self.search_applies_move();
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

        let search_limits = Limits {
            depth: limits.depth.map(|d| d as i32),
            movetime: limits.movetime.or(Some(Duration::from_millis(400))),
            nodes: None,
            ..Default::default()
        };
        // If only depth is set, don't also force a short movetime.
        let search_limits = if limits.depth.is_some() && limits.movetime.is_none() {
            Limits {
                depth: limits.depth.map(|d| d as i32),
                movetime: None,
                nodes: None,
                ..Default::default()
            }
        } else {
            search_limits
        };

        let mut board = self.board.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let stop_t = Arc::clone(&stop);
        let result_t = Arc::clone(&result);

        let handle = std::thread::spawn(move || {
            let mut tt = TranspositionTable::new(16);
            let out = search::go(&mut board, search_limits, &mut tt, &stop_t, None);
            if let Ok(mut slot) = result_t.lock() {
                *slot = Some(out);
            }
        });

        self.live = Some(LiveSearch {
            stop,
            result,
            handle: Some(handle),
        });
    }

    pub fn stop(&mut self) {
        if self.info.thinking {
            if let Some(live) = &self.live {
                live.stop.store(true, Ordering::Relaxed);
            }
            self.finish_search(true);
        }
    }

    pub fn poll(&mut self) {
        if !self.info.thinking {
            return;
        }
        let Some(started) = self.go_started else {
            return;
        };
        self.info.time = started.elapsed();

        let ready = self
            .live
            .as_ref()
            .and_then(|l| l.result.lock().ok())
            .map(|g| g.is_some())
            .unwrap_or(false);

        if ready {
            self.finish_search(false);
        }
    }

    fn finish_search(&mut self, stopped: bool) {
        if let Some(mut live) = self.live.take() {
            live.stop.store(true, Ordering::Relaxed);
            if let Some(handle) = live.handle.take() {
                let _ = handle.join();
            }
            let result = live
                .result
                .lock()
                .ok()
                .and_then(|mut g| g.take());
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
            self.status = "Search failed".into();
            return;
        };

        self.info.depth = result.depth.max(0) as u32;
        self.info.score_cp = result.score;
        self.info.nodes = result.nodes;
        self.info.time = result.time;
        self.info.pv = result
            .pv
            .iter()
            .map(|m| m.to_string())
            .collect::<Vec<_>>()
            .join(" ");

        if result.best_move.is_none() {
            self.status = "No legal moves".into();
            return;
        }

        let mv = result.best_move;
        self.info.bestmove = Some(mv.to_string());
        if apply {
            match self.play_move(mv) {
                Ok(()) => {
                    self.status = if stopped {
                        format!("Stopped → played {mv}")
                    } else {
                        format!("Bot plays {mv}")
                    };
                }
                Err(e) => self.status = format!("Engine move failed: {e}"),
            }
        } else {
            self.status = format!("Best move: {mv} (board unchanged)");
        }
    }
}

impl Default for EngineSession {
    fn default() -> Self {
        Self::new()
    }
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
    fn accepts_short_pawn_move() {
        let mut s = EngineSession::new();
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
    fn browse_imported_game_steps() {
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
        s.load_analyzed_game(game).unwrap();
        assert_eq!(s.analyzed().unwrap().cursor, 4);
        assert_eq!(s.mode(), PlayMode::Analyze);
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
}
