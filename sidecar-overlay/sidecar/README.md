# Tansen sidecar module

<!--
SPDX-License-Identifier: GPL-3.0-or-later
A thin HTTP wrapper over the pinned upstream shared module's YouTube Music
client. It changes nothing in that module; every seam lives here.
-->

A local HTTP server that exposes the shared module's catalogue and stream
resolution to the Rust desktop app (`crates/sources`). It exists so the
working Kotlin YouTube Music client does not have to be rewritten in Rust.

- Binds **127.0.0.1 only**; it is a private helper for the app on the same machine.
- No account/cookie: anonymous Innertube access only. Personalized endpoints are
  out of scope for the sidecar.
- progressive audio streams only. HLS/DASH manifests are not served; see the
  desktop repo's `docs/feature-matrix.md` for the recorded gap.

## Build

```bash
JAVA_HOME=/path/to/jdk-17 ./gradlew :sidecar:fatJar
# -> sidecar/build/libs/sidecar-all.jar
```

## Run

```bash
java -jar sidecar/build/libs/sidecar-all.jar --port 8765
# or: TANSEN_SIDECAR_PORT=8765 java -jar ...
```

The Rust side spawns and supervises this process itself (`--port` with a free
port, health-checked, killed on drop); running it by hand is for debugging.

## Endpoints

See the desktop repo's `docs/sidecar-api.md` for the full contract and the
Rust field mappings. Short form:

| Route | Meaning |
|---|---|
| `GET /health` | liveness + version |
| `GET /search?q=&filter=all\|songs\|albums\|artists\|playlists\|videos` | mixed rows |
| `GET /track/{videoId}` | one track's metadata |
| `GET /track/{videoId}/stream[?prefer=aac]` | progressive stream URL (+format/itag/kbps) |
| `GET /album/{browseId}`, `GET /playlist/{id}` | track lists |
| `GET /lyrics/track/{videoId}` | provider-raced lyrics, or 404 |

## License

GPL-3.0-or-later. Everything this module calls is part of the same GPL
codebase; provenance and the pinned commit live in `scripts/build-sidecar.sh`.
