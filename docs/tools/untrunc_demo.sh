#!/bin/bash
# Remplaçant d'untrunc pour les captures du mode d'emploi uniquement. Jamais livré.
# Il « répare » en recopiant le début de la référence, à la longueur du fichier cassé.
[ "$1" = "-s" ] && shift
export PATH="$(dirname "$0"):/usr/local/bin:/usr/bin:/bin"
REF="$1"; BAD="$2"
case "$BAD" in *C009*) echo "failed: no mdat found"; exit 1;; esac
dir=$(dirname "$BAD"); base=$(basename "$BAD"); base="${base%.*}"
rs=$(stat -c%s "$REF"); bs=$(stat -c%s "$BAD")
d=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "$REF")
t=$(python3 -c "print(round($d*$bs/$rs,2))")
ffmpeg -loglevel error -y -i "$REF" -t "$t" -c copy "$dir/${base}.mov_fixed.mov"
echo "Info: version 'v403-stub'"; echo "Found $t s"
