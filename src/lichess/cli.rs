//! CLI for the Lichess bot daemon: `openchess lichess run|account`.
//!
//! ```text
//! cargo run --features lichess -- lichess run --dry-run
//! ```

use super::client::{Client, NdjsonItem};
use super::events::StreamEvent;
use super::LichessError;
use serde::Deserialize;
use std::collections::HashSet;
use std::process::ExitCode;

const DEFAULT_TOKEN_ENV: &str = "LICHESS_TOKEN";

/// Run the lichess CLI with arguments after the `lichess` subcommand.
pub fn run(args: impl IntoIterator<Item = String>) -> ExitCode {
    let mut args: Vec<String> = args.into_iter().collect();
    if args.is_empty() || args.iter().any(|a| a == "-h" || a == "--help") {
        print_help();
        return if args.is_empty() {
            ExitCode::from(2)
        } else {
            ExitCode::SUCCESS
        };
    }

    let sub = args.remove(0);
    match sub.as_str() {
        "run" => match run_event_loop(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lichess error: {e}");
                ExitCode::FAILURE
            }
        },
        "account" => match show_account(&args) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("lichess error: {e}");
                match e {
                    LichessError::MissingToken(_) => ExitCode::from(2),
                    _ => ExitCode::FAILURE,
                }
            }
        },
        other => {
            eprintln!("unknown lichess subcommand: {other}");
            print_help();
            ExitCode::from(2)
        }
    }
}

fn print_help() {
    eprintln!(
        "usage: openchess lichess <run|account> [options]\n\n\
         run      Connect to /api/stream/event and log events (dry-run by default)\n\
         account  Print username and title from /api/account\n\n\
         run options:\n\
           --dry-run           Log events only, never POST (default)\n\
           --token-env VAR     Env var for API token (default: {DEFAULT_TOKEN_ENV})"
    );
}

#[derive(Default)]
struct RunOptions {
    dry_run: bool,
    token_env: String,
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, String> {
    let mut opts = RunOptions {
        dry_run: true,
        token_env: DEFAULT_TOKEN_ENV.into(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                opts.dry_run = true;
                i += 1;
            }
            "--token-env" => {
                let Some(var) = args.get(i + 1) else {
                    return Err("--token-env requires a variable name".into());
                };
                opts.token_env = var.clone();
                i += 2;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(opts)
}

fn parse_token_env(args: &[String]) -> Result<String, String> {
    let mut token_env = DEFAULT_TOKEN_ENV.to_string();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--token-env" => {
                let Some(var) = args.get(i + 1) else {
                    return Err("--token-env requires a variable name".into());
                };
                token_env = var.clone();
                i += 2;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(token_env)
}

fn run_event_loop(args: &[String]) -> Result<(), LichessError> {
    let opts = parse_run_options(args).map_err(|e| LichessError::Http(e))?;
    if !opts.dry_run {
        eprintln!("lichess: --play not implemented yet (P9-03); running dry-run");
    }

    let client = Client::from_env(&opts.token_env)?;
    eprintln!("lichess: connecting to event stream (dry-run)");

    let mut stream = client.open_ndjson_stream("/api/stream/event")?;
    let mut seen_game_starts = HashSet::new();
    let mut seen_challenges = HashSet::new();

    loop {
        match stream.read_item()? {
            None => {
                // TODO P9-07: exponential backoff reconnect
                eprintln!("lichess: event stream closed");
                return Err(LichessError::Http("event stream closed".into()));
            }
            Some(NdjsonItem::Keepalive) => {
                eprintln!("lichess keepalive");
            }
            Some(NdjsonItem::Event(event)) => {
                log_event(&event, &mut seen_game_starts, &mut seen_challenges)
            }
        }
    }
}

fn log_event(
    event: &StreamEvent,
    seen_game_starts: &mut HashSet<String>,
    seen_challenges: &mut HashSet<String>,
) {
    match event {
        StreamEvent::GameStart { game } => {
            let replay = !seen_game_starts.insert(game.game_id.clone());
            let tag = if replay { "replay" } else { "new" };
            eprintln!(
                "lichess event gameStart {tag} gameId={} color={} speed={} rated={} variant={} opponent={}",
                game.game_id,
                game.color,
                game.speed,
                game.rated,
                game.variant.key,
                game.opponent.username,
            );
        }
        StreamEvent::GameFinish { game } => {
            eprintln!(
                "lichess event gameFinish gameId={}",
                game.game_id
            );
        }
        StreamEvent::Challenge { challenge } => {
            let replay = !seen_challenges.insert(challenge.id.clone());
            let tag = if replay { "replay" } else { "new" };
            eprintln!(
                "lichess event challenge {tag} id={} speed={} rated={} challenger={} variant={} status={}",
                challenge.id,
                challenge.speed,
                challenge.rated,
                challenge.challenger.name,
                challenge.variant.key,
                challenge.status,
            );
        }
        StreamEvent::ChallengeCanceled { challenge } => {
            eprintln!(
                "lichess event challengeCanceled id={}",
                challenge.id
            );
        }
        StreamEvent::ChallengeDeclined { challenge } => {
            eprintln!(
                "lichess event challengeDeclined id={}",
                challenge.id
            );
        }
    }
}

#[derive(Debug, Deserialize)]
struct Account {
    username: String,
    #[serde(default)]
    title: Option<String>,
}

fn show_account(args: &[String]) -> Result<(), LichessError> {
    let token_env = parse_token_env(args).map_err(|e| LichessError::Http(e))?;
    let client = Client::from_env(&token_env)?;
    let account: Account = client.get_json("/api/account")?;
    let title = account.title.as_deref().unwrap_or("(none)");
    println!("username: {}", account.username);
    println!("title: {title}");
    Ok(())
}
