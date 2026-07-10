//! CLI for fetching chess.com PGNs: `openchess chesscom <url|username>`.
//!
//! ```text
//! cargo run -- chesscom <url|username> [--list] [--index N]
//! ```

use super::{
    fetch_latest_pgn, fetch_pgn_by_index, fetch_pgn_from_url, list_recent_games, looks_like_game_url,
};
use std::process::ExitCode;

/// Run the chess.com CLI with arguments after the `chesscom` subcommand.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut args: Vec<String> = args.into_iter().collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        eprintln!(
            "usage: openchess chesscom <game-url|username> [--list] [--index N]\n\n\
             Prints PGN to stdout. Use --list to print recent games from the\n\
             latest archive month (newest first). --index N selects the Nth\n\
             game (0 = newest) instead of the latest."
        );
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    let mut list = false;
    let mut index: Option<usize> = None;
    let mut positional: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" => {
                list = true;
                i += 1;
            }
            "--index" => {
                i += 1;
                let Some(raw) = args.get(i) else {
                    eprintln!("error: --index requires a number");
                    return ExitCode::from(2);
                };
                match raw.parse::<usize>() {
                    Ok(n) => index = Some(n),
                    Err(_) => {
                        eprintln!("error: invalid --index value: {raw}");
                        return ExitCode::from(2);
                    }
                }
                i += 1;
            }
            flag if flag.starts_with('-') => {
                eprintln!("error: unknown flag {flag}");
                return ExitCode::from(2);
            }
            _ => {
                if positional.is_some() {
                    eprintln!("error: unexpected argument {}", args[i]);
                    return ExitCode::from(2);
                }
                positional = Some(std::mem::take(&mut args[i]));
                i += 1;
            }
        }
    }

    let Some(target) = positional else {
        eprintln!("error: missing game URL or username");
        return ExitCode::from(2);
    };

    if list && index.is_some() {
        eprintln!("error: use either --list or --index, not both");
        return ExitCode::from(2);
    }

    let result = if looks_like_game_url(&target) {
        if list || index.is_some() {
            eprintln!("error: --list / --index only apply to usernames");
            return ExitCode::from(2);
        }
        fetch_pgn_from_url(&target).map(|pgn| {
            println!("{pgn}");
        })
    } else if list {
        list_recent_games(&target).map(|games| {
            if games.is_empty() {
                eprintln!("no games in latest archive month");
                return;
            }
            for (i, g) in games.iter().enumerate() {
                let wr = g.white_result.as_deref().unwrap_or("?");
                let br = g.black_result.as_deref().unwrap_or("?");
                eprintln!(
                    "{i}\t{}\t{} ({wr}) vs {} ({br})\tend_time={}",
                    g.url, g.white, g.black, g.end_time
                );
            }
        })
    } else if let Some(n) = index {
        fetch_pgn_by_index(&target, n).map(|pgn| {
            println!("{pgn}");
        })
    } else {
        fetch_latest_pgn(&target).map(|pgn| {
            println!("{pgn}");
        })
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
