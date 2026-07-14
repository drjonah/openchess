//! Filtering / matchmaking config for the Lichess bot daemon.
//!
//! Load from a TOML or JSON file (`LichessConfig::load_from_path`); CLI flags
//! override file values. Defaults are production-safe for casual bot-vs-bot:
//! rated off, humans declined, one game at a time, ponder never used.

use super::events::ChallengeInfo;
use super::LichessError;
use serde::Deserialize;
use std::path::Path;

/// Speeds Lichess reports on challenges. UltraBullet is disallowed for bots.
pub const DEFAULT_SPEEDS: &[&str] = &["bullet", "blitz", "rapid", "classical"];

/// Challenge acceptance + matchmaking policy.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct LichessConfig {
    /// Environment variable holding the API token.
    pub token_env: String,
    /// Accept incoming challenges at all.
    pub accept_challenges: bool,
    /// Accept rated challenges. Default false until the L2-06 rated gate.
    pub accept_rated: bool,
    /// Accept challenges from human (non-BOT) accounts. Default false (bots preferred).
    pub accept_humans: bool,
    /// Allowed `speed` values (see [`DEFAULT_SPEEDS`]).
    pub speeds: Vec<String>,
    /// Allowed `variant.key` values (OpenChess is standard-only for now).
    pub variants: Vec<String>,
    /// Inclusive opponent-rating band (avoids sand-bagging accusations).
    pub min_opponent_rating: u32,
    pub max_opponent_rating: u32,
    /// Max simultaneous games. Phase 1 / pre-L2-07 always clamps to 1.
    pub max_concurrent_games: u32,
}

impl Default for LichessConfig {
    fn default() -> Self {
        Self {
            token_env: "LICHESS_TOKEN".into(),
            accept_challenges: true,
            accept_rated: false,
            accept_humans: false,
            speeds: DEFAULT_SPEEDS.iter().map(|s| (*s).to_string()).collect(),
            variants: vec!["standard".into()],
            min_opponent_rating: 0,
            max_opponent_rating: 4000,
            max_concurrent_games: 1,
        }
    }
}

/// Explicit CLI overrides applied on top of defaults / a config file.
///
/// `None` means "leave the loaded value alone".
#[derive(Clone, Debug, Default)]
pub struct ConfigOverrides {
    pub token_env: Option<String>,
    pub accept_challenges: Option<bool>,
    pub accept_rated: Option<bool>,
    pub accept_humans: Option<bool>,
    pub speeds: Option<Vec<String>>,
    pub variants: Option<Vec<String>>,
    pub min_opponent_rating: Option<u32>,
    pub max_opponent_rating: Option<u32>,
    pub max_concurrent_games: Option<u32>,
}

/// Outcome of filtering one incoming challenge.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChallengeDecision {
    Accept,
    /// Decline with a Lichess decline `reason` keyword.
    Decline(&'static str),
}

impl LichessConfig {
    /// Load from a `.toml` or `.json` path. Extension selects the parser.
    pub fn load_from_path(path: &Path) -> Result<Self, LichessError> {
        let text = std::fs::read_to_string(path).map_err(|e| {
            LichessError::Http(format!("could not read config {}: {e}", path.display()))
        })?;
        let mut cfg = Self::parse_str(&text, path)?;
        cfg.clamp();
        Ok(cfg)
    }

    /// Parse config text; `path` is used only for extension / error context.
    pub fn parse_str(text: &str, path: &Path) -> Result<Self, LichessError> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "toml" => toml::from_str(text).map_err(|e| {
                LichessError::Http(format!("invalid TOML config {}: {e}", path.display()))
            }),
            "json" => serde_json::from_str(text).map_err(|e| {
                LichessError::Http(format!("invalid JSON config {}: {e}", path.display()))
            }),
            other => Err(LichessError::Http(format!(
                "unsupported config extension .{other} for {} (use .toml or .json)",
                path.display()
            ))),
        }
    }

    /// Apply CLI overrides; CLI wins over file / defaults.
    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) {
        if let Some(v) = &overrides.token_env {
            self.token_env = v.clone();
        }
        if let Some(v) = overrides.accept_challenges {
            self.accept_challenges = v;
        }
        if let Some(v) = overrides.accept_rated {
            self.accept_rated = v;
        }
        if let Some(v) = overrides.accept_humans {
            self.accept_humans = v;
        }
        if let Some(v) = &overrides.speeds {
            self.speeds = v.clone();
        }
        if let Some(v) = &overrides.variants {
            self.variants = v.clone();
        }
        if let Some(v) = overrides.min_opponent_rating {
            self.min_opponent_rating = v;
        }
        if let Some(v) = overrides.max_opponent_rating {
            self.max_opponent_rating = v;
        }
        if let Some(v) = overrides.max_concurrent_games {
            self.max_concurrent_games = v;
        }
        self.clamp();
    }

    /// Keep concurrent games at the supported floor until L2-07 ships.
    pub fn clamp(&mut self) {
        if self.max_concurrent_games == 0 {
            self.max_concurrent_games = 1;
        }
        // Multi-game is L2-07; refuse silent >1 until that lands.
        if self.max_concurrent_games > 1 {
            self.max_concurrent_games = 1;
        }
        if self.speeds.is_empty() {
            self.speeds = DEFAULT_SPEEDS.iter().map(|s| (*s).to_string()).collect();
        }
        if self.variants.is_empty() {
            self.variants = vec!["standard".into()];
        }
        if self.min_opponent_rating > self.max_opponent_rating {
            std::mem::swap(
                &mut self.min_opponent_rating,
                &mut self.max_opponent_rating,
            );
        }
    }

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

    /// One-line summary for operator logs (never includes the token).
    pub fn summary_line(&self) -> String {
        format!(
            "accept_challenges={} accept_rated={} accept_humans={} speeds={:?} variants={:?} rating={}..{} max_games={} token_env={}",
            self.accept_challenges,
            self.accept_rated,
            self.accept_humans,
            self.speeds,
            self.variants,
            self.min_opponent_rating,
            self.max_opponent_rating,
            self.max_concurrent_games,
            self.token_env,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lichess::events::{ChallengeInfo, Challenger, Variant};
    use std::path::PathBuf;

    fn challenge(speed: &str, variant: &str, rated: bool) -> ChallengeInfo {
        challenge_with_title(speed, variant, rated, None)
    }

    fn bot_challenge(speed: &str, variant: &str, rated: bool) -> ChallengeInfo {
        challenge_with_title(speed, variant, rated, Some("BOT"))
    }

    fn challenge_with_title(
        speed: &str,
        variant: &str,
        rated: bool,
        title: Option<&str>,
    ) -> ChallengeInfo {
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
                title: title.map(str::to_string),
                rating: Some(1500),
            },
            time_control: None,
        }
    }

    #[test]
    fn defaults_accept_bot_casual_only() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&bot_challenge("blitz", "standard", false)),
            ChallengeDecision::Accept
        );
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("noBot")
        );
        assert_eq!(
            cfg.decide(&bot_challenge("blitz", "standard", true)),
            ChallengeDecision::Decline("casual")
        );
    }

    #[test]
    fn declines_non_standard_variant() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&bot_challenge("blitz", "chess960", false)),
            ChallengeDecision::Decline("variant")
        );
    }

    #[test]
    fn declines_disallowed_speed() {
        let cfg = LichessConfig::default();
        assert_eq!(
            cfg.decide(&bot_challenge("ultraBullet", "standard", false)),
            ChallengeDecision::Decline("timeControl")
        );
    }

    #[test]
    fn accept_rated_allows_rated_without_forcing_rated_only() {
        let cfg = LichessConfig {
            accept_rated: true,
            accept_humans: true,
            ..Default::default()
        };
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", true)),
            ChallengeDecision::Accept
        );
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Accept
        );
    }

    #[test]
    fn declines_out_of_band_rating() {
        let cfg = LichessConfig {
            accept_humans: true,
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
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("noBot")
        );
        assert_eq!(
            cfg.decide(&bot_challenge("blitz", "standard", false)),
            ChallengeDecision::Accept
        );
    }

    #[test]
    fn loads_toml_and_json() {
        let dir = std::env::temp_dir().join("openchess-lichess-config-test");
        let _ = std::fs::create_dir_all(&dir);

        let toml_path = dir.join("lichess.toml");
        std::fs::write(
            &toml_path,
            r#"
token_env = "MY_TOKEN"
accept_rated = true
accept_humans = true
speeds = ["blitz", "rapid"]
variants = ["standard"]
min_opponent_rating = 800
max_opponent_rating = 2000
max_concurrent_games = 1
"#,
        )
        .unwrap();
        let from_toml = LichessConfig::load_from_path(&toml_path).unwrap();
        assert_eq!(from_toml.token_env, "MY_TOKEN");
        assert!(from_toml.accept_rated);
        assert!(from_toml.accept_humans);
        assert_eq!(from_toml.speeds, vec!["blitz", "rapid"]);

        let json_path = dir.join("lichess.json");
        std::fs::write(
            &json_path,
            r#"{
  "accept_rated": false,
  "accept_humans": false,
  "speeds": ["rapid"]
}"#,
        )
        .unwrap();
        let from_json = LichessConfig::load_from_path(&json_path).unwrap();
        assert!(!from_json.accept_rated);
        assert!(!from_json.accept_humans);
        assert_eq!(from_json.speeds, vec!["rapid"]);
        // Unset fields keep serde defaults.
        assert_eq!(from_json.token_env, "LICHESS_TOKEN");
    }

    #[test]
    fn cli_overrides_win_over_file() {
        let mut cfg = LichessConfig {
            accept_rated: false,
            accept_humans: false,
            speeds: vec!["rapid".into()],
            ..Default::default()
        };
        cfg.apply_overrides(&ConfigOverrides {
            accept_rated: Some(true),
            accept_humans: Some(true),
            speeds: Some(vec!["blitz".into()]),
            token_env: Some("OTHER_TOKEN".into()),
            ..Default::default()
        });
        assert!(cfg.accept_rated);
        assert!(cfg.accept_humans);
        assert_eq!(cfg.speeds, vec!["blitz"]);
        assert_eq!(cfg.token_env, "OTHER_TOKEN");
    }

    #[test]
    fn clamp_forces_single_game_until_l2_07() {
        let mut cfg = LichessConfig {
            max_concurrent_games: 4,
            ..Default::default()
        };
        cfg.clamp();
        assert_eq!(cfg.max_concurrent_games, 1);
    }

    #[test]
    fn rejects_unknown_extension() {
        let err = LichessConfig::parse_str("{}", Path::new("lichess.yaml")).unwrap_err();
        assert!(err.to_string().contains("unsupported config extension"));
    }

    #[test]
    fn file_alone_drives_accept_filter() {
        // Acceptance for L2-04: config file alone (no CLI overrides) filters.
        let text = r#"
accept_rated = false
accept_humans = false
speeds = ["blitz"]
variants = ["standard"]
"#;
        let cfg = LichessConfig::parse_str(text, &PathBuf::from("ops.toml")).unwrap();
        assert_eq!(
            cfg.decide(&bot_challenge("blitz", "standard", false)),
            ChallengeDecision::Accept
        );
        assert_eq!(
            cfg.decide(&bot_challenge("rapid", "standard", false)),
            ChallengeDecision::Decline("timeControl")
        );
        assert_eq!(
            cfg.decide(&challenge("blitz", "standard", false)),
            ChallengeDecision::Decline("noBot")
        );
    }
}
