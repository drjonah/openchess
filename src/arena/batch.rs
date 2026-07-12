//! Headless batch runner for `openchess arena run` (P11-02).

use std::fs;
use std::io;
use std::path::PathBuf;

use super::export::slot_pgn;
use super::runner::{Arena, ArenaConfig};
use super::slot::{Outcome, SlotEvent};

/// Options for a headless batch run.
#[derive(Clone, Debug, Default)]
pub struct BatchOptions {
    pub arena: ArenaConfig,
    /// Directory to write one PGN per finished game (created if missing).
    pub pgn_dir: Option<PathBuf>,
}

/// Aggregate result of a batch run.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchSummary {
    pub games: usize,
    pub white_wins: usize,
    pub black_wins: usize,
    pub draws: usize,
    pub unfinished: usize,
    pub total_plies: usize,
}

impl BatchSummary {
    pub fn avg_plies(&self) -> f64 {
        if self.games == 0 {
            0.0
        } else {
            self.total_plies as f64 / self.games as f64
        }
    }
}

/// Run a full batch to completion, forwarding each event to `on_event`.
///
/// Writes PGN files when `pgn_dir` is set. Returns the aggregate summary.
pub fn run(
    options: &BatchOptions,
    on_event: &mut dyn FnMut(&SlotEvent),
) -> io::Result<BatchSummary> {
    if let Some(dir) = &options.pgn_dir {
        fs::create_dir_all(dir)?;
    }

    let mut arena = Arena::from_config(&options.arena);
    arena.run_to_completion(on_event);

    let mut summary = BatchSummary {
        games: arena.len(),
        ..BatchSummary::default()
    };

    for slot in arena.slots() {
        summary.total_plies += slot.ply_count();
        match slot.outcome() {
            Outcome::WhiteWin => summary.white_wins += 1,
            Outcome::BlackWin => summary.black_wins += 1,
            Outcome::Draw => summary.draws += 1,
            Outcome::Unfinished => summary.unfinished += 1,
        }

        if let Some(dir) = &options.pgn_dir {
            let path = dir.join(format!("game-{:03}.pgn", slot.id));
            fs::write(path, slot_pgn(slot))?;
        }
    }

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::arena::runner::ArenaConfig;
    use crate::config::SideStrength;

    fn strength(depth: u32) -> SideStrength {
        SideStrength {
            depth,
            movetime_ms: 0,
        }
    }

    #[test]
    fn batch_completes_and_summary_totals_match() {
        crate::lookup::initialize();
        let options = BatchOptions {
            arena: ArenaConfig {
                games: 6,
                white: strength(1),
                black: strength(1),
                ply_limit: 60,
                ..ArenaConfig::default()
            },
            pgn_dir: None,
        };
        let mut moves = 0usize;
        let mut finishes = 0usize;
        let summary = run(&options, &mut |e| match e {
            SlotEvent::Move { .. } => moves += 1,
            SlotEvent::Finish { .. } => finishes += 1,
        })
        .unwrap();

        assert_eq!(summary.games, 6);
        assert_eq!(
            summary.white_wins + summary.black_wins + summary.draws + summary.unfinished,
            6
        );
        assert_eq!(finishes, 6);
        assert!(moves >= 6);
        assert!(summary.avg_plies() > 0.0);
    }

    #[test]
    fn batch_writes_pgn_files() {
        crate::lookup::initialize();
        let dir = std::env::temp_dir().join(format!("openchess-arena-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let options = BatchOptions {
            arena: ArenaConfig {
                games: 3,
                white: strength(1),
                black: strength(1),
                ply_limit: 30,
                ..ArenaConfig::default()
            },
            pgn_dir: Some(dir.clone()),
        };
        let summary = run(&options, &mut |_| {}).unwrap();
        assert_eq!(summary.games, 3);
        for id in 0..3 {
            let path = dir.join(format!("game-{id:03}.pgn"));
            assert!(path.exists(), "missing {path:?}");
            let text = fs::read_to_string(&path).unwrap();
            assert!(text.contains("[Event \"OpenChess Arena\"]"));
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
