//! `cargo xtask check`: the `ci.yml` checks, one per step.

use std::path::Path;
use std::time::Instant;

use anyhow::{bail, Context};
use clap::ValueEnum;

use crate::cmd::{cargo, output, run, say, tool};
use crate::{architecture, commits};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Step {
    /// cargo fmt --check
    Fmt,
    /// No OS-specific code outside banc-platform
    Architecture,
    /// cargo clippy, warnings refused
    Clippy,
    /// cargo nextest and doc tests
    Tests,
    /// Migrations applied to an empty database, schema compared with schema.sql
    Migrations,
    /// Frontend: lint, svelte-check, vitest, build
    Frontend,
    /// Python scripts for the Resolve bridge: ruff and compilation
    Python,
    /// cargo deny: vulnerabilities, licenses, duplicates, sources
    Deny,
    /// gitleaks on the branch's commits and on uncommitted changes
    Secrets,
    /// Commit message convention for the branch
    Commits,
}

pub fn run_steps(root: &Path, steps: &[Step], base: &str) -> anyhow::Result<()> {
    let steps = if steps.is_empty() {
        Step::value_variants()
    } else {
        steps
    };
    for step in steps {
        let name = step
            .to_possible_value()
            .map(|value| value.get_name().to_owned())
            .unwrap_or_default();
        say(&format!("\n=== {name} ==="));
        let start = Instant::now();
        run_step(root, *step, base).with_context(|| format!("step `{name}`"))?;
        say(&format!("=== {name}: ok ({:.0?}) ===", start.elapsed()));
    }
    say("\nAll requested checks pass.");
    Ok(())
}

fn run_step(root: &Path, step: Step, base: &str) -> anyhow::Result<()> {
    match step {
        Step::Fmt => run(cargo(&["fmt", "--all", "--check"]).current_dir(root)),
        Step::Architecture => {
            let violations = architecture::scan(root)?;
            for violation in &violations {
                say(&format!("  {violation}"));
            }
            if !violations.is_empty() {
                bail!(
                    "OS-specific code outside banc-platform ({} line(s))",
                    violations.len()
                );
            }
            Ok(())
        }
        Step::Clippy => {
            let args = ["clippy", "--workspace", "--all-targets", "--locked"];
            run(cargo(&args)
                .args(["--", "-D", "warnings"])
                .current_dir(root))
        }
        Step::Tests => {
            run(cargo(&["nextest", "run", "--workspace", "--locked"]).current_dir(root))?;
            run(cargo(&["test", "--workspace", "--doc", "--locked"]).current_dir(root))
        }
        Step::Migrations => {
            let args = [
                "test",
                "--locked",
                "-p",
                "banc-server",
                "--test",
                "migrations",
            ];
            run(cargo(&args).current_dir(root))
        }
        Step::Frontend => {
            let ui = root.join("ui");
            for args in [
                &["install", "--frozen-lockfile"][..],
                &["lint"],
                &["check"],
                &["test"],
                &["build"],
            ] {
                run(tool("pnpm").args(args).current_dir(&ui))?;
            }
            Ok(())
        }
        Step::Python => python(root),
        Step::Deny => run(cargo(&["deny", "--locked", "check"]).current_dir(root)),
        Step::Secrets => {
            let range = format!("{}..HEAD", base_ref(root, base)?);
            let common = ["git", "--no-banner", "--redact"];
            run(tool("gitleaks")
                .args(common)
                .arg(format!("--log-opts={range}"))
                .current_dir(root))?;
            // Uncommitted changes: staged, then unstaged.
            run(tool("gitleaks")
                .args(common)
                .args(["--pre-commit", "--staged"])
                .current_dir(root))?;
            run(tool("gitleaks")
                .args(common)
                .arg("--pre-commit")
                .current_dir(root))
        }
        Step::Commits => commit_messages(root, base),
    }
}

const PYTHON_DIR: &str = "resolve-scripts";
const COMPILE: &str =
    "import sys; compile(open(sys.argv[1], encoding='utf-8').read(), sys.argv[1], 'exec')";

fn python(root: &Path) -> anyhow::Result<()> {
    run(tool("ruff").args(["check", PYTHON_DIR]).current_dir(root))?;
    run(tool("ruff")
        .args(["format", "--check", PYTHON_DIR])
        .current_dir(root))?;

    // Compile without running and without writing __pycache__.
    let python = if cfg!(windows) { "python" } else { "python3" };
    let mut scripts = Vec::new();
    for entry in std::fs::read_dir(root.join(PYTHON_DIR)).context("reading resolve-scripts")? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "py") {
            scripts.push(path);
        }
    }
    scripts.sort();
    for script in scripts {
        run(tool(python)
            .args(["-W", "error", "-c", COMPILE])
            .arg(&script)
            .current_dir(root))?;
    }
    Ok(())
}

/// The starting reference: the one requested, otherwise the local `main` branch.
fn base_ref(root: &Path, base: &str) -> anyhow::Result<String> {
    for candidate in [base, "main"] {
        let exists = tool("git")
            .args(["rev-parse", "--verify", "--quiet"])
            .arg(format!("{candidate}^{{commit}}"))
            .current_dir(root)
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if exists {
            return Ok(candidate.to_owned());
        }
    }
    bail!("git reference `{base}` not found (run `git fetch origin main`)")
}

fn commit_messages(root: &Path, base: &str) -> anyhow::Result<()> {
    let range = format!("{}..HEAD", base_ref(root, base)?);
    // One record per commit: hash, author, message.
    let log = output(
        tool("git")
            .args(["log", "--no-merges", "--format=%h%x1f%an%x1f%B%x1e"])
            .arg(&range)
            .current_dir(root),
    )?;
    let mut refused = 0;
    let mut checked = 0;
    for record in log.split('\x1e') {
        let mut fields = record.trim_start().splitn(3, '\x1f');
        let (Some(hash), Some(author), Some(message)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if commits::is_exempt_author(author) {
            continue;
        }
        checked += 1;
        let errors = commits::check_message(message);
        if !errors.is_empty() {
            refused += 1;
            let subject = message.lines().next().unwrap_or_default();
            say(&format!("{hash} {subject}"));
            for error in errors {
                say(&format!("  - {error}"));
            }
        }
    }
    say(&format!("{checked} commit(s) checked in {range}"));
    if refused > 0 {
        say(commits::HELP);
        bail!("{refused} commit message(s) refused");
    }
    Ok(())
}
