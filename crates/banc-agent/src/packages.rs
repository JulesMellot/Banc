//! Compares what is on this workstation with what the manifest asks for.
//!
//! Pure decisions, without touching the machine: the heart of the product,
//! so every rule has its test. Banc only ever removes what it installed
//! itself.

use std::collections::BTreeMap;

use banc_proto::machine::Os;
use banc_proto::manifest::{Manifest, Package, PackageBuild, PackageKind};

/// A package present on this workstation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalPackage {
    pub version: String,
    /// BLAKE3 of the file Banc installed it from. `None` when the package
    /// was found on the machine, installed by someone else.
    pub installed_from: Option<String>,
}

impl LocalPackage {
    fn installed_by_banc(&self) -> bool {
        self.installed_from.is_some()
    }

    /// Whether this is what the build asks for. A package installed by
    /// someone else has no hash to compare: its version decides.
    fn matches(&self, package: &Package, build: &PackageBuild) -> bool {
        if self.version != package.version {
            return false;
        }
        match (&self.installed_from, &build.file) {
            (Some(hash), Some(file)) => *hash == file.blake3,
            _ => true,
        }
    }
}

/// The workstation's packages, by identifier.
pub type LocalState = BTreeMap<String, LocalPackage>;

/// One thing to do on this workstation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Installs a package that Banc does not manage yet.
    Install { id: String },
    /// Replaces a version that Banc installed.
    Upgrade { id: String, from: String },
    /// Removes a package that Banc installed and the manifest dropped.
    Remove { id: String },
    /// A person has to install it: Banc shows the instructions.
    Manual { id: String },
}

impl Action {
    /// Whether doing it changes files that Resolve may be using.
    fn needs_resolve_closed(&self) -> bool {
        !matches!(self, Self::Manual { .. })
    }
}

/// What the manifest means for this workstation.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Plan {
    /// Removals first, then the manifest's order.
    pub actions: Vec<Action>,
    /// Required packages without a build for this machine.
    pub unavailable: Vec<String>,
    /// The Resolve version is not the required one: projects stay closed.
    pub resolve_mismatch: bool,
}

impl Plan {
    /// What can be done now. Nothing is installed while Resolve is open.
    pub fn runnable(&self, resolve_open: bool) -> Vec<&Action> {
        self.actions
            .iter()
            .filter(|action| !resolve_open || !action.needs_resolve_closed())
            .collect()
    }
}

/// The machine the plan is for.
#[derive(Debug, Clone, Copy)]
pub struct Machine<'a> {
    pub os: Os,
    pub arch: &'a str,
    /// The installed Resolve version, if any.
    pub resolve: Option<&'a str>,
}

/// Decides what to do for `manifest` on this machine.
pub fn plan(manifest: &Manifest, machine: Machine<'_>, local: &LocalState) -> Plan {
    let mut plan = Plan {
        resolve_mismatch: manifest.resolve.as_ref().is_some_and(|required| {
            !machine
                .resolve
                .is_some_and(|installed| required.accepts(installed))
        }),
        ..Plan::default()
    };
    for (id, package) in local {
        let listed = manifest.packages.iter().any(|wanted| wanted.id == *id);
        if !listed && package.installed_by_banc() {
            plan.actions.push(Action::Remove { id: id.clone() });
        }
    }
    for package in &manifest.packages {
        match package.build_for(machine.os, machine.arch) {
            Some(build) => plan.actions.extend(action_for(package, build, local)),
            None if package.required => plan.unavailable.push(package.id.clone()),
            None => {}
        }
    }
    plan
}

fn action_for(package: &Package, build: &PackageBuild, local: &LocalState) -> Option<Action> {
    let id = package.id.clone();
    let present = local.get(&package.id);
    if present.is_some_and(|found| found.matches(package, build)) {
        return None;
    }
    // An optional package is never imposed, only kept up to date.
    if present.is_none() && !package.required {
        return None;
    }
    if build.kind == PackageKind::Manual {
        return Some(Action::Manual { id });
    }
    Some(match present {
        Some(found) if found.installed_by_banc() => Action::Upgrade {
            id,
            from: found.version.clone(),
        },
        // Someone else's install is left in place: only its owner removes it.
        _ => Action::Install { id },
    })
}

#[cfg(test)]
mod tests {
    use banc_proto::manifest::{PackageFile, RequiredResolve};

    use super::*;

    const HASH: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OTHER_HASH: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const MAC: Machine<'static> = Machine {
        os: Os::Macos,
        arch: "aarch64",
        resolve: Some("21.1.0"),
    };

    fn package(id: &str, kind: PackageKind) -> Package {
        Package {
            id: id.to_owned(),
            version: "2.0".to_owned(),
            required: true,
            builds: vec![PackageBuild {
                os: Os::Macos,
                arch: None,
                kind,
                file: Some(PackageFile {
                    blake3: HASH.to_owned(),
                    size: 1,
                }),
            }],
        }
    }

    fn manifest(packages: Vec<Package>) -> Manifest {
        Manifest {
            version: 1,
            resolve: None,
            packages,
        }
    }

    fn by_banc(version: &str, hash: &str) -> LocalPackage {
        LocalPackage {
            version: version.to_owned(),
            installed_from: Some(hash.to_owned()),
        }
    }

    fn by_someone_else(version: &str) -> LocalPackage {
        LocalPackage {
            version: version.to_owned(),
            installed_from: None,
        }
    }

    fn local(packages: &[(&str, LocalPackage)]) -> LocalState {
        packages
            .iter()
            .map(|(id, package)| ((*id).to_owned(), package.clone()))
            .collect()
    }

    fn actions(manifest: &Manifest, state: &LocalState) -> Vec<Action> {
        plan(manifest, MAC, state).actions
    }

    fn install(id: &str) -> Action {
        Action::Install { id: id.to_owned() }
    }

    #[test]
    fn a_missing_required_package_is_installed() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        assert_eq!(actions(&wanted, &LocalState::new()), [install("font")]);
    }

    #[test]
    fn an_up_to_date_workstation_has_nothing_to_do() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        let state = local(&[("font", by_banc("2.0", HASH))]);
        assert_eq!(plan(&wanted, MAC, &state), Plan::default());
    }

    #[test]
    fn a_new_version_replaces_what_banc_installed() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        let state = local(&[("font", by_banc("1.0", OTHER_HASH))]);
        assert_eq!(
            actions(&wanted, &state),
            [Action::Upgrade {
                id: "font".to_owned(),
                from: "1.0".to_owned()
            }]
        );
    }

    #[test]
    fn the_same_version_from_another_file_is_replaced() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        let state = local(&[("font", by_banc("2.0", OTHER_HASH))]);
        assert!(matches!(
            actions(&wanted, &state)[..],
            [Action::Upgrade { .. }]
        ));
    }

    #[test]
    fn the_right_version_installed_by_someone_else_is_accepted() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        let state = local(&[("font", by_someone_else("2.0"))]);
        assert_eq!(actions(&wanted, &state), []);
    }

    #[test]
    fn an_old_version_from_someone_else_is_installed_over_never_upgraded() {
        let wanted = manifest(vec![package("font", PackageKind::Font)]);
        let state = local(&[("font", by_someone_else("1.0"))]);
        assert_eq!(actions(&wanted, &state), [install("font")]);
    }

    #[test]
    fn a_package_dropped_from_the_manifest_is_removed_if_banc_installed_it() {
        let state = local(&[("old", by_banc("1.0", HASH))]);
        assert_eq!(
            actions(&manifest(vec![]), &state),
            [Action::Remove {
                id: "old".to_owned()
            }]
        );
    }

    #[test]
    fn what_banc_did_not_install_is_never_removed() {
        let state = local(&[("theirs", by_someone_else("1.0"))]);
        assert_eq!(actions(&manifest(vec![]), &state), []);
    }

    #[test]
    fn removals_come_first_then_the_order_of_the_manifest() {
        let wanted = manifest(vec![
            package("zebra", PackageKind::Font),
            package("alpha", PackageKind::Font),
        ]);
        let state = local(&[("old", by_banc("1.0", HASH))]);
        assert_eq!(
            actions(&wanted, &state),
            [
                Action::Remove {
                    id: "old".to_owned()
                },
                install("zebra"),
                install("alpha"),
            ]
        );
    }

    #[test]
    fn an_optional_package_is_not_imposed_but_stays_up_to_date() {
        let mut optional = package("extra", PackageKind::Font);
        optional.required = false;
        let wanted = manifest(vec![optional]);
        assert_eq!(actions(&wanted, &LocalState::new()), []);
        let state = local(&[("extra", by_banc("1.0", OTHER_HASH))]);
        assert!(matches!(
            actions(&wanted, &state)[..],
            [Action::Upgrade { .. }]
        ));
    }

    #[test]
    fn a_manual_package_is_left_to_a_person() {
        let wanted = manifest(vec![package("licence", PackageKind::Manual)]);
        assert_eq!(
            actions(&wanted, &LocalState::new()),
            [Action::Manual {
                id: "licence".to_owned()
            }]
        );
    }

    #[test]
    fn a_required_package_without_a_build_here_is_reported() {
        let wanted = manifest(vec![package("mac-only", PackageKind::Font)]);
        let windows = Machine {
            os: Os::Windows,
            arch: "x86_64",
            resolve: None,
        };
        let result = plan(&wanted, windows, &LocalState::new());
        assert_eq!(result.actions, []);
        assert_eq!(result.unavailable, ["mac-only"]);
    }

    #[test]
    fn an_optional_package_without_a_build_here_is_ignored() {
        let mut optional = package("mac-only", PackageKind::Font);
        optional.required = false;
        let windows = Machine {
            os: Os::Windows,
            arch: "x86_64",
            resolve: None,
        };
        let result = plan(&manifest(vec![optional]), windows, &LocalState::new());
        assert_eq!(result, Plan::default());
    }

    #[test]
    fn nothing_is_installed_while_resolve_is_open() {
        let wanted = manifest(vec![
            package("font", PackageKind::Font),
            package("licence", PackageKind::Manual),
        ]);
        let state = local(&[("old", by_banc("1.0", HASH))]);
        let result = plan(&wanted, MAC, &state);
        assert_eq!(result.runnable(false).len(), 3);
        assert_eq!(
            result.runnable(true),
            [&Action::Manual {
                id: "licence".to_owned()
            }]
        );
    }

    #[test]
    fn the_resolve_version_is_compared() {
        let mut wanted = manifest(vec![]);
        let check = |wanted: &Manifest, resolve| {
            let machine = Machine { resolve, ..MAC };
            plan(wanted, machine, &LocalState::new()).resolve_mismatch
        };
        assert!(!check(&wanted, None), "no version chosen by the team");

        wanted.resolve = Some(RequiredResolve {
            version: "21.1".to_owned(),
        });
        assert!(!check(&wanted, Some("21.1.0")));
        assert!(check(&wanted, Some("21.0.4")));
        assert!(check(&wanted, None), "Resolve is not installed");
    }
}
