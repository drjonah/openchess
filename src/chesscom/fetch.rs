//! Fetch PGN strings from chess.com by game URL or username.

use crate::chesscom::client;
use crate::chesscom::url::{normalize_username, parse_game_url, ParsedGameUrl};
use crate::chesscom::ChessComError;
use serde::Deserialize;

/// Compact game metadata for `--list` / index selection.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct GameSummary {
    pub url: String,
    pub white: String,
    pub black: String,
    pub end_time: u64,
    pub pgn: Option<String>,
    /// chess.com side result string (`win`, `resigned`, `agreed`, …).
    pub white_result: Option<String>,
    pub black_result: Option<String>,
}

/// Win / loss / draw from the perspective of a given player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameOutcome {
    Win,
    Loss,
    Draw,
}

impl GameSummary {
    /// Color the given username played, if they were in this game.
    pub fn color_for(&self, username: &str) -> Option<&'static str> {
        let u = username.to_ascii_lowercase();
        if self.white.eq_ignore_ascii_case(&u) {
            Some("White")
        } else if self.black.eq_ignore_ascii_case(&u) {
            Some("Black")
        } else {
            None
        }
    }

    /// Opponent username relative to `username`.
    pub fn opponent_for(&self, username: &str) -> &str {
        let u = username.to_ascii_lowercase();
        if self.white.eq_ignore_ascii_case(&u) {
            &self.black
        } else if self.black.eq_ignore_ascii_case(&u) {
            &self.white
        } else {
            "?"
        }
    }

    /// Outcome for `username` (win / loss / draw), if known.
    pub fn outcome_for(&self, username: &str) -> Option<GameOutcome> {
        let u = username.to_ascii_lowercase();
        let result = if self.white.eq_ignore_ascii_case(&u) {
            self.white_result.as_deref()
        } else if self.black.eq_ignore_ascii_case(&u) {
            self.black_result.as_deref()
        } else {
            return None;
        };
        result.and_then(classify_result)
    }

    /// Short relative time from `end_time` (unix seconds).
    pub fn relative_time(&self, now_secs: u64) -> String {
        format_relative_time(self.end_time, now_secs)
    }
}

fn classify_result(result: &str) -> Option<GameOutcome> {
    match result.to_ascii_lowercase().as_str() {
        "win" => Some(GameOutcome::Win),
        "checkmated" | "resigned" | "timeout" | "abandoned" | "lose" | "lost" => {
            Some(GameOutcome::Loss)
        }
        "agreed" | "stalemate" | "repetition" | "insufficient" | "50move" | "timevsinsufficient"
        | "draw" => Some(GameOutcome::Draw),
        _ => None,
    }
}

fn format_relative_time(end_time: u64, now_secs: u64) -> String {
    if end_time == 0 {
        return "unknown".into();
    }
    let ago = now_secs.saturating_sub(end_time);
    if ago < 60 {
        "just now".into()
    } else if ago < 3600 {
        let m = ago / 60;
        format!("{m}m ago")
    } else if ago < 86400 {
        let h = ago / 3600;
        format!("{h}h ago")
    } else if ago < 86400 * 14 {
        let d = ago / 86400;
        format!("{d}d ago")
    } else {
        // Fall back to YYYY-MM-DD via crude day count from unix epoch.
        let days = end_time / 86400;
        // 1970-01-01 + days — good enough for list display without chrono.
        let (y, m, d) = civil_from_days(days as i64);
        format!("{y:04}-{m:02}-{d:02}")
    }
}

/// Howard Hinnant civil_from_days (proleptic Gregorian).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = (yoe as i64 + era * 400) as i32;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

#[derive(Deserialize)]
struct ArchivesResponse {
    archives: Vec<String>,
}

#[derive(Deserialize)]
struct GamesResponse {
    games: Vec<ArchiveGame>,
}

#[derive(Deserialize)]
struct ArchiveGame {
    url: String,
    pgn: Option<String>,
    end_time: Option<u64>,
    white: Option<PlayerSide>,
    black: Option<PlayerSide>,
}

#[derive(Deserialize)]
struct PlayerSide {
    username: Option<String>,
    result: Option<String>,
}

#[derive(Deserialize)]
struct CallbackResponse {
    game: CallbackGame,
}

#[derive(Deserialize)]
struct CallbackGame {
    #[serde(rename = "pgnHeaders")]
    pgn_headers: PgnHeaders,
}

#[derive(Deserialize)]
struct PgnHeaders {
    #[serde(rename = "White")]
    white: String,
    #[serde(rename = "Date")]
    date: String,
}

/// Resolve a chess.com game URL to its full PGN.
pub fn fetch_pgn_from_url(url: &str) -> Result<String, ChessComError> {
    let parsed = parse_game_url(url)?;
    let (username, year, month) = resolve_archive_key(&parsed)?;
    let games = fetch_month_games(&username, year, month)?;
    let game = games
        .into_iter()
        .find(|g| game_url_matches_id(&g.url, &parsed.id))
        .ok_or_else(|| {
            ChessComError::NotFound(format!(
                "game {} not in {}'s {}/{:02} archive",
                parsed.id, username, year, month
            ))
        })?;
    game.pgn
        .ok_or_else(|| ChessComError::NotFound(format!("game {} has no PGN field", parsed.id)))
}

/// Most recent completed game (by `end_time`) in the player's latest archive month.
pub fn fetch_latest_pgn(username: &str) -> Result<String, ChessComError> {
    let mut games = list_recent_games_inner(username)?;
    let game = games
        .pop()
        .ok_or_else(|| ChessComError::NotFound(format!("no games for {username}")))?;
    game.pgn
        .ok_or_else(|| ChessComError::NotFound(format!("latest game for {username} has no PGN")))
}

/// Games from the player's most recent archive month, newest first.
pub fn list_recent_games(username: &str) -> Result<Vec<GameSummary>, ChessComError> {
    let mut games = list_recent_games_inner(username)?;
    games.reverse(); // newest first for listing / index
    Ok(games)
}

/// Select the Nth game (0 = newest) from the latest archive month.
pub fn fetch_pgn_by_index(username: &str, index: usize) -> Result<String, ChessComError> {
    let games = list_recent_games(username)?;
    let game = games.get(index).ok_or_else(|| {
        ChessComError::NotFound(format!(
            "index {index} out of range ({} games in latest month)",
            games.len()
        ))
    })?;
    game.pgn
        .clone()
        .ok_or_else(|| ChessComError::NotFound(format!("game at index {index} has no PGN")))
}

fn list_recent_games_inner(username: &str) -> Result<Vec<GameSummary>, ChessComError> {
    let user = normalize_username(username)?;
    let archive_url = latest_archive_url(&user)?;
    let resp: GamesResponse = client::get_json(&archive_url).map_err(|e| map_user_http_err(&user, e))?;
    let mut out: Vec<GameSummary> = resp
        .games
        .into_iter()
        .filter_map(|g| {
            let pgn = g.pgn?;
            let (white, white_result) = split_side(g.white);
            let (black, black_result) = split_side(g.black);
            Some(GameSummary {
                url: g.url,
                white,
                black,
                end_time: g.end_time.unwrap_or(0),
                pgn: Some(pgn),
                white_result,
                black_result,
            })
        })
        .collect();
    out.sort_by_key(|g| g.end_time);
    Ok(out)
}

fn split_side(side: Option<PlayerSide>) -> (String, Option<String>) {
    match side {
        Some(p) => (
            p.username.unwrap_or_else(|| "?".into()),
            p.result,
        ),
        None => ("?".into(), None),
    }
}

fn map_user_http_err(username: &str, err: ChessComError) -> ChessComError {
    match &err {
        ChessComError::Http(msg) if msg.contains("HTTP 404") => {
            ChessComError::NotFound(format!("chess.com user '{username}' not found"))
        }
        _ => err,
    }
}

fn latest_archive_url(username: &str) -> Result<String, ChessComError> {
    let url = format!("https://api.chess.com/pub/player/{username}/games/archives");
    let resp: ArchivesResponse =
        client::get_json(&url).map_err(|e| map_user_http_err(username, e))?;
    resp.archives
        .last()
        .cloned()
        .ok_or_else(|| ChessComError::NotFound(format!("no games for chess.com user '{username}'")))
}

fn resolve_archive_key(parsed: &ParsedGameUrl) -> Result<(String, u32, u32), ChessComError> {
    let callback = format!(
        "https://www.chess.com/callback/{}/game/{}",
        parsed.kind.as_str(),
        parsed.id
    );
    let resp: CallbackResponse = client::get_json(&callback)?;
    let username = normalize_username(&resp.game.pgn_headers.white)?;
    let (year, month) = parse_pgn_date(&resp.game.pgn_headers.date)?;
    Ok((username, year, month))
}

fn parse_pgn_date(date: &str) -> Result<(u32, u32), ChessComError> {
    // "YYYY.MM.DD"
    let mut parts = date.split('.');
    let year: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ChessComError::Json(format!("bad PGN Date year: {date:?}")))?;
    let month: u32 = parts
        .next()
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| ChessComError::Json(format!("bad PGN Date month: {date:?}")))?;
    if !(1..=12).contains(&month) {
        return Err(ChessComError::Json(format!("bad PGN Date month: {date:?}")));
    }
    Ok((year, month))
}

fn fetch_month_games(
    username: &str,
    year: u32,
    month: u32,
) -> Result<Vec<ArchiveGame>, ChessComError> {
    let url = format!("https://api.chess.com/pub/player/{username}/games/{year}/{month:02}");
    let resp: GamesResponse = client::get_json(&url)?;
    Ok(resp.games)
}

fn game_url_matches_id(game_url: &str, id: &str) -> bool {
    game_url
        .trim_end_matches('/')
        .rsplit('/')
        .next()
        .is_some_and(|last| last == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_pgn_date_ok() {
        assert_eq!(parse_pgn_date("2024.01.15").unwrap(), (2024, 1));
    }

    #[test]
    fn match_game_id_from_url() {
        assert!(game_url_matches_id(
            "https://www.chess.com/game/live/169227053782",
            "169227053782"
        ));
        assert!(!game_url_matches_id(
            "https://www.chess.com/game/live/1",
            "169227053782"
        ));
    }

    /// Live smoke test — run with `cargo test --features chesscom -- --ignored`.
    #[test]
    #[ignore]
    fn fetch_hikaru_latest_smoke() {
        let pgn = fetch_latest_pgn("hikaru").expect("fetch latest");
        assert!(pgn.contains("[Event"), "expected PGN headers, got: {pgn}");
    }

    #[test]
    fn outcome_and_opponent() {
        let g = GameSummary {
            url: String::new(),
            white: "Alice".into(),
            black: "Bob".into(),
            end_time: 1_700_000_000,
            pgn: None,
            white_result: Some("win".into()),
            black_result: Some("resigned".into()),
        };
        assert_eq!(g.color_for("alice"), Some("White"));
        assert_eq!(g.opponent_for("alice"), "Bob");
        assert_eq!(g.outcome_for("alice"), Some(GameOutcome::Win));
        assert_eq!(g.outcome_for("bob"), Some(GameOutcome::Loss));
        assert_eq!(g.relative_time(1_700_000_000 + 120), "2m ago");
    }

    #[test]
    fn classify_draw_results() {
        assert_eq!(classify_result("agreed"), Some(GameOutcome::Draw));
        assert_eq!(classify_result("stalemate"), Some(GameOutcome::Draw));
    }
}
