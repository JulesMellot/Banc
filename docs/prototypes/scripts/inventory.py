"""Describes the project open in Resolve as JSON: clips, paths, proxies, timelines.

Used by prototypes P1 (paths seen from another OS) and P2 (proxies without
originals). Changes nothing.

    python3 inventory.py --output ../p1/mac.json
"""

import argparse
import os
import platform

from resolve_connection import connect, current_project, write_json

# Properties copied at the top of each clip, to read the JSON without going through everything.
KEY_PROPERTIES = ("File Path", "Proxy", "Proxy Media Path", "Type", "Resolution", "Duration")


def describe_clip(clip, folder):
    properties = clip.GetClipProperty() or {}
    path = properties.get("File Path", "")
    proxy = properties.get("Proxy Media Path", "")
    return {
        "folder": folder,
        "name": clip.GetName(),
        "id": clip.GetMediaId(),
        "keys": {key: properties.get(key) for key in KEY_PROPERTIES},
        "file_present_here": bool(path) and os.path.exists(path),
        "proxy_present_here": bool(proxy) and os.path.exists(proxy),
        "properties": properties,
    }


def walk(folder, path):
    clips = [describe_clip(clip, path) for clip in folder.GetClipList() or []]
    for subfolder in folder.GetSubFolderList() or []:
        clips.extend(walk(subfolder, path + "/" + subfolder.GetName()))
    return clips


def describe_timeline(timeline):
    tracks = []
    for index in range(1, int(timeline.GetTrackCount("video")) + 1):
        items = []
        for item in timeline.GetItemListInTrack("video", index) or []:
            source = item.GetMediaPoolItem()
            items.append(
                {
                    "name": item.GetName(),
                    "start": item.GetStart(),
                    "end": item.GetEnd(),
                    "clip": source.GetMediaId() if source else None,
                }
            )
        tracks.append({"track": f"V{index}", "items": items})
    return {"name": timeline.GetName(), "video_tracks": tracks}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--output", help="JSON file to write (otherwise standard output)")
    args = parser.parse_args()

    resolve = connect()
    project = current_project(resolve)
    root = project.GetMediaPool().GetRootFolder()
    timelines = [
        project.GetTimelineByIndex(index) for index in range(1, int(project.GetTimelineCount()) + 1)
    ]
    storage = resolve.GetMediaStorage()
    write_json(
        {
            "workstation": {"system": platform.system(), "version": platform.release()},
            "resolve": {
                "product": resolve.GetProductName(),
                "version": resolve.GetVersionString(),
            },
            "database": resolve.GetProjectManager().GetCurrentDatabase(),
            "project": project.GetName(),
            "media_storage_volumes": storage.GetMountedVolumeList() if storage else None,
            "clips": walk(root, root.GetName()),
            "timelines": [describe_timeline(timeline) for timeline in timelines if timeline],
        },
        args.output,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
