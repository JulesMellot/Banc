# Developing Banc

## Tools

| Tool | Version | Installation |
|---|---|---|
| Rust | the one in `rust-toolchain.toml` | rustup installs it on the first `cargo` |
| cargo-nextest | 0.9.151 | `cargo install --locked cargo-nextest@0.9.151` |
| cargo-deny | 0.20.2 | `cargo install --locked cargo-deny@0.20.2` |
| Node.js | 22 or later | |
| pnpm | the one in `ui/package.json` | `corepack enable` |
| Python | 3.7 or later | |
| ruff | 0.16.9 | `pipx install ruff==0.16.9` |
| gitleaks | 8.30.1 | `go install github.com/zricethezav/gitleaks/v8@v8.30.1` |

The versions in this table are those of `.github/workflows/ci.yml`. Change both together.

To build the Linux arm64 target from an amd64 machine, you also need `gcc-aarch64-linux-gnu`.

## Commands

| Command | Effect |
|---|---|
| `cargo xtask check` | All CI checks, in order, stopping at the first failure |
| `cargo xtask check clippy tests` | Only the steps listed |
| `cargo xtask hooks` | Enables the repository's `commit-msg` hook |
| `cargo xtask schema` | Rewrites `migrations/schema.sql` after adding a migration |
| `cargo xtask dev-server` | Server with `dev/server.toml`, data in `.dev/` |
| `cargo xtask dev-agent` | Agent in the foreground, logs in `.dev/logs` |
| `cargo xtask dev-ui` | Frontend in development mode |
| `cargo xtask dist` | Release binaries for the current machine in `dist/<target>/` |
| `cargo xtask dist --target <target>` | Same for another target, option can be repeated |
| `cargo xtask idle-bench` | Agent memory and CPU at rest, on Linux |
| `cargo xtask pr-title "<title>"` | Checks a PR title |

## The `check` steps

| Step | What it runs |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `architecture` | Refuses any OS-specific code outside `banc-platform` |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `tests` | `cargo nextest run --workspace --locked`, then the doc tests |
| `migrations` | Applies the migrations to an empty database and compares the schema with `migrations/schema.sql` |
| `frontend` | In `ui/`: `pnpm install --frozen-lockfile`, `lint`, `check`, `test`, `build` |
| `python` | `ruff check`, `ruff format --check`, compilation of each script in `resolve-scripts/` |
| `deny` | `cargo deny --locked check` |
| `secrets` | `gitleaks` on the branch's commits and on uncommitted changes |
| `commits` | Commit message convention for the branch |

`secrets` and `commits` compare the branch with `origin/main`. The `--base <ref>` option changes that starting point.

## Commit messages

The full rule is in section 15 of `docs/PLAN.md`. The check lives in `crates/xtask/src/commits.rs`, with its tests. It verifies:

- the `type(scope): description` format and the lists of types and scopes;
- the scope, required for `feat`, `fix`, `refactor`, `perf` and `test`;
- the description, which starts with a lowercase verb in the imperative mood and ends without a period;
- the first line, 72 characters at most;
- a `Refs: phase N` line in the body, with the prototype if there is one;
- a `BREAKING CHANGE:` line when the title carries a `!`.

The imperative check looks at the ending of the first word. It stops `added`, `adding` or `fixes`; it does not understand English.

Dependabot commits are exempt.

## Rules enforced by the tools

- `unwrap()` and `expect()` outside tests: refused by clippy, configured in `Cargo.toml` and `clippy.toml`.
- `println!`, `eprintln!`, `dbg!`, `todo!`: refused by clippy. A binary writes to its logs.
- OS `cfg` outside `banc-platform`: refused by the `architecture` step. `xtask` is a development tool; the rule does not apply to it.
- Licenses and duplicate dependencies: `deny.toml`. Adding a license to the list is decided in review.

## Adding a migration

See `migrations/README.md`.
