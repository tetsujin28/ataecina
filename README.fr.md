<p align="center">
  <img src="src-tauri/icons/128x128.png" width="96" alt="Ataecina logo">
</p>

<h1 align="center">Ataecina</h1>
<p align="center"><b>by Domingo Olmo Martin</b></p>
<p align="center">
  <a href="README.md">English</a> · <b>Français</b> · <a href="README.es.md">Español</a>
</p>

Ataecina ramène les images perdues. Il lit les cartes de caméra (exFAT et FAT32), retrouve les fichiers supprimés et ceux d'une carte formatée par erreur, restaure le format d'une carte abîmée, et répare les rushes MOV et MP4 cassés à l'aide d'un rush sain de référence.

Gratuit, pour Windows. Interface en français, anglais et espagnol.

> Ataecina est une déesse de la péninsule ibérique antique, souvent interprétée comme « la Renaissante ». Comme elle, ce logiciel ramène à la lumière des images que l'on croyait perdues.

## Téléchargement

Rendez-vous sur la **[page des Releases](https://github.com/tetsujin28/ataecina/releases)** :

| Fichier | Usage |
|---|---|
| `Ataecina_x.y.z_portable.exe` | Sans installation, il suffit de le lancer |
| `Ataecina_x.y.z_x64-setup.exe` ou `.msi` | Installateur |

**Premier lancement.** L'application n'est pas encore signée : Windows affiche un avertissement une fois. Cliquez sur « Informations complémentaires », puis « Exécuter quand même ».

La lecture directe d'une carte demande les droits administrateur. Ataecina propose de se relancer en administrateur quand c'est nécessaire.

## Ce qu'il fait

**Onglet Récupération**
- **Lecture de la carte** (exFAT, FAT32) : noms et dossiers d'origine, fichiers supprimés, recherche approfondie après un formatage, restauration du secteur de démarrage depuis sa copie de secours.
- **Recherche PhotoRec** : tous formats de carte, MOV / MP4 / BRAW, R3D, ARRIRAW, MXF, pour les fichiers en morceaux.
- **Image de la carte** au préalable, selon la méthode de GNU ddrescue : zones saines d'abord, zones abîmées en dernier, reprise après une interruption.

**Onglet Réparation**
- Réparation par lot avec untrunc et une ou plusieurs références, mode normal puis mode pas à pas.
- Vérification par ffprobe (et décodage complet par ffmpeg sur demande), vignettes, `journal.csv`.

Les fichiers d'origine ne sont jamais modifiés.

## Outils utilisés

Ataecina pilote ces outils gratuits, à installer depuis leur site officiel :
- [untrunc](https://github.com/anthwlock/untrunc) (réparation)
- [FFmpeg](https://ffmpeg.org) (ffprobe et ffmpeg, vérifications et vignettes)
- [TestDisk / PhotoRec](https://www.cgsecurity.org) (recherche PhotoRec)

Placez-les à côté de `Ataecina.exe`, dans un dossier `tools`, ou dans le PATH. Leur emplacement peut aussi être indiqué dans l'application.

## Compilation

Il faut Rust et la CLI Tauri 2.

```
cargo test -p ataecina-core
cd src-tauri && cargo tauri build
```

Le moteur (`core`) fournit un outil en ligne de commande, `ataecina-cli`, utilisé pour les tests.

## Licence

MIT. © 2026 Domingo Olmo Martin.
