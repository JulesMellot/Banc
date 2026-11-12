"""Makes test rushes and their proxies with ffmpeg.

Prototype P2, when no real rushes are at hand. Each clip has a different
timecode and hue. Proxies go into a Proxy subfolder, with the same name as
the original, as Resolve does.

    python3 test_media.py /Volumes/Banc/P2/Rushes/D01/CamA --count 3
    python3 test_media.py B:\\P2\\Rushes\\D01\\CamA --no-proxies
"""

import argparse
import os
import shutil
import subprocess
import sys

from resolve_connection import fail

FRAMES_PER_SECOND = 25


def encode(output, number, duration, size, profile):
    """Encodes a ProRes clip: tinted test pattern, 440 Hz tone, timecode specific to the clip."""
    hue = (number * 47) % 360
    timecode = f"{10 + number:02d}:00:00:00"
    command = [
        "ffmpeg",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        f"testsrc2=size={size}:rate={FRAMES_PER_SECOND}:duration={duration}",
        "-f",
        "lavfi",
        "-i",
        f"sine=frequency=440:duration={duration}",
        "-vf",
        f"hue=h={hue}",
        "-c:v",
        "prores_ks",
        "-profile:v",
        profile,
        "-c:a",
        "pcm_s16le",
        "-timecode",
        timecode,
        output,
    ]
    subprocess.run(command, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("folder", help="folder for the originals, created if missing")
    parser.add_argument("--count", type=int, default=3)
    parser.add_argument("--duration", type=int, default=10, help="in seconds")
    parser.add_argument("--prefix", default="A")
    parser.add_argument("--no-proxies", action="store_true", help="originals only")
    args = parser.parse_args()

    if shutil.which("ffmpeg") is None:
        fail("ffmpeg not found on the PATH")
    proxies = os.path.join(args.folder, "Proxy")
    os.makedirs(proxies if not args.no_proxies else args.folder, exist_ok=True)
    for number in range(1, args.count + 1):
        name = f"{args.prefix}{number:03d}.mov"
        # UHD original in ProRes 422 HQ, 960x540 proxy in ProRes Proxy.
        encode(os.path.join(args.folder, name), number, args.duration, "3840x2160", "3")
        if not args.no_proxies:
            encode(os.path.join(proxies, name), number, args.duration, "960x540", "0")
        sys.stdout.write(name + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
