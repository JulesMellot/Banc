# Banc

Banc prépare les postes DaVinci Resolve d'une équipe de montage. Un serveur sur un VPS décrit ce que chaque poste doit avoir. Un agent installé sur chaque machine vérifie le poste, installe ce qui manque, récupère les fichiers des projets et ouvre Resolve dessus. Outil interne.

Le dépôt en est à la phase 0 du plan. Les trois binaires démarrent, ouvrent leurs journaux et attendent. Le produit commence en phase 1.

## Où lire

- `docs/PLAN.md` : le cahier des charges, phase par phase.
- `CLAUDE.md` : les principes, la stack et les règles de code.
- `docs/developpement.md` : installer les outils et lancer les contrôles.
- `docs/ci.md` : les workflows et les réglages GitHub à poser à la main.
- `docs/prototypes.md` : les réponses des prototypes Resolve.
- `docs/mesures.md` : la mémoire et le CPU au repos, phase par phase.

## Démarrer

```bash
cargo xtask hooks
```

```bash
cargo xtask check
```

La première commande active le contrôle des messages de commit. La seconde lance tout ce que lance la CI.
