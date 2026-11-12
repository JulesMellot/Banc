"""Décrit en JSON le projet ouvert dans Resolve : clips, chemins, proxys, timelines.

Sert aux prototypes P1 (chemins vus d'un autre OS) et P2 (proxys sans originaux).
Ne modifie rien.

    python3 inventaire.py --sortie ../p1/mac.json
"""

import argparse
import os
import platform

from resolve_connexion import connecter, ecrire_json, projet_courant

# Propriétés recopiées en tête de chaque clip, pour lire le JSON sans tout parcourir.
PROPRIETES_CLES = ("File Path", "Proxy", "Proxy Media Path", "Type", "Resolution", "Duration")


def decrire_clip(clip, dossier):
    proprietes = clip.GetClipProperty() or {}
    chemin = proprietes.get("File Path", "")
    proxy = proprietes.get("Proxy Media Path", "")
    return {
        "dossier": dossier,
        "nom": clip.GetName(),
        "id": clip.GetMediaId(),
        "cles": {cle: proprietes.get(cle) for cle in PROPRIETES_CLES},
        "fichier_present_ici": bool(chemin) and os.path.exists(chemin),
        "proxy_present_ici": bool(proxy) and os.path.exists(proxy),
        "proprietes": proprietes,
    }


def parcourir(dossier, chemin):
    clips = [decrire_clip(clip, chemin) for clip in dossier.GetClipList() or []]
    for sous_dossier in dossier.GetSubFolderList() or []:
        clips.extend(parcourir(sous_dossier, chemin + "/" + sous_dossier.GetName()))
    return clips


def decrire_timeline(timeline):
    pistes = []
    for index in range(1, int(timeline.GetTrackCount("video")) + 1):
        elements = []
        for element in timeline.GetItemListInTrack("video", index) or []:
            source = element.GetMediaPoolItem()
            elements.append(
                {
                    "nom": element.GetName(),
                    "debut": element.GetStart(),
                    "fin": element.GetEnd(),
                    "clip": source.GetMediaId() if source else None,
                }
            )
        pistes.append({"piste": f"V{index}", "elements": elements})
    return {"nom": timeline.GetName(), "pistes_video": pistes}


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--sortie", help="fichier JSON à écrire (sinon sortie standard)")
    args = parser.parse_args()

    resolve = connecter()
    projet = projet_courant(resolve)
    racine = projet.GetMediaPool().GetRootFolder()
    timelines = [
        projet.GetTimelineByIndex(index) for index in range(1, int(projet.GetTimelineCount()) + 1)
    ]
    stockage = resolve.GetMediaStorage()
    ecrire_json(
        {
            "poste": {"systeme": platform.system(), "version": platform.release()},
            "resolve": {
                "produit": resolve.GetProductName(),
                "version": resolve.GetVersionString(),
            },
            "base": resolve.GetProjectManager().GetCurrentDatabase(),
            "projet": projet.GetName(),
            "volumes_media_storage": stockage.GetMountedVolumeList() if stockage else None,
            "clips": parcourir(racine, racine.GetName()),
            "timelines": [decrire_timeline(timeline) for timeline in timelines if timeline],
        },
        args.sortie,
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
