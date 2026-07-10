//! HTTP helpers for chess.com public endpoints.

use crate::chesscom::ChessComError;
use serde::de::DeserializeOwned;

const USER_AGENT: &str = "OpenChess/0.1 (+https://github.com/jonahlysne/openchess)";

pub fn get_json<T: DeserializeOwned>(url: &str) -> Result<T, ChessComError> {
    let body = get_text(url)?;
    serde_json::from_str(&body).map_err(|e| ChessComError::Json(format!("{url}: {e}")))
}

pub fn get_text(url: &str) -> Result<String, ChessComError> {
    let response = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/json")
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(code, resp) => {
                let detail = resp.into_string().unwrap_or_default();
                ChessComError::Http(format!("{url}: HTTP {code}: {detail}"))
            }
            other => ChessComError::Http(format!("{url}: {other}")),
        })?;

    response
        .into_string()
        .map_err(|e| ChessComError::Http(format!("{url}: read body: {e}")))
}
