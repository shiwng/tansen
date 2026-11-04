#!/bin/bash
# Click at a window-relative point, so a drifting window position can't skew it.
# Usage: ui-click.sh <relx> <rely> [owner]   (default owner: Music)
# Activates the app first and clicks twice: on a background window the first
# click only brings it forward, the second one reaches the control.
set -euo pipefail
here=$(cd "$(dirname "$0")" && pwd)
relx=$1; rely=$2; owner=${3:-Music}
read -r id x y w h _rest < <(swift "$here/find-window.swift" "$owner")
[ -n "${id:-}" ] || { echo "no window for $owner" >&2; exit 1; }
open -a "$owner" 2>/dev/null || true
sleep 0.4
swift "$here/input.swift" click $((x + relx)) $((y + rely))
sleep 0.25
swift "$here/input.swift" click $((x + relx)) $((y + rely))
