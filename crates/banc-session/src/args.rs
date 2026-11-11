//! Options de la ligne de commande.

use std::path::PathBuf;

pub const HELP: &str = "\
banc-session : assistant de session de Banc

Usage : banc-session [options]

Options :
  --log-dir <dossier>  écrit les journaux dans ce dossier
  --stderr             recopie les journaux sur la sortie d'erreur
  --version            affiche la version
  --help               affiche cette aide
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
    fn sans_option() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Run(Options::default()));
    }

    #[test]
    fn dossier_des_journaux_et_sortie_d_erreur() {
        let command = parse_args(&["--log-dir", "/tmp/journaux", "--stderr"]).unwrap();
        let expected = Options {
            log_dir: Some(PathBuf::from("/tmp/journaux")),
            stderr: true,
        };
        assert_eq!(command, Command::Run(expected));
    }

    #[test]
    fn version_et_aide() {
        assert_eq!(parse_args(&["--version"]).unwrap(), Command::Version);
        assert_eq!(parse_args(&["--help"]).unwrap(), Command::Help);
    }

    #[test]
    fn option_inconnue_refusee() {
        assert!(parse_args(&["--inconnue"]).is_err());
        assert!(parse_args(&["argument"]).is_err());
        assert!(parse_args(&["--log-dir"]).is_err());
    }
}
