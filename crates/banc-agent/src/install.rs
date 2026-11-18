//! Puts a verified package file in place, and takes it away again.
//!
//! Everything here happens after the file's hash was checked against the
//! signed manifest. Banc never replaces what it did not install: a file
//! already there is either the same content, left alone, or a conflict to
//! report.

use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use banc_platform::paths::ResolveDirs;
use banc_proto::manifest::{PackageBuild, PackageKind, Place};

/// An installer that runs longer than this is stopped.
const INSTALLER_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const FONT_EXTENSIONS: [&str; 3] = ["ttf", "otf", "ttc"];

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("`{0}` already exists and was not installed by Banc")]
    Conflict(String),
    #[error("the archive is not usable: {0}")]
    Archive(String),
    #[error("this archive contains no font")]
    NoFont,
    #[error("the installer ended with code {0}")]
    Installer(i32),
    #[error("the installer was stopped by the system")]
    Killed,
    #[error("the installer did not finish in time")]
    TimedOut,
    #[error("this package cannot be installed automatically")]
    NotInstallable,
    #[error("{0}")]
    Io(#[from] std::io::Error),
}

/// Where fonts go.
#[derive(Debug, Clone)]
pub enum Fonts {
    /// The fonts of the system.
    System,
    /// A plain folder, for tests.
    Folder(PathBuf),
}

/// The folders a package can be installed to.
#[derive(Debug, Clone)]
pub struct Places {
    pub resolve: ResolveDirs,
    pub fonts: Fonts,
}

impl Places {
    fn dir(&self, place: Place) -> &Path {
        match place {
            Place::Ofx => &self.resolve.ofx,
            Place::Lut => &self.resolve.lut,
            Place::FusionTemplates => &self.resolve.fusion_templates,
            Place::Scripts => &self.resolve.scripts,
        }
    }

    fn font_path(&self, name: &str) -> std::io::Result<PathBuf> {
        match &self.fonts {
            Fonts::System => banc_platform::fonts::path_of(name),
            Fonts::Folder(folder) => Ok(folder.join(name)),
        }
    }

    fn put_font(&self, source: &Path, name: &str) -> std::io::Result<PathBuf> {
        match &self.fonts {
            Fonts::System => banc_platform::fonts::install(source, name),
            Fonts::Folder(folder) => {
                std::fs::create_dir_all(folder)?;
                let target = folder.join(name);
                std::fs::copy(source, &target)?;
                Ok(target)
            }
        }
    }

    fn take_font(&self, path: &Path) -> std::io::Result<()> {
        match &self.fonts {
            Fonts::System => banc_platform::fonts::remove(path),
            Fonts::Folder(_) => remove_entry(path),
        }
    }
}

/// Installs `file`, the verified download of `build`. `owned` is what Banc
/// installed for the previous version. Returns what Banc now owns.
pub fn install(
    build: &PackageBuild,
    file: &Path,
    file_name: &str,
    work: &Path,
    owned: &[PathBuf],
    places: &Places,
) -> Result<Vec<PathBuf>, InstallError> {
    match (build.kind, build.place) {
        (PackageKind::Copy, Some(place)) => copy_to(file, file_name, places.dir(place), owned),
        (PackageKind::Font, _) => install_fonts(file, file_name, work, owned, places),
        (PackageKind::Manual | PackageKind::Copy, _) => Err(InstallError::NotInstallable),
        (kind, _) => {
            let invocation =
                invocation(kind, file, &build.args).ok_or(InstallError::NotInstallable)?;
            if kind == PackageKind::Run {
                banc_platform::fs::set_installed_mode(file, true)?;
            }
            invocation.run(INSTALLER_TIMEOUT)?;
            Ok(Vec::new())
        }
    }
}

/// Removes what Banc installed for a package.
pub fn remove(
    kind: Option<PackageKind>,
    files: &[PathBuf],
    places: &Places,
) -> Result<(), InstallError> {
    for file in files {
        if kind == Some(PackageKind::Font) {
            places.take_font(file)?;
        } else {
            remove_entry(file)?;
        }
    }
    Ok(())
}

fn remove_entry(path: &Path) -> std::io::Result<()> {
    let result = match std::fs::symlink_metadata(path) {
        Ok(found) if found.is_dir() => std::fs::remove_dir_all(path),
        Ok(_) => std::fs::remove_file(path),
        Err(error) => Err(error),
    };
    match result {
        Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

fn is_zip(name: &str) -> bool {
    Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

fn same_content(a: &Path, b: &Path) -> std::io::Result<bool> {
    let hash = |path: &Path| -> std::io::Result<blake3::Hash> {
        let mut hasher = blake3::Hasher::new();
        std::io::copy(&mut File::open(path)?, &mut hasher)?;
        Ok(hasher.finalize())
    };
    Ok(std::fs::metadata(a)?.len() == std::fs::metadata(b)?.len() && hash(a)? == hash(b)?)
}

/// Copies a file, or the content of an archive, into `dir`. Nothing changes
/// in `dir` until everything is unpacked, next to its final place. If a step
/// fails, the previous version is put back.
fn copy_to(
    file: &Path,
    file_name: &str,
    dir: &Path,
    owned: &[PathBuf],
) -> Result<Vec<PathBuf>, InstallError> {
    std::fs::create_dir_all(dir)?;
    let staging = dir.join(".banc-new");
    remove_entry(&staging)?;
    std::fs::create_dir_all(&staging)?;
    let result = place_staged(file, file_name, dir, &staging, owned);
    let _ = remove_entry(&staging);
    result
}

fn place_staged(
    file: &Path,
    file_name: &str,
    dir: &Path,
    staging: &Path,
    owned: &[PathBuf],
) -> Result<Vec<PathBuf>, InstallError> {
    let new = staging.join("new");
    std::fs::create_dir_all(&new)?;
    if is_zip(file_name) {
        unpack(file, &new)?;
    } else {
        let target = new.join(file_name);
        std::fs::copy(file, &target)?;
        banc_platform::fs::set_installed_mode(&target, false)?;
    }

    let owned_set: HashSet<&Path> = owned.iter().map(PathBuf::as_path).collect();
    let mut moves = Vec::new();
    for entry in std::fs::read_dir(&new)? {
        let entry = entry?;
        let target = dir.join(entry.file_name());
        let free = std::fs::symlink_metadata(&target).is_err();
        if free || owned_set.contains(target.as_path()) {
            moves.push((entry.path(), target));
        } else if entry.path().is_file()
            && target.is_file()
            && same_content(&entry.path(), &target)?
        {
            // Already there with this content: it stays someone else's.
        } else {
            return Err(InstallError::Conflict(
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
    }

    // The previous version steps aside, and comes back if anything fails.
    let old = staging.join("old");
    std::fs::create_dir_all(&old)?;
    let mut aside = Vec::new();
    let mut placed = Vec::new();
    let mut step = || -> std::io::Result<()> {
        for (index, path) in owned.iter().enumerate() {
            if std::fs::symlink_metadata(path).is_ok() {
                let parked = old.join(index.to_string());
                std::fs::rename(path, &parked)?;
                aside.push((parked, path.clone()));
            }
        }
        for (from, to) in &moves {
            std::fs::rename(from, to)?;
            placed.push(to.clone());
        }
        Ok(())
    };
    if let Err(error) = step() {
        for path in &placed {
            let _ = remove_entry(path);
        }
        for (parked, path) in &aside {
            let _ = std::fs::rename(parked, path);
        }
        return Err(error.into());
    }
    Ok(placed)
}

/// Unpacks an archive into `dir`. An entry that would land outside, or a
/// link pointing outside, refuses the whole archive.
fn unpack(archive: &Path, dir: &Path) -> Result<(), InstallError> {
    let bad = |error: &dyn std::fmt::Display| InstallError::Archive(error.to_string());
    let mut archive = zip::ZipArchive::new(File::open(archive)?).map_err(|e| bad(&e))?;
    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|e| bad(&e))?;
        let name = entry.name().map_err(|e| bad(&e))?.into_owned();
        let relative = entry
            .enclosed_name()
            .ok_or_else(|| InstallError::Archive(format!("unsafe path `{name}`")))?;
        if is_archive_noise(&relative) {
            continue;
        }
        let target = dir.join(&relative);
        if entry.is_dir() {
            std::fs::create_dir_all(&target)?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if entry.is_symlink() {
            let mut link = String::new();
            std::io::Read::read_to_string(&mut entry, &mut link)?;
            if !link_stays_inside(&relative, Path::new(&link)) {
                return Err(InstallError::Archive(format!("unsafe link `{name}`")));
            }
            banc_platform::fs::symlink(Path::new(&link), &target)?;
            continue;
        }
        let executable = entry.unix_mode().is_some_and(|mode| mode & 0o111 != 0);
        std::io::copy(&mut entry, &mut File::create(&target)?)?;
        banc_platform::fs::set_installed_mode(&target, executable)?;
    }
    Ok(())
}

/// What macOS adds to the archives it makes.
fn is_archive_noise(path: &Path) -> bool {
    path.components().any(
        |part| matches!(part, Component::Normal(name) if name == "__MACOSX" || name == ".DS_Store"),
    )
}

/// Whether a relative link found at `entry` stays inside the archive.
fn link_stays_inside(entry: &Path, link: &Path) -> bool {
    let mut depth = entry.components().count().saturating_sub(1);
    for part in link.components() {
        match part {
            Component::Normal(_) => depth += 1,
            Component::CurDir => {}
            Component::ParentDir if depth > 0 => depth -= 1,
            _ => return false,
        }
    }
    true
}

fn font_files(dir: &Path, found: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            font_files(&path, found)?;
        } else if is_font(&path) {
            found.push(path);
        }
    }
    Ok(())
}

fn is_font(path: &Path) -> bool {
    path.extension().is_some_and(|extension| {
        FONT_EXTENSIONS
            .iter()
            .any(|font| extension.eq_ignore_ascii_case(font))
    })
}

fn install_fonts(
    file: &Path,
    file_name: &str,
    work: &Path,
    owned: &[PathBuf],
    places: &Places,
) -> Result<Vec<PathBuf>, InstallError> {
    let unpacked = work.join("fonts");
    remove_entry(&unpacked)?;
    std::fs::create_dir_all(&unpacked)?;
    let result = (|| {
        let mut fonts = Vec::new();
        if is_zip(file_name) {
            unpack(file, &unpacked)?;
            font_files(&unpacked, &mut fonts)?;
        } else {
            let single = unpacked.join(file_name);
            std::fs::copy(file, &single)?;
            if is_font(&single) {
                fonts.push(single);
            }
        }
        if fonts.is_empty() {
            return Err(InstallError::NoFont);
        }
        place_fonts(&fonts, owned, places)
    })();
    let _ = remove_entry(&unpacked);
    result
}

fn place_fonts(
    fonts: &[PathBuf],
    owned: &[PathBuf],
    places: &Places,
) -> Result<Vec<PathBuf>, InstallError> {
    // Everything is checked before the first font changes.
    let mut wanted = Vec::new();
    for font in fonts {
        let name = font
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .ok_or(InstallError::NoFont)?;
        let target = places.font_path(&name)?;
        let mine = owned.contains(&target);
        if target.exists() && !mine {
            if same_content(font, &target)? {
                continue;
            }
            return Err(InstallError::Conflict(name));
        }
        wanted.push((font, name, target));
    }
    let targets: Vec<&PathBuf> = wanted.iter().map(|(_, _, target)| target).collect();
    for path in owned {
        if !targets.contains(&path) {
            places.take_font(path)?;
        }
    }
    let mut placed = Vec::new();
    for (font, name, _) in &wanted {
        placed.push(places.put_font(font, name)?);
    }
    Ok(placed)
}

/// How an installer is started.
#[derive(Debug, PartialEq, Eq)]
pub struct Invocation {
    program: OsString,
    args: Vec<OsString>,
    /// Exit codes that mean success.
    success: &'static [i32],
}

/// Success, and success with a restart to come, for `msiexec`.
const MSI_SUCCESS: &[i32] = &[0, 1641, 3010];

/// The command that installs `file`, without any window. `args` come from
/// the signed manifest.
pub fn invocation(kind: PackageKind, file: &Path, args: &[String]) -> Option<Invocation> {
    let text = |values: &[&str]| values.iter().map(OsString::from).collect::<Vec<_>>();
    let file = file.as_os_str().to_owned();
    let (program, arguments, success): (OsString, Vec<OsString>, &'static [i32]) = match kind {
        PackageKind::Pkg => {
            let mut arguments = text(&["-pkg"]);
            arguments.push(file);
            arguments.extend(text(&["-target", "/"]));
            ("/usr/sbin/installer".into(), arguments, &[0])
        }
        PackageKind::Msi => {
            let mut arguments = text(&["/i"]);
            arguments.push(file);
            arguments.extend(text(&["/qn", "/norestart"]));
            ("msiexec".into(), arguments, MSI_SUCCESS)
        }
        PackageKind::Deb => {
            let mut arguments = text(&["--install"]);
            arguments.push(file);
            ("dpkg".into(), arguments, &[0])
        }
        PackageKind::Rpm => {
            let mut arguments = text(&["--upgrade", "--replacepkgs"]);
            arguments.push(file);
            ("rpm".into(), arguments, &[0])
        }
        PackageKind::Exe | PackageKind::Run => {
            (file, args.iter().map(OsString::from).collect(), &[0])
        }
        PackageKind::Copy | PackageKind::Font | PackageKind::Manual => return None,
    };
    Some(Invocation {
        program,
        args: arguments,
        success,
    })
}

impl Invocation {
    fn run(&self, limit: Duration) -> Result<(), InstallError> {
        let mut child = Command::new(&self.program)
            .args(&self.args)
            .env("DEBIAN_FRONTEND", "noninteractive")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        let start = Instant::now();
        loop {
            if let Some(status) = child.try_wait()? {
                return match status.code() {
                    Some(code) if self.success.contains(&code) => Ok(()),
                    Some(code) => Err(InstallError::Installer(code)),
                    None => Err(InstallError::Killed),
                };
            }
            if start.elapsed() > limit {
                let _ = child.kill();
                let _ = child.wait();
                return Err(InstallError::TimedOut);
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use banc_proto::machine::Os;
    use banc_proto::manifest::PackageFile;
    use zip::write::SimpleFileOptions;

    use super::*;

    struct Bench {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        places: Places,
    }

    fn bench() -> Bench {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().to_path_buf();
        let places = Places {
            resolve: ResolveDirs {
                ofx: root.join("ofx"),
                lut: root.join("lut"),
                fusion_templates: root.join("templates"),
                scripts: root.join("scripts"),
            },
            fonts: Fonts::Folder(root.join("fonts")),
        };
        std::fs::create_dir_all(root.join("work")).unwrap();
        Bench {
            _tmp: tmp,
            root,
            places,
        }
    }

    fn build(kind: PackageKind, place: Option<Place>) -> PackageBuild {
        PackageBuild {
            os: Os::Linux,
            arch: None,
            kind,
            file: Some(PackageFile {
                blake3: "ab".repeat(32),
                size: 1,
                name: "file".to_owned(),
            }),
            place,
            args: Vec::new(),
            instructions: None,
        }
    }

    impl Bench {
        fn write(&self, name: &str, content: &[u8]) -> PathBuf {
            let path = self.root.join("work").join(name);
            std::fs::write(&path, content).unwrap();
            path
        }

        fn zip(&self, name: &str, entries: &[(&str, &[u8])]) -> PathBuf {
            let path = self.root.join("work").join(name);
            let mut zip = zip::ZipWriter::new(File::create(&path).unwrap());
            for (entry, content) in entries {
                zip.start_file(*entry, SimpleFileOptions::default())
                    .unwrap();
                zip.write_all(content).unwrap();
            }
            zip.finish().unwrap();
            path
        }

        fn install(
            &self,
            build: &PackageBuild,
            file: &Path,
            owned: &[PathBuf],
        ) -> Result<Vec<PathBuf>, InstallError> {
            let name = file.file_name().unwrap().to_str().unwrap().to_owned();
            install(
                build,
                file,
                &name,
                &self.root.join("work"),
                owned,
                &self.places,
            )
        }
    }

    fn ofx() -> PackageBuild {
        build(PackageKind::Copy, Some(Place::Ofx))
    }

    #[test]
    fn a_bundle_is_unpacked_into_the_plugin_folder() {
        let bench = bench();
        let archive = bench.zip(
            "plugin.zip",
            &[
                ("Glow.ofx.bundle/Contents/Info.plist", b"plist"),
                ("Glow.ofx.bundle/Contents/MacOS/Glow.ofx", b"binary"),
                ("__MACOSX/Glow.ofx.bundle/._Info.plist", b"noise"),
            ],
        );
        let owned = bench.install(&ofx(), &archive, &[]).unwrap();
        let bundle = bench.root.join("ofx/Glow.ofx.bundle");
        assert_eq!(owned, std::slice::from_ref(&bundle));
        assert_eq!(
            std::fs::read(bundle.join("Contents/MacOS/Glow.ofx")).unwrap(),
            b"binary"
        );
        assert!(!bench.root.join("ofx/__MACOSX").exists());
        assert!(!bench.root.join("ofx/.banc-new").exists(), "nothing left");
    }

    #[test]
    fn a_single_file_is_copied_under_its_name() {
        let bench = bench();
        let lut = bench.write("Team.cube", b"lut");
        let owned = bench
            .install(&build(PackageKind::Copy, Some(Place::Lut)), &lut, &[])
            .unwrap();
        assert_eq!(owned, [bench.root.join("lut/Team.cube")]);
        assert_eq!(std::fs::read(&owned[0]).unwrap(), b"lut");
    }

    #[test]
    fn a_new_version_replaces_the_previous_one() {
        let bench = bench();
        let first = bench.zip("v1.zip", &[("Old.ofx.bundle/a", b"1")]);
        let owned = bench.install(&ofx(), &first, &[]).unwrap();
        let second = bench.zip("v2.zip", &[("New.ofx.bundle/a", b"2")]);
        let owned = bench.install(&ofx(), &second, &owned).unwrap();
        assert_eq!(owned, [bench.root.join("ofx/New.ofx.bundle")]);
        assert!(!bench.root.join("ofx/Old.ofx.bundle").exists());
    }

    #[test]
    fn what_someone_else_installed_is_never_replaced() {
        let bench = bench();
        let theirs = bench.root.join("ofx/Glow.ofx.bundle");
        std::fs::create_dir_all(&theirs).unwrap();
        std::fs::write(theirs.join("mine"), b"theirs").unwrap();
        let archive = bench.zip("plugin.zip", &[("Glow.ofx.bundle/a", b"new")]);
        let error = bench.install(&ofx(), &archive, &[]).unwrap_err();
        assert!(matches!(error, InstallError::Conflict(_)), "{error}");
        assert_eq!(std::fs::read(theirs.join("mine")).unwrap(), b"theirs");
    }

    #[test]
    fn the_same_file_already_there_is_left_alone_and_not_owned() {
        let bench = bench();
        std::fs::create_dir_all(bench.root.join("lut")).unwrap();
        std::fs::write(bench.root.join("lut/Team.cube"), b"lut").unwrap();
        let lut = bench.write("Team.cube", b"lut");
        let owned = bench
            .install(&build(PackageKind::Copy, Some(Place::Lut)), &lut, &[])
            .unwrap();
        assert_eq!(owned, Vec::<PathBuf>::new());
    }

    #[test]
    fn an_archive_cannot_write_outside_its_folder() {
        let bench = bench();
        let archive = bench.zip("evil.zip", &[("../../evil", b"x")]);
        let error = bench.install(&ofx(), &archive, &[]).unwrap_err();
        assert!(matches!(error, InstallError::Archive(_)), "{error}");
        assert!(!bench.root.join("evil").exists());
    }

    #[test]
    fn links_stay_inside_the_archive() {
        let entry = Path::new("A.bundle/Contents/Frameworks/Current");
        assert!(link_stays_inside(entry, Path::new("Versions/A")));
        assert!(link_stays_inside(entry, Path::new("../Resources")));
        assert!(!link_stays_inside(entry, Path::new("../../../../etc")));
        assert!(!link_stays_inside(entry, Path::new("/etc/passwd")));
    }

    #[test]
    fn removing_takes_away_what_banc_installed_only() {
        let bench = bench();
        let archive = bench.zip("plugin.zip", &[("Glow.ofx.bundle/a", b"1")]);
        let owned = bench.install(&ofx(), &archive, &[]).unwrap();
        std::fs::write(bench.root.join("ofx/Other.ofx"), b"theirs").unwrap();
        remove(Some(PackageKind::Copy), &owned, &bench.places).unwrap();
        assert!(!owned[0].exists());
        assert!(bench.root.join("ofx/Other.ofx").exists());
        remove(Some(PackageKind::Copy), &owned, &bench.places).unwrap();
    }

    #[test]
    fn fonts_are_installed_from_a_file_or_an_archive() {
        let bench = bench();
        let font = build(PackageKind::Font, None);
        let single = bench.write("Inter.ttf", b"font");
        let owned = bench.install(&font, &single, &[]).unwrap();
        assert_eq!(owned, [bench.root.join("fonts/Inter.ttf")]);

        let family = bench.zip(
            "family.zip",
            &[("static/Mono-Bold.otf", b"bold"), ("OFL.txt", b"licence")],
        );
        let owned = bench.install(&font, &family, &[]).unwrap();
        assert_eq!(owned, [bench.root.join("fonts/Mono-Bold.otf")]);
        assert!(!bench.root.join("fonts/OFL.txt").exists());
    }

    #[test]
    fn a_font_upgrade_removes_the_files_it_no_longer_has() {
        let bench = bench();
        let font = build(PackageKind::Font, None);
        let first = bench.zip("v1.zip", &[("A.ttf", b"a"), ("B.ttf", b"b")]);
        let owned = bench.install(&font, &first, &[]).unwrap();
        let second = bench.zip("v2.zip", &[("A.ttf", b"a2")]);
        let owned = bench.install(&font, &second, &owned).unwrap();
        assert_eq!(owned, [bench.root.join("fonts/A.ttf")]);
        assert_eq!(std::fs::read(&owned[0]).unwrap(), b"a2");
        assert!(!bench.root.join("fonts/B.ttf").exists());
    }

    #[test]
    fn someone_elses_different_font_is_a_conflict() {
        let bench = bench();
        std::fs::create_dir_all(bench.root.join("fonts")).unwrap();
        std::fs::write(bench.root.join("fonts/Inter.ttf"), b"theirs").unwrap();
        let font = bench.write("Inter.ttf", b"ours");
        let error = bench
            .install(&build(PackageKind::Font, None), &font, &[])
            .unwrap_err();
        assert!(matches!(error, InstallError::Conflict(_)), "{error}");
        assert_eq!(
            std::fs::read(bench.root.join("fonts/Inter.ttf")).unwrap(),
            b"theirs"
        );
    }

    #[test]
    fn an_archive_without_a_font_is_refused() {
        let bench = bench();
        let archive = bench.zip("empty.zip", &[("readme.txt", b"x")]);
        let error = bench
            .install(&build(PackageKind::Font, None), &archive, &[])
            .unwrap_err();
        assert!(matches!(error, InstallError::NoFont), "{error}");
    }

    fn command(kind: PackageKind, args: &[&str]) -> (String, Vec<String>) {
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let found = invocation(kind, Path::new("/tmp/file"), &args).unwrap();
        (
            found.program.to_string_lossy().into_owned(),
            found
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        )
    }

    #[test]
    fn installers_run_without_a_window() {
        assert_eq!(
            command(PackageKind::Pkg, &[]),
            (
                "/usr/sbin/installer".to_owned(),
                vec![
                    "-pkg".to_owned(),
                    "/tmp/file".to_owned(),
                    "-target".to_owned(),
                    "/".to_owned()
                ]
            )
        );
        let (program, args) = command(PackageKind::Msi, &[]);
        assert_eq!(program, "msiexec");
        assert_eq!(args, ["/i", "/tmp/file", "/qn", "/norestart"]);
        assert_eq!(command(PackageKind::Deb, &[]).0, "dpkg");
        assert_eq!(command(PackageKind::Rpm, &[]).0, "rpm");
    }

    #[test]
    fn signed_arguments_reach_the_installer() {
        let (program, args) = command(PackageKind::Exe, &["/S", "/NORESTART"]);
        assert_eq!(program, "/tmp/file");
        assert_eq!(args, ["/S", "/NORESTART"]);
    }

    #[test]
    fn copies_have_no_installer() {
        for kind in [PackageKind::Copy, PackageKind::Font, PackageKind::Manual] {
            assert_eq!(invocation(kind, Path::new("x"), &[]), None);
        }
    }

    #[test]
    fn exit_codes_decide() {
        let run = |program: &str, args: &[&str], success| {
            Invocation {
                program: program.into(),
                args: args.iter().map(OsString::from).collect(),
                success,
            }
            .run(Duration::from_secs(20))
        };
        let exe = std::env::current_exe().unwrap();
        let exe = exe.to_str().unwrap();
        // The test program itself: listing its tests succeeds.
        run(exe, &["--list"], &[0]).unwrap();
        assert!(matches!(
            run(exe, &["--not-an-option"], &[0]),
            Err(InstallError::Installer(_))
        ));
        run(exe, &["--not-an-option"], &[0, 101]).unwrap();
    }
}
