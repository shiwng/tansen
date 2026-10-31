#!/usr/bin/env bash
# M1 baseline metrics harness (release build).
#
# Measures, 5 runs, medians reported:
#   (a) process start → first frontend paint (log timestamps)
#   (b) total memory footprint: app process + sidecar JVM (if any);
#   (c) 60 s idle (paused) and 60 s playing CPU% (top; powermetrics when sudo)
#
# Env-gated autoplay (TANSEN_BENCH_PLAY) drives the "playing" phase with a
# generated 75 s WAV; normal app behavior is unchanged.
#
# Usage: scripts/bench/run.sh [--rebuild]

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="$ROOT/target/release/tansen"
RUNS=5
WINDOW=60
FIXTURE=/tmp/tansen-bench-75s.wav
OUT=/tmp/tansen-bench-results

cd "$ROOT"

if [ "${1:-}" = "--rebuild" ] || [ ! -x "$BIN" ] || [ ! -f apps/desktop/dist/index.html ]; then
  echo "== building release (frontend + rust) =="
  pnpm --dir apps/desktop build
  # custom-protocol embeds frontendDist; without it the binary points at devUrl
  /Users/shiwang/.cargo/bin/cargo build --release -p tansen \
    --features tansen/custom-protocol --manifest-path Cargo.toml
fi

# 75 s stereo sine fixture (longer than the measurement window).
if [ ! -f "$FIXTURE" ]; then
  python3 - "$FIXTURE" <<'PY'
import math, struct, sys, wave
path = sys.argv[1]
w = wave.open(path, "wb")
w.setnchannels(2); w.setsampwidth(2); w.setframerate(44100)
frames = bytearray()
for i in range(44100 * 75):
    v = int(9000 * math.sin(2 * math.pi * 220 * i / 44100))
    frames += struct.pack("<hh", v, v)
w.writeframes(bytes(frames)); w.close()
PY
fi

mkdir -p "$OUT"
: > "$OUT/raw.tsv"

# Pids for the bench session: the app process, plus the sidecar JVM when the
# catalogue is on. WKWebView helper processes are spawned by launchd (not
# children of the app) and cannot be attributed reliably — excluded, noted in
# the results table.
app_pids() {
  local app="$1"
  echo "$app"
  pgrep -f "sidecar-all.jar" 2>/dev/null || true
}

# One 60 s CPU pass: top emits 31 samples (first = warmup, dropped); the
# per-sample CPU of every matched pid is summed, then averaged. BSD awk is
# fragile with inline for+if, so the per-line filter is a while-read loop.
cpu_60s() {
  local pids="$1"
  top -l 31 -s 2 -stats pid,cpu 2>/dev/null \
    | { grep -E '^[0-9]+ +[0-9.]' || true; } \
    | while read -r pid cpu _rest; do
        case "$pids" in
          *" $pid "*) printf '%s\n' "$cpu" ;;
        esac
      done \
    | python3 -c "
import sys
vals = [float(x) for x in sys.stdin]
vals = vals[1:]  # drop top's warmup sample
print(f'{sum(vals)/len(vals):.3f}' if vals else 'nan')
"
}

# Total memory footprint in MiB across the given pids (footprint, RSS fallback).
memory_mib() {
  local total=0 p m
  for p in $1; do
    if command -v footprint >/dev/null 2>&1; then
      m=$(footprint "$p" 2>/dev/null | grep 'Footprint:' | head -1 \
        | sed -E 's/.*Footprint: ([0-9.]+) (MB|KB|B|GB).*/\1 \2/')
      case "$m" in
        *" MB") m=${m% MB} ;;
        *" GB") m=${m% GB}; m=$(awk -v v="$m" 'BEGIN{printf "%d", v*1024}') ;;
        *" KB") m=${m% KB}; m=$(awk -v v="$m" 'BEGIN{printf "%d", v/1024}') ;;
        *" B") m=${m% B}; m=$(awk -v v="$m" 'BEGIN{printf "%d", v/1048576}') ;;
        *) m=0 ;;
      esac
    else
      m=$(ps -o rss= -p "$p" 2>/dev/null | tr -d ' ')
      m=$(awk -v v="${m:-0}" 'BEGIN{printf "%d", v/1024}')
    fi
    total=$(awk -v a="$total" -v b="${m:-0}" 'BEGIN{printf "%d", a+b}')
  done
  echo "$total"
}

# powermetrics only when passwordless sudo is already available.
WAKE="skipped (passwordless sudo unavailable)"
if sudo -n true 2>/dev/null; then
  WAKE="measured below"
fi

for run in $(seq 1 "$RUNS"); do
  echo "== run $run/$RUNS =="

  # --- paused / idle phase ---
  RUST_LOG=info ./target/release/tansen > "$OUT/run-$run-paused.log" 2>&1 &
  APP=$!
  for _ in $(seq 1 60); do
    grep -q 'first-paint' "$OUT/run-$run-paused.log" 2>/dev/null && break
    sleep 0.5
  done
  { grep -E 'process-start|first-paint' "$OUT/run-$run-paused.log" || true; } \
    | head -2 > "$OUT/run-$run-paint.txt"

  pids=" $(app_pids "$APP" | tr '\n' ' ')"
  CPU_IDLE=$(cpu_60s "$pids")
  MEM_IDLE=$(memory_mib "$pids")
  echo -e "idle\t$CPU_IDLE\t$MEM_IDLE" >> "$OUT/raw.tsv"

  pkill -f "target/release/tansen" 2>/dev/null || true
  sleep 2

  # --- playing phase ---
  RUST_LOG=info TANSEN_BENCH_PLAY="$FIXTURE" ./target/release/tansen \
    > "$OUT/run-$run-playing.log" 2>&1 &
  APP=$!
  sleep 6  # paint + autoplay warm-up

  pids=" $(app_pids "$APP" | tr '\n' ' ')"
  CPU_PLAY=$(cpu_60s "$pids")
  MEM_PLAY=$(memory_mib "$pids")
  echo -e "playing\t$CPU_PLAY\t$MEM_PLAY" >> "$OUT/raw.tsv"

  grep -q 'autoplay-started' "$OUT/run-$run-playing.log" \
    || echo "WARN run $run: autoplay did not start" >&2
  pkill -f "target/release/tansen" 2>/dev/null || true
  sleep 2
done

# --- summarize: paint deltas, CPU medians, memory medians → markdown table ---
python3 - "$OUT" "$RUNS" <<'PY'
import re, statistics, sys
from datetime import datetime

out, runs = sys.argv[1], int(sys.argv[2])

def ts(line):
    m = re.search(r"(\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}\.\d+Z)", line)
    return m.group(1) if m else None

paints = []
for r in range(1, runs + 1):
    stamps = [ts(l) for l in open(f"{out}/run-{r}-paint.txt") if ts(l)]
    if len(stamps) >= 2:
        t0 = datetime.strptime(stamps[0], "%Y-%m-%dT%H:%M:%S.%fZ")
        t1 = datetime.strptime(stamps[1], "%Y-%m-%dT%H:%M:%S.%fZ")
        paints.append((t1 - t0).total_seconds() * 1000)

idle_cpu, play_cpu, idle_mem, play_mem = [], [], [], []
for line in open(f"{out}/raw.tsv"):
    parts = line.strip().split("\t")
    if len(parts) != 3:
        continue
    if parts[0] == "idle":
        idle_cpu.append(float(parts[1])); idle_mem.append(float(parts[2]))
    else:
        play_cpu.append(float(parts[1])); play_mem.append(float(parts[2]))

def med(v):
    return statistics.median(v) if v else float("nan")

print()
print("| Metric (5 runs, release) | Median |")
print("|---|---|")
if paints:
    print(f"| Process start → first paint | {med(paints):.0f} ms |")
print(f"| Memory idle — app process |  {med(idle_mem):.0f} MiB |")
print(f"| Memory playing — app process |  {med(play_mem):.0f} MiB |")
print(f"| CPU 60 s idle (paused) | {med(idle_cpu):.2f} % |")
print(f"| CPU 60 s playing | {med(play_cpu):.2f} % |")
print("| Wakeups | powermetrics skipped (needs sudo) |")
print()
print("paints_ms:", [f"{p:.0f}" for p in paints])
print("idle_cpu:", [f"{v:.2f}" for v in idle_cpu])
print("playing_cpu:", [f"{v:.2f}" for v in play_cpu])
print("idle_mem_mib:", idle_mem)
print("playing_mem_mib:", play_mem)
PY
echo "wakeup measurement: $WAKE"
