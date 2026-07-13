//! Filtering / matchmaking config for the Lichess bot daemon.
//!
//! Deserializable (JSON today; the same shape maps cleanly onto the TOML
//! sketched in `research/LICHESS.md §11.3`) but primarily driven by CLI flags
//! and sane defaults so a token is the only strictly required input.

use super::events::ChallengeInfo;
use serde::Deserialize;

/// Speeds Lichess reports on challenges. UltraBullet is disallowed for bots.
pub const DEFAULT_SPEEDS: &[&str] = &["bullet", "blitz", "rapid", "classical"];

/// Challenge acceptance + matchmaking policy.
#[derive(Clone, Debug, Deserialize)]
#[serde(default)]
pub struct LichessConfig {
    /// Environment variable holding the API token.
    pub token_env: String,
    /// Accept incoming challenges at all.
    pub accept_challenges: bool,
    /// Accept rated challenges (Lichess advises casual while debugging).
    pub accept_rated: bool,
    /// Accept challenges from human (non-BOT) accounts.
    pub accept_humans: bool,
    /// Allowed `speed` values (see [`DEFAULT_SPEEDS`]).
    pub speeds: Vec<String>,
    /// Allowed `variant.key` values (OpenChess is standard-only for now).
    pub variants: Vec<String>,
    /// Inclusive opponent-rating band (avoids sand-bagging accusations).
    pub min_opponent_rating: u32,
    pub max_opponent_rating: u32,
}

impl Default for LichessConfig {
    fn default() -> Self {
        Self {
            token_env: "LICHESS_TOKEN".into(),
            accept_challenges: true,
            accept_rated: false,
            accept_humans: true,
            speeds: DEFAULT_SPEEDS.iter().map(|s| (*s).to_string()).collect(),
            variants: vec!["standard".into()],
            min_opponent_rating: 0,
            max_opponent_rating: 4000,
        }
    }
}

/// Outcome of filtering one incoming challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChallengeDecision {
    Accept,
    /// Decline with a Lichess decline `reason` keyword.
    Decline(&'static str),
}

impl LichessConfig {
    /// Decide whether to accept `challenge`, returning a decline reason otherwise.
    ///
    /// Reason keywords match the Lichess decline API (`generic`, `variant`,
    /// `timeControl`, `rated`, `casual`, `noBot`, `later`).
    pub fn decide(&self, challenge: &ChallengeInfo) -> ChallengeDecision {
        if !self.accept_challenges {
            return ChallengeDecision::Decline("later");
        }
        if !self.variants.iter().any(|v| v == &challenge.variant.key) {
            return ChallengeDecision::Decline("variant");
        }
        if !self.speeds.iter().any(|s| s == &challenge.speed) {
            return ChallengeDecision::Decline("timeControl");
        }
        if challenge.rated && !self.accept_rated {
            return ChallengeDecision::Decline("casual");
        }
        if !challenge.rated && self.accept_rated {
            // We only want rated games right now.
            return ChallengeDecision::Decline("rated");
        }
        if !self.accept_humans && !challenge.challenger.is_bot() {
            return ChallengeDecision::Decline("noBot");
        }
        if let Some(rating) = challenge.challenger.rating
            && (rating < self.min_opponent_rating || rating > self.max_opponent_rating)
        {
            return ChallengeDecision::Decline("generic");
        }
        ChallengeDecision::Accept
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lichess::events::{ChallengeInfo, Challenger, Variant};

    fn challenge(speed: &str, variant: &str, rated: bool) -> ChallengeInfo {
        ChallengeInfo {
            id: "abc123".into(),
            status: "created".into(),
            speed: speed.into(),
            rated,
            variant: Variant {
                key: variant.into(),
            },
            challenger: Challenger {
                id: None,
                name: "someone".into(),
                title: None,
                rating: Some(1500),
            },
            time_control: None,
        }
    }

    #[test]
    fn accepts_standard_casual_by_default() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Accept
        );
    }

    #[test]
    fn declines_non_standard_variant() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&challenge("blitz", "chess960", false)),
            ChallengeDecision::Decline("variant")
        );
    }

    #[test]
    fn declines_disallowed_speed() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&challenge("ultraBullet", "standard", false)),
            ChallengeDecision::Decline("timeControl")
        );
    }

    #[test]
    fn declines_rated_when_casual_only() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", true)),
            ChallengeDecision::Decline("casual")
        );
    }

    #[test]
    fn accepts_rated_when_configured() {
        let cfg = LichessConfig {
            accept_rated: true,
            ..Default::default()
        };
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", true)),
            ChallengeDecision::Accept
        );
        // Casual now declined when we only want rated.
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("rated")
        );
    }

    #[test]
    fn declines_out_of_band_rating() {
        let cfg = LichessConfig {
            min_opponent_rating: 1000,
            max_opponent_rating: 1400,
            ..Default::default()
        };
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("generic")
        );
    }

    #[test]
    fn declines_humans_when_bot_only() {
        let cfg = LichessConfig {
            accept_humans: false,
            ..Default::default()
        };
        // Default challenger has no title → treated as human.
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("noBot")
        );
    }
}
