//! Lichess Bot API client (feature = `"lichess"`).
//!
//! Headless daemon: NDJSON event/game streams, challenge filter, and in-process
//! search-driven play (`openchess lichess …`).

pub mod challenge;
pub mod client;
pub mod cli;
pub mod config;
pub mod events;
pub mod game;
pub mod pgn;

pub use client::Client;
pub use config::LichessConfig;
pub use events::StreamEvent;

/// Errors from Lichess HTTP or JSON parsing.
#[derive(Debug)]
pub enum LichessError {
    MissingToken(String),
    Http(String),
    Json(String),
    RateLimited,
}

impl std::fmt::Display for LichessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingToken(var) => write!(
                f,
                "missing {var} environment variable (see .env.example)"
            ),
            Self::Http(m) => write!(f, "HTTP error: {m}"),
            Self::Json(m) => write!(f, "JSON error: {m}"),
            Self::RateLimited => write!(f, "rate limited (HTTP 429)"),
        }
    }
}

impl std::error::Error for LichessError {}
