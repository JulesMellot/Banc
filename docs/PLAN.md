# Banc build plan

This document is the specification for Claude Code. Each phase delivers something usable and ends with acceptance criteria. A phase does not start until the previous one passes.

Context: Banc is an internal tool for a company's salaried editors and colorists, on company machines. Every machine has DaVinci Resolve Studio.

## 1. The product on one page

### What the user experiences

1. The admin creates an invitation. They get a link `banc://join?u=https://banc.mycompany.com&k=XXXX&a=FINGERPRINT` and a key to copy.
2. The editor installs Banc and clicks the link (or pastes the URL and the key). Banc asks once for the workstation's administrator password and, on a Mac, for file access permissions.
3. Banc checks the workstation: Resolve version, plugins, fonts, LUTs, DCTLs, Fusion templates, presets, Banc volume, shared project library. It fixes on its own whatever it can.
4. The editor sees the list of projects and opens one. Banc fetches what is needed, then opens Resolve on it.
5. From then on they work in Resolve as usual. Everything they add to the project (rushes, music dragged into the timeline, graphics, renders) becomes available to the others without them thinking about it.

### Core principle

A Banc project is meant to be shared. Everything that enters a project is published, however it got there. Banc adapts to the way people work in Resolve and catches up behind them.

### Roles

- **Workstation**: editor or colorist.
- **Depot**: the NAS. Keeps a copy of everything that is published. Docker or Linux binary, no interface.
- **Render** (optional): a machine dedicated to renders. The product works without it.
- **Server**: on the VPS. API, web admin, iroh relay, file cache, coordination, PostgreSQL for collaboration.

A user also has a person role: admin, editor, colorist. It sets what they can do and what their workstation downloads.

### What Banc does not do (on purpose)

- No activity while Resolve is closed, apart from actions the user asked for and transfers in progress.
- No virtual disk with a driver. A real local volume, at the same path everywhere.
- No plugin license activation. Banc installs, checks presence, and reports.
- No automatic deletion of published files.

## 2. Architecture

```
                    VPS (only exposed point, port 443)
          ┌──────────────────────────────────────────────┐
          │ banc-server                                  │
          │   HTTPS API, web admin, coordination         │
          │   iroh relay                                 │
          │   file cache (quota + expiry)                │
          │   PostgreSQL tunnel endpoint                 │
          │ PostgreSQL 18 (listens on 127.0.0.1 only)    │
          │ SQLite + PostgreSQL backups -> depot         │
          └──────────────────────┬───────────────────────┘
                                 │ outbound connections, iroh (QUIC)
      ┌──────────────┬───────────┼──────────────┬─────────────────┐
┌─────┴─────┐ ┌──────┴────┐ ┌────┴──────┐ ┌─────┴───────┐  ┌──────┴──────┐
│ Mac       │ │ Windows   │ │ Linux     │ │ render      │  │ NAS depot   │
│ station   │ │ station   │ │ station   │ │ (optional)  │  │             │
└───────────┘ └───────────┘ └───────────┘ └─────────────┘  └─────────────┘
   Machines talk to each other directly whenever possible (same LAN or
   successful NAT traversal), otherwise through the VPS relay.
```

### Transport

iroh 1.x connects two machines identified by their public key, directly or through a relay, without a VPN. Files are identified by their BLAKE3 hash and verified during the transfer.

### VPS exposure

- One domain name, port 443, a Let's Encrypt certificate obtained by the server.
- The HTTPS API and the iroh relay share port 443.
- PostgreSQL listens on `127.0.0.1` only.

If the VPS is down, coordination and collaboration stop. That is accepted. Reservation mode keeps working on the files already present, and publications go out when the server comes back.

### Local network

Agents on the same LAN find each other through iroh's local discovery and exchange files without going through the Internet.

### Identity and security

- Each machine has an Ed25519 key, which is its iroh identifier. The private key stays in the OS keychain.
- Invitation: 32-byte token, single use, valid 7 days, carries the role.
- A machine only accepts iroh connections from public keys declared for the team. Revoking a machine removes it from the list and cuts its tunnels.
- Admin accounts: argon2id, optional TOTP.
- The PostgreSQL password is generated at install time. Resolve's default pair (`postgres` / `DaVinci`) is never used.

### The VPS cannot make workstations run anything

The agent runs with admin rights and installs software. A compromised VPS must not be able to take advantage of that.

- **Admin key.** On first setup, the admin's Banc application generates an Ed25519 key pair. The private key stays in the keychain of their workstation. Banc has them write down a recovery phrase on paper. The private key is never sent to the VPS.
- **Signed manifest.** The manifest (Resolve version, packages with their hash and install command, enforced settings) is signed with the admin key. A manifest is published from the admin's Banc application, not from the browser.
- **Fingerprint in the invitation.** The invitation link contains the fingerprint of the admin public key (`a=`). The workstation records it at enrollment. From then on it refuses any manifest whose signature does not match, and any package whose hash is not in a signed manifest.
- **Banc updates.** Signed with a release key kept off the VPS (the developer's machine or a CI secret). The agent refuses an unsigned binary.
- What the VPS controls without a signature: project file lists and access rights. These are data, never executed code.
- Changing the admin key requires an action on every workstation (a new invitation). This is intended.

## 3. Data model

SQLite on the VPS:

- `users`: name, person role, permissions.
- `machines`: name, OS, arch, role, public key, last connection, agent version, detected Resolve version, last report, latency to the VPS.
- `invites`
- `manifests`: content, signature, version. The latest published one applies.
- `packages`, `package_builds`: installable items, one variant per OS and arch.
- `blobs`: BLAKE3 hash, size.
- `blob_locations`: who holds which blob (machine, depot, cache), date of last verification.
- `projects`: name, mode (`reservation` or `collaborative`), template, members, state (`active`, `archived`), lock (reservation mode).
- `project_files`: relative path, class (`original`, `proxy`, `media`, `export`), current hash, revision number, author, date, state (`present`, `retired`).
- `project_file_revisions`: hash history of a given path.
- `project_versions`: published `.drp` files (reservation mode).
- `render_jobs`: project, timeline, preset, chosen machine, scheduled time, status, progress, output file.
- `events`: audit log.

### Manifest

```json
{
  "version": 12,
  "resolve": { "edition": "studio", "version": "21.1" },
  "packages": [
    { "id": "boris-continuum", "kind": "ofx", "version": "2025.0", "required": true },
    { "id": "font-inter", "kind": "font", "version": "4.0", "required": true }
  ],
  "volume": { "unix": "/Volumes/Banc", "windows": "B:" },
  "collaboration": { "postgres_version": 18, "local_address": "127.0.0.2" },
  "limits": { "consolidation_ask_above_gb": 5, "free_proxies_after_days": 30 },
  "signature": "..."
}
```

### Package detection rules

`file_exists`, `bundle_version` (Mac), `registry` (Windows), `ofx_bundle`, `font`, `command` (last resort).

## 4. The Banc volume and the folder tree

### The same path everywhere

Resolve stores absolute paths. Banc creates a dedicated volume, visible as a disk, with no driver and no kernel extension:

| OS | Path | How Banc creates it |
|---|---|---|
| macOS | `/Volumes/Banc` | APFS volume added to the container of the chosen disk (`diskutil apfs addVolume`). Shares the disk's space, mounts by itself at startup. |
| Linux | `/Volumes/Banc` | Mount point or link to the chosen storage folder. |
| Windows | `B:\` | System-wide persistent drive letter (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\DOS Devices`). The letter can be set in the manifest. |

Mac and Linux share the same path. That leaves a single mapping, `/Volumes/Banc` to `B:\`, because Resolve only accepts one Mapped Mount per location. Banc writes it into Resolve's Media Storage preferences (Resolve closed, backup copy before writing).

### Folder tree, created and maintained by Banc

Nobody creates a folder by hand.

```
/Volumes/Banc/<project>/          (B:\<project>\ on Windows)
  Rushes/D03/CamA/A001.braw      class original (D03: shoot day 3)
  Rushes/D03/CamA/Proxy/A001.mov class proxy
  Audio/                         class media
  Graphics/                      class media
  Media/                         class media (videos added outside ingest)
  Exports/                       class export
  .banc/                         local metadata, never published
```

### Settings enforced in every Resolve project

Applied by script at creation, checked at every opening, restored if someone changes them:

- proxies generated in `Proxy` subfolders next to the originals;
- default render destination in `Exports/`;
- render cache and gallery stills on the workstation's local disk, outside the Banc volume;
- Fusion renders and "Render in Place" in `Media/`.

## 5. Files

### Four classes, two behaviors

| Class | Content | Distribution | Mutable |
|---|---|---|---|
| `original` | camera rushes | on demand | no |
| `proxy` | proxies | automatic | no |
| `media` | sound, graphics, added videos | automatic | yes, with revisions |
| `export` | renders | automatic for the author, on demand for the others | no |

- **Automatic**: as soon as a file is published, the server notifies the workstations that have the project open. They fetch it right away. A workstation that opens the project later fetches the files on opening.
- **On demand**: the file is available to everyone but only downloaded on request. Main button: "Fetch the originals for this timeline", which only takes the rushes used by the current timeline.
- The person role can change the rule. Example: a colorist automatically receives the originals of the timelines they open.

### Publishing

1. The agent hashes new files (size and date compared with its local index before hashing).
2. It sends the list to the server, which answers with the unknown hashes.
3. The workstation pushes nothing to the VPS. It announces its files, and the other machines come and get them from it over a direct connection: the depot first, and the workstations that have the project open at the same time.
4. The VPS cache only receives a file in two cases: the depot cannot be reached directly from this workstation (offline, or NAT traversal impossible), or the user asks for "Publish and shut down" while the depot is too slow to receive.
5. Class `original`: always sent directly to the depot, never to the cache.
6. Until the depot has confirmed everything, the interface shows what is left to send. Nothing is lost, since the workstation keeps its files.

### Fetching

Peer-to-peer comes first. A direct connection between two machines costs the VPS nothing. A relayed connection or a download from the cache uses its bandwidth.

Sources, in order of preference:

1. a workstation on the same LAN;
2. any machine reachable over a direct connection: running workstations and the depot;
3. the VPS cache, if the file is there;
4. a machine reachable only through the relay. In that case the server also caches the file on the way, so that the next ones do not go through the relay again.

Rules:

- **Several sources at once.** Files are spread across all available direct sources, and large files are split into ranges. Each chunk is verified by hash, so mixing sources is safe.
- **Every running workstation serves the others**, even with Resolve closed. It answers a request, with no scan and no watching, so it costs nothing at rest.
- **Upload limits per workstation**, so as not to disturb the person working on it: no limit on the LAN, capped at half the measured outbound upload rate, paused during a render. Adjustable by the admin.
- The agent tests the direct connection before choosing, and switches back to a direct source as soon as one becomes available during a transfer.
- Four files in parallel, resume after interruption.

Option for the depot: opening a UDP port on the NAS's router guarantees direct connections to it in almost every case. This is optional; everything else works without it.

### Modified files (class `media`)

A retouched logo or a replaced piece of music keeps its path but changes content.

- **Detection.** While Resolve is open, and at every publication, the agent compares the size and date of `media` files with its index. A changed file is rehashed and published as a new revision of the same path.
- **Reception.** The other workstations download the revision to a temporary file, then replace the old one in a single step (atomic rename). Then the session helper asks Resolve to reload the clip.
- **Locked file.** On Windows, Resolve may hold the file open and block the replacement. Banc retries when the project closes and shows "logo.png: new version pending" until then.
- **Conflict.** If two people modify the same file without having seen each other's revision, the first publication keeps the path. The second is stored next to it under a derived name (`logo (Paul, 11-11).png`), and both people are notified.
- **History.** All revisions stay in the depot. A revision can be restored from Banc.

### Immutable files (classes `original` and `proxy`)

A file in these classes must never change. If its content differs from the published hash, Banc propagates nothing. It reports possible corruption and offers to restore from the depot.

### Copies and safety of originals

- An original is "safe" when it exists in two verified copies: the workstation that did the ingest, and the depot.
- At that point, and not before, Banc shows "Two verified copies, you can format the card".
- Banc tells the ingest workstation that it holds the second copy and asks it to keep it.
- "Free up space" is refused for an original as long as no other copy exists somewhere besides the depot (another workstation, the render machine, a second depot).
- The admin sees the number of copies of each project in the dashboard.
- A second depot (another disk or another site) can be added later. It enrolls like the first one.

### Deletions

- Removing a clip from the project or deleting a file on a workstation deletes nothing in the depot. The file moves to the `retired` state and is no longer distributed.
- Only the admin deletes blobs, when archiving or deleting a project, with confirmation.

### Workstation disk space

- Proxies and media of a project not opened for N days (adjustable, 30 by default) are offered for freeing. Banc only frees what the depot has confirmed.
- An alert warns when a workstation's Banc volume drops below 10% free space.

### VPS cache

The cache is a fallback, not the normal path. It is used when no direct source exists: workstations switched off and depot unreachable directly, or a network that blocks direct connections.

- Adjustable quota, least requested files evicted first.
- The admin sees, per project and per month, the share of transfers done directly, through the relay and through the cache. This is the indicator to watch for cost.
- Expiry after N days without a request.
- A file is never deleted from the cache until the depot has confirmed it.
- Files of projects marked active stay in the cache within the quota.
- The admin sees cache usage and the outbound volume from the VPS over the month.

### NAS depot

- Blobs stored by hash in a folder chosen by the admin. No interface.
- Full integrity check once a month, at low priority.
- Receives the VPS's SQLite and PostgreSQL backups every night.

## 6. The life of a project

### Creating a project

1. In Banc, "New project": name, mode, members, project template.
2. The template (defined by the admin) carries resolution, frame rate, color management, proxy format and the enforced settings of section 4.
3. Banc creates the folder tree in the Banc volume and declares it to the server.
4. Banc creates the project in Resolve by script. In collaborative mode, in the shared library. In reservation mode, in the local database, followed by a first `.drp` publication.

### Adding rushes

"Add rushes" button in Banc:

1. Choice of source (card, disk, folder), shoot day and camera. Banc files them under `Rushes/D03/CamA/`, keeping the card's original structure.
2. Copy verified by hash. The card is never modified. A report is kept in `.banc/`.
3. Originals sent to the depot.
4. Proxy generation (see below) and publication.
5. Clips imported into the project by script, into bins by day and by camera.
6. "Two verified copies" message once the depot has confirmed everything.

Someone who copies rushes by hand into the Banc volume, or imports them from an external disk, is caught by consolidation.

### Proxies

- Where they are generated, chosen by the admin per project: the ingesting workstation (default), or a render machine if there is one.
- Tool to be chosen in prototype P4: Blackmagic Proxy Generator, Resolve's render queue as individual clips, or ffmpeg for the formats it reads.
- Folder watching is allowed during generation, and only during generation.
- Proxy-to-original link: automatic when the proxy is in the `Proxy` subfolder. Fallback: `MediaPoolItem.LinkProxyMedia`.
- Proxy format to be chosen in the prototype, with one constraint: readable and generatable on all three OSes.

### Automatic consolidation

It catches any file that entered the project by another route, for example music dragged from Downloads into the timeline.

1. **When.** Only while Resolve is open on a Banc project. Triggers: change in the project's clip count, publication, project close. Safety net every 5 minutes.
2. **What.** The session helper reads the path of the clips added since the last pass (`GetClipProperty("File Path")`). It never walks the whole project again; it keeps an index of clips already seen.
3. **Copy.** A clip outside the Banc volume is copied into the project: `Audio/` for sound, `Graphics/` for images, `Media/` for videos.
4. **Replacement.** Banc points the clip at the copy (`MediaPoolItem.ReplaceClip`). The timeline does not move.
5. **Publication** as class `media`, hence automatic distribution.

Safeguards:

- Consolidation runs on the workstation of the person who imported the file. In collaborative mode that person holds the bin, so the replacement does not run into someone else's lock.
- If the replacement fails (locked bin), Banc retries later and the file is published anyway.
- Above a threshold (5 GB by default), Banc asks before copying.
- Two different files with the same name: the second is renamed.
- A file already in the Banc volume is published without a copy.

## 7. Two project modes

### Reservation mode

- Open: latest `.drp` version imported into the local database, lock taken if the user wants to edit.
- Publish: `.drp` export, new version, lock released or kept.
- A single lock holder. The admin can force it. A lock with no sign of life for 24 h is reported to the admin.
- A local project with the same name is renamed, never overwritten.
- Works offline. Publications go out when the connection comes back.

### Collaborative mode

Resolve's native collaboration on a shared PostgreSQL database, reachable without a VPN.

**On the VPS**

- PostgreSQL 18 in Docker (the version bundled with Resolve 21.1's Project Server, which accepts network libraries from PostgreSQL 13 onwards).
- Backups taken from the `elliotmatson/Docker-Davinci-Resolve-Project-Server` project (`prodrigestivill/postgres-backup-local`, `.backup` format restorable from Resolve). Frequency: every hour, because all workstations share the same PostgreSQL account and one mistake can affect everything.
- No pgAdmin service.

**The tunnel**

- Resolve's "Add Project Library" window has no port field. Resolve connects on 5432.
- `banc-agent` listens on `127.0.0.2:5432` so as not to get in the way of a local PostgreSQL. Linux and Windows already accept this address. On macOS, the service adds the alias at startup (`ifconfig lo0 alias 127.0.0.2 up`).
- Each connection goes through an iroh stream to the VPS, which connects it to its local PostgreSQL.
- The tunnel is only open while Resolve is running.

**Declaring the library in Resolve**, options in order:

1. The `.resolvedbkey` access key, which the Project Server can generate. Dropped onto the project manager, it adds the database without any typing. If Banc can generate it itself, the user has nothing to type.
2. The API: `ProjectManager.GetDatabaseList()` and `SetCurrentDatabase({...})`.
3. The configuration file where Resolve keeps its list of libraries.
4. Fallback: Banc shows the four values with a "copy" button per field, to paste once into Network > Add Project Library > Connect.

**Safeguards**

- Before opening a collaborative project, Banc checks that the automatically distributed files are present.
- Above 40 ms of latency to the VPS, Banc warns and suggests reservation mode.
- Tunnel down: the interface reports it immediately.

## 8. The Resolve version

- The admin sets in the manifest the version every workstation must have.
- The agent detects the installed version at enrollment, at every connection and every time Resolve starts.
- A workstation without the right version cannot open a project through Banc. Banc shows why, downloads the right installer from the server and runs it.
- **Version change**, started by the admin:
  1. The admin publishes a manifest with the new version and a switchover date.
  2. All workstations are notified. Each one downloads the installer in advance.
  3. At switchover, Banc asks for collaborative projects to be closed and checks that nobody is connected to the database any more.
  4. Full PostgreSQL backup, sent to the depot.
  5. Workstations install the new version. The first updated workstation opens the library, which Resolve upgrades.
  6. Workstations not yet updated stay blocked until they are.
- **Compatibility check.** After every Resolve version change, the agent checks that the preference files it writes still have the expected format. If not, it writes nothing, switches to guided mode (the user makes the setting once) and notifies the admin.

## 9. Rendering

A render machine is a bonus. Everything works without one.

- **Intercepted local render.** While Resolve is open, the session helper follows the render queue (`GetRenderJobList`, `GetRenderJobStatus`). When a render finishes, Banc publishes the file as class `export` with the timeline, author and date, and notifies the team. A render made outside `Exports/` is copied there.
- **Deferred render.** "Render later" queues the render on the user's workstation. It starts at a chosen time or when the workstation is idle.
- **Delegated render.** A workstation can declare itself available for other people's renders (at night, or when idle). Banc first fetches the timeline's originals there, then starts the render.
- **Dedicated machine**, if there is one: same mechanism, always preferred.
- Order of choice: free dedicated machine, idle volunteer workstation, otherwise the user's workstation.
- A machine only receives a render if it has the right Resolve version and all required packages.
- `render_job.py` script: open the project, `LoadRenderPreset`, `SetRenderSettings`, `AddRenderJob`, `StartRendering`, track progress.
- System notification, and optional webhook (Discord, Slack, ntfy).

## 10. What runs, and when

| Workstation state | Banc activity |
|---|---|
| Resolve closed, nothing requested | One connection to the server and a heartbeat every 5 minutes. Nothing else. |
| Another workstation requests a file | Sends the file within the rate limits, then back to rest. |
| Transfer in progress | The transfer, then back to rest. |
| Resolve open on a Banc project | PostgreSQL tunnel (collaborative), consolidation, detection of modified media, render queue tracking, automatic reception of new files. |
| Proxy generation | Watching of the folder concerned, until it finishes. |
| Time of a deferred render | Starting Resolve and the render. |

Targets at rest: service under 15 MB of RAM, session helper under 10 MB, CPU at 0%.

## 11. Paths per OS

To be checked on a real machine and frozen in `banc-platform`.

| Item | macOS | Windows | Linux |
|---|---|---|---|
| OFX plugins | `/Library/OFX/Plugins` | `C:\Program Files\Common Files\OFX\Plugins` | `/usr/OFX/Plugins` |
| LUT | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT` | `C:\ProgramData\Blackmagic Design\DaVinci Resolve\Support\LUT` | `/opt/resolve/LUT` or `~/.local/share/DaVinciResolve/LUT` |
| Fusion templates | `~/Library/Application Support/Blackmagic Design/DaVinci Resolve/Fusion/Templates` | `%APPDATA%\Blackmagic Design\DaVinci Resolve\Support\Fusion\Templates` | `~/.local/share/DaVinciResolve/Fusion/Templates` |
| Scripts | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/Fusion/Scripts` | `%PROGRAMDATA%\Blackmagic Design\DaVinci Resolve\Fusion\Scripts` | `/opt/resolve/Fusion/Scripts` |
| Scripting API | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting/Modules` | `%PROGRAMDATA%\Blackmagic Design\DaVinci Resolve\Support\Developer\Scripting\Modules` | `/opt/resolve/Developer/Scripting/Modules` |

The `README.txt` in the `Developer/Scripting` folder of the installed version is authoritative.

## 12. Prototypes to build before coding

These are throwaway scripts, run by hand. Each one answers a question that part of the plan depends on. Record the answers in `docs/prototypes.md`.

- **P1. Paths.** Create the APFS volume on a Mac and the `B:` letter on a PC. Import a clip on the Mac into a shared project, check that it is online on the PC with the Mapped Mount. Find the Media Storage preferences file and check that it can be written to.
- **P2. Proxies without originals.** On a workstation that only has the `Proxy` folders, are clips linked to their proxies and editable?
- **P3. Shared library.** Generate a `.resolvedbkey` key with the Project Server, examine its format, test it with the address `127.0.0.2`. Test `SetCurrentDatabase` on a database missing from the list.
- **P4. Remote collaboration.** Resolve on a remote PostgreSQL database through an SSH tunnel. Measure the acceptable latency.
- **P5. Consolidation.** `ReplaceClip` on a clip used in a timeline: does the timeline stay intact? In collaborative mode, what happens if the bin is locked by someone else? How long does reading the paths of 10,000 clips take?
- **P6. Proxies.** Can the Proxy Generator be driven without its window? Which formats on each OS? Otherwise, render queue as individual clips or ffmpeg, then `LinkProxyMedia`.
- **P7. Rendering.** Follow the render queue through the API while a user starts a render by hand. Test `-nogui`.
- **P8. Open file.** On Windows, replace a file that Resolve is using. Find out how to make it reload the clip.
- **P9. Plugins.** Test silent installation of the plugins the team actually uses.
- **P10. Technical.** Stability of `iroh-blobs`. Python version accepted by Resolve. Resolve's "External scripting" setting.

## 13. Phases

Indicative durations for one developer assisted by Claude Code. Phases 0 to 4 make up a first version useful day to day.

### Phase 0. Skeleton (3 to 4 days)

- Cargo workspace, `xtask`.
- All the continuous integration of section 15: workflows, protection of `main`, commit message check, `cargo xtask check`.
- Logs with rotation, TOML server configuration.

Acceptance:
- The three binaries build for every target.
- A PR with a formatting error, a clippy warning, a broken test or a badly named commit is refused by CI.
- Pushing directly to `main` is impossible.

### Phase 1. Enrollment, inventory, admin key (2 weeks)

- Server: enrollment, invitations, machines, iroh endpoint and relay, minimal web admin.
- Agent: machine key, connection, heartbeat, system service, session helper, `banc://` link, inventory, Resolve version detection.
- Admin key: generation in the application, recovery phrase, fingerprint in the invitation.
- `cargo xtask idle-bench`: the command has existed since phase 0 for the agent on Linux. Extend it to the session helper, macOS and Windows.

Acceptance:
- One machine of each OS enrolls with a link, without a terminal.
- The admin sees the machines and their Resolve version.
- Resource targets at rest are met.

### Phase 2. Signed manifest, installation, Resolve version (2 to 3 weeks)

- Package store, manifest editor, signature on publication.
- Agent: signature verification, comparison of local state against the manifest, installation (`copy`, `pkg`, `msi`, `exe`, `deb`, `rpm`, `run`, `font`, `manual`).
- Nothing is installed while Resolve is open. Banc only uninstalls what it installed.
- Opening projects is blocked if the Resolve version does not match.
- Machines-by-packages grid in the admin.

Acceptance:
- An `.ofx.bundle` plugin, a font, a LUT and a preset install by themselves on all three OSes.
- A manifest modified directly in the server database is refused by the workstations.
- A package whose hash does not match is refused.

### Phase 3. Volume, files, depot, cache (3 to 4 weeks)

- Creation of the Banc volume on all three OSes, writing of the Mapped Mount.
- `banc-blobs`: store, hashes, verified transfer, resume, source selection.
- Depot role and Docker image. VPS cache.
- File classes, automatic and on-demand distribution, revisions, corruption detection, two-copy rule, deletions, freeing space.

Acceptance:
- A workstation publishes 50 GB then shuts down. Another one, on another network with no open port, fetches them.
- Two workstations on the same LAN exchange files without going out to the Internet.
- Between two remote workstations on a direct connection, 50 GB transferred do not move the VPS outbound counter.
- A downloading workstation takes its files from two other workstations at once.
- A modified `media` file reaches the others as a new revision. Two concurrent modifications give two files and two notifications.
- "Free up space" is refused for an original that has only one copy elsewhere.
- The cache never deletes a file missing from the depot.

### Phase 4. Reservation projects (3 to 4 weeks)

- Python scripts for the Resolve bridge.
- Project creation with template and enforced settings.
- Ingest: verified copy, sending to the depot, proxies, import into bins.
- Automatic consolidation.
- Open, publish, lock, offline work.
- "Fetch the originals for this timeline".

Acceptance:
- A project created and filled on a Mac opens on Windows and Linux with its proxies linked.
- Music dragged from Downloads into the timeline ends up in `Audio/`, published, with no change in the timeline.
- Two users cannot hold the lock at the same time.

### Phase 5. Collaboration (2 to 3 weeks)

- PostgreSQL and backups on the VPS, copy to the depot.
- `banc-tunnel`, `127.0.0.2` alias, library declaration using the option chosen.
- Collaborative project creation, automatic direct distribution between open workstations.
- Resolve version change procedure.

Acceptance:
- Two workstations behind two different home routers work in the same project, without a VPN or an open port.
- Music added by one is online for the other in less than a minute.
- PostgreSQL cannot be reached from the Internet on any port.
- A backup is present on the NAS and can be restored from Resolve.

### Phase 6. Rendering (2 to 3 weeks)

- Intercepted local render and publication in `Exports/`.
- Deferred render.
- Render delegated to a volunteer workstation or a dedicated machine.
- Notifications.

Acceptance:
- A render started by hand in Resolve appears in `Exports/` for its author and in the project list for the others.
- A deferred render starts at the scheduled time.
- A machine missing a required plugin does not receive the render.

### Phase 7. Distribution and updates (2 weeks)

- Signed automatic update, rollback if the new version does not start.
- Installers: signed and notarized `.pkg`, signed `.msi`, `.deb`, `.rpm`, AppImage.
- Server as a complete Docker Compose setup. Depot as a Docker image for Synology and QNAP.
- Nightly SQLite backup to the depot.

Acceptance:
- A new version reaches every workstation within the hour.
- A fresh install on a Mac shows no Gatekeeper warning.

### Phase 8. Finishing touches (ongoing)

- Project archiving: everything gathered on the depot, final `.drp`, cache and workstations freed.
- Admin dashboard: copies per project, workstation space, transfers, cache, renders.
- One-click diagnostics.
- One-page user documentation, admin documentation.

## 14. Starting prompts for Claude Code

One session per line:

1. "Read CLAUDE.md and docs/PLAN.md. Carry out phase 0."
2. "Prepare prototypes P1 and P2: the scripts and the procedure, which I run by hand. Tell me what to observe and where to record the result."
3. "Prototypes P3 and P4."
4. "Prototypes P5 to P8."
5. "Phase 1, server side. Write the tests."
6. "Phase 1, agent side. Start with macOS."
7. Then phase by phase.

After each phase, ask Claude Code to update `CLAUDE.md` with the real commands, the decisions taken and the `idle-bench` measurements. After each prototype, have it correct this plan if the answer changes an assumption.

## 15. Continuous integration and commits

To be set up in phase 0, before any product code.

### Repository rules

- `main` is protected: no direct push, no force push, everything goes through a PR.
- A PR can only be merged if all required checks are green and the branch is up to date with `main`.
- "Squash" merge. The PR title becomes the commit message on `main`, so it follows the same convention as commits.
- One branch per task, named `type/short-topic` (`feat/ingest-verified-copy`, `fix/tunnel-reconnect`).

### `ci.yml` workflow, on every push to any branch

Fast checks, on Linux, to get an answer within a few minutes:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo nextest run --workspace --locked` and the doc tests
- SQL migrations applied to an empty database, then schema check
- Frontend: `pnpm lint`, `svelte-check`, `vitest`, `pnpm build`
- Python scripts for the Resolve bridge: `ruff check`, `ruff format --check`, compilation of each script
- `cargo deny check`: known vulnerabilities, allowed licenses, duplicate or banned dependencies
- Secret scanning of the diff (`gitleaks`)
- Commit message check (see below)

### `pr.yml` workflow, on every PR and every PR update

All of `ci.yml`, plus what costs more:

- **Three-OS matrix**: clippy and tests on Linux, macOS and Windows. The `banc-platform` code differs per OS; a check on Linux alone does not cover it.
- **Build of every target**: Linux amd64 and arm64, macOS arm64 and amd64, Windows amd64.
- **End-to-end integration tests**, on Linux, without Resolve (using the `banc-resolve` fake): a server, a depot and two agents started in containers. Scenarios: enrollment, signed manifest accepted and forged manifest refused, publishing and fetching a file, revision of a `media` file, refusal of a package with a modified hash, TCP tunnel to a test PostgreSQL.
- **Idle measurement**: `cargo xtask idle-bench` on the Linux agent. The PR fails if idle memory or CPU exceeds the targets of section 10.
- **PR title** checked against the same convention as commits.
- Automatic cancellation of running executions when a new commit arrives on the same PR.

### `nightly.yml` workflow, every night on `main`

- Full matrix of the three OSes and every target.
- `cargo deny check advisories` with the day's advisory database, to be warned about a vulnerability published in a dependency already in use.
- Long integration tests (large files, network cut simulated during a transfer, resume).
- An issue opened automatically on failure.

### `release.yml` workflow, on a `vX.Y.Z` tag (phase 7)

- Build of every target, installers, signing and notarization.
- Binaries signed with the release key, kept in the CI secrets.
- Changelog generated from the commits.

### Other settings

- Build cache (`Swatinem/rust-cache`) to keep runs short.
- `Cargo.lock` committed and `--locked` everywhere.
- Dependency updates proposed every week by Renovate or Dependabot, as grouped PRs.
- GitHub Actions minutes on a private repository cost more on macOS and Windows than on Linux. That is why the checks on every push stay on Linux and the three-OS matrix runs on PRs. Check current pricing when starting.

### `cargo xtask check`

A local command that runs the same checks as `ci.yml`. Claude Code runs it before every commit. A commit that does not pass `cargo xtask check` must not be created.

### Commit naming

Conventional Commits convention, checked by CI on every commit of a PR and on its title.

```
type(scope): description

Optional body: why this change, not how.

Refs: phase 3, P5
```

- **Types**: `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`.
- **Scopes**, one per commit, among: `server`, `agent`, `session`, `blobs`, `tunnel`, `resolve`, `platform`, `proto`, `ui`, `scripts`, `packaging`, `ci`, `docs`.
- The scope is required for `feat`, `fix`, `refactor`, `perf` and `test`. It is optional for `docs`, `build`, `ci` and `chore`, which often touch the whole repository.
- **Description** in English, starting with a verb in the imperative mood (`add`, not `added` or `adds`), no capital letter and no final period, 72 characters at most for the first line.
- A breaking change carries a `!` after the scope and a `BREAKING CHANGE:` line in the body.
- A commit does one thing. A refactor and a feature go in two commits.
- The footer cites the plan phase, and the prototype if there is one. The `Refs:` line is required. Prototype work alone is written `Refs: P1, P2`.
- Dependabot commits keep their own format and are not checked.

Examples:

```
feat(blobs): prefer direct sources over the cache
fix(tunnel): reopen the stream after a relay outage
test(agent): cover the rejection of a badly signed manifest
ci: add the idle measurement to PRs
refactor(platform)!: group Resolve paths by OS
```

Refused messages: `update`, `wip`, `fix bug`, `corrections`, `feat(agent): added the heartbeat`, or any message without a type.
