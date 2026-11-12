"""Relie à la main les clips du projet ouvert aux proxys de leur sous-dossier Proxy.

Prototype P2, cas de secours : Resolve n'a pas fait le lien tout seul. Pour
chaque clip sans proxy, cherche <dossier de l'original>/Proxy/<même nom>.* et
appelle MediaPoolItem.LinkProxyMedia. Sans --appliquer, ne fait que lister.

    python3 lier_proxys.py
    python3 lier_proxys.py --appliquer --sortie ../p2/liens.json
"""

import argparse
import ntpath
import os
import posixpath

from resolve_connexion import connecter, ecrire_json, projet_courant


def tous_les_clips(dossier):
    clips = list(dossier.GetClipList() or [])
    for sous_dossier in dossier.GetSubFolderList() or []:
        clips.extend(tous_les_clips(sous_dossier))
    return clips


def chercher_proxy(chemin_original):
    """Proxy candidat dans le sous-dossier Proxy, quel que soit le style du chemin."""
    module = ntpath if "\\" in chemin_original else posixpath
    dossier, nom = module.split(chemin_original)
    racine_nom = module.splitext(nom)[0]
    dossier_proxy = module.join(dossier, "Proxy")
    if not os.path.isdir(dossier_proxy):
        return None
    for candidat in sorted(os.listdir(dossier_proxy)):
        if os.path.splitext(candidat)[0] == racine_nom:
            return module.join(dossier_proxy, candidat)
    return None


def main():
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--appliquer", action="store_true", help="appeler LinkProxyMedia")
    parser.add_argument("--sortie", help="fichier JSON à écrire (sinon sortie standard)")
    args = parser.parse_args()

    projet = projet_courant(connecter())
    resultats = []
    for clip in tous_les_clips(projet.GetMediaPool().GetRootFolder()):
        chemin = clip.GetClipProperty("File Path") or ""
        ligne = {
            "clip": clip.GetName(),
            "original": chemin,
            "proxy_avant": clip.GetClipProperty("Proxy Media Path") or "",
        }
        ligne["candidat"] = chercher_proxy(chemin) if chemin else None
        if args.appliquer and ligne["candidat"] and not ligne["proxy_avant"]:
            ligne["lien_accepte"] = bool(clip.LinkProxyMedia(ligne["candidat"]))
            ligne["proxy_apres"] = clip.GetClipProperty("Proxy Media Path") or ""
        resultats.append(ligne)
    ecrire_json({"appliquer": args.appliquer, "clips": resultats}, args.sortie)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
