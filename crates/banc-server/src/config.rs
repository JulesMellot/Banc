//! Server configuration, read from a TOML file.
//!
//! The commented example is in `packaging/server/banc-server.toml`.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use banc_proto::invite::normalize_server;

use serde::Deserialize;

/// Name of the SQLite file in `data_dir`.
const DATABASE_FILE: &str = "banc.sqlite";
/// Name of the server's iroh key in `data_dir`.
const SERVER_KEY_FILE: &str = "server.key";

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Server data folder (SQLite database, server key, later the file
    /// cache).
    pub data_dir: PathBuf,
    /// Address at which workstations and admins reach the server, written
    /// into invitation links. `https://`, or `http://localhost` in
    /// development. When absent, the server learns it from the request that
    /// creates the first admin.
    #[serde(default)]
    pub public_url: Option<String>,
    #[serde(default)]
    pub http: HttpConfig,
    #[serde(default)]
    pub iroh: IrohConfig,
    #[serde(default)]
    pub relay: RelayConfig,
    #[serde(default)]
    pub logs: LogsConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct HttpConfig {
    /// API and web admin. TLS is terminated in front of the server.
    pub listen: SocketAddr,
    /// Folder of the built web admin (`ui/dist`). When absent, the server
    /// only serves the API.
    pub admin_ui: Option<PathBuf>,
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([127, 0, 0, 1], 8080)),
            admin_ui: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct IrohConfig {
    /// UDP address of the server's iroh endpoint.
    pub listen: SocketAddr,
    /// Public addresses of that endpoint, given to agents so that they can
    /// try a direct connection before the relay. Empty: relay only.
    pub public_addrs: Vec<SocketAddr>,
}

impl Default for IrohConfig {
    fn default() -> Self {
        Self {
            listen: SocketAddr::from(([0, 0, 0, 0], 443)),
            public_addrs: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct RelayConfig {
    /// Runs the team's iroh relay in the server process.
    pub enabled: bool,
    /// HTTP address of the relay. TLS is terminated in front of it.
    pub listen: SocketAddr,
    /// Public URL of the relay. Default: `public_url`.
    pub url: Option<String>,
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            listen: SocketAddr::from(([127, 0, 0, 1], 3340)),
            url: None,
        }
    }
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
        config.public_url = config
            .public_url
            .as_deref()
            .map(normalize_server)
            .transpose()
            .map_err(|_| ConfigError::Invalid("public_url must be an https:// address"))?;
        if let Some(url) = &config.relay.url {
            let url = normalize_server(url)
                .map_err(|_| ConfigError::Invalid("relay.url must be an https:// address"))?;
            config.relay.url = Some(url);
        }
        if matches!(&config.http.admin_ui, Some(dir) if dir.as_os_str().is_empty()) {
            return Err(ConfigError::Invalid("http.admin_ui is empty"));
        }
        config.data_dir = base.join(&config.data_dir);
        config.logs.dir = config.logs.dir.map(|dir| base.join(dir));
        config.http.admin_ui = config.http.admin_ui.map(|dir| base.join(dir));
        Ok(config)
    }

    pub fn database_path(&self) -> PathBuf {
        self.data_dir.join(DATABASE_FILE)
    }

    pub fn server_key_path(&self) -> PathBuf {
        self.data_dir.join(SERVER_KEY_FILE)
    }

    /// Public URL of the relay, `None` when it is disabled or when the
    /// public address is not known yet.
    pub fn relay_url(&self) -> Option<String> {
        if !self.relay.enabled {
            return None;
        }
        self.relay.url.clone().or_else(|| self.public_url.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINIMAL: &str =
        "data_dir = \"/var/lib/banc\"\npublic_url = \"https://banc.example.com\"\n";

    fn parse(text: &str) -> Result<Config, ConfigError> {
        Config::parse(text, Path::new("/etc/banc"))
    }

    #[test]
    fn minimal_config() {
        let config = parse(MINIMAL).unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/var/lib/banc"));
        assert_eq!(
            config.public_url.as_deref(),
            Some("https://banc.example.com")
        );
        assert_eq!(config.logs, LogsConfig::default());
        assert_eq!(config.http, HttpConfig::default());
        assert_eq!(config.iroh, IrohConfig::default());
        assert_eq!(config.relay, RelayConfig::default());
        assert_eq!(
            config.database_path(),
            PathBuf::from("/var/lib/banc/banc.sqlite")
        );
        assert_eq!(
            config.server_key_path(),
            PathBuf::from("/var/lib/banc/server.key")
        );
        assert_eq!(
            config.relay_url().as_deref(),
            Some("https://banc.example.com")
        );
    }

    #[test]
    fn network_sections() {
        let config = parse(&format!(
            r#"{MINIMAL}
            [http]
            listen = "0.0.0.0:9000"
            admin_ui = "admin"

            [iroh]
            listen = "0.0.0.0:7842"
            public_addrs = ["203.0.113.5:7842"]

            [relay]
            listen = "127.0.0.1:3341"
            url = "https://relay.example.com/"
            "#
        ))
        .unwrap();
        assert_eq!(config.http.listen, "0.0.0.0:9000".parse().unwrap());
        assert_eq!(config.http.admin_ui, Some(PathBuf::from("/etc/banc/admin")));
        assert_eq!(
            config.iroh.public_addrs,
            ["203.0.113.5:7842".parse().unwrap()]
        );
        assert_eq!(
            config.relay_url().as_deref(),
            Some("https://relay.example.com")
        );

        let disabled = parse(&format!("{MINIMAL}\n[relay]\nenabled = false")).unwrap();
        assert_eq!(disabled.relay_url(), None);
    }

    #[test]
    fn public_url_checked() {
        let with_url = |url: &str| parse(&format!("data_dir = \"/d\"\npublic_url = \"{url}\""));
        assert!(matches!(
            with_url("banc.example.com"),
            Err(ConfigError::Invalid(_))
        ));
        assert!(matches!(
            with_url("http://banc.example.com"),
            Err(ConfigError::Invalid(_))
        ));
        assert_eq!(
            with_url("http://localhost:8080/")
                .unwrap()
                .public_url
                .as_deref(),
            Some("http://localhost:8080")
        );
        let learned = parse("data_dir = \"/d\"").unwrap();
        assert_eq!(learned.public_url, None);
        assert_eq!(learned.relay_url(), None);
        let bad_relay = format!("{MINIMAL}\n[relay]\nurl = \"ftp://x\"");
        assert!(matches!(parse(&bad_relay), Err(ConfigError::Invalid(_))));
    }

    #[test]
    fn full_config() {
        let config = parse(
            r#"
            data_dir = "/var/lib/banc"
            public_url = "https://banc.example.com"

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
        let config = parse(
            "data_dir = \"data\"\npublic_url = \"https://b.example\"\n[logs]\ndir = \"logs\"",
        )
        .unwrap();
        assert_eq!(config.data_dir, PathBuf::from("/etc/banc/data"));
        assert_eq!(config.logs.dir, Some(PathBuf::from("/etc/banc/logs")));
    }

    #[test]
    fn unknown_key_refused() {
        let err =
            parse("data_dir = \"/d\"\npublic_url = \"https://b.example\"\nport = 443").unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err:?}");
        let err =
            parse("data_dir = \"/d\"\npublic_url = \"https://b.example\"\n[logs]\nlvl = \"info\"")
                .unwrap_err();
        assert!(matches!(err, ConfigError::Parse(_)), "{err:?}");
    }

    #[test]
    fn data_dir_required() {
        assert!(matches!(parse(""), Err(ConfigError::Parse(_))));
        assert!(matches!(
            parse("data_dir = \"\"\npublic_url = \"https://b.example\"\n"),
            Err(ConfigError::Invalid(_))
        ));
    }

    #[test]
    fn log_values_checked() {
        let invalid = |logs: &str| {
            parse(&format!(
                "data_dir = \"/d\"\npublic_url = \"https://b.example\"\n[logs]\n{logs}"
            ))
        };
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
            let text = format!(
                "data_dir = \"/d\"\npublic_url = \"https://b.example\"\n[logs]\nlevel = \"{name}\""
            );
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
