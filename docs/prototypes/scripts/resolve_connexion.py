"""Connexion à DaVinci Resolve pour les scripts des prototypes.

Règle les variables d'environnement décrites dans le README.txt de
Developer/Scripting si elles manquent, puis charge le module de Resolve.
"""

import json
import os
import platform
import subprocess
import sys


def chemins_par_defaut():
    """Renvoie (dossier de l'API, bibliothèque fusionscript) pour cet OS."""
    systeme = platform.system()
    if systeme == "Darwin":
        api = "/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting"
        lib = (
            "/Applications/DaVinci Resolve/DaVinci Resolve.app"
            "/Contents/Libraries/Fusion/fusionscript.so"
        )
    elif systeme == "Windows":
        programdata = os.environ.get("PROGRAMDATA", "C:\\ProgramData")
        api = os.path.join(
            programdata, "Blackmagic Design", "DaVinci Resolve", "Support", "Developer", "Scripting"
        )
        lib = "C:\\Program Files\\Blackmagic Design\\DaVinci Resolve\\fusionscript.dll"
    else:
        api = "/opt/resolve/Developer/Scripting"
        lib = "/opt/resolve/libs/Fusion/fusionscript.so"
    return api, lib


def connecter():
    """Renvoie l'objet Resolve, ou quitte avec un message clair."""
    api, lib = chemins_par_defaut()
    os.environ.setdefault("RESOLVE_SCRIPT_API", api)
    os.environ.setdefault("RESOLVE_SCRIPT_LIB", lib)
    modules = os.path.join(os.environ["RESOLVE_SCRIPT_API"], "Modules")
    if modules not in sys.path:
        sys.path.append(modules)
    try:
        import DaVinciResolveScript  # noqa: PLC0415
    except ImportError as erreur:
        quitter(f"module de script de Resolve introuvable dans {modules} : {erreur}")
    resolve = DaVinciResolveScript.scriptapp("Resolve")
    if resolve is None:
        quitter(
            "Resolve ne répond pas. Il doit être lancé, et le réglage "
            "Préférences > Système > Général > External scripting using doit être sur Local."
        )
    return resolve


def projet_courant(resolve):
    """Renvoie le projet ouvert dans Resolve, ou quitte."""
    projet = resolve.GetProjectManager().GetCurrentProject()
    if projet is None:
        quitter("aucun projet ouvert dans Resolve")
    return projet


def resolve_est_lance():
    """Vrai si un processus Resolve tourne sur ce poste."""
    if platform.system() == "Windows":
        sortie = subprocess.run(
            ["tasklist", "/FI", "IMAGENAME eq Resolve.exe", "/NH"],
            capture_output=True,
            text=True,
            check=False,
        ).stdout
        return "resolve.exe" in sortie.lower()
    nom = "Resolve" if platform.system() == "Darwin" else "resolve"
    return subprocess.run(["pgrep", "-x", nom], capture_output=True, check=False).returncode == 0


def ecrire_json(objet, chemin=None):
    """Écrit l'objet en JSON lisible, dans le fichier donné ou sur la sortie standard."""
    texte = json.dumps(objet, ensure_ascii=False, indent=2, sort_keys=True) + "\n"
    if chemin is None:
        sys.stdout.write(texte)
        return
    with open(chemin, "w", encoding="utf-8") as fichier:
        fichier.write(texte)
    sys.stderr.write(f"écrit : {chemin}\n")


def quitter(message):
    """Écrit l'erreur sur la sortie d'erreur et quitte avec le code 1."""
    sys.stderr.write(f"erreur : {message}\n")
    sys.exit(1)
