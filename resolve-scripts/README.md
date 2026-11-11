# Scripts Python du pont Resolve

Ces scripts sont embarqués dans `banc-session` et lancés par lui seul, quand il faut parler à DaVinci Resolve. Ils arrivent en phase 4. Les règles :

- Un script lit ses arguments sur la ligne de commande et écrit un seul objet JSON sur la sortie standard. Les erreurs vont sur la sortie d'erreur, avec un code de sortie non nul.
- Bibliothèque standard uniquement, plus le module `DaVinciResolveScript` fourni par Resolve.
- La syntaxe reste compatible avec la plus ancienne version de Python que Resolve accepte. Tant que le prototype P10 n'a pas de réponse dans `docs/prototypes.md`, c'est Python 3.7 (`ruff.toml`).

Contrôles (`cargo xtask check python`) : `ruff check`, `ruff format --check`, puis compilation de chaque script.

`diagnostic.py` affiche la version de Python qui l'exécute et dit si le module de script de Resolve se charge. Il sert au prototype P10 et au diagnostic d'un poste.
