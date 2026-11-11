//! Journaux sur disque avec rotation quotidienne.
//!
//! Un fichier par jour, `<nom>.<AAAA-MM-JJ>.log`, et suppression des plus
//! anciens au-delà de `keep_files`. L'écriture se fait hors du fil appelant.

use std::path::PathBuf;

use tracing::level_filters::LevelFilter;
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, Layer, Registry};

/// Lignes en attente d'écriture gardées en mémoire.
const BUFFERED_LINES: usize = 1024;

/// Réglages des journaux d'un binaire.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Dossier des fichiers, créé s'il n'existe pas.
    pub dir: PathBuf,
    /// Préfixe des fichiers, en pratique le nom du binaire.
    pub name: String,
    /// Niveau le plus bavard qui est gardé.
    pub level: Level,
    /// Nombre de fichiers quotidiens gardés.
    pub keep_files: usize,
    /// Écrit aussi sur la sortie d'erreur, pour le développement.
    pub stderr: bool,
}

impl LogConfig {
    /// Niveau `info`, 14 jours gardés, pas de sortie d'erreur.
    pub fn new(dir: impl Into<PathBuf>, name: impl Into<String>) -> Self {
        Self {
            dir: dir.into(),
            name: name.into(),
            level: Level::INFO,
            keep_files: 14,
            stderr: false,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("impossible de créer le dossier des journaux {dir}")]
    CreateDir {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("impossible d'ouvrir le journal dans {dir}")]
    Open {
        dir: PathBuf,
        #[source]
        source: tracing_appender::rolling::InitError,
    },
    #[error("les journaux sont déjà initialisés dans ce processus")]
    AlreadyInitialized,
}

/// À garder en vie jusqu'à la fin du processus : sa destruction vide le tampon
/// d'écriture sur le disque.
#[must_use = "les dernières lignes sont perdues si le garde est détruit trop tôt"]
#[derive(Debug)]
pub struct LogGuard {
    _worker: WorkerGuard,
}

/// Installe les journaux pour tout le processus.
pub fn init(config: &LogConfig) -> Result<LogGuard, LogError> {
    let (subscriber, guard) = build(config)?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| LogError::AlreadyInitialized)?;
    Ok(guard)
}

/// Construit le collecteur sans l'installer.
pub fn build(
    config: &LogConfig,
) -> Result<(Box<dyn Subscriber + Send + Sync>, LogGuard), LogError> {
    std::fs::create_dir_all(&config.dir).map_err(|source| LogError::CreateDir {
        dir: config.dir.clone(),
        source,
    })?;
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix(&config.name)
        .filename_suffix("log")
        .max_log_files(config.keep_files.max(1))
        .build(&config.dir)
        .map_err(|source| LogError::Open {
            dir: config.dir.clone(),
            source,
        })?;
    // La file est allouée d'avance : la taille par défaut (128 000 lignes)
    // coûte 4 Mo de mémoire au repos. Au-delà, les lignes sont perdues
    // plutôt que de bloquer l'appelant.
    let (writer, worker) = NonBlockingBuilder::default()
        .buffered_lines_limit(BUFFERED_LINES)
        .finish(appender);

    let filter = LevelFilter::from_level(config.level);
    let file = fmt::layer()
        .with_ansi(false)
        .with_writer(writer)
        .with_filter(filter);
    let stderr = config.stderr.then(|| {
        fmt::layer()
            .with_ansi(false)
            .with_writer(std::io::stderr)
            .with_filter(filter)
    });
    let subscriber = Registry::default().with(file).with(stderr);
    Ok((Box::new(subscriber), LogGuard { _worker: worker }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_logs(dir: &std::path::Path) -> (Vec<String>, String) {
        let mut names = Vec::new();
        let mut content = String::new();
        for entry in std::fs::read_dir(dir).unwrap() {
            let entry = entry.unwrap();
            names.push(entry.file_name().to_string_lossy().into_owned());
            content.push_str(&std::fs::read_to_string(entry.path()).unwrap());
        }
        (names, content)
    }

    #[test]
    fn ecrit_dans_un_fichier_date_et_filtre_par_niveau() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("journaux");
        let config = LogConfig::new(&dir, "banc-test");

        let (subscriber, guard) = build(&config).unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(poste = "mac-01", "ligne gardée");
            tracing::debug!("ligne ignorée");
        });
        drop(guard);

        let (names, content) = read_logs(&dir);
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("banc-test."), "{names:?}");
        assert!(names[0].ends_with(".log"), "{names:?}");
        assert!(content.contains("ligne gardée"), "{content}");
        assert!(content.contains("mac-01"), "{content}");
        assert!(!content.contains("ligne ignorée"), "{content}");
    }

    #[test]
    fn supprime_les_fichiers_les_plus_anciens() {
        let tmp = tempfile::tempdir().unwrap();
        for day in ["2020-01-01", "2020-01-02", "2020-01-03"] {
            std::fs::write(tmp.path().join(format!("banc-test.{day}.log")), "ancien\n").unwrap();
        }
        let mut config = LogConfig::new(tmp.path(), "banc-test");
        config.keep_files = 2;

        let (subscriber, guard) = build(&config).unwrap();
        tracing::subscriber::with_default(subscriber, || tracing::info!("nouveau"));
        drop(guard);

        // La rotation trie par date de création du fichier : on vérifie le
        // nombre gardé et la présence du fichier du jour, pas l'ancien retenu.
        let (names, content) = read_logs(tmp.path());
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(content.contains("nouveau"), "{content}");
    }

    #[test]
    fn dossier_impossible_a_creer() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("fichier");
        std::fs::write(&file, "").unwrap();

        let result = build(&LogConfig::new(file.join("journaux"), "banc-test"));
        assert!(matches!(result, Err(LogError::CreateDir { .. })));
    }
}
