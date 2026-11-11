"""Affiche en JSON la version de Python et l'état du module de script de Resolve.

Sert au prototype P10 et au diagnostic d'un poste. Ne modifie rien.
"""

import json
import platform
import sys


def resolve_module_error():
    """Renvoie None si le module de script de Resolve se charge, sinon l'erreur."""
    try:
        import DaVinciResolveScript  # noqa: F401, PLC0415
    except Exception as error:
        return f"{type(error).__name__}: {error}"
    return None


def main():
    report = {
        "python": platform.python_version(),
        "executable": sys.executable,
        "system": platform.system(),
        "machine": platform.machine(),
        "resolve_module_error": resolve_module_error(),
    }
    json.dump(report, sys.stdout, ensure_ascii=False)
    sys.stdout.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
