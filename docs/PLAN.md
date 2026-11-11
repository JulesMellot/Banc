# Plan de construction de Banc

Ce document sert de cahier des charges à Claude Code. Chaque phase livre quelque chose d'utilisable et se termine par des critères d'acceptation. On ne commence pas une phase tant que la précédente ne passe pas.

Contexte : Banc est un outil interne, pour les monteurs et étalonneurs salariés d'une société, sur des machines de la société. Toutes les machines ont DaVinci Resolve Studio.

## 1. Le produit en une page

### Ce que vit l'utilisateur

1. L'admin crée une invitation. Il obtient un lien `banc://join?u=https://banc.masociete.fr&k=XXXX&a=EMPREINTE` et une clé à copier.
2. Le monteur installe Banc et clique sur le lien (ou colle l'URL et la clé). Banc demande une fois le mot de passe administrateur du poste, et sur Mac les autorisations d'accès aux fichiers.
3. Banc vérifie le poste : version de Resolve, plugins, polices, LUT, DCTL, templates Fusion, presets, volume Banc, bibliothèque de projets partagée. Il répare seul ce qu'il peut.
4. Le monteur voit la liste des projets. Il en ouvre un. Banc récupère ce qu'il faut, puis ouvre Resolve dessus.
5. Ensuite il travaille dans Resolve comme d'habitude. Tout ce qu'il ajoute au projet (rushes, musique glissée dans la timeline, graphismes, rendus) devient accessible aux autres sans qu'il y pense.

### Principe central

Un projet Banc est fait pour être partagé. Tout ce qui entre dans un projet est publié, quelle que soit la façon dont c'est entré. Banc s'adapte à la façon dont les gens travaillent dans Resolve et rattrape derrière eux.

### Les rôles

- **Poste** : monteur ou étalonneur.
- **Dépôt** : le NAS. Garde une copie de tout ce qui est publié. Docker ou binaire Linux, sans interface.
- **Rendu** (optionnel) : machine dédiée aux rendus. Le produit fonctionne sans.
- **Serveur** : sur le VPS. API, admin web, relais iroh, cache de fichiers, coordination, PostgreSQL de la collaboration.

Un utilisateur a aussi un rôle de personne : admin, monteur, étalonneur. Il règle ce qu'il peut faire et ce que son poste télécharge.

### Ce que Banc ne fait pas (volontairement)

- Aucune activité quand Resolve est fermé, hors action demandée par l'utilisateur et transferts en cours.
- Pas de disque virtuel avec pilote. Un vrai volume local au même chemin partout.
- Pas d'activation de licences de plugins. Banc installe, vérifie la présence, et signale.
- Pas de suppression automatique de fichiers publiés.

## 2. Architecture

```
                    VPS (seul point exposé, port 443)
          ┌──────────────────────────────────────────────┐
          │ banc-server                                  │
          │   API HTTPS, admin web, coordination         │
          │   relais iroh                                │
          │   cache de fichiers (quota + expiration)     │
          │   arrivée des tunnels PostgreSQL             │
          │ PostgreSQL 18 (écoute 127.0.0.1 uniquement)  │
          │ sauvegardes SQLite + PostgreSQL -> dépôt     │
          └──────────────────────┬───────────────────────┘
                                 │ connexions sortantes, iroh (QUIC)
      ┌──────────────┬───────────┼──────────────┬─────────────────┐
┌─────┴─────┐ ┌──────┴────┐ ┌────┴──────┐ ┌─────┴───────┐  ┌──────┴──────┐
│ poste Mac │ │ poste Win │ │ poste Lin │ │ rendu       │  │ dépôt NAS   │
│           │ │           │ │           │ │ (optionnel) │  │             │
└───────────┘ └───────────┘ └───────────┘ └─────────────┘  └─────────────┘
   Les machines se parlent en direct dès que possible (même LAN ou
   traversée de NAT réussie), sinon via le relais du VPS.
```

### Transport

iroh 1.x relie deux machines identifiées par leur clé publique, en direct ou par relais, sans VPN. Les fichiers sont identifiés par leur empreinte BLAKE3 et vérifiés pendant le transfert.

### Exposition du VPS

- Un nom de domaine, le port 443, certificat Let's Encrypt obtenu par le serveur.
- L'API HTTPS et le relais iroh partagent le port 443.
- PostgreSQL n'écoute que sur `127.0.0.1`.

Si le VPS est en panne, la coordination et la collaboration s'arrêtent. C'est accepté. Le mode réservation continue de fonctionner sur les fichiers déjà présents, et les publications partent au retour du serveur.

### Réseau local

Les agents d'un même LAN se trouvent par la découverte locale d'iroh et s'échangent les fichiers sans passer par Internet.

### Identité et sécurité

- Chaque machine a une clé Ed25519, qui est son identifiant iroh. La clé privée reste dans le trousseau de l'OS.
- Invitation : jeton de 32 octets, usage unique, 7 jours, porte le rôle.
- Une machine n'accepte de connexion iroh que des clés publiques déclarées pour l'équipe. Révoquer une machine la retire de la liste et coupe ses tunnels.
- Comptes admin : argon2id, TOTP optionnel.
- Le mot de passe PostgreSQL est généré à l'installation. Le couple par défaut de Resolve (`postgres` / `DaVinci`) n'est jamais utilisé.

### Le VPS ne peut rien faire exécuter aux postes

L'agent tourne avec les droits admin et installe des logiciels. Un VPS piraté ne doit pas pouvoir en profiter.

- **Clé admin.** À la première configuration, l'application Banc de l'admin génère une paire Ed25519. La clé privée reste dans le trousseau de son poste. Banc lui fait noter une phrase de récupération sur papier. La clé privée n'est jamais envoyée au VPS.
- **Manifeste signé.** Le manifeste (version de Resolve, paquets avec leur empreinte et leur commande d'installation, réglages imposés) est signé par la clé admin. La publication d'un manifeste se fait depuis l'application Banc de l'admin, pas depuis le navigateur.
- **Empreinte dans l'invitation.** Le lien d'invitation contient l'empreinte de la clé publique admin (`a=`). Le poste l'enregistre à l'enrôlement. Il refuse ensuite tout manifeste dont la signature ne correspond pas, et tout paquet dont l'empreinte n'est pas dans un manifeste signé.
- **Mises à jour de Banc.** Signées par une clé de publication gardée hors du VPS (poste du développeur ou secret de CI). L'agent refuse un binaire non signé.
- Ce que le VPS contrôle sans signature : les listes de fichiers des projets et les droits d'accès. Ce sont des données, jamais du code exécuté.
- Changer de clé admin demande une action sur chaque poste (nouvelle invitation). C'est voulu.

## 3. Modèle de données

SQLite sur le VPS :

- `users` : nom, rôle de personne, droits.
- `machines` : nom, OS, arch, rôle, clé publique, dernière connexion, version de l'agent, version de Resolve détectée, dernier rapport, latence vers le VPS.
- `invites`
- `manifests` : contenu, signature, version. Le dernier publié s'applique.
- `packages`, `package_builds` : éléments installables, une variante par OS et arch.
- `blobs` : empreinte BLAKE3, taille.
- `blob_locations` : qui possède quel blob (machine, dépôt, cache), date de dernière vérification.
- `projects` : nom, mode (`reservation` ou `collaboratif`), modèle, membres, état (`actif`, `archive`), verrou (mode réservation).
- `project_files` : chemin relatif, classe (`original`, `proxy`, `media`, `export`), empreinte courante, numéro de révision, auteur, date, état (`present`, `retire`).
- `project_file_revisions` : historique des empreintes d'un même chemin.
- `project_versions` : `.drp` publiés (mode réservation).
- `render_jobs` : projet, timeline, preset, machine choisie, heure prévue, statut, progression, fichier produit.
- `events` : journal d'audit.

### Manifeste

```json
{
  "version": 12,
  "resolve": { "edition": "studio", "version": "21.1" },
  "packages": [
    { "id": "boris-continuum", "kind": "ofx", "version": "2025.0", "required": true },
    { "id": "font-inter", "kind": "font", "version": "4.0", "required": true }
  ],
  "volume": { "unix": "/Volumes/Banc", "windows": "B:" },
  "collaboration": { "postgres_version": 18, "local_address": "127.0.0.2" },
  "limits": { "consolidation_ask_above_gb": 5, "free_proxies_after_days": 30 },
  "signature": "..."
}
```

### Règles de détection d'un paquet

`file_exists`, `bundle_version` (Mac), `registry` (Windows), `ofx_bundle`, `font`, `command` (dernier recours).

## 4. Le volume Banc et l'arborescence

### Un chemin identique partout

Resolve enregistre des chemins absolus. Banc crée un volume dédié, visible comme un disque, sans pilote ni extension noyau :

| OS | Chemin | Comment Banc le crée |
|---|---|---|
| macOS | `/Volumes/Banc` | Volume APFS ajouté au conteneur du disque choisi (`diskutil apfs addVolume`). Partage l'espace du disque, se monte seul au démarrage. |
| Linux | `/Volumes/Banc` | Point de montage ou lien vers le dossier de stockage choisi. |
| Windows | `B:\` | Lettre de lecteur persistante pour tout le système (`HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\DOS Devices`). Lettre réglable dans le manifeste. |

Mac et Linux partagent le même chemin. Il reste une seule correspondance, `/Volumes/Banc` avec `B:\`, parce que Resolve n'accepte qu'un Mapped Mount par emplacement. Banc l'écrit dans les préférences Media Storage de Resolve (Resolve fermé, copie de sauvegarde avant écriture).

### Arborescence, créée et entretenue par Banc

Personne ne crée de dossier à la main.

```
/Volumes/Banc/<projet>/          (B:\<projet>\ sur Windows)
  Rushes/J03/CamA/A001.braw      classe original
  Rushes/J03/CamA/Proxy/A001.mov classe proxy
  Audio/                         classe media
  Graphics/                      classe media
  Media/                         classe media (vidéos ajoutées hors ingest)
  Exports/                       classe export
  .banc/                         métadonnées locales, jamais publié
```

### Réglages imposés dans chaque projet Resolve

Appliqués à la création par script, vérifiés à chaque ouverture, remis en place si quelqu'un les change :

- proxys générés dans des sous-dossiers `Proxy` à côté des originaux ;
- destination de rendu par défaut dans `Exports/` ;
- cache de rendu et images de galerie sur le disque local du poste, hors du volume Banc ;
- rendus Fusion et « Render in Place » dans `Media/`.

## 5. Les fichiers

### Quatre classes, deux comportements

| Classe | Contenu | Distribution | Modifiable |
|---|---|---|---|
| `original` | rushes caméra | à la demande | non |
| `proxy` | proxys | automatique | non |
| `media` | son, graphismes, vidéos ajoutées | automatique | oui, avec révisions |
| `export` | rendus | automatique pour l'auteur, à la demande pour les autres | non |

- **Automatique** : dès qu'un fichier est publié, le serveur prévient les postes qui ont le projet ouvert. Ils le récupèrent tout de suite. Un poste qui ouvre le projet plus tard les récupère à l'ouverture.
- **À la demande** : le fichier est disponible pour tout le monde mais n'est téléchargé que sur demande. Bouton principal : « Récupérer les originaux de cette timeline », qui ne prend que les rushes utilisés par la timeline courante.
- Le rôle de personne peut changer la règle. Exemple : un étalonneur reçoit automatiquement les originaux des timelines qu'il ouvre.

### Publier

1. L'agent calcule l'empreinte des fichiers nouveaux (taille et date comparées à son index local avant de hasher).
2. Il envoie la liste au serveur, qui répond avec les empreintes inconnues.
3. Le poste ne pousse rien vers le VPS. Il annonce ses fichiers, et ce sont les autres machines qui viennent les chercher chez lui en connexion directe : le dépôt d'abord, et les postes qui ont le projet ouvert en même temps.
4. Le cache du VPS ne reçoit un fichier que dans deux cas : le dépôt n'est pas joignable en direct depuis ce poste (hors ligne, ou traversée de NAT impossible), ou l'utilisateur demande « Publier et éteindre » alors que le dépôt est trop lent à recevoir.
5. Classe `original` : toujours en direct vers le dépôt, jamais dans le cache.
6. Tant que le dépôt n'a pas tout confirmé, l'interface affiche ce qui reste à envoyer. Rien n'est perdu puisque le poste garde ses fichiers.

### Récupérer

Le pair-à-pair passe avant tout. Une connexion directe entre deux machines ne coûte rien au VPS. Une connexion relayée ou un téléchargement depuis le cache consomme sa bande passante.

Sources, par ordre de préférence :

1. un poste du même LAN ;
2. toute machine joignable en connexion directe : postes allumés et dépôt ;
3. le cache du VPS, si le fichier s'y trouve ;
4. une machine joignable seulement par le relais. Dans ce cas le serveur met aussi le fichier en cache au passage, pour que les suivants ne repassent pas par le relais.

Règles :

- **Plusieurs sources à la fois.** Les fichiers sont répartis entre toutes les sources directes disponibles, et les gros fichiers découpés par plages. Chaque bloc est vérifié par empreinte, donc mélanger les sources est sans risque.
- **Tout poste allumé sert les autres**, même Resolve fermé. C'est une réponse à une demande, sans scan ni surveillance, donc sans coût au repos.
- **Limites d'envoi par poste**, pour ne pas gêner celui qui travaille : sans limite sur le LAN, plafonné à la moitié du débit montant mesuré vers l'extérieur, en pause pendant un rendu. Réglable par l'admin.
- L'agent teste la connexion directe avant de choisir, et rebascule sur une source directe dès qu'une devient disponible en cours de transfert.
- Quatre fichiers en parallèle, reprise après coupure.

Option pour le dépôt : ouvrir un port UDP sur la box du NAS garantit des connexions directes vers lui dans presque tous les cas. C'est facultatif, le reste fonctionne sans.

### Fichiers modifiés (classe `media`)

Un logo retouché ou une musique remplacée gardent leur chemin mais changent de contenu.

- **Détection.** Pendant que Resolve est ouvert, et à chaque publication, l'agent compare taille et date des fichiers `media` à son index. Un fichier changé est rehashé et publié comme nouvelle révision du même chemin.
- **Réception.** Les autres postes téléchargent la révision dans un fichier temporaire, puis remplacent l'ancien d'un seul geste (renommage atomique). Ensuite l'assistant demande à Resolve de recharger le clip.
- **Fichier verrouillé.** Sur Windows, Resolve peut tenir le fichier ouvert et bloquer le remplacement. Banc réessaie à la fermeture du projet et affiche d'ici là « logo.png : nouvelle version en attente ».
- **Conflit.** Si deux personnes modifient le même fichier sans avoir vu la révision de l'autre, la première publication garde le chemin. La seconde est enregistrée à côté sous un nom dérivé (`logo (Paul, 11-11).png`), et les deux personnes sont prévenues.
- **Historique.** Toutes les révisions restent dans le dépôt. On peut restaurer une révision depuis Banc.

### Fichiers immuables (classes `original` et `proxy`)

Un fichier de ces classes ne doit jamais changer. Si son contenu diffère de l'empreinte publiée, Banc ne propage rien. Il signale une corruption possible et propose de restaurer depuis le dépôt.

### Copies et sécurité des originaux

- Un original est « en sécurité » quand il existe en deux copies vérifiées : le poste qui a fait l'ingest et le dépôt.
- À ce moment, et pas avant, Banc affiche « Deux copies vérifiées, tu peux formater la carte ».
- Banc prévient le poste d'ingest qu'il porte la seconde copie et lui demande de la garder.
- « Libérer l'espace » est refusé pour un original tant qu'il n'existe pas ailleurs une autre copie en plus du dépôt (autre poste, machine de rendu, second dépôt).
- L'admin voit dans le tableau de bord le nombre de copies de chaque projet.
- Un second dépôt (autre disque ou autre lieu) peut être ajouté plus tard. Il s'enrôle comme le premier.

### Suppressions

- Retirer un clip du projet ou supprimer un fichier sur un poste ne supprime rien dans le dépôt. Le fichier passe à l'état `retire` et n'est plus distribué.
- Seul l'admin supprime des blobs, à l'archivage ou à la suppression d'un projet, avec confirmation.

### Espace disque des postes

- Les proxys et médias d'un projet non ouvert depuis N jours (réglable, 30 par défaut) sont proposés à la libération. Banc ne libère que ce que le dépôt a confirmé.
- Une alerte prévient quand le volume Banc d'un poste passe sous 10 % d'espace libre.

### Cache du VPS

Le cache est un secours, pas le chemin normal. Il sert quand aucune source directe n'existe : postes éteints et dépôt injoignable en direct, ou réseau qui bloque les connexions directes.

- Quota réglable, sortie des fichiers les moins demandés en premier.
- L'admin voit, par projet et par mois, la part des transferts faite en direct, par le relais et par le cache. C'est l'indicateur à surveiller pour le coût.
- Expiration après N jours sans demande.
- Un fichier n'est jamais supprimé du cache tant que le dépôt ne l'a pas confirmé.
- Les fichiers des projets marqués actifs restent en cache dans la limite du quota.
- L'admin voit l'occupation du cache et le volume sorti du VPS sur le mois.

### Dépôt NAS

- Blobs rangés par empreinte dans un dossier choisi par l'admin. Aucune interface.
- Vérification complète de l'intégrité une fois par mois, à basse priorité.
- Reçoit chaque nuit les sauvegardes SQLite et PostgreSQL du VPS.

## 6. La vie d'un projet

### Créer un projet

1. Dans Banc, « Nouveau projet » : nom, mode, membres, modèle de projet.
2. Le modèle (défini par l'admin) porte résolution, cadence, gestion couleur, format de proxy et les réglages imposés de la section 4.
3. Banc crée l'arborescence dans le volume Banc et la déclare au serveur.
4. Banc crée le projet dans Resolve par script. En collaboratif, dans la bibliothèque partagée. En réservation, dans la base locale, suivi d'une première publication `.drp`.

### Ajouter des rushes

Bouton « Ajouter des rushes » dans Banc :

1. Choix de la source (carte, disque, dossier), du jour de tournage et de la caméra. Banc range dans `Rushes/J03/CamA/` en gardant la structure d'origine de la carte.
2. Copie vérifiée par empreinte. La carte n'est jamais modifiée. Un rapport est gardé dans `.banc/`.
3. Envoi des originaux au dépôt.
4. Génération des proxys (voir ci-dessous) et publication.
5. Import des clips dans le projet par script, dans des chutiers par jour et par caméra.
6. Message « Deux copies vérifiées » quand le dépôt a tout confirmé.

Quelqu'un qui copie des rushes à la main dans le volume Banc, ou qui les importe depuis un disque externe, est rattrapé par la consolidation.

### Proxys

- Lieu de génération, au choix de l'admin par projet : le poste qui ingère (par défaut), ou une machine de rendu s'il y en a une.
- Outil à choisir au prototype P4 : Blackmagic Proxy Generator, file de rendu de Resolve en clips individuels, ou ffmpeg pour les formats qu'il lit.
- Une surveillance de dossiers est admise pendant la génération, et seulement pendant.
- Lien proxy et original : automatique quand le proxy est dans le sous-dossier `Proxy`. Secours : `MediaPoolItem.LinkProxyMedia`.
- Format de proxy à choisir au prototype, avec une contrainte : lisible et générable sur les trois OS.

### Consolidation automatique

Elle rattrape tout fichier entré dans le projet par un autre chemin, par exemple une musique glissée des Téléchargements dans la timeline.

1. **Quand.** Seulement pendant que Resolve est ouvert sur un projet Banc. Déclencheurs : changement du nombre de clips du projet, publication, fermeture du projet. Filet de sécurité toutes les 5 minutes.
2. **Quoi.** L'assistant lit le chemin des clips ajoutés depuis le dernier passage (`GetClipProperty("File Path")`). Il ne reparcourt jamais tout le projet, il garde un index des clips déjà vus.
3. **Copie.** Un clip hors du volume Banc est copié dans le projet : `Audio/` pour le son, `Graphics/` pour les images, `Media/` pour les vidéos.
4. **Remplacement.** Banc pointe le clip vers la copie (`MediaPoolItem.ReplaceClip`). La timeline ne bouge pas.
5. **Publication** en classe `media`, donc distribution automatique.

Garde-fous :

- La consolidation tourne sur le poste de la personne qui a importé le fichier. C'est elle qui tient le chutier en mode collaboratif, donc le remplacement ne se heurte pas au verrou d'un autre.
- Si le remplacement échoue (chutier verrouillé), Banc réessaie plus tard et le fichier est quand même publié.
- Au-dessus d'un seuil (5 Go par défaut), Banc demande avant de copier.
- Deux fichiers différents du même nom : le second est renommé.
- Un fichier déjà dans le volume Banc est publié sans copie.

## 7. Deux modes de projet

### Mode réservation

- Ouvrir : dernière version `.drp` importée dans la base locale, verrou pris si l'utilisateur veut modifier.
- Publier : export `.drp`, nouvelle version, verrou relâché ou gardé.
- Un seul détenteur du verrou. L'admin peut le forcer. Un verrou sans signe de vie depuis 24 h est signalé à l'admin.
- Un projet local du même nom est renommé, jamais écrasé.
- Fonctionne hors ligne. Les publications partent au retour de la connexion.

### Mode collaboratif

La collaboration native de Resolve sur une base PostgreSQL partagée, joignable sans VPN.

**Sur le VPS**

- PostgreSQL 18 en Docker (version embarquée par le Project Server de Resolve 21.1, qui accepte les bibliothèques réseau à partir de PostgreSQL 13).
- Sauvegardes reprises du projet `elliotmatson/Docker-Davinci-Resolve-Project-Server` (`prodrigestivill/postgres-backup-local`, format `.backup` restaurable depuis Resolve). Fréquence : toutes les heures, car tous les postes partagent le même compte PostgreSQL et une erreur peut tout toucher.
- Pas de pgAdmin en service.

**Le tunnel**

- La fenêtre « Ajouter une bibliothèque de projets » de Resolve n'a pas de champ port. Resolve se connecte sur 5432.
- `banc-agent` écoute sur `127.0.0.2:5432` pour ne pas gêner un PostgreSQL local. Linux et Windows acceptent déjà cette adresse. Sur macOS, le service ajoute l'alias au démarrage (`ifconfig lo0 alias 127.0.0.2 up`).
- Chaque connexion passe dans un flux iroh jusqu'au VPS, qui la relie à son PostgreSQL local.
- Le tunnel n'est ouvert que pendant que Resolve tourne.

**Déclarer la bibliothèque dans Resolve**, pistes dans l'ordre :

1. La clé d'accès `.resolvedbkey`, que le Project Server sait générer. Glissée sur le gestionnaire de projets, elle ajoute la base sans saisie. Si Banc peut la générer lui-même, l'utilisateur n'a rien à taper.
2. L'API : `ProjectManager.GetDatabaseList()` et `SetCurrentDatabase({...})`.
3. Le fichier de configuration où Resolve garde sa liste de bibliothèques.
4. Secours : Banc affiche les quatre valeurs avec un bouton « copier » par champ, à coller une fois dans Réseau > Ajouter une bibliothèque de projets > Connecter.

**Garde-fous**

- Avant d'ouvrir un projet collaboratif, Banc vérifie que les fichiers à distribution automatique sont présents.
- Au-dessus de 40 ms de latence vers le VPS, Banc prévient et propose le mode réservation.
- Tunnel coupé : l'interface le signale tout de suite.

## 8. La version de Resolve

- L'admin fixe dans le manifeste la version que tous les postes doivent avoir.
- L'agent détecte la version installée à l'enrôlement, à chaque connexion et à chaque lancement de Resolve.
- Un poste qui n'a pas la bonne version ne peut pas ouvrir de projet par Banc. Banc affiche pourquoi, télécharge le bon installeur depuis le serveur et le lance.
- **Changement de version**, lancé par l'admin :
  1. L'admin publie un manifeste avec la nouvelle version et une date de bascule.
  2. Tous les postes sont prévenus. Chacun télécharge l'installeur à l'avance.
  3. À la bascule, Banc demande de fermer les projets collaboratifs et vérifie que plus personne n'est connecté à la base.
  4. Sauvegarde PostgreSQL complète, envoyée au dépôt.
  5. Les postes installent la nouvelle version. Le premier poste à jour ouvre la bibliothèque, que Resolve met à niveau.
  6. Les postes pas encore à jour restent bloqués jusqu'à leur mise à jour.
- **Contrôle de compatibilité.** Après chaque changement de version de Resolve, l'agent vérifie que les fichiers de préférences qu'il écrit ont toujours le format attendu. Sinon il n'écrit rien, passe en mode guidé (l'utilisateur fait le réglage une fois) et prévient l'admin.

## 9. Le rendu

Une machine de rendu est un plus. Tout fonctionne sans.

- **Rendu local intercepté.** Pendant que Resolve est ouvert, l'assistant suit la file de rendu (`GetRenderJobList`, `GetRenderJobStatus`). À la fin d'un rendu, Banc publie le fichier en classe `export` avec la timeline, l'auteur et la date, et prévient l'équipe. Un rendu fait hors de `Exports/` y est copié.
- **Rendu différé.** « Rendre plus tard » met le rendu en file sur le poste de l'utilisateur. Il part à une heure choisie ou quand le poste est inactif.
- **Rendu délégué.** Un poste peut se déclarer disponible pour les rendus des autres (la nuit, ou quand il est inactif). Banc y récupère d'abord les originaux de la timeline, puis lance le rendu.
- **Machine dédiée**, s'il y en a une : même mécanisme, toujours prioritaire.
- Ordre de choix : machine dédiée libre, poste volontaire inactif, sinon le poste de l'utilisateur.
- Une machine ne reçoit un rendu que si elle a la bonne version de Resolve et tous les paquets requis.
- Script `render_job.py` : ouverture du projet, `LoadRenderPreset`, `SetRenderSettings`, `AddRenderJob`, `StartRendering`, suivi de la progression.
- Notification système, et webhook optionnel (Discord, Slack, ntfy).

## 10. Ce qui tourne, et quand

| État du poste | Activité de Banc |
|---|---|
| Resolve fermé, rien demandé | Une connexion au serveur et un signe de vie toutes les 5 minutes. Rien d'autre. |
| Un autre poste demande un fichier | Envoi du fichier dans les limites de débit, puis retour au repos. |
| Transfert en cours | Le transfert, puis retour au repos. |
| Resolve ouvert sur un projet Banc | Tunnel PostgreSQL (collaboratif), consolidation, détection des médias modifiés, suivi de la file de rendu, réception automatique des nouveaux fichiers. |
| Génération de proxys | Surveillance du dossier concerné, jusqu'à la fin. |
| Heure d'un rendu différé | Lancement de Resolve et du rendu. |

Cibles au repos : service sous 15 Mo de RAM, assistant de session sous 10 Mo, CPU à 0 %.

## 11. Chemins par OS

À vérifier sur une vraie machine et à figer dans `banc-platform`.

| Élément | macOS | Windows | Linux |
|---|---|---|---|
| Plugins OFX | `/Library/OFX/Plugins` | `C:\Program Files\Common Files\OFX\Plugins` | `/usr/OFX/Plugins` |
| LUT | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/LUT` | `C:\ProgramData\Blackmagic Design\DaVinci Resolve\Support\LUT` | `/opt/resolve/LUT` ou `~/.local/share/DaVinciResolve/LUT` |
| Templates Fusion | `~/Library/Application Support/Blackmagic Design/DaVinci Resolve/Fusion/Templates` | `%APPDATA%\Blackmagic Design\DaVinci Resolve\Support\Fusion\Templates` | `~/.local/share/DaVinciResolve/Fusion/Templates` |
| Scripts | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/Fusion/Scripts` | `%PROGRAMDATA%\Blackmagic Design\DaVinci Resolve\Fusion\Scripts` | `/opt/resolve/Fusion/Scripts` |
| API de script | `/Library/Application Support/Blackmagic Design/DaVinci Resolve/Developer/Scripting/Modules` | `%PROGRAMDATA%\Blackmagic Design\DaVinci Resolve\Support\Developer\Scripting\Modules` | `/opt/resolve/Developer/Scripting/Modules` |

Le `README.txt` du dossier `Developer/Scripting` de la version installée fait foi.

## 12. Prototypes à faire avant de coder

Ce sont des scripts jetables, lancés à la main. Chacun répond à une question dont dépend une partie du plan. Noter les réponses dans `docs/prototypes.md`.

- **P1. Chemins.** Créer le volume APFS sur un Mac et la lettre `B:` sur un PC. Importer un média sur Mac dans un projet partagé, vérifier qu'il est en ligne sur le PC avec le Mapped Mount. Trouver le fichier des préférences Media Storage et vérifier qu'on peut y écrire.
- **P2. Proxys sans originaux.** Sur un poste qui n'a que les dossiers `Proxy`, les clips sont-ils reliés à leurs proxys et montables ?
- **P3. Bibliothèque partagée.** Générer une clé `.resolvedbkey` avec le Project Server, examiner son format, tester avec l'adresse `127.0.0.2`. Tester `SetCurrentDatabase` sur une base absente de la liste.
- **P4. Collaboration à distance.** Resolve sur une base PostgreSQL distante par un tunnel SSH. Mesurer la latence acceptable.
- **P5. Consolidation.** `ReplaceClip` sur un clip utilisé dans une timeline : la timeline reste-t-elle intacte ? En collaboratif, que se passe-t-il si le chutier est verrouillé par un autre ? Combien de temps prend la lecture des chemins de 10 000 clips ?
- **P6. Proxys.** Le Proxy Generator se commande-t-il sans sa fenêtre ? Quels formats sur chaque OS ? Sinon, file de rendu en clips individuels ou ffmpeg, puis `LinkProxyMedia`.
- **P7. Rendu.** Suivre la file de rendu par l'API pendant qu'un utilisateur lance un rendu à la main. Tester `-nogui`.
- **P8. Fichier ouvert.** Sur Windows, remplacer un fichier que Resolve utilise. Trouver comment faire recharger le clip.
- **P9. Plugins.** Tester l'installation silencieuse des plugins réellement utilisés par l'équipe.
- **P10. Technique.** Stabilité d'`iroh-blobs`. Version de Python acceptée par Resolve. Réglage « External scripting » de Resolve.

## 13. Phases

Durées indicatives pour un développeur assisté de Claude Code. Les phases 0 à 4 forment une première version utile au quotidien.

### Phase 0. Squelette (3 à 4 jours)

- Workspace Cargo, `xtask`.
- Toute l'intégration continue de la section 15 : workflows, protection de `main`, contrôle des messages de commit, `cargo xtask check`.
- Logs avec rotation, configuration TOML du serveur.

Acceptation :
- Les trois binaires se compilent pour toutes les cibles.
- Une PR avec une erreur de format, un avertissement clippy, un test cassé ou un commit mal nommé est refusée par la CI.
- On ne peut pas pousser directement sur `main`.

### Phase 1. Enrôlement, inventaire, clé admin (2 semaines)

- Serveur : enrôlement, invitations, machines, endpoint et relais iroh, admin web minimal.
- Agent : clé machine, connexion, signe de vie, service système, assistant de session, lien `banc://`, inventaire, détection de la version de Resolve.
- Clé admin : génération dans l'application, phrase de récupération, empreinte dans l'invitation.
- `cargo xtask idle-bench` : la commande existe depuis la phase 0 pour l'agent sous Linux. L'étendre à l'assistant de session, à macOS et à Windows.

Acceptation :
- Une machine de chaque OS s'enrôle avec un lien, sans terminal.
- L'admin voit les machines et leur version de Resolve.
- Cibles de consommation au repos tenues.

### Phase 2. Manifeste signé, installation, version de Resolve (2 à 3 semaines)

- Dépôt de paquets, éditeur de manifeste, signature à la publication.
- Agent : vérification de signature, comparaison état local et manifeste, installation (`copy`, `pkg`, `msi`, `exe`, `deb`, `rpm`, `run`, `font`, `manual`).
- Rien ne s'installe pendant que Resolve est ouvert. Banc ne désinstalle que ce qu'il a installé.
- Blocage de l'ouverture des projets si la version de Resolve ne correspond pas.
- Grille machines et paquets dans l'admin.

Acceptation :
- Un plugin `.ofx.bundle`, une police, une LUT et un preset s'installent seuls sur les trois OS.
- Un manifeste modifié directement dans la base du serveur est refusé par les postes.
- Un paquet dont l'empreinte ne correspond pas est refusé.

### Phase 3. Volume, fichiers, dépôt, cache (3 à 4 semaines)

- Création du volume Banc sur les trois OS, écriture du Mapped Mount.
- `banc-blobs` : magasin, empreintes, transfert vérifié, reprise, choix des sources.
- Rôle dépôt et image Docker. Cache du VPS.
- Classes de fichiers, distribution automatique et à la demande, révisions, détection de corruption, règle des deux copies, suppressions, libération d'espace.

Acceptation :
- Un poste publie 50 Go puis s'éteint. Un autre, sur un autre réseau sans port ouvert, les récupère.
- Deux postes du même LAN s'échangent les fichiers sans sortir sur Internet.
- Entre deux postes distants en connexion directe, 50 Go transférés ne font pas bouger le compteur de sortie du VPS.
- Un poste qui télécharge prend ses fichiers chez deux autres postes à la fois.
- Un fichier `media` modifié arrive en nouvelle révision chez les autres. Deux modifications concurrentes donnent deux fichiers et deux notifications.
- « Libérer l'espace » est refusé pour un original qui n'a qu'une copie ailleurs.
- Le cache ne supprime jamais un fichier absent du dépôt.

### Phase 4. Projets en réservation (3 à 4 semaines)

- Scripts Python du pont Resolve.
- Création de projet avec modèle et réglages imposés.
- Ingest : copie vérifiée, envoi au dépôt, proxys, import dans les chutiers.
- Consolidation automatique.
- Ouvrir, publier, verrou, travail hors ligne.
- « Récupérer les originaux de cette timeline ».

Acceptation :
- Un projet créé et alimenté sur Mac s'ouvre sur Windows et Linux avec ses proxys reliés.
- Une musique glissée des Téléchargements dans la timeline se retrouve dans `Audio/`, publiée, sans changement dans la timeline.
- Deux utilisateurs ne peuvent pas détenir le verrou en même temps.

### Phase 5. Collaboration (2 à 3 semaines)

- PostgreSQL et sauvegardes sur le VPS, copie vers le dépôt.
- `banc-tunnel`, alias `127.0.0.2`, déclaration de la bibliothèque selon la piste retenue.
- Création de projet collaboratif, distribution automatique en direct entre postes ouverts.
- Procédure de changement de version de Resolve.

Acceptation :
- Deux postes derrière deux box différentes travaillent dans le même projet, sans VPN ni port ouvert.
- Une musique ajoutée par l'un est en ligne chez l'autre en moins d'une minute.
- PostgreSQL n'est joignable depuis Internet par aucun port.
- Une sauvegarde est présente sur le NAS et se restaure depuis Resolve.

### Phase 6. Rendu (2 à 3 semaines)

- Rendu local intercepté et publication dans `Exports/`.
- Rendu différé.
- Rendu délégué à un poste volontaire ou à une machine dédiée.
- Notifications.

Acceptation :
- Un rendu lancé à la main dans Resolve apparaît dans `Exports/` chez son auteur et dans la liste du projet pour les autres.
- Un rendu différé part à l'heure prévue.
- Une machine sans un plugin requis ne reçoit pas le rendu.

### Phase 7. Distribution et mises à jour (2 semaines)

- Mise à jour automatique signée, retour arrière si la nouvelle version ne démarre pas.
- Installeurs : `.pkg` signé et notarisé, `.msi` signé, `.deb`, `.rpm`, AppImage.
- Serveur en Docker Compose complet. Dépôt en image Docker pour Synology et QNAP.
- Sauvegarde nocturne de SQLite vers le dépôt.

Acceptation :
- Une nouvelle version arrive sur tous les postes dans l'heure.
- Une installation fraîche sur Mac n'affiche aucun avertissement Gatekeeper.

### Phase 8. Finitions (en continu)

- Archivage d'un projet : tout rassemblé sur le dépôt, `.drp` final, libération du cache et des postes.
- Tableau de bord admin : copies par projet, espace des postes, transferts, cache, rendus.
- Diagnostic en un clic.
- Documentation utilisateur d'une page, documentation admin.

## 14. Prompts de démarrage pour Claude Code

Une session par ligne :

1. « Lis CLAUDE.md et docs/PLAN.md. Réalise la phase 0. »
2. « Prépare les prototypes P1 et P2 : les scripts et la marche à suivre, que je lance à la main. Dis-moi quoi observer et où noter le résultat. »
3. « Prototypes P3 et P4. »
4. « Prototypes P5 à P8. »
5. « Phase 1, partie serveur. Écris les tests. »
6. « Phase 1, partie agent. Commence par macOS. »
7. Puis phase par phase.

Après chaque phase, demande à Claude Code de mettre à jour `CLAUDE.md` avec les commandes réelles, les décisions prises et les mesures de `idle-bench`. Après chaque prototype, fais-lui corriger ce plan si la réponse change une hypothèse.

## 15. Intégration continue et commits

À mettre en place en phase 0, avant tout code de produit.

### Règles du dépôt

- `main` est protégée : pas de push direct, pas de force push, tout passe par une PR.
- Une PR ne peut être fusionnée que si tous les contrôles obligatoires sont verts et que la branche est à jour avec `main`.
- Fusion en « squash ». Le titre de la PR devient le message du commit sur `main`, donc il suit la même convention que les commits.
- Une branche par tâche, nommée `type/sujet-court` (`feat/ingest-copie-verifiee`, `fix/tunnel-reconnexion`).

### Workflow `ci.yml`, à chaque push sur toute branche

Contrôles rapides, sur Linux, pour avoir une réponse en quelques minutes :

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets --locked -- -D warnings`
- `cargo nextest run --workspace --locked` et les tests de documentation
- Migrations SQL appliquées sur une base vide, puis vérification du schéma
- Frontend : `pnpm lint`, `svelte-check`, `vitest`, `pnpm build`
- Scripts Python du pont Resolve : `ruff check`, `ruff format --check`, compilation de chaque script
- `cargo deny check` : failles connues, licences autorisées, dépendances en double ou interdites
- Recherche de secrets dans le diff (`gitleaks`)
- Contrôle des messages de commit (voir plus bas)

### Workflow `pr.yml`, à chaque PR et à chaque mise à jour d'une PR

Tout `ci.yml`, plus ce qui coûte plus cher :

- **Matrice des trois OS** : clippy et tests sur Linux, macOS et Windows. Le code de `banc-platform` diffère par OS, un contrôle sur Linux seul ne le couvre pas.
- **Compilation de toutes les cibles** : Linux amd64 et arm64, macOS arm64 et amd64, Windows amd64.
- **Tests d'intégration de bout en bout**, sur Linux, sans Resolve (avec le faux de `banc-resolve`) : un serveur, un dépôt et deux agents lancés dans des conteneurs. Scénarios : enrôlement, manifeste signé accepté et manifeste falsifié refusé, publication et récupération d'un fichier, révision d'un fichier `media`, refus d'un paquet à l'empreinte modifiée, tunnel TCP vers un PostgreSQL de test.
- **Mesure au repos** : `cargo xtask idle-bench` sur l'agent Linux. La PR échoue si la mémoire ou le CPU au repos dépassent les cibles de la section 10.
- **Titre de la PR** contrôlé avec la même convention que les commits.
- Annulation automatique des exécutions en cours quand un nouveau commit arrive sur la même PR.

### Workflow `nightly.yml`, chaque nuit sur `main`

- Matrice complète des trois OS et toutes les cibles.
- `cargo deny check advisories` avec la base de failles du jour, pour être prévenu d'une faille publiée dans une dépendance déjà en place.
- Tests d'intégration longs (gros fichiers, coupure réseau simulée pendant un transfert, reprise).
- Ouverture automatique d'une issue en cas d'échec.

### Workflow `release.yml`, sur un tag `vX.Y.Z` (phase 7)

- Compilation de toutes les cibles, installeurs, signature et notarisation.
- Signature des binaires avec la clé de publication, gardée dans les secrets de la CI.
- Journal des changements généré à partir des commits.

### Autres réglages

- Cache de compilation (`Swatinem/rust-cache`) pour garder des exécutions courtes.
- `Cargo.lock` versionné et `--locked` partout.
- Mises à jour de dépendances proposées chaque semaine par Renovate ou Dependabot, en PR groupées.
- Les minutes de GitHub Actions sur un dépôt privé sont comptées plus cher sur macOS et Windows que sur Linux. C'est pour ça que les contrôles à chaque push restent sur Linux et que la matrice des trois OS tourne sur les PR. Vérifier les tarifs en vigueur au démarrage.

### `cargo xtask check`

Une commande locale qui lance les mêmes contrôles que `ci.yml`. Claude Code la lance avant chaque commit. Un commit qui ne passe pas `cargo xtask check` ne doit pas être créé.

### Nommage des commits

Convention Conventional Commits, contrôlée par la CI sur chaque commit d'une PR et sur son titre.

```
type(portée): description

Corps facultatif : pourquoi ce changement, pas comment.

Refs: phase 3, P5
```

- **Types** : `feat`, `fix`, `refactor`, `perf`, `test`, `docs`, `build`, `ci`, `chore`.
- **Portées**, une par commit, parmi : `server`, `agent`, `session`, `blobs`, `tunnel`, `resolve`, `platform`, `proto`, `ui`, `scripts`, `packaging`, `ci`, `docs`.
- La portée est obligatoire pour `feat`, `fix`, `refactor`, `perf` et `test`. Elle est facultative pour `docs`, `build`, `ci` et `chore`, qui touchent souvent tout le dépôt.
- **Description** en français, à l'infinitif, sans majuscule ni point final, 72 caractères au plus pour la première ligne.
- Un changement cassant porte un `!` après la portée et une ligne `BREAKING CHANGE:` dans le corps.
- Un commit fait une seule chose. Un refactor et une fonctionnalité vont dans deux commits.
- Le pied de message cite la phase du plan, et le prototype s'il y en a un. La ligne `Refs:` est obligatoire. Un travail de prototype seul s'écrit `Refs: P1, P2`.
- Les commits de Dependabot gardent leur propre format et ne sont pas contrôlés.

Exemples :

```
feat(blobs): choisir les sources directes avant le cache
fix(tunnel): rouvrir le flux après une coupure du relais
test(agent): couvrir le refus d'un manifeste mal signé
ci: ajouter la mesure au repos sur les PR
refactor(platform)!: regrouper les chemins Resolve par OS
```

Messages refusés : `update`, `wip`, `fix bug`, `corrections`, ou tout message sans type.
