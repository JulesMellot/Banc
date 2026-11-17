# Banc

Banc sets up the DaVinci Resolve workstations of an editing team. A server on a VPS describes what each workstation must have. An agent installed on each machine checks the workstation, installs what is missing, fetches the project files and opens Resolve on them. Internal tool.

The repository is at the end of phase 1 of the plan: a workstation joins the team with an invitation link and stays connected to the server over iroh; the admin sees the machines and their Resolve version. What remains is the hand test on the three OSes (`docs/phase1-checklist.md`). The status, step by step, is at the top of `docs/PLAN.md`.

## Where to read

- `docs/PLAN.md`: the specification, phase by phase.
- `CLAUDE.md`: principles, stack and code rules.
- `docs/development.md`: installing the tools and running the checks.
- `docs/ci.md`: the workflows and the GitHub settings to apply by hand.
- `docs/prototypes.md`: the answers from the Resolve prototypes.
- `docs/measurements.md`: memory and CPU at rest, phase by phase.
- `docs/server.md`: the server, enrollment and the HTTP API.
- `docs/workstation.md`: the agent, the window and the tray icon.
- `docs/phase1-checklist.md`: the hand test of phase 1.

## Getting started

```bash
cargo xtask hooks
```

```bash
cargo xtask check
```

The first command enables the commit message check. The second runs everything CI runs.
