"""Finds the file where Resolve stores its Media Storage preferences, and tests writing to it.

Prototype P1. Three commands:

    python3 preferences.py snapshot before         # copy of the preference files
    python3 preferences.py diff before after       # what changed between two snapshots
    python3 preferences.py restore after <file>    # rewrites a file, Resolve closed

Snapshots go to --dir (default ../p1/snapshots). Only files under 2 MB are
kept, excluding caches, LUTs, Fusion and project databases.
"""

import argparse
import base64
import datetime
import difflib
import hashlib
import json
import os
import platform
import shutil
import sys

from resolve_connection import fail, resolve_is_running

MAX_SIZE = 2 * 1024 * 1024

# Ignored folders: large, and unrelated to preferences.
IGNORED = {
    "cacheclip",
    "developer",
    "fusion",
    "gallery",
    "logs",
    "lut",
    "resolve disk database",
    "resolve project library",
    "resolve project libraries",
}


def roots():
    home = os.path.expanduser("~")
    system = platform.system()
    if system == "Darwin":
        return [
            os.path.join(home, "Library", "Preferences", "Blackmagic Design"),
            os.path.join(home, "Library", "Application Support", "Blackmagic Design"),
            "/Library/Application Support/Blackmagic Design",
            "/Library/Preferences/Blackmagic Design",
        ]
    if system == "Windows":
        return [
            os.path.join(os.environ.get(variable, ""), "Blackmagic Design")
            for variable in ("APPDATA", "LOCALAPPDATA", "PROGRAMDATA")
            if os.environ.get(variable)
        ]
    return [
        os.path.join(home, ".local", "share", "DaVinciResolve"),
        "/opt/resolve/configs",
    ]


def list_files(root):
    for folder, subfolders, files in os.walk(root):
        subfolders[:] = [name for name in subfolders if name.lower() not in IGNORED]
        for name in files:
            yield os.path.join(folder, name)


def describe(path):
    try:
        size = os.path.getsize(path)
        if size > MAX_SIZE:
            return {"size": size, "too_large": True}
        with open(path, "rb") as file:
            content = file.read()
    except OSError as error:
        return {"error": str(error)}
    return {
        "size": size,
        "modified": os.path.getmtime(path),
        "sha256": hashlib.sha256(content).hexdigest(),
        "content": base64.b64encode(content).decode("ascii"),
    }


def snapshot_path(folder, name):
    return os.path.join(folder, name + ".json")


def read_snapshot(folder, name):
    path = snapshot_path(folder, name)
    if not os.path.exists(path):
        fail(f"snapshot not found: {path}")
    with open(path, encoding="utf-8") as file:
        return json.load(file)


def snapshot(args):
    if resolve_is_running():
        sys.stderr.write("warning: Resolve is running, it may not have written yet.\n")
    files = {}
    present = [root for root in roots() if os.path.isdir(root)]
    for root in present:
        for path in list_files(root):
            files[path] = describe(path)
    os.makedirs(args.dir, exist_ok=True)
    with open(snapshot_path(args.dir, args.name), "w", encoding="utf-8") as output:
        json.dump(
            {
                "date": datetime.datetime.now().isoformat(timespec="seconds"),
                "system": platform.system(),
                "roots": present,
                "files": files,
            },
            output,
            ensure_ascii=False,
            indent=1,
        )
    sys.stderr.write(f"{len(files)} files captured in {args.name}\n")
    return 0


def text(description):
    """The file content as text, or None if it cannot be read as text."""
    if "content" not in description:
        return None
    try:
        return base64.b64decode(description["content"]).decode("utf-8").splitlines(True)
    except UnicodeDecodeError:
        return None


def diff(args):
    before = read_snapshot(args.dir, args.before)["files"]
    after = read_snapshot(args.dir, args.after)["files"]
    for path in sorted(set(before) - set(after)):
        sys.stdout.write(f"deleted: {path}\n")
    for path in sorted(set(after) - set(before)):
        sys.stdout.write(f"added: {path}\n")
    for path in sorted(set(before) & set(after)):
        old, new = before[path], after[path]
        if old.get("sha256") == new.get("sha256"):
            continue
        sys.stdout.write(f"modified: {path}\n")
        old_lines, new_lines = text(old), text(new)
        if old_lines is None or new_lines is None:
            old_size, new_size = old.get("size"), new.get("size")
            sys.stdout.write(f"  binary, {old_size} -> {new_size} bytes\n")
            continue
        sys.stdout.writelines(difflib.unified_diff(old_lines, new_lines, args.before, args.after))
    return 0


def restore(args):
    if resolve_is_running():
        fail("Resolve is running. Close it before writing to its preferences.")
    description = read_snapshot(args.dir, args.name)["files"].get(args.file)
    if description is None or "content" not in description:
        fail(f"this file has no content in snapshot {args.name}: {args.file}")
    if os.path.exists(args.file):
        timestamp = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        backup = f"{args.file}.banc-{timestamp}"
        shutil.copy2(args.file, backup)
        sys.stderr.write(f"backup: {backup}\n")
    with open(args.file, "wb") as file:
        file.write(base64.b64decode(description["content"]))
    sys.stderr.write(f"rewritten from snapshot {args.name}: {args.file}\n")
    return 0


def main():
    here = os.path.dirname(os.path.abspath(__file__))
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--dir",
        default=os.path.join(here, "..", "p1", "snapshots"),
        help="snapshot folder",
    )
    commands = parser.add_subparsers(dest="command")
    commands.required = True

    command = commands.add_parser("snapshot", help="capture the preferences")
    command.add_argument("name")
    command.set_defaults(action=snapshot)

    command = commands.add_parser("diff", help="compare two snapshots")
    command.add_argument("before")
    command.add_argument("after")
    command.set_defaults(action=diff)

    command = commands.add_parser("restore", help="rewrite a file from a snapshot")
    command.add_argument("name")
    command.add_argument("file")
    command.set_defaults(action=restore)

    args = parser.parse_args()
    return args.action(args)


if __name__ == "__main__":
    raise SystemExit(main())
