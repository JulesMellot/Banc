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
- `cargo xtask dev-server`, `cargo xtask dev-agent`, `cargo xtask dev-ui` (the server serves `ui/dist`: run `pnpm build` in `ui/` first, or use `dev-ui`, which proxies `/api` to the server)
- `cargo nextest run --workspace` for the tests alone
- `cargo xtask schema` after adding a migration, then review the diff of `migrations/schema.sql`
- `cargo xtask dist`: release binaries for the current machine in `dist/<target>/`, `--target <target>` for another target. Installers arrive in phase 7.
- `cargo xtask idle-bench`: agent memory and CPU at rest, on Linux
- `cargo xtask hooks`: enables the `commit-msg` hook

## Status and decisions

Phase 0 is done. Prototypes P1 and P2 have their procedure, waiting to be run by hand. Phase 1 is half done: the server side is finished, the agent side (agent, session helper, application, admin key generation) is next. The detailed status is at the top of `docs/PLAN.md`, the server in `docs/server.md`.

- **Rust toolchain pinned** in `rust-toolchain.toml`, so that clippy does not change its mind between two runs. Dependabot proposes version bumps.
- **Dependency versions** declared once, in `[workspace.dependencies]` of the root `Cargo.toml`.
- **Command line** of the three binaries with `lexopt`, not `clap`: the agent counts its bytes. `clap` is reserved for `xtask`.
- **Logs**: `banc_platform::logging`, one file per day. The write queue is limited to 1,024 lines; the default cost 4 MB at rest.
- **`tracing` without the `#[instrument]` macro**: it pulls in `tracing-attributes` and more proc-macro code. iroh already brings `syn` 3 next to `syn` 2 (skip in `deny.toml`); do not add a third reason to keep both.
- **Migrations**: `migrations/NNNN_name.sql` files, embedded in `banc-server`. `migrations/schema.sql` keeps the resulting schema and CI compares it. `0001_enrollment.sql`: admins, sessions, team admin key, invitations, machines, events.
- **Server configuration**: `banc_server::config`, commented example in `packaging/server/banc-server.toml`. An unknown key is refused. `public_url` is required.
- **Tauri window**: no `ui/src-tauri` crate yet. It would require webkit2gtk in every Linux CI job. `ui/` contains the Svelte frontend alone, which is for now the web admin served by the server (`http.admin_ui`).
- **TypeScript 6**: `svelte-check` and `typescript-eslint` do not accept version 7 yet.
- **Commit scope** optional for the `docs`, `build`, `ci` and `chore` types. `Refs:` line required. Dependabot commits exempt.
- **Interface text** in `ui/src/lib/i18n/fr.json`, read through `t('key')`. Server error codes map to `error.<code>`.
- **Python scripts**: Python 3.7 syntax until prototype P10 has answered.
- **Protection of `main`**: `.github/rulesets/main.json`, to be imported into GitHub. Required checks: `PR ready` and `PR title`. See `docs/ci.md`.
- **Languages**: everything in English, except the values in `ui/src/lib/i18n/fr.json`, which are the French interface text. The keys of that file are in English.

Phase 1 decisions (server side):

- **iroh 1.4** (QUIC through `noq`, not `quinn`), `tls-ring`, `presets::Minimal`: no n0 relays, no n0 DNS. Key types come from `iroh-base` (`banc_proto::keys` re-exports them): one Ed25519 implementation for machine keys, admin key and signatures.
- **Signatures** always go through `banc_proto::keys::sign` / `verify`, with a domain label (`banc/enroll/1`, `banc/admin-key/1`). Phase 2 adds `banc/manifest/1`. A signature for one purpose is never valid for another.
- **Enrollment over HTTPS**, signed with the machine key. The relay and the control channel only accept enrolled keys, so an unenrolled machine has nothing but HTTPS.
- **Control channel**: one iroh connection per agent, ALPN `banc/control/1`, length-prefixed JSON frames (64 KB max), heartbeat every 5 minutes. The server can push on it later; it carries data, never commands.
- **Relay** run inside the server (`iroh-relay` with the `server` feature), with an access check against the `machines` table. The server joins it through its local address, agents through the public URL.
- **TLS terminated in front** of the server (example `packaging/server/Caddyfile`). The plan's "certificate obtained by the server" moves to phase 7.
- **Server key in a file** (`data_dir/server.key`, 0600, written by `banc_platform::fs::write_private`): no keychain on the VPS. Workstation secrets still go to the keychain.
- **First admin** through a one-time token written to `data_dir/setup-token`, never logged.
- **Admin passwords** argon2id (crate defaults), sessions of 12 hours, token hash only in the database. TOTP is not done yet.
- **Invitation key to copy**: `<token>-<fingerprint>`, both base32. Fingerprint: 16 bytes of a BLAKE3 key derivation of the admin public key.
- **No `users` table yet**: the invitation carries the user's name and person role, copied to the machine. A table will come when a user needs several machines or permissions.
- **`cargo deny`**: BSD-2-Clause, BSD-3-Clause and ISC allowed, CDLA-Permissive-2.0 for `webpki-roots` only. Duplicate versions and two "unmaintained" notices from the iroh tree are listed with their reason in `deny.toml`.
- **Integration tests** with clippy's `unwrap_used`: an integration test file starts with `#![cfg(test)]` so that its helper functions count as test code.

## Commits and CI

Details are in section 15 of `docs/PLAN.md`. To be followed every time:

- Run `cargo xtask check` before every commit. Do not create a commit if it fails, and never bypass a check (`--no-verify`, an `#[allow]` added to silence clippy, a disabled test).
- Messages in the `type(scope): description` format, description in English starting with a verb in the imperative mood, 72 characters at most. Types: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`. Scopes: `server`, `agent`, `session`, `blobs`, `tunnel`, `resolve`, `platform`, `proto`, `ui`, `scripts`, `packaging`, `ci`, `docs`.
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
- Interface text is in French, in `ui/src/lib/i18n/fr.json`. Everything else (code, comments, logs, errors, command-line help, tests, docs, commits) is in English.
- Every function that compares a local state with a manifest or a file list has tests. This is the heart of the product.
- Every write to a private Resolve file (preferences, library list) goes through `banc-resolve`, happens with Resolve closed, keeps a backup copy, and first checks that the format is the expected one. If the format has changed, write nothing and switch to guided mode.
- Before coding a function that depends on a Resolve behavior, check that the matching prototype in section 12 of the plan has an answer in `docs/prototypes.md`.
- Measure memory and CPU at rest at the end of each phase (`cargo xtask idle-bench`) and record the result in `docs/measurements.md`.

## What cannot be tested in CI

Resolve does not exist in CI. `banc-resolve` provides a fake used by the tests. Real tests are done by hand on a machine with Resolve Studio, using each phase's checklist in `docs/PLAN.md`.
