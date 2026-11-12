"""Trouve le fichier où Resolve range ses préférences Media Storage, et teste l'écriture.

Prototype P1. Trois commandes :

    python3 preferences.py photo avant      # copie des fichiers de préférences
    python3 preferences.py ecarts avant apres   # ce qui a changé entre deux photos
    python3 preferences.py remettre apres <fichier>   # réécrit un fichier, Resolve fermé

Les photos vont dans --dossier (par défaut ../p1/photos). Seuls les fichiers de
moins de 2 Mo sont gardés, hors caches, LUT, Fusion et bases de projets.
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

from resolve_connexion import quitter, resolve_est_lance

TAILLE_MAX = 2 * 1024 * 1024

# Dossiers ignorés : volumineux, et sans rapport avec les préférences.
IGNORES = {
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


def racines():
    maison = os.path.expanduser("~")
    systeme = platform.system()
    if systeme == "Darwin":
        return [
            os.path.join(maison, "Library", "Preferences", "Blackmagic Design"),
            os.path.join(maison, "Library", "Application Support", "Blackmagic Design"),
            "/Library/Application Support/Blackmagic Design",
            "/Library/Preferences/Blackmagic Design",
        ]
    if systeme == "Windows":
        return [
            os.path.join(os.environ.get(variable, ""), "Blackmagic Design")
            for variable in ("APPDATA", "LOCALAPPDATA", "PROGRAMDATA")
            if os.environ.get(variable)
        ]
    return [
        os.path.join(maison, ".local", "share", "DaVinciResolve"),
        "/opt/resolve/configs",
    ]


def lister(racine):
    for dossier, sous_dossiers, fichiers in os.walk(racine):
        sous_dossiers[:] = [nom for nom in sous_dossiers if nom.lower() not in IGNORES]
        for nom in fichiers:
            yield os.path.join(dossier, nom)


def decrire(chemin):
    try:
        taille = os.path.getsize(chemin)
        if taille > TAILLE_MAX:
            return {"taille": taille, "trop_gros": True}
        with open(chemin, "rb") as fichier:
            contenu = fichier.read()
    except OSError as erreur:
        return {"erreur": str(erreur)}
    return {
        "taille": taille,
        "modifie": os.path.getmtime(chemin),
        "sha256": hashlib.sha256(contenu).hexdigest(),
        "contenu": base64.b64encode(contenu).decode("ascii"),
    }


def chemin_photo(dossier, nom):
    return os.path.join(dossier, nom + ".json")


def lire_photo(dossier, nom):
    chemin = chemin_photo(dossier, nom)
    if not os.path.exists(chemin):
        quitter(f"photo introuvable : {chemin}")
    with open(chemin, encoding="utf-8") as fichier:
        return json.load(fichier)


def photo(args):
    if resolve_est_lance():
        sys.stderr.write("attention : Resolve est lancé, il n'a peut-être pas encore écrit.\n")
    fichiers = {}
    presentes = [racine for racine in racines() if os.path.isdir(racine)]
    for racine in presentes:
        for chemin in lister(racine):
            fichiers[chemin] = decrire(chemin)
    os.makedirs(args.dossier, exist_ok=True)
    with open(chemin_photo(args.dossier, args.nom), "w", encoding="utf-8") as sortie:
        json.dump(
            {
                "date": datetime.datetime.now().isoformat(timespec="seconds"),
                "systeme": platform.system(),
                "racines": presentes,
                "fichiers": fichiers,
            },
            sortie,
            ensure_ascii=False,
            indent=1,
        )
    sys.stderr.write(f"{len(fichiers)} fichiers photographiés dans {args.nom}\n")
    return 0


def texte(description):
    """Contenu du fichier en texte, ou None s'il n'est pas lisible comme du texte."""
    if "contenu" not in description:
        return None
    try:
        return base64.b64decode(description["contenu"]).decode("utf-8").splitlines(True)
    except UnicodeDecodeError:
        return None


def ecarts(args):
    avant = lire_photo(args.dossier, args.avant)["fichiers"]
    apres = lire_photo(args.dossier, args.apres)["fichiers"]
    for chemin in sorted(set(avant) - set(apres)):
        sys.stdout.write(f"supprimé : {chemin}\n")
    for chemin in sorted(set(apres) - set(avant)):
        sys.stdout.write(f"ajouté : {chemin}\n")
    for chemin in sorted(set(avant) & set(apres)):
        ancien, nouveau = avant[chemin], apres[chemin]
        if ancien.get("sha256") == nouveau.get("sha256"):
            continue
        sys.stdout.write(f"modifié : {chemin}\n")
        lignes_avant, lignes_apres = texte(ancien), texte(nouveau)
        if lignes_avant is None or lignes_apres is None:
            taille_avant, taille_apres = ancien.get("taille"), nouveau.get("taille")
            sys.stdout.write(f"  binaire, {taille_avant} -> {taille_apres} octets\n")
            continue
        sys.stdout.writelines(
            difflib.unified_diff(lignes_avant, lignes_apres, args.avant, args.apres)
        )
    return 0


def remettre(args):
    if resolve_est_lance():
        quitter("Resolve est lancé. Le fermer avant d'écrire dans ses préférences.")
    description = lire_photo(args.dossier, args.nom)["fichiers"].get(args.fichier)
    if description is None or "contenu" not in description:
        quitter(f"ce fichier n'a pas de contenu dans la photo {args.nom} : {args.fichier}")
    if os.path.exists(args.fichier):
        horodatage = datetime.datetime.now().strftime("%Y%m%d-%H%M%S")
        sauvegarde = f"{args.fichier}.banc-{horodatage}"
        shutil.copy2(args.fichier, sauvegarde)
        sys.stderr.write(f"sauvegarde : {sauvegarde}\n")
    with open(args.fichier, "wb") as fichier:
        fichier.write(base64.b64decode(description["contenu"]))
    sys.stderr.write(f"réécrit depuis la photo {args.nom} : {args.fichier}\n")
    return 0


def main():
    ici = os.path.dirname(os.path.abspath(__file__))
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument(
        "--dossier",
        default=os.path.join(ici, "..", "p1", "photos"),
        help="dossier des photos",
    )
    commandes = parser.add_subparsers(dest="commande")
    commandes.required = True

    commande = commandes.add_parser("photo", help="photographier les préférences")
    commande.add_argument("nom")
    commande.set_defaults(action=photo)

    commande = commandes.add_parser("ecarts", help="comparer deux photos")
    commande.add_argument("avant")
    commande.add_argument("apres")
    commande.set_defaults(action=ecarts)

    commande = commandes.add_parser("remettre", help="réécrire un fichier depuis une photo")
    commande.add_argument("nom")
    commande.add_argument("fichier")
    commande.set_defaults(action=remettre)

    args = parser.parse_args()
    return args.action(args)


if __name__ == "__main__":
    raise SystemExit(main())
