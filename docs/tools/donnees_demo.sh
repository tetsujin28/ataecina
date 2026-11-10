#!/bin/bash
# Prépare les cartes, les rushes et les outils de démonstration pour captures.py.
# Usage : sudo DEMO=/ bash docs/tools/donnees_demo.sh <chemin de l'exécutable compilé avec --features serveur-test>
set -euo pipefail
EXE="$1"; DEMO="${DEMO:-/}"; HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$DEMO/C:/Ataecina"; DD="$DEMO/D:"; WORK="$DEMO/ataecina-demo"; SRC="$WORK/rushes"
mkdir -p "$APP" "$DD/Images" "$DD/Recuperation" "$DD/References" "$DD/Rushes_a_reparer" "$WORK/outils" "$SRC"
cp "$EXE" "$APP/Ataecina"

# 8 rushes ProRes 422 HQ de démonstration
for i in 1 2 3 4 5 6 7 8; do
  d=$((4 + (i * 3) % 6)); n="A001_1110$((1400 + i * 7))_C00$i.mov"
  [ -f "$SRC/$n" ] || ffmpeg -loglevel error -y -f lavfi -i "testsrc2=size=1920x1080:rate=24:duration=$d" \
    -f lavfi -i "sine=frequency=$((300 + i * 40)):duration=$d" -c:v prores_ks -profile:v 3 -c:a pcm_s24le "$SRC/$n"
done

# E: carte FAT32 (rushes dans CLIP/) avec deux rushes supprimés, F: la même reformatée, G: secteur de démarrage effacé
cd "$DD/Images"
dd if=/dev/zero of=A001.img bs=1M count=1024 status=none
mformat -i A001.img -F -v A001 ::
mmd -i A001.img ::CLIP
for f in "$SRC"/*.mov; do mcopy -i A001.img "$f" ::CLIP/; done
mdel -i A001.img ::CLIP/A001_11101421_C003.mov ::CLIP/A001_11101442_C006.mov
cp A001.img A001_formatee.img && mformat -i A001_formatee.img -F -v A001 ::
cp A001.img A001_abimee.orig && dd if=/dev/zero of=A001_abimee.orig bs=512 count=1 conv=notrunc status=none
cd "$WORK" && ln -sf "$DD/Images/A001.img" "E:" && ln -sf "$DD/Images/A001_formatee.img" "F:" && ln -sf "$DD/Images/A001_abimee.img" "G:"

# Réparation : une référence saine, deux rushes coupés (sans index), un rush sain, un fichier illisible
cp "$SRC/A001_11101407_C001.mov" "$DD/References/"
cd "$DD/Rushes_a_reparer"
for c in A001_11101414_C002 A001_11101435_C005; do head -c $(( $(stat -c%s "$SRC/$c.mov") - 3000000 )) "$SRC/$c.mov" > "$c.mov"; done
cp "$SRC/A001_11101449_C007.mov" .
head -c 20000000 /dev/urandom > A001_11101463_C009.mov

# Outils copiés à côté de l'exe pendant la seconde phase des captures
cp "$HERE/untrunc_demo.sh" "$WORK/outils/untrunc" && chmod +x "$WORK/outils/untrunc"
printf '#!/bin/sh\necho "PhotoRec (démo)"\n' > "$WORK/outils/photorec" && chmod +x "$WORK/outils/photorec"
cp "$(command -v ffmpeg)" "$WORK/outils/ffmpeg"; cp "$(command -v ffprobe)" "$WORK/outils/ffprobe"
echo "Données de démonstration prêtes dans $DEMO"
