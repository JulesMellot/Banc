//! Keeps the workstation's programs up to date.
//!
//! Once an hour the agent reads the signed description of the latest
//! version. A newer version is downloaded and checked file by file, waits
//! for Resolve to be closed, then [`apply`] installs it. The download site
//! is not trusted: only the release key decides what gets installed.
//!
//! [`Monitor`] tells the window where this stands, and lets a person ask for
//! a check right away.

pub mod apply;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Context};
use banc_proto::keys::PublicKey;
use banc_proto::local::{UpdatePhase, UpdateStatus};
use banc_proto::packages::short_error;
use banc_proto::update::{self, Release, ReleaseFile, SignedRelease, Version};
use tokio::sync::Notify;

use self::apply::{Pending, Work};

/// Where versions are published. `BANC_UPDATE_SITE` replaces it at build
/// time, for the update tests.
const SITE: &str = match option_env!("BANC_UPDATE_SITE") {
    Some(site) => site,
    None => "https://github.com/JulesMellot/Banc/releases",
};
/// Seconds between two checks. `BANC_UPDATE_CHECK_SECS` replaces it at build
/// time, for the update tests.
const CHECK_SECS: Option<&str> = option_env!("BANC_UPDATE_CHECK_SECS");
const CHECK_EVERY: Duration = Duration::from_secs(3600);
/// After a start, leaves the machine to settle first.
const FIRST_CHECK: Duration = Duration::from_secs(120);
/// How often a downloaded version looks whether Resolve has been closed.
const RESOLVE_POLL: Duration = Duration::from_secs(60);
/// An update older than this is not running any more.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(900);
/// `update.json` is small: anything bigger is not it.
const MAX_DESCRIPTION: usize = 1024 * 1024;
/// The file that remembers a version that failed here.
const FAILED_FILE: &str = "update-failed";

/// The argument that makes the agent apply a downloaded update.
pub const APPLY_COMMAND: &str = "update-apply";

pub struct Settings {
    pub current: Version,
    pub key: PublicKey,
    pub site: String,
    /// This agent's program.
    pub program: PathBuf,
    pub state_dir: PathBuf,
    /// `macos-aarch64`, `windows-x86_64`.
    pub platform: String,
    pub first_check: Duration,
    pub every: Duration,
    /// How long the copy of the agent is given to finish an update.
    pub takeover: Duration,
}

impl Settings {
    /// `None` for a development build, which never updates itself.
    pub fn for_this_machine(state_dir: PathBuf) -> anyhow::Result<Option<Self>> {
        let current: Version = banc_proto::VERSION.parse()?;
        if current.is_development() {
            return Ok(None);
        }
        let test_interval = CHECK_SECS
            .and_then(|secs| secs.parse().ok())
            .map(Duration::from_secs);
        Ok(Some(Self {
            current,
            key: update::release_key()?,
            site: SITE.to_owned(),
            program: std::env::current_exe().context("locating the agent binary")?,
            state_dir,
            platform: format!(
                "{}-{}",
                banc_platform::machine::OS,
                banc_platform::machine::ARCH
            ),
            first_check: test_interval.unwrap_or(FIRST_CHECK),
            every: test_interval.unwrap_or(CHECK_EVERY),
            takeover: UPDATE_TIMEOUT,
        }))
    }

    fn failed_file(&self) -> PathBuf {
        failed_file(&self.state_dir)
    }
}

pub fn failed_file(state_dir: &Path) -> PathBuf {
    state_dir.join(FAILED_FILE)
}

/// Where the update stands, shared with the local endpoint.
#[derive(Default)]
pub struct Monitor {
    status: Mutex<UpdateStatus>,
    wake: Notify,
    /// A person asked: a version that failed here gets another chance.
    retry_failed: AtomicBool,
}

impl Monitor {
    pub fn status(&self) -> UpdateStatus {
        self.status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// Looks for a new version now.
    pub fn check_now(&self) {
        self.retry_failed.store(true, Ordering::Relaxed);
        self.wake.notify_one();
    }

    fn set(&self, phase: UpdatePhase, version: Option<Version>, error: Option<String>) {
        *self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = UpdateStatus {
            phase,
            version: version.map(|version| version.to_string()),
            error: error.map(|error| short_error(&error)),
        };
    }

    /// Waits for `delay`, or for a person asking for a check.
    async fn wait(&self, delay: Duration) {
        tokio::select! {
            () = tokio::time::sleep(delay) => {}
            () = self.wake.notified() => {}
        }
    }
}

/// How a check ended.
#[derive(Debug, PartialEq, Eq)]
enum Checked {
    UpToDate,
    /// The newest version already failed on this machine.
    FailedHere(Version),
    /// The copy of the agent is installing this version.
    HandedOver(Version),
}

/// Checks for a new version until the agent stops.
pub async fn watch(settings: Settings, monitor: std::sync::Arc<Monitor>) {
    monitor.wait(settings.first_check).await;
    loop {
        monitor.set(UpdatePhase::Checking, None, None);
        match check(&settings, &monitor).await {
            Ok(Checked::UpToDate) => monitor.set(UpdatePhase::UpToDate, None, None),
            Ok(Checked::FailedHere(version)) => {
                let reason = read_failure_reason(&settings.failed_file());
                monitor.set(UpdatePhase::Failed, Some(version), reason);
            }
            Ok(Checked::HandedOver(version)) => {
                // The copy of the agent stops this service when it succeeds.
                // Still running after that long: it never took over.
                tokio::time::sleep(settings.takeover).await;
                tracing::error!(%version, "the update did not start");
                monitor.set(
                    UpdatePhase::Failed,
                    Some(version),
                    Some("the update did not start".to_owned()),
                );
            }
            Err(error) => {
                tracing::warn!("update check failed: {error:#}");
                monitor.set(UpdatePhase::Failed, None, Some(format!("{error:#}")));
            }
        }
        monitor.wait(settings.every).await;
    }
}

async fn check(settings: &Settings, monitor: &Monitor) -> anyhow::Result<Checked> {
    let root = apply::install_root(&settings.program).context("no folder for the agent")?;
    let work = Work::for_root(&root);
    if is_running(&work) {
        return Ok(Checked::UpToDate);
    }
    remove_leftovers(&work)?;

    let client = http_client()?;
    let Some(signed) = fetch_description(&client, &settings.site).await? else {
        return Ok(Checked::UpToDate);
    };
    let release = signed.open(&settings.key)?;
    let retry = monitor.retry_failed.swap(false, Ordering::Relaxed);
    let failed = read_failed(&settings.failed_file()).filter(|_| !retry);
    let version = release.version;
    if !update::should_install(settings.current, version, failed) {
        return Ok(if version > settings.current && failed == Some(version) {
            Checked::FailedHere(version)
        } else {
            Checked::UpToDate
        });
    }
    let Some(files) = release.files(&settings.platform) else {
        tracing::debug!(platform = settings.platform, "no release for this platform");
        return Ok(Checked::UpToDate);
    };
    tracing::info!(%version, "downloading an update");
    monitor.set(UpdatePhase::Downloading, Some(version), None);
    download_all(&client, settings, &release, files, &work).await?;

    while banc_platform::machine::resolve_running() {
        monitor.set(UpdatePhase::WaitingForResolve, Some(version), None);
        tracing::debug!("update waiting for Resolve to close");
        tokio::time::sleep(RESOLVE_POLL).await;
    }
    monitor.set(UpdatePhase::Installing, Some(version), None);
    hand_over(settings, &root, &work, version, files)?;
    tracing::info!(%version, "installing the update");
    Ok(Checked::HandedOver(version))
}

/// An update in progress: its copy of the agent is at work.
fn is_running(work: &Work) -> bool {
    std::fs::metadata(work.pending())
        .and_then(|metadata| metadata.modified())
        .is_ok_and(|modified| {
            SystemTime::now()
                .duration_since(modified)
                .is_ok_and(|age| age < UPDATE_TIMEOUT)
        })
}

fn remove_leftovers(work: &Work) -> anyhow::Result<()> {
    match std::fs::remove_dir_all(work.dir()) {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
            Err(error).with_context(|| format!("removing {}", work.dir().display()))
        }
        _ => Ok(()),
    }
}

fn read_failed(path: &Path) -> Option<Version> {
    std::fs::read_to_string(path).ok()?.trim().parse().ok()
}

fn read_failure_reason(failed: &Path) -> Option<String> {
    let reason = std::fs::read_to_string(apply::reason_file(failed)).ok()?;
    Some(reason.trim().to_owned()).filter(|reason| !reason.is_empty())
}

/// No read limit on the whole request: a slow connection may take minutes to
/// bring a file, as long as data keeps arriving.
fn http_client() -> anyhow::Result<reqwest::Client> {
    // Already installed by an earlier call: fine.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(30))
        .read_timeout(Duration::from_secs(60))
        .user_agent(format!("banc-agent/{}", banc_proto::VERSION))
        .build()
        .context("starting the HTTP client")
}

/// `None` while nothing has been published.
async fn fetch_description(
    client: &reqwest::Client,
    site: &str,
) -> anyhow::Result<Option<SignedRelease>> {
    let url = format!("{site}/latest/download/update.json");
    let mut response = client.get(&url).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let mut response_body = Vec::new();
    response = response.error_for_status()?;
    while let Some(chunk) = response.chunk().await? {
        response_body.extend_from_slice(&chunk);
        if response_body.len() > MAX_DESCRIPTION {
            bail!("the release description is too large");
        }
    }
    Ok(Some(serde_json::from_slice(&response_body)?))
}

async fn download_all(
    client: &reqwest::Client,
    settings: &Settings,
    release: &Release,
    files: &[ReleaseFile],
    work: &Work,
) -> anyhow::Result<()> {
    for file in files {
        let url = format!(
            "{}/download/v{}/{}",
            settings.site, release.version, file.asset
        );
        download(client, &url, &work.staged().join(&file.path), file)
            .await
            .with_context(|| format!("downloading {}", file.path))?;
    }
    Ok(())
}

/// Brings one file. It only stays on disk if it is exactly the signed one.
async fn download(
    client: &reqwest::Client,
    url: &str,
    path: &Path,
    file: &ReleaseFile,
) -> anyhow::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let result = async {
        let mut response = client.get(url).send().await?.error_for_status()?;
        let mut output = std::fs::File::create(path)?;
        let mut hasher = blake3::Hasher::new();
        let mut size = 0;
        while let Some(chunk) = response.chunk().await? {
            size += chunk.len() as u64;
            if size > file.size {
                bail!("larger than the signed release says");
            }
            hasher.update(&chunk);
            output.write_all(&chunk)?;
        }
        output.sync_all()?;
        if size != file.size || hasher.finalize().to_hex().as_str() != file.blake3 {
            bail!("does not match the signed release");
        }
        banc_platform::fs::set_installed_mode(path, file.executable)?;
        Ok(())
    }
    .await;
    if result.is_err() {
        let _ = std::fs::remove_file(path);
    }
    result
}

/// Starts a copy of this agent, which replaces the installed programs.
fn hand_over(
    settings: &Settings,
    root: &Path,
    work: &Work,
    version: Version,
    files: &[ReleaseFile],
) -> anyhow::Result<()> {
    let pending = Pending {
        version,
        root: root.to_path_buf(),
        files: files.to_vec(),
    };
    let name = settings
        .program
        .file_name()
        .context("no name for the agent binary")?;
    let guard = work.guard_dir().join(name);
    std::fs::create_dir_all(work.guard_dir())?;
    std::fs::copy(&settings.program, &guard).context("copying the agent")?;
    banc_platform::fs::set_installed_mode(&guard, true)?;
    std::fs::write(work.pending(), serde_json::to_vec(&pending)?)?;
    banc_platform::service::launch_updater(
        &guard,
        &[APPLY_COMMAND.into(), work.pending().into_os_string()],
    )
    .context("starting the update")
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;

    use banc_proto::keys::SecretKey;

    use super::*;

    /// A download site: serves these paths, 404 for the others.
    fn site(pages: HashMap<String, Vec<u8>>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let pages = Arc::new(pages);
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut request = [0; 2048];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]);
                let path = request.split_whitespace().nth(1).unwrap_or("").to_owned();
                let (status, body) = match pages.get(&path) {
                    Some(body) => ("200 OK", body.clone()),
                    None => ("404 Not Found", Vec::new()),
                };
                let head = format!(
                    "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(&body);
            }
        });
        format!("http://{address}")
    }

    fn release_file(content: &[u8]) -> ReleaseFile {
        ReleaseFile {
            path: "banc-agent".to_owned(),
            asset: "linux-x86_64.agent".to_owned(),
            size: content.len() as u64,
            blake3: blake3::hash(content).to_hex().to_string(),
            executable: true,
        }
    }

    fn signed(version: &str, file: ReleaseFile) -> Vec<u8> {
        let release = Release {
            version: version.parse().unwrap(),
            platforms: BTreeMap::from([("linux-x86_64".to_owned(), vec![file])]),
        };
        let signed = SignedRelease::sign(&SecretKey::from_bytes(&[9; 32]), &release).unwrap();
        serde_json::to_vec(&signed).unwrap()
    }

    fn client() -> reqwest::Client {
        http_client().unwrap()
    }

    #[tokio::test]
    async fn nothing_published_yet() {
        let url = site(HashMap::new());
        assert_eq!(fetch_description(&client(), &url).await.unwrap(), None);
    }

    #[tokio::test]
    async fn description_is_read_and_its_signature_checked() {
        let body = signed("1.2.0", release_file(b"agent"));
        let url = site(HashMap::from([(
            "/latest/download/update.json".to_owned(),
            body,
        )]));
        let description = fetch_description(&client(), &url).await.unwrap().unwrap();
        let key = SecretKey::from_bytes(&[9; 32]).public();
        assert_eq!(description.open(&key).unwrap().version.to_string(), "1.2.0");
        let other = SecretKey::from_bytes(&[8; 32]).public();
        assert!(description.open(&other).is_err());
    }

    #[tokio::test]
    async fn an_oversized_description_is_refused() {
        let body = vec![b' '; MAX_DESCRIPTION + 1];
        let url = site(HashMap::from([(
            "/latest/download/update.json".to_owned(),
            body,
        )]));
        assert!(fetch_description(&client(), &url).await.is_err());
    }

    #[tokio::test]
    async fn a_good_file_is_kept() {
        let tmp = tempfile::tempdir().unwrap();
        let url = site(HashMap::from([("/a".to_owned(), b"agent".to_vec())]));
        let path = tmp.path().join("new/banc-agent");
        download(
            &client(),
            &format!("{url}/a"),
            &path,
            &release_file(b"agent"),
        )
        .await
        .unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"agent");
    }

    #[tokio::test]
    async fn a_file_that_does_not_match_is_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("banc-agent");
        for served in [&b"evil!"[..], b"age", b"agent and more"] {
            let url = site(HashMap::from([("/a".to_owned(), served.to_vec())]));
            let result = download(
                &client(),
                &format!("{url}/a"),
                &path,
                &release_file(b"agent"),
            )
            .await;
            assert!(result.is_err(), "{served:?}");
            assert!(!path.exists(), "{served:?}");
        }
    }

    #[tokio::test]
    async fn a_missing_file_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let url = site(HashMap::new());
        let path = tmp.path().join("banc-agent");
        let result = download(&client(), &format!("{url}/a"), &path, &release_file(b"x")).await;
        assert!(result.is_err());
        assert!(!path.exists());
    }

    #[test]
    fn the_failed_version_is_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = failed_file(tmp.path());
        assert_eq!(read_failed(&path), None);
        std::fs::write(&path, "1.2.0\n").unwrap();
        assert_eq!(read_failed(&path), Some("1.2.0".parse().unwrap()));
        std::fs::write(&path, "garbage").unwrap();
        assert_eq!(read_failed(&path), None);
    }

    #[test]
    fn the_reason_of_a_failure_is_read_back() {
        let tmp = tempfile::tempdir().unwrap();
        let path = failed_file(tmp.path());
        assert_eq!(read_failure_reason(&path), None);
        std::fs::write(apply::reason_file(&path), "replacing the files: denied\n").unwrap();
        assert_eq!(
            read_failure_reason(&path).as_deref(),
            Some("replacing the files: denied")
        );
    }

    fn settings(tmp: &Path, site: String, current: &str) -> Settings {
        Settings {
            current: current.parse().unwrap(),
            key: SecretKey::from_bytes(&[9; 32]).public(),
            site,
            program: tmp.join("Banc/banc-agent"),
            state_dir: tmp.to_path_buf(),
            platform: "linux-x86_64".to_owned(),
            first_check: Duration::from_secs(3600),
            every: Duration::from_secs(3600),
            takeover: Duration::from_secs(3600),
        }
    }

    fn published(version: &str) -> String {
        site(HashMap::from([(
            "/latest/download/update.json".to_owned(),
            signed(version, release_file(b"agent")),
        )]))
    }

    #[tokio::test]
    async fn the_same_version_is_up_to_date() {
        let tmp = tempfile::tempdir().unwrap();
        let settings = settings(tmp.path(), published("1.2.0"), "1.2.0");
        let checked = check(&settings, &Monitor::default()).await.unwrap();
        assert_eq!(checked, Checked::UpToDate);
    }

    #[tokio::test]
    async fn a_version_that_failed_here_is_reported_until_a_person_retries() {
        let tmp = tempfile::tempdir().unwrap();
        // Nothing to download: a retry fails on the missing file.
        let settings = settings(tmp.path(), published("1.2.0"), "1.1.0");
        std::fs::write(settings.failed_file(), "1.2.0").unwrap();
        let monitor = Monitor::default();
        let checked = check(&settings, &monitor).await.unwrap();
        assert_eq!(checked, Checked::FailedHere("1.2.0".parse().unwrap()));

        monitor.check_now();
        let retried = check(&settings, &monitor).await.unwrap_err();
        assert!(
            format!("{retried:#}").contains("downloading"),
            "{retried:#}"
        );
        assert_eq!(monitor.status().phase, UpdatePhase::Downloading);
        let again = check(&settings, &monitor).await.unwrap();
        assert_eq!(again, Checked::FailedHere("1.2.0".parse().unwrap()));
    }

    #[tokio::test]
    async fn a_person_does_not_wait_for_the_next_hour() {
        let monitor = Monitor::default();
        monitor.check_now();
        let hour = monitor.wait(Duration::from_secs(3600));
        assert!(tokio::time::timeout(Duration::from_secs(5), hour)
            .await
            .is_ok());
    }

    #[test]
    fn a_recent_update_is_left_alone_and_an_old_folder_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let work = Work::for_root(&tmp.path().join("Banc"));
        assert!(!is_running(&work));
        std::fs::create_dir_all(work.staged()).unwrap();
        std::fs::write(work.pending(), "{}").unwrap();
        assert!(is_running(&work));
        std::fs::remove_file(work.pending()).unwrap();
        assert!(!is_running(&work));
        remove_leftovers(&work).unwrap();
        assert!(!work.dir().exists());
        remove_leftovers(&work).unwrap();
    }
}
