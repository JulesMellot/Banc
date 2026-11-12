//! Command-line options.

use std::path::PathBuf;

pub const HELP: &str = "\
banc-server : serveur de Banc

Usage : banc-server --config <fichier> [options]

Options :
  --config <fichier>  fichier de configuration TOML (obligatoire)
  --stderr            recopie les journaux sur la sortie d'erreur
  --version           affiche la version
  --help              affiche cette aide
";

#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    pub config: PathBuf,
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

    let mut config = None;
    let mut stderr = false;
    while let Some(arg) = parser.next()? {
        match arg {
            Long("config") => config = Some(PathBuf::from(parser.value()?)),
            Long("stderr") => stderr = true,
            Long("version") => return Ok(Command::Version),
            Long("help") => return Ok(Command::Help),
            _ => return Err(arg.unexpected()),
        }
    }
    let config = config.ok_or("l'option --config <fichier> est obligatoire")?;
    Ok(Command::Run(Options { config, stderr }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_args(args: &[&str]) -> Result<Command, lexopt::Error> {
        parse(lexopt::Parser::from_args(args))
    }

    #[test]
    fn configuration_obligatoire() {
        assert!(parse_args(&[]).is_err());
        assert!(parse_args(&["--stderr"]).is_err());
        assert!(parse_args(&["--config"]).is_err());
    }

    #[test]
    fn configuration_et_sortie_d_erreur() {
        let command = parse_args(&["--config", "/etc/banc/banc-server.toml", "--stderr"]).unwrap();
        let expected = Options {
            config: PathBuf::from("/etc/banc/banc-server.toml"),
            stderr: true,
        };
        assert_eq!(command, Command::Run(expected));
    }

    #[test]
    fn version_et_aide_sans_configuration() {
        assert_eq!(parse_args(&["--version"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn option_inconnue_refusee() {
        assert!(parse_args(&["--config", "c.toml", "--inconnue"]).is_err());
        assert!(parse_args(&["c.toml"]).is_err());
    }
}
