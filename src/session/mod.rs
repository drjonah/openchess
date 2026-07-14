//! Shared engine search-session primitives used by the TUI and arena (P11-09).
//!
//! Owns background `search::go` spawning / live-info polling / stop-join.
//! Does not own play modes, move input, or UI state.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crate::board::Board;
use crate::search::{self, Limits, SearchResult};
use crate::transposition::TranspositionTable;
use crate::types::Color;

/// Default movetime applied when a [`GoLimits`] specifies neither depth nor
/// movetime, so an otherwise-empty request still terminates promptly.
const DEFAULT_GO_MOVETIME: Duration = Duration::from_millis(400);

/// Depth / movetime request shared by the TUI, config, and arena front-ends.
///
/// This is the front-end vocabulary for "how hard to think"; it is lowered to
/// the engine's [`Limits`] via [`GoLimits::to_search_limits`].
#[derive(Clone, Copy, Debug, Default)]
pub struct GoLimits {
    pub depth: Option<u32>,
    pub movetime: Option<Duration>,
}

impl GoLimits {
    /// Lower a front-end request to engine [`Limits`].
    ///
    /// A depth-only request stays depth-only (no implicit movetime cap); any
    /// other request keeps its movetime, falling back to [`DEFAULT_GO_MOVETIME`]
    /// when none was given.
    pub fn to_search_limits(self) -> Limits {
        let movetime = if self.depth.is_some() && self.movetime.is_none() {
            None
        } else {
            self.movetime.or(Some(DEFAULT_GO_MOVETIME))
        };
        Limits {
            depth: self.depth.map(|d| d as i32),
            movetime,
            nodes: None,
            ..Default::default()
        }
    }
}

/// How a play session is driven (shared by the TUI and user config).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlayMode {
    PlayerVsPlayer,
    PlayerVsBot { human: Color },
    BotVsBot,
    Analyze,
}

impl PlayMode {
    pub const ALL: [PlayMode; 5] = [
        PlayMode::PlayerVsPlayer,
        PlayMode::PlayerVsBot {
            human: Color::White,
        },
        PlayMode::PlayerVsBot {
            human: Color::Black,
        },
        PlayMode::BotVsBot,
        PlayMode::Analyze,
    ];

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
            PlayMode::Analyze => "Analyze",
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
            PlayMode::BotVsBot => "Engine plays both sides (per-color strength)",
            PlayMode::Analyze => "Start empty or import FEN / PGN / game",
        }
    }
}

/// Compact search stats published to the UI / inspector.
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

impl SearchInfo {
    /// Overlay the latest per-iteration snapshot from a running worker.
    ///
    /// Copies only the streamed search stats; the lifecycle fields (`thinking`,
    /// `bestmove`) are owned by the caller and left untouched.
    pub fn apply_live(&mut self, live: &LiveInfo) {
        self.depth = live.depth;
        self.score_cp = live.score_cp;
        self.nodes = live.nodes;
        self.time = live.time;
        self.pv = live.pv.clone();
    }
}

/// Per-iteration search stats shared from the worker thread.
#[derive(Clone, Debug, Default)]
pub struct LiveInfo {
    pub depth: u32,
    pub score_cp: i32,
    pub nodes: u64,
    pub time: Duration,
    pub pv: String,
}

/// Background search job started by [`LiveSearch::spawn`].
///
/// Internals are private on purpose: drive the job through
/// [`request_stop`](Self::request_stop), [`shutdown`](Self::shutdown),
/// [`is_ready`](Self::is_ready), [`snapshot_live`](Self::snapshot_live), and
/// [`take_result`](Self::take_result) rather than poking the shared state.
pub struct LiveSearch {
    stop: Arc<AtomicBool>,
    result: Arc<Mutex<Option<SearchResult>>>,
    live_info: Arc<Mutex<LiveInfo>>,
    handle: Option<JoinHandle<()>>,
}

impl LiveSearch {
    /// Spawn a single-threaded search with a private TT of `hash_mb` megabytes.
    pub fn spawn(board: Board, limits: Limits, hash_mb: usize) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let result = Arc::new(Mutex::new(None));
        let live_info = Arc::new(Mutex::new(LiveInfo::default()));
        let stop_t = Arc::clone(&stop);
        let result_t = Arc::clone(&result);
        let live_info_t = Arc::clone(&live_info);

        let handle = std::thread::spawn(move || {
            let mut board = board;
            let tt = TranspositionTable::new(hash_mb);
            let mut on_info =
                |depth: i32, score: i32, nodes: u64, time: Duration, pv: &str, _hashfull: u32| {
                    if let Ok(mut snap) = live_info_t.lock() {
                        snap.depth = depth.max(0) as u32;
                        snap.score_cp = score;
                        snap.nodes = nodes;
                        snap.time = time;
                        snap.pv = pv.to_string();
                    }
                };
            let out = search::go(&mut board, limits, &tt, &stop_t, Some(&mut on_info));
            if let Ok(mut slot) = result_t.lock() {
                *slot = Some(out);
            }
        });

        Self {
            stop,
            result,
            live_info,
            handle: Some(handle),
        }
    }

    /// Request a soft stop (the worker still needs to be joined via [`Self::shutdown`]).
    pub fn request_stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    /// Signal stop and join the worker thread.
    pub fn shutdown(&mut self) {
        self.request_stop();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }

    /// True when the worker has published a final [`SearchResult`].
    pub fn is_ready(&self) -> bool {
        self.result
            .lock()
            .ok()
            .map(|g| g.is_some())
            .unwrap_or(false)
    }

    /// Clone the latest live-info snapshot (empty if the lock is poisoned).
    pub fn snapshot_live(&self) -> LiveInfo {
        self.live_info
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Join the worker and take the final result (if any).
    pub fn take_result(&mut self) -> Option<SearchResult> {
        self.shutdown();
        self.result.lock().ok().and_then(|mut g| g.take())
    }
}

/// Convert a side-to-move-relative search score to White-relative centipawns.
pub fn stm_score_to_white(score: i32, stm: Color) -> i32 {
    match stm {
        Color::White => score,
        Color::Black => -score,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::score::VALUE_MATE;

    #[test]
    fn stm_score_to_white_preserves_white_stm() {
        assert_eq!(stm_score_to_white(42, Color::White), 42);
        assert_eq!(stm_score_to_white(-15, Color::White), -15);
    }

    #[test]
    fn stm_score_to_white_negates_black_stm() {
        assert_eq!(stm_score_to_white(50, Color::Black), -50);
        assert_eq!(stm_score_to_white(-30, Color::Black), 30);
    }

    #[test]
    fn stm_score_to_white_flips_mate_for_black() {
        let black_mates = VALUE_MATE - 3;
        assert_eq!(stm_score_to_white(black_mates, Color::Black), -black_mates);
        assert!(stm_score_to_white(black_mates, Color::Black) <= -VALUE_MATE + 1000);
    }
}
