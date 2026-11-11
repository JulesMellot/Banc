//! Lancement des outils externes.

use std::io::Write;
use std::path::PathBuf;
use std::process::Command;

use anyhow::{bail, Context};

/// Racine du dépôt.
pub fn root() -> anyhow::Result<PathBuf> {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let root = manifest
        .ancestors()
        .nth(2)
        .context("racine du dépôt introuvable")?;
    Ok(root.to_path_buf())
}

/// Écrit une ligne sur la sortie d'erreur.
pub fn say(message: &str) {
    // Si la sortie d'erreur est fermée, il n'y a plus personne à prévenir.
    let _ = writeln!(std::io::stderr(), "{message}");
}

/// Commande pour un outil du PATH. Sous Windows, pnpm est un script `.cmd`.
pub fn tool(program: &str) -> Command {
    if cfg!(windows) && program == "pnpm" {
        Command::new("pnpm.cmd")
    } else {
        Command::new(program)
    }
}

pub fn cargo(args: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO"));
    command.args(args);
    command
}

fn describe(command: &Command) -> String {
    let mut text = command.get_program().to_string_lossy().into_owned();
    for arg in command.get_args() {
        text.push(' ');
        text.push_str(&arg.to_string_lossy());
    }
    text
}

fn spawn_error(command: &Command, error: std::io::Error) -> anyhow::Error {
    let program = command.get_program().to_string_lossy().into_owned();
    if error.kind() == std::io::ErrorKind::NotFound {
        anyhow::anyhow!("outil introuvable : {program} (voir docs/developpement.md)")
    } else {
        anyhow::Error::new(error).context(format!("lancement de {program}"))
    }
}

/// Lance la commande et échoue si elle échoue.
pub fn run(command: &mut Command) -> anyhow::Result<()> {
    say(&format!("$ {}", describe(command)));
    let status = command.status().map_err(|e| spawn_error(command, e))?;
    if !status.success() {
        bail!("échec de `{}` ({status})", describe(command));
    }
    Ok(())
}

/// Lance la commande et renvoie sa sortie standard.
pub fn output(command: &mut Command) -> anyhow::Result<String> {
    let output = command.output().map_err(|e| spawn_error(command, e))?;
    if !output.status.success() {
        bail!(
            "échec de `{}` ({}) : {}",
            describe(command),
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("sortie qui n'est pas en UTF-8")
}
