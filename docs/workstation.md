# The workstation

Phase 1 status of the three programs installed on a workstation: the agent, the window and the session helper. The server side is in `docs/server.md`.

## Three programs

| Program | Runs as | Lifetime | Role in phase 1 |
|---|---|---|---|
| `banc-agent` | system service (root, LocalSystem) | always | Machine key, enrollment, connection to the server, inventory |
| `banc-app` (the window) | the user | while the window is open | `banc://` links, joining a team, the workstation's state, the admin key |
| `banc-session` | the user | the whole session | Tray icon with the agent's state (macOS, Windows) |

The window and the tray icon never talk to the server about the workstation: they ask the agent, over its local endpoint. The one exception is the admin key, which belongs to a person, not to the machine: the window registers it on the server itself.

## Local endpoint

A Unix socket (`/run/banc/agent.sock` on Linux, `/var/run/banc/agent.sock` on macOS) or the named pipe `\\.\pipe\banc-agent` on Windows. Any local user may connect; the pipe's ACL lets authenticated users read and write.

One request per connection, framed like the control channel (`banc_proto::control::write_frame`). Two requests exist (`banc_proto::local`):

- `status`: version, enrollment (server, machine, roles, admin key fingerprint, connection state), inventory;
- `enroll`: a `banc://join` link, or a server address and the key to copy, and an optional machine name. Refused once the machine is enrolled.

Nothing on this endpoint runs a command or changes anything else on the workstation.

## Joining a team

```
user clicks banc://join?u=…&k=…&a=…
    │
    ▼
window (deep link) ── reads the link, shows the server and the admin key fingerprint
    │  "Rejoindre"
    ▼
agent (local endpoint)
    │ generates the machine key
    │ POST /api/v1/enroll, signed with it
    │ checks the answer's admin key against the link's fingerprint
    │ stores the key (secret store), then agent.json
    ▼
iroh connection to the server: hello, heartbeat every 5 minutes
```

If the server's admin key does not match the fingerprint, the agent refuses and keeps nothing: the link pointed to the wrong server, or the server is not trustworthy. The window says so (`error.admin_key_mismatch`).

If the service is not installed, the window offers to install it. The OS asks for the administrator password once (`banc_platform::elevate`), then runs `banc-agent service install`, which registers and starts the service: systemd unit `banc-agent.service`, launchd daemon `com.banc.agent`, Windows service `BancAgent`.

## What the agent keeps

| What | macOS | Windows | Linux |
|---|---|---|---|
| Enrollment (`agent.json`, no secret) | `/Library/Application Support/Banc` | `%ProgramData%\Banc` | `/var/lib/banc` |
| Machine key (`machine-key`) | System keychain | Credential Manager (LocalSystem) | `/var/lib/banc/secrets`, root, 0600 |
| Logs | `/Library/Logs/Banc` | `%ProgramData%\Banc\logs` | `/var/log/banc` |

A missing key next to an `agent.json` makes the agent forget the enrollment: the machine enrolls again with a new key.

## Connection to the server

The agent dials the server's iroh identifier with the relay and direct addresses it received at enrollment. After the hello it sends a heartbeat at the interval the server asks (5 minutes), or a report when the inventory changed (Resolve installed or updated). A cut connection is retried after 1 s, then 2, 4… up to 5 minutes. When the server answers that the machine is unknown or revoked, the agent stops trying and its state says `revoked`.

## Inventory

Computer name and OS version are read once at start; the installed Resolve at every heartbeat (a few file reads):

- macOS: `Info.plist` of `/Applications/DaVinci Resolve/DaVinci Resolve.app`;
- Windows: the installed program named "DaVinci Resolve" or "DaVinci Resolve Studio" (`DisplayVersion`, whose `21.1.00014` means 21.1.0 build 14), not its companions such as the Control Panels;
- Linux: the desktop entry and read-me of `/opt/resolve`.

These sources come from section 11 of the plan and must be confirmed on real machines (step 5 of `docs/phase1-checklist.md`).

## The admin key

On the admin's workstation only, in the window under "Clé d'administration":

1. **Create**: the window generates the key and shows its recovery code, 11 groups of 5 characters. The admin writes it down and types the last group back; only then is the key stored, in the user's keychain (`admin-key`).
2. **Register**: with an admin account of the web admin, the window logs in to the server and sends the public key with a signature that proves it holds the private one. Replacing a key already registered needs the "Remplacer" box: pending invitations then stop working.
3. **Restore**: on a new workstation, from the recovery code. A typo is caught by the checksum.

The server never receives the private key. Phase 2 uses this key to sign the manifest.

## Tray icon

`banc-session` shows an icon in the menu bar (macOS, no Dock icon) or the notification area (Windows). Its menu: the agent's state, "Ouvrir Banc", "Quitter". The state is asked every minute. On Linux there is no icon yet: `tray-icon` needs GTK there; the helper waits for the end of the session. Starting it at login is the installers' job (phase 7).

## Resource use at rest

See `docs/measurements.md`. On Linux: agent 5.2 MB alone, 10.2 MB enrolled and connected, about 2 CPU ticks a minute for iroh's keep-alives; session helper 2.9 MB.
