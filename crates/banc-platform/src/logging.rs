//! On-disk logs with daily rotation.
//!
//! One file per day, `<name>.<YYYY-MM-DD>.log`, and the oldest are deleted
//! beyond `keep_files`. Writing happens off the calling thread.

use std::path::PathBuf;

use tracing::level_filters::LevelFilter;
use tracing::{Level, Subscriber};
use tracing_appender::non_blocking::{NonBlockingBuilder, WorkerGuard};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{fmt, Layer, Registry};

/// Lines waiting to be written, kept in memory.
const BUFFERED_LINES: usize = 1024;

/// Log settings for a binary.
#[derive(Debug, Clone)]
pub struct LogConfig {
    /// Folder for the files, created if it does not exist.
    pub dir: PathBuf,
    /// File name prefix, in practice the binary's name.
    pub name: String,
    /// Most verbose level that is kept.
    pub level: Level,
    /// Number of daily files kept.
    pub keep_files: usize,
    /// Also writes to standard error, for development.
    pub stderr: bool,
}

impl LogConfig {
    /// `info` level, 14 days kept, no standard error output.
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
    #[error("cannot create the log folder {dir}")]
    CreateDir {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot open the log in {dir}")]
    Open {
        dir: PathBuf,
        #[source]
        source: tracing_appender::rolling::InitError,
    },
    #[error("logs are already initialized in this process")]
    AlreadyInitialized,
}

/// Keep alive until the process ends: dropping it flushes the write buffer
/// to disk.
#[must_use = "the last lines are lost if the guard is dropped too early"]
#[derive(Debug)]
pub struct LogGuard {
    _worker: WorkerGuard,
}

/// Installs the logs for the whole process.
pub fn init(config: &LogConfig) -> Result<LogGuard, LogError> {
    let (subscriber, guard) = build(config)?;
    tracing::subscriber::set_global_default(subscriber)
        .map_err(|_| LogError::AlreadyInitialized)?;
    Ok(guard)
}

/// Builds the subscriber without installing it.
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
    // The queue is preallocated: the default size (128,000 lines) costs
    // 4 MB of memory at rest. Beyond the limit, lines are dropped rather
    // than blocking the caller.
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
    fn writes_to_a_dated_file_filtered_by_level() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("logs");
        let config = LogConfig::new(&dir, "banc-test");

        let (subscriber, guard) = build(&config).unwrap();
        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(station = "mac-01", "kept line");
            tracing::debug!("ignored line");
        });
        drop(guard);

        let (names, content) = read_logs(&dir);
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].starts_with("banc-test."), "{names:?}");
        assert!(names[0].ends_with(".log"), "{names:?}");
        assert!(content.contains("kept line"), "{content}");
        assert!(content.contains("mac-01"), "{content}");
        assert!(!content.contains("ignored line"), "{content}");
    }

    #[test]
    fn deletes_the_oldest_files() {
        let tmp = tempfile::tempdir().unwrap();
        for day in ["2020-01-01", "2020-01-02", "2020-01-03"] {
            std::fs::write(tmp.path().join(format!("banc-test.{day}.log")), "old\n").unwrap();
        }
        let mut config = LogConfig::new(tmp.path(), "banc-test");
        config.keep_files = 2;

        let (subscriber, guard) = build(&config).unwrap();
        tracing::subscriber::with_default(subscriber, || tracing::info!("new"));
        drop(guard);

        // Rotation sorts by file creation date: check the number kept and
        // the presence of today's file, not which old one was retained.
        let (names, content) = read_logs(tmp.path());
        assert_eq!(names.len(), 2, "{names:?}");
        assert!(content.contains("new"), "{content}");
    }

    #[test]
    fn folder_cannot_be_created() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("file");
        std::fs::write(&file, "").unwrap();

        let result = build(&LogConfig::new(file.join("logs"), "banc-test"));
        assert!(matches!(result, Err(LogError::CreateDir { .. })));
    }
}
