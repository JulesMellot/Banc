# Banc

**Every DaVinci Resolve workstation in your team, set up the same way. No VPN, no shared drive to mount, no "it works on my machine".**

An editor installs Banc, clicks an invitation link, and that's it. Banc checks their workstation, installs the plugins, fonts, LUTs and Fusion templates the team uses, fetches the media of the project they open, and launches Resolve on it. Everything they add to the project, rushes, music, graphics, renders, reaches the rest of the team without them thinking about it.

Mac, Windows and Linux. One small server on a VPS. A NAS as the permanent depot.

> Internal tool, built in the open. Banc is in active development: see [where it stands](#where-it-stands).

## Why it's different

- **One click to join.** Install, click the link, type the admin password once. No terminal, no config file, no IT ticket.
- **Peer to peer first.** Machines connect directly to each other over [iroh](https://iroh.computer), across NATs, on the LAN or across the world. The VPS coordinates and only carries media as a last resort.
- **No VPN, no open port.** Every machine dials out. The server exposes one port, 443. Workstations and the NAS expose nothing.
- **The server cannot take over your machines.** What a workstation installs is described in a manifest signed with the admin's key, which never leaves the admin's computer. A compromised VPS can't get a single command run.
- **Banc follows you, not the other way around.** Drag a file into the timeline however you like: Banc notices, publishes it, and catches up behind you.
- **Nothing ever disappears on its own.** Published files are only deleted by an admin. A local file is only freed once the depot has confirmed its copy.
- **Light enough to forget.** Under 15 MB of RAM and 0% CPU at rest for the agent. Nothing happens while Resolve is closed, unless you asked for it.
- **Real disks.** No virtual drive, no kernel extension, no FUSE: a normal local volume, at the same path on every machine.

## How it works

```
                   VPS · banc-server (port 443 only)
          API · web admin · iroh relay · cache · coordination
                               │
                 outbound connections only (QUIC)
     ┌────────────┬────────────┼────────────┬────────────┐
   Mac         Windows       Linux       Render       NAS depot
 workstation  workstation  workstation  (optional)
     └──────────── direct peer-to-peer when possible ────┘
```

- **`banc-server`** on the VPS: invitations, machines, the team's manifest, the relay, the render queue. One Docker image, deployable on Coolify or Dokploy in a few clicks.
- **The agent**, a system service on each machine: checks the workstation, installs what is missing, moves files, applies signed updates with automatic rollback.
- **The tray icon and the window**: what the editor sees. The window only runs while it is open.
- **The depot** on the NAS: keeps a copy of everything that has been published.

Every file is identified by its BLAKE3 hash and verified before it is used. Every package and every update is signed.

## Where it stands

| | |
|---|---|
| ✅ Joining a team | Invitation link, enrollment, machine identity, live connection to the server, Resolve detection |
| ✅ Signed packages | The admin builds the team's manifest in the web admin and signs it in the app; workstations install plugins, fonts, templates and installers, and report what they have |
| ✅ Automatic updates | Signed releases, installed while Resolve is closed, rolled back if the new version doesn't answer |
| 🚧 Next | Peer-to-peer file transfer, projects, proxies, shared project libraries, the render machine |

Phases 1 and 2 pass in CI and are waiting for their hand test on real machines. The full plan, phase by phase, is in [`docs/PLAN.md`](docs/PLAN.md).

## Install the server

The server is a prebuilt image, `ghcr.io/julesmellot/banc-server` (amd64, arm64), that runs comfortably on a 2 GB VPS. On Coolify or Dokploy, point it at this repository's `docker-compose.yml` and give it a domain: there is nothing else to fill in. Step by step, including a plain VPS with Caddy: [`packaging/server/README.md`](packaging/server/README.md).

The workstation app for macOS (`.dmg`, Apple silicon and Intel) and Windows (`.exe`) is attached to each [release](https://github.com/JulesMellot/Banc/releases). Linux packages arrive with phase 7; until then, `cargo xtask dist` builds the binaries.

## Build it

Rust (stable, pinned in `rust-toolchain.toml`), Tokio, iroh, axum and SQLite on the back; Svelte and Tauri 2 for the interface.

```bash
cargo xtask hooks   # enable the commit message check
cargo xtask check   # run everything CI runs
```

| To read | For |
|---|---|
| [`docs/PLAN.md`](docs/PLAN.md) | The specification and the current status |
| [`CLAUDE.md`](CLAUDE.md) | Principles, stack, decisions and code rules |
| [`docs/development.md`](docs/development.md) | Tools to install, dev servers, checks |
| [`docs/server.md`](docs/server.md) | The server, enrollment, the HTTP API |
| [`docs/workstation.md`](docs/workstation.md) | The agent, the window, the tray icon |
| [`docs/ci.md`](docs/ci.md) | Workflows, releases, GitHub settings |
| [`docs/measurements.md`](docs/measurements.md) | Memory and CPU at rest, phase by phase |
| [`docs/prototypes.md`](docs/prototypes.md) | What we learned from Resolve itself |

## License

Proprietary. All rights reserved.
