# Migrations de la base du serveur

Une migration est un fichier `NNNN_nom_en_minuscules.sql`. Les numéros se suivent à partir de `0001`, sans trou. Une migration fusionnée dans `main` ne se modifie plus : on en ajoute une nouvelle.

`banc-server` embarque ces fichiers à la compilation et applique ceux qui manquent à l'ouverture de la base.

`schema.sql` garde le schéma obtenu une fois toutes les migrations appliquées sur une base vide. Après l'ajout d'une migration :

```bash
cargo xtask schema
```

Relis le diff de `schema.sql` avant de committer. `cargo xtask check migrations` échoue si le fichier ne correspond plus aux migrations.

Le modèle de données visé est décrit dans la section 3 de `docs/PLAN.md`. Les premières tables arrivent en phase 1.
