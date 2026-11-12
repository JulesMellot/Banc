# P1. Chemins, Mapped Mount, préférences Media Storage

Questions de la section 12 du plan :

1. Peut-on créer le volume Banc comme le prévoit la section 4 : volume APFS `/Volumes/Banc` sur Mac, lettre `B:` persistante sur Windows ?
2. Un média importé sur Mac dans un projet est-il en ligne sur le PC grâce au Mapped Mount, sans relier à la main ? Et dans l'autre sens ?
3. Dans quel fichier Resolve range-t-il Media Storage et Mapped Mount, sous quel format, et Banc peut-il y écrire ?

Il faut un Mac et un PC Windows avec la même version de Resolve Studio, et une clé USB ou un partage réseau pour passer les fichiers. Compter une demi-journée.

Les scripts sont dans `docs/prototypes/scripts/`. Lire d'abord son `README.md` (Python, réglage « External scripting »). Les commandes ci-dessous se lancent depuis ce dossier.

## 1. Le volume sur Mac

```bash
diskutil apfs list
```

Repérer le conteneur APFS du disque interne : celui qui contient le volume `Macintosh HD - Data`, souvent `disk3` sur Apple Silicon.

```bash
sudo diskutil apfs addVolume disk3 APFS Banc
```

À observer et noter :

- [ ] Le volume est monté sur `/Volumes/Banc` sans autre commande.
- [ ] `touch /Volumes/Banc/essai` fonctionne sans `sudo`, avec le compte du monteur.
- [ ] `diskutil info /Volumes/Banc` : noter les lignes « Owners » et « Mount Point ».
- [ ] Après un redémarrage, le volume est remonté au même endroit avant l'ouverture de Resolve.
- [ ] L'espace libre affiché est celui du disque interne (le volume partage le conteneur).
- [ ] Le volume apparaît-il sur le bureau et dans le Finder ? Spotlight l'indexe-t-il (`mdutil -s /Volumes/Banc`) ?

## 2. La lettre `B:` sur Windows

Dans PowerShell lancé en administrateur. Remplacer `D:\Banc` par `C:\Banc` s'il n'y a qu'un disque.

```powershell
New-Item -ItemType Directory -Force D:\Banc
```

```powershell
reg add "HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\DOS Devices" /v B: /t REG_SZ /d "\??\D:\Banc" /f
```

Redémarrer le PC.

À observer et noter :

- [ ] `B:` apparaît dans l'Explorateur, pour le compte du monteur.
- [ ] Le monteur, sans droits d'administrateur, peut créer un fichier dans `B:\`.
- [ ] L'espace libre affiché pour `B:` est celui de `D:`.
- [ ] Un service système voit aussi `B:` (l'agent Banc sera un service). Test, en administrateur :

  ```powershell
  schtasks /create /tn banc-p1 /tr "cmd /c dir B:\ > C:\banc-p1.txt" /sc once /st 23:59 /ru SYSTEM /f
  ```

  ```powershell
  schtasks /run /tn banc-p1
  ```

  Lire `C:\banc-p1.txt`, puis supprimer la tâche : `schtasks /delete /tn banc-p1 /f`.

## 3. Trouver le fichier des préférences

Sur chaque poste, dans l'ordre. Fermer Resolve avant chaque photo : il écrit souvent ses préférences en quittant.

```bash
python3 preferences.py photo 0-depart
```

1. Ouvrir Resolve. Préférences > Système > Media Storage : ajouter `/Volumes/Banc` (Mac) ou `B:\` (PC). Enregistrer, quitter Resolve.

   ```bash
   python3 preferences.py photo 1-volume
   ```

2. Ouvrir Resolve. Dans la même liste, sur la ligne du volume Banc, renseigner la colonne Mapped Mount avec le chemin de l'autre OS : `B:\` sur le Mac, `/Volumes/Banc` sur le PC. Enregistrer, quitter.

   ```bash
   python3 preferences.py photo 2-mapped
   ```

3. Comparer :

   ```bash
   python3 preferences.py ecarts 0-depart 1-volume
   ```

   ```bash
   python3 preferences.py ecarts 1-volume 2-mapped
   ```

Les photos sont écrites dans `docs/prototypes/p1/photos/`, que git ignore : elles peuvent contenir les identifiants des bases de projets. Ne recopier dans la réponse que l'extrait utile.

À noter :

- [ ] Le chemin exact du fichier modifié sur chaque OS, et s'il est propre à l'utilisateur ou commun au poste.
- [ ] Son format (XML, texte, binaire) et l'extrait qui décrit le volume et son Mapped Mount.
- [ ] D'autres fichiers modifiés en même temps.
- [ ] La forme acceptée par l'interface : `B:`, `B:\` ou `B:/` ; `/Volumes/Banc` avec ou sans `/` final.

## 4. Banc peut-il écrire ce fichier ?

1. Ouvrir Resolve, supprimer la ligne du volume Banc dans Media Storage, enregistrer, quitter.
2. Réécrire le fichier depuis la photo qui contient le Mapped Mount, Resolve fermé. Le script refuse si Resolve tourne et garde une copie `.banc-<date>` à côté du fichier :

   ```bash
   python3 preferences.py remettre 2-mapped "<chemin du fichier, copié depuis la sortie de ecarts>"
   ```

   Sur Windows, si le fichier est sous `C:\ProgramData`, lancer la commande en administrateur et noter que c'était nécessaire.
3. Ouvrir Resolve. La ligne du volume et son Mapped Mount sont-elles revenues ?
4. Si le fichier est du texte : Resolve fermé, modifier le Mapped Mount à la main dans un éditeur (par exemple `B:\` devient `C:\`), rouvrir Resolve et vérifier qu'il affiche la nouvelle valeur. Remettre ensuite la bonne valeur.

À noter :

- [ ] Resolve relit-il le fichier réécrit sans rien signaler ?
- [ ] Le fichier porte-t-il une somme de contrôle, une version ou une signature qui empêcherait une modification ?
- [ ] Faut-il des droits d'administrateur pour l'écrire ?

## 5. Un média importé sur Mac, ouvert sur le PC

1. Sur le Mac, fabriquer deux clips de test, ou copier deux vrais rushes au même endroit :

   ```bash
   python3 medias_test.py /Volumes/Banc/P1/Rushes/J01/CamA --nombre 2 --sans-proxys
   ```

2. Dans Resolve sur le Mac, créer le projet `P1` dans la bibliothèque locale, importer ces deux clips, les poser dans une timeline. Puis :

   ```bash
   python3 inventaire.py --sortie ../p1/mac.json
   ```

3. Fichier > Exporter le projet : `P1.drp`.
4. Copier le dossier `/Volumes/Banc/P1` vers `B:\P1` sur le PC, avec la même arborescence, et copier `P1.drp`.
5. Sur le PC, importer `P1.drp` dans la bibliothèque locale et l'ouvrir. Ne rien relier à la main.

   ```bash
   python inventaire.py --sortie ../p1/pc.json
   ```

Si une bibliothèque partagée (PostgreSQL ou Project Server) existe déjà entre les deux postes, refaire l'essai avec elle au lieu du `.drp`, et noter si le résultat diffère.

À observer et noter :

- [ ] Sur le PC, les clips sont-ils en ligne (pas de vignette rouge), et la timeline se lit-elle ?
- [ ] Dans `pc.json`, le champ `File Path` des clips : `/Volumes/Banc/...` ou `B:\...` ? C'est le chemin que la consolidation lira (P5). `fichier_present_ici` vaut `true` seulement si l'API donne le chemin traduit.
- [ ] Les clips restent-ils en ligne après avoir quitté Resolve et rouvert le projet sur le PC ?
- [ ] Sans le Mapped Mount (le retirer, rouvrir le projet), les clips passent-ils hors ligne ? C'est la preuve que c'est bien lui qui fait le travail.

## 6. Dans l'autre sens

Sur le PC, importer un troisième clip placé dans `B:\P1\Rushes\J01\CamA`, l'ajouter à la timeline, exporter le `.drp`, le copier avec le clip sur le Mac, l'ouvrir.

- [ ] Le clip ajouté sur le PC est-il en ligne sur le Mac ?
- [ ] `inventaire.py` sur le Mac : quel `File Path` pour ce clip ?

## Où noter la réponse

Dans `docs/prototypes.md`, section « P1. Chemins », avec le modèle de réponse du fichier : date, version de Resolve, versions de macOS et Windows, réponse à chaque question, et ce que ça change dans le plan.

Ajouter dans `docs/prototypes/p1/` : `mac.json`, `pc.json`, l'extrait du fichier de préférences, et les captures d'écran utiles (Media Storage, clips en ligne ou hors ligne).

Mettre à jour la section 4 de `docs/PLAN.md` si une réponse contredit une hypothèse : commande de création du volume, valeur du registre, forme du Mapped Mount, fichier à écrire.
