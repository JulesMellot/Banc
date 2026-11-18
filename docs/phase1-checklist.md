# Phase 1 hand test

The phase 1 acceptance criteria (section 13 of `docs/PLAN.md`) need real machines: a click on a link, the OS's administrator prompt, a window, a tray icon, Resolve installed. CI covers the rest. This is the procedure, with what to write down.

What you need: the VPS with a domain name, a Mac, a Windows PC and a Linux workstation, each with Resolve Studio. The Mac and Windows builds come from the `Bundles` workflow on GitHub; building on the machine itself needs the tools of `docs/development.md`.

## 1. Server on the VPS

Follow `packaging/server/README.md`: Coolify, or a plain VPS with Docker. Then check that `https://<domain>/api/v1/info` answers with `"setup_required": true`.

## 2. First admin and admin key (on the Mac)

1. Open `https://<domain>`, enter the code from `/var/lib/banc/setup-token` in the server container, choose a name and a password.
2. On the Mac: take the build from the `Bundles` workflow (`banc-macos-aarch64` for Apple silicon, `banc-macos-x86_64` for Intel), or run `cargo xtask bundle-app` on the Mac. Install `Banc.app` into `/Applications`. A downloaded build is not notarized until phase 7: remove the quarantine once with `xattr -dr com.apple.quarantine /Applications/Banc.app`.
3. Open Banc, "Clé d'administration", "Créer la clé". Write the code down, type the last group back.
4. "Enregistrer": server address, admin name and password.
5. In the web admin, the admin key section shows the same fingerprint as the window.

## 3. One machine of each OS enrolls with a link

For each OS, after installing the bundle (`.app` or `.dmg` on macOS, the NSIS installer on Windows, the `.deb` on Linux: not the AppImage, whose path changes at each start):

1. In the web admin, create an invitation (role of your choice). Copy the link.
2. On the machine, click the link (send it to yourself by mail or chat). Expected: the Banc window opens on "Rejoindre l'équipe" with the server and the fingerprint of step 2.
3. The first time, the window says the service is not installed: "Installer". Expected: the OS's own administrator prompt, once.
4. "Rejoindre". Expected: the window shows the workstation, "Relié au serveur" within a few seconds.
5. In the web admin: the machine is "En ligne", with its OS and Resolve version.

No terminal at any step. Note any step that asks for one.

Also try: a second click on the same link (expected: invitation already used), and the key pasted by hand with the server address instead of the link.

## 4. The admin sees the machines and their Resolve version

For each machine, compare the version shown in the web admin with Resolve's "About" box. The build number is not shown, and the edition only appears when the installed files name Studio, which they do not on macOS or Windows. If one is wrong or missing, note the OS and send:

- macOS: `plutil -p "/Applications/DaVinci Resolve/DaVinci Resolve.app/Contents/Info.plist" | grep -i -E "version|name"`;
- Windows: `Get-ItemProperty HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\* | Where-Object DisplayName -like "DaVinci*" | Select DisplayName, DisplayVersion`;
- Linux: `ls /opt/resolve/docs /usr/share/applications | grep -i resolve`.

Then update Resolve (or uninstall it) on one machine: the web admin follows within 5 minutes, and the audit log has a `resolve_changed` event.

## 5. Revocation

Revoke one machine in the web admin. Expected: it goes offline at once, its window says it was removed from the team, and restarting it does not bring it back.

## 6. Resource use at rest

Leave each enrolled machine 5 minutes without touching Banc, Resolve closed, then:

- macOS: `ps -o rss=,time= -p $(pgrep -x banc-agent) $(pgrep -x banc-session)`, twice one minute apart;
- Windows (PowerShell): `Get-Process banc-agent, banc-session | Select Name, WorkingSet64, CPU`, twice one minute apart;
- Linux: `cargo xtask idle-bench` gives the same figures.

Targets: agent under 15 MB, session helper under 10 MB, CPU time not moving between the two readings (or by a few hundredths of a second). Write the figures into `docs/measurements.md`.

## 7. Tray icon (macOS, Windows)

Start `banc-session` (next to the application; phase 7 starts it at login). Expected: an icon in the menu bar or the notification area, its menu with the agent's state, "Ouvrir Banc" opens the window, "Quitter" ends it.

## Recording

Write the result of each step, the OS versions and the Resolve versions into the phase 1 PR or an issue. A failure of steps 3 to 6 blocks phase 2.
