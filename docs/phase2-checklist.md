# Phase 2 hand test

CI proves the rules with a real server and a real agent on Linux, macOS and Windows, in temporary folders. This checklist covers what only a real workstation can show. Record the results here, with the date and the versions.

Prepare in the web admin, section "Paquets de l'équipe": upload the files, add the packages, save the draft. Publish from the Banc application of the workstation that holds the admin key.

## On a Mac, a Windows PC and a Linux workstation

- [ ] An `.ofx.bundle` plugin, zipped, appears in Resolve's OpenFX panel.
- [ ] A font appears in Resolve's title tools. On Windows, note whether a new session is needed.
- [ ] A LUT appears in Resolve's LUT list.
- [ ] A Fusion template appears in Resolve's Effects library, from the all-users folder.
- [ ] With Resolve open, the package stays "En attente", and installs within 5 minutes after Resolve closes.
- [ ] A package dropped from the manifest disappears from the workstation.
- [ ] A plugin already installed by hand is reported as a failure and stays untouched.

## Security

- [ ] In the server database, change the payload of the latest manifest: the workstations show the refusal and install nothing.
- [ ] In `data_dir/packages`, replace a file with another of the same size: the workstations report the failure and install nothing.

## Prototype P9

For each plugin the team really uses, record in `docs/prototypes.md` the installer type, the silent arguments, and whether it installs from the system service without a session.

## Resolve version

- [ ] A manifest requiring another Resolve version shows the block in the window and in the machines grid.
