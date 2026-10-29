#!/usr/bin/env bash
# Build the Kotlin sidecar jar from pinned upstream sources + Tansen's overlay.
#
# Output: apps/desktop/src-tauri/resources/sidecar-all.jar (a Tauri bundle
# resource, shipped inside the packaged app).
#
# Env:
#   BITCHORD_REF   upstream tag/commit to build against (default: v1.8.1)
#   BITCHORD_DIR   where the upstream checkout lives / should be cloned
#                  (default: <repo>/../BitChord, the documented dev layout)
#   JAVA_HOME      JDK 17+ home (required by Gradle)
#
# SPDX-License-Identifier: GPL-3.0-or-later

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
REF="${BITCHORD_REF:-v1.8.1}"
UP="${BITCHORD_DIR:-$(cd "$REPO_ROOT/.." && pwd)/BitChord}"
OVERLAY="$REPO_ROOT/sidecar-overlay"
OUT="$REPO_ROOT/apps/desktop/src-tauri/resources"

if [ ! -d "$OVERLAY/sidecar" ]; then
  echo "overlay missing: $OVERLAY/sidecar" >&2
  exit 1
fi

# --- upstream checkout at the pinned ref -------------------------------------
if [ ! -d "$UP/.git" ]; then
  echo "cloning upstream $REF into $UP"
  git clone --depth 1 --branch "$REF" https://github.com/kushagrasinghx/BitChord "$UP"
else
  # A dirty or custom checkout is the developer's live tree: never clobber it.
  if [ -n "$(git -C "$UP" status --porcelain)" ]; then
    echo "ERROR: $UP has uncommitted changes; commit/stash them or point" >&2
    echo "BITCHORD_DIR at a clean checkout." >&2
    exit 1
  fi
  current="$(git -C "$UP" rev-parse HEAD)"
  target="$(git -C "$UP" rev-parse "$REF^{commit}" 2>/dev/null || echo "")"
  if [ -z "$target" ]; then
    git -C "$UP" fetch --depth 1 origin tag "$REF"
    target="$(git -C "$UP" rev-parse "$REF^{commit}")"
  fi
  if [ "$current" != "$target" ]; then
    echo "checking out $REF in $UP (from $current)"
    git -C "$UP" checkout --quiet "$REF"
  fi
fi

# --- overlay: module sources + settings patch (idempotent) --------------------
rm -rf "$UP/sidecar"
cp -R "$OVERLAY/sidecar" "$UP/sidecar"

SETTINGS="$UP/settings.gradle.kts"
grep -q 'include(":sidecar")' "$SETTINGS" || \
  python3 - "$SETTINGS" <<'PY'
import sys
path = sys.argv[1]
s = open(path).read()
s = s.replace('include(":sharedUi")', 'include(":sharedUi")\ninclude(":sidecar")', 1)
open(path, "w").write(s)
PY
grep -q 'mavenLocal()' "$SETTINGS" || \
  python3 - "$SETTINGS" <<'PY'
import sys
path = sys.argv[1]
s = open(path).read()
s = s.replace("""    repositories {
        google()""", """    repositories {
        // Local Maven holds artifacts JitPack's transfer keeps truncating
        // (innertubex-desktop); populated below by this script.
        mavenLocal()
        google()""", 1)
open(path, "w").write(s)
PY

# JitPack's transfer of innertubex-desktop-v0.7.0 truncates repeatedly
# (observed three times at ~1 MB of 1.9 MB), so Gradle's own download is not
# reliable. Pre-seed ~/.m2 with resumed, retried curl fetches; Gradle then
# resolves from mavenLocal(). Build-time only — nothing downloads at runtime.
M2="$HOME/.m2/repository/com/github/MetrolistGroup/innertubex/innertubex-desktop/v0.7.0"
mkdir -p "$M2"
for f in innertubex-desktop-v0.7.0.jar innertubex-desktop-v0.7.0.pom; do
  if [ ! -f "$M2/$f" ] || [ ! -f "$M2/$f.sha1" ]; then
    curl -sSL --retry 10 --retry-all-errors -C - -o "$M2/$f" \
      "https://jitpack.io/com/github/MetrolistGroup/innertubex/innertubex-desktop/v0.7.0/$f"
    shasum -a 1 "$M2/$f" | cut -d' ' -f1 > "$M2/$f.sha1"
  fi
done

# --- build ---------------------------------------------------------------------
if [ -z "${JAVA_HOME:-}" ] && [ -x "/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home/bin/java" ]; then
  export JAVA_HOME="/opt/homebrew/opt/openjdk@17/libexec/openjdk.jdk/Contents/Home"
fi
if [ -z "${JAVA_HOME:-}" ]; then
  echo "JAVA_HOME not set and no default JDK 17 found" >&2
  exit 1
fi
export ANDROID_HOME="${ANDROID_HOME:-$HOME/Library/Android/sdk}"

(cd "$UP" && ./gradlew :sidecar:fatJar --no-daemon -q)

mkdir -p "$OUT"
JAR="$UP/sidecar/build/libs/sidecar-all.jar"
cp "$JAR" "$OUT/sidecar-all.jar"
echo "sidecar jar -> $OUT/sidecar-all.jar"
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$OUT/sidecar-all.jar"
fi
