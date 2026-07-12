//! User config (`~/.config/openchess/config.json`).
//!
//! Common fields (`bot`, `eval`, `analysis`, `tui`) are edited from the TUI settings overlay.
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
const DEFAULT_EVAL_DEPTH: u32 = 6;
const DEFAULT_EVAL_MOVETIME_MS: u64 = 250;
const DEFAULT_ANALYSIS_DEPTH: u32 = 10;
const DEFAULT_ANALYSIS_MOVETIME_MS: u64 = 500;
const MIN_DEPTH: u32 = 1;
const MAX_DEPTH: u32 = 64;
const MIN_MOVETIME_MS: u64 = 50;
const MAX_MOVETIME_MS: u64 = 60_000;
/// Extra margin over Move Overhead required for a bot/live movetime (P7-05).
///
/// Keeps the hard budget (`movetime − overhead`) safely above zero so a live
/// game (Lichess / TUI bot) never searches with a ~0 ms hard limit.
const MIN_MOVETIME_MARGIN_MS: u64 = 50;
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
    pub eval: EvalConfig,
    pub analysis: AnalysisConfig,
    pub tui: TuiConfig,
    pub engine: EngineConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bot: BotConfig::default(),
            eval: EvalConfig::default(),
            analysis: AnalysisConfig::default(),
            tui: TuiConfig::default(),
            engine: EngineConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct BotConfig {
    /// Shared strength for Player vs Bot and Analyze / manual search.
    pub depth: u32,
    pub movetime_ms: u64,
    /// Bot vs Bot: White's search limits.
    pub white: SideStrength,
    /// Bot vs Bot: Black's search limits.
    pub black: SideStrength,
}

impl Default for BotConfig {
    fn default() -> Self {
        Self {
            depth: DEFAULT_DEPTH,
            movetime_ms: DEFAULT_MOVETIME_MS,
            white: SideStrength::default(),
            black: SideStrength::default(),
        }
    }
}

/// Per-side depth / movetime (used for Bot vs Bot mismatched strength).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct SideStrength {
    pub depth: u32,
    pub movetime_ms: u64,
}

impl Default for SideStrength {
    fn default() -> Self {
        Self {
            depth: DEFAULT_DEPTH,
            movetime_ms: DEFAULT_MOVETIME_MS,
        }
    }
}

impl SideStrength {
    fn to_go_limits(&self) -> GoLimits {
        GoLimits {
            depth: Some(self.depth),
            movetime: Some(Duration::from_millis(self.movetime_ms)),
        }
    }

    fn clamp_with_floor(&mut self, floor_ms: u64) {
        self.depth = self.depth.clamp(MIN_DEPTH, MAX_DEPTH);
        self.movetime_ms = self
            .movetime_ms
            .clamp(MIN_MOVETIME_MS, MAX_MOVETIME_MS)
            .max(floor_ms);
    }

    fn adjust_depth(&mut self, delta: i32) {
        let next = (self.depth as i32 + delta).clamp(MIN_DEPTH as i32, MAX_DEPTH as i32);
        self.depth = next as u32;
    }

    fn adjust_movetime(&mut self, delta_ms: i64) {
        let next = (self.movetime_ms as i64 + delta_ms)
            .clamp(MIN_MOVETIME_MS as i64, MAX_MOVETIME_MS as i64);
        self.movetime_ms = next as u64;
    }
}

/// Limits for the live eval-bar search (separate from bot play).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct EvalConfig {
    pub depth: u32,
    pub movetime_ms: u64,
}

impl Default for EvalConfig {
    fn default() -> Self {
        Self {
            depth: DEFAULT_EVAL_DEPTH,
            movetime_ms: DEFAULT_EVAL_MOVETIME_MS,
        }
    }
}

/// Limits for post-game analysis of imported games.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    pub depth: u32,
    pub movetime_ms: u64,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            depth: DEFAULT_ANALYSIS_DEPTH,
            movetime_ms: DEFAULT_ANALYSIS_MOVETIME_MS,
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

/// Safe lower bound for a live/bot movetime given a Move Overhead (P7-05).
///
/// Ensures `movetime ≥ overhead + margin` so the hard budget stays positive.
pub fn movetime_floor_ms(move_overhead_ms: u64) -> u64 {
    (move_overhead_ms + MIN_MOVETIME_MARGIN_MS).max(MIN_MOVETIME_MS)
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

    /// Limits for the side about to move: BvB uses per-color strength, otherwise shared bot.
    pub fn play_go_limits(&self, mode: Option<PlayMode>, side_to_move: Color) -> GoLimits {
        match mode {
            Some(PlayMode::BotVsBot) => self.side_go_limits(side_to_move),
            _ => self.go_limits(),
        }
    }

    pub fn side_go_limits(&self, side: Color) -> GoLimits {
        match side {
            Color::White => self.bot.white.to_go_limits(),
            Color::Black => self.bot.black.to_go_limits(),
        }
    }

    pub fn eval_go_limits(&self) -> GoLimits {
        GoLimits {
            depth: Some(self.eval.depth),
            movetime: Some(Duration::from_millis(self.eval.movetime_ms)),
        }
    }

    pub fn analysis_go_limits(&self) -> GoLimits {
        GoLimits {
            depth: Some(self.analysis.depth),
            movetime: Some(Duration::from_millis(self.analysis.movetime_ms)),
        }
    }

    pub fn clamp(&mut self) {
        // Clamp overhead first so the movetime floor is computed from a sane value.
        self.engine.move_overhead_ms = self.engine.move_overhead_ms.clamp(0, MAX_MOVETIME_MS);
        let floor = movetime_floor_ms(self.engine.move_overhead_ms);
        self.bot.depth = self.bot.depth.clamp(MIN_DEPTH, MAX_DEPTH);
        self.bot.movetime_ms = self
            .bot
            .movetime_ms
            .clamp(MIN_MOVETIME_MS, MAX_MOVETIME_MS)
            .max(floor);
        self.bot.white.clamp_with_floor(floor);
        self.bot.black.clamp_with_floor(floor);
        self.eval.depth = self.eval.depth.clamp(MIN_DEPTH, MAX_DEPTH);
        self.eval.movetime_ms = self
            .eval
            .movetime_ms
            .clamp(MIN_MOVETIME_MS, MAX_MOVETIME_MS);
        self.analysis.depth = self.analysis.depth.clamp(MIN_DEPTH, MAX_DEPTH);
        self.analysis.movetime_ms = self
            .analysis
            .movetime_ms
            .clamp(MIN_MOVETIME_MS, MAX_MOVETIME_MS);
        self.engine.hash_mb = self.engine.hash_mb.clamp(MIN_HASH_MB, MAX_HASH_MB);
        self.engine.threads = self.engine.threads.clamp(MIN_THREADS, MAX_THREADS);
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

    pub fn adjust_white_depth(&mut self, delta: i32) {
        self.bot.white.adjust_depth(delta);
    }

    pub fn adjust_white_movetime(&mut self, delta_ms: i64) {
        self.bot.white.adjust_movetime(delta_ms);
    }

    pub fn adjust_black_depth(&mut self, delta: i32) {
        self.bot.black.adjust_depth(delta);
    }

    pub fn adjust_black_movetime(&mut self, delta_ms: i64) {
        self.bot.black.adjust_movetime(delta_ms);
    }

    pub fn adjust_eval_depth(&mut self, delta: i32) {
        let next = (self.eval.depth as i32 + delta).clamp(MIN_DEPTH as i32, MAX_DEPTH as i32);
        self.eval.depth = next as u32;
    }

    pub fn adjust_eval_movetime(&mut self, delta_ms: i64) {
        let next = (self.eval.movetime_ms as i64 + delta_ms)
            .clamp(MIN_MOVETIME_MS as i64, MAX_MOVETIME_MS as i64);
        self.eval.movetime_ms = next as u64;
    }

    pub fn adjust_analysis_depth(&mut self, delta: i32) {
        let next = (self.analysis.depth as i32 + delta).clamp(MIN_DEPTH as i32, MAX_DEPTH as i32);
        self.analysis.depth = next as u32;
    }

    pub fn adjust_analysis_movetime(&mut self, delta_ms: i64) {
        let next = (self.analysis.movetime_ms as i64 + delta_ms)
            .clamp(MIN_MOVETIME_MS as i64, MAX_MOVETIME_MS as i64);
        self.analysis.movetime_ms = next as u64;
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
        assert_eq!(back.bot.white.depth, DEFAULT_DEPTH);
        assert_eq!(back.bot.black.movetime_ms, DEFAULT_MOVETIME_MS);
        assert_eq!(back.eval.depth, DEFAULT_EVAL_DEPTH);
        assert_eq!(back.eval.movetime_ms, DEFAULT_EVAL_MOVETIME_MS);
        assert_eq!(back.analysis.depth, DEFAULT_ANALYSIS_DEPTH);
        assert_eq!(back.analysis.movetime_ms, DEFAULT_ANALYSIS_MOVETIME_MS);
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
        assert_eq!(cfg.bot.white.depth, DEFAULT_DEPTH);
        assert_eq!(cfg.bot.black.depth, DEFAULT_DEPTH);
        assert_eq!(cfg.eval.depth, DEFAULT_EVAL_DEPTH);
        assert_eq!(cfg.eval.movetime_ms, DEFAULT_EVAL_MOVETIME_MS);
        assert_eq!(cfg.analysis.depth, DEFAULT_ANALYSIS_DEPTH);
        assert_eq!(cfg.analysis.movetime_ms, DEFAULT_ANALYSIS_MOVETIME_MS);
        assert_eq!(cfg.tui.default_mode, DefaultPlayMode::PlayerVsPlayer);
    }

    #[test]
    fn eval_section_roundtrips() {
        let json = r#"{
            "bot": { "depth": 10, "movetime_ms": 5000 },
            "eval": { "depth": 4, "movetime_ms": 2500 }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.eval.depth, 4);
        assert_eq!(cfg.eval.movetime_ms, 2500);
        let limits = cfg.eval_go_limits();
        assert_eq!(limits.depth, Some(4));
        assert_eq!(limits.movetime, Some(Duration::from_millis(2500)));
    }

    #[test]
    fn analysis_section_roundtrips() {
        let json = r#"{
            "analysis": { "depth": 12, "movetime_ms": 800 }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.analysis.depth, 12);
        assert_eq!(cfg.analysis.movetime_ms, 800);
        let limits = cfg.analysis_go_limits();
        assert_eq!(limits.depth, Some(12));
        assert_eq!(limits.movetime, Some(Duration::from_millis(800)));
    }

    #[test]
    fn bot_vs_bot_uses_per_side_strength() {
        let json = r#"{
            "bot": {
                "depth": 8,
                "movetime_ms": 450,
                "white": { "depth": 12, "movetime_ms": 5000 },
                "black": { "depth": 2, "movetime_ms": 100 }
            }
        }"#;
        let cfg: Config = serde_json::from_str(json).unwrap();
        let white = cfg.play_go_limits(Some(PlayMode::BotVsBot), Color::White);
        let black = cfg.play_go_limits(Some(PlayMode::BotVsBot), Color::Black);
        let pvb = cfg.play_go_limits(
            Some(PlayMode::PlayerVsBot {
                human: Color::White,
            }),
            Color::Black,
        );
        assert_eq!(white.depth, Some(12));
        assert_eq!(white.movetime, Some(Duration::from_millis(5000)));
        assert_eq!(black.depth, Some(2));
        assert_eq!(black.movetime, Some(Duration::from_millis(100)));
        // PvB still uses shared bot limits, not the side strengths.
        assert_eq!(pvb.depth, Some(8));
        assert_eq!(pvb.movetime, Some(Duration::from_millis(450)));
    }

    #[test]
    fn clamp_limits() {
        let mut cfg = Config::default();
        cfg.bot.depth = 0;
        cfg.bot.movetime_ms = 1;
        cfg.bot.white.depth = 0;
        cfg.bot.black.movetime_ms = 1;
        cfg.eval.depth = 0;
        cfg.eval.movetime_ms = 1;
        cfg.analysis.depth = 0;
        cfg.analysis.movetime_ms = 1;
        cfg.engine.elo = 50;
        cfg.clamp();
        let floor = movetime_floor_ms(cfg.engine.move_overhead_ms);
        assert_eq!(cfg.bot.depth, MIN_DEPTH);
        // Bot / live movetimes clamp up to the overhead-aware floor (P7-05).
        assert_eq!(cfg.bot.movetime_ms, floor);
        assert_eq!(cfg.bot.white.depth, MIN_DEPTH);
        assert_eq!(cfg.bot.black.movetime_ms, floor);
        // Eval / analysis use the plain minimum (not clock-critical).
        assert_eq!(cfg.eval.depth, MIN_DEPTH);
        assert_eq!(cfg.eval.movetime_ms, MIN_MOVETIME_MS);
        assert_eq!(cfg.analysis.depth, MIN_DEPTH);
        assert_eq!(cfg.analysis.movetime_ms, MIN_MOVETIME_MS);
        assert_eq!(cfg.engine.elo, MIN_ELO);
    }

    #[test]
    fn movetime_floor_keeps_hard_budget_positive() {
        // P7-05: with the default 50 ms overhead, a 50 ms bot movetime would
        // leave a 0 ms hard budget. The floor must lift it clear of overhead.
        let mut cfg = Config::default();
        cfg.engine.move_overhead_ms = 50;
        cfg.bot.movetime_ms = 50;
        cfg.bot.white.movetime_ms = 50;
        cfg.bot.black.movetime_ms = 50;
        cfg.clamp();
        let floor = movetime_floor_ms(50);
        assert!(floor > 50, "floor must exceed overhead");
        assert_eq!(cfg.bot.movetime_ms, floor);
        assert_eq!(cfg.bot.white.movetime_ms, floor);
        assert_eq!(cfg.bot.black.movetime_ms, floor);
        // Hard budget = movetime − overhead is strictly positive.
        assert!(cfg.bot.movetime_ms - cfg.engine.move_overhead_ms > 0);
    }

    #[test]
    fn movetime_floor_scales_with_overhead() {
        let mut cfg = Config::default();
        cfg.engine.move_overhead_ms = 300;
        cfg.bot.movetime_ms = 100;
        cfg.clamp();
        assert_eq!(cfg.bot.movetime_ms, movetime_floor_ms(300));
        assert!(cfg.bot.movetime_ms > cfg.engine.move_overhead_ms);
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
