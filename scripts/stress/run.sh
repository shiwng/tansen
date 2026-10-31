#!/usr/bin/env bash
# M4 audio-reliability stress (debug build): playback for N seconds while the
# stress driver scans the library and repeats queries/searches every 2 s.
#
# Reports from the app log: cpal stream-error count, decoder read histogram
# (p50/p99 quantized to buckets), slowest read, stress round counts.
#
# Usage: scripts/stress/run.sh [seconds]   (default 300 = 5 minutes)
#
# Optional env:
#   TANSEN_STALL_MS / TANSEN_STALL_EVERY — mid-track read stall hook (debug
#   builds): sleep TANSEN_STALL_MS per every TANSEN_STALL_EVERY-th sample.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
DUR="${1:-300}"

BIN=./target/debug/tansen
if [ ! -x "$BIN" ]; then
  echo "== building debug app (embedded frontend; debug keeps counters on) =="
  /Users/shiwang/.cargo/bin/cargo build -p tansen --features tansen/custom-protocol
fi

# Fixture library: 30 short stereo WAVs (the scan/query target).
LIB=/tmp/tansen-stress-lib
mkdir -p "$LIB"
COUNT=$(ls "$LIB" 2>/dev/null | wc -l | tr -d ' ')
if [ "$COUNT" -lt 30 ]; then
  python3 - "$LIB" <<'PY'
import math, os, struct, sys, wave
lib = sys.argv[1]
for n in range(30):
    p = os.path.join(lib, f"stress-{n:02}.wav")
    if os.path.exists(p):
        continue
    w = wave.open(p, "wb")
    w.setnchannels(2); w.setsampwidth(2); w.setframerate(44100)
    frames = bytearray()
    for i in range(44100 * 3):
        v = int(9000 * math.sin(2 * math.pi * (200 + 40 * n) * i / 44100))
        frames += struct.pack("<hh", v, v)
    w.writeframes(bytes(frames)); w.close()
PY
fi

# Playback fixture: one long WAV covering the whole run (the bench hook plays
# the file once; the queue stops when it ends).
PLAY=/tmp/tansen-stress-play.wav
if [ ! -f "$PLAY" ]; then
  python3 - "$PLAY" "$DUR" <<'PY'
import math, struct, sys, wave
path, secs = sys.argv[1], int(sys.argv[2])
w = wave.open(path, "wb")
w.setnchannels(2); w.setsampwidth(2); w.setframerate(44100)
frames = bytearray()
for i in range(44100 * (secs + 20)):
    v = int(9000 * math.sin(2 * math.pi * 220 * i / 44100))
    frames += struct.pack("<hh", v, v)
w.writeframes(bytes(frames)); w.close()
PY
fi

OUT=/tmp/tansen-stress.log
: > "$OUT"
echo "== stress run: ${DUR}s (lib=$LIB) =="
TANSEN_STRESS=1 TANSEN_STRESS_LIB="$LIB" TANSEN_BENCH_PLAY="$PLAY" RUST_LOG=info \
  "$BIN" > "$OUT" 2>&1 &
APP=$!
sleep "$DUR"
kill "$APP" 2>/dev/null || true
sleep 1

echo "== audio reliability (final windows) =="
grep -a 'audio-metrics' "$OUT" | tail -3
echo
echo "== stress driver =="
echo "scans:       $(grep -ac 'scan started' "$OUT" || true)"
echo "autoplays:   $(grep -ac 'autoplay-started' "$OUT" || true)"
echo
echo "== cpal stream errors (rodio default log line, if any) =="
grep -a 'audio stream error' "$OUT" | head -5
echo "(end)"
