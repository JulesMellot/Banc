# The server

Phase 1 status of `banc-server`: what it serves, how a machine joins the team, and how to run it.

## Parts

One process, three listeners:

| Part | Default address | Role |
|---|---|---|
| HTTP API and web admin | `127.0.0.1:8080` (TCP) | Enrollment, server information, admin |
| iroh endpoint | `0.0.0.0:443` (UDP) | Control channel with the agents (ALPN `banc/control/1`) |
| iroh relay | `127.0.0.1:3340` (TCP), reached through the HTTP port | Relay of the team's iroh traffic, enrolled machines only |

The server runs in Docker (`packaging/server/README.md`). TLS is terminated in front of it, by Coolify's proxy or by the Caddy of `packaging/server/compose.yaml`. The HTTP port carries everything: the API, the web admin, and the relay's `/relay`, `/ping` and `/generate_204` paths, which it forwards to the relay's local listener. Publishing UDP 443 is optional: without it, agents reach the server through the relay.

The server keeps in `data_dir` (the `banc-data` Docker volume):

- `banc.sqlite`: the database;
- `server.key`: its iroh key, mode 0600. The VPS has no keychain;
- `setup-token`: present only until the first admin exists.

At rest the release build uses 13.4 MB of resident memory and no CPU (see `docs/measurements.md`).

## Joining the team

```
admin's Banc app        web admin              server                      agent
      │                     │                     │                          │
      │ PUT admin-key ──────────────────────────► │ public key only          │
      │                     │ POST invites ─────► │ token hash stored        │
      │                     │ ◄──── link + key    │                          │
      │                     │                     │                          │
      │                     │   banc://join?u=…&k=<token>&a=<fingerprint>    │
      │                     │ ─────────────────────────────────────────────► │
      │                     │                     │ ◄──── POST /api/v1/enroll│
      │                     │                     │  token + machine key,    │
      │                     │                     │  signed by that key      │
      │                     │                     │ ────► admin key, server  │
      │                     │                     │       iroh id, relay URL │
      │                     │                     │                          │ checks admin key
      │                     │                     │                          │ against fingerprint
      │                     │                     │ ◄════ iroh banc/control/1│
      │                     │                     │  hello, then heartbeat   │
      │                     │                     │  every 5 minutes         │
```

1. **Admin key.** The admin's Banc application generates the key pair, keeps the private key in the keychain and sends the public key with a signature that proves possession (`AdminKeyRegistration`). Replacing a registered key requires `replace: true`; pending invitations then become obsolete.
2. **Invitation.** 32 random bytes, single use, valid 7 days. The server stores the BLAKE3 hash of the token only, so the link is shown once. The link carries the fingerprint of the admin key: the first 16 bytes of a BLAKE3 key derivation of the public key, in base32. The key to copy is `<token>-<fingerprint>`.
3. **Enrollment.** The agent generates its machine key, signs the token and its public key with it, and posts the request. The server consumes the invitation and records the machine in one transaction. A key already known, even revoked, is refused: a machine that comes back enrolls with a new key.
4. **Control channel.** The agent connects to the server's iroh endpoint. The handshake proves the machine key; the server closes any connection from a key that is not enrolled or is revoked. The agent says hello with its version and inventory (hostname, OS version, Resolve), then sends a heartbeat every 5 minutes and a report when the inventory changes. Three missed heartbeats close the connection.
5. **Revocation.** The admin revokes a machine: its connection is closed at once, the relay refuses it, and it cannot reconnect.

## HTTP API

Errors are JSON, `{"error": "<code>", "message": "<English text>"}`. The web admin maps codes to French text (`error.<code>` in `ui/src/lib/i18n/fr.json`).

| Method and path | Session | Purpose |
|---|---|---|
| `GET /api/v1/info` | no | Version, server iroh id, relay URL, direct addresses, `setup_required` |
| `POST /api/v1/enroll` | no | Enrollment (`banc_proto::enroll`) |
| `POST /api/v1/admin/setup` | no | First admin, with the `setup-token` file |
| `POST /api/v1/admin/login` | no | Opens a 12-hour session (cookie `banc_session`, `HttpOnly`, `SameSite=Strict`) |
| `POST /api/v1/admin/logout` | yes | Ends the session |
| `GET /api/v1/admin/me` | yes | Current admin |
| `GET`, `PUT /api/v1/admin/admin-key` | yes | Team admin key and its fingerprint |
| `GET`, `POST /api/v1/admin/invites` | yes | List, create (answer contains the link and the key, once) |
| `POST /api/v1/admin/invites/{id}/revoke` | yes | Cancel a pending invitation |
| `GET /api/v1/admin/machines` | yes | Machines, with `online` and the last inventory |
| `POST /api/v1/admin/machines/{id}/revoke` | yes | Revoke a machine and cut its connection |

Enrollment refusal codes: `invite_unknown`, `invite_used`, `invite_revoked`, `invite_expired`, `invite_obsolete` (admin key replaced), `machine_key_known`, `bad_signature`.

Every change (admin created, admin key set, invitation created or revoked, machine enrolled or revoked, Resolve version changed) writes a row to the `events` table.

## First start

1. Start the server. With no admin, it writes `data_dir/setup-token` (mode 0600) and logs the file path, never the token.
2. Open the web admin, enter the token, choose a name and a password (12 characters at least). The file is deleted.
3. Register the admin key from the Banc window ("Clé d'administration", see `docs/workstation.md`). Until then the web admin refuses to create invitations.

The full setup on the VPS is step 1 of `docs/phase1-checklist.md`. `packaging/server/smoke-test.sh` starts the same Compose setup locally and checks it; PRs run it.
