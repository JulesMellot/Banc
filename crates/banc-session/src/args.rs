//! Command-line options.

use std::path::PathBuf;

pub const HELP: &str = "\
banc-session: Banc session helper

Usage: banc-session [options]

Options:
  --log-dir <dir>  write the logs to this folder
  --stderr         also copy the logs to standard error
  --version        print the version
  --help           print this help
";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub log_dir: Option<PathBuf>,
    pub stderr: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
}

pub fn parse(mut parser: lexopt::Parser) -> Result<Command, lexopt::Error> {
    use lexopt::prelude::*;

    let mut options = Options::default();
    while let Some(arg) = parser.next()? {
        match arg {
            Long("log-dir") => options.log_dir = Some(parser.value()?.into()),
            Long("stderr") => options.stderr = true,
            Long("version") => return Ok(Command::Version),
            Long("help") => return Ok(Command::Help),
            _ => return Err(arg.unexpected()),
        }
    }
    Ok(Command::Run(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, lexopt::Error> {
        parse(lexopt::Parser::from_args(args))
    }

    #[test]
    fn no_option() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Run(Options::default()));
    }

    #[test]
    fn log_dir_and_stderr() {
        let command = parse_args(&["--log-dir", "/tmp/logs", "--stderr"]).unwrap();
        let expected = Options {
            log_dir: Some(PathBuf::from("/tmp/logs")),
            stderr: true,
        };
        assert_eq!(command, Command::Run(expected));
    }

    #[test]
    fn version_and_help() {
        assert_eq!(parse_args(&["--version"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn unknown_option_refused() {
        assert!(parse_args(&["--unknown"]).is_err());
        assert!(parse_args(&["argument"]).is_err());
        assert!(parse_args(&["--log-dir"]).is_err());
    }
}
