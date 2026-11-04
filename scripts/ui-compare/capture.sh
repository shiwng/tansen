#!/bin/bash
# capture.sh <out.png> [owner-substring] — captures only that app's front window.
set -u
OUT="${1:?usage: capture.sh <out.png> [owner]}"
OWNER="${2:-Music}"
LINE=$(swift "$(dirname "$0")/find-window.swift" "$OWNER" | head -1)
[ -z "$LINE" ] && { echo "no window for $OWNER"; exit 1; }
ID=$(echo "$LINE" | awk '{print $1}')
# settle: two consecutive identical captures (10 fps max), then keep the frame
prev=""
for _ in 1 2 3 4 5 6 7 8; do
  screencapture -x -o -l"$ID" "$OUT" || exit 1
  cur=$(shasum "$OUT" | cut -d" " -f1)
  [ "$cur" = "$prev" ] && break
  prev="$cur"; sleep 0.3
done
echo "$ID $(sips -g pixelWidth -g pixelHeight "$OUT" | tail -2 | awk '{print $2}' | tr '\n' 'x' | sed 's/x$//') $OUT"
