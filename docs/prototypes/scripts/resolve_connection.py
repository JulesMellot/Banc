"""Connection to DaVinci Resolve for the prototype scripts.

Sets the environment variables described in the README.txt of
Developer/Scripting if they are missing, then loads the Resolve module.
"""

import json
import os
import platform
import subprocess
import sys


def default_paths():
    """Returns (API folder, fusionscript library) for this OS."""
    system = platform.system()
    if system == "Darwin":
        api = "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting"
        lib = (
            "/Applications/DaVinci Resolve/DaVinci Resolve.app"
            "/Contents/Libraries/Fusion/fusionscript.so"
        )
    elif system == "Windows":
        programdata = os.environ.get("PROGRAMDATA", "C:\\ProgramData")
        api = os.path.join(
            programdata, "Blackmagic Design", "DaVinci Resolve", "Support", "Developer", "Scripting"
        )
        lib = "C:\\Program Files\\Blackmagic Design\\DaVinci Resolve\\fusionscript.dll"
    else:
        api = "/opt/resolve/Developer/Scripting"
        lib = "/opt/resolve/libs/Fusion/fusionscript.so"
    return api, lib


def connect():
    """Returns the Resolve object, or exits with a clear message."""
    api, lib = default_paths()
    os.environ.setdefault("RESOLVE_SCRIPT_API", api)
    os.environ.setdefault("RESOLVE_SCRIPT_LIB", lib)
    modules = os.path.join(os.environ["RESOLVE_SCRIPT_API"], "Modules")
    if modules not in sys.path:
        sys.path.append(modules)
    try:
        import DaVinciResolveScript  # noqa: PLC0415
    except ImportError as error:
        fail(f"Resolve scripting module not found in {modules}: {error}")
    resolve = DaVinciResolveScript.scriptapp("Resolve")
    if resolve is None:
        fail(
            "Resolve does not answer. It must be running, and the setting "
            "Preferences > System > General > External scripting using must be set to Local."
        )
    return resolve


def current_project(resolve):
    """Returns the project open in Resolve, or exits."""
    project = resolve.GetProjectManager().GetCurrentProject()
    if project is None:
        fail("no project open in Resolve")
    return project


def resolve_is_running():
    """True if a Resolve process is running on this workstation."""
    if platform.system() == "Windows":
        output = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq Resolve.exe", "/NH"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
        return "resolve.exe" in output.lower()
    name = "Resolve" if platform.system() == "Darwin" else "resolve"
    return subprocess.run(["pgrep", "-x", name], capture_output=True, check=False).returncode == 0


def write_json(value, path=None):
    """Writes the value as readable JSON, to the given file or to standard output."""
    text = json.dumps(value, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if path is None:
        sys.stdout.write(text)
        return
    with open(path, "w", encoding="utf-8") as file:
        file.write(text)
    sys.stderr.write(f"written: {path}\n")


def fail(message):
    """Writes the error to standard error and exits with code 1."""
    sys.stderr.write(f"error: {message}\n")
    sys.exit(1)
