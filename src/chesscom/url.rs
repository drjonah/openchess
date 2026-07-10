//! Parse chess.com game links and normalize usernames.

use crate::chesscom::ChessComError;

/// Live (real-time) or daily (correspondence) game.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameKind {
    Live,
    Daily,
}

impl GameKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Daily => "daily",
        }
    }
}

/// Parsed `https://www.chess.com/game/{live|daily}/{id}` URL.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedGameUrl {
    pub kind: GameKind,
    pub id: String,
}

/// Parse a chess.com game URL into kind + numeric id.
pub fn parse_game_url(raw: &str) -> Result<ParsedGameUrl, ChessComError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ChessComError::InvalidUrl("empty".into()));
    }

    let without_query = s.split(['?', '#']).next().unwrap_or(s);
    let without_slash = without_query.trim_end_matches('/');

    let path = if let Some(rest) = strip_host(without_slash) {
        rest
    } else if without_slash.starts_with("game/") {
        without_slash
    } else {
        return Err(ChessComError::InvalidUrl(format!(
            "expected chess.com game URL, got {raw:?}"
        )));
    };

    let mut parts = path.split('/').filter(|p| !p.is_empty());
    let first = parts.next();
    if first != Some("game") {
        return Err(ChessComError::InvalidUrl(format!(
            "path must start with /game/, got {raw:?}"
        )));
    }

    let kind = match parts.next() {
        Some("live") => GameKind::Live,
        Some("daily") => GameKind::Daily,
        Some(other) => {
            return Err(ChessComError::InvalidUrl(format!(
                "game type must be live or daily, got {other:?}"
            )));
        }
        None => {
            return Err(ChessComError::InvalidUrl(
                "missing game type (live|daily)".into(),
            ));
        }
    };

    let id = parts
        .next()
        .ok_or_else(|| ChessComError::InvalidUrl("missing game id".into()))?;
    if !id.chars().all(|c| c.is_ascii_digit()) || id.is_empty() {
        return Err(ChessComError::InvalidUrl(format!(
            "game id must be numeric, got {id:?}"
        )));
    }
    if parts.next().is_some() {
        return Err(ChessComError::InvalidUrl(format!(
            "unexpected path after game id: {raw:?}"
        )));
    }

    Ok(ParsedGameUrl {
        kind,
        id: id.to_string(),
    })
}

fn strip_host(s: &str) -> Option<&str> {
    const PREFIXES: &[&str] = &[
        "https://www.chess.com/",
        "http://www.chess.com/",
        "https://chess.com/",
        "http://chess.com/",
        "www.chess.com/",
        "chess.com/",
    ];
    for p in PREFIXES {
        if let Some(rest) = s.strip_prefix(p) {
            return Some(rest);
        }
    }
    None
}

/// Normalize a chess.com username for API paths (lowercase, no spaces).
pub fn normalize_username(raw: &str) -> Result<String, ChessComError> {
    let s = raw.trim();
    if s.is_empty() {
        return Err(ChessComError::InvalidUsername("empty".into()));
    }
    if s.contains('/') || s.contains('?') || s.contains('#') {
        return Err(ChessComError::InvalidUsername(format!(
            "looks like a URL path, not a username: {raw:?}"
        )));
    }
    if !s
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        return Err(ChessComError::InvalidUsername(format!(
            "invalid characters in {raw:?}"
        )));
    }
    Ok(s.to_ascii_lowercase())
}

/// True if the input looks like a chess.com game URL rather than a username.
pub fn looks_like_game_url(raw: &str) -> bool {
    let s = raw.trim();
    s.contains("chess.com") || s.starts_with("game/") || s.contains("/game/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_live_full_url() {
        let p = parse_game_url("https://www.chess.com/game/live/169227053782").unwrap();
        assert_eq!(p.kind, GameKind::Live);
        assert_eq!(p.id, "169227053782");
    }

    #[test]
    fn parse_daily_trailing_slash_and_query() {
        let p = parse_game_url("https://www.chess.com/game/daily/12345/?foo=1").unwrap();
        assert_eq!(p.kind, GameKind::Daily);
        assert_eq!(p.id, "12345");
    }

    #[test]
    fn parse_bare_path() {
        let p = parse_game_url("game/live/99").unwrap();
        assert_eq!(p.kind, GameKind::Live);
        assert_eq!(p.id, "99");
    }

    #[test]
    fn reject_non_numeric_id() {
        assert!(parse_game_url("https://www.chess.com/game/live/abc").is_err());
    }

    #[test]
    fn reject_empty() {
        assert!(parse_game_url("").is_err());
        assert!(normalize_username("").is_err());
    }

    #[test]
    fn normalize_username_lowercases() {
        assert_eq!(normalize_username("HiKaRu").unwrap(), "hikaru");
    }
}
