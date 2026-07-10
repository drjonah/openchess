//! Import FEN, move lists, PGN, and (optionally) chess.com games.

use super::game::{AnalyzedGame, GameHeaders};
use super::session::EngineSession;
use std::fs;
use std::path::Path;

const START_FEN: &str = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";

/// Result of a successful import attempt.
pub enum ImportResult {
    /// Board / analyzed game already loaded into the session.
    Loaded,
    /// Username resolved; caller should open the game picker.
    #[cfg(feature = "chesscom")]
    BrowseGames {
        username: String,
        games: Vec<crate::chesscom::GameSummary>,
        from_cache: bool,
        fetched_at: u64,
    },
}

pub fn import_into(session: &mut EngineSession, raw: &str) -> Result<ImportResult, String> {
    let text = raw.trim();
    if text.is_empty() {
        return Err("empty import".into());
    }

    if let Some(username) = parse_user_spec(text) {
        return browse_chesscom_user(username);
    }

    if looks_like_chesscom_url(text) {
        import_chesscom_url(session, text)?;
        return Ok(ImportResult::Loaded);
    }

    if looks_like_path(text) && Path::new(text).is_file() {
        let contents = fs::read_to_string(text).map_err(|e| format!("read {text}: {e}"))?;
        import_text(session, &contents)?;
        return Ok(ImportResult::Loaded);
    }

    // Bare chess.com username (not a move / FEN / PGN).
    if let Some(username) = parse_bare_username(text) {
        return browse_chesscom_user(username);
    }

    import_text(session, text)?;
    Ok(ImportResult::Loaded)
}

/// Load a PGN string into the session (used after game-list selection).
#[cfg(feature = "chesscom")]
pub fn import_pgn(session: &mut EngineSession, pgn: &str) -> Result<(), String> {
    import_pgn_lite(session, pgn)
}

/// `user:gmdrj`, or a chess.com member profile URL.
fn parse_user_spec(text: &str) -> Option<String> {
    let s = text.trim();
    if let Some(rest) = strip_prefix_ci(s, "user:") {
        let rest = rest.trim();
        let username = rest.split(':').next().unwrap_or(rest).trim();
        if username.is_empty() {
            return None;
        }
        return Some(username.to_string());
    }
    for prefix in [
        "https://www.chess.com/member/",
        "http://www.chess.com/member/",
        "https://chess.com/member/",
        "http://chess.com/member/",
        "www.chess.com/member/",
        "chess.com/member/",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let username = rest
                .split(['/', '?', '#'])
                .next()
                .unwrap_or("")
                .trim();
            if !username.is_empty() {
                return Some(username.to_string());
            }
        }
    }
    None
}

/// Single token that looks like a chess.com username, not a SAN/UCI move.
fn parse_bare_username(text: &str) -> Option<String> {
    let s = text.trim();
    if s.contains(char::is_whitespace) {
        return None;
    }
    if looks_like_chess_move(s) {
        return None;
    }
    #[cfg(feature = "chesscom")]
    {
        crate::chesscom::normalize_username(s).ok()
    }
    #[cfg(not(feature = "chesscom"))]
    {
        // Still recognize so we can give a feature-flag error.
        if s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            && !s.is_empty()
            && !s.contains('/')
        {
            Some(s.to_ascii_lowercase())
        } else {
            None
        }
    }
}

/// Plausible SAN or UCI so we don't treat `e4` / `Nf3` as usernames.
fn looks_like_chess_move(tok: &str) -> bool {
    let t = tok.trim();
    if t.is_empty() || t.len() > 7 {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    if matches!(lower.as_str(), "o-o" | "o-o-o" | "0-0" | "0-0-0") {
        return true;
    }
    if !t
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '=' | '+' | '#' | 'x' | 'X'))
    {
        return false;
    }
    let bytes = lower.as_bytes();
    // UCI: e2e4 / e7e8q
    if (bytes.len() == 4 || bytes.len() == 5)
        && (b'a'..=b'h').contains(&bytes[0])
        && (b'1'..=b'8').contains(&bytes[1])
        && (b'a'..=b'h').contains(&bytes[2])
        && (b'1'..=b'8').contains(&bytes[3])
        && (bytes.len() == 4 || matches!(bytes[4], b'q' | b'r' | b'b' | b'n'))
    {
        return true;
    }
    // SAN must contain a destination square [a-h][1-8].
    let has_square = bytes.windows(2).any(|w| {
        (b'a'..=b'h').contains(&w[0]) && (b'1'..=b'8').contains(&w[1])
    });
    if !has_square {
        return false;
    }
    // Starts like a move: piece letter, or pawn file a–h.
    matches!(
        t.chars().next(),
        Some('N' | 'B' | 'R' | 'Q' | 'K' | 'a'..='h' | 'A'..='H')
    )
}

fn looks_like_chesscom_url(text: &str) -> bool {
    let s = text.trim();
    // Member URLs are handled by parse_user_spec.
    if s.contains("/member/") {
        return false;
    }
    s.contains("chess.com") || s.starts_with("game/") || s.contains("/game/")
}

fn browse_chesscom_user(username: String) -> Result<ImportResult, String> {
    #[cfg(feature = "chesscom")]
    {
        let user = crate::chesscom::normalize_username(&username).map_err(|e| e.to_string())?;
        if let Ok(Some(cache)) = crate::chesscom::load_games(&user) {
            if !cache.games.is_empty() {
                return Ok(ImportResult::BrowseGames {
                    username: user,
                    games: cache.games,
                    from_cache: true,
                    fetched_at: cache.fetched_at,
                });
            }
        }
        fetch_and_cache_games(user)
    }
    #[cfg(not(feature = "chesscom"))]
    {
        let _ = username;
        Err("chess.com username browse needs `--features chesscom`".into())
    }
}

/// Re-fetch from chess.com, overwrite the disk cache, and return a fresh browse result.
#[cfg(feature = "chesscom")]
pub fn refresh_chesscom_user(username: &str) -> Result<ImportResult, String> {
    let user = crate::chesscom::normalize_username(username).map_err(|e| e.to_string())?;
    fetch_and_cache_games(user)
}

#[cfg(feature = "chesscom")]
fn fetch_and_cache_games(user: String) -> Result<ImportResult, String> {
    let games = crate::chesscom::list_recent_games(&user).map_err(|e| e.to_string())?;
    if games.is_empty() {
        return Err(format!("no games for chess.com user '{user}'"));
    }
    let fetched_at = match crate::chesscom::save_games(&user, &games) {
        Ok(_) => std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        Err(_) => 0,
    };
    Ok(ImportResult::BrowseGames {
        username: user,
        games,
        from_cache: false,
        fetched_at,
    })
}

fn import_chesscom_url(session: &mut EngineSession, url: &str) -> Result<(), String> {
    #[cfg(feature = "chesscom")]
    {
        let pgn = crate::chesscom::fetch_pgn_from_url(url).map_err(|e| e.to_string())?;
        return import_pgn_lite(session, &pgn);
    }
    #[cfg(not(feature = "chesscom"))]
    {
        let _ = (session, url);
        Err("chess.com game URL import needs `--features chesscom`".into())
    }
}

fn looks_like_path(text: &str) -> bool {
    text.contains('/') || text.contains('\\') || text.ends_with(".fen") || text.ends_with(".pgn")
}

fn import_text(session: &mut EngineSession, text: &str) -> Result<(), String> {
    let trimmed = text.trim();
    if trimmed.contains("[Event") || trimmed.contains("[FEN") || trimmed.contains("1.") {
        return import_pgn_lite(session, trimmed);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "fen ") {
        return session.load_fen(rest.trim());
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "moves ") {
        return load_move_list(session, START_FEN, rest);
    }
    if let Some(rest) = strip_prefix_ci(trimmed, "startpos moves ") {
        return load_move_list(session, START_FEN, rest);
    }
    if looks_like_fen(trimmed) {
        return session.load_fen(trimmed);
    }
    if trimmed.split_whitespace().count() >= 1
        && !trimmed.contains('/')
        && trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c.is_whitespace() || c == '-' || c == '=')
    {
        let toks: Vec<_> = trimmed.split_whitespace().collect();
        if toks.iter().all(|t| (2..=7).contains(&t.len())) {
            return load_move_list(session, START_FEN, trimmed);
        }
    }
    Err(
        "import: FEN, PGN, game URL, username, user:NAME, member URL, `moves …`, or file path"
            .into(),
    )
}

fn load_move_list(session: &mut EngineSession, start_fen: &str, list: &str) -> Result<(), String> {
    let tokens: Vec<String> = list.split_whitespace().map(|s| s.to_string()).collect();
    if tokens.is_empty() {
        return Err("no moves in list".into());
    }
    let plies = EngineSession::resolve_move_tokens(start_fen, &tokens)?;
    let game = AnalyzedGame::new(start_fen.to_string(), plies, GameHeaders::default());
    session.load_analyzed_game(game)
}

fn strip_prefix_ci<'a>(s: &'a str, prefix: &str) -> Option<&'a str> {
    if s.len() >= prefix.len() && s[..prefix.len()].eq_ignore_ascii_case(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn looks_like_fen(s: &str) -> bool {
    s.split_whitespace()
        .next()
        .map(|p| p.matches('/').count() == 7)
        .unwrap_or(false)
}

fn import_pgn_lite(session: &mut EngineSession, pgn: &str) -> Result<(), String> {
    let start_fen = extract_fen_tag(pgn).unwrap_or_else(|| START_FEN.to_string());
    let headers = extract_headers(pgn);
    let movetext = strip_pgn_tags(pgn);
    let tokens = extract_move_tokens(&movetext);

    if tokens.is_empty() {
        if extract_fen_tag(pgn).is_some() {
            return session.load_fen(&start_fen);
        }
        return Err("PGN: no moves found".into());
    }

    let plies = EngineSession::resolve_move_tokens(&start_fen, &tokens)?;
    let n = plies.len();
    let game = AnalyzedGame::new(start_fen, plies, headers);
    session.load_analyzed_game(game)?;
    session.set_status(format!("Imported {n} plies from PGN · ←/→ step"));
    Ok(())
}

fn extract_headers(pgn: &str) -> GameHeaders {
    GameHeaders {
        white: extract_tag(pgn, "White"),
        black: extract_tag(pgn, "Black"),
        result: extract_tag(pgn, "Result"),
    }
}

fn extract_tag(pgn: &str, name: &str) -> Option<String> {
    let key = format!("[{name} \"");
    let start = pgn.find(&key)? + key.len();
    let end = pgn[start..].find('"')? + start;
    let val = pgn[start..end].trim();
    if val.is_empty() || val == "?" {
        None
    } else {
        Some(val.to_string())
    }
}

fn extract_fen_tag(pgn: &str) -> Option<String> {
    extract_tag(pgn, "FEN")
}

fn strip_pgn_tags(pgn: &str) -> String {
    let mut out = String::new();
    for line in pgn.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            continue;
        }
        out.push_str(t);
        out.push(' ');
    }
    strip_brace_comments(&out)
}

/// Remove `{...}` comments (clocks, annotations) from movetext.
fn strip_brace_comments(movetext: &str) -> String {
    let mut out = String::with_capacity(movetext.len());
    let mut chars = movetext.chars();
    while let Some(c) = chars.next() {
        if c == '{' {
            for d in chars.by_ref() {
                if d == '}' {
                    break;
                }
            }
            out.push(' ');
        } else {
            out.push(c);
        }
    }
    out
}

fn extract_move_tokens(movetext: &str) -> Vec<String> {
    movetext
        .split_whitespace()
        .filter_map(|tok| {
            let t = tok.trim_matches(|c: char| matches!(c, '.' | ';' | '!' | '?'));
            if t.chars().all(|c| c.is_ascii_digit() || c == '.') {
                return None;
            }
            if matches!(t, "*" | "1-0" | "0-1" | "1/2-1/2") {
                return None;
            }
            Some(t.to_string())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn move_heuristic_accepts_san_and_uci() {
        assert!(looks_like_chess_move("e4"));
        assert!(looks_like_chess_move("Nf3"));
        assert!(looks_like_chess_move("exd5"));
        assert!(looks_like_chess_move("e8=Q"));
        assert!(looks_like_chess_move("O-O"));
        assert!(looks_like_chess_move("e2e4"));
        assert!(!looks_like_chess_move("hikaru"));
        assert!(!looks_like_chess_move("magnuscarlsen"));
    }
}
