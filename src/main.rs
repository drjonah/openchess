//! OpenChess binary: `openchess tui` for the terminal UI; default is UCI.

use std::process::ExitCode;

fn main() -> ExitCode {
    openchess::lookup::initialize();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("tui") => {
            if let Err(err) = openchess::tui::run() {
                eprintln!("tui error: {err}");
                return ExitCode::FAILURE;
            }
            ExitCode::SUCCESS
        }
        #[cfg(feature = "chesscom")]
        Some("chesscom") => openchess::chesscom::cli::run(args),
        #[cfg(feature = "lichess")]
        Some("lichess") => openchess::lichess::cli::run(args),
        Some("uci") | None => {
            openchess::uci::message_loop();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn print_usage() {
    #[cfg(all(feature = "chesscom", feature = "lichess"))]
    eprintln!("usage: openchess [tui|uci|chesscom|lichess]");
    #[cfg(all(feature = "chesscom", not(feature = "lichess")))]
    eprintln!("usage: openchess [tui|uci|chesscom]");
    #[cfg(all(not(feature = "chesscom"), feature = "lichess"))]
    eprintln!("usage: openchess [tui|uci|lichess]");
    #[cfg(not(any(feature = "chesscom", feature = "lichess")))]
    eprintln!("usage: openchess [tui|uci]");
}
