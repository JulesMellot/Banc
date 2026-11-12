# Measurements at rest

Targets from section 10 of the plan: agent service under 15 MB of RAM, session helper under 10 MB, CPU at 0%.

`cargo xtask idle-bench` starts the agent built in release mode, waits 2 seconds, then reads `/proc` for 10 seconds. Memory is resident memory, `VmRSS`, shared pages included. The CPU allowance is 0.1% of elapsed time, i.e. one clock tick over 10 seconds. The command fails beyond that, and `pr.yml` runs it on every PR.

| Phase | Date | Machine | Agent, memory | Agent, CPU | Session helper |
|---|---|---|---|---|---|
| 0 | 2026-11-11 | Linux x86_64, development container | 2.8 MB | 0 ticks in 10 s | not measured, it exits at startup |

## Notes

**Phase 0.** The agent does nothing yet: a single-threaded Tokio runtime, the logs, waiting for the shutdown signal. The 2.8 MB are therefore the floor, leaving 12 MB for iroh, the HTTP client and everything else.

The first measurement gave 6.6 MB. `tracing-appender` preallocates its write queue, 128,000 lines by default, which costs 4 MB. The queue is brought down to 1,024 lines in `banc-platform/src/logging.rs`.

macOS and Windows measurements: to be done by hand in phase 1, once `idle-bench` can read memory on those OSes.
