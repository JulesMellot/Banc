# Python scripts for the Resolve bridge

These scripts are embedded in `banc-session` and run by it alone, when it needs to talk to DaVinci Resolve. They arrive in phase 4. The rules:

- A script reads its arguments from the command line and writes a single JSON object to standard output. Errors go to standard error, with a non-zero exit code.
- Standard library only, plus the `DaVinciResolveScript` module provided by Resolve.
- The syntax stays compatible with the oldest Python version Resolve accepts. Until prototype P10 has an answer in `docs/prototypes.md`, that is Python 3.7 (`ruff.toml`).

Checks (`cargo xtask check python`): `ruff check`, `ruff format --check`, then compilation of each script.

`diagnostic.py` prints the Python version running it and says whether Resolve's scripting module loads. It serves prototype P10 and workstation diagnostics.
