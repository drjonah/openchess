//! `openchess nnue-data` — training-data pipeline CLI (Q2-01).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli_util::{parse_value, take_value};
use crate::lookup;

use super::format::validate_bullet_text;
use super::generate::{
    fixture_options, generate_dataset, load_openings, write_bullet_text, GenerateOptions,
};

/// Dispatch `nnue-data` subcommands.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    lookup::initialize();
    let _ = crate::eval::Network::embedded_shared();

    let args: Vec<String> = args.into_iter().collect();
    let (sub, rest) = match args.split_first() {
        Some((first, _)) if matches!(first.as_str(), "-h" | "--help" | "help") => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Some((first, rest)) if !first.starts_with('-') => (first.as_str(), rest),
        _ => {
            print_usage();
            return ExitCode::from(2);
        }
    };

    match sub {
        "generate" => cmd_generate(rest),
        "validate" => cmd_validate(rest),
        "fixture" => cmd_fixture(rest),
        other => {
            eprintln!("unknown nnue-data subcommand: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn cmd_generate(args: &[String]) -> ExitCode {
    let parsed = match parse_generate(args) {
        Ok(p) => p,
        Err(e) if e == "help" => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("nnue-data generate: {e}");
            print_usage();
            return ExitCode::from(2);
        }
    };

    let openings = match &parsed.openings_path {
        Some(path) => match load_openings(path) {
            Ok(o) => o,
            Err(e) => {
                eprintln!("nnue-data generate: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => vec![crate::board::Board::startpos().to_fen()],
    };

    let opts = GenerateOptions {
        openings,
        games: parsed.games,
        play_depth: parsed.play_depth,
        label_depth: parsed.label_depth,
        min_ply: parsed.min_ply,
        max_plies: parsed.max_plies,
        sample_every: parsed.sample_every,
        seed: parsed.seed,
        random_move_prob: parsed.random_move_prob,
    };

    eprintln!(
        "nnue-data: generating games={} play_depth={} label_depth={} …",
        opts.games, opts.play_depth, opts.label_depth
    );

    let records = match generate_dataset(&opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nnue-data generate failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = write_bullet_text(&parsed.output, &records) {
        eprintln!("nnue-data: write {}: {e}", parsed.output.display());
        return ExitCode::FAILURE;
    }

    println!(
        "wrote {} records → {}",
        records.len(),
        parsed.output.display()
    );
    ExitCode::SUCCESS
}

fn cmd_validate(args: &[String]) -> ExitCode {
    let path = match args.first() {
        Some(p) if !p.starts_with('-') => Path::new(p),
        _ => {
            eprintln!("usage: openchess nnue-data validate <file.txt>");
            return ExitCode::from(2);
        }
    };
    let text = match fs::read_to_string(path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("nnue-data validate: {e}");
            return ExitCode::FAILURE;
        }
    };
    match validate_bullet_text(&text) {
        Ok(n) => {
            println!("ok: {n} records in {}", path.display());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nnue-data validate: {e}");
            ExitCode::FAILURE
        }
    }
}

fn cmd_fixture(args: &[String]) -> ExitCode {
    let mut out = PathBuf::from("tools/nnue-data/out/fixture_bullet.txt");
    let mut openings = PathBuf::from("tools/nnue-data/fixtures/openings.epd");
    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => {
                eprintln!(
                    "usage: openchess nnue-data fixture [--openings PATH] [--output PATH]"
                );
                return ExitCode::SUCCESS;
            }
            "--output" => match take_value(args, &mut i, "--output") {
                Ok(v) => out = PathBuf::from(v),
                Err(e) => {
                    eprintln!("nnue-data fixture: {e}");
                    return ExitCode::from(2);
                }
            },
            "--openings" => match take_value(args, &mut i, "--openings") {
                Ok(v) => openings = PathBuf::from(v),
                Err(e) => {
                    eprintln!("nnue-data fixture: {e}");
                    return ExitCode::from(2);
                }
            },
            other => {
                eprintln!("nnue-data fixture: unknown flag {other}");
                return ExitCode::from(2);
            }
        }
        i += 1;
    }

    let loaded = match load_openings(&openings) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("nnue-data fixture: {e}");
            return ExitCode::FAILURE;
        }
    };
    let opts = fixture_options(loaded);
    eprintln!(
        "nnue-data fixture: openings={} games={} depths={}/{}",
        opts.openings.len(),
        opts.games,
        opts.play_depth,
        opts.label_depth
    );

    let records = match generate_dataset(&opts) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nnue-data fixture failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    if records.is_empty() {
        eprintln!("nnue-data fixture: produced zero records");
        return ExitCode::FAILURE;
    }
    if let Err(e) = write_bullet_text(&out, &records) {
        eprintln!("nnue-data fixture: write {}: {e}", out.display());
        return ExitCode::FAILURE;
    }

    let text = fs::read_to_string(&out).expect("just wrote");
    match validate_bullet_text(&text) {
        Ok(n) => {
            println!(
                "fixture ok: {n} Bullet records → {}",
                out.display()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("nnue-data fixture validate failed: {e}");
            ExitCode::FAILURE
        }
    }
}

struct ParsedGenerate {
    openings_path: Option<PathBuf>,
    output: PathBuf,
    games: usize,
    play_depth: i32,
    label_depth: i32,
    min_ply: u32,
    max_plies: u32,
    sample_every: u32,
    seed: Option<u64>,
    random_move_prob: f32,
}

fn parse_generate(args: &[String]) -> Result<ParsedGenerate, String> {
    let mut openings_path = None;
    let mut output = PathBuf::from("tools/nnue-data/out/data.txt");
    let mut games = 4usize;
    let mut play_depth = 4i32;
    let mut label_depth = 4i32;
    let mut min_ply = 4u32;
    let mut max_plies = 80u32;
    let mut sample_every = 1u32;
    let mut seed = None;
    let mut random_move_prob = 0.0f32;

    let mut i = 0usize;
    while i < args.len() {
        match args[i].as_str() {
            "-h" | "--help" | "help" => return Err("help".into()),
            "--openings" => {
                openings_path = Some(PathBuf::from(take_value(args, &mut i, "--openings")?));
            }
            "--output" | "-o" => {
                output = PathBuf::from(take_value(args, &mut i, "--output")?);
            }
            "--games" => {
                games = parse_value(&take_value(args, &mut i, "--games")?, "--games")?;
            }
            "--play-depth" => {
                play_depth =
                    parse_value(&take_value(args, &mut i, "--play-depth")?, "--play-depth")?;
            }
            "--label-depth" => {
                label_depth =
                    parse_value(&take_value(args, &mut i, "--label-depth")?, "--label-depth")?;
            }
            "--min-ply" => {
                min_ply = parse_value(&take_value(args, &mut i, "--min-ply")?, "--min-ply")?;
            }
            "--max-plies" => {
                max_plies =
                    parse_value(&take_value(args, &mut i, "--max-plies")?, "--max-plies")?;
            }
            "--sample-every" => {
                sample_every = parse_value(
                    &take_value(args, &mut i, "--sample-every")?,
                    "--sample-every",
                )?;
            }
            "--seed" => {
                seed = Some(parse_value(&take_value(args, &mut i, "--seed")?, "--seed")?);
            }
            "--random-move-prob" => {
                random_move_prob = parse_value(
                    &take_value(args, &mut i, "--random-move-prob")?,
                    "--random-move-prob",
                )?;
            }
            other => return Err(format!("unknown flag {other}")),
        }
        i += 1;
    }

    Ok(ParsedGenerate {
        openings_path,
        output,
        games,
        play_depth,
        label_depth,
        min_ply,
        max_plies,
        sample_every,
        seed,
        random_move_prob,
    })
}

fn print_usage() {
    eprintln!(
        "\
usage:
  openchess nnue-data generate [options]
  openchess nnue-data validate <file.txt>
  openchess nnue-data fixture [--openings PATH] [--output PATH]

generate options:
  --openings PATH       EPD/FEN seeds (default: startpos)
  --output / -o PATH    Bullet text output (default: tools/nnue-data/out/data.txt)
  --games N             self-play games (default: 4)
  --play-depth D        move-choice search depth (default: 4)
  --label-depth D       quiet-position label depth (default: 4)
  --min-ply N           first ply to sample (default: 4)
  --max-plies N         stop game after N plies (default: 80)
  --sample-every N      sample stride (default: 1)
  --seed U64            enable seeded random-move spice
  --random-move-prob P  random move probability when --seed set (default: 0)

See research/nnue-training.md for the Q2-01 pipeline."
    );
}
