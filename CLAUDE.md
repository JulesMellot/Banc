# Banc

Banc (working name) sets up the DaVinci Resolve workstations of an editing team. A central server on a VPS describes what each workstation must have. An agent installed on each machine (Mac, Windows, Linux) checks the workstation, installs what is missing, fetches proxies on demand, opens projects (in reservation mode or in Resolve collaboration) and sends renders to a dedicated machine. A NAS acts as the permanent depot for files.

The full plan, phase by phase, is in `docs/PLAN.md`. Read it before any task. Work on one phase at a time and only move to the next when its acceptance criteria pass.

## Non-negotiable principles

- The user installs an application, pastes a URL and a key (or clicks an invitation link), and that's it. Any feature that requires one more manual step must be flagged before it is coded.
- No VPN. Machines identify themselves by their iroh public key and connect outbound. No inbound port on workstations or on the NAS. The VPS only exposes 443 (API + iroh relay).
- Peer-to-peer first. A file is fetched from a directly reachable machine (workstation or depot) before the VPS cache or relay. The VPS coordinates; it only carries media as a fallback.
- Minimal, measured and maintained resource usage:
  - agent service at rest: under 15 MB of RAM, CPU at 0% outside an ongoing action;
  - session helper (system tray icon): under 10 MB;
  - interface window: started on click, process ended when it closes;
  - server: must run on a VPS with 2 GB of RAM with PostgreSQL alongside.
- No activity while Resolve is closed, apart from requested actions and transfers in progress. While Resolve is open on a Banc project, light monitoring is allowed: consolidation, modified media, render queue, proxy generation. The table in section 10 of the plan is authoritative.
- Everything that enters a project is published, however it got there. Banc catches up behind the user instead of imposing a procedure on them.
- The VPS cannot make workstations run anything. The manifest is signed with the admin key, kept on the admin's workstation. The agent refuses any unsigned manifest, package or update. Never add a path through which the server triggers a command on a workstation without a signature.
- Banc never deletes a published file without an admin action, and never frees a local file that the depot has not confirmed.
- No virtual disk, no kernel extension, no FUSE. The Banc volume is a real local volume (APFS on Mac, drive letter on Windows, mount point on Linux).
- Internal tool: company machines, salaried editors, Resolve Studio everywhere. No guest mode.

## Stack

- Stable Rust, 2021 edition, Cargo workspace.
- Runtime: Tokio. The agent uses a `current_thread` runtime.
- Peer-to-peer networking: `iroh` 1.x (connection by public key, NAT traversal, relay). Self-hosted relay on the VPS with `iroh-relay`.
- File transfer: `iroh-blobs` if its version is stable at the start, otherwise an in-house protocol over iroh streams with BLAKE3 and streaming verification (`bao-tree`). Decision to be recorded here in phase 3.
- HTTP server: `axum`. Database: SQLite through `rusqlite` (`bundled` feature) and `rusqlite_migration`.
- Interface: Tauri 2, Svelte + TypeScript frontend. Native system tray icon with `tray-icon` + `tao`, no web view.
- System services: `service-manager` crate (launchd, Windows service, systemd).
- Secrets: `keyring` crate. Signatures: `ed25519-dalek`. Updates: `self-replace`.
- Logs: `tracing` + `tracing-appender` with rotation.
- Resolve bridge: embedded Python scripts, run by the session helper only when it needs to talk to Resolve.

## Workspace

```
crates/
  banc-proto/       shared messages and types (serde)
  banc-platform/    all OS-specific code (paths, services, elevation, fonts)
  banc-blobs/       hashes, local store, transfer, source selection
  banc-resolve/     version detection, Python script calls, fake for tests
  banc-tunnel/      TCP tunnel over iroh (PostgreSQL)
  banc-agent/       binary: system service (installs, transfers, tunnel)
  banc-session/     binary: system tray icon + Resolve bridge, in the user session
  banc-server/      binary: API, admin, iroh relay, cache, render queue
ui/                 Tauri application (user window) and web admin (same frontend)
resolve-scripts/    embedded Python scripts
migrations/         server SQL
dev/                development configuration
packaging/          pkg, msi, deb/rpm, Docker image (server and NAS depot)
docs/               PLAN.md and technical notes
```

## Commands

Details, with the tools to install, are in `docs/development.md`.

- `cargo xtask check`: the same checks as CI on every push. Steps: `fmt`, `architecture`, `clippy`, `tests`, `migrations`, `frontend`, `python`, `deny`, `secrets`, `commits`. `cargo xtask check clippy tests` only runs the steps listed.
- `cargo xtask dev-server`, `cargo xtask dev-agent`, `cargo xtask dev-ui`
- `cargo nextest run --workspace` for the tests alone
- `cargo xtask schema` after adding a migration, then review the diff of `migrations/schema.sql`
- `cargo xtask dist`: release binaries for the current machine in `dist/<target>/`, `--target <target>` for another target. Installers arrive in phase 7.
- `cargo xtask idle-bench`: agent memory and CPU at rest, on Linux
- `cargo xtask hooks`: enables the `commit-msg` hook

## Status and decisions

Phase 0 is done. The three binaries start, open their logs and wait for shutdown. No product feature exists yet.

- **Rust toolchain pinned** in `rust-toolchain.toml`, so that clippy does not change its mind between two runs. Dependabot proposes version bumps.
- **Dependency versions** declared once, in `[workspace.dependencies]` of the root `Cargo.toml`.
- **Command line** of the three binaries with `lexopt`, not `clap`: the agent counts its bytes. `clap` is reserved for `xtask`.
- **Logs**: `banc_platform::logging`, one file per day. The write queue is limited to 1,024 lines; the default cost 4 MB at rest.
- **`tracing` without the `#[instrument]` macro**: it pulls in a second version of `syn`, which `cargo deny` refuses.
- **Migrations**: `migrations/NNNN_name.sql` files, embedded in `banc-server`. `migrations/schema.sql` keeps the resulting schema and CI compares it. The folder is empty until phase 1.
- **Server configuration**: `banc_server::config`, commented example in `packaging/server/banc-server.toml`. An unknown key is refused.
- **Tauri window**: no `ui/src-tauri` crate yet. It would require webkit2gtk in every Linux CI job without bringing anything before phase 1. `ui/` contains the Svelte frontend alone.
- **TypeScript 6**: `svelte-check` and `typescript-eslint` do not accept version 7 yet.
- **Commit scope** optional for the `docs`, `build`, `ci` and `chore` types. `Refs:` line required. Dependabot commits exempt.
- **Interface text** in `ui/src/lib/i18n/fr.json`, read through `t('key')`.
- **Python scripts**: Python 3.7 syntax until prototype P10 has answered.
- **Protection of `main`**: `.github/rulesets/main.json`, to be imported into GitHub. See `docs/ci.md`.
- **Languages**: documentation (README, `docs/`, this file) and code comments in English. Interface text and commit descriptions in French.

## Commits and CI

Details are in section 15 of `docs/PLAN.md`. To be followed every time:

- Run `cargo xtask check` before every commit. Do not create a commit if it fails, and never bypass a check (`--no-verify`, an `#[allow]` added to silence clippy, a disabled test).
- Messages in the `type(scope): description` format, description in French starting with an infinitive verb, 72 characters at most. Types: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`. Scopes: `server`, `agent`, `session`, `blobs`, `tunnel`, `resolve`, `platform`, `proto`, `ui`, `scripts`, `packaging`, `ci`, `docs`.
- A commit does one thing. The message footer cites the phase and the prototype (`Refs: phase 3, P5`).
- Never push directly to `main`. One `type/short-topic` branch per task, then a PR whose title follows the same convention.
- A PR is only ready when all checks are green. If a check fails, fix the cause.
- Every new behavior comes with its tests in the same PR.

## Code rules

- Everything that depends on the OS lives in `banc-platform`, with `#[cfg(target_os = ...)]` grouped by module. No OS `cfg` anywhere else.
- No `unwrap()` or `expect()` outside tests and startup. Errors with `thiserror` in libraries, `anyhow` in binaries.
- Every action that modifies the workstation writes a line to the local log and reports to the server.
- A downloaded file is verified by its BLAKE3 hash before it is moved into place or executed. A file that does not match is deleted.
- Secrets go into the OS keychain, never into a plain-text file.
- Visible messages are in French, in a translation file.
- Every function that compares a local state with a manifest or a file list has tests. This is the heart of the product.
- Every write to a private Resolve file (preferences, library list) goes through `banc-resolve`, happens with Resolve closed, keeps a backup copy, and first checks that the format is the expected one. If the format has changed, write nothing and switch to guided mode.
- Before coding a function that depends on a Resolve behavior, check that the matching prototype in section 12 of the plan has an answer in `docs/prototypes.md`.
- Measure memory and CPU at rest at the end of each phase (`cargo xtask idle-bench`) and record the result in `docs/measurements.md`.

## What cannot be tested in CI

Resolve does not exist in CI. `banc-resolve` provides a fake used by the tests. Real tests are done by hand on a machine with Resolve Studio, using each phase's checklist in `docs/PLAN.md`.
