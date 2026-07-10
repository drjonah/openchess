//! Chess.com Published-Data API client (feature = `"chesscom"`).
//!
//! Fetch game PGNs by URL or username. The TUI import flow can browse a
//! player's recent games and load a selected PGN into Analyze mode.
//! Downloaded lists are written to `~/.cache/openchess/chesscom/{user}.json`.

mod cache;
pub mod cli;
mod client;
mod fetch;
mod url;

pub use cache::{cache_path, format_cache_age, load_games, save_games, GamesCache};
pub use fetch::{
    fetch_latest_pgn, fetch_pgn_by_index, fetch_pgn_from_url, list_recent_games, GameOutcome,
    GameSummary,
};
pub use url::{looks_like_game_url, normalize_username, parse_game_url, GameKind, ParsedGameUrl};

/// Errors from chess.com URL parsing or HTTP fetch.
#[derive(Debug)]
pub enum ChessComError {
    InvalidUrl(String),
    InvalidUsername(String),
    Http(String),
    Json(String),
    NotFound(String),
}

impl std::fmt::Display for ChessComError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidUrl(m) => write!(f, "invalid game URL: {m}"),
            Self::InvalidUsername(m) => write!(f, "invalid username: {m}"),
            Self::Http(m) => write!(f, "HTTP error: {m}"),
            Self::Json(m) => write!(f, "JSON error: {m}"),
            Self::NotFound(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for ChessComError {}
