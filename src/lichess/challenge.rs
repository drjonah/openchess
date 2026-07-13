//! Challenge handling: filter/accept incoming (P9-04) and create outbound (P9-05).

use super::client::Client;
use super::config::{ChallengeDecision, LichessConfig};
use super::events::ChallengeInfo;
use super::LichessError;

/// Apply the config filter to an incoming challenge and accept or decline it.
///
/// Returns `true` when the challenge was accepted.
pub fn handle_incoming(
    client: &Client,
    config: &LichessConfig,
    challenge: &ChallengeInfo,
) -> Result<bool, LichessError> {
    match config.decide(challenge) {
        ChallengeDecision::Accept => {
            client.post_empty(&format!("/api/challenge/{}/accept", challenge.id))?;
            Ok(true)
        }
        ChallengeDecision::Decline(reason) => {
            decline(client, &challenge.id, reason)?;
            Ok(false)
        }
    }
}

/// Decline `challenge_id` with a Lichess decline `reason` keyword.
pub fn decline(client: &Client, challenge_id: &str, reason: &str) -> Result<(), LichessError> {
    client.post_form(
        &format!("/api/challenge/{challenge_id}/decline"),
        &[("reason", reason)],
    )?;
    Ok(())
}

/// Parameters for an outbound challenge (`POST /api/challenge/{user}`).
#[derive(Clone, Debug)]
pub struct OutboundChallenge {
    pub username: String,
    pub clock_limit_secs: u32,
    pub clock_increment_secs: u32,
    pub rated: bool,
    /// `white`, `black`, or `random`.
    pub color: String,
    pub variant: String,
}

impl Default for OutboundChallenge {
    fn default() -> Self {
        Self {
            username: String::new(),
            clock_limit_secs: 300,
            clock_increment_secs: 3,
            rated: false,
            color: "random".into(),
            variant: "standard".into(),
        }
    }
}

impl OutboundChallenge {
    /// Form fields for the challenge-create request.
    ///
    /// `keepAliveStream=true` holds the challenge open past the ~20s realtime
    /// expiry so the acceptance / game-start can be observed.
    pub fn form_fields(&self) -> Vec<(&'static str, String)> {
        vec![
            ("clock.limit", self.clock_limit_secs.to_string()),
            ("clock.increment", self.clock_increment_secs.to_string()),
            ("rated", self.rated.to_string()),
            ("color", self.color.clone()),
            ("variant", self.variant.clone()),
            ("keepAliveStream", "true".to_string()),
        ]
    }

    /// Send the challenge. Returns the raw response body.
    pub fn send(&self, client: &Client) -> Result<String, LichessError> {
        let owned = self.form_fields();
        let form: Vec<(&str, &str)> = owned.iter().map(|(k, v)| (*k, v.as_str())).collect();
        client.post_form(&format!("/api/challenge/{}", self.username), &form)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_fields_include_clock_and_keepalive() {
        let ch = OutboundChallenge {
            username: "someBot".into(),
            clock_limit_secs: 180,
            clock_increment_secs: 2,
            rated: false,
            color: "white".into(),
            variant: "standard".into(),
        };
        let fields = ch.form_fields();
        let get = |k: &str| {
            fields
                .iter()
                .find(|(fk, _)| *fk == k)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(get("clock.limit").as_deref(), Some("180"));
        assert_eq!(get("clock.increment").as_deref(), Some("2"));
        assert_eq!(get("rated").as_deref(), Some("false"));
        assert_eq!(get("color").as_deref(), Some("white"));
        assert_eq!(get("variant").as_deref(), Some("standard"));
        assert_eq!(get("keepAliveStream").as_deref(), Some("true"));
    }
}
