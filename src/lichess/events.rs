//! NDJSON event types for `GET /api/stream/event`.

use serde::Deserialize;

/// Top-level event from the global Lichess event stream.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum StreamEvent {
    GameStart {
        game: GameInfo,
    },
    GameFinish {
        game: GameFinishInfo,
    },
    Challenge {
        challenge: ChallengeInfo,
    },
    ChallengeCanceled {
        challenge: ChallengeRef,
    },
    ChallengeDeclined {
        challenge: ChallengeRef,
    },
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    #[serde(rename = "gameId")]
    pub game_id: String,
    pub fen: String,
    pub color: String,
    pub speed: String,
    pub rated: bool,
    #[serde(default)]
    pub is_my_turn: bool,
    pub variant: Variant,
    pub opponent: Opponent,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GameFinishInfo {
    #[serde(rename = "gameId")]
    pub game_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ChallengeInfo {
    pub id: String,
    pub status: String,
    pub speed: String,
    pub rated: bool,
    pub variant: Variant,
    pub challenger: Challenger,
    #[serde(default)]
    pub time_control: Option<TimeControl>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ChallengeRef {
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Variant {
    pub key: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Opponent {
    pub username: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Challenger {
    pub name: String,
    /// Player title (`"BOT"`, `"GM"`, …) when set.
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub rating: Option<u32>,
}

impl Challenger {
    /// True when the challenger is a bot account.
    pub fn is_bot(&self) -> bool {
        self.title.as_deref() == Some("BOT")
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TimeControl {
    #[serde(rename = "type")]
    pub kind: String,
    pub limit: u32,
    pub increment: u32,
    #[serde(default)]
    pub show: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/lichess")
            .join(name);
        std::fs::read_to_string(path).expect("read fixture")
    }

    #[test]
    fn deserializes_game_start() {
        let event: StreamEvent = serde_json::from_str(&fixture("game_start.json")).unwrap();
        let StreamEvent::GameStart { game } = event else {
            panic!("expected gameStart");
        };
        assert_eq!(game.game_id, "0FgNPGRz");
        assert_eq!(game.color, "white");
        assert_eq!(game.speed, "blitz");
        assert!(!game.rated);
        assert_eq!(game.variant.key, "standard");
        assert_eq!(game.opponent.username, "bernstein-2ply");
    }

    #[test]
    fn deserializes_challenge() {
        let event: StreamEvent = serde_json::from_str(&fixture("challenge.json")).unwrap();
        let StreamEvent::Challenge { challenge } = event else {
            panic!("expected challenge");
        };
        assert_eq!(challenge.id, "uGK4MHaQ");
        assert_eq!(challenge.speed, "rapid");
        assert!(!challenge.rated);
        assert_eq!(challenge.variant.key, "standard");
        assert_eq!(challenge.challenger.name, "bernstein-2ply");
        let tc = challenge.time_control.unwrap();
        assert_eq!(tc.limit, 300);
        assert_eq!(tc.increment, 1);
    }

    #[test]
    fn deserializes_game_finish() {
        let event: StreamEvent = serde_json::from_str(&fixture("game_finish.json")).unwrap();
        let StreamEvent::GameFinish { game } = event else {
            panic!("expected gameFinish");
        };
        assert_eq!(game.game_id, "0FgNPGRz");
    }
}
