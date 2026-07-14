//! Small shared helpers for the hand-rolled subcommand CLIs (`arena`, `lichess`).
//!
//! These front-ends parse `Vec<String>` argument lists by hand rather than
//! pulling in a CLI crate; the helpers here remove the copy-pasted numeric
//! parsing and value-taking that each one previously carried.

use std::str::FromStr;

/// Parse a flag's string value into `T`, with a uniform error message.
///
/// ```
/// # use openchess::cli_util::parse_value;
/// assert_eq!(parse_value::<u32>("42", "--depth").unwrap(), 42);
/// assert!(parse_value::<u32>("x", "--depth").is_err());
/// ```
pub fn parse_value<T: FromStr>(value: &str, flag: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("{flag}: invalid value '{value}'"))
}

/// Take the argument following a flag, advancing the cursor past it.
///
/// `i` points at the flag on entry; on success it is advanced to the consumed
/// value so the caller's `i += 1` step lands on the next flag.
pub fn take_value(args: &[String], i: &mut usize, flag: &str) -> Result<String, String> {
    *i += 1;
    args.get(*i)
        .cloned()
        .ok_or_else(|| format!("{flag} needs a value"))
}
