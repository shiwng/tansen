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
# Content identity of the upstream the shipped jar is built from. The tag
# name alone is mutable; whoever controls it could change shipped code
# without any Tansen-side commit. Both this script and CI pin the SHA and
# refuse to build from anything else. Keep in sync with
# .github/workflows/ci.yml.
BITCHORD_SHA="${BITCHORD_SHA:-7d956f211b8a46a87184fd3ff87432b51a111dfa}"
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

# --- provenance gate: the built jar is executable content shipped in the
# app; build only the audited commit, never "whatever the tag means now".
built="$(git -C "$UP" rev-parse HEAD)"
if [ "$built" != "$BITCHORD_SHA" ]; then
  echo "ERROR: upstream checkout $built is not the pinned commit $BITCHORD_SHA" >&2
  echo "(tag $REF may have moved; verify the new commit and update" >&2
  echo "BITCHORD_SHA here and in .github/workflows/ci.yml)" >&2
  exit 1
fi
echo "upstream provenance OK: $built"

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
# resolves from mavenLocal(). The tag is mutable, so every download is checked
# against a digest recorded in this script. Build-time only — nothing
# downloads at runtime.
M2="$HOME/.m2/repository/com/github/MetrolistGroup/innertubex/innertubex-desktop/v0.7.0"
mkdir -p "$M2"

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d' ' -f1
  else
    shasum -a 256 "$1" | cut -d' ' -f1
  fi
}

fetch_verified() {
  local file="$1" expected="$2"
  local dest="$M2/$file"
  local url="https://jitpack.io/com/github/MetrolistGroup/innertubex/innertubex-desktop/v0.7.0/$file"
  if [ ! -f "$dest" ] || [ "$(sha256_of "$dest")" != "$expected" ]; then
    rm -f "$dest"
    curl -sSL --retry 10 --retry-all-errors -C - -o "$dest" "$url"
    local actual
    actual="$(sha256_of "$dest")"
    if [ "$actual" != "$expected" ]; then
      echo "checksum mismatch for $file: expected $expected, got $actual" >&2
      rm -f "$dest"
      exit 1
    fi
  fi
  shasum -a 1 "$dest" | cut -d' ' -f1 > "$dest.sha1"
}

fetch_verified "innertubex-desktop-v0.7.0.jar" \
  "1b8195634a8a3ada50629f2584a35f82bdbc7041e5974f957b8c3132c435cc8f"
fetch_verified "innertubex-desktop-v0.7.0.pom" \
  "c716dc9c1014335c3f048263a497b762123959d35ed57f88b5b47b6c23fcd6f1"

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

# F7: bundle a trimmed jlink Java runtime next to the jar so the app runs
# on a clean macOS without system Java (8-module set verified by the
# sidecar smoke test: /health, /search, /track, /stream, /lyrics).
JLINK="$JAVA_HOME/bin/jlink"
if [ -x "$JLINK" ]; then
  rm -rf "$OUT/jre"
  "$JLINK" \
    --add-modules java.base,java.logging,java.xml,java.net.http,jdk.httpserver,jdk.unsupported,jdk.crypto.ec,jdk.dynalink \
    --no-man-pages --no-header-files --strip-debug --compress=2 \
    --output "$OUT/jre" >/dev/null
  # jlink marks module/lib files read-only; tauri's resource copier must be
  # able to overwrite them on rebuild.
  chmod -R u+w "$OUT/jre"
  echo "jre -> $OUT/jre ($(du -sh "$OUT/jre" | cut -f1))"
else
  echo "jlink not found in JAVA_HOME; skipping bundled JRE" >&2
fi
if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 "$OUT/sidecar-all.jar"
fi

# Supply chain: innertubex-desktop-v0.7.0 jar/pom are SHA-256-pinned above,
# and NewPipeExtractor is pinned to a commit hash in the overlay's
# build.gradle.kts. The .sha1 files written for Gradle are only mavenLocal
# naming metadata, never the integrity check.
