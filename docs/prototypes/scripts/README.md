# Scripts des prototypes

Scripts jetables, lancés à la main sur un poste qui a Resolve Studio. Ils ne sont pas embarqués dans Banc. Ils suivent les mêmes règles que `resolve-scripts/` : bibliothèque standard seule, syntaxe Python 3.7, JSON sur la sortie standard, erreurs sur la sortie d'erreur.

| Script | Prototype | Effet |
|---|---|---|
| `inventaire.py` | P1, P2 | Décrit le projet ouvert : clips, chemins, proxys, timelines. Ne modifie rien. |
| `preferences.py` | P1 | Photographie les fichiers de préférences de Resolve, compare deux photos, réécrit un fichier Resolve fermé. |
| `medias_test.py` | P2 | Fabrique des rushes ProRes et leurs proxys avec ffmpeg. |
| `lier_proxys.py` | P2 | Relie les clips aux proxys de leur sous-dossier `Proxy` par `LinkProxyMedia`. |
| `resolve_connexion.py` | tous | Module commun : connexion à Resolve, écriture du JSON. |

## Préparer le poste

1. Python 3 en 64 bits. Sur Mac, celui des outils de ligne de commande Apple suffit (`python3`). Sur Windows, celui de python.org, case « Add to PATH » cochée.
2. Dans Resolve : Préférences > Système > Général > « External scripting using » sur **Local**. Redémarrer Resolve.
3. Vérifier la connexion, Resolve ouvert sur un projet :

   ```
   python3 inventaire.py
   ```

   Sur Windows, remplacer `python3` par `python`. Le script règle seul `RESOLVE_SCRIPT_API`, `RESOLVE_SCRIPT_LIB` et le chemin des modules, avec les emplacements par défaut de Resolve. Si Resolve est installé ailleurs, définir ces deux variables avant de lancer le script, comme l'explique le `README.txt` de `Developer/Scripting`.

Noter dans la réponse de P10 la version de Python qui a fonctionné (`python3 --version`).

## Contrôle

Les scripts ne sont pas dans `cargo xtask check`. Avant de les modifier :

```
ruff check docs/prototypes/scripts
ruff format --check docs/prototypes/scripts
```
