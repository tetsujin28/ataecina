# Ataecina

Application gratuite de Domingo Olmo Martin (Dom) pour ramener des images perdues : lecture des cartes caméra exFAT et FAT32, fichiers supprimés, carte formatée par erreur, restauration du secteur de démarrage, image de carte façon GNU ddrescue, recherche PhotoRec, et réparation par lot des MOV / MP4 cassés avec untrunc et un rush sain de référence.

**Windows uniquement.** Bêta (version 0.11.4). Licence MIT. App sœur de [Bandua](https://github.com/tetsujin28/bandua).

## Règles de travail

- **Ne jamais lancer de fabrication GitHub Actions sans que Dom le demande explicitement.** Cela inclut « Run workflow » (`workflow_dispatch`) et la publication d'une Release, qui déclenche aussi `build.yml`.
- **Toujours passer par une branche et une pull request**, fusionnée sur GitHub (commits « Verified »). Ne jamais pousser directement sur `main`.
- **Aucun tiret cadratin (caractère U+2014)** dans les textes écrits pour Dom : interface, journal, README, notes de version, messages de commit. Utiliser virgules, points ou parenthèses.
- Échanges avec Dom en français. Code, identifiants et commentaires en français, comme le code existant (`analyser`, `copier`, `Etat`…).
- Interface et messages **toujours en trois langues** (français, anglais, espagnol). Le texte français sert de clé : ajouter la traduction dans `core/src/i18n.rs` (fonction `l()`, messages du moteur) ou dans `ui/i18n.js` (`I18N_DICT`). Une clé absente reste en français.
- README : `README.md` (anglais, langue principale), `README.fr.md`, `README.es.md`, à tenir synchronisés, avec les captures `docs/*-en|fr|es.webp`.
- Interface sombre.

## Principe non négociable

**Les fichiers d'origine et la carte ne sont jamais modifiés**, sauf action explicite de restauration du secteur de démarrage demandée par l'utilisateur. Tout le reste lit la carte (ou son image) et écrit ailleurs. La lecture directe d'une carte demande les droits administrateur ; l'app propose de se relancer en administrateur.

## Organisation

```
core/                 ataecina-core (moteur, sans interface)
  src/fsread.rs       lecture exFAT / FAT32 en lecture seule, fichiers supprimés, recherche après formatage
  src/analyse.rs      analyse d'une carte, copie des fichiers retrouvés, restauration du secteur de démarrage
  src/imager.rs       image de carte façon ddrescue (zones saines d'abord, reprise)
  src/photorec.rs     pilotage de PhotoRec
  src/repair.rs       réparation untrunc (mode normal puis pas à pas), vérification ffprobe / ffmpeg
  src/platform.rs     lecteurs, droits administrateur, écriture de secteurs (Windows, windows-sys)
  src/source.rs       abstraction de source (fichier, lecteur, MemSource pour les tests)
  src/tools.rs        localisation de untrunc, ffmpeg, ffprobe, photorec
  src/i18n.rs         traductions du moteur
  src/bin/cli.rs      ataecina-cli, pour les tests
core/tests/cartes.rs  tests sur de vraies petites cartes (fixtures .img.gz exFAT / FAT32, formatées ou non)
src-tauri/            application Tauri 2. Une seule commande `api(path, body)` qui route vers /api/state, /api/fs/*, /api/recover/*, /api/start…
ui/                   index.html (interface complète), i18n.js : HTML/CSS/JS sans npm ni framework
```

Les outils externes (untrunc, FFmpeg, TestDisk / PhotoRec) ne sont **pas** livrés : l'utilisateur les installe et les place à côté de l'exe, dans `tools/` ou dans le PATH.

## Commandes

```
cargo test -p ataecina-core           # à lancer avant chaque PR (passe aussi sous Linux)
cargo install tauri-cli --version "^2" --locked
cd src-tauri && cargo tauri dev       # lancer l'app (Windows)
```

Hors Windows, l'interface peut être testée dans un navigateur avec la feature `serveur-test` : `ATAECINA_HTTP=127.0.0.1:8080 cargo run -p ataecina --features serveur-test` (ajouter `ATAECINA_DEMO_WINDOWS=1` pour simuler Windows, `ATAECINA_CONFIG` pour choisir le fichier de configuration).

## Publier une version

Changer le numéro dans `core/Cargo.toml`, `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, la constante `VERSION` de `src-tauri/src/main.rs` (« Bêta x.y.z ») et les deux mentions « Bêta x.y.z » de `ui/index.html`. Dom crée ensuite la Release `vX.Y.Z` lui-même. Fichiers produits : `Ataecina_x.y.z_portable.exe`, `-setup.exe` et `.msi`.
