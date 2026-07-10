//! Session cache for chess.com game lists (latest archive month).

use crate::chesscom::fetch::GameSummary;
use crate::chesscom::ChessComError;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CACHE_DIR_NAME: &str = "openchess";
const CACHE_SUBDIR: &str = "chesscom";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GamesCache {
    pub username: String,
    /// Unix seconds when this cache was written.
    pub fetched_at: u64,
    /// Newest-first games from the latest archive month.
    pub games: Vec<GameSummary>,
}

impl GamesCache {
    pub fn new(username: String, games: Vec<GameSummary>) -> Self {
        Self {
            username,
            fetched_at: now_unix_secs(),
            games,
        }
    }

    /// Relative age of this cache for status lines (`2h ago`, `just now`, …).
    pub fn age_label(&self, now_secs: u64) -> String {
        format_cache_age(self.fetched_at, now_secs)
    }
}

/// Relative age label for a cache `fetched_at` timestamp.
pub fn format_cache_age(fetched_at: u64, now_secs: u64) -> String {
    if fetched_at == 0 {
        return "unknown".into();
    }
    let ago = now_secs.saturating_sub(fetched_at);
    if ago < 60 {
        "just now".into()
    } else if ago < 3600 {
        format!("{}m ago", ago / 60)
    } else if ago < 86400 {
        format!("{}h ago", ago / 3600)
    } else {
        format!("{}d ago", ago / 86400)
    }
}

/// `~/.cache/openchess/chesscom/{username}.json` (or `$XDG_CACHE_HOME/...`).
pub fn cache_path(username: &str) -> PathBuf {
    cache_dir().join(format!("{username}.json"))
}

pub fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join(CACHE_DIR_NAME).join(CACHE_SUBDIR);
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".cache")
        .join(CACHE_DIR_NAME)
        .join(CACHE_SUBDIR)
}

/// Write the full downloaded game list for a username.
pub fn save_games(username: &str, games: &[GameSummary]) -> Result<PathBuf, ChessComError> {
    let dir = cache_dir();
    fs::create_dir_all(&dir).map_err(|e| {
        ChessComError::Http(format!("cache mkdir {}: {e}", dir.display()))
    })?;
    let path = cache_path(username);
    let payload = GamesCache::new(username.to_string(), games.to_vec());
    let json = serde_json::to_string_pretty(&payload)
        .map_err(|e| ChessComError::Json(format!("cache serialize: {e}")))?;
    fs::write(&path, json)
        .map_err(|e| ChessComError::Http(format!("cache write {}: {e}", path.display())))?;
    Ok(path)
}

/// Load a previously cached game list, if present.
pub fn load_games(username: &str) -> Result<Option<GamesCache>, ChessComError> {
    let path = cache_path(username);
    if !path.is_file() {
        return Ok(None);
    }
    let raw = fs::read_to_string(&path)
        .map_err(|e| ChessComError::Http(format!("cache read {}: {e}", path.display())))?;
    let cache: GamesCache = serde_json::from_str(&raw)
        .map_err(|e| ChessComError::Json(format!("cache parse {}: {e}", path.display())))?;
    Ok(Some(cache))
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_path_uses_username() {
        let p = cache_path("hikaru");
        assert!(p.ends_with("hikaru.json"));
        assert!(p.to_string_lossy().contains("chesscom"));
    }

    #[test]
    fn age_label_formats() {
        assert_eq!(format_cache_age(100, 100), "just now");
        assert_eq!(format_cache_age(100, 100 + 120), "2m ago");
        assert_eq!(format_cache_age(100, 100 + 7200), "2h ago");
    }
}
