# Intégration continue

## Les workflows

| Fichier | Déclencheur | Contenu |
|---|---|---|
| `ci.yml` | chaque push | Les étapes de `cargo xtask check`, sur Linux, en cinq jobs |
| `pr.yml` | chaque PR et chaque mise à jour | `ci.yml`, clippy et tests sur macOS et Windows, compilation des cinq cibles, mesure au repos |
| `titre-pr.yml` | ouverture, mise à jour et changement de titre d'une PR | Convention du titre |
| `nightly.yml` | chaque nuit à 2 h 17 UTC | Trois OS, cinq cibles, failles du jour, issue ouverte en cas d'échec |
| `plateformes.yml` | appelé par `pr.yml` et `nightly.yml` | La matrice des OS et des cibles |

Les jobs appellent `cargo xtask check <étape>`. Un contrôle se modifie dans `crates/xtask`, pas dans le YAML.

Sur une PR, Linux est déjà couvert par `ci.yml`. La matrice de `pr.yml` ne relance donc que macOS et Windows, qui coûtent plus cher à la minute. La nuit, les trois OS tournent.

Le titre a son propre fichier parce qu'un titre corrigé relancerait sinon la matrice complète.

## Réglages GitHub à poser une fois

Ces réglages vivent dans GitHub, pas dans le dépôt. Aucun outil de cette phase ne peut les poser à ta place.

1. **Protection de `main`.** Dans Settings, Rules, Rulesets, choisis « Import a ruleset » et donne `.github/rulesets/main.json`. Le fichier interdit le push direct, le force push et la suppression. Il exige une PR à jour avec `main`, fusionnée en squash, avec les contrôles `PR prête` et `Titre de la PR` au vert.
2. **Fusion en squash.** Dans Settings, General, Pull Requests, ne garde que « Allow squash merging » et choisis « Pull request title » comme message par défaut.
3. **Dependabot.** Dans Settings, Advanced Security, active « Dependabot alerts » et « Dependabot version updates ».

Sur un dépôt privé, GitHub n'applique les rulesets qu'à partir de l'offre Pro ou Team. Avec un compte gratuit, la protection s'importe mais ne bloque rien. Vérifie en essayant un push direct sur `main` après l'import.

GitHub n'affiche un contrôle dans la liste des contrôles exigés qu'après sa première exécution. Ouvre une PR avant de vérifier que les deux noms du ruleset sont reconnus.

## Critères d'acceptation de la phase 0

| Critère | Où il se vérifie |
|---|---|
| Les trois binaires se compilent pour toutes les cibles | Job « Compilation » de `pr.yml`, cinq cibles |
| Une PR avec une erreur de format, un avertissement clippy, un test cassé ou un commit mal nommé est refusée | `ci.yml` échoue, donc `PR prête` échoue |
| On ne peut pas pousser directement sur `main` | Ruleset importé, à tester à la main |

## Ce qui reste à brancher

Le plan demande ces contrôles, mais ils n'ont rien à tester avant que le produit existe.

- **Tests de bout en bout** dans `pr.yml`, avec un serveur, un dépôt et deux agents dans des conteneurs. Le premier scénario, l'enrôlement, arrive en phase 1. Le job s'ajoute à la liste `needs` de `PR prête`.
- **Mesure au repos de l'assistant de session.** `idle-bench` ne mesure que l'agent, parce que l'assistant s'arrête tout de suite en phase 0. À étendre en phase 1.
- **Tests longs** dans `nightly.yml`, en phase 3.
- **`release.yml`**, en phase 7.

## Coût

Les minutes macOS et Windows sont facturées plus cher que les minutes Linux sur un dépôt privé. Vérifie les tarifs en vigueur. Si la facture monte, le premier levier est de retirer la cible `x86_64-apple-darwin` de `pr.yml` et de la garder la nuit.
