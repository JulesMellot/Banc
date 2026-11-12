"""Links the clips of the open project to the proxies in their Proxy subfolder, by hand.

Prototype P2, fallback case: Resolve did not make the link by itself. For
each clip without a proxy, looks for <original's folder>/Proxy/<same name>.*
and calls MediaPoolItem.LinkProxyMedia. Without --apply, only lists.

    python3 link_proxies.py
    python3 link_proxies.py --apply --output ../p2/links.json
"""

import argparse
import ntpath
import os
import posixpath

from resolve_connection import connect, current_project, write_json


def all_clips(folder):
    clips = list(folder.GetClipList() or [])
    for subfolder in folder.GetSubFolderList() or []:
        clips.extend(all_clips(subfolder))
    return clips


def find_proxy(original_path):
    """Candidate proxy in the Proxy subfolder, whatever the path style."""
    module = ntpath if "\\" in original_path else posixpath
    folder, name = module.split(original_path)
    stem = module.splitext(name)[0]
    proxy_folder = module.join(folder, "Proxy")
    if not os.path.isdir(proxy_folder):
        return None
    for candidate in sorted(os.listdir(proxy_folder)):
        if os.path.splitext(candidate)[0] == stem:
            return module.join(proxy_folder, candidate)
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--apply", action="store_true", help="call LinkProxyMedia")
    parser.add_argument("--output", help="JSON file to write (otherwise standard output)")
    args = parser.parse_args()

    project = current_project(connect())
    results = []
    for clip in all_clips(project.GetMediaPool().GetRootFolder()):
        path = clip.GetClipProperty("File Path") or ""
        row = {
            "clip": clip.GetName(),
            "original": path,
            "proxy_before": clip.GetClipProperty("Proxy Media Path") or "",
        }
        row["candidate"] = find_proxy(path) if path else None
        if args.apply and row["candidate"] and not row["proxy_before"]:
            row["link_accepted"] = bool(clip.LinkProxyMedia(row["candidate"]))
            row["proxy_after"] = clip.GetClipProperty("Proxy Media Path") or ""
        results.append(row)
    write_json({"apply": args.apply, "clips": results}, args.output)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
