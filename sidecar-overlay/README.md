# Sidecar overlay

Source of truth for the Kotlin sidecar module (`sidecar/`) plus the two-line
patch applied to upstream's `settings.gradle.kts` (module include + a
`mavenLocal()` repository that holds artifacts JitPack's transfer keeps
truncating).

The module builds against upstream's `:shared`, so it cannot be a standalone
Gradle build. `scripts/build-sidecar.sh` is the only supported build path:

1. Ensures an upstream BitChord checkout at the pinned tag
   (`BITCHORD_REF`, default `v1.8.1`) exists — cloned fresh or reusing a clean
   local checkout at `BITCHORD_DIR`.
2. Copies this overlay into the checkout (never the reverse).
3. Runs `:sidecar:fatJar` and copies `sidecar-all.jar` into
   `apps/desktop/src-tauri/resources/`, which Tauri packages as a bundle
   resource.

GPL-3.0-or-later, like upstream. See `../docs/sidecar-api.md` for the HTTP
contract and `../IMPLEMENTATION.md` §6 for how the app consumes it.
