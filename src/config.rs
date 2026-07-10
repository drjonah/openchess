//! User config (`~/.config/openchess/config.json`).
//!
//! Common fields (`bot`, `tui`) are edited from the TUI settings overlay.
//! Advanced `engine` fields are file-only until real search/UCI lands.

use crate::tui::session::{GoLimits, PlayMode};
use crate::types::Color;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

const CONFIG_DIR_NAME: &str = "openchess";
const CONFIG_FILE_NAME: &str = "config.json";

const DEFAULT_DEPTH: u32 = 8;
const DEFAULT_MOVETIME_MS: u64 = 450;
const MIN_DEPTH: u32 = 1;
const MAX_DEPTH: u32 = 64;
const MIN_MOVETIME_MS: u64 = 50;
const MAX_MOVETIME_MS: u64 = 60_000;
const MIN_HASH_MB: u32 = 1;
const MAX_HASH_MB: u32 = 65_536;
const MIN_THREADS: u32 = 1;
const MAX_THREADS: u32 = 512;
const MIN_ELO: u32 = 800;
const MAX_ELO: u32 = 3000;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub bot: BotConfig,
    pub tui: TuiConfig,
    pub engine: EngineConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bot: BotConfig::default(),
            tui: TuiConfig::default(),
            engine: EngineConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    pub depth: u32,
    pub movetime_ms: u64,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            depth: DEFAULT_DEPTH,
            movetime_ms: DEFAULT_MOVETIME_MS,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct TuiConfig {
    pub default_mode: DefaultPlayMode,
    pub flip_board: bool,
    pub show_eval_bar: bool,
}

impl Default for TuiConfig {
    fn default() -> Self {
        Self {
            default_mode: DefaultPlayMode::PlayerVsPlayer,
            flip_board: false,
            show_eval_bar: false,
        }
    }
}

/// Persisted play-mode default (serde-friendly).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DefaultPlayMode {
    #[default]
    PlayerVsPlayer,
    PlayerVsBotWhite,
    PlayerVsBotBlack,
    BotVsBot,
    Analyze,
}

impl DefaultPlayMode {
    pub fn to_play_mode(self) -> PlayMode {
        match self {
            DefaultPlayMode::PlayerVsPlayer => PlayMode::PlayerVsPlayer,
            DefaultPlayMode::PlayerVsBotWhite => PlayMode::PlayerVsBot {
                human: Color::White,
            },
            DefaultPlayMode::PlayerVsBotBlack => PlayMode::PlayerVsBot {
                human: Color::Black,
            },
            DefaultPlayMode::BotVsBot => PlayMode::BotVsBot,
            DefaultPlayMode::Analyze => PlayMode::Analyze,
        }
    }

    pub fn from_play_mode(mode: PlayMode) -> Self {
        match mode {
            PlayMode::PlayerVsPlayer => DefaultPlayMode::PlayerVsPlayer,
            PlayMode::PlayerVsBot {
                human: Color::White,
            } => DefaultPlayMode::PlayerVsBotWhite,
            PlayMode::PlayerVsBot {
                human: Color::Black,
            } => DefaultPlayMode::PlayerVsBotBlack,
            PlayMode::BotVsBot => DefaultPlayMode::BotVsBot,
            PlayMode::Analyze => DefaultPlayMode::Analyze,
        }
    }

    pub fn title(self) -> &'static str {
        self.to_play_mode().title()
    }

    pub fn next(self) -> Self {
        match self {
            DefaultPlayMode::PlayerVsPlayer => DefaultPlayMode::PlayerVsBotWhite,
            DefaultPlayMode::PlayerVsBotWhite => DefaultPlayMode::PlayerVsBotBlack,
            DefaultPlayMode::PlayerVsBotBlack => DefaultPlayMode::BotVsBot,
            DefaultPlayMode::BotVsBot => DefaultPlayMode::Analyze,
            DefaultPlayMode::Analyze => DefaultPlayMode::PlayerVsPlayer,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            DefaultPlayMode::PlayerVsPlayer => DefaultPlayMode::Analyze,
            DefaultPlayMode::PlayerVsBotWhite => DefaultPlayMode::PlayerVsPlayer,
            DefaultPlayMode::PlayerVsBotBlack => DefaultPlayMode::PlayerVsBotWhite,
            DefaultPlayMode::BotVsBot => DefaultPlayMode::PlayerVsBotBlack,
            DefaultPlayMode::Analyze => DefaultPlayMode::BotVsBot,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineConfig {
    pub hash_mb: u32,
    pub threads: u32,
    pub move_overhead_ms: u64,
    pub limit_strength: bool,
    pub elo: u32,
}

impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            hash_mb: 16,
            threads: 1,
            move_overhead_ms: 50,
            limit_strength: false,
            elo: 1400,
        }
    }
}

impl Config {
    /// Resolved config file path (`$XDG_CONFIG_HOME/openchess/config.json` or `~/.config/...`).
    pub fn path() -> PathBuf {
        config_dir().join(CONFIG_FILE_NAME)
    }

    /// Load from disk, or create defaults if missing/invalid.
    pub fn load() -> (Self, Option<String>) {
        let path = Self::path();
        if !path.exists() {
            let cfg = Self::default();
            if let Err(e) = cfg.save() {
                return (
                    cfg,
                    Some(format!("could not create config {}: {e}", path.display())),
                );
            }
            return (cfg, None);
        }

        match fs::read_to_string(&path) {
            Ok(text) => match serde_json::from_str::<Config>(&text) {
                Ok(mut cfg) => {
                    cfg.clamp();
                    (cfg, None)
                }
                Err(e) => (
                    Self::default(),
                    Some(format!(
                        "invalid config {}, using defaults: {e}",
                        path.display()
                    )),
                ),
            },
            Err(e) => (
                Self::default(),
                Some(format!(
                    "could not read config {}, using defaults: {e}",
                    path.display()
                )),
            ),
        }
    }

    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut cfg = self.clone();
        cfg.clamp();
        let json = serde_json::to_string_pretty(&cfg)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        fs::write(path, format!("{json}\n"))
    }

    pub fn go_limits(&self) -> GoLimits {
        GoLimits {
            depth: Some(self.bot.depth),
            movetime: Some(Duration::from_millis(self.bot.movetime_ms)),
        }
    }

    pub fn clamp(&mut self) {
        self.bot.depth = self.bot.depth.clamp(MIN_DEPTH, MAX_DEPTH);
        self.bot.movetime_ms = self.bot.movetime_ms.clamp(MIN_MOVETIME_MS, MAX_MOVETIME_MS);
        self.engine.hash_mb = self.engine.hash_mb.clamp(MIN_HASH_MB, MAX_HASH_MB);
        self.engine.threads = self.engine.threads.clamp(MIN_THREADS, MAX_THREADS);
        self.engine.move_overhead_ms = self
            .engine
            .move_overhead_ms
            .clamp(0, MAX_MOVETIME_MS);
        self.engine.elo = self.engine.elo.clamp(MIN_ELO, MAX_ELO);
    }

    pub fn adjust_depth(&mut self, delta: i32) {
        let next = (self.bot.depth as i32 + delta).clamp(MIN_DEPTH as i32, MAX_DEPTH as i32);
        self.bot.depth = next as u32;
    }

    pub fn adjust_movetime(&mut self, delta_ms: i64) {
        let next = (self.bot.movetime_ms as i64 + delta_ms)
            .clamp(MIN_MOVETIME_MS as i64, MAX_MOVETIME_MS as i64);
        self.bot.movetime_ms = next as u64;
    }
}

fn config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join(CONFIG_DIR_NAME);
        }
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".config")
        .join(CONFIG_DIR_NAME)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_roundtrips_json() {
        let cfg = Config::default();
        let json = serde_json::to_string_pretty(&cfg).unwrap();
        let back: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(back.bot.depth, DEFAULT_DEPTH);
        assert_eq!(back.bot.movetime_ms, DEFAULT_MOVETIME_MS);
        assert_eq!(back.engine.hash_mb, 16);
    }

    #[test]
    fn unknown_keys_ignored() {
        let json = r#"{
            "bot": { "depth": 12, "movetime_ms": 300, "extra": true },
            "future": 1
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.bot.depth, 12);
        assert_eq!(cfg.bot.movetime_ms, 300);
        assert_eq!(cfg.tui.default_mode, DefaultPlayMode::PlayerVsPlayer);
    }

    #[test]
    fn clamp_limits() {
        let mut cfg = Config::default();
        cfg.bot.depth = 0;
        cfg.bot.movetime_ms = 1;
        cfg.engine.elo = 50;
        cfg.clamp();
        assert_eq!(cfg.bot.depth, MIN_DEPTH);
        assert_eq!(cfg.bot.movetime_ms, MIN_MOVETIME_MS);
        assert_eq!(cfg.engine.elo, MIN_ELO);
    }

    #[test]
    fn play_mode_cycle() {
        let mut m = DefaultPlayMode::PlayerVsPlayer;
        for _ in 0..5 {
            m = m.next();
        }
        assert_eq!(m, DefaultPlayMode::PlayerVsPlayer);
    }
}
