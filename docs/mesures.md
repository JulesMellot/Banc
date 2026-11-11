# Mesures au repos

Cibles de la section 10 du plan : service agent sous 15 Mo de RAM, assistant de session sous 10 Mo, CPU à 0 %.

`cargo xtask idle-bench` lance l'agent compilé en release, attend 2 secondes, puis lit `/proc` pendant 10 secondes. La mémoire est la mémoire résidente, `VmRSS`, pages partagées comprises. Le CPU admis est de 0,1 % du temps écoulé, soit un tic d'horloge sur 10 secondes. La commande échoue au-delà, et `pr.yml` la lance sur chaque PR.

| Phase | Date | Machine | Agent, mémoire | Agent, CPU | Assistant |
|---|---|---|---|---|---|
| 0 | 2026-11-11 | Linux x86_64, conteneur de développement | 2,8 Mo | 0 tic en 10 s | pas mesuré, il s'arrête au démarrage |

## Notes

**Phase 0.** L'agent ne fait encore rien : un runtime Tokio à un fil, les journaux, l'attente du signal d'arrêt. Les 2,8 Mo sont donc le plancher, et il reste 12 Mo pour iroh, le client HTTP et tout le reste.

La première mesure donnait 6,6 Mo. `tracing-appender` alloue d'avance sa file d'écriture, 128 000 lignes par défaut, ce qui coûte 4 Mo. La file est ramenée à 1 024 lignes dans `banc-platform/src/logging.rs`.

Mesures macOS et Windows : à faire à la main en phase 1, quand `idle-bench` saura lire la mémoire sur ces OS.
