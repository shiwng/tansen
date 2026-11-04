#!/bin/bash
# Batch gate: the release bundle must contain no Design Lab code.
# Usage: scripts/ui-compare/check-release-lab.sh [dist-dir]
set -u
DIST="${1:-apps/desktop/dist}"
[ -d "$DIST" ] || { echo "no dist at $DIST — run pnpm build first"; exit 2; }
if grep -rq "data-glisse-backdrop\|__lab" "$DIST"; then
  echo "FAIL: lab markers found in $DIST:"
  grep -rl "data-glisse-backdrop\|__lab" "$DIST"
  exit 1
fi
echo "OK: no lab code in $DIST"
