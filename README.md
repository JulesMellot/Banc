# Banc

Banc sets up the DaVinci Resolve workstations of an editing team. A server on a VPS describes what each workstation must have. An agent installed on each machine checks the workstation, installs what is missing, fetches the project files and opens Resolve on them. Internal tool.

The repository is halfway through phase 1 of the plan. The server works: invitations, enrollment, the iroh control channel and relay, a minimal web admin. The agent and the session helper still only start and wait; they are next. The status, step by step, is at the top of `docs/PLAN.md`.

## Where to read

- `docs/PLAN.md`: the specification, phase by phase.
- `CLAUDE.md`: principles, stack and code rules.
- `docs/development.md`: installing the tools and running the checks.
- `docs/ci.md`: the workflows and the GitHub settings to apply by hand.
- `docs/prototypes.md`: the answers from the Resolve prototypes.
- `docs/measurements.md`: memory and CPU at rest, phase by phase.
- `docs/server.md`: the server, enrollment and the HTTP API.

## Getting started

```bash
cargo xtask hooks
```

```bash
cargo xtask check
```

The first command enables the commit message check. The second runs everything CI runs.
