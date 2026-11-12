"""Prints the Python version and the state of Resolve's scripting module as JSON.

Used by prototype P10 and for workstation diagnostics. Changes nothing.
"""

import json
import platform
import sys


def resolve_module_error():
    """Returns None if Resolve's scripting module loads, otherwise the error."""
    try:
        import DaVinciResolveScript  # noqa: F401, PLC0415
    except Exception as error:
        return f"{type(error).__name__}: {error}"
    return None


def main():
    report = {
        "python": platform.python_version(),
        "executable": sys.executable,
        "system": platform.system(),
        "machine": platform.machine(),
        "resolve_module_error": resolve_module_error(),
    }
    json.dump(report, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
