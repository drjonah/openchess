//! CLI for the Lichess bot daemon: `openchess lichess run|account|challenge`.
//!
//! ```text
//! cargo run --features lichess -- lichess run --dry-run
//! cargo run --features lichess -- lichess run --play
//! cargo run --features lichess -- lichess challenge <username>
//! ```

use super::challenge::{self, OutboundChallenge};
use super::client::{Client, NdjsonItem};
use super::config::LichessConfig;
use super::events::StreamEvent;
use super::game::{self, PlayOptions};
use super::{pgn, LichessError};
use serde::Deserialize;
use std::collections::HashSet;
use std::process::ExitCode;
use std::time::Duration;

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
    let result = match sub.as_str() {
        "run" => run_event_loop(&args),
        "account" => show_account(&args),
        "challenge" => create_challenge(&args),
        other => {
            eprintln!("unknown lichess subcommand: {other}");
            print_help();
            return ExitCode::from(2);
        }
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e @ LichessError::MissingToken(_)) => {
            eprintln!("lichess error: {e}");
            ExitCode::from(2)
        }
        Err(e) => {
            eprintln!("lichess error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn print_help() {
    eprintln!(
        "usage: openchess lichess <run|account|challenge> [options]\n\n\
         run               Connect to /api/stream/event (dry-run by default)\n\
         account           Print id/username/title from /api/account\n\
         challenge <user>  Create an outbound challenge\n\n\
         run options:\n\
         \x20 --dry-run           Log events only, never POST (default)\n\
         \x20 --play              Accept challenges and play games\n\
         \x20 --accept-rated      Accept rated challenges (default: casual only)\n\
         \x20 --movetime MS       Fixed think time per move (default: use clock)\n\
         \x20 --hash MB           Transposition table size (default: 16)\n\
         \x20 --no-own-book       Disable opening book (search only)\n\
         \x20 --book-file PATH    Polyglot `.bin` book file\n\
         \x20 --repertoire        Enable deep curated repertoire\n\
         \x20 --book-style S      mixed|solid|aggressive (with --repertoire)\n\
         \x20 --token-env VAR     Env var for API token (default: {DEFAULT_TOKEN_ENV})\n\n\
         challenge options:\n\
         \x20 --clock-limit S     Base clock seconds (default: 300)\n\
         \x20 --clock-increment S Increment seconds (default: 3)\n\
         \x20 --rated             Rated game (default: casual)\n\
         \x20 --color C           white|black|random (default: random)\n\
         \x20 --token-env VAR     Env var for API token (default: {DEFAULT_TOKEN_ENV})"
    );
}

struct RunOptions {
    play: bool,
    token_env: String,
    accept_rated: bool,
    movetime_ms: Option<u64>,
    hash_mb: u32,
    own_book: bool,
    book_file: Option<String>,
    repertoire: bool,
    book_style: String,
}

impl Default for RunOptions {
    fn default() -> Self {
        Self {
            play: false,
            token_env: DEFAULT_TOKEN_ENV.into(),
            accept_rated: false,
            movetime_ms: None,
            hash_mb: 16,
            own_book: true,
            book_file: None,
            repertoire: false,
            book_style: "mixed".into(),
        }
    }
}

fn parse_run_options(args: &[String]) -> Result<RunOptions, String> {
    let mut opts = RunOptions::default();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--dry-run" => {
                opts.play = false;
                i += 1;
            }
            "--play" => {
                opts.play = true;
                i += 1;
            }
            "--accept-rated" => {
                opts.accept_rated = true;
                i += 1;
            }
            "--movetime" => {
                opts.movetime_ms = Some(parse_num(args.get(i + 1), "--movetime")?);
                i += 2;
            }
            "--hash" => {
                opts.hash_mb = parse_num(args.get(i + 1), "--hash")? as u32;
                i += 2;
            }
            "--no-own-book" => {
                opts.own_book = false;
                i += 1;
            }
            "--book-file" => {
                opts.book_file = Some(take_value(args.get(i + 1), "--book-file")?);
                i += 2;
            }
            "--repertoire" => {
                opts.repertoire = true;
                i += 1;
            }
            "--book-style" => {
                opts.book_style = take_value(args.get(i + 1), "--book-style")?;
                i += 2;
            }
            "--token-env" => {
                opts.token_env = take_value(args.get(i + 1), "--token-env")?;
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
                token_env = take_value(args.get(i + 1), "--token-env")?;
                i += 2;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(token_env)
}

fn take_value(v: Option<&String>, flag: &str) -> Result<String, String> {
    v.cloned().ok_or_else(|| format!("{flag} requires a value"))
}

fn parse_num(v: Option<&String>, flag: &str) -> Result<u64, String> {
    crate::cli_util::parse_value(&take_value(v, flag)?, flag)
}

fn run_event_loop(args: &[String]) -> Result<(), LichessError> {
    let opts = parse_run_options(args).map_err(LichessError::Http)?;
    let client = Client::from_env(&opts.token_env)?;

    let config = LichessConfig {
        accept_rated: opts.accept_rated,
        ..Default::default()
    };
    let mut book_cfg = crate::config::Config::load().0.book;
    book_cfg.enabled = opts.own_book;
    if let Some(path) = &opts.book_file {
        book_cfg.file = Some(std::path::PathBuf::from(path));
    }
    if opts.repertoire {
        book_cfg.repertoire = true;
        book_cfg.style = opts.book_style.clone();
    }
    book_cfg.clamp();
    let mut play_options = PlayOptions {
        hash_mb: opts.hash_mb.max(1),
        fixed_movetime: opts.movetime_ms.map(Duration::from_millis),
        book: crate::book::Book::from_config(&book_cfg),
        ..Default::default()
    };
    // P7-05: keep a positive hard budget (movetime − overhead) on the bot path.
    if let Some(mt) = play_options.fixed_movetime {
        let floor = Duration::from_millis(crate::config::movetime_floor_ms(
            play_options.move_overhead.as_millis() as u64,
        ));
        play_options.fixed_movetime = Some(mt.max(floor));
    }

    // Our own account id is needed to determine our color and to ignore our
    // own outbound challenges.
    let account: Account = client.get_json("/api/account")?;
    let my_id = account.id.clone().unwrap_or_else(|| account.username.clone());

    if opts.play {
        eprintln!("lichess: play mode as '{my_id}' (rated={})", opts.accept_rated);
    } else {
        eprintln!("lichess: dry-run (log only; use --play to accept and play)");
    }

    let mut seen_game_starts = HashSet::new();
    let mut seen_challenges = HashSet::new();
    let mut attempt: u32 = 0;

    loop {
        match client.open_ndjson_stream::<StreamEvent>("/api/stream/event") {
            Ok(mut stream) => {
                attempt = 0;
                eprintln!("lichess: connected to event stream");
                loop {
                    match stream.read_item() {
                        Ok(None) => {
                            eprintln!("lichess: event stream closed");
                            break;
                        }
                        Ok(Some(NdjsonItem::Keepalive)) => {}
                        Ok(Some(NdjsonItem::Event(event))) => {
                            handle_event(
                                &client,
                                &config,
                                &play_options,
                                &my_id,
                                opts.play,
                                &event,
                                &mut seen_game_starts,
                                &mut seen_challenges,
                            );
                        }
                        Err(LichessError::RateLimited) => {
                            eprintln!("lichess: rate limited; sleeping 60s");
                            std::thread::sleep(pgn::RATE_LIMIT_SLEEP);
                        }
                        Err(e) => {
                            eprintln!("lichess: stream error: {e}");
                            break;
                        }
                    }
                }
            }
            Err(LichessError::RateLimited) => {
                eprintln!("lichess: rate limited on connect; sleeping 60s");
                std::thread::sleep(pgn::RATE_LIMIT_SLEEP);
                continue;
            }
            Err(e) => {
                eprintln!("lichess: connect failed: {e}");
            }
        }

        let delay = pgn::backoff_delay(attempt);
        attempt = attempt.saturating_add(1);
        eprintln!("lichess: reconnecting in {:?}", delay);
        std::thread::sleep(delay);
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_event(
    client: &Client,
    config: &LichessConfig,
    play_options: &PlayOptions,
    my_id: &str,
    play: bool,
    event: &StreamEvent,
    seen_game_starts: &mut HashSet<String>,
    seen_challenges: &mut HashSet<String>,
) {
    match event {
        StreamEvent::Challenge { challenge } => {
            let replay = !seen_challenges.insert(challenge.id.clone());
            let from_us = challenge.challenger.is_us(my_id);
            eprintln!(
                "lichess event challenge {} id={} speed={} rated={} challenger={} variant={}",
                if replay { "replay" } else { "new" },
                challenge.id,
                challenge.speed,
                challenge.rated,
                challenge.challenger.name,
                challenge.variant.key,
            );
            // P9-07 / LICHESS §6: stream reconnects replay open challenges — do
            // not accept/decline twice for the same id.
            if play && !from_us && !replay {
                match challenge::handle_incoming(client, config, challenge) {
                    Ok(true) => eprintln!("lichess: accepted challenge {}", challenge.id),
                    Ok(false) => eprintln!("lichess: declined challenge {}", challenge.id),
                    Err(e) => eprintln!("lichess: challenge {} error: {e}", challenge.id),
                }
            }
        }
        StreamEvent::GameStart { game } => {
            let replay = !seen_game_starts.insert(game.game_id.clone());
            eprintln!(
                "lichess event gameStart {} gameId={} color={} speed={} rated={} opponent={}",
                if replay { "replay" } else { "new" },
                game.game_id,
                game.color,
                game.speed,
                game.rated,
                game.opponent.username,
            );
            // Replayed gameStart means an in-flight game after reconnect; play
            // again so the per-game stream can resume (LICHESS §11.4).
            if play {
                eprintln!("lichess: playing game {}", game.game_id);
                match game::play_game(client, &game.game_id, my_id, play_options.clone()) {
                    Ok(()) => {
                        eprintln!("lichess: game {} finished", game.game_id);
                        match pgn::export_game(client, &game.game_id) {
                            Ok(path) => eprintln!("lichess: saved PGN {}", path.display()),
                            Err(e) => eprintln!("lichess: PGN export failed: {e}"),
                        }
                    }
                    Err(e) => {
                        eprintln!("lichess: game {} error: {e}", game.game_id);
                        // Allow a later event-stream replay to resume this game.
                        seen_game_starts.remove(&game.game_id);
                    }
                }
            }
        }
        StreamEvent::GameFinish { game } => {
            eprintln!("lichess event gameFinish gameId={}", game.game_id);
        }
        StreamEvent::ChallengeCanceled { challenge } => {
            eprintln!("lichess event challengeCanceled id={}", challenge.id);
        }
        StreamEvent::ChallengeDeclined { challenge } => {
            eprintln!("lichess event challengeDeclined id={}", challenge.id);
        }
    }
}

#[derive(Debug, Deserialize)]
struct Account {
    #[serde(default)]
    id: Option<String>,
    username: String,
    #[serde(default)]
    title: Option<String>,
}

fn show_account(args: &[String]) -> Result<(), LichessError> {
    let token_env = parse_token_env(args).map_err(LichessError::Http)?;
    let client = Client::from_env(&token_env)?;
    let account: Account = client.get_json("/api/account")?;
    let title = account.title.as_deref().unwrap_or("(none)");
    if let Some(id) = &account.id {
        println!("id: {id}");
    }
    println!("username: {}", account.username);
    println!("title: {title}");
    if title != "BOT" {
        eprintln!(
            "note: this account is not a BOT — run the one-time upgrade before playing:\n  \
             curl -d '' https://lichess.org/api/bot/account/upgrade -H \"Authorization: Bearer $TOKEN\""
        );
    }
    Ok(())
}

struct ChallengeOptions {
    username: String,
    token_env: String,
    clock_limit_secs: u32,
    clock_increment_secs: u32,
    rated: bool,
    color: String,
}

fn parse_challenge_options(args: &[String]) -> Result<ChallengeOptions, String> {
    let mut username: Option<String> = None;
    let mut opts = ChallengeOptions {
        username: String::new(),
        token_env: DEFAULT_TOKEN_ENV.into(),
        clock_limit_secs: 300,
        clock_increment_secs: 3,
        rated: false,
        color: "random".into(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--clock-limit" => {
                opts.clock_limit_secs = parse_num(args.get(i + 1), "--clock-limit")? as u32;
                i += 2;
            }
            "--clock-increment" => {
                opts.clock_increment_secs = parse_num(args.get(i + 1), "--clock-increment")? as u32;
                i += 2;
            }
            "--rated" => {
                opts.rated = true;
                i += 1;
            }
            "--color" => {
                opts.color = take_value(args.get(i + 1), "--color")?;
                i += 2;
            }
            "--token-env" => {
                opts.token_env = take_value(args.get(i + 1), "--token-env")?;
                i += 2;
            }
            flag if flag.starts_with('-') => return Err(format!("unknown flag {flag}")),
            other => {
                if username.is_some() {
                    return Err(format!("unexpected argument {other}"));
                }
                username = Some(other.to_string());
                i += 1;
            }
        }
    }
    opts.username = username.ok_or("challenge requires a <username>")?;
    Ok(opts)
}

fn create_challenge(args: &[String]) -> Result<(), LichessError> {
    let opts = parse_challenge_options(args).map_err(LichessError::Http)?;
    let client = Client::from_env(&opts.token_env)?;
    let challenge = OutboundChallenge {
        username: opts.username.clone(),
        clock_limit_secs: opts.clock_limit_secs,
        clock_increment_secs: opts.clock_increment_secs,
        rated: opts.rated,
        color: opts.color,
        variant: "standard".into(),
    };
    eprintln!(
        "lichess: challenging {} ({}+{}, rated={})",
        opts.username, opts.clock_limit_secs, opts.clock_increment_secs, opts.rated
    );
    challenge.send(&client)?;
    eprintln!("lichess: challenge sent (accept within ~20s on lichess.org)");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_defaults_to_dry_run() {
        let opts = parse_run_options(&[]).unwrap();
        assert!(!opts.play);
        assert_eq!(opts.token_env, DEFAULT_TOKEN_ENV);
    }

    #[test]
    fn run_parses_play_and_movetime() {
        let opts = parse_run_options(&[
            "--play".into(),
            "--movetime".into(),
            "500".into(),
            "--hash".into(),
            "32".into(),
            "--accept-rated".into(),
        ])
        .unwrap();
        assert!(opts.play);
        assert_eq!(opts.movetime_ms, Some(500));
        assert_eq!(opts.hash_mb, 32);
        assert!(opts.accept_rated);
    }

    #[test]
    fn fixed_movetime_is_clamped_to_overhead_floor() {
        // Mirrors the P7-05 clamp applied in run_event_loop when --movetime is set.
        let overhead = crate::time::DEFAULT_MOVE_OVERHEAD;
        let floor = Duration::from_millis(crate::config::movetime_floor_ms(
            overhead.as_millis() as u64,
        ));
        let too_small = Duration::from_millis(50);
        assert!(too_small < floor);
        assert_eq!(too_small.max(floor), floor);
    }

    #[test]
    fn run_rejects_unknown_flag() {
        assert!(parse_run_options(&["--nope".into()]).is_err());
    }

    #[test]
    fn challenge_requires_username() {
        assert!(parse_challenge_options(&[]).is_err());
        let opts = parse_challenge_options(&[
            "someBot".into(),
            "--clock-limit".into(),
            "180".into(),
            "--rated".into(),
        ])
        .unwrap();
        assert_eq!(opts.username, "someBot");
        assert_eq!(opts.clock_limit_secs, 180);
        assert!(opts.rated);
    }

    #[test]
    fn token_env_flag_parsed() {
        let env = parse_token_env(&["--token-env".into(), "MY_TOKEN".into()]).unwrap();
        assert_eq!(env, "MY_TOKEN");
    }
}
