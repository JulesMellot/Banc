//! Command-line options.

use std::ffi::OsString;
use std::path::PathBuf;

pub const HELP: &str = "\
banc-agent: Banc system service

Usage:
  banc-agent [options]                     run the agent
  banc-agent enroll <link> [--name <name>] join a team with a banc://join link
  banc-agent enroll --server <url> --key <key> [--name <name>]
  banc-agent status                        show the agent's state
  banc-agent service install|uninstall     register or remove the system service
  banc-agent update-apply <file>           install a downloaded update (started by the agent)

Options:
  --state-dir <dir>  data folder (default: the system one; secrets go there too)
  --socket <path>    local endpoint of the agent
  --log-dir <dir>    write the logs to this folder
  --stderr           also copy the logs to standard error
  --service          started by the service manager
  --version          print the version
  --help             print this help
";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub log_dir: Option<PathBuf>,
    pub state_dir: Option<PathBuf>,
    pub socket: Option<PathBuf>,
    pub stderr: bool,
    pub service: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Invite {
    Link(String),
    Key { server: String, key: String },
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Enroll {
        invite: Invite,
        name: Option<String>,
        socket: Option<PathBuf>,
    },
    Status {
        socket: Option<PathBuf>,
    },
    InstallService,
    UninstallService,
    ApplyUpdate(PathBuf),
    Help,
    Version,
}

fn text(value: OsString) -> Result<String, lexopt::Error> {
    value.into_string().map_err(lexopt::Error::NonUnicodeValue)
}

pub fn parse(mut parser: lexopt::Parser) -> Result<Command, lexopt::Error> {
    use lexopt::prelude::*;

    let mut options = Options::default();
    let mut subcommand: Option<String> = None;
    let mut positional: Vec<String> = Vec::new();
    let mut name = None;
    let mut server = None;
    let mut key = None;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("log-dir") => options.log_dir = Some(parser.value()?.into()),
            Long("state-dir") => options.state_dir = Some(parser.value()?.into()),
            Long("socket") => options.socket = Some(parser.value()?.into()),
            Long("stderr") => options.stderr = true,
            Long("service") => options.service = true,
            Long("name") => name = Some(text(parser.value()?)?),
            Long("server") => server = Some(text(parser.value()?)?),
            Long("key") => key = Some(text(parser.value()?)?),
            Long("version") => return Ok(Command::Version),
            Long("help") => return Ok(Command::Help),
            Value(value) if subcommand.is_none() => subcommand = Some(text(value)?),
            Value(value) => positional.push(text(value)?),
            _ => return Err(arg.unexpected()),
        }
    }
    let socket = options.socket.clone();
    match (subcommand.as_deref(), positional.as_slice()) {
        (None, []) => {
            if name.is_some() || server.is_some() || key.is_some() {
                return Err("--name, --server and --key go with `enroll`".into());
            }
            Ok(Command::Run(options))
        }
        (Some("enroll"), [link]) if server.is_none() && key.is_none() => Ok(Command::Enroll {
            invite: Invite::Link(link.clone()),
            name,
            socket,
        }),
        (Some("enroll"), []) => match (server, key) {
            (Some(server), Some(key)) => Ok(Command::Enroll {
                invite: Invite::Key { server, key },
                name,
                socket,
            }),
            _ => Err("enroll needs a link, or --server and --key".into()),
        },
        (Some("status"), []) => Ok(Command::Status { socket }),
        (Some("service"), [action]) if action == "install" => Ok(Command::InstallService),
        (Some("service"), [action]) if action == "uninstall" => Ok(Command::UninstallService),
        (Some(banc_agent::update::APPLY_COMMAND), [file]) => {
            Ok(Command::ApplyUpdate(PathBuf::from(file)))
        }
        (Some(other), _) => Err(format!("unknown command or arguments: {other}").into()),
        (None, _) => Err("unexpected arguments".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, lexopt::Error> {
        parse(lexopt::Parser::from_args(args))
    }

    #[test]
    fn run_options() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Run(Options::default()));
        let command = parse_args(&[
            "--log-dir",
            "/tmp/logs",
            "--state-dir",
            "/tmp/state",
            "--socket",
            "/tmp/a.sock",
            "--stderr",
            "--service",
        ])
        .unwrap();
        let expected = Options {
            log_dir: Some(PathBuf::from("/tmp/logs")),
            state_dir: Some(PathBuf::from("/tmp/state")),
            socket: Some(PathBuf::from("/tmp/a.sock")),
            stderr: true,
            service: true,
        };
        assert_eq!(command, Command::Run(expected));
    }

    #[test]
    fn enroll_forms() {
        assert_eq!(
            parse_args(&["enroll", "banc://join?u=x", "--name", "Station 3"]).unwrap(),
            Command::Enroll {
                invite: Invite::Link("banc://join?u=x".to_owned()),
                name: Some("Station 3".to_owned()),
                socket: None,
            }
        );
        assert_eq!(
            parse_args(&[
                "enroll",
                "--server",
                "https://b",
                "--key",
                "k-f",
                "--socket",
                "/s"
            ])
            .unwrap(),
            Command::Enroll {
                invite: Invite::Key {
                    server: "https://b".to_owned(),
                    key: "k-f".to_owned()
                },
                name: None,
                socket: Some(PathBuf::from("/s")),
            }
        );
        assert!(parse_args(&["enroll"]).is_err());
        assert!(parse_args(&["enroll", "--server", "https://b"]).is_err());
        assert!(parse_args(&["enroll", "link", "--key", "k"]).is_err());
    }

    #[test]
    fn other_commands() {
        assert_eq!(
            parse_args(&["status"]).unwrap(),
            Command::Status { socket: None }
        );
        assert_eq!(
            parse_args(&["service", "install"]).unwrap(),
            Command::InstallService
        );
        assert_eq!(
            parse_args(&["service", "uninstall"]).unwrap(),
            Command::UninstallService
        );
        assert_eq!(
            parse_args(&["update-apply", "/a/pending.json"]).unwrap(),
            Command::ApplyUpdate(PathBuf::from("/a/pending.json"))
        );
        assert_eq!(parse_args(&["--version"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn refused() {
        assert!(parse_args(&["--unknown"]).is_err());
        assert!(parse_args(&["argument"]).is_err());
        assert!(parse_args(&["--log-dir"]).is_err());
        assert!(parse_args(&["service", "restart"]).is_err());
        assert!(parse_args(&["--name", "x"]).is_err());
        assert!(parse_args(&["status", "extra"]).is_err());
    }
}
