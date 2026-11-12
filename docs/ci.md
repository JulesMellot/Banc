# Continuous integration

## The workflows

| File | Trigger | Content |
|---|---|---|
| `ci.yml` | every push | The `cargo xtask check` steps, on Linux, in five jobs |
| `pr.yml` | every PR and every update | `ci.yml`, clippy and tests on macOS and Windows, build of the five targets, idle measurement |
| `pr-title.yml` | PR opened, updated or retitled | Title convention |
| `nightly.yml` | every night at 2:17 UTC | Three OSes, five targets, the day's advisories, issue opened on failure |
| `platforms.yml` | called by `pr.yml` and `nightly.yml` | The OS and target matrix |

The jobs call `cargo xtask check <step>`. A check is changed in `crates/xtask`, not in the YAML.

On a PR, Linux is already covered by `ci.yml`. The `pr.yml` matrix therefore only reruns macOS and Windows, which cost more per minute. At night, all three OSes run.

The title has its own file because otherwise a corrected title would rerun the full matrix.

## GitHub settings to apply once

These settings live in GitHub, not in the repository. No tool from this phase can apply them for you.

1. **Protection of `main`.** In Settings, Rules, Rulesets, choose "Import a ruleset" and give it `.github/rulesets/main.json`. The file forbids direct pushes, force pushes and deletion. It requires a PR up to date with `main`, squash-merged, with the `PR ready` and `PR title` checks green. If an older version of the ruleset is already imported (with the `PR prête` and `Titre de la PR` checks), edit it or import this one again: otherwise no PR can be merged, because those checks no longer exist.
2. **Squash merge.** In Settings, General, Pull Requests, keep only "Allow squash merging" and choose "Pull request title" as the default message.
3. **Dependabot.** In Settings, Advanced Security, enable "Dependabot alerts" and "Dependabot version updates".

On a private repository, GitHub only enforces rulesets from the Pro or Team plan upwards. With a free account, the protection imports but blocks nothing. Check by trying a direct push to `main` after the import.

GitHub only lists a check among the required checks after its first run. Open a PR before checking that both names in the ruleset are recognized.

## Phase 0 acceptance criteria

| Criterion | Where it is checked |
|---|---|
| The three binaries build for every target | "Build" job of `pr.yml`, five targets |
| A PR with a formatting error, a clippy warning, a broken test or a badly named commit is refused | `ci.yml` fails, so `PR ready` fails |
| Pushing directly to `main` is impossible | Imported ruleset, to be tested by hand |

## What remains to be wired up

The plan asks for these checks, but they have nothing to test before the product exists.

- **End-to-end tests** in `pr.yml`, with a server, a depot and two agents in containers. The first scenario, enrollment, arrives in phase 1. The job is added to the `needs` list of `PR ready`.
- **Idle measurement of the session helper.** `idle-bench` only measures the agent, because the session helper exits immediately in phase 0. To be extended in phase 1.
- **Long tests** in `nightly.yml`, in phase 3.
- **`release.yml`**, in phase 7.

## Cost

macOS and Windows minutes are billed at a higher rate than Linux minutes on a private repository. Check current pricing. If the bill goes up, the first lever is to remove the `x86_64-apple-darwin` target from `pr.yml` and keep it for the nightly run.
