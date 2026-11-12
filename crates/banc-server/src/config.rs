//! Server configuration, read from a TOML file.
//!
//! The commented example is in `packaging/server/banc-server.toml`.

use std::path::{Path, PathBuf};

use serde::Deserialize;

/// Name of the SQLite file in `data_dir`.
const DATABASE_FILE: &str = "banc.sqlite";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Server data folder (SQLite database, later the file cache).
    pub data_dir: PathBuf,
    #[serde(default)]
    pub logs: LogsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct LogsConfig {
    /// Log folder. When absent: the usual location for the OS.
    pub dir: Option<PathBuf>,
    pub level: LogLevel,
    /// Number of days of logs kept.
    pub keep_days: u16,
}

impl Default for LogsConfig {
    fn default() -> Self {
        Self {
            dir: None,
            level: LogLevel::Info,
            keep_days: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl From<LogLevel> for tracing::Level {
    fn from(level: LogLevel) -> Self {
        match level {
            LogLevel::Error => Self::ERROR,
            LogLevel::Warn => Self::WARN,
            LogLevel::Info => Self::INFO,
            LogLevel::Debug => Self::DEBUG,
            LogLevel::Trace => Self::TRACE,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read the configuration {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("unreadable configuration: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("configuration refused: {0}")]
    Invalid(&'static str),
}

impl Config {
    /// Reads and checks the file. Relative paths start from the configuration
    /// file's folder.
    pub fn load(path: &Path) -> Result<Self, ConfigError> {
        let text = std::fs::read_to_string(path).map_err(|source| ConfigError::Read {
            path: path.to_path_buf(),
            source,
        })?;
        let base = match path.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => parent,
            _ => Path::new("."),
        };
        Self::parse(&text, base)
    }

    /// Checks a TOML text. `base` is the starting point for relative paths.
    pub fn parse(text: &str, base: &Path) -> Result<Self, ConfigError> {
        let mut config: Self = toml::from_str(text)?;
        if config.data_dir.as_os_str().is_empty() {
            return Err(ConfigError::Invalid("data_dir is empty"));
        }
        if config.logs.keep_days == 0 {
            return Err(ConfigError::Invalid("logs.keep_days must be at least 1"));
        }
        if matches!(&config.logs.dir, Some(dir) if dir.as_os_str().is_empty()) {
            return Err(ConfigError::Invalid("logs.dir is empty"));
        }
        config.data_dir = base.join(&config.data_dir);
        config.logs.dir = config.logs.dir.map(|dir| base.join(dir));
        Ok(config)
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("/etc/banc"))
    }

    #[test]
    fn minimal_config() {
        let config = parse(r#"data_dir = "/var/lib/banc""#).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/var/lib/banc"));
        assert_eq!(config.logs, LogsConfig::default());
        assert_eq!(
            config.database_path(),
            PathBuf::from("/var/lib/banc/banc.sqlite")
        );
    }

    #[test]
    fn full_config() {
        let config = parse(
            r#"
            data_dir = "/var/lib/banc"

            [logs]
            dir = "/var/log/banc"
            level = "debug"
            keep_days = 7
            "#,
        )
        .unwrap();
        assert_eq!(config.logs.dir, Some(PathBuf::from("/var/log/banc")));
        assert_eq!(config.logs.level, LogLevel::Debug);
        assert_eq!(config.logs.keep_days, 7);
    }

    #[test]
    fn relative_paths_from_the_file_folder() {
        let config = parse("data_dir = \"data\"\n[logs]\ndir = \"logs\"").unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/etc/banc/data"));
        assert_eq!(config.logs.dir, Some(PathBuf::from("/etc/banc/logs")));
    }

    #[test]
    fn unknown_key_refused() {
        let err = parse("data_dir = \"/d\"\nport = 443").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err:?}");
        let err = parse("data_dir = \"/d\"\n[logs]\nlvl = \"info\"").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err:?}");
    }

    #[test]
    fn data_dir_required() {
        assert!(matches!(parse(""), Err(ConfigError::Parse(_))));
        assert!(matches!(
            parse(r#"data_dir = """#),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn log_values_checked() {
        let invalid = |logs: &str| parse(&format!("data_dir = \"/d\"\n[logs]\n{logs}"));
        assert!(matches!(
            invalid("keep_days = 0"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            invalid(r#"dir = """#),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            invalid(r#"level = "chatty""#),
            Err(ConfigError::Parse(_))
        ));
    }

    #[test]
    fn log_levels() {
        let level = |name: &str| {
            let text = format!("data_dir = \"/d\"\n[logs]\nlevel = \"{name}\"");
            tracing::Level::from(parse(&text).unwrap().logs.level)
        };
        assert_eq!(level("error"), tracing::Level::ERROR);
        assert_eq!(level("warn"), tracing::Level::WARN);
        assert_eq!(level("info"), tracing::Level::INFO);
        assert_eq!(level("debug"), tracing::Level::DEBUG);
        assert_eq!(level("trace"), tracing::Level::TRACE);
    }

    #[test]
    fn missing_file() {
        let err = Config::load(Path::new("/missing/path/banc-server.toml")).unwrap_err();
        assert!(matches!(err, ConfigError::Read { .. }), "{err:?}");
    }

    #[test]
    fn repository_examples_are_valid() {
        let packaged = include_str!("../../../packaging/server/banc-server.toml");
        parse(packaged).unwrap();
        let dev = include_str!("../../../dev/server.toml");
        parse(dev).unwrap();
    }
}
