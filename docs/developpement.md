# Développer Banc

## Outils

| Outil | Version | Installation |
|---|---|---|
| Rust | celle de `rust-toolchain.toml` | rustup l'installe au premier `cargo` |
| cargo-nextest | 0.9.151 | `cargo install --locked cargo-nextest@0.9.151` |
| cargo-deny | 0.20.2 | `cargo install --locked cargo-deny@0.20.2` |
| Node.js | 22 ou plus | |
| pnpm | celle de `ui/package.json` | `corepack enable` |
| Python | 3.7 ou plus | |
| ruff | 0.16.9 | `pipx install ruff==0.16.9` |
| gitleaks | 8.30.1 | `go install github.com/zricethezav/gitleaks/v8@v8.30.1` |

Les versions de ce tableau sont celles de `.github/workflows/ci.yml`. Change les deux ensemble.

Pour compiler la cible Linux arm64 depuis une machine amd64, il faut aussi `gcc-aarch64-linux-gnu`.

## Commandes

| Commande | Effet |
|---|---|
| `cargo xtask check` | Tous les contrôles de la CI, dans l'ordre, arrêt au premier échec |
| `cargo xtask check clippy tests` | Seulement les étapes citées |
| `cargo xtask hooks` | Active le crochet `commit-msg` du dépôt |
| `cargo xtask schema` | Réécrit `migrations/schema.sql` après l'ajout d'une migration |
| `cargo xtask dev-server` | Serveur avec `dev/serveur.toml`, données dans `.dev/` |
| `cargo xtask dev-agent` | Agent au premier plan, journaux dans `.dev/journaux` |
| `cargo xtask dev-ui` | Frontend en mode développement |
| `cargo xtask dist` | Binaires release de la machine dans `dist/<cible>/` |
| `cargo xtask dist --target <cible>` | Même chose pour une autre cible, option répétable |
| `cargo xtask idle-bench` | Mémoire et CPU de l'agent au repos, sous Linux |
| `cargo xtask pr-title "<titre>"` | Contrôle un titre de PR |

## Les étapes de `check`

| Étape | Ce qu'elle lance |
|---|---|
| `fmt` | `cargo fmt --all --check` |
| `architecture` | Refuse tout code spécifique à un OS hors de `banc-platform` |
| `clippy` | `cargo clippy --workspace --all-targets --locked -- -D warnings` |
| `tests` | `cargo nextest run --workspace --locked`, puis les tests de documentation |
| `migrations` | Applique les migrations sur une base vide et compare le schéma à `migrations/schema.sql` |
| `frontend` | Dans `ui/` : `pnpm install --frozen-lockfile`, `lint`, `check`, `test`, `build` |
| `python` | `ruff check`, `ruff format --check`, compilation de chaque script de `resolve-scripts/` |
| `deny` | `cargo deny --locked check` |
| `secrets` | `gitleaks` sur les commits de la branche et sur les changements pas encore committés |
| `commits` | Convention des messages de commit de la branche |

`secrets` et `commits` comparent la branche à `origin/main`. L'option `--base <ref>` change ce point de départ.

## Messages de commit

La règle complète est dans la section 15 de `docs/PLAN.md`. Le contrôle vit dans `crates/xtask/src/commits.rs`, avec ses tests. Il vérifie :

- le format `type(portée): description` et les listes de types et de portées ;
- la portée, obligatoire pour `feat`, `fix`, `refactor`, `perf` et `test` ;
- la description, qui commence par un verbe à l'infinitif en minuscules et finit sans point ;
- la première ligne, 72 caractères au plus ;
- une ligne `Refs: phase N` dans le corps, avec le prototype s'il y en a un ;
- une ligne `BREAKING CHANGE:` quand le titre porte un `!`.

Le contrôle de l'infinitif regarde la terminaison du premier mot. Il arrête `update` ou `correction de`, il ne lit pas le français.

Les commits de Dependabot sont dispensés.

## Règles vérifiées par les outils

- `unwrap()` et `expect()` hors tests : refusés par clippy, réglage dans `Cargo.toml` et `clippy.toml`.
- `println!`, `eprintln!`, `dbg!`, `todo!` : refusés par clippy. Un binaire écrit dans ses journaux.
- `cfg` d'OS hors de `banc-platform` : refusé par l'étape `architecture`. `xtask` est un outil de développement, la règle ne le concerne pas.
- Licences et doublons de dépendances : `deny.toml`. Ajouter une licence à la liste se décide en revue.

## Ajouter une migration

Voir `migrations/README.md`.
