"""Fabrique des rushes de test et leurs proxys avec ffmpeg.

Prototype P2, quand on n'a pas de vrais rushes sous la main. Chaque clip a un
timecode et une teinte différents. Les proxys vont dans un sous-dossier Proxy,
avec le même nom que l'original, comme le fait Resolve.

    python3 medias_test.py /Volumes/Banc/P2/Rushes/J01/CamA --nombre 3
    python3 medias_test.py B:\\P2\\Rushes\\J01\\CamA --sans-proxys
"""

import argparse
import os
import shutil
import subprocess
import sys

from resolve_connexion import quitter

IMAGES_PAR_SECONDE = 25


def encoder(sortie, numero, duree, taille, profil):
    """Encode un clip ProRes : mire teintée, son à 440 Hz, timecode propre au clip."""
    teinte = (numero * 47) % 360
    timecode = f"{10 + numero:02d}:00:00:00"
    commande = [
        "ffmpeg",
        "-hide_banner",
        "-loglevel",
        "error",
        "-y",
        "-f",
        "lavfi",
        "-i",
        f"testsrc2=size={taille}:rate={IMAGES_PAR_SECONDE}:duration={duree}",
        "-f",
        "lavfi",
        "-i",
        f"sine=frequency=440:duration={duree}",
        "-vf",
        f"hue=h={teinte}",
        "-c:v",
        "prores_ks",
        "-profile:v",
        profil,
        "-c:a",
        "pcm_s16le",
        "-timecode",
        timecode,
        sortie,
    ]
    subprocess.run(commande, check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("dossier", help="dossier des originaux, créé s'il manque")
    parser.add_argument("--nombre", type=int, default=3)
    parser.add_argument("--duree", type=int, default=10, help="en secondes")
    parser.add_argument("--prefixe", default="A")
    parser.add_argument("--sans-proxys", action="store_true", help="originaux seuls")
    args = parser.parse_args()

    if shutil.which("ffmpeg") is None:
        quitter("ffmpeg introuvable dans le PATH")
    proxys = os.path.join(args.dossier, "Proxy")
    os.makedirs(proxys if not args.sans_proxys else args.dossier, exist_ok=True)
    for numero in range(1, args.nombre + 1):
        nom = f"{args.prefixe}{numero:03d}.mov"
        # Original UHD en ProRes 422 HQ, proxy 960x540 en ProRes Proxy.
        encoder(os.path.join(args.dossier, nom), numero, args.duree, "3840x2160", "3")
        if not args.sans_proxys:
            encoder(os.path.join(proxys, nom), numero, args.duree, "960x540", "0")
        sys.stdout.write(nom + "\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
