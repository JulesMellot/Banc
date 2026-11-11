# Banc

Banc (nom de travail) prépare les postes DaVinci Resolve d'une équipe de montage. Un serveur central sur un VPS décrit ce que chaque poste doit avoir. Un agent installé sur chaque machine (Mac, Windows, Linux) vérifie le poste, installe ce qui manque, récupère les proxys à la demande, ouvre les projets (en réservation ou en collaboration Resolve) et envoie les rendus vers une machine dédiée. Un NAS sert de dépôt permanent des fichiers.

Le plan complet, phase par phase, est dans `docs/PLAN.md`. Lis-le avant toute tâche. Travaille une phase à la fois et ne passe à la suivante que lorsque ses critères d'acceptation passent.

## Principes non négociables

- L'utilisateur installe une application, colle une URL et une clé (ou clique sur un lien d'invitation), et c'est fini. Toute fonctionnalité qui demande une manipulation manuelle de plus doit être signalée avant d'être codée.
- Pas de VPN. Les machines s'identifient par leur clé publique iroh et se connectent en sortie. Aucun port entrant sur les postes ni sur le NAS. Le VPS n'expose que le 443 (API + relais iroh).
- Pair-à-pair d'abord. Un fichier se prend chez une machine joignable en direct (poste ou dépôt) avant le cache ou le relais du VPS. Le VPS coordonne, il ne transporte les médias qu'en secours.
- Consommation minimale, mesurée et tenue :
  - service agent au repos : moins de 15 Mo de RAM, CPU à 0 % hors action en cours ;
  - assistant de session (icône de la barre système) : moins de 10 Mo ;
  - fenêtre d'interface : lancée au clic, processus terminé à la fermeture ;
  - serveur : doit tourner sur un VPS à 2 Go de RAM avec PostgreSQL à côté.
- Aucune activité quand Resolve est fermé, hors action demandée et transferts en cours. Pendant que Resolve est ouvert sur un projet Banc, une surveillance légère est admise : consolidation, médias modifiés, file de rendu, génération de proxys. Le tableau de la section 10 du plan fait foi.
- Tout ce qui entre dans un projet est publié, quelle que soit la façon dont c'est entré. Banc rattrape derrière l'utilisateur au lieu de lui imposer une marche à suivre.
- Le VPS ne peut rien faire exécuter aux postes. Le manifeste est signé par la clé admin, gardée sur le poste de l'admin. L'agent refuse tout manifeste, paquet ou mise à jour non signé. Ne jamais ajouter de chemin par lequel le serveur déclenche une commande sur un poste sans signature.
- Banc ne supprime jamais un fichier publié sans action de l'admin, et ne libère jamais un fichier local que le dépôt n'a pas confirmé.
- Pas de disque virtuel, pas d'extension noyau, pas de FUSE. Le volume Banc est un vrai volume local (APFS sur Mac, lettre de lecteur sur Windows, point de montage sur Linux).
- Outil interne : machines de la société, monteurs salariés, Resolve Studio partout. Pas de mode invité.

## Stack

- Rust stable, édition 2021, workspace Cargo.
- Runtime : Tokio. L'agent utilise un runtime `current_thread`.
- Réseau pair-à-pair : `iroh` 1.x (connexion par clé publique, traversée de NAT, relais). Relais auto-hébergé sur le VPS avec `iroh-relay`.
- Transfert de fichiers : `iroh-blobs` si sa version est stable au démarrage, sinon un protocole maison sur flux iroh avec BLAKE3 et vérification au fil de l'eau (`bao-tree`). Décision à noter ici en phase 3.
- Serveur HTTP : `axum`. Base : SQLite via `rusqlite` (feature `bundled`) et `rusqlite_migration`.
- Interface : Tauri 2, frontend Svelte + TypeScript. Icône de barre système native avec `tray-icon` + `tao`, sans vue web.
- Services système : crate `service-manager` (launchd, service Windows, systemd).
- Secrets : crate `keyring`. Signatures : `ed25519-dalek`. Mise à jour : `self-replace`.
- Logs : `tracing` + `tracing-appender` avec rotation.
- Pont Resolve : scripts Python embarqués, lancés par l'assistant de session uniquement quand il faut parler à Resolve.

## Workspace

```
crates/
  banc-proto/       messages et types partagés (serde)
  banc-platform/    tout le code spécifique OS (chemins, services, élévation, polices)
  banc-blobs/       empreintes, magasin local, transfert, choix des sources
  banc-resolve/     détection de version, appel des scripts Python, faux pour les tests
  banc-tunnel/      tunnel TCP sur iroh (PostgreSQL)
  banc-agent/       binaire : service système (installations, transferts, tunnel)
  banc-session/     binaire : icône barre système + pont Resolve, dans la session utilisateur
  banc-server/      binaire : API, admin, relais iroh, cache, file de rendu
ui/                 application Tauri (fenêtre utilisateur) et admin web (même frontend)
resolve-scripts/    scripts Python embarqués
migrations/         SQL du serveur
packaging/          pkg, msi, deb/rpm, image Docker (serveur et dépôt NAS)
docs/               PLAN.md et notes techniques
```

## Commandes

À compléter en phase 0. Prévoir au minimum :

- `cargo xtask dev-server`, `cargo xtask dev-agent`, `cargo xtask dev-ui`
- `cargo test --workspace`
- `cargo xtask dist` (binaires et installeurs des cibles disponibles sur la machine)
- `cargo xtask check` (les mêmes contrôles que la CI à chaque push : format, clippy, tests, migrations, frontend, scripts Python, `cargo deny`)

## Commits et CI

Le détail est dans la section 15 de `docs/PLAN.md`. À respecter à chaque fois :

- Lance `cargo xtask check` avant chaque commit. Ne crée pas de commit s'il échoue, et ne contourne jamais un contrôle (`--no-verify`, `#[allow]` ajouté pour faire taire clippy, test désactivé).
- Messages au format `type(portée): description`, description en français à l'infinitif, 72 caractères au plus. Types : `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`. Portées : `server`, `agent`, `session`, `blobs`, `tunnel`, `resolve`, `platform`, `proto`, `ui`, `scripts`, `packaging`, `ci`, `docs`.
- Un commit fait une seule chose. Le pied de message cite la phase et le prototype (`Refs: phase 3, P5`).
- Jamais de push direct sur `main`. Une branche `type/sujet-court` par tâche, puis une PR dont le titre suit la même convention.
- Une PR n'est prête que lorsque tous les contrôles sont verts. Si un contrôle échoue, corrige la cause.
- Tout nouveau comportement arrive avec ses tests dans la même PR.

## Règles de code

- Tout ce qui dépend de l'OS vit dans `banc-platform`, avec `#[cfg(target_os = ...)]` regroupés par module. Aucun `cfg` d'OS ailleurs.
- Pas de `unwrap()` ni `expect()` hors tests et démarrage. Erreurs avec `thiserror` dans les bibliothèques, `anyhow` dans les binaires.
- Toute action qui modifie le poste écrit une ligne dans le journal local et remonte au serveur.
- Un fichier téléchargé est vérifié par son empreinte BLAKE3 avant d'être déplacé à sa place ou exécuté. Un fichier qui ne correspond pas est supprimé.
- Les secrets vont dans le trousseau de l'OS, jamais dans un fichier en clair.
- Les messages visibles sont en français, dans un fichier de traduction.
- Chaque fonction qui compare un état local à un manifeste ou à une liste de fichiers a des tests. C'est le cœur du produit.
- Toute écriture dans un fichier privé de Resolve (préférences, liste des bibliothèques) passe par `banc-resolve`, se fait Resolve fermé, garde une copie de sauvegarde, et vérifie d'abord que le format est celui attendu. Si le format a changé, ne rien écrire et passer en mode guidé.
- Avant de coder une fonction qui dépend d'un comportement de Resolve, vérifie que le prototype correspondant de la section 12 du plan a une réponse dans `docs/prototypes.md`.
- Mesure la mémoire et le CPU au repos à la fin de chaque phase (`cargo xtask idle-bench`, à créer en phase 1) et note le résultat dans `docs/mesures.md`.

## Ce qui ne se teste pas en CI

Resolve n'existe pas en CI. `banc-resolve` fournit un faux utilisé par les tests. Les tests réels se font à la main sur une machine avec Resolve Studio, avec la checklist de chaque phase dans `docs/PLAN.md`.
