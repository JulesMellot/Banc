//! Replaces the installed programs with a downloaded version.
//!
//! This runs from a copy of the old agent, outside the service: the agent
//! cannot replace itself and then judge its own replacement. The copy stops
//! the service, swaps the files, starts the service, and puts the old
//! version back if the new one does not answer.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{bail, Context};
use banc_proto::update::{ReleaseFile, Version};
use serde::{Deserialize, Serialize};

/// What the agent hands over to its copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub version: Version,
    pub root: PathBuf,
    pub files: Vec<ReleaseFile>,
}

/// The folder of an update, next to the application: same disk, so that
/// moving files is instant, and same protection as the application itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    dir: PathBuf,
}

impl Work {
    pub fn for_root(root: &Path) -> Self {
        let mut name = root.file_name().unwrap_or_default().to_os_string();
        name.push(".update");
        Self {
            dir: root.with_file_name(name),
        }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The downloaded version.
    pub fn staged(&self) -> PathBuf {
        self.dir.join("new")
    }

    /// The version it replaces, kept until the new one has proved itself.
    pub fn previous(&self) -> PathBuf {
        self.dir.join("previous")
    }

    pub fn pending(&self) -> PathBuf {
        self.dir.join("pending.json")
    }

    /// Where the copy of the old agent runs from.
    pub fn guard_dir(&self) -> PathBuf {
        self.dir.join("guard")
    }
}

/// The folder an update replaces: the `.app` bundle holding the program on
/// macOS, the program's folder elsewhere.
pub fn install_root(program: &Path) -> Option<PathBuf> {
    let bundle = program
        .ancestors()
        .find(|path| is_bundle(path))
        .map(Path::to_path_buf);
    bundle.or_else(|| program.parent().map(Path::to_path_buf))
}

fn is_bundle(path: &Path) -> bool {
    path.extension().is_some_and(|extension| extension == "app")
}

/// BLAKE3 of a file, in hexadecimal.
pub fn hash_file(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Checks the downloaded files once more, right before they are installed.
/// A file already moved by an interrupted update is not there any more.
pub fn verify_staged(staged: &Path, files: &[ReleaseFile]) -> anyhow::Result<()> {
    for file in files {
        let path = staged.join(&file.path);
        if !path.exists() {
            continue;
        }
        let hash = hash_file(&path).with_context(|| format!("reading {}", path.display()))?;
        if hash != file.blake3 {
            bail!("{} does not match the signed release", file.path);
        }
    }
    Ok(())
}

fn rename(from: &Path, to: &Path) -> std::io::Result<()> {
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(from, to)
}

/// Puts the new version in place. Every step can run again after an
/// interruption. A bundle is replaced whole, which keeps its signature
/// coherent. Elsewhere file by file: Windows does not let a folder holding a
/// running program be renamed, but does let the program itself be.
pub fn swap(root: &Path, work: &Work, files: &[ReleaseFile]) -> std::io::Result<()> {
    if is_bundle(root) {
        if root.exists() && !work.previous().exists() {
            rename(root, &work.previous())?;
        }
        if work.staged().exists() {
            rename(&work.staged(), root)?;
        }
        return Ok(());
    }
    for file in files {
        let installed = root.join(&file.path);
        let saved = work.previous().join(&file.path);
        let staged = work.staged().join(&file.path);
        if !staged.exists() {
            continue;
        }
        if installed.exists() && !saved.exists() {
            rename(&installed, &saved)?;
        }
        rename(&staged, &installed)?;
    }
    Ok(())
}

/// Puts the previous version back.
pub fn roll_back(root: &Path, work: &Work, files: &[ReleaseFile]) -> std::io::Result<()> {
    if is_bundle(root) {
        if work.previous().exists() {
            if root.exists() {
                std::fs::remove_dir_all(root)?;
            }
            rename(&work.previous(), root)?;
        }
        return Ok(());
    }
    for file in files {
        let installed = root.join(&file.path);
        let saved = work.previous().join(&file.path);
        if saved.exists() {
            rename(&saved, &installed)?;
        } else if !work.staged().join(&file.path).exists() && installed.exists() {
            // Added by the new version: the old one did not have it.
            std::fs::remove_file(&installed)?;
        }
    }
    Ok(())
}

/// The service, as the update sees it. Replaced in tests.
pub trait Supervisor {
    fn stop(&self) -> std::io::Result<()>;
    fn start(&self) -> std::io::Result<()>;
    /// The version of the agent answering on the local endpoint.
    fn running_version(&self) -> Option<String>;
    fn wait(&self, duration: Duration);
}

/// How long the new version is given.
#[derive(Debug, Clone, Copy)]
pub struct Timing {
    /// To answer after it starts.
    pub answer_within: Duration,
    /// To still be answering after that: a crash at startup looks healthy
    /// for a moment.
    pub stable_for: Duration,
    pub poll: Duration,
}

impl Timing {
    pub const REAL: Self = Self {
        answer_within: Duration::from_secs(120),
        stable_for: Duration::from_secs(20),
        poll: Duration::from_secs(2),
    };
}

fn answers(supervisor: &impl Supervisor, version: &str) -> bool {
    supervisor.running_version().as_deref() == Some(version)
}

fn is_healthy(supervisor: &impl Supervisor, version: Version, timing: Timing) -> bool {
    let version = version.to_string();
    let deadline = Instant::now() + timing.answer_within;
    while !answers(supervisor, &version) {
        if Instant::now() >= deadline {
            return false;
        }
        supervisor.wait(timing.poll);
    }
    supervisor.wait(timing.stable_for);
    answers(supervisor, &version)
}

#[derive(Debug, PartialEq, Eq)]
pub enum Outcome {
    Installed,
    /// The new version failed: the old one is back and this version is not
    /// tried again.
    RolledBack,
    /// Nothing was replaced.
    NotStarted,
}

/// Runs the update. `failed` is the file that remembers a version that did
/// not work on this machine.
pub fn apply(
    pending: &Pending,
    work: &Work,
    supervisor: &impl Supervisor,
    timing: Timing,
    failed: &Path,
) -> Outcome {
    let Pending {
        version,
        root,
        files,
    } = pending;
    if let Err(error) = verify_staged(&work.staged(), files) {
        tracing::error!(%version, "update refused: {error:#}");
        remember_failure(failed, *version);
        return finish(work, supervisor, Outcome::NotStarted);
    }
    if let Err(error) = supervisor.stop() {
        tracing::error!(%version, "cannot stop the service: {error}");
        return finish(work, supervisor, Outcome::NotStarted);
    }
    let installed = match swap(root, work, files) {
        Ok(()) => match supervisor.start() {
            Ok(()) => is_healthy(supervisor, *version, timing),
            Err(error) => {
                tracing::error!(%version, "the new version does not start: {error}");
                false
            }
        },
        Err(error) => {
            tracing::error!(%version, "replacing the files: {error}");
            false
        }
    };
    if installed {
        tracing::info!(%version, "update installed");
        return finish(work, supervisor, Outcome::Installed);
    }

    tracing::error!(%version, "the new version is not working, going back");
    if let Err(error) = supervisor.stop() {
        tracing::error!("stopping the new version: {error}");
    }
    if let Err(error) = roll_back(root, work, files) {
        tracing::error!("putting the previous version back: {error}");
    }
    remember_failure(failed, *version);
    finish(work, supervisor, Outcome::RolledBack)
}

fn remember_failure(failed: &Path, version: Version) {
    if let Err(error) = banc_platform::fs::write_private(failed, version.to_string().as_bytes()) {
        tracing::error!("remembering the failed version: {error}");
    }
}

/// Makes sure the service runs, and removes what the update left. The copy
/// of the agent stays, it is running: the next check removes it.
fn finish(work: &Work, supervisor: &impl Supervisor, outcome: Outcome) -> Outcome {
    if outcome != Outcome::Installed {
        if let Err(error) = supervisor.start() {
            tracing::debug!("starting the service: {error}");
        }
    }
    let _ = std::fs::remove_dir_all(work.staged());
    let _ = std::fs::remove_dir_all(work.previous());
    let _ = std::fs::remove_file(work.pending());
    outcome
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};

    use super::*;

    const NEW: &[u8] = b"new agent";

    fn file(path: &str, content: &[u8]) -> ReleaseFile {
        ReleaseFile {
            path: path.to_owned(),
            asset: "asset".to_owned(),
            size: content.len() as u64,
            blake3: blake3::hash(content).to_hex().to_string(),
            executable: true,
        }
    }

    fn write(path: &Path, content: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn read(path: &Path) -> Vec<u8> {
        std::fs::read(path).unwrap()
    }

    /// An installed version and a staged one.
    struct Setup {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        work: Work,
        failed: PathBuf,
        files: Vec<ReleaseFile>,
    }

    fn setup(root_name: &str, program: &str) -> Setup {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join(root_name);
        let work = Work::for_root(&root);
        write(&root.join(program), b"old agent");
        write(&work.staged().join(program), NEW);
        Setup {
            failed: tmp.path().join("update-failed"),
            files: vec![file(program, NEW)],
            _tmp: tmp,
            root,
            work,
        }
    }

    impl Setup {
        fn pending(&self) -> Pending {
            Pending {
                version: "1.2.0".parse().unwrap(),
                root: self.root.clone(),
                files: self.files.clone(),
            }
        }
    }

    /// A service whose agent answers with a scripted version.
    #[derive(Default)]
    struct FakeService {
        answers: RefCell<Vec<Option<String>>>,
        fails_to_start: Cell<bool>,
        fails_to_stop: Cell<bool>,
        starts: Cell<u32>,
        stops: Cell<u32>,
    }

    impl FakeService {
        fn answering(versions: &[Option<&str>]) -> Self {
            let service = Self::default();
            let mut answers: Vec<_> = versions
                .iter()
                .map(|version| version.map(str::to_owned))
                .collect();
            answers.reverse();
            *service.answers.borrow_mut() = answers;
            service
        }
    }

    impl Supervisor for FakeService {
        fn stop(&self) -> std::io::Result<()> {
            self.stops.set(self.stops.get() + 1);
            if self.fails_to_stop.get() {
                return Err(std::io::Error::other("refused"));
            }
            Ok(())
        }

        fn start(&self) -> std::io::Result<()> {
            self.starts.set(self.starts.get() + 1);
            if self.fails_to_start.get() {
                return Err(std::io::Error::other("refused"));
            }
            Ok(())
        }

        fn running_version(&self) -> Option<String> {
            self.answers.borrow_mut().pop().flatten()
        }

        fn wait(&self, _duration: Duration) {}
    }

    const FAST: Timing = Timing {
        answer_within: Duration::from_millis(20),
        stable_for: Duration::ZERO,
        poll: Duration::ZERO,
    };

    #[test]
    fn root_of_each_install() {
        assert_eq!(
            install_root(Path::new(
                "/Applications/Banc.app/Contents/MacOS/banc-agent"
            )),
            Some(PathBuf::from("/Applications/Banc.app"))
        );
        assert_eq!(
            install_root(Path::new("/opt/banc/banc-agent")),
            Some(PathBuf::from("/opt/banc"))
        );
    }

    #[test]
    fn work_folder_sits_next_to_the_application() {
        let work = Work::for_root(Path::new("/Applications/Banc.app"));
        assert_eq!(work.dir(), Path::new("/Applications/Banc.app.update"));
    }

    #[test]
    fn a_healthy_bundle_replaces_the_old_one() {
        let s = setup("Banc.app", "Contents/MacOS/banc-agent");
        write(&s.root.join("Contents/stale"), b"from the old version");
        let service = FakeService::answering(&[None, Some("1.2.0"), Some("1.2.0")]);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(read(&s.root.join("Contents/MacOS/banc-agent")), NEW);
        assert!(!s.root.join("Contents/stale").exists());
        assert!(!s.work.previous().exists());
        assert!(!s.work.staged().exists());
        assert!(!s.failed.exists());
        assert_eq!(service.starts.get(), 1);
    }

    #[test]
    fn a_healthy_folder_keeps_the_files_of_the_installer() {
        let s = setup("Banc", "banc-agent.exe");
        write(&s.root.join("uninstall.exe"), b"installer's");
        let service = FakeService::answering(&[Some("1.2.0"), Some("1.2.0")]);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::Installed);
        assert_eq!(read(&s.root.join("banc-agent.exe")), NEW);
        assert_eq!(read(&s.root.join("uninstall.exe")), b"installer's");
    }

    #[test]
    fn a_version_that_never_answers_is_rolled_back() {
        for (root, program) in [
            ("Banc.app", "Contents/MacOS/banc-agent"),
            ("Banc", "banc-agent.exe"),
        ] {
            let s = setup(root, program);
            let service = FakeService::default();
            let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
            assert_eq!(outcome, Outcome::RolledBack);
            assert_eq!(read(&s.root.join(program)), b"old agent");
            assert_eq!(std::fs::read_to_string(&s.failed).unwrap(), "1.2.0");
            // Started for the new version, then for the old one.
            assert_eq!(service.starts.get(), 2);
        }
    }

    #[test]
    fn a_version_that_crashes_after_answering_is_rolled_back() {
        let s = setup("Banc", "banc-agent.exe");
        let service = FakeService::answering(&[Some("1.2.0"), None]);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::RolledBack);
        assert_eq!(read(&s.root.join("banc-agent.exe")), b"old agent");
    }

    #[test]
    fn the_old_version_still_answering_is_not_a_success() {
        let s = setup("Banc", "banc-agent.exe");
        let service = FakeService::answering(&[Some("1.1.0"), Some("1.1.0")]);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::RolledBack);
    }

    #[test]
    fn a_service_that_refuses_to_start_is_rolled_back() {
        let s = setup("Banc.app", "Contents/MacOS/banc-agent");
        let service = FakeService::default();
        service.fails_to_start.set(true);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::RolledBack);
        assert_eq!(
            read(&s.root.join("Contents/MacOS/banc-agent")),
            b"old agent"
        );
    }

    #[test]
    fn a_staged_file_that_changed_is_never_installed() {
        let s = setup("Banc", "banc-agent.exe");
        write(&s.work.staged().join("banc-agent.exe"), b"tampered");
        let service = FakeService::default();
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::NotStarted);
        assert_eq!(read(&s.root.join("banc-agent.exe")), b"old agent");
        assert_eq!(service.stops.get(), 0);
        assert!(!s.work.staged().exists());
    }

    #[test]
    fn a_service_that_cannot_be_stopped_keeps_the_old_files() {
        let s = setup("Banc", "banc-agent.exe");
        let service = FakeService::default();
        service.fails_to_stop.set(true);
        let outcome = apply(&s.pending(), &s.work, &service, FAST, &s.failed);
        assert_eq!(outcome, Outcome::NotStarted);
        assert_eq!(read(&s.root.join("banc-agent.exe")), b"old agent");
        assert!(!s.failed.exists(), "worth trying again later");
    }

    #[test]
    fn an_interrupted_bundle_swap_resumes() {
        let s = setup("Banc.app", "Contents/MacOS/banc-agent");
        // Interrupted between the two moves.
        rename(&s.root, &s.work.previous()).unwrap();
        swap(&s.root, &s.work, &s.files).unwrap();
        assert_eq!(read(&s.root.join("Contents/MacOS/banc-agent")), NEW);
        assert_eq!(
            read(&s.work.previous().join("Contents/MacOS/banc-agent")),
            b"old agent"
        );
        // Run again once finished: nothing moves.
        swap(&s.root, &s.work, &s.files).unwrap();
        assert_eq!(read(&s.root.join("Contents/MacOS/banc-agent")), NEW);
    }

    #[test]
    fn an_interrupted_folder_swap_rolls_back_each_file_correctly() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Banc");
        let work = Work::for_root(&root);
        write(&root.join("moved.exe"), b"old moved");
        write(&root.join("waiting.exe"), b"old waiting");
        write(&work.staged().join("waiting.exe"), b"new waiting");
        write(&work.staged().join("added.exe"), b"new added");
        let files = vec![
            file("moved.exe", b"new moved"),
            file("added.exe", b"new added"),
            file("waiting.exe", b"new waiting"),
        ];
        // `moved.exe` was swapped, and `added.exe` already moved in.
        write(&work.previous().join("moved.exe"), b"old moved");
        write(&root.join("moved.exe"), b"new moved");
        rename(&work.staged().join("added.exe"), &root.join("added.exe")).unwrap();

        roll_back(&root, &work, &files).unwrap();
        assert_eq!(read(&root.join("moved.exe")), b"old moved");
        assert_eq!(read(&root.join("waiting.exe")), b"old waiting");
        assert!(!root.join("added.exe").exists());
    }

    #[test]
    fn pending_round_trip() {
        let s = setup("Banc", "banc-agent.exe");
        let json = serde_json::to_string(&s.pending()).unwrap();
        assert_eq!(serde_json::from_str::<Pending>(&json).unwrap(), s.pending());
    }
}
