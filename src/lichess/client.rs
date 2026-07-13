//! HTTP helpers for the Lichess Bot API.
//!
//! One authenticated [`Client`] wraps the bearer token and exposes the small
//! surface the daemon needs: JSON/text GETs, long-lived NDJSON streams, and the
//! REST POSTs for challenges and in-game actions. Streaming is generic over the
//! decoded event type so both the global event stream ([`super::StreamEvent`])
//! and the per-game stream ([`super::game::GameStreamEvent`]) share one reader.

use crate::lichess::LichessError;
use serde::de::DeserializeOwned;
use std::io::{BufRead, BufReader, Read};
use std::marker::PhantomData;

pub const BASE_URL: &str = "https://lichess.org";
const USER_AGENT: &str = "OpenChess/0.1 (+https://github.com/jonahlysne/openchess)";

/// Authenticated Lichess API client.
pub struct Client {
    token: String,
}

impl Client {
    /// Load bearer token from `token_env` (default `LICHESS_TOKEN`).
    pub fn from_env(token_env: &str) -> Result<Self, LichessError> {
        let token =
            std::env::var(token_env).map_err(|_| LichessError::MissingToken(token_env.into()))?;
        if token.is_empty() {
            return Err(LichessError::MissingToken(token_env.into()));
        }
        Ok(Self { token })
    }

    /// Construct a client from an explicit token (used in tests).
    pub fn with_token(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }

    pub fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, LichessError> {
        let url = format!("{BASE_URL}{path}");
        let body = self.get_text(path)?;
        serde_json::from_str(&body).map_err(|e| LichessError::Json(format!("{url}: {e}")))
    }

    pub fn get_text(&self, path: &str) -> Result<String, LichessError> {
        let url = format!("{BASE_URL}{path}");
        let response = self
            .request(ureq::get(&url))
            .call()
            .map_err(|e| map_ureq_error(&url, e))?;

        response
            .into_string()
            .map_err(|e| LichessError::Http(format!("{url}: read body: {e}")))
    }

    /// Open a long-lived NDJSON stream decoding each line into `T`.
    pub fn open_ndjson_stream<T: DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<NdjsonStream<T>, LichessError> {
        let url = format!("{BASE_URL}{path}");
        let response = self
            .request(ureq::get(&url))
            .call()
            .map_err(|e| map_ureq_error(&url, e))?;

        Ok(NdjsonStream::new(response.into_reader()))
    }

    /// POST with an empty body (challenge accept/decline, resign, abort, …).
    pub fn post_empty(&self, path: &str) -> Result<(), LichessError> {
        let url = format!("{BASE_URL}{path}");
        self.request(ureq::post(&url))
            .call()
            .map_err(|e| map_ureq_error(&url, e))?;
        Ok(())
    }

    /// POST a form-encoded body (outbound challenge creation).
    pub fn post_form(&self, path: &str, form: &[(&str, &str)]) -> Result<String, LichessError> {
        let url = format!("{BASE_URL}{path}");
        let response = self
            .request(ureq::post(&url))
            .send_form(form)
            .map_err(|e| map_ureq_error(&url, e))?;
        response
            .into_string()
            .map_err(|e| LichessError::Http(format!("{url}: read body: {e}")))
    }

    /// Play `uci_move` in `game_id` (`POST /api/bot/game/{id}/move/{uci}`).
    pub fn play_move(&self, game_id: &str, uci_move: &str) -> Result<(), LichessError> {
        self.post_empty(&format!("/api/bot/game/{game_id}/move/{uci_move}"))
    }

    /// Resign `game_id`.
    pub fn resign(&self, game_id: &str) -> Result<(), LichessError> {
        self.post_empty(&format!("/api/bot/game/{game_id}/resign"))
    }

    /// Abort `game_id` (only valid before either side has moved twice).
    pub fn abort(&self, game_id: &str) -> Result<(), LichessError> {
        self.post_empty(&format!("/api/bot/game/{game_id}/abort"))
    }

    fn request(&self, req: ureq::Request) -> ureq::Request {
        req.set("User-Agent", USER_AGENT)
            .set("Accept", "application/x-ndjson, application/json")
            .set("Authorization", &format!("Bearer {}", self.token))
    }
}

/// One item from an NDJSON stream.
#[derive(Debug, PartialEq, Eq)]
pub enum NdjsonItem<T> {
    /// Empty line keepalive (sent every ~7s).
    Keepalive,
    /// Parsed JSON event.
    Event(T),
}

/// Blocking NDJSON line reader decoding each non-empty line into `T`.
pub struct NdjsonStream<T> {
    reader: BufReader<Box<dyn Read + Send + Sync>>,
    _marker: PhantomData<T>,
}

impl<T: DeserializeOwned> NdjsonStream<T> {
    pub(crate) fn new(reader: Box<dyn Read + Send + Sync>) -> Self {
        Self {
            reader: BufReader::new(reader),
            _marker: PhantomData,
        }
    }

    /// Build a stream from any reader (used in tests with in-memory fixtures).
    pub fn from_reader(reader: impl Read + Send + Sync + 'static) -> Self {
        Self::new(Box::new(reader))
    }

    /// Read the next stream item. `None` means EOF (stream closed).
    pub fn read_item(&mut self) -> Result<Option<NdjsonItem<T>>, LichessError> {
        let mut line = String::new();
        let n = self
            .reader
            .read_line(&mut line)
            .map_err(|e| LichessError::Http(format!("read stream: {e}")))?;
        if n == 0 {
            return Ok(None);
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            return Ok(Some(NdjsonItem::Keepalive));
        }
        let event: T = serde_json::from_str(trimmed)
            .map_err(|e| LichessError::Json(format!("stream line: {e}: {trimmed}")))?;
        Ok(Some(NdjsonItem::Event(event)))
    }
}

fn map_ureq_error(url: &str, err: ureq::Error) -> LichessError {
    match err {
        ureq::Error::Status(429, _) => LichessError::RateLimited,
        ureq::Error::Status(code, resp) => {
            let detail = resp.into_string().unwrap_or_default();
            LichessError::Http(format!("{url}: HTTP {code}: {detail}"))
        }
        other => LichessError::Http(format!("{url}: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lichess::StreamEvent;
    use std::io::Cursor;

    #[test]
    fn reads_events_and_keepalives_from_ndjson() {
        // Blank lines are keepalives; each JSON line decodes into the event type.
        let body = concat!(
            "\n",
            "{\"type\":\"gameFinish\",\"game\":{\"gameId\":\"abc\"}}\n",
            "\n",
        );
        let mut stream: NdjsonStream<StreamEvent> = NdjsonStream::from_reader(Cursor::new(body));

        assert_eq!(stream.read_item().unwrap(), Some(NdjsonItem::Keepalive));
        match stream.read_item().unwrap() {
            Some(NdjsonItem::Event(StreamEvent::GameFinish { game })) => {
                assert_eq!(game.game_id, "abc");
            }
            other => panic!("expected gameFinish event, got {other:?}"),
        }
        assert_eq!(stream.read_item().unwrap(), Some(NdjsonItem::Keepalive));
        assert_eq!(stream.read_item().unwrap(), None);
    }

    #[test]
    fn malformed_line_is_an_error() {
        let mut stream: NdjsonStream<StreamEvent> =
            NdjsonStream::from_reader(Cursor::new("{not json}\n"));
        assert!(matches!(stream.read_item(), Err(LichessError::Json(_))));
    }
}
