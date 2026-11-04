#!/bin/bash
# window-set.sh <pid> <x> <y> <w> <h> — position/size a window via AX (no app scripting).
set -u
PID="${1:?usage: window-set.sh pid x y w h}"; X=$2; Y=$3; W=$4; H=$5
osascript <<OSA
tell application "System Events" to tell (first process whose unix id is $PID)
  set position of window 1 to {$X, $Y}
  set size of window 1 to {$W, $H}
end tell
OSA
