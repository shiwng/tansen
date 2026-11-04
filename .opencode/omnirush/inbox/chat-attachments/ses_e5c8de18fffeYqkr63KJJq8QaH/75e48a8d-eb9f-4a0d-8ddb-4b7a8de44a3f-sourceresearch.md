# BitChord — Music Source Investigation (ASDST100)

**Repository:** <https://github.com/kushagrasinghx/BitChord>
**Revision examined:** `b690dc146009d4815825fa4a20815ef2e4ee8898` (merge of `v1.8.2`, Fri Oct 30 2026)
**Method:** static source-code analysis only. No app was built or run, no network calls were made to any provider, and no BitChord or Tansen file was modified.
**Report scope:** where music actually comes from, where metadata comes from, how a track becomes a playable stream, how streaming/playback works, what formats/quality are real, and how the multi-provider system fits together.

---

## 1. Executive answer

| Question | Verified answer |
|---|---|
| **Actual music source** | **YouTube Music / YouTube audio**, served from Google's `googlevideo.com` CDN. BitChord hosts no music and bundles no catalogue. |
| **Metadata & search source** | **YouTube Music's private Innertube `WEB_REMIX` API** at `https://music.youtube.com/youtubei/v1` (search, browse, home, library, radio, lyrics keys). |
| **Audio delivery mechanism** | At play time BitChord resolves a video ID to a short-lived, signed `googlevideo.com` URL, then Media3/ExoPlayer fetches it over HTTP in bounded byte ranges through `ResolvingDataSource` → `ChunkedDataSource` → `OkHttpDataSource` (optionally through a disk cache). |
| **Bundled alternative providers** | Only **JioSaavn**, and it is **disabled by default**. |
| **Lossless** | **Not bundled.** FLAC/ALAC/Hi-Res/Atmos can only arrive from a **user-configured HTTP addon** or a **legacy JS module index**. YouTube itself is lossy Opus (~171 kbps ceiling). |
| **Backend (`backend/`)** | A Go **"Listen Together" party/WebSocket server**. It carries no audio and is unrelated to sourcing. |
| **Local audio** | Device storage (`MediaStore`), SMB and WebDAV libraries are separate *library* paths for the user's own files, not streaming catalogue providers. |

In one sentence: **BitChord is a YouTube Music client with a pluggable source layer; the music is YouTube's, the catalogue metadata is YouTube Music's Innertube API, and everything else (JioSaavn, addons, modules) is optional and user-supplied.**

---

## 2. Architecture / data-flow diagram

```
┌───────────────────────────────────────────────────────────────────────────┐
│ UI / ViewModel                                                            │
│  MainViewModel.runSearch()            MainViewModel.runAddonSearch()      │
│  (YouTube path, line 2276)            (addon-only tab, line 2167)         │
│  MainViewModel.sourceResults()        (non-YouTube fan-out, line 2487)    │
└───────────────┬───────────────────────────────┬───────────────────────────┘
                │                               │
                ▼                               ▼
┌───────────────────────────────┐   ┌───────────────────────────────────────┐
│ YtMusicRepository             │   │ SourceRegistry.activeForPlayback()    │
│  searchPage() line 334        │   │  ADDON(0) / CUSTOM_MODULE(0)          │
│        │                      │   │  MODULE(1) / JIOSAAVN(2) / YOUTUBE(3) │
│        ▼                      │   └───────────────┬───────────────────────┘
│ Innertube (WEB_REMIX)         │                   │
│  POST music.youtube.com/      │        ┌──────────┴──────────┐
│  youtubei/v1/search           │        ▼                     ▼
└───────────────┬───────────────┘   AddonSource          JioSaavnSource
                │                   ModuleSource         (opt-in, 320 kbps)
                ▼                   (QuickJS, legacy)
        SearchResult rows
                │
                ▼
┌───────────────────────────────────────────────────────────────────────────┐
│ Track selection → Song                                                    │
│  YouTube tracks keep a bare video ID.                                     │
│  Source-backed tracks become bitchord://source?s=<config>&t=<trackId>     │
│  (SourceRegistry.trackUri / parseTrackKey)                                │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌───────────────────────────────────────────────────────────────────────────┐
│ Song.toMediaItem()  (PlayerConnection.kt:647)                             │
│  bitchord://watch?v=<id>&n=&a=&d=…      (YouTube / substitutable)         │
│  bitchord://source?s=…&t=…&n=&a=&d=…    (pinned provider row)             │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌───────────────────────────────────────────────────────────────────────────┐
│ PlaybackService — ResolvingDataSource.Resolver  (line 1394)               │
│                                                                           │
│  authority == "source" ──► SourceResolver.resolve(uri)  (line 107/196)    │
│                              │ pinned source → lossless upgrade →         │
│                              │ fallback bestAcross()                      │
│                              ▼                                            │
│  authority == "watch"  ──► StreamChoice pin? → QualityUpgrade? →          │
│                            substituteForYouTube() race →                  │
│                            StreamResolver.resolve(videoId)                │
│                              │ InnerTubeX (first)                         │
│                              │   └─► NewPipe extractor (fallback)         │
│                              │   └─► probe URL, cache 15 min, 403 logic   │
│                              ▼                                            │
│                         real https://…googlevideo.com/… URL               │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌───────────────────────────────────────────────────────────────────────────┐
│ Transport chain (built at PlaybackService.kt:1718)                        │
│  ResolvingDataSource.Factory                                              │
│    └─ ChunkedDataSource.Factory(OkHttpDataSource.Factory(Http.client))    │
│         └─ bounded 1 MiB ranges (avoids googlevideo pacing)               │
│  AudioCache.playbackFactory() wraps it for read-ahead / disk cache        │
└───────────────┬───────────────────────────────────────────────────────────┘
                ▼
┌───────────────────────────────────────────────────────────────────────────┐
│ Media3 ExoPlayer 1.11.0                                                   │
│  DefaultMediaSourceFactory → progressive / HLS / DASH                     │
│  Renderers: platform MediaCodec + vendored FFmpeg + custom DSD extractor  │
│  Audio sink: DefaultAudioSink / bit-perfect USB path                      │
└───────────────────────────────────────────────────────────────────────────┘
```

---

## 3. The source abstraction

### 3.1 `MusicSource` — the only contract

`app/src/main/java/com/music/bitchord/data/sources/MusicSource.kt`

- `SourceStream` (lines 14–55): `url`, `format: StreamFormat`, `headers`, `belowRequest`, `sourceConfigId`.
- `StreamRequest` (lines 66–75): three tiers — `Lossless`, `Capped(maxKbps)`, `Best`.
- `MusicSource` (lines 95–157): `health()`, `search(query, limit, waitForAll, request)`, `stream(trackId, request)`.

The interface is deliberately narrow. Its own doc says a source is **not** asked for home feeds, related tracks or radio; those stay with YouTube, the only source that has them (lines 98–102).

### 3.2 `SourceKind` — fixed protocol list and ranking

`app/src/main/java/com/music/bitchord/data/sources/SourceKind.kt`

| Kind | Rank | Lossless capable | Prefetch | Bundled? | Evidence |
|---|---:|---|---|---|---|
| `ADDON` | 0 | yes | no | no (user-added) | lines 72–82 |
| `CUSTOM_MODULE` | 0 | yes | no | no (legacy user-added) | lines 92–101 |
| `MODULE` | 1 | yes | no | no (legacy, retired) | lines 120–128 |
| `JIOSAAVN` | 2 | no | yes | yes, **off by default** | lines 130–138 |
| `YOUTUBE` | 3 | no | no | yes, **always on** | lines 146–154 |

Rank is the walk order. `SourceRegistry.active()` sorts on it, so user-added sources are asked first, then the legacy module, then JioSaavn, then YouTube.

The file explicitly says the list is fixed and small on purpose: every protocol the app speaks is one that can be read in the repo, and **a source cannot teach the app a new way to behave after it ships** (lines 6–12).

### 3.3 `SourceRegistry` — seeding and identity

`app/src/main/java/com/music/bitchord/data/sources/SourceRegistry.kt`

- `sourcesForInit()` (lines 136–158) seeds built-ins and applies one-time migrations.
- `BUILT_IN_KINDS = listOf(SourceKind.JIOSAAVN, SourceKind.YOUTUBE)` (line 549).
- Any legacy `SourceKind.MODULE` config is filtered out (line 150).
- JioSaavn is forced off once via `KEY_JIOSAAVN_OPT_IN_V1` (lines 154, 552–553); YouTube is forced on (line 153).
- `SourceConfig` default policy is documented as **"JioSaavn off, YouTube on"** (lines 141–142).

**Verified consequence:** a fresh install has exactly two source entries — YouTube (enabled) and JioSaavn (disabled). **No addon or module index ships with the build.** Lossless is therefore impossible until the user adds a source.

Track identity:

- YouTube tracks keep a bare video ID (`YouTubeSource.kt` lines 18–23).
- Source-backed tracks use `SourceRegistry.trackKey` / `parseTrackKey` and `trackUri`, producing `bitchord://source?s=<configId>&t=<trackId>`.

---

## 4. Search flow (Search → Track Selection)

### 4.1 The YouTube search path

`app/src/main/java/com/music/bitchord/ui/MainViewModel.kt`

1. `runSearch()` (line 2276) builds a request and, for the YouTube tab, calls `YtMusicRepository.searchPage(request.query, request.filter, request.anonymous)` at line 2340.
2. `prefetchTopResult(rows)` (line 2460, defined 2527) warms the top result's stream URL so the likely first tap starts faster.

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/YtMusicRepository.kt`

- `searchPage()` (lines 334–342) calls `Innertube.search(query, filter.params, anonymous)` and parses with `InnertubeParser.parseSearchPage`.
- `search()` (lines 358–359) is the candidate-list helper used by the source adapter.
- Continuations: `searchContinuation()` (lines 345–352).
- Typeahead: `searchSuggestions()` (lines 372–375) and `searchTypeahead()` (lines 389+); the latter is deliberately unauthenticated so keystrokes do not pollute account history.

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/Innertube.kt`

- Base URL: `MUSIC_BASE = "https://music.youtube.com/youtubei/v1"` (line 88).
- `search()` (lines 629–640) POSTs to the `search` endpoint, authenticated or anonymous.
- `searchContinuation()` (lines 643–652).
- `searchSuggestions()` (lines 660–666), `searchTypeahead()` (lines 676–681).
- Requests carry `X-YouTube-Client-Name: 67` and `X-YouTube-Client-Version` (lines 1142–1143, 1209–1210), plus a body `clientName = "WEB_REMIX"` (lines 1158–1159, 1219–1220).

**Verified:** search results and all catalogue metadata are YouTube Music Innertube responses, not a third-party search API.

### 4.2 The multi-source search fan-out

`MainViewModel.sourceResults()` (lines 2487–2517):

- Only the **Songs** filter fans out (line 2491); albums/artists/playlists remain browse-shaped and YouTube-only.
- It asks `SourceRegistry.activeForPlayback()` (line 2492), not every enabled source, so a row shown is one the current quality ceiling would actually let play.
- Every non-YouTube source is asked in parallel with a per-source timeout (`SOURCE_SEARCH_TIMEOUT_MS`, line 2504).
- Results are split **above** and **below** YouTube's position in the active list (lines 2509–2516), so the Sources screen's drag order becomes visible in search.

`MainViewModel.runAddonSearch()` (lines 2167–2196):

- Asks every enabled addon in parallel with `ADDON_SEARCH_TIMEOUT_MS` (lines 2181–2189).
- Merges rows **round-robin** so no single catalogue buries the others (lines 2190–2193).
- Uses `SourceRegistry.active()` rather than the playback list, so addons skipped by the quality ceiling are still searchable; their rows are dimmed on tap (comment lines 2171–2173).

`YouTubeSource.search()` (lines 48–58) simply delegates to `YtMusicRepository.search(..., SearchFilter.SONGS)` and maps `SearchResult.Track` rows to `Song`.

### 4.3 Track selection

- A YouTube row keeps its video ID.
- A row from JioSaavn, an addon or a module is re-keyed into a `bitchord://source` URI carrying the source config ID, that source's own track ID, and a `matchQuery()` of title/artist/duration/album/explicit/video flags (`PlayerConnection.kt` lines 625–635, 683–697).
- The match fields ride in the URI because the resolver runs on ExoPlayer's loader thread with only a `DataSpec`; it cannot reach back into the UI (`SourceResolver.kt` lines 174–189).

---

## 5. Provider resolution (Provider → Audio Resolution)

### 5.1 Quality request is decided once

`app/src/main/java/com/music/bitchord/data/sources/SourceResolver.kt`

- `requestForNow()` (lines 54–70) maps the connection's effective ceiling to:
  - `LOSSLESS` → `StreamRequest.Lossless`
  - unlimited lossy ceiling → `StreamRequest.Best`
  - capped → `StreamRequest.Capped(maxKbps)`
- `requestForDownload()` (lines 95–101) reads `AppSettings.downloadQuality` only, independent of connection ceilings, because the answer becomes a file.

### 5.2 Source-backed playback URI

`SourceResolver.resolve(uri)` (lines 107–116) extracts `s` and `t` and calls the full resolver.

`SourceResolver.resolve(configId, trackId, target, pickedMix)` (lines 196–260):

1. Compute the request tier (line 202).
2. Get `activeForPlayback()` (line 203).
3. Find the pinned source **only among enabled sources**, so disabling a source also disables queued rows from it (lines 204–209).
4. **Lossless upgrade** (lines 218–233): if lossless is requested, the pinned source cannot serve it, and the row was not hand-picked, walk sources ranked above the pin and take the first genuinely lossless answer.
5. **Pinned source** (lines 235–244): ask it for the track.
6. **Cross-source fallback** (lines 254–259): if the pinned source fails, ask every other active source (except YouTube for a hand-picked addon row) via `bestAcross()`.

### 5.3 Racing multiple providers

`SourceResolver.bestAcross()` (lines 955–1045):

- Launches every source's `matchAndStream()` concurrently (lines 972–985).
- Takes the first usable answer for playback (line 1012 and following).
- On equal quality, the higher-ranked source wins, regardless of arrival order (lines 1008–1010, documented 938–941).
- With `waitForAll = true` (used by the background upgrade), it waits for every source instead of cancelling the slow ones.
- Sources still running when a playback answer is taken are cancelled; the second look re-asks them.

`SourceResolver.matchAndStream()` (line 1051):

- Tries each query `TrackMatcher.queries(target)` offers.
- Requires the candidate to actually be the recording (title/artist, optional strict length, optional shared artist).
- Calls the source's `stream()`.

### 5.4 YouTube-queued tracks and substitution

- `substituteForYouTube(target)` (lines 285–305): for a YouTube-queued track, asks only sources ranked **above** YouTube, racing them; returns the first playable copy.
- `prefetchSubstitute(target)` (lines 395–401): asks only sources marked `worthPrefetching` (currently JioSaavn), so the answer can be pinned before playback.
- `directAddons()` / `directStream()` (lines 317–343): "Direct Source Stream" asks only addons ranked above YouTube and waits for them.
- `youtubeFallback(target)` (lines 354–368): the last rung for a failed source-backed row; it searches YouTube Music for the metadata and streams the match.

### 5.5 YouTube audio resolution

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/StreamResolver.kt`

`resolve(videoId)` (lines 262–297):

- Checks a short-lived URL cache (`recent`, `URL_TTL_MS`).
- Refuses known-permanently-unplayable IDs without a request (`PermanentlyUnplayableException`, lines 269–272).
- Coalesces concurrent resolves for the same ID (line 274).

`resolveUncached(videoId)` (lines 427–489) is the actual pipeline:

1. **InnerTubeX first** — `innerTubeXStreamAllowingUploads(videoId)` (line 430).
2. **Live streams** use their HLS playlist instead of a growing adaptive window (lines 432–437).
3. **NewPipe fallback** — `newPipeStream(videoId, ::pickForQuality)` (line 440) if InnerTubeX finds nothing usable.
4. Failures are classified; permanent ones are remembered for 10 minutes (lines 480–484).

`innerTubeXStream()` (lines 497–535):

- Calls `InnerTubeXResolver.extract(videoId, maxKbps, skip, requireM4a, uploaded)` (line 508).
- **Probes** the returned URL before accepting it (line 515).
- Skips a client profile that minted an unusable URL and retries, up to `INNERTUBEX_ATTEMPTS` (lines 506, 520–526).
- Escalates to "ask as the user's own upload" on a refusal while signed in (lines 528–532, 538–560).

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/InnerTubeXResolver.kt`

- Uses `com.metrolist.innertubex` (imports lines 4–20): `InnerTubeExtractor`, `AudioQuality`, `ContentHints`, `PoTokenResult`, `TokenProvider`, `YouTubeCipherService`, `PlayerConfigRepository`, `RemotePlayerConfigStore`.
- This is the component that handles live client catalogues, cipher/signature solving, and PoToken/WebView challenges.

**Verified extraction order:** InnerTubeX → NewPipe → probe. The code comments and `resolveUncached` both establish this.

### 5.6 JioSaavn

`app/src/main/java/com/music/bitchord/data/sources/JioSaavnSource.kt` + `data/jiosaavn/JioSaavnService.kt`

- API base is `https://www.jiosaavn.com/api.php` (comment line 121).
- Search uses `__call=search.getResults` (around lines 180–207).
- Stream lookup uses `__call=song.getDetails` (lines 209–225).
- The media URL is **DES-encrypted**; it is decrypted with the 8-byte key `38346591` (lines 152–161).
- `selectBestSaavnStream()` (lines 66–82) rewrites `_96` to `_320` **only when the catalogue says `320kbps=true`** (lines 42–47, 77–81). It refuses to invent a 320 rendition.
- `JioSaavnSource` refuses streams at or below 96 kbps (`JioSaavnSource.kt` stream logic).
- JioSaavn is 320 kbps AAC/MP4, lossy, and **off by default**.

### 5.7 Addons (user-supplied HTTP servers)

`app/src/main/java/com/music/bitchord/data/sources/AddonSource.kt` + `data/sources/addon/AddonClient.kt`

- An addon is an ordinary HTTP server, not code (AddonClient doc lines 20–26; AddonModels lines 10–19).
- Endpoints:
  - `GET /manifest.json` — `manifest()` (line 82)
  - `GET /search?q=…` — `search()` (lines 133–147)
  - `GET /stream/{id}` — `stream()` (lines 164–171)
- `settingsFor()` (lines 182–231) negotiates `quality` (`LOSSLESS`/`HIGH`/`LOW`) and optional `atmos=auto`, rather than asserting a spelling the addon may not understand.
- `AddonStream` (`AddonModels.kt` lines 219–320) declares `url`, `format`, `quality`, `codec`, `container`, `manifest` (`none`/`hls`/`dash`), `encrypted`, `sampleRate`, `bitDepth`, `bitrate`, `error`.
- DRM/encrypted answers are **refused** (`isEncrypted`, lines 288–300).
- `AddonSource` converts the answer into `SourceStream`, sets `StreamContainer` for HLS/DASH, and refuses malformed URLs.

### 5.8 Legacy modules (JS plugins)

`app/src/main/java/com/music/bitchord/data/sources/ModuleSource.kt` + `module/ModuleManager.kt`, `module/QuickJsExecutor.kt`

- A module index is a JSON URL listing `SpineModule` descriptors; each ships JS exporting `searchTracks()` and `getTrackStreamUrl()` (`ModuleSource.kt` lines 20–25; `SourceKind.kt` lines 104–119).
- `ModuleManager.searchTracks()` (lines 240–277) loads the index, evaluates the JS in QuickJS, and calls the export.
- `ModuleManager` stream call (around lines 281–320) passes the requested tier (`LOSSLESS`/`HIGH`/`LOW`) and module settings.
- `QuickJsExecutor` (line 55+) keeps a pool of whole interpreters, each with the module script pre-evaluated, and wires in a `fetch()` implementation. The JS runs with no Android runtime access (SourceKind lines 111–112).
- **This kind is legacy and no longer created by the UI.** `SourceRegistry` removes the retired built-in `MODULE` config, while preserving a user's `CUSTOM_MODULE` (lines 147–150).

---

## 6. Stream delivery (Stream)

### 6.1 Virtual URIs

`app/src/main/java/com/music/bitchord/playback/PlayerConnection.kt`

`Song.toMediaItem()` (lines 647–701) builds one of:

- `bitchord://watch?v=<videoId>&n=…&a=…&d=…` (line 697)
- `bitchord://source?s=<configId>&t=<trackId>&n=…&a=…&d=…&mix=…` (lines 683–684)

A downloaded or local file substitutes a `content://` / `file://` URI at this point (lines 673–676), deliberately not later in the HTTP-only chain (comments 649–660).

### 6.2 `ResolvingDataSource` — the seam

`app/src/main/java/com/music/bitchord/playback/PlaybackService.kt`

- `ResolvingDataSource.Resolver { dataSpec -> … }` starts at line 1394.
- **Source branch** (lines 1409–1452): `SourceResolver.resolve(uri)` (or `youtubeFallback` for Direct YouTube), then rewrite the `DataSpec` URI to the real URL and attach per-stream headers.
- **YouTube branch** (lines 1453+):
  - Direct YouTube rollback (lines 1464–1486).
  - Automix analysis reads bypass the race (lines 1487–1504).
  - Forced upgrade streams (lines 1505–1538).
  - `StreamChoice.of(videoId)` pin (lines 1554–1607): the same source keeps serving the track for every re-open.
  - No substitutable source → plain `StreamResolver.resolve` (lines 1613–1637).
  - Cache already holds YouTube bytes → finish with YouTube (lines 1638–1672).
  - Otherwise `resolveWithModulePriority()` races higher-ranked modules against YouTube (lines 1673–1678).
- `Resolved` sealed interface (lines 5419–5422): `Module(stream)` or `YouTube(url)`.
- `resolveWithModulePriority()` (line 5465) races the sources and hands the losing-but-still-running lookup to `QualityUpgrade` if YouTube wins (doc lines 5424–5432).

### 6.3 `StreamChoice` — one source per track

`app/src/main/java/com/music/bitchord/playback/StreamChoice.kt`

- ExoPlayer re-opens a source many times: initial read, seek, resume, continuation fetch.
- Without a pin, a continuation fetch could resolve to a *different* source's file and append the middle of an MP4 to a WebM (doc lines 11–23).
- `StreamChoice` remembers the chosen source and stream for the track/cache entry (TTL ~15 minutes), and `StreamChoice.of(videoId)` is consulted before any new decision.

### 6.4 `ChunkedDataSource` — bounded ranges

`app/src/main/java/com/music/bitchord/playback/ChunkedDataSource.kt`

- googlevideo paces a continuous response to roughly playback speed: about **15 kB/s** for an unbounded read.
- The same bytes requested as bounded ranges are served at line rate: measured **~5.7 MB/s** (doc lines 16–30).
- Therefore every fetch is a run of bounded ranges (1 MiB by default; `STREAM_CHUNK_BYTES` at `PlaybackService.kt` line 1722).

### 6.5 Factory chain

`PlaybackService.kt` lines 1718–1733:

```
ResolvingDataSource.Factory(
    ChunkedDataSource.Factory(OkHttpDataSource.Factory(Http.client), STREAM_CHUNK_BYTES)
) { dataSpec -> … }
```

- `mediaSourceFactory = DefaultMediaSourceFactory(...)` at line 1749.
- `StreamContainer.served()` is told the resolved URL so HLS/DASH can be detected (`StreamContainer.kt` lines 7–30).
- `AudioCache.playbackFactory()` wraps the chain for read-ahead and the disk cache (`AudioCache.kt` line 1186).

### 6.6 `StreamContainer` — manifests

`app/src/main/java/com/music/bitchord/playback/StreamContainer.kt`

- Declares whether the resolved URL is progressive audio or an HLS/DASH manifest.
- Needed because the playback URIs are `bitchord://` and carry no extension; `DefaultMediaSourceFactory` would otherwise build a progressive source and fail on a manifest (lines 7–24).
- The app supports Media3 `HlsMediaSource` and `DashMediaSource`; Tidal addons in particular return DASH (or previously HLS).

---

## 7. Playback (Playback)

### 7.1 Player construction

`PlaybackService.buildPlayer()` (lines 3445–3460):

- `ExoPlayer.Builder(this)`
- `.setRenderersFactory(silenceSkippingRenderers(...))`
- `.setMediaSourceFactory(requireNotNull(mediaSourceFactory))`
- `.setLoadControl(farBufferingLoadControl())`
- `.setAudioAttributes(...)`, `.setHandleAudioBecomingNoisy(...)`
- `.setMaxSeekToPreviousPositionMs(...)`

### 7.2 Decoders

- **Platform MediaCodec** is the default renderer path.
- **Vendored FFmpeg** (`androidx.media3.decoder.ffmpeg.FfmpegAudioRenderer`, imported at `PlaybackService.kt` line 37; vendored under `app/src/main/java/androidx/media3/decoder/ffmpeg`) handles ALAC, AC-4, E-AC-3/AC-3, TrueHD and DTS.
- **ALAC always goes to FFmpeg**, because MediaCodec is not offered it (`withoutPlatformAlac`, comment around lines 9605–9613).
- **Custom DSD extractor** (`DsdExtractorsFactory`, imported line 85; `playback/dsd/`) adds DSF/DFF containers. DSD64–DSD1024 is decimated to **176.4 kHz PCM** (`DsdFormat.kt` lines 8–31).
- **Bit-perfect USB output** path (`BitPerfectOutput`, `BitPerfectAudioSink`, `BitPerfectRoutingSink`; imports lines 90–92; sink construction around lines 6636–6693).
- **Dolby Atmos** is gated by `DeviceCodecs` (`DeviceCodecs.kt`): E-AC-3 is vendor-licensed, so the app checks for the decoder before handing over an Atmos rendition (lines 11–23).

### 7.3 Caching and prefetch

`app/src/main/java/com/music/bitchord/playback/AudioCache.kt` (2,444 lines)

- Disk cache directory `audio_cache` under `noBackupFilesDir` (line 1166, init line 224).
- `playbackFactory()` (line 1186) and `readAheadCacheFactory()` (line 1329).
- `prefetchQueue()` (line 1370) warms upcoming tracks; `QUEUE_LOOKAHEAD = 1`, `QUEUE_DEPTH = 2` (lines 192–198).
- `cacheKeyOf(uri)` (line 2171) derives the Media3 cache key from the virtual URI.
- `heldByYouTube()` (line 573) prevents a cache entry started by YouTube from being finished by another source.
- The cache is bypassed for local files and content URIs so they are not written a second time.

### 7.4 Downloads

`app/src/main/java/com/music/bitchord/download/`

- `Downloads.kt` drives the queue and record of saved files.
- `Downloader.kt` fetches through `SourceResolver` (import line 15), so a download uses the same provider ladder and quality rules (`requestForDownload`).
- `OfflineDash.kt` / `OfflineHls.kt` handle segmented manifests.
- Taggers (`FlacTagger`, `Mp4Tagger`, `WebmTagger`, `MediaTagger`, `LyricsTag`) write metadata into the downloaded file.
- Playback prefers a verified saved file via `Downloads.verifiedSavedUri()` in `Song.toMediaItem()` (lines 673–674).

### 7.5 Quality upgrades

- `SourceResolver.upgradeFor()` (line 460) runs while audio is already playing.
- `worthSwapping()` (line 819) refuses a swap that is not genuinely better.
- `isBetter()` (line 1307) compares codec/lossless/bitrate/sample rate.
- `QualityUpgrade` (playback package) performs the mid-song swap and can audition a stream before committing.

---

## 8. Formats and quality that are genuinely supported

### 8.1 Format model

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/sources/StreamFormat.kt` (lines 17–59)

- Fields: `codec`, `kbps`, `sampleRateHz`, `bitDepth`, `kbpsInferred`.
- `isLossless` is decided on **codec alone** (lines 39–40): `flac`, `alac`, `wav`, `aiff`, `ape`, `wv`, `dsf`, `dff` (line 56).
- `isDolbyAtmos` recognises `eac3-joc`, `ec3-joc`, `dolby-atmos` (line 57). Atmos is immersive but **not** lossless.
- Unknown codec means unknown, not false — callers must decide.

### 8.2 Quality ceilings

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/settings/PlayerEnums.kt` (lines 17–28)

| Setting | Bitrate ceiling | What may answer |
|---|---|---|
| `LOW` | 64 kbps | YouTube only |
| `MEDIUM` | best available | YouTube only (~171 kbps Opus) |
| `HIGH` | best available | JioSaavn + YouTube; **addons skipped** |
| `LOSSLESS` | best available | addons + JioSaavn + YouTube |

`app/src/main/java/com/music/bitchord/data/settings/AppSettings.kt` `AudioQuality.permits(kind)` (lines 38–46):

```kotlin
LOSSLESS -> true
HIGH     -> !kind.canServeLossless
MEDIUM, LOW -> kind == SourceKind.YOUTUBE
```

### 8.3 What each provider actually yields

| Provider | Codec/format | Realistic ceiling | Lossless? |
|---|---|---|---|
| YouTube | Opus (WebM) or AAC (MP4) | ~171 kbps Opus | **No** |
| JioSaavn | AAC/MP4 | 320 kbps, only when catalogue says so | **No** |
| Addon | whatever it serves: FLAC, ALAC, WAV, MP3, AAC, DASH/HLS, E-AC-3 JOC | user's backend (often Tidal/Qobuz) | **Yes, if the addon has it** |
| Legacy module | same as addon via JS backend | user's backend | **Yes, if the module has it** |
| Local / SMB / WebDAV | whatever file is on disk | file's own | **Yes, if the file is lossless** |

**Verified bottom line:** with a stock install (YouTube only), the best available audio is YouTube's lossy Opus. Lossless/Atmos is a *bring-your-own-source* feature.

### 8.4 Container/transport support

- Progressive audio (WebM/MP4/FLAC/etc.).
- HLS via Media3 `media3-exoplayer-hls` (build.gradle.kts line 299).
- DASH via Media3 `media3-exoplayer-dash` (line 307).
- DSD (DSF/DFF) via the custom extractor.
- Dolby Atmos (E-AC-3 JOC) only where the device exposes a decoder.

---

## 9. APIs, libraries and dependencies

From `app/build.gradle.kts` (verified line numbers):

| Dependency | Version | Purpose | Line |
|---|---|---|---|
| Media3 ExoPlayer / session / common | 1.11.0 | Playback engine | 293–295 |
| Media3 OkHttp datasource | 1.11.0 | HTTP transport | 296 |
| Media3 HLS / DASH | 1.11.0 | Manifest playback | 299, 307 |
| InnerTubeX (`com.github.MetrolistGroup.innertubex`) | v0.7.0 | YouTube extraction, ciphers, PoToken | 366 |
| NewPipeExtractor | v0.26.3 (stripped jar) | YouTube extraction fallback | 246, 249–251 |
| Rhino | 1.8.1 | JS engine (legacy module support) | 386–387 |
| QuickJS (`io.github.dokar3:quickjs-kt-android`) | 1.0.14 | Sandboxed module JS | 394 |
| Ktor client (core/okhttp/content-negotiation/json/websockets) | 3.5.2 | Innertube, JioSaavn, HTTP JSON | 356–363 |
| kotlinx.serialization | 1.11.0 | JSON models | 360 |
| Coil | 3.0.4 | Artwork | 320–324 |
| SMBJ | 0.15.0 | SMB library | 397 |
| ONNX Runtime | 1.28.0 | Local audio analysis (Automix) | 403 |
| BouncyCastle | 1.85 | Crypto (incl. JioSaavn DES handling) | 430 |
| OkHttp | via Media3 datasource / mockwebserver 5.3.2 in tests | HTTP | 296, 437 |

Other relevant implementation pieces:

- **Innertube** (`Innertube.kt`) — hand-written WEB_REMIX client over Ktor.
- **InnerTubeXResolver** — bridges to the InnerTubeX library.
- **NewPipeExtractor** — `StreamResolver` imports `org.schabi.newpipe.extractor.*` (lines 21–25) and calls `ServiceList.YouTube`.
- **QuickJS executor** — `QuickJsExecutor.kt` with an interpreter pool and a wired `fetch()`.

---

## 10. What is *not* the music source

These are commonly mistaken for the source layer; the code shows otherwise.

### 10.1 `backend/` is not a music backend

`backend/README.md` describes a Go + WebSocket **"Listen Together"** party server: create a six-character code, up to five devices listen together, anyone can control playback, no database. It synchronises playback state and clocks; it serves no audio and holds no catalogue. The app's music paths never call it.

### 10.2 Spotify is not a streaming provider

`shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/spotify/SpotifyImporter.kt` and `SpotifyLibrary.kt` import library/playlist metadata. They do not resolve audio. Canvas lookups (`data/canvas/SpotifyCanvas.kt`, `TidalCanvas.kt`, `AppleMusicCanvas.kt`) fetch artwork/video loops only.

### 10.3 Local, SMB and WebDAV are libraries, not catalogues

- `LocalMediaRepository` reads `MediaStore` audio (lines 89–97) for the Library tab.
- `SmbRepository` lists an SMB share into `Song` rows.
- `WebDavRepository` does the same for WebDAV.
- These are the user's own files and play through local/content URIs, not through the provider ladder.

### 10.4 The `MODULE` kind is retired

The UI no longer creates `MODULE` sources, and `SourceRegistry` deletes the old built-in one on init. `CUSTOM_MODULE` is preserved only for users who configured one in an earlier build.

---

## 11. Complete flow, end to end

### Path A — a YouTube Music track (default)

1. **Search:** `MainViewModel.runSearch()` → `YtMusicRepository.searchPage()` → `Innertube.search()` → `music.youtube.com/youtubei/v1/search` (WEB_REMIX).
2. **Selection:** the chosen `Song` keeps its video ID.
3. **Queue:** `Song.toMediaItem()` builds `bitchord://watch?v=<id>&n=&a=&d=…`.
4. **Resolution:** ExoPlayer opens the item; `ResolvingDataSource.Resolver` sees `watch`.
5. **Substitution check:** if a source is ranked above YouTube, `substituteForYouTube()` races it; otherwise skip.
6. **YouTube resolve:** `StreamResolver.resolve(videoId)` → InnerTubeX extraction (client catalogue + ciphers + PoToken) → probe → NewPipe fallback if needed.
7. **Stream:** the returned signed `googlevideo.com` URL is put into the `DataSpec`, with the User-Agent the URL expects.
8. **Transport:** `ChunkedDataSource` fetches bounded 1 MiB ranges through OkHttp; `AudioCache` may serve/record bytes.
9. **Playback:** Media3 selects progressive/HLS/DASH, decodes with MediaCodec/FFmpeg/DSD, outputs through the audio sink (optionally bit-perfect USB).
10. **Upgrade:** if lossless is enabled and a higher-ranked source might have a better copy, `QualityUpgrade` can swap mid-track after an audition.

### Path B — a JioSaavn or addon row

1. **Search:** the source's own `/search` or JioSaavn `search.getResults`.
2. **Selection:** the row becomes `bitchord://source?s=<configId>&t=<trackId>&…`.
3. **Resolution:** `SourceResolver.resolve(uri)`; lossless upgrade if requested and possible; pinned source; cross-source fallback.
4. **Provider answer:** JioSaavn decrypts its DES URL and conditionally upgrades `_96`→`_320`; an addon returns JSON with `url`, codec/container/manifest/quality.
5. **Stream:** `ResolvingDataSource` rewrites to the real URL and headers.
6. **Playback:** same Media3 pipeline; HLS/DASH is declared via `StreamContainer`.

### Path C — a downloaded/local track

1. `Song.toMediaItem()` substitutes a verified `content://` / `file://` URI.
2. `AudioCache.playbackFactory()` bypasses the disk cache for local URIs.
3. `DefaultDataSource` picks `ContentDataSource` / file source.
4. Media3 decodes and plays; no network, no provider resolution.

---

## 12. Verified findings vs assumptions vs unknowns

### 12.1 Verified from source code

- YouTube Music is the primary catalogue and the only always-enabled source.
- Search/metadata go through `music.youtube.com/youtubei/v1` with WEB_REMIX client headers.
- Audio comes from `googlevideo.com` URLs minted at play time; BitChord hosts nothing.
- Extraction order is InnerTubeX → NewPipe → probe, with a 15-minute URL cache and permanent-unplayable memory.
- JioSaavn is bundled but off by default; it is 320 kbps AAC at best, DES-encrypted URL, conditional `_320` rewrite.
- Addons are plain HTTP (`/manifest.json`, `/search`, `/stream/{id}`), never execute code on-device, and can serve lossless/Atmos.
- Legacy modules are QuickJS-sandboxed JS plugins; the `MODULE` kind is retired and removed on init.
- The stock build ships **no** lossless provider.
- `ChunkedDataSource` exists because unbounded googlevideo reads are throttled (~15 kB/s vs ~5.7 MB/s ranged).
- `StreamChoice` pins one source per track/cache entry to prevent mixed-container corruption.
- Media3 1.11.0 + vendored FFmpeg + custom DSD extractor + optional bit-perfect USB output.
- The Go backend is a Listen Together party server only.

### 12.2 Reasonable inferences (not directly asserted by a single line)

- The `com.metrolist.innertubex` library performs the actual YouTube player-JS/cipher/PoToken work; BitChord's `InnerTubeXResolver` is a wrapper that supplies budgets, client skips and upload escalation.
- Addon backends in the wild frequently front Tidal/Qobuz/Deezer, based on the code's own comments and format vocabulary (Tidal `HI_RES_LOSSLESS`, DASH manifests, Deezer ARL errors), but **no specific backend is bundled or guaranteed**.
- JioSaavn's availability and 320 kbps catalogue coverage can vary by region and time; the code only trusts the API's `320kbps` flag.

### 12.3 Unknown / not determinable statically

- **Exact live YouTube client profiles, cipher versions and PoToken behaviour** at any future date — these are remote and change; the code is built to adapt but the current live set was not exercised.
- **Which addon(s) a given user has configured** — none ship, and the report cannot know a user's private index URL.
- **Actual measured bitrate/quality of a specific track** — `StreamFormat` fields are declarations; the player measures real values separately (`NerdStats`), and the report did not run the app.
- **Whether JioSaavn's `320kbps` flag is truthful for every track** — the code guards against inventing 320, but cannot verify the CDN's bytes without playback.
- **Regional/catalogue licensing** — YouTube Music availability differs by country; the app reads a region setting but the report cannot assert catalogue contents.
- **Server-side behaviour of third-party addons** — their catalogues, legality and reliability are outside this repository.
- **Exact MediaCodec support on a given device** — `DeviceCodecs` probes at runtime; results are device-specific.

---

## 13. Key file reference index

| Concern | File | Key lines |
|---|---|---|
| Source contract | `app/src/main/java/com/music/bitchord/data/sources/MusicSource.kt` | 14–55, 66–75, 95–157 |
| Source kinds/ranks | `app/src/main/java/com/music/bitchord/data/sources/SourceKind.kt` | 16–167 |
| Seeding/migration | `app/src/main/java/com/music/bitchord/data/sources/SourceRegistry.kt` | 136–158, 549 |
| Resolution/fallback/quality | `app/src/main/java/com/music/bitchord/data/sources/SourceResolver.kt` | 54–70, 107–116, 196–260, 285–305, 955–1045, 1051, 1307 |
| YouTube adapter | `app/src/main/java/com/music/bitchord/data/sources/YouTubeSource.kt` | 48–58, 70–73 |
| JioSaavn adapter | `app/src/main/java/com/music/bitchord/data/sources/JioSaavnSource.kt` | 10–108 |
| JioSaavn API/crypto | `app/src/main/java/com/music/bitchord/data/jiosaavn/JioSaavnService.kt` | 66–82, 121, 152–161, 209–225 |
| Addon adapter | `app/src/main/java/com/music/bitchord/data/sources/AddonSource.kt` | 18–469 |
| Addon protocol | `app/src/main/java/com/music/bitchord/data/sources/addon/AddonClient.kt` | 82, 133–147, 164–171, 182–231 |
| Addon wire shapes | `app/src/main/java/com/music/bitchord/data/sources/addon/AddonModels.kt` | 219–320 |
| Module adapter | `app/src/main/java/com/music/bitchord/data/sources/ModuleSource.kt` | 20–25, 380–430 |
| Module runtime | `app/src/main/java/com/music/bitchord/data/sources/module/ModuleManager.kt` | 240–320 |
| QuickJS sandbox | `app/src/main/java/com/music/bitchord/data/sources/module/QuickJsExecutor.kt` | 55+ |
| Search orchestration | `app/src/main/java/com/music/bitchord/ui/MainViewModel.kt` | 2167–2196, 2276, 2340, 2487–2517 |
| YouTube metadata API | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/YtMusicRepository.kt` | 334–359 |
| Innertube client | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/Innertube.kt` | 88, 629–681, 1142–1143 |
| YouTube stream pipeline | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/StreamResolver.kt` | 262–297, 427–489, 497–535 |
| InnerTubeX bridge | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/innertube/InnerTubeXResolver.kt` | 1–408 |
| Playback resolution | `app/src/main/java/com/music/bitchord/playback/PlaybackService.kt` | 1394–1709, 1718–1733, 1749, 3445–3460, 5419–5422, 5465 |
| Media item/URI | `app/src/main/java/com/music/bitchord/playback/PlayerConnection.kt` | 625–635, 647–701 |
| Source pinning | `app/src/main/java/com/music/bitchord/playback/StreamChoice.kt` | 1–206 |
| Range transport | `app/src/main/java/com/music/bitchord/playback/ChunkedDataSource.kt` | 16–30 |
| Manifest detection | `app/src/main/java/com/music/bitchord/playback/StreamContainer.kt` | 7–30 |
| Disk cache | `app/src/main/java/com/music/bitchord/playback/AudioCache.kt` | 224, 1166, 1186, 1329, 1370, 2171 |
| Format model | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/sources/StreamFormat.kt` | 17–59 |
| Quality settings | `shared/src/jvmSharedMain/kotlin/com/music/bitchord/data/settings/PlayerEnums.kt` | 17–28 |
| Quality gating | `app/src/main/java/com/music/bitchord/data/settings/AppSettings.kt` | 38–46 |
| Device codecs | `app/src/main/java/com/music/bitchord/data/sources/DeviceCodecs.kt` | 11–23 |
| DSD | `app/src/main/java/com/music/bitchord/playback/dsd/DsdFormat.kt` | 8–31 |
| Downloads | `app/src/main/java/com/music/bitchord/download/Downloads.kt`, `Downloader.kt` | — |
| Dependencies | `app/build.gradle.kts` | 246–251, 293–307, 320–324, 356–403, 430 |
| Party server (unrelated) | `backend/README.md` | — |

---

## 14. Final summary

- **Actual music source:** YouTube / YouTube Music audio from Google's `googlevideo.com` CDN. BitChord is a client, not a host.
- **Metadata source:** YouTube Music Innertube `WEB_REMIX` API (`https://music.youtube.com/youtubei/v1`), via `Innertube` and `YtMusicRepository`.
- **Audio delivery mechanism:** play-time extraction of a signed `googlevideo.com` URL (InnerTubeX first, NewPipe fallback, then probe), fetched by Media3/ExoPlayer through `ResolvingDataSource` → `ChunkedDataSource` (bounded 1 MiB ranges) → `OkHttpDataSource`, with an optional disk cache.
- **Optional providers:** JioSaavn (off by default, 320 kbps AAC max) and user-added HTTP addons or legacy QuickJS modules (may serve lossless/Atmos).
- **Lossless:** only via a user-supplied addon/module (or local lossless files). Not bundled, and impossible on YouTube alone.
- **Report path:** `reports/music-source-investigation.md`
- **MD5 checksum:** computed over the final file and reported with the delivery message; a sidecar `reports/music-source-investigation.md.md5` records the same value.

### Unresolved questions

1. Which exact InnerTubeX client profiles/ciphers/PoToken strategy will be live at any future date (remote, adaptive, not exercised here).
2. Which addon(s) a given user has configured and what backends those addons front.
3. Whether every JioSaavn track's `320kbps` flag corresponds to a genuinely available 320 kbps rendition.
4. Real measured bitrate/format of any specific track — declarations were inspected, not playback measurements.
5. Device-specific decoder availability (especially E-AC-3/Atmos and ALAC) — probed at runtime, not statically knowable.
6. Regional catalogue availability and licensing for YouTube Music and JioSaavn.
7. Legal/operational status of third-party addons and module indexes — outside the repository.
