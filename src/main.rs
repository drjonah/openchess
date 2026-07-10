//! OpenChess binary: `openchess tui` for the terminal UI; default is UCI.

fn main() {
    openchess::lookup::initialize();

    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some("tui") => {
            if let Err(err) = openchess::tui::run() {
                eprintln!("tui error: {err}");
                std::process::exit(1);
            }
        }
        Some("uci") | None => {
            openchess::uci::message_loop();
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            eprintln!("usage: openchess [tui|uci]");
            std::process::exit(2);
        }
    }
}
