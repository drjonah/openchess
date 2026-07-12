//! HTTP helpers for the Lichess Bot API.

use crate::lichess::{LichessError, StreamEvent};
use serde::de::DeserializeOwned;
use std::io::{BufRead, BufReader, Read};

pub const BASE_URL: &str = "https://lichess.org";
const USER_AGENT: &str = "OpenChess/0.1 (+https://github.com/jonahlysne/openchess)";

/// Authenticated Lichess API client.
pub struct Client {
    token: String,
}

impl Client {
    /// Load bearer token from `token_env` (default `LICHESS_TOKEN`).
    pub fn from_env(token_env: &str) -> Result<Self, LichessError> {
        let token = std::env::var(token_env).map_err(|_| LichessError::MissingToken(token_env.into()))?;
        if token.is_empty() {
            return Err(LichessError::MissingToken(token_env.into()));
        }
        Ok(Self { token })
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

    /// Open a long-lived NDJSON stream (caller reads lines until EOF).
    pub fn open_ndjson_stream(&self, path: &str) -> Result<NdjsonStream, LichessError> {
        let url = format!("{BASE_URL}{path}");
        let response = self
            .request(ureq::get(&url))
            .call()
            .map_err(|e| map_ureq_error(&url, e))?;

        let reader = response.into_reader();

        Ok(NdjsonStream {
            reader: BufReader::new(reader),
        })
    }

    /// POST with an empty body (challenge accept/decline, etc.).
    pub fn post_empty(&self, path: &str) -> Result<(), LichessError> {
        let url = format!("{BASE_URL}{path}");
        self.request(ureq::post(&url))
            .call()
            .map_err(|e| map_ureq_error(&url, e))?;
        Ok(())
    }

    fn request(&self, mut req: ureq::Request) -> ureq::Request {
        req = req
            .set("User-Agent", USER_AGENT)
            .set("Accept", "application/x-ndjson, application/json")
            .set("Authorization", &format!("Bearer {}", self.token));
        req
    }
}

/// One item from an NDJSON stream.
#[derive(Debug)]
pub enum NdjsonItem {
    /// Empty line keepalive (sent every ~7s).
    Keepalive,
    /// Parsed JSON event.
    Event(StreamEvent),
}

/// Blocking NDJSON line reader over a Lichess stream response.
pub struct NdjsonStream {
    reader: BufReader<Box<dyn Read + Send + Sync>>,
}

impl NdjsonStream {
    /// Read the next stream item. `None` means EOF.
    pub fn read_item(&mut self) -> Result<Option<NdjsonItem>, LichessError> {
        let mut line = String::new();
        loop {
            line.clear();
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
            let event: StreamEvent = serde_json::from_str(trimmed)
                .map_err(|e| LichessError::Json(format!("stream line: {e}: {trimmed}")))?;
            return Ok(Some(NdjsonItem::Event(event)));
        }
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
