# P2. Proxys sans originaux

Question de la section 12 du plan : sur un poste qui n'a que les dossiers `Proxy`, les clips sont-ils reliés à leurs proxys et peut-on monter ?

C'est le cas normal d'un poste Banc : les originaux restent sur le dépôt et sur le poste qui a ingéré, les autres récupèrent les proxys (section 5 du plan). Si la réponse est non, le partage des fichiers est à revoir.

Il faut un poste avec Resolve Studio, idéalement deux (Mac et PC) pour le scénario B. Le volume Banc de P1 aide mais n'est pas obligatoire : un dossier quelconque suffit pour le scénario A. Compter deux à trois heures.

Les scripts sont dans `docs/prototypes/scripts/`. Lire d'abord son `README.md`. Les commandes ci-dessous se lancent depuis ce dossier.

## 1. Préparer les rushes et leurs proxys

De vrais rushes de l'équipe valent mieux que des fichiers de test, surtout s'ils sont en BRAW ou en R3D. Les proxys doivent finir dans un sous-dossier `Proxy` à côté des originaux, avec le même nom de fichier :

```
/Volumes/Banc/P2/Rushes/J01/CamA/A001.braw
/Volumes/Banc/P2/Rushes/J01/CamA/Proxy/A001.mov
```

Trois façons de les obtenir, de la plus proche du futur Banc à la plus simple. En faire au moins une, noter laquelle :

1. **Proxys générés par Resolve.** Dans les réglages du projet, régler l'emplacement de génération des proxys pour qu'ils aillent dans un sous-dossier à côté des originaux (noter le nom exact du réglage et sa valeur). Importer les originaux, clic droit > Generate Proxy Media.
2. **Blackmagic Proxy Generator**, en surveillance du dossier `Rushes`. Noter s'il range les proxys au même endroit.
3. **Fichiers de test**, si on n'a rien d'autre :

   ```bash
   python3 medias_test.py /Volumes/Banc/P2/Rushes/J01/CamA --nombre 3
   ```

   Originaux UHD en ProRes 422 HQ, proxys 960x540 en ProRes Proxy, même timecode.

Dans Resolve, créer le projet `P2`, importer les originaux (pas les proxys). Monter une timeline avec les trois clips : une coupe, un clip recadré, une transition.

```bash
python3 inventaire.py --sortie ../p2/0-complet.json
```

À noter :

- [ ] Avec les options 2 et 3, Resolve a-t-il relié seul les proxys à l'import ? Dans `0-complet.json`, le champ `Proxy Media Path` de chaque clip doit être rempli.
- [ ] Le nom des champs de proxy que donne l'API (`Proxy`, `Proxy Media Path`, d'autres ?).

Si les proxys ne sont pas reliés, passer à la section 4 avant de continuer.

## 2. Scénario A : même poste, originaux retirés

1. Quitter Resolve.
2. Déplacer les originaux hors du volume, sans toucher au dossier `Proxy` :

   ```bash
   mkdir -p ~/P2-originaux-retires
   ```

   ```bash
   mv /Volumes/Banc/P2/Rushes/J01/CamA/A00* ~/P2-originaux-retires/
   ```

3. Ouvrir Resolve et le projet `P2`.

   ```bash
   python3 inventaire.py --sortie ../p2/A-sans-originaux.json
   ```

À observer, pour chacun des trois réglages du menu Playback > Proxy Handling (noter les noms exacts) :

- [ ] Les clips du Media Pool apparaissent-ils hors ligne, ou avec leur proxy ? Quelle icône ou quel badge ?
- [ ] Le viewer et la timeline se lisent-ils ?
- [ ] Peut-on monter : poser un clip dans la timeline, le couper, le raccourcir, changer un recadrage ?
- [ ] La page Color travaille-t-elle sur le proxy ? Message d'avertissement ?
- [ ] Le recadrage de la timeline (fait sur l'original UHD) reste-t-il juste sur le proxy 960x540 ? C'est la question de la résolution du proxy par rapport à la timeline.
- [ ] Rendu depuis la page Deliver sans les originaux : refusé, fait sur les proxys, ou fait avec un avertissement ? Noter le réglage de rendu concerné (« Use proxy media » ou équivalent).

Remettre ensuite les originaux à leur place, Resolve fermé, rouvrir le projet :

- [ ] Resolve repasse-t-il seul sur les originaux, sans relier à la main ?

## 3. Scénario B : un autre poste, proxys seuls

Le cas réel. Il faut que P1 ait répondu pour le Mapped Mount si les deux postes n'ont pas le même OS.

1. Sur le premier poste, exporter `P2.drp`.
2. Sur le second poste, recréer seulement les dossiers `Proxy` au même chemin (`/Volumes/Banc/P2/...` ou `B:\P2\...`), sans aucun original.
3. Importer `P2.drp`, ouvrir le projet.

   ```bash
   python3 inventaire.py --sortie ../p2/B-autre-poste.json
   ```

Mêmes observations que le scénario A, plus :

- [ ] Les proxys sont-ils trouvés alors qu'ils n'ont jamais existé à ce chemin sur ce poste ?
- [ ] Ajouter un clip à la timeline sur ce poste, exporter le `.drp`, le rouvrir sur le premier poste qui a les originaux : le montage fait sur proxys s'applique-t-il bien aux originaux ?

## 4. Secours : relier par l'API

À faire si Resolve n'a pas relié les proxys seul, à l'import ou dans un des scénarios. D'abord la liste, sans rien modifier :

```bash
python3 lier_proxys.py
```

Puis le lien :

```bash
python3 lier_proxys.py --appliquer --sortie ../p2/liens.json
```

À noter :

- [ ] `LinkProxyMedia` accepte-t-il le lien quand l'original est hors ligne (`lien_accepte`) ?
- [ ] Le lien survit-il à la fermeture et la réouverture du projet ?
- [ ] Sur le scénario B entre Mac et PC : le champ `original` est-il au format de l'autre OS ? Dans ce cas le script ne trouve pas le dossier `Proxy`, et c'est une information pour P5.

## Où noter la réponse

Dans `docs/prototypes.md`, section « P2. Proxys sans originaux », avec le modèle de réponse du fichier : date, version de Resolve, OS, origine des proxys (Resolve, Proxy Generator ou fichiers de test), et la réponse à chaque question.

Ajouter dans `docs/prototypes/p2/` les fichiers JSON produits et les captures d'écran utiles (Media Pool, réglages de proxy, message de rendu).

Mettre à jour les sections 5 et 6 de `docs/PLAN.md` (« Récupérer », « Proxys ») si une réponse contredit une hypothèse, par exemple si les proxys seuls ne suffisent pas pour monter.
