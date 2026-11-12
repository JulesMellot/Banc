# Banc

Banc sets up the DaVinci Resolve workstations of an editing team. A server on a VPS describes what each workstation must have. An agent installed on each machine checks the workstation, installs what is missing, fetches the project files and opens Resolve on them. Internal tool.

The repository is at phase 0 of the plan. The three binaries start, open their logs and wait. The product starts in phase 1.

## Where to read

- `docs/PLAN.md`: the specification, phase by phase.
- `CLAUDE.md`: principles, stack and code rules.
- `docs/development.md`: installing the tools and running the checks.
- `docs/ci.md`: the workflows and the GitHub settings to apply by hand.
- `docs/prototypes.md`: the answers from the Resolve prototypes.
- `docs/measurements.md`: memory and CPU at rest, phase by phase.

## Getting started

```bash
cargo xtask hooks
```

```bash
cargo xtask check
```

The first command enables the commit message check. The second runs everything CI runs.
