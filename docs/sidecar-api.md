# Sidecar API contract

The Kotlin sidecar (upstream `BitChord/sidecar/`) is a local HTTP server over
`:shared`'s YouTube Music client. The Rust `YtMusicSource`
(`crates/sources/src/yt.rs`) spawns it, health-checks it, and translates its
JSON. All traffic is `127.0.0.1` only; no cookies, anonymous Innertube access.

## Launch

```
java -jar sidecar-all.jar --port <free port>
```

- Port: `--port` (either `--port=N` or `--port N`), else env
  `BITCHORD_SIDECAR_PORT`, else 8765.
- The Rust side finds a free port itself and always passes `--port`.
- `GET /health` is polled (400 ms interval, 20 s budget) until `{ok: true}`.
- SIGTERM → clean exit 0. The Rust supervisor also kills the child on drop.

Discovery on the Rust side (first hit wins):

| What | Order |
|---|---|
| Jar | `BITCHORD_SIDECAR_JAR` env → `sidecar_jar` setting → `<exe>/../BitChord/sidecar/build/libs/sidecar-all.jar` → `<cwd>/../BitChord/...` |
| Java | `java_path` setting → `$JAVA_HOME/bin/java` → `java` on PATH |

The source participates in playback only when the `catalogue_sources` setting
is `"true"` (local sources stay the default); see `registry::build_active`.

## Endpoints

Errors are JSON `{"error": "..."}` with status 400 (bad request), 404 (not
found / no lyrics), 502 (upstream failure), 500 (unexpected). Every request
logs one line to stdout (`[sidecar] OK 200 GET /search in 132ms`).

### `GET /health`

```json
{ "ok": true, "version": "1.8.1-sidecar.1", "uptimeMs": 1234 }
```

Rust: `SourceHealth::Ok { detail: version }`; connect failure → Unreachable.

### `GET /search?q=<query>&filter=all|songs|albums|artists|playlists|videos`

```json
{
  "tracks": [
    { "videoId": "…", "title": "…", "artist": "…",
      "artists": [{ "name": "…", "browseId": "…" }],
      "album": "…", "albumId": "…",
      "durationText": "4:41", "durationSec": 281,
      "thumbnailUrl": "https://…", "isVideo": false, "isExplicit": false }
  ],
  "browse": [
    { "kind": "album|artist|playlist", "browseId": "…", "title": "…",
      "subtitle": "…", "thumbnailUrl": "https://…" }
  ]
}
```

Default filter is `all`. Rust maps `tracks` → `CatalogTrack` (`track_id` =
`videoId`, `config_id` = `"yt"`, `format: None` — the row does not grade
itself; the stream answer does). `browse` rows are not consumed yet; they
feed album/artist pages later.

### `GET /track/{videoId}`

One track's metadata (same shape as a `tracks[]` row), resolved through the
watch-queue endpoint. 404 when YouTube has no watch entry for the id.

### `GET /track/{videoId}/stream[?prefer=aac]`

```json
{ "url": "https://…googlevideo.com/…", "format": "audio/mp4; codecs=\"mp4a.40.2\"",
  "container": "mp4", "itag": 140, "kbps": 128, "loudnessDb": -1.2,
  "expiresAt": 1730000000000 }
```

- Without `prefer`: whatever `StreamResolver.resolve` picks (any container —
  often Opus/WebM, which this desktop build cannot decode; recorded gap).
- With `prefer=aac`: `StreamResolver.resolveForDownload(requireM4a = true)` —
  a guaranteed progressive MP4/AAC rendition, the container Symphonia decodes
  directly. **This is what the Rust source requests.**
- `itag` is parsed from the googlevideo URL; `format` comes from the
  resolver, `container`/mime from a zero-byte ranged GET when the resolver
  did not state one. `expiresAt` is `now + 6 h` (googlevideo URLs expire in
  about six hours).
- Progressive itags only. HLS/DASH manifests are out of scope for the
  sidecar — recorded as a gap in `docs/feature-matrix.md`.

Rust maps this to `SourceStream { format: StreamFormat { codec: "AAC",
is_lossless: false, kbps }, .. }`.

### `GET /album/{browseId}` · `GET /playlist/{id}`

```json
{ "title": "…", "subtitle": "Album • Artist • 2024", "thumbnailUrl": "https://…",
  "description": "…", "continuation": "token-or-null",
  "tracks": [ …same shape as search tracks… ] }
```

Album ids are `MPREb…`; playlist ids accept bare `PL…`/`OLAK5uy…` (a `VL`
prefix is added when missing). Continuations are exposed but not followed by
the Rust side yet.

### `GET /lyrics/track/{videoId}`

Runs the full provider race (`LyricsRepository`) against the track's own
metadata.

```json
{ "source": "LRCLIB", "synced": true,
  "lines": [ { "timeMs": 0, "text": "…" }, { "timeMs": 14210, "text": "…" } ] }
```

- 404 + `{"error": …}` when the track metadata or lyrics cannot be found.
- `synced` is a summary flag: it is true when any line carries `timeMs > 0`.
  Word-level timings are not in this payload yet (the upstream
  `LyricLine` model carries them; the sidecar exports line-level only —
  recorded gap).
- Plain-text lyrics come back as lines with `timeMs: 0`.

## What the sidecar does not do (recorded gaps)

- No account/cookie endpoints (no home feed, library, likes, playlists CRUD).
- No HLS/DASH — progressive streams only.
- No word-level lyric timings over the wire yet.
- PoToken/BotGuard: the sidecar runs `StreamResolver` exactly as the phone
  does, with InnerTubeX's client catalog. Clients that require a web
  BotGuard token fail resolution on desktop the same way they would without
  a WebView minter; `resolveForDownload(requireM4a)` picks from the clients
  that still answer. This is observed behavior, not a guarantee — the
  round-trip test is the evidence.
