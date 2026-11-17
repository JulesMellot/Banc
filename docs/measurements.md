# Measurements at rest

Targets from section 10 of the plan: agent service under 15 MB of RAM, session helper under 10 MB, CPU at 0%.

`cargo xtask idle-bench` starts the agent built in release mode and reads `/proc` for 60 seconds, three times: the agent alone, the agent enrolled and connected to a server with its relay (10 seconds after its enrollment), then the session helper next to it. Memory is resident memory, `VmRSS`, shared pages included. The CPU allowance is 0.1% of elapsed time, i.e. six clock ticks over 60 seconds. The command fails beyond that, and `pr.yml` runs it on every PR.

| Phase | Date | Machine | Agent, memory | Agent, CPU | Session helper |
|---|---|---|---|---|---|
| 0 | 2026-11-11 | Linux x86_64, development container | 2.8 MB | 0 ticks in 10 s | not measured, it exits at startup |
| 1, alone | 2026-11-17 | Linux x86_64, Debian 13 workstation | 5.2 MB | 0 ticks in 60 s | 2.9 MB, 0 ticks (no tray icon on Linux) |
| 1, enrolled and connected | 2026-11-17 | Linux x86_64, Debian 13 workstation | 10.2 MB | 2 ticks in 60 s | same |
| 1 | to do | macOS | | | |
| 1 | to do | Windows | | | |

The server has no target of its own besides fitting on a 2 GB VPS next to PostgreSQL. It is measured by hand, release build, after start and three API calls, then 10 seconds idle.

| Phase | Date | Machine | Server, memory | Server, CPU | Threads |
|---|---|---|---|---|---|
| 1, server side | 2026-11-17 | Linux x86_64, Debian 13 workstation | 18.7 MB | 0 ticks in 10 s | 4 (2 Tokio workers) |
| 1, size profile | 2026-11-17 | Linux x86_64, Debian 13 workstation | 13.4 MB | 0 ticks in 10 s | 4 (2 Tokio workers) |

## Notes

**Phase 0.** The agent does nothing yet: a single-threaded Tokio runtime, the logs, waiting for the shutdown signal. The 2.8 MB are therefore the floor, leaving 12 MB for iroh, the HTTP client and everything else.

The first measurement gave 6.6 MB. `tracing-appender` preallocates its write queue, 128,000 lines by default, which costs 4 MB. The queue is brought down to 1,024 lines in `banc-platform/src/logging.rs`.

macOS and Windows measurements: to be done by hand in phase 1, once `idle-bench` can read memory on those OSes.

**Phase 1, workstation side.** Not enrolled, the agent waits on its local endpoint: 5.2 MB, more than phase 0 only because its binary grew (iroh, rustls, reqwest). Enrolled, it keeps an iroh connection to the server and to the relay: 10.2 MB, of which about 2 MB are its own memory and 8 MB the code pages of its binary. That is why the release profile is optimized for size: with `opt-level = 3` and thin LTO the binary was 13 MB and the enrolled agent 14.1 MB, against a 15 MB target. The two ticks a minute are iroh's keep-alives (QUIC every 5 seconds per path, relay every 15 seconds), kept for NAT traversal.

macOS and Windows: by hand, step 6 of `docs/phase1-checklist.md`. On those OSes the session helper carries a tray icon and an event loop, which Linux does not have yet: its 10 MB target is to be checked there.

**Phase 1, server side.** The agent is unchanged since phase 0, so its measurement stands. The server runs the API, the iroh endpoint (connected to its own relay) and the relay, with a database holding one admin. The release binary is 17.6 MB. With the size profile of the workstation side: 10.2 MB and 13.4 MB at rest.
