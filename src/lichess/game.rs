//! Per-game stream types and the single-game handler (P9-03).
//!
//! The handler follows `research/LICHESS.md §7`: open the game NDJSON stream,
//! rebuild the position from `initialFen` + the space-separated `moves` list on
//! every `gameState`, and — when it is our turn — run [`crate::search`] with a
//! [`crate::time::TimeBudget`] derived from the Lichess clock, then POST the
//! best move. Position replay is stateless (rebuilt from scratch each update),
//! which is reconnect-safe.
//!
//! The decision logic lives in [`GameDriver`] and is fully testable offline
//! without a token; [`play_game`] wires it to a live [`Client`] stream.

use super::client::{Client, NdjsonItem};
use super::LichessError;
use crate::board::Board;
use crate::search::{self, Limits};
use crate::transposition::TranspositionTable;
use crate::types::{Color, Move};
use serde::Deserialize;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Options controlling how the bot searches during a game.
#[derive(Clone, Copy, Debug)]
pub struct PlayOptions {
    /// Network/GUI latency reserve subtracted from the hard budget.
    pub move_overhead: Duration,
    /// Transposition-table size for the game.
    pub hash_mb: u32,
    /// Fixed think time overriding the clock (tests / simple fixed-time bots).
    pub fixed_movetime: Option<Duration>,
}

impl Default for PlayOptions {
    fn default() -> Self {
        Self {
            move_overhead: crate::time::DEFAULT_MOVE_OVERHEAD,
            hash_mb: 16,
            fixed_movetime: None,
        }
    }
}

/// One line of the per-game NDJSON stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GameStreamEvent {
    /// First line: static metadata + embedded initial [`GameState`].
    GameFull(GameFull),
    /// Incremental update: new move list + clocks + status.
    GameState(GameState),
    ChatLine(ChatLine),
    OpponentGone(OpponentGone),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameFull {
    pub id: String,
    /// `"startpos"` or a FEN string.
    #[serde(default = "startpos_literal")]
    pub initial_fen: String,
    pub white: Player,
    pub black: Player,
    pub state: GameState,
    #[serde(default)]
    pub speed: Option<String>,
}

fn startpos_literal() -> String {
    "startpos".into()
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    /// Player account id (absent for the Lichess AI).
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameState {
    /// Space-separated UCI move list from the start position.
    #[serde(default)]
    pub moves: String,
    /// Remaining time in milliseconds.
    #[serde(default)]
    pub wtime: u64,
    #[serde(default)]
    pub btime: u64,
    /// Increment per move in milliseconds.
    #[serde(default)]
    pub winc: u64,
    #[serde(default)]
    pub binc: u64,
    /// `started`, `mate`, `resign`, `stalemate`, `draw`, `aborted`, …
    #[serde(default = "started_literal")]
    pub status: String,
    #[serde(default)]
    pub winner: Option<String>,
}

fn started_literal() -> String {
    "started".into()
}

impl GameState {
    /// True once the game has reached a terminal status.
    pub fn is_over(&self) -> bool {
        !matches!(self.status.as_str(), "started" | "created")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChatLine {
    #[serde(default)]
    pub room: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OpponentGone {
    #[serde(default)]
    pub gone: bool,
    #[serde(default)]
    pub claim_win_in_seconds: Option<u32>,
}

/// What the driver wants the network layer to do after an event.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GameAction {
    /// Nothing to do (not our turn, or informational line).
    Wait,
    /// Play this move (already a legal UCI move for the current position).
    PlayMove(Move),
    /// The game reached a terminal status; tear down the handler.
    Finished,
}

/// Stateless-per-update decision engine for one game (offline-testable).
pub struct GameDriver {
    my_id: String,
    my_color: Option<Color>,
    initial_fen: String,
    options: PlayOptions,
    tt: TranspositionTable,
}

impl GameDriver {
    pub fn new(my_id: impl Into<String>, options: PlayOptions) -> Self {
        Self {
            my_id: my_id.into(),
            my_color: None,
            initial_fen: startpos_literal(),
            options,
            tt: TranspositionTable::new(options.hash_mb.max(1) as usize),
        }
    }

    /// Color the bot plays, once known from the `gameFull` line.
    pub fn my_color(&self) -> Option<Color> {
        self.my_color
    }

    /// Handle the `gameFull` line: resolve our color, capture the start FEN,
    /// then act on the embedded state.
    pub fn on_game_full(&mut self, full: &GameFull) -> Result<GameAction, LichessError> {
        self.initial_fen = full.initial_fen.clone();
        self.my_color = resolve_color(&self.my_id, full);
        self.on_game_state(&full.state)
    }

    /// Handle a `gameState` line: if the game is live and it is our turn, pick a
    /// move; otherwise wait (or report the game finished).
    pub fn on_game_state(&mut self, state: &GameState) -> Result<GameAction, LichessError> {
        if state.is_over() {
            return Ok(GameAction::Finished);
        }
        let board = self.rebuild_board(&state.moves)?;
        let Some(my_color) = self.my_color else {
            return Ok(GameAction::Wait);
        };
        if board.side_to_move() != my_color {
            return Ok(GameAction::Wait);
        }
        match self.pick_move(&board, state) {
            Some(mv) => Ok(GameAction::PlayMove(mv)),
            None => Ok(GameAction::Wait),
        }
    }

    /// Rebuild the position from the start FEN by replaying every UCI move.
    fn rebuild_board(&self, moves: &str) -> Result<Board, LichessError> {
        let mut board = if self.initial_fen.is_empty() || self.initial_fen == "startpos" {
            Board::startpos()
        } else {
            Board::from_fen(&self.initial_fen)
                .map_err(|e| LichessError::Http(format!("bad initialFen: {e}")))?
        };
        for tok in moves.split_whitespace() {
            let mv = board
                .parse_uci_move(tok)
                .map_err(|e| LichessError::Http(format!("bad move '{tok}': {e}")))?;
            board.make(mv);
        }
        Ok(board)
    }

    /// Run a search for the side to move and return the best legal move.
    fn pick_move(&self, board: &Board, state: &GameState) -> Option<Move> {
        let limits = self.limits_for(state);
        let stop = AtomicBool::new(false);
        let mut search_board = board.clone();
        let result = search::go(&mut search_board, limits, &self.tt, &stop, None);
        (!result.best_move.is_none()).then_some(result.best_move)
    }

    /// Build search [`Limits`] from the Lichess clock (or the fixed override).
    ///
    /// The side to move is resolved inside [`crate::time::TimeBudget`] from the
    /// board, so both sides' clocks are always supplied here.
    fn limits_for(&self, state: &GameState) -> Limits {
        let mut limits = Limits {
            move_overhead: self.options.move_overhead,
            ..Default::default()
        };
        if let Some(mt) = self.options.fixed_movetime {
            limits.movetime = Some(mt);
        } else {
            limits.wtime = Some(Duration::from_millis(state.wtime));
            limits.btime = Some(Duration::from_millis(state.btime));
            limits.winc = Some(Duration::from_millis(state.winc));
            limits.binc = Some(Duration::from_millis(state.binc));
        }
        limits
    }
}

/// Resolve our color from the `gameFull` player ids (case-insensitive).
fn resolve_color(my_id: &str, full: &GameFull) -> Option<Color> {
    let me = my_id.to_ascii_lowercase();
    let matches = |p: &Player| {
        p.id
            .as_deref()
            .map(|id| id.to_ascii_lowercase() == me)
            .unwrap_or(false)
    };
    if matches(&full.white) {
        Some(Color::White)
    } else if matches(&full.black) {
        Some(Color::Black)
    } else {
        None
    }
}

/// Play one game to completion over a live stream.
///
/// Reads the per-game NDJSON stream, drives [`GameDriver`], and POSTs moves. On
/// a move-POST failure it resigns rather than risk flagging (per §11.4).
pub fn play_game(
    client: &Client,
    game_id: &str,
    my_id: &str,
    options: PlayOptions,
) -> Result<(), LichessError> {
    let mut stream = client
        .open_ndjson_stream::<GameStreamEvent>(&format!("/api/bot/game/stream/{game_id}"))?;
    let mut driver = GameDriver::new(my_id, options);

    loop {
        let Some(item) = stream.read_item()? else {
            // Stream closed; caller decides whether to reconnect.
            return Ok(());
        };
        let action = match item {
            NdjsonItem::Keepalive => continue,
            NdjsonItem::Event(GameStreamEvent::GameFull(full)) => driver.on_game_full(&full)?,
            NdjsonItem::Event(GameStreamEvent::GameState(state)) => driver.on_game_state(&state)?,
            NdjsonItem::Event(GameStreamEvent::ChatLine(_))
            | NdjsonItem::Event(GameStreamEvent::OpponentGone(_)) => GameAction::Wait,
        };
        match action {
            GameAction::Wait => {}
            GameAction::Finished => return Ok(()),
            GameAction::PlayMove(mv) => {
                let uci = mv.to_string();
                if let Err(e) = client.play_move(game_id, &uci) {
                    eprintln!("lichess game {game_id}: move {uci} failed: {e}; resigning");
                    let _ = client.resign(game_id);
                    return Err(e);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init() {
        crate::lookup::initialize();
    }

    fn fast_options() -> PlayOptions {
        PlayOptions {
            fixed_movetime: Some(Duration::from_millis(30)),
            hash_mb: 1,
            ..Default::default()
        }
    }

    fn full(white_id: &str, black_id: &str, moves: &str) -> GameFull {
        GameFull {
            id: "testgame".into(),
            initial_fen: "startpos".into(),
            white: Player {
                id: Some(white_id.into()),
                name: Some(white_id.into()),
            },
            black: Player {
                id: Some(black_id.into()),
                name: Some(black_id.into()),
            },
            state: GameState {
                moves: moves.into(),
                status: "started".into(),
                ..Default::default()
            },
            speed: Some("blitz".into()),
        }
    }

    #[test]
    fn resolves_color_case_insensitively() {
        let g = full("OpenChessBot", "opponent", "");
        assert_eq!(resolve_color("openchessbot", &g), Some(Color::White));
        assert_eq!(resolve_color("OPPONENT", &g), Some(Color::Black));
        assert_eq!(resolve_color("stranger", &g), None);
    }

    #[test]
    fn plays_a_legal_move_on_our_turn() {
        init();
        let mut driver = GameDriver::new("openchessbot", fast_options());
        let action = driver.on_game_full(&full("openchessbot", "opp", "")).unwrap();
        match action {
            GameAction::PlayMove(mv) => {
                let legal = Board::startpos().legal_moves();
                assert!(legal.contains(&mv), "bot move {mv} not legal at startpos");
            }
            other => panic!("expected a move as White at startpos, got {other:?}"),
        }
    }

    #[test]
    fn waits_when_not_our_turn() {
        init();
        // We are Black; White has not moved yet → wait.
        let mut driver = GameDriver::new("openchessbot", fast_options());
        let action = driver.on_game_full(&full("opp", "openchessbot", "")).unwrap();
        assert_eq!(action, GameAction::Wait);
    }

    #[test]
    fn replays_moves_and_answers_as_black() {
        init();
        let mut driver = GameDriver::new("openchessbot", fast_options());
        // White opened 1.e4; now Black (us) to move.
        let action = driver
            .on_game_full(&full("opp", "openchessbot", "e2e4"))
            .unwrap();
        match action {
            GameAction::PlayMove(mv) => {
                let mut board = Board::startpos();
                board.make(board.parse_uci_move("e2e4").unwrap());
                assert!(board.legal_moves().contains(&mv));
            }
            other => panic!("expected a Black reply, got {other:?}"),
        }
    }

    #[test]
    fn terminal_status_finishes() {
        init();
        let mut driver = GameDriver::new("openchessbot", fast_options());
        driver.on_game_full(&full("openchessbot", "opp", "")).unwrap();
        let state = GameState {
            moves: "e2e4 e7e5".into(),
            status: "mate".into(),
            ..Default::default()
        };
        assert_eq!(driver.on_game_state(&state).unwrap(), GameAction::Finished);
    }

    #[test]
    fn rebuilds_from_custom_initial_fen() {
        init();
        let mut driver = GameDriver::new("openchessbot", fast_options());
        let mut g = full("openchessbot", "opp", "");
        // A legal position with White to move.
        g.initial_fen = "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 0 1".into();
        // White id but Black to move in this FEN → we (White) wait.
        let action = driver.on_game_full(&g).unwrap();
        assert_eq!(action, GameAction::Wait);
    }

    #[test]
    fn clock_limits_map_milliseconds() {
        let driver = GameDriver::new("x", PlayOptions::default());
        let state = GameState {
            wtime: 120_000,
            btime: 60_000,
            winc: 2_000,
            binc: 1_000,
            ..Default::default()
        };
        let limits = driver.limits_for(&state);
        assert_eq!(limits.wtime, Some(Duration::from_millis(120_000)));
        assert_eq!(limits.btime, Some(Duration::from_millis(60_000)));
        assert_eq!(limits.winc, Some(Duration::from_millis(2_000)));
        assert_eq!(limits.binc, Some(Duration::from_millis(1_000)));
        assert!(limits.movetime.is_none());
    }

    #[test]
    fn play_game_drives_a_mock_stream_to_completion() {
        init();
        // gameFull (our turn as White) then a terminal gameState.
        let lines = concat!(
            "{\"type\":\"gameFull\",\"id\":\"g1\",\"initialFen\":\"startpos\",",
            "\"white\":{\"id\":\"me\",\"name\":\"me\"},\"black\":{\"id\":\"opp\",\"name\":\"opp\"},",
            "\"state\":{\"type\":\"gameState\",\"moves\":\"\",\"wtime\":60000,\"btime\":60000,\"winc\":0,\"binc\":0,\"status\":\"started\"}}\n",
            "{\"type\":\"gameState\",\"moves\":\"e2e4 e7e5\",\"wtime\":59000,\"btime\":60000,\"winc\":0,\"binc\":0,\"status\":\"resign\",\"winner\":\"white\"}\n",
        );
        let mut stream: super::super::client::NdjsonStream<GameStreamEvent> =
            super::super::client::NdjsonStream::from_reader(std::io::Cursor::new(lines));

        let mut driver = GameDriver::new("me", fast_options());
        let mut moves_played = 0;
        loop {
            match stream.read_item().unwrap() {
                None => break,
                Some(NdjsonItem::Keepalive) => {}
                Some(NdjsonItem::Event(GameStreamEvent::GameFull(full))) => {
                    if let GameAction::PlayMove(mv) = driver.on_game_full(&full).unwrap() {
                        assert!(Board::startpos().legal_moves().contains(&mv));
                        moves_played += 1;
                    }
                }
                Some(NdjsonItem::Event(GameStreamEvent::GameState(state)))
                    if driver.on_game_state(&state).unwrap() == GameAction::Finished =>
                {
                    break;
                }
                _ => {}
            }
        }
        assert_eq!(moves_played, 1, "should have moved once as White at startpos");
    }

    #[test]
    fn parses_game_full_fixture() {
        let text = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/lichess/game_full.json"),
        )
        .expect("read fixture");
        let event: GameStreamEvent = serde_json::from_str(&text).unwrap();
        let GameStreamEvent::GameFull(full) = event else {
            panic!("expected gameFull");
        };
        assert_eq!(full.white.id.as_deref(), Some("openchessbot"));
        assert_eq!(full.initial_fen, "startpos");
        assert_eq!(full.state.status, "started");
        assert_eq!(full.state.wtime, 300_000);
    }

    #[test]
    fn parses_game_state_fixture() {
        let text = std::fs::read_to_string(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/lichess/game_state.json"),
        )
        .expect("read fixture");
        let event: GameStreamEvent = serde_json::from_str(&text).unwrap();
        let GameStreamEvent::GameState(state) = event else {
            panic!("expected gameState");
        };
        assert_eq!(state.moves, "e2e4 e7e5 g1f3");
        assert!(!state.is_over());
        // Sanity: the move list rebuilds into a legal position (Black to move).
        init();
        let driver = GameDriver::new("x", PlayOptions::default());
        let board = driver.rebuild_board(&state.moves).unwrap();
        assert_eq!(board.side_to_move(), Color::Black);
    }
}
