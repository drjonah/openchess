//! `openchess arena` command-line front.
//!
//! - `arena run …` — headless batch (P11-02).
//! - `arena watch …` / bare `arena` — live text monitor (a lightweight
//!   stand-in for the ratatui inspector, P11-04).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use crate::config::SideStrength;

use super::batch::{self, BatchOptions};
use super::profile::ProfileSet;
use super::runner::{Arena, ArenaConfig, DEFAULT_HASH_MB, DEFAULT_PLY_LIMIT};
use super::slot::SlotStatus;

const DEFAULT_GAMES: usize = 1;
const DEFAULT_DEPTH: u32 = 6;

struct ParsedOptions {
    arena: ArenaConfig,
    pgn_dir: Option<PathBuf>,
    jsonl: bool,
}

/// Dispatch `arena` subcommands.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let args: Vec<String> = args.into_iter().collect();

    let (sub, rest): (&str, &[String]) = match args.split_first() {
        Some((first, _)) if matches!(first.as_str(), "-h" | "--help" | "help") => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some((first, rest)) if !first.starts_with('-') => (first.as_str(), rest),
        _ => ("watch", args.as_slice()),
    };

    match sub {
        "run" => run_batch(rest),
        "watch" => run_watch(rest),
        other => {
            eprintln!("unknown arena subcommand: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn run_batch(args: &[String]) -> ExitCode {
    let opts = match parse_options(args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("arena run: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let jsonl = opts.jsonl;
    let total = opts.arena.games.max(1);
    let options = BatchOptions {
        arena: opts.arena,
        pgn_dir: opts.pgn_dir,
    };

    let mut finished = 0usize;
    let mut on_event = |event: &super::slot::SlotEvent| {
        if matches!(event, super::slot::SlotEvent::Finish { .. }) {
            finished += 1;
            eprintln!("arena: finished {finished}/{total}");
        }
        if jsonl {
            println!("{}", super::export::event_jsonl(event));
        }
    };

    match batch::run(&options, &mut on_event) {
        Ok(summary) => {
            let line = format!(
                "games={} white_wins={} black_wins={} draws={} unfinished={} avg_plies={:.1}",
                summary.games,
                summary.white_wins,
                summary.black_wins,
                summary.draws,
                summary.unfinished,
                summary.avg_plies(),
            );
            // Keep the human summary on stderr when streaming JSONL on stdout.
            if jsonl {
                eprintln!("{line}");
            } else {
                println!("{line}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("arena run failed: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_watch(args: &[String]) -> ExitCode {
    let opts = match parse_options(args) {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("arena watch: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let mut arena = Arena::from_config(&opts.arena);
    let mut out = std::io::stdout();
    loop {
        let _ = arena.tick();
        render_monitor(&mut out, &arena);
        if arena.all_finished() || (arena.thinking_count() == 0 && !arena.any_runnable()) {
            break;
        }
        std::thread::sleep(Duration::from_millis(250));
    }
    // Final frame.
    render_monitor(&mut out, &arena);
    println!();
    ExitCode::SUCCESS
}

fn render_monitor(out: &mut std::io::Stdout, arena: &Arena) {
    let mut buf = String::new();
    // Clear screen + home cursor.
    buf.push_str("\x1b[2J\x1b[H");
    buf.push_str("OpenChess arena — live monitor (Ctrl-C to quit)\n\n");
    buf.push_str(" id  status     ply  last     eval    result\n");
    buf.push_str(" --- ---------- ---- -------- ------- --------\n");
    for snap in arena.snapshots() {
        let status = match snap.status {
            SlotStatus::Idle => "idle",
            SlotStatus::Thinking => "thinking",
            SlotStatus::Paused => "paused",
            SlotStatus::Finished => "finished",
        };
        let last = snap.last_move.clone().unwrap_or_else(|| "-".into());
        let eval = snap
            .eval_white_cp
            .map(|cp| format!("{:+.2}", cp as f64 / 100.0))
            .unwrap_or_else(|| "-".into());
        let result = if snap.status == SlotStatus::Finished {
            snap.outcome.result_tag()
        } else {
            "-"
        };
        buf.push_str(&format!(
            " {:>3} {:<10} {:>4} {:<8} {:>7} {:>8}\n",
            snap.id, status, snap.ply, last, eval, result
        ));
    }
    let _ = out.write_all(buf.as_bytes());
    let _ = out.flush();
}

fn parse_options(args: &[String]) -> Result<ParsedOptions, String> {
    let mut games = DEFAULT_GAMES;
    let mut shared = SideStrength {
        depth: DEFAULT_DEPTH,
        movetime_ms: 0,
    };
    let mut white: Option<SideStrength> = None;
    let mut black: Option<SideStrength> = None;
    let mut concurrency = 1usize;
    let mut hash_mb = DEFAULT_HASH_MB;
    let mut ply_limit = DEFAULT_PLY_LIMIT;
    let mut pgn_dir: Option<PathBuf> = None;
    let mut jsonl = false;
    let mut profiles = ProfileSet::default();
    let mut alternate_colors = true;

    let mut i = 0;
    while i < args.len() {
        let flag = args[i].clone();
        match flag.as_str() {
            "--games" => games = parse_usize(&take_value(args, &mut i, &flag)?, &flag)?,
            "--depth" => {
                shared.depth = parse_u32(&take_value(args, &mut i, &flag)?, &flag)?.clamp(1, 64)
            }
            "--movetime" => {
                shared.movetime_ms = parse_u64(&take_value(args, &mut i, &flag)?, &flag)?
            }
            "--white-depth" => {
                let v = parse_u32(&take_value(args, &mut i, &flag)?, &flag)?.clamp(1, 64);
                white.get_or_insert_with(|| shared.clone()).depth = v;
            }
            "--white-movetime" => {
                let v = parse_u64(&take_value(args, &mut i, &flag)?, &flag)?;
                white.get_or_insert_with(|| shared.clone()).movetime_ms = v;
            }
            "--black-depth" => {
                let v = parse_u32(&take_value(args, &mut i, &flag)?, &flag)?.clamp(1, 64);
                black.get_or_insert_with(|| shared.clone()).depth = v;
            }
            "--black-movetime" => {
                let v = parse_u64(&take_value(args, &mut i, &flag)?, &flag)?;
                black.get_or_insert_with(|| shared.clone()).movetime_ms = v;
            }
            "--concurrency" => {
                concurrency = parse_usize(&take_value(args, &mut i, &flag)?, &flag)?.max(1)
            }
            "--hash" => hash_mb = parse_usize(&take_value(args, &mut i, &flag)?, &flag)?.max(1),
            "--max-plies" => {
                ply_limit = parse_usize(&take_value(args, &mut i, &flag)?, &flag)?.max(1)
            }
            "--pgn-dir" => pgn_dir = Some(PathBuf::from(take_value(args, &mut i, &flag)?)),
            "--jsonl" => jsonl = true,
            "--no-alternate-colors" => alternate_colors = false,
            "--profile" => {
                let path = take_value(args, &mut i, &flag)?;
                profiles = ProfileSet::load(&path).map_err(|e| format!("--profile {path}: {e}"))?;
            }
            other => return Err(format!("unknown flag: {other}")),
        }
        i += 1;
    }

    // If only one side was overridden, the other keeps the shared strength.
    let white = white.unwrap_or_else(|| shared.clone());
    let black = black.unwrap_or_else(|| shared.clone());

    let arena = ArenaConfig {
        games: games.max(1),
        white,
        black,
        ply_limit,
        concurrency,
        hash_mb,
        profiles: profiles.profiles,
        alternate_colors,
    };

    Ok(ParsedOptions {
        arena,
        pgn_dir,
        jsonl,
    })
}

fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn parse_usize(s: &str, flag: &str) -> Result<usize, String> {
    s.parse().map_err(|_| format!("{flag}: not a number: {s}"))
}

fn parse_u32(s: &str, flag: &str) -> Result<u32, String> {
    s.parse().map_err(|_| format!("{flag}: not a number: {s}"))
}

fn parse_u64(s: &str, flag: &str) -> Result<u64, String> {
    s.parse().map_err(|_| format!("{flag}: not a number: {s}"))
}

fn print_usage() {
    eprintln!(
        "usage: openchess arena <run|watch> [options]\n\n\
         Bulk local Bot-vs-Bot battles.\n\n\
         subcommands:\n\
         \x20 run      headless batch; prints a W/D/L summary (exit 0)\n\
         \x20 watch    live text monitor of all games (default)\n\n\
         options:\n\
         \x20 --games N               number of concurrent games (default 1)\n\
         \x20 --depth D               search depth for both sides (default 6)\n\
         \x20 --movetime MS           movetime per move (0 = depth-only)\n\
         \x20 --white-depth D         White-only depth override\n\
         \x20 --white-movetime MS     White-only movetime override\n\
         \x20 --black-depth D         Black-only depth override\n\
         \x20 --black-movetime MS     Black-only movetime override\n\
         \x20 --concurrency K         max searches in flight (default 1 = serial)\n\
         \x20 --hash MB               per-search TT size (default 8)\n\
         \x20 --max-plies N           adjudicate a draw after N plies (default 400)\n\
         \x20 --pgn-dir DIR           write one PGN per finished game\n\
         \x20 --jsonl                 emit a JSONL move/finish event stream\n\
         \x20 --profile FILE          JSON strength profiles assigned across slots\n\
         \x20 --no-alternate-colors   keep White/Black strengths fixed (no color swap)"
    );
}
