//! PGN export + game log (P9-06) and reconnect/backoff helpers (P9-07).

use super::client::Client;
use super::LichessError;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Base backoff before the first reconnect attempt.
const BACKOFF_BASE_SECS: u64 = 4;
/// Cap on the exponential reconnect backoff.
const BACKOFF_MAX_SECS: u64 = 32;
/// Sleep after an HTTP 429 (rate limit).
pub const RATE_LIMIT_SLEEP: Duration = Duration::from_secs(60);

/// Exponential reconnect delay for `attempt` (0-based): 4s, 8s, 16s, 32s, capped.
pub fn backoff_delay(attempt: u32) -> Duration {
    let secs = BACKOFF_BASE_SECS
        .saturating_mul(1u64 << attempt.min(6))
        .min(BACKOFF_MAX_SECS);
    Duration::from_secs(secs)
}

/// Directory PGNs are written to (`$XDG_CACHE_HOME/openchess/lichess` or
/// `~/.cache/openchess/lichess`).
pub fn cache_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("openchess").join("lichess");
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".cache").join("openchess").join("lichess")
}

/// File path for a game's PGN under `dir`.
pub fn pgn_path(dir: &Path, game_id: &str) -> PathBuf {
    dir.join(format!("{game_id}.pgn"))
}

/// Fetch a finished game's PGN (`GET /game/export/{id}`).
pub fn fetch_pgn(client: &Client, game_id: &str) -> Result<String, LichessError> {
    client.get_text(&format!("/game/export/{game_id}"))
}

/// Fetch and save a game's PGN under [`cache_dir`], returning the written path.
pub fn export_game(client: &Client, game_id: &str) -> Result<PathBuf, LichessError> {
    let pgn = fetch_pgn(client, game_id)?;
    let dir = cache_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| LichessError::Http(format!("create {}: {e}", dir.display())))?;
    let path = pgn_path(&dir, game_id);
    std::fs::write(&path, pgn).map_err(|e| LichessError::Http(format!("write pgn: {e}")))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_grows_then_caps() {
        assert_eq!(backoff_delay(0), Duration::from_secs(4));
        assert_eq!(backoff_delay(1), Duration::from_secs(8));
        assert_eq!(backoff_delay(2), Duration::from_secs(16));
        assert_eq!(backoff_delay(3), Duration::from_secs(32));
        // Capped thereafter (no overflow at large attempt counts).
        assert_eq!(backoff_delay(4), Duration::from_secs(32));
        assert_eq!(backoff_delay(100), Duration::from_secs(32));
    }

    #[test]
    fn pgn_path_uses_game_id() {
        let p = pgn_path(Path::new("/tmp/x"), "abcd1234");
        assert_eq!(p, PathBuf::from("/tmp/x/abcd1234.pgn"));
    }

    #[test]
    fn cache_dir_ends_with_openchess_lichess() {
        let dir = cache_dir();
        assert!(
            dir.ends_with("openchess/lichess"),
            "unexpected cache dir: {}",
            dir.display()
        );
    }
}
