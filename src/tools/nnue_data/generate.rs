//! Self-play / search-labeled training data generation.
//!
//! Plays short games from opening seeds, keeps quiet positions, labels each
//! with a shallow search score (white-relative CP) and the eventual WDL.

use std::fs;
use std::io::{self, Write};
use std::path::Path;

use crate::board::Board;
use crate::search;

use super::format::{
    is_mate_score, white_relative_result, white_relative_score, TrainingRecord,
};
use super::quiet::is_quiet_training_position;

/// Options for [`generate_dataset`].
#[derive(Clone, Debug)]
pub struct GenerateOptions {
    /// Opening FENs (one per game seed; cycled if `games` exceeds length).
    pub openings: Vec<String>,
    /// Number of games to play.
    pub games: usize,
    /// Search depth used to choose moves during self-play.
    pub play_depth: i32,
    /// Search depth used to label quiet positions.
    pub label_depth: i32,
    /// Skip sampling before this ply (half-move count from the opening).
    pub min_ply: u32,
    /// Hard stop for game length (plies from the opening).
    pub max_plies: u32,
    /// Sample every N plies once past `min_ply` (1 = every quiet position).
    pub sample_every: u32,
    /// Optional RNG seed; when set, a fraction of moves are random for diversity.
    pub seed: Option<u64>,
    /// Probability in `[0, 1]` of picking a random legal move instead of search.
    pub random_move_prob: f32,
}

impl Default for GenerateOptions {
    fn default() -> Self {
        Self {
            openings: vec![Board::startpos().to_fen()],
            games: 1,
            play_depth: 4,
            label_depth: 4,
            min_ply: 4,
            max_plies: 80,
            sample_every: 1,
            seed: None,
            random_move_prob: 0.0,
        }
    }
}

/// Tiny deterministic options for the Q2-01 fixture (fast on a laptop).
pub fn fixture_options(openings: Vec<String>) -> GenerateOptions {
    GenerateOptions {
        openings,
        games: 2,
        play_depth: 2,
        label_depth: 2,
        min_ply: 2,
        max_plies: 24,
        sample_every: 1,
        seed: Some(0x02_01_c0de),
        random_move_prob: 0.15,
    }
}

/// Load opening FENs from an EPD/FEN file (one position per non-empty line).
pub fn load_openings(path: &Path) -> io::Result<Vec<String>> {
    let text = fs::read_to_string(path)?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let fen = epd_to_fen(trimmed).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{}: {e}", path.display(), i + 1),
            )
        })?;
        // Validate early.
        Board::from_fen(&fen).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}:{}: invalid FEN: {e}", path.display(), i + 1),
            )
        })?;
        out.push(fen);
    }
    if out.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no openings in {}", path.display()),
        ));
    }
    Ok(out)
}

/// Convert a FEN or EPD line into a 6-field FEN string.
fn epd_to_fen(line: &str) -> Result<String, String> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    if fields.len() < 4 {
        return Err("need at least 4 FEN/EPD fields".into());
    }
    let hm = fields.get(4).filter(|t| t.chars().all(|c| c.is_ascii_digit()));
    let fm = fields.get(5).filter(|t| t.chars().all(|c| c.is_ascii_digit()));
    Ok(format!(
        "{} {} {} {} {} {}",
        fields[0],
        fields[1],
        fields[2],
        fields[3],
        hm.unwrap_or(&"0"),
        fm.unwrap_or(&"1"),
    ))
}

/// Generate training records by self-play.
pub fn generate_dataset(opts: &GenerateOptions) -> Result<Vec<TrainingRecord>, String> {
    if opts.openings.is_empty() {
        return Err("no openings provided".into());
    }
    if opts.games == 0 {
        return Err("--games must be > 0".into());
    }
    if opts.play_depth < 1 || opts.label_depth < 1 {
        return Err("depths must be >= 1".into());
    }
    if opts.sample_every == 0 {
        return Err("--sample-every must be >= 1".into());
    }

    let mut rng = XorShift64::new(opts.seed.unwrap_or(0xC0FFEE));
    let use_random = opts.seed.is_some() && opts.random_move_prob > 0.0;
    let mut records = Vec::new();

    for game_idx in 0..opts.games {
        let fen = &opts.openings[game_idx % opts.openings.len()];
        let mut board = Board::from_fen(fen).map_err(|e| format!("opening FEN: {e}"))?;
        let mut pending: Vec<(String, i32)> = Vec::new();
        let mut ply = 0u32;

        loop {
            let result = board.game_result();
            if result.is_over() {
                let wdl = white_relative_result(result)
                    .ok_or_else(|| "internal: finished game without WDL".to_string())?;
                for (fen, score_cp) in pending.drain(..) {
                    records.push(TrainingRecord {
                        fen,
                        score_cp,
                        result: wdl,
                    });
                }
                break;
            }
            if ply >= opts.max_plies {
                // Unfinished: treat as draw for fixture/pipeline stability.
                for (fen, score_cp) in pending.drain(..) {
                    records.push(TrainingRecord {
                        fen,
                        score_cp,
                        result: 0.5,
                    });
                }
                break;
            }

            if ply >= opts.min_ply
                && (ply - opts.min_ply).is_multiple_of(opts.sample_every)
                && is_quiet_training_position(&board)
            {
                let mut probe = board.clone();
                let labeled = search::go_depth(&mut probe, opts.label_depth);
                if !is_mate_score(labeled.score) {
                    let score_cp =
                        white_relative_score(board.side_to_move(), labeled.score);
                    pending.push((board.to_fen(), score_cp));
                }
            }

            let mv = if use_random && rng.next_f32() < opts.random_move_prob {
                let legal = board.legal_moves();
                if legal.is_empty() {
                    break;
                }
                legal[rng.next_usize(legal.len())]
            } else {
                let mut probe = board.clone();
                let played = search::go_depth(&mut probe, opts.play_depth);
                if played.best_move.is_none() {
                    break;
                }
                played.best_move
            };

            board.make(mv);
            ply += 1;
        }
    }

    Ok(records)
}

/// Write records as Bullet text (one line each).
pub fn write_bullet_text(path: &Path, records: &[TrainingRecord]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let mut file = fs::File::create(path)?;
    writeln!(
        file,
        "# OpenChess NNUE training data (Bullet text): FEN | score_cp | result"
    )?;
    writeln!(
        file,
        "# score = white-relative CP; result = white-relative WDL (1.0/0.5/0.0)"
    )?;
    for rec in records {
        writeln!(file, "{}", rec.to_bullet_line())?;
    }
    Ok(())
}

/// Tiny xorshift64 for reproducible random-move spice.
struct XorShift64(u64);

impl XorShift64 {
    fn new(seed: u64) -> Self {
        Self(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    fn next_f32(&mut self) -> f32 {
        (self.next_u64() as f32) / (u64::MAX as f32)
    }

    fn next_usize(&mut self, limit: usize) -> usize {
        debug_assert!(limit > 0);
        (self.next_u64() as usize) % limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lookup;
    use super::super::format::validate_bullet_text;

    #[test]
    fn fixture_generates_valid_bullet_text() {
        lookup::initialize();
        // Keep this lighter than `nnue-data fixture` so `cargo test` stays snappy;
        // the shell script is the full Q2-01 acceptance gate.
        let mut opts = fixture_options(vec![Board::startpos().to_fen()]);
        opts.games = 1;
        opts.max_plies = 16;
        opts.play_depth = 1;
        opts.label_depth = 1;
        let records = generate_dataset(&opts).unwrap();
        assert!(
            !records.is_empty(),
            "fixture should produce at least one quiet sample"
        );
        let mut text = String::new();
        for r in &records {
            text.push_str(&r.to_bullet_line());
            text.push('\n');
        }
        let n = validate_bullet_text(&text).unwrap();
        assert_eq!(n, records.len());
        // Scores should be white-relative finite CPs (not mate band).
        for r in &records {
            assert!(r.score_cp.abs() < 30_000, "score {}", r.score_cp);
            assert!(r.result == 0.0 || r.result == 0.5 || r.result == 1.0);
            assert!(Board::from_fen(&r.fen).is_ok());
        }
    }

    #[test]
    fn epd_line_parses() {
        let fen = epd_to_fen(
            "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3 c0 \"foo\";",
        )
        .unwrap();
        assert!(fen.starts_with("rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq e3"));
        Board::from_fen(&fen).unwrap();
    }
}
