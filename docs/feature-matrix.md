# Feature parity matrix

Statuses (per the project brief): `NOT AUDITED · AUDITED · NOT IMPLEMENTED · IN PROGRESS · IMPLEMENTED · TESTED · BLOCKED · NOT APPLICABLE`.

- **IMPLEMENTED** = present in the code. **TESTED** = passed its required test (unit, integration, or verified behavior — never "it compiles").
- **BLOCKED** / **NOT APPLICABLE** always carry the reason.
- Reference: upstream = the pinned upstream Kotlin sources the sidecar compiles against (see `scripts/build-sidecar.sh`).

Legend for "how": [V] parity-checked against the upstream web player (GPL-3.0) · [P] ported/reimplemented in Rust/TS · [R] Rust backend.

## 1. Shell & navigation (design parity)

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| Glass sidebar (Search/Home/New + Library sections + Playlists + Settings gear) | `web/app/src/shell/Sidebar.tsx` | TESTED | [V]+[P] | Same classes/structure; sections re-bound to the local library. Verified: DOM capture shows nav tree with `aria-current` (REA browser scenario, 2026-10-28) |
| Design tokens (colors, type scale, radii, motion, z-layers) | `styles/tokens.css` | TESTED | [V] | Verbatim; DM Sans self-hosted (variable woff2) replacing the Google-CDN link |
| Tailwind v4 glue (`@theme inline`, `type-*`, `glass-material`, `glass-pill`, `artwork-stroke`, `hairline-*`, no-preflight reset) | `styles/global.css` | TESTED | [V] | Verbatim + DM Sans `@font-face` |
| SF Pro Display (lyrics font) | `assets/fonts/sf-pro-*` | IMPLEMENTED | [V] | ⚠ License flag: Apple-proprietary face shipped upstream without its own license; kept for lyrics parity, recorded in README limitations |
| Wordmark + logo | `logo-mark.svg` | IMPLEMENTED | [V] | |
| Player pill (LCD: artwork→fullscreen, title/artist links, 2px seek line with hover times, transport, volume, Up Next/lyrics toggles) | `PlayerBar.tsx`, `Lcd.tsx` | TESTED | [V]+[P] | Video/motion/nerdstats code paths stripped; `useScrubber` re-driven by engine `Position` events (10 Hz) + rAF interpolation |
| Side panel (Up Next ⇄ lyrics, slide-in) | `SidePanel.tsx` | IMPLEMENTED | [V] | |
| Full-screen player (artwork + mesh backdrop, lyrics/queue columns, idle chrome fade, browser fullscreen) | `FullScreenPlayer.tsx` | IMPLEMENTED | [V]+[P] | Video surface, motion artwork, NerdStats, favorite button omitted (see §7) |
| Mesh backdrop (artwork-derived palette) | `MeshBackdrop.tsx` + `lib/meshPalette.ts` | IMPLEMENTED | [V] | Canvas-based; works on local artwork via `art://` |
| Global keyboard shortcuts (Space, ⌘←/→/↑/↓, ⌘F, /, Esc) | — | NOT IMPLEMENTED | | The vendored hook is not part of the current tree; search, menus, dialogs, and the full-screen player keep their own keys |
| Scrubber (drag-seek with pending-commit, `--progress` rAF) | `useScrubber.ts` | IMPLEMENTED | [V] | `getAudio()` returns null on desktop; `getPlayhead()` interpolates engine ticks |
| Mobile drawer + top bar (<484px) | `MobileTopBar.tsx` | IMPLEMENTED | [V]+[P] | Responsive rules from the original kept |
| Router (lazy page registry, preload on hover) | `pageRegistry.tsx`, `routes.tsx` | TESTED | [V]+[P] | Local routes; router redirect verified in browser scenario |
| Light/dark themes | `tokens.css` | IMPLEMENTED | [V] | `prefers-color-scheme` only (upstream behavior); no manual toggle upstream → none here |

## 2. Library

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| Import folders (dialog → scan) | MediaStore equivalent | TESTED | [R] | `folder_add`+`scan_start`; end-to-end cargo test on a real WAV; dedupe verified (rescan adds 0) |
| Metadata extraction (tags, duration, codec, bitrate, sample rate, bit depth, channels) | MediaStore fields | TESTED | [R] | lofty; asserted in scanner test |
| Artwork extraction + cache | MediaStore albumart | TESTED | [R] | Content-hash cache (`sha256(path+mtime)`); unchanged files never rewritten |
| `art://` serving to the webview | content:// | IMPLEMENTED | [R] | Custom Tauri protocol, path-scoped |
| Songs view (sortable track list) | LibraryScreen songs | TESTED | [V] TrackList | Virtualized ≥80 rows as upstream |
| Albums / Artists grids + detail pages | `LocalGroups.kt` pages | TESTED | [R]+[P] | SQL groupings (`persistence::views`), tested incl. album_artist fallback; DetailHeader + TrackList |
| Recently Played | history section | IMPLEMENTED | [R]+[P] | `play_history` rows on every track start |
| **Recently Added** (library additions, newest first) | MediaStore date-added | IMPLEMENTED | [R]+[P] | `recently_added_get` ordered by `added_at` (kept across rescans); the toolbar's in-place “Find in Recently Added” filters it without leaving the page |
| Search (albums/artists/songs) | SearchScreen | IMPLEMENTED | [P] | Client-side over the index; catalogue typeahead and remote search rows come from the sidecar when sources are enabled |
| Scan progress + summary states | — | IMPLEMENTED | [R]+[P] | `scan://progress`/`scan://done` events |
| Missing-file detection (mark, not delete) | playback-time restream | TESTED | [R] | Scanner marks `missing`, verified in test |
| Playlists: create / rename / delete / set contents / play | YTM playlists (local analog) | TESTED | [R] | Repo-level; schema tested; UI wired (create/rename/delete/play) |
| **Add to Playlist** (track ⋯ menu → picker, or create-on-the-spot) | TrackMenu items | TESTED | [R]+[P] | `playlist_add_tracks` (append); picker vendored from upstream's view shape |
| **Playlist reorder page** (drag rows, Save/Cancel) | `ReorderPlaylistPage.kt` | TESTED | [P] | Dedicated page like the original; `reorderMove` helper unit-tested (6 vitest cases) |
| Playlist reorder UI | PlayerQueue drag | IMPLEMENTED | [V] UpNext | Drag-move and remove on Up Next rows; `ReorderPlaylistPage` handles in-playlist order |
| **Queue persistence across restarts** | `LastPlayed.saveQueue` | TESTED | [P] | Frontend `localStorage` mirror (`tansen:playback`, versioned, corrupt payloads dropped); restore re-seeds the engine paused at the saved position and re-resolves remote URLs — `player.persist.test.ts` |
| **Library watcher (auto-rescan on file change)** | MediaStore auto-update | TESTED | [R] | `notify` watcher, 2 s debounce, initial scan on subscribe, `watch_folders` setting gates it, scans skipped while a manual scan runs; integration test covers add/file/setting-off phases |

## 3. Playback

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| Play/pause/stop/next/previous/seek/volume | PlaybackService | TESTED | [R] | Queue algebra unit-tested (9 tests); engine smoke-launched; transport wired end-to-end |
| Queue ops (load/insert/jump/remove/clear/move) | QueueCoordinator tiers | TESTED | [R] | `Queue` model tested incl. `Move` index-tracking |
| Repeat off/all/one | repeat modes | TESTED | [R] | Unit-tested (end-of-queue stop, repeat-all wrap, repeat-one) |
| Shuffle | shuffle | IMPLEMENTED | [P] | Seed-random start + setting; in-queue deterministic shuffle (upstream `QueueWindow`) pending |
| Gapless advance | Media3 gapless | IMPLEMENTED | [R] | 2 s pre-append + position-drop handoff detection; measured-seam validation pending |
| Native media keys (macOS Now Playing) | MediaSession | IMPLEMENTED | [R] objc2-media-player | Attached on the main thread; play/pause/seek/next/prev/stop wired; metadata and anchor updates ride engine publishes (MediaPlayer interpolates elapsed). Windows SMTC is not implemented |
| Playback errors surface + auto-advance | `recoverFrom` | IMPLEMENTED | [R]+[P] | `TrackError` event → UI error band + queue advance |
| Crossfade 0–12 s, linear or equal-power | `CrossfadeController` (two ExoPlayers) | IMPLEMENTED | [R]+[P] | Second sink + fade ramps in `playback::pipeline`; duration/curve from Settings are applied through `audio_settings_set` |
| Playback speed | speed | NOT IMPLEMENTED | | rodio `set_speed` available |
| Skip silence | FFmpeg renderers | IMPLEMENTED | [R] | `SilenceSkipConfig` on the pipeline, threshold + minimum duration in Settings |
| Equalizer (10-band peaking) | `EqualizerProcessor` | IMPLEMENTED | [R]+[P] | `playback::dsp::EqProcessor`, presets and per-band gains persisted; live via `audio_settings_set` |
| Loudness normalization | `DspChain` | IMPLEMENTED | [R] | Background analysis caches per-track gain; smoothed in the callback. No separate limiter stage |
| Audio cache | SimpleCache 512 MB | IMPLEMENTED | [R] | Remote streams use the persistent disk cache (`StreamCache`); local files read disk directly |
| Sleep timer | `SleepTimer.kt` | NOT IMPLEMENTED | | |
| Listening stats (wall-clock) | `ListeningRecorder` | IMPLEMENTED | [R] | Simplified: one history row per track start (not sampled wall-clock yet) |

## 4. Lyrics

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| Provider system: 13 providers raced in priority order, word > line > plain | `LyricsRepository.kt` → web `lyrics/` | TESTED | [V] | `repository.ts` vendored verbatim; **26 upstream vitest tests pass** (parse 15, clock 10, repository 1) |
| ISRC identification (BiniLyrics, 2.5 s cap, LRU) | same | TESTED | [V] | Covered by repository test |
| Parsers: TTML, enhanced-LRC/A2, QRC/YRC karaoke, plain LRC/text | `TtmlLyrics.kt` etc. | TESTED | [V] | `parse.test.ts` |
| Apple-style word animation (`LyricClock`, am-lyrics renderer) | `LyricClock.kt` → `clock.ts` | TESTED | [V] | `clock.test.ts`; panel wired to engine playhead via `getPlayhead()` |
| Provider settings (enable/order/precede-syllables/key) | `AppSettings` lyrics keys | TESTED | [V] | `LyricsSettings.tsx` + zustand localStorage store |
| Provider picker + per-provider states | `LyricsProviderSheet` | IMPLEMENTED | [V] | |
| HTTP layer | `LyricsHttp.kt` → extension bridge | TESTED | [P] | Rewritten: webview `fetch` for CORS-open hosts, **Rust `proxy_get`** (host-allowlisted) for the rest — replaces the upstream browser-extension bridge |
| YouTube transcript/Music providers | Innertube | BLOCKED | | Reason: no Innertube client on desktop yet (same PoToken spike as §5); stubs return "missing" honestly |
| Spotify Canvas lyric backdrop | `SpotifyCanvas.kt` | BLOCKED | | Reason: needs a Spotify user session; upstream needs it too. Code path stripped, not faked |
| Translation | Google endpoints | NOT IMPLEMENTED | | Optional upstream; off by default |

## 5. Sources & discovery

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| **`MusicSource` trait** (health/search/stream + `StreamRequest` tiers, `StreamFormat` comparison rules) | `MusicSource.kt` | TESTED | [R] | `crates/sources`: trait + types ported with the tier semantics verbatim; registry unit-tested |
| **Addon HTTP sources** (manifest/search/stream, SharedCalls dedup, 429 quiet period) | `AddonClient.kt` | TESTED | [R] | Integration-tested against a real local mock server: health Ok/Rejected/Unreachable distinctions, search rows, tiered stream, cache |
| **Internet Radio** (Radio Browser discovery, favourites, custom stream URLs, live playback) | — | IMPLEMENTED | [R]+[P] | Rust-side directory with a 10-min cache and paging, public-host validation and no HLS; `SourceKind::Radio` plays through the engine's `LiveReader` (unseekable, unbounded body). Station name/country/tags are shown as station info — no fabricated now-playing track. Favourites/recent/custom URLs persist in `localStorage`. Live station audio was not re-verified in this pre-publication run |
| **Source management UI** (add by URL with manifest validation, health dots, enable/disable, remove) | `SourcesScreen.kt` | IMPLEMENTED | [R]+[P] | Settings → Sources; registration refuses servers without a valid manifest |
| **Engine playback of remote streams** (ranged HTTP reader, no full download) | `ChunkedDataSource.kt` finding | TESTED | [R] | `playback::remote::HttpRangeReader`: 512 KiB `Range` chunks (googlevideo throttles unbounded reads); MP4 `moov` seeks answered over the wire. Hermetic test decodes a WAV served ranged-only; the sidecar round-trip decodes 5 s **through the engine path** in 4.5 s total (was 142 s downloading). Engine queue entries carry http(s) paths or `sidecar://` placeholders; resolve-ahead swaps URLs before the gapless preload needs them |
| **YouTube search + play in the UI** | SearchScreen | TESTED | [P] | Search page gains a Library/YouTube Music segmented tab (upstream's source tabs); rows play via `player_play_remote` — start track resolved immediately, queue rows resolve-ahead; Play Next/queue work on remote rows too; history skips catalogue rows (no library id) |
| Catalogue flag wired to the UI | — | TESTED | [P] | Settings → Sources toggle (`catalogue_sources`), launch fails with a named reason; unavailable state renders honestly in the search tab |
| Racing resolver (`bestAcross`, lossless upgrades, strict matching) | `SourceResolver.kt` | NOT IMPLEMENTED | | Resolution is sequential, not raced: pinned source → one transient retry → ranked fallback ladder with strict metadata matching. Racing waits for ≥2 real non-local sources |
| QuickJS modules | `ModuleManager.kt` | NOT IMPLEMENTED | | Via `rquickjs` |
| JioSaavn | `JioSaavnSource.kt` | NOT IMPLEMENTED | | |
| **YouTube Music via Kotlin sidecar** (search, track metadata, albums/playlists, lyrics, AAC stream resolution) | `:shared` Innertube/StreamResolver | TESTED | [R]+[K] | The pinned upstream Kotlin client runs as a local HTTP sidecar (GPL-3.0, touches nothing in `:shared`); Rust `YtMusicSource` supervises the JVM process (spawn → health-retry → kill-on-drop) and implements the `MusicSource` trait. Round-trip test: search → `?prefer=aac` stream → **Symphonia decodes 5 s, RMS 0.126 (non-silence)**. Live observations: itag 140/131 progressive AAC resolves in ~2 s with **no PoToken needed** on this client path |
| Catalogue feature flag | — | TESTED | [R] | `catalogue_sources` setting gates `YtMusicSource` construction (`registry::build_active`); off by default, local sources unaffected; a failed launch leaves the source out with a warning, never breaks local playback |
| Stream format selection (progressive itags) | StreamResolver itags | IN PROGRESS | | `?prefer=aac` guarantees MP4/AAC via `resolveForDownload(requireM4a)`; the default (no param) may return Opus/WebM, which this Symphonia build cannot decode — recorded below |
| HLS / DASH streaming | media3 DASH/HLS | BLOCKED | | Reason: upstream's phone player handles manifests via media3; the desktop rodio/Symphonia stack decodes progressive files only. Sidecar serves progressive itags; manifest support needs a demuxer/player change |
| Word-level lyric timings over the sidecar | `LyricLine.words` | NOT IMPLEMENTED | | Sidecar exports line-level only; word data exists upstream in `LyricLine` and can be added to the payload when the desktop lyrics UI lands |
| PoToken/BotGuard dependency | InnerTubeX TokenProvider | OBSERVED OK (this path) | | `resolveForDownload(requireM4a)` resolves without a BotGuard token today (client catalog incl. ANDROID_MUSIC/IOS paths). No guarantee it stays that way; the sidecar has no WebView minter. The round-trip test is the evidence, re-runnable |
| Racing resolver + lossless upgrades + strict matching | `SourceResolver.kt` | NOT IMPLEMENTED | | Logic documented; ports after ≥2 real sources exist to test against |
| Cross-source fallback (`youtubeFallback`) | `PlaybackFallback.kt` | IMPLEMENTED | [R] | Sequential ranked ladder in `resolver.rs`: skip the pinned source and Local, search each fallback, require a strict metadata match, then resolve |

## 6. Artwork & visual extras

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| Artwork display (artwork-stroke, radiosity glow) | sharedUi | IMPLEMENTED | [V] | Artwork + FSPlayer layers |
| Dynamic page tint | `ArtworkPalette` → `usePageTint` | IMPLEMENTED | [V] | Album/playlist pages tint from artwork (`art://` now sends CORS so the canvas can read pixels); tokens' `[data-tint]` rules vendored |
| Animated canvas (Apple/Tidal/Spotify/community) | `CanvasRepository` | BLOCKED | | Reason: third-party catalogue endpoints + sessions; upstream needs them too |
| Artist signatures / Apple artist art | canvas/* | BLOCKED | | Same reason |

## 7. Integrations

| Feature | Upstream | Status | How | Notes / test |
|---|---|---|---|---|
| **Google Sign-In** (Authorization Code + PKCE S256, loopback redirect, Keychain token) | — | IMPLEMENTED (build-time client ID) | [R]+[P] | Requires `TANSEN_GOOGLE_CLIENT_ID` at build time; without it the sign-in command returns a named “not configured” error instead of failing silently. Tokens stay in the macOS Keychain and never cross IPC |
| Last.fm scrobbling | `LastFM.kt` | NOT IMPLEMENTED | | MD5-signed web-services flow documented |
| ListenBrainz | `ListenBrainzManager.kt` | NOT IMPLEMENTED | | Token API |
| Discord Rich Presence | kizzy gateway | NOT IMPLEMENTED | | User-token gateway; opt-in only, no official API |
| Listen Together | `ListenTogether.kt` + Go server | NOT IMPLEMENTED | | Protocol documented; Go reference server exists upstream |
| Downloads + taggers | `download/*` | IMPLEMENTED | [R]+[P] | Persistent queue for Subsonic (original or server transcode), direct addon streams, and local-file copies; YouTube Music and Spotify are refused with a reason. Pause/resume/retry/cancel/clear in Settings; add from track menu or playlist. Tagging after download is not implemented |
| Spotify import/library | `SpotifyImporter.kt` | IMPLEMENTED | [R]+[P] | Public playlists import through the embed page (no user session); tracks are matched to YouTube Music and can be saved as a local playlist |
| Chromecast | `CastController.kt` | NOT APPLICABLE | | Reason: no desktop Cast SDK equivalent; upstream feature is Android/GMS-bound |
| Favorites / like status | `LikeState.kt` | IMPLEMENTED | [R]+[P] | Player heart likes tracks and albums for Home recommendations; persisted per `(source, id)`, no social counts |
| Android Auto / Wear / widgets / TV / web bridge | app module | NOT APPLICABLE | | Reason: platform-bound features with no desktop equivalent |

## 8. Settings coverage

| Upstream setting group | Status | Notes |
|---|---|---|
| Playback (repeat/shuffle/autoplay/gapless/crossfade/loudness/skip-silence/speed/automix) | IN PROGRESS | Repeat, shuffle, autoplay, gapless, crossfade (linear/equal-power), loudness, and skip-silence are live; speed and automix are pending; every visible control works or says so |
| Audio output (output depth/device/loudness/Atmos/spatial) | PARTIAL | 16/32-bit output depth is selectable and rebuilds the stream; output device follows the OS default (no in-app picker); Atmos/spatial are not implemented |
| Equalizer | IMPLEMENTED | Ten peaking bands with presets, persisted and applied live in the callback |
| Downloads / cache | IMPLEMENTED | Persistent download queue for Subsonic/addon/local; remote stream disk cache. See §3 and §7 |
| Sources/network | IMPLEMENTED | Add-by-URL with manifest validation, health, enable/disable, remove; catalogue off by default |
| Appearance (theme/liquid glass/canvas/nerd stats) | PARTIAL | `showNerdStats` stored (panel pending); upstream theme is media-query based (kept); liquid-glass/canvas toggles pending with their features |
| Lyrics (sources/order/offset/translation) | IMPLEMENTED | Provider order/enable/prefer-syllables/key live; per-track offset pending |
| Library (filter non-music, sort, folders) | IN PROGRESS | Folder management + **watch-folders toggle** live; non-music filter (ringtones etc.) pending |
| Integrations | NOT IMPLEMENTED | Last.fm, ListenBrainz, Discord, Listen Together, and Chromecast are not implemented |
