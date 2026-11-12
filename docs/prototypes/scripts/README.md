# Prototype scripts

Throwaway scripts, run by hand on a workstation that has Resolve Studio. They are not embedded in Banc. They follow the same rules as `resolve-scripts/`: standard library only, Python 3.7 syntax, JSON on standard output, errors on standard error.

| Script | Prototype | Effect |
|---|---|---|
| `inventory.py` | P1, P2 | Describes the open project: clips, paths, proxies, timelines. Changes nothing. |
| `preferences.py` | P1 | Snapshots Resolve's preference files, compares two snapshots, rewrites a file with Resolve closed. |
| `test_media.py` | P2 | Makes ProRes rushes and their proxies with ffmpeg. |
| `link_proxies.py` | P2 | Links clips to the proxies in their `Proxy` subfolder through `LinkProxyMedia`. |
| `resolve_connection.py` | all | Shared module: connection to Resolve, JSON output. |

## Preparing the workstation

1. 64-bit Python 3. On a Mac, the one from Apple's command-line tools is enough (`python3`). On Windows, the one from python.org, with the "Add to PATH" box checked.
2. In Resolve: Preferences > System > General > "External scripting using" set to **Local**. Restart Resolve.
3. Check the connection, with Resolve open on a project:

   ```
   python3 inventory.py
   ```

   On Windows, replace `python3` with `python`. The script sets `RESOLVE_SCRIPT_API`, `RESOLVE_SCRIPT_LIB` and the module path by itself, using Resolve's default locations. If Resolve is installed elsewhere, set these two variables before running the script, as explained in the `README.txt` of `Developer/Scripting`.

Record in the P10 answer the Python version that worked (`python3 --version`).

## Checks

The scripts are not part of `cargo xtask check`. Before changing them:

```
ruff check docs/prototypes/scripts
ruff format --check docs/prototypes/scripts
```
