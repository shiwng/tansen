# BitChord technical audit

Reference materials: GitHub repo `kushagrasinghx/BitChord` (cloned 2026-10-27, HEAD `04a4a838`, version **1.8.1 / versionCode 29**) and the supplied APK `BitChord-v1.8.1.apk` (SHA-256 `931b9ad5641ab7a820554ca7c679bf7b834457df09fb9998d85bc180bfe27b5b`, 171,425,982 bytes).

APK identity evidence: apktool decode reports `package=com.music.bitchord`, `versionName=1.8.1`, `versionCode=29`, `minSdk=26`, `targetSdk=36`; v2-only signing with a single self-consistent signer (certificate SHA-256 `b5895722…`), no rotation lineage. **Repo version == APK version**, so source code is an accurate behavioral reference; the APK corroborates screens and the manifest.

License: **GPL-3.0**. Implication: any translation of BitChord's Kotlin logic into Rust/TS must remain GPL — this project adopts GPL-3.0-or-later. No code is copied mechanically; behavior is re-implemented idiomatically with attribution.

## 1. Module map (as built)

| Gradle module | Contents | Notes |
|---|---|---|
| `app` | Android host: Compose `MainActivity` (tab shell + detail stack), `PlaybackService` (9,028 lines), sources, downloads, settings, auth, SMB/WebDAV, canvas, scrobbling, widgets, Wear bridge | All Media3/ExoPlayer/WebView/media-store specific |
| `shared` (`commonMain`) | Pure Kotlin: `Models.kt` (Song/Album/Artist/Playlist, queue tiers), playlist ordering (LIS-based minimal moves), party wire models, server clock, settings enums, EQ curves, automix planning math | Ports 1:1 to Rust |
| `shared` (`jvmSharedMain`) | Innertube client (WEB_REMIX) + parser (1,551 lines), `YtMusicRepository` (1,068 lines), `StreamResolver` (1,191 lines), PoToken system, 5,389 lines of lyrics code, queue algebra (`QueueWindow`/`QueueTimeline`/`ContextRegistry`), kizzy Discord gateway, Spotify importer/library | Logic portable; engines (NewPipe extractor, InnerTubeX lib, BotGuard WebView) are the sticky parts |
| `sharedUi` | Compose Multiplatform UI (jvmSharedMain targets Android + JVM desktop): Home/Search/Library/Explore screens, 3,725-line `NowPlayingScreen`, Apple-style synced-lyrics renderer (three-copy sweep, word lift/fall, `LyricClock`), `ArtworkPalette` (median-cut quantizer + HSL mapping) | Compose rendering must be rebuilt for React; algorithms (LyricClock, palette) are fully specified and unit-tested |
| `backend` | Go listening-party server (WebSocket hub, roles, invite codes, server clock) | Can be reused as-is or re-hosted |
| `tv` / `wearos` / `web` | Platform variants | Out of desktop scope |

## 2. Navigation & screens

Four bottom tabs (Home, Explore, Library, Search) + a full-screen now-playing sheet over the tabs; detail pages (album/artist/playlist) live on a separate interleaved detail stack, not routes (`AppNavigator.kt`). Settings is a route. Now-playing screen: artwork page ⇄ lyrics page toggle (no pager), queue panel, output/cast/provider sheets, horizontal drag-to-skip, tap-lyric-to-seek, sleep timer. Typography: SF Pro Display, heavy tight display scale; black/white Apple-Music-style Material3 schemes.

## 3. Discovery & source resolution

- **Catalogue**: `YtMusicRepository` over a minimal `WEB_REMIX` youtubei client (`music.youtube.com/youtubei/v1`), SAPISIDHASH cookie auth, visitorData minted from `youtube.com/sw.js_data` (a `player` request without it is silently throttled). Covers home/search/browse/radio/library/playlist CRUD/history.
- **Streaming**: `StreamResolver` → InnerTubeX library (client catalog + cipher tiers + PoToken) → NewPipe extractor failsafe (off hot path) → **probe before trust** (bytes must be fetched before a URL is cached). PoToken = Android WebView running BotGuard; without it web-client stream URLs 403. `ChunkedDataSource` re-issues googlevideo reads as ranged requests (unbounded reads are throttled to ~15 kB/s). `PlaybackTracker` sends play/ATR/watchtime pings.
- **Source abstraction** (`MusicSource.kt`): three methods — `health()`, `search(query, limit, waitForAll, request)`, `stream(trackId, request)` — with `StreamRequest` = Lossless | Capped(maxKbps) | Best and `SourceHealth` = Ok | Unreachable | Rejected. Kinds ranked: ADDON(0), CUSTOM_MODULE(0), MODULE(1), JIOSAAVN(2), YOUTUBE(3).
- **Resolver** (`SourceResolver.kt`): quality question answered once per connection; with a lossless request, sources ranked above the pin are asked first for a genuinely-lossless upgrade; pinned source next; then all remaining sources **race** (`bestAcross`, first usable answer wins — rank only breaks ties) because sources differ in speed by ~2 orders of magnitude (measured: JioSaavn 0.4 s vs module 13.5 s). `upgradeFor` runs mid-track with `waitForAll` and a `worthSwapping` bar (lossless, or ≥96 kbps gain). Strict length checks (±2 s for mid-track swap, ±3 s for same-recording) prevent playing a different cut under the right title.
- **Modules**: QuickJS sandbox (`searchTracks(query, limit, context)`, `getTrackStreamUrl(id, quality, context)` JSON contract; only `fetch` wired in). **Addons**: plain HTTP JSON — `GET /manifest.json`, `/search?q=&quality=`, `/stream/{id}?quality=&atmos=`; nothing executed client-side.

## 4. Playback & audio

- Media3 `MediaLibraryService` with **two full ExoPlayers**; crossfade = incoming track loaded on the standby player, equal-power sin²/cos² gain curves driven off the *incoming* player's position, roles swap at the curve midpoint. Automix ("Smart Fade"): ONNX beat/key analysis (vendored "Beat This!" model + JVM DSP halves) → `TransitionPlan` (BEATMATCHED / DJ_ASSISTED / PLAIN_CROSSFADE), filter sweeps / bass swap / echo out, tempo stretch ≤10% speed-up only.
- Gapless: Media3 queue advance + `LoudnessProcessor` gain promotion at the exact stream boundary (`PrecisionAudioSink.setOutputStreamOffsetUs`).
- DSP chain (float32 blocks): Spatial → 10-band parametric EQ (bilinear SVF, gliding coefficients) → transition filter → loudness normalization (YouTube `loudnessDb`) + instant-attack limiter. Skip silence via custom renderers + FFmpeg JNI.
- Caching: `SimpleCache` on-disk, keyed by videoId, 512 MB default LRU, read-ahead of next track in 2 MB chunks; `StreamChoice` pins per-video source for 15 min so cache entries are never spliced from different servers.
- Queue: three tiers (CONTEXT / USER_QUEUE / AUTOPLAY) via `QueueCoordinator` + shared `QueueTimeline`/`QueueWindow` (25-ahead window, low water 12, deterministic shuffle per cycle); radio/autoplay built from watch queue with per-artist caps. Queue snapshot persisted to SharedPreferences; recoverable playback state.
- Sleep timer (deadline or end-of-track), playback speed, volume normalization, DSD (DSF/DFF) decode via extractor factories.

## 5. Local library

`LocalMediaRepository` queries **MediaStore** (no own database): `IS_MUSIC != 0`, duration ≥ 30 s, extension whitelist (mp3/m4a/mp4/flac/ogg/opus/aac/webm/dsf/dff), ringtone/podcast filters, optional SAF folder filter; artwork via MediaStore `content://media/external/audio/albumart/<albumId>`. Downloads merge back via saved URI → videoId mapping. `LocalGroups.kt` groups folders into albums/artists with stable page ids that survive rescans. Missing-file handling happens at playback (drop record, restream). SMB (SMBJ) and WebDAV (PROPFIND) repositories share the same `Song` shape, album = parent folder, cover = sibling image file.

## 6. Lyrics

17 providers, raced in user-configurable priority order (declaration order is default): LrcRed, BiniLyrics, BetterLyrics(+Portato), PaxSenix(×3), LyricsPlus, SimpMusic(hidden), Unison, YouTube transcript/music, Megalobiz(hidden), KuGou, LrcLib, Musixmatch, Genius (lazy-started, plain text). Word-synced beats line-synced outright; otherwise highest-priority line-synced wins unless `prioritizeSyllableSync`. ISRC identification runs before the race (BiniLyrics, 2.5 s cap, 100-entry LRU) so matches settle on a recording, not a name; a file's own ISRC tag wins. Formats: Apple TTML (per-syllable spans, background `x-bg` roles), QRC/YRC karaoke, enhanced A2/LRC word stamps, plain LRC/text. `LyricLine` carries words/syllables, sweep spans, animation windows (Apple-style rise-and-settle), duet alignment (alternating sides on voice change). Translation = Google endpoints with LRU + 2 MB disk cache. Embedded lyrics from file tags (custom `BITCHORD_LYRICS` preferred, then LRC) and `lyrics.lrc` sidecars.

## 7. Downloads, settings, auth, persistence

- **Downloads**: `DownloadService` (foreground dataSync). Routed: source `.mpd`/`.m3u8` → DASH-saved-as-HLS package or CMAF segments reassembled into one native FLAC/M4A (`ManifestFile` rewrites `fLaC` STREAMINFO; MP4 init+fragments with `styp`/`sidx` dropped); encrypted/multi-period rejected; else YouTube progressive with `requireM4a` when exporting to MediaStore. Taggers: FLAC (streaming block rewrite), MP4 (box surgery + whole-file `sidx`), WebM; cover + embedded lyrics. Export to `Music/BitChord`; interrupted downloads don't survive (memory-only queue); saved records verified lazily.
- **Settings** (`AppSettings.kt`, 2,108 lines): ~90 `MutableStateFlow` keys over SharedPreferences, grouped: playback/quality (per-connection audio quality, crossfade seconds, smart fade, skip silence, speed, repeat/shuffle/autoplay), audio output (PCM mode, USB DAC, loudness, Atmos, spatial), EQ, downloads (quality, wifi-only, export), sources/network (WebDAV/SMB credentials via AuthStore), cache (512 MB default), appearance (theme, liquid glass, canvas, nerd stats), lyrics (sources + order, offset, translation), library, integrations (Last.fm/ListenBrainz/Spotify/Discord).
- **Auth**: Jetpack `EncryptedSharedPreferences` (self-healing keyset); Google/YT Music = WebView cookie capture (`SAPISID`/`__Secure-*PAPISID` names checked); Spotify = `spdc` cookie; Discord = `localStorage.token` scrape. SAPISIDHASH signing for Innertube.
- **Stats**: wall-clock listening deltas (8 s cap, play counts at ≥30 s or ≥50%), monthly JSON buckets, Replay year-story, backup export.

## 8. Integrations

Last.fm web-services (`auth.getToken` → `getSession`, MD5-signed posts, libre.fm endpoint swap); ListenBrainz token API; Discord Rich Presence as the **user account token** over the gateway WebSocket (kizzy), activity with timestamps adjusted for playback speed; Listen Together = WS protocol with `seq`-ordered frames, server heartbeat, NTP-style clock offset (min-RTT wins), host-gated queue ops, Go reference server; Canvas (animated artwork) from Apple/Tidal/Spotify/community index with LRU + negative caching; Spotify playlist import (embed-page scrape + TrackMatcher matching to YTM) and library read (pathfinder GraphQL).

## 9. Tests & build

21 Kotlin test files / 124 tests, all covering **pure logic** (queue window/timeline, lyric clock/focus/offset, automix planners, server clock, playlist moves, models, context registry, party queue reading). No tests for network layers or Compose UI. Build: Gradle KMP, `dev`/`prod` flavors, ABI splits, optional keystore (unsigned release otherwise), CMake FFmpeg JNI, bundled Wear APK asset; no CI workflows found beyond standard GitHub hosting.

## 10. Port assessment

- **Reusable concepts, must re-implement**: everything (Compose UI, Media3, Android services do not exist on desktop).
- **Directly portable as logic** (GPL-compatible translation): models, queue algebra, `TrackMatcher`, resolver ordering rules, lyrics parsing (TTML/LRC/QRC), `LyricClock`, `ArtworkPalette` quantizer math, playlist minimal-move algorithm, party protocol + clock sync, scrobble gating rules.
- **Blocked / external-service-dependent**: YouTube Music streaming requires a working BotGuard PoToken pipeline on desktop (WebView or equivalent) — the single biggest risk; Discord RPC needs a user-token gateway (no official third-party presence API); Spotify library needs the user's own session; canvas providers depend on third-party endpoints.
- **Uncertainties**: exact Automix ONNX model reuse on desktop (licensing + runtime); whether the module index host remains available; JioSaavn/addon endpoint stability.
