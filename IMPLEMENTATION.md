# Tansen — Implementation & Architecture

Tansen is a local-first desktop music player for **macOS and Windows**. Rust owns
state, audio and background work; React owns what you see. A local Kotlin
sidecar supplies YouTube Music catalogue and streams. This document explains the
whole system end to end: the stack, every module and file, how they connect at
runtime, and how each feature works. The companion `REPORT.md` records what is
done and what is not; `docs/feature-matrix.md` is the per-feature status table.

---

## 1. The stack, and why

| Layer | Technology | Why |
|---|---|---|
| Shell | **Tauri 2** | Native window, small binaries, Rust backend, web UI. Chosen over Electron for size and native integration. |
| Core | **Rust** (edition 2021, stable) | Audio, scanning, storage, sources — one language for all non-UI work. |
| UI | **React 19 + TypeScript** (strict), **Tailwind CSS v4** | Component model + design tokens as CSS. The UI is vendored from BitChord's own GPL web player, so React was fixed by that choice. |
| State | **zustand** stores, **TanStack Query v5** for server-ish data | Small, selector-based; Query handles caching/retry for page data. |
| Audio | **rodio + Symphonia** (cpal underneath) | Pure-Rust decode of MP3/FLAC/MP4-AAC/OGG/WAV/AIFF/WavPack/APE; rodio owns the output device. |
| HTTP | **reqwest** (rustls) — async + blocking clients | Sources, artwork protocol, range-streaming. |
| Storage | **SQLite** via rusqlite (bundled), WAL mode | One file, versioned migrations. |
| Metadata/artwork | **lofty** | Tags + embedded pictures for every supported codec. |
| OS media keys | **souvlaki** | Now Playing (macOS), SMTC (Windows), MPRIS (Linux) behind one API. |
| File watching | **notify** | Library auto-rescan. |
| YouTube Music | **Kotlin sidecar** (JDK HttpServer) over upstream's `:shared` client | Reuses the working, GPL Kotlin Innertube/StreamResolver code instead of rewriting a fragile protocol. |
| Build | Cargo workspace + pnpm + Vite 7 | Standard Tauri layout. |

License: **GPL-3.0-or-later** — required because design and logic are
translated from BitChord (GPL-3.0) and the UI is vendored from its web player.
The Tansen name and app mark are original.

---

## 2. Repository layout

```
tansen/
├── apps/desktop/            the Tauri application (UI + shell)
│   ├── src/                 React UI (see §5)
│   └── src-tauri/           Tauri shell: commands, event forwarding, art protocol (see §4)
├── crates/                  the Rust core, five crates (see §3)
├── docs/                    audit of upstream, architecture, feature matrix, plan, sidecar API
├── tests/fixtures/          committed audio fixtures (a generated WAV)
└── README.md
../BitChord/                 upstream checkout @ v1.8.1 tag + sidecar/ module (sibling on purpose;
                             the sidecar builds against :shared and the app discovers it at ../BitChord)
```

Two workspace facts worth knowing:

- `apps/desktop/src-tauri` is a workspace member; the frontend is one npm
  package (`apps/desktop/package.json`).
- `../BitChord/sidecar/build/libs/sidecar-all.jar` is the only artifact the app
  needs from the upstream tree; discovery order is
  `BITCHORD_SIDECAR_JAR` env → `sidecar_jar` setting → exe-relative/cwd candidates.

---

## 3. The Rust core (`crates/`)

Five crates, one dependency direction. Role map (M1 baseline):
**core = app-core · audio = playback · library = library · catalogue = sources · app = persistence** (app data/state).

```
app-core ──► playback, sources, library, persistence
playback ──► persistence (TrackRef model only)
sources ──► persistence
library ──► persistence
persistence   (SQLite; depends on nothing else of ours)
```

The Tauri shell (`apps/desktop/src-tauri`) depends on all five. Nothing inside
`crates/` knows Tauri exists.

### 3.1 `persistence` — schema + repositories

| File | Role |
|---|---|
| `src/lib.rs` | `Db` (one `Connection` behind a `Mutex`), WAL + foreign-key pragmas, ordered migrations keyed on `PRAGMA user_version`. M11: `Db::open_read_only` opens additional `SQLITE_OPEN_READ_ONLY` connections to the same WAL database — queries run there never contend with the writer; writes through the handle fail at the SQLite level. |
| `src/repo.rs` (M11) | `tracks_by_ids(db, ids)`: chunked (500) parameterized `IN` query returning rows in input order, duplicates collapsed, unknown ids skipped — the queue-rows query behind the new `queue_tracks_get` command, which the frontend drives when the engine publishes a new `queue_rev`. The engine itself touches no database. | Migration 1: settings, library_folders, tracks, playlists, playlist_tracks, play_history. Migration 2: playback_state (the recoverable queue). |
| `src/models.rs` | `Track` (a library row), `TrackRef` (what the queue/player need: id, title, artist, album, duration, art path, path), `Folder`. |
| `src/repo.rs` | Typed queries: settings get/set, folders, track upsert-by-path (rescan-idempotent), mark/purge missing, playlist CRUD + append + reorder, history insert. |
| `src/views.rs` | Read models: `albums()` / `artists()` groupings (SQL, with the album_artist-fallback rule), `album_tracks()`, `artist_tracks()`, `history()`, and `playback_state` save/load/clear. |
| `tests/views.rs` | Album/artist grouping rules + playback_state round-trip. |

Connection rules: short transactions only; scanning/artwork/decoding never hold
the lock; `user_version` migrations are append-only.

### 3.2 `playback` — audio engine + queue model

| File | Role |
|---|---|
| `src/remote.rs` | `HttpRangeReader`: blocking `Read + Seek` over 512 KiB `Range` chunks (offline test oracle + probe). `PrefetchReader` (M15): the production http(s) path — a dedicated fetch task owns all network I/O and tops a bounded 4 MiB buffer (refill below 512 KiB); the audio callback pops from memory only and never performs a network request. Mid-stream stalls surface on a tokio watch as `StreamStatus::Starved` (engine → `Buffering`) and refill as `Refilled` (→ `Playing`). A seek drops the buffer and issues a fresh Range request at the target offset. M16: a 403/410 (expired or revoked URL) is terminal — the fetch task parks and reports `UrlDead`; the engine re-resolves once, rebuilds at the death position (Resolving→Buffering→Playing), and a second failure surfaces `Error(Resolve)`. `RodioFactory::prepare` also refuses media already past `ResolvedMedia.expiry` upfront. |
| `src/cache.rs` | `StreamCache`: bounded (512 MiB LRU) disk cache for streamed tracks, sha1-of-URL keyed, complete-file-only. The engine plays cached copies from disk; the resolve-ahead service prefetches the next track into it. |
| `src/probe.rs` | `decode_rms(reader, window)`: Symphonia decode without a device — the test oracle for "this stream contains sound". |
| `src/metrics.rs` | Audio reliability counters (M4, debug builds only): cpal stream-error count (via `OutputStreamBuilder::with_error_callback`), decoder read-latency histogram (`MeasuredSource` wraps every decoder in the engine's `append`), slowest read, quantized p50/p99; `TANSEN_STALL_MS`/`TANSEN_STALL_EVERY` env hook sleeps on rodio's feeder thread mid-track. Release passes decoders through untouched. |
| `src/pipeline.rs` + `src/ring.rs` | The rodio-backed pipeline the M6 `PipelineFactory` drives. **M19**: a dedicated decode thread per (track, seek epoch) runs Symphonia and fills a lock-free **SPSC ring buffer** (`ring.rs`, UnsafeCell slots behind Release/Acquire cursors — the audio callback pops with atomics only, applying volume from an f32-bits atomic and incrementing `frames_played` per frame; **no locks, allocations, or I/O in the callback**). The stream pipeline is immutable per (track, seek epoch): a load or seek builds a fresh decode thread + ring and swaps the consumer into the sink; the old epoch is shut down via a shared flag (a parked PrefetchReader wakes through its own shutdown `Arc`). Underflow emits silence and flags Starved → `Buffering` (M15 watch); refill → `Refilled` → `Playing`. Volume changes are a single atomic store. `FrameCounted` wraps every appended decoder and bumps `frames_played` (lock-free `AtomicU64`, one increment per channel-group — no locks, no allocation on the feeder path); `pos_ms()` derives from frames ÷ sample rate. Parallel path: production still runs `engine.rs` until M10 removes it. |

M10 removed the legacy rodio engine and its pure queue algebra
(`engine.rs`, `queue.rs`): playback state lives in the app-core task engine,
which drives `pipeline` through the `PipelineFactory` seam. The shell's
watch→event task carries metadata to the webview, the OS and `play_history`.

### 3.3 `library` — folders to rows

| File | Role |
|---|---|
| `src/metadata.rs` | lofty extraction → `Track` (title falls back to filename), codec naming, extension whitelist. |
| `src/art.rs` | Embedded picture → `art/<sha256(path+mtime)>.<ext>` content-hash cache; unchanged files never rewrite; `prune_orphans`. |
| `src/scanner.rs` | walkdir over registered folders (bounded by rayon) → **stat compare** (M12: stored `mtime_sec`+`size_bytes` vs the file — unchanged rows skip tag parsing entirely; returned files un-mark via upsert as before) → parallel tag parse for changed files only → **batched writes** (200 files per `upsert_batch` transaction, replacing two autocommits per file) → mark-missing (kept, not deleted) → artwork. Progress callback every 25 files. Runs off the UI/engine: `spawn_blocking` in the shell command, watcher's own thread, never on the engine. |

### 3.4 `sources` — the catalogue abstraction

| File | Role |
|---|---|
| `src/lib.rs` | The `MusicSource` trait (`health` / `search` / `stream` / **`resolve`**), `StreamRequest` (Lossless / Capped / Best — upstream semantics), `StreamFormat` (lossless/atmos/bitrate + `is_better_than` comparison), `SourceHealth` (Ok / Unreachable / **Rejected** — a down server is retryable, a refusing one is not), `CatalogTrack`, `SourceKind` (Addon / YouTube, ranked). M13: `resolve` is a default method on the existing trait (no new trait) returning **`ResolvedMedia { url, headers, expiry, length, mime }`** — the formalized URL-handoff contract (the sidecar hands Rust a URL, never bytes; M3). `SourceStream` carries `mime` + `expiry_ms` where the source states them (sidecar `container`/`expiresAt`); byte length stays unknown until a provider reports it. |
| `src/addon.rs` | HTTP addon adapter: `manifest.json` / `search` / `stream/{id}` with SharedCalls-style dedup (10/10/5 min) and a 429 quiet period. Integration-tested against a dependency-free TcpListener mock. |
| `src/registry.rs` | `SourceConfig` list persisted in settings (`sources_config`), kind+rank ordering, `build_active()` builds enabled addon sources; the YouTube source joins only when `catalogue_sources` is on and the sidecar launches. |
| `src/yt.rs` | `YtMusicSource`: spawns `java -jar sidecar-all.jar --port <free>`, health-polls (400 ms × 20 s) on a scoped OS thread (blocking client — never inside a runtime), kills the JVM on drop. Java discovery rejects the macOS stub (`/usr/bin/java` runs but exits 1 — success means exit 0). Translates the sidecar JSON contract to the trait. M14 supervisor: `ensure_running` gates every call — a dead JVM restarts through exponential backoff (500 ms doubling to a 30 s cap, reset by one healthy check; inside the window callers get an immediate error, never a spawn storm). Per-call timeouts: 20 s HTTP / 3 s health. Spawn measured at 476 ms (M3), so the supervisor does NOT idle-shutdown — the JVM persists for the process lifetime. |
| `tests/addon_source.rs`, `tests/yt_sidecar.rs` | Mock-server integration tests; the ignored-by-default round-trip (network) proves search → stream → decode 5 s via the engine's ranged reader. |

### 3.5 `app-core` — the application brain

| File | Role |
|---|---|
| `src/state.rs` | `AppState`: owns Db, Scanner, Media, scan_events channel, cached `YtHandle` (folders/tracks/scan/playlists/settings/yt). M10 removed the playback surface — the engine lives in the shell, restore moved to the frontend mirror. `migrate_legacy_data_dir()` renames pre-0.3.0 `bitchord-desktop` → `tansen` once. |
| `src/playback_types.rs` | Normalized playback types (M5, pure — no I/O, no Tauri, no serde yet): `EntryId` (queue identity, never an index), `Status` {Idle/Resolving/Buffering/Playing/Paused/Error(kind)}, `Anchor` {pos_ms, rate 0\|1}, `PlaybackState` {rev, status, entry, queue_rev, anchor, volume}, `Command` {Play/Pause/Resume/Next/Prev/Seek/SetVolume/Enqueue{tracks, placement}}, and `validate_transition` — the pure law for status changes (explicit edge graph + the anchor rule: the clock runs only while `Playing`). Groundwork for a future engine rework; nothing calls it yet. |
| `src/playback_engine.rs` | The task-based playback engine (M6): one tokio task owns the `PlaybackState`; commands arrive on a bounded mpsc (64, backpressure), every published change goes out on a `tokio::sync::watch` with `rev` +1. `PipelineFactory` is the only seam (`prepare`/`release`; a fake backs the tests — no real audio). The engine also holds the play-order queue (storage only — addressing stays by `EntryId`) and walks the M5 edge graph, refusing illegal commands silently. A bounded journal records every publish (watch coalesces; tests assert over the complete history). M7: `PipelineFactory` grew `transport` (Play/Pause/Seek/Volume); M8 added `position_ms`/`duration_ms` reads and the **one-shot end-of-track deadline**: armed once when Playing with a known duration, confirmed against `position_ms` (frames) on fire — advance to the next entry, re-arm if the anchor moved, cancel on pause/seek/skip — never periodic. Queue changes (body *and* cursor moves) bump `queue_rev`; the body itself never enters `PlaybackState`. The real factory lives in the `engine_demo` debug binary. |
| `src/media.rs` | souvlaki wrapper: created/attached on the main thread (macOS requirement), updated from any thread, OS pushes throttled to 2 Hz. |
| `src/history.rs` | Plain thread over engine events; one `play_history` row per local track start (`id > 0`). |
| `src/persist_state.rs` | Plain thread; flushes queue/index/position/repeat to `playback_state`. |
| `src/watcher.rs` | notify watcher over library roots: 2 s debounce, initial scan on subscribe (pre-existing files fire no events), resubscribes when the folder list changes, honors the `watch_folders` setting, yields when a manual scan holds the lock, reports summaries on `scan_events`. |
| `src/remote.rs` | Placeholder helpers + resolve-ahead thread: on `QueueChanged`, resolves the next 2 `sidecar://` rows via the sidecar, patches the engine, then prefetches the immediate next track into the stream disk cache. 1 h URL cache. |
| `src/yt_handle.rs` | `YtHandle` (sidecar launched once per process) + `RemoteTrack`/`PlayMode`/`CatalogRow` DTOs + `await_sync` helper (one-shot runtime; callers are sync commands / plain threads). |
| `tests/restore_playback.rs` | Restart persistence end-to-end (needs an audio device; skips gracefully). |
| `tests/library_watcher.rs` | Watcher integration: add file → indexed; setting off → stops. |

---

## 4. The Tauri shell (`apps/desktop/src-tauri`)

| File | Role |
|---|---|
| `src/main.rs` | Tracing init → legacy data-dir migration → Builder: dialog plugin, `art://` protocol (serves cached artwork bytes, CORS header so the canvas can read pixels), setup (AppState::open, restore, macOS Now Playing via `platform_macos`, event forwarders), 30+ registered commands. The lyrics proxy disk-caches cacheable provider responses for 24 h (token endpoints excluded). M9: the task-based playback engine also starts here (rodio `PipelineFactory` behind it), every watch change is forwarded to the frontend as the `playback_state` event, and `engine_command` rides beside the legacy surface. |
| `src/platform_macos.rs` | macOS Now Playing + remote commands via MediaPlayer (`objc2` family — the only allowed dependency, replacing souvlaki). Subscribes to the engine watch: Now Playing updates **only on rev changes** (elapsed = anchor position, rate = anchor rate; MediaPlayer interpolates — no timers). Remote commands (toggle/play/pause/next/prev/skip ±10 s/scrub) map 1:1 onto `Command` into the engine channel. M18 output monitor: a 2 s poll of the default device name + NSWorkspace sleep/wake notifications, all routed into the engine as **Commands** (`Pause`, `RebuildOutput`) — never state mutation. `#[cfg(target_os = "macos")]`. |
| `src/commands.rs` | The whole command surface (§7). Each is thin: call app-core, map errors to strings. Async commands use `spawn_blocking` for anything blocking. |
| `tauri.conf.json` | Product `Tansen`, identifier `app.tansen.desktop`, window/title, CSP, dev/build commands (`pnpm dev` / `pnpm build`), bundler config. |
| `capabilities/default.json` | Core event/window permissions + dialog. |

Event forwarders (tasks started in setup):

- **player://event** — every `EngineEvent` → the webview, and → souvlaki
  (metadata on TrackChanged, position at 2 Hz).
- **scan://progress / scan://done** — manual scans and watcher scans alike, so
  library views refresh.
- **`art://` protocol** — request path is checked against the art cache
  directory, then served with mime + cache headers.

---

## 5. The React UI (`apps/desktop/src`)

### 5.1 Design system (vendored from the upstream web player, GPL-3.0)

- `styles/tokens.css` — the full token contract: type scale (SF-style sizes),
  spacing, radii, motion easings, z-layers, light/dark via
  `prefers-color-scheme`, `[data-tint]` page-tint overrides, `[data-full-screen]`
  on-dark mapping, `prefers-contrast` handling.
- `styles/global.css` — Tailwind v4 glue: `@theme inline` token bindings,
  custom utilities (`type-*`, `glass-material`, `glass-pill`, `artwork-stroke`,
  `hairline-*`), no-preflight reset, `@font-face` for DM Sans (self-hosted,
  OFL) and the lyrics face.
- `components/icons/LogoMark.tsx` — original three-bars mark (BitChord's logo
  is not used).

### 5.2 Shell

```
AppShell (grid rows: content / player pill)
├── Sidebar          glass nav: Search/Home/New + Library sections + Playlists + Settings
├── MobileTopBar     <484 px drawer bar
├── main#scrollable-page   the one scroll container (routes render inside)
├── PlayerBar        floating glass pill: Lcd (artwork→fullscreen, title/artist links,
│                    2 px seek line w/ hover times) + transport + VolumeControl + panel toggles
├── SidePanel        Up Next ⇄ LyricsPanel (slide-in)
└── FullScreenPlayer fixed overlay: MeshBackdrop (artwork palette), artwork column
                     (radiosity glow), lyrics/queue columns, Scrubber, transport
```

| File | Notes |
|---|---|
| `shell/useScrubber.ts` | rAF scrubber; reads `getPlayhead()` (engine ticks interpolated) — no HTMLAudioElement exists on this backend. |
| `shell/useKeyboardShortcuts.ts` | Space, ⌘←→↑↓, ⌘F, /, Esc. |
| `shell/LyricsPanel.tsx` | `<am-lyrics>` web component fed TTML; `LyricClock` from `lyrics/clock.ts` steered by the playhead; provider picker + settings dialog. |
| `shell/UpNext.tsx` | Queue list with drag-to-move, clear, per-row menus. |
| `shell/MeshBackdrop.tsx` | 16-cell artwork palette (lib/meshPalette.ts) animated as the backdrop. |

### 5.3 State

- `state/player.ts` — the player store. Mirrors the upstream store contract
  (flat fields the shell reads) but is backed by the task engine: actions are
  `engine_command` calls; `playback_state` events mutate it;
  `getPlayhead()` interpolates the published anchor locally
  (`pos_ms + (now − receivedAt) × rate` — no polling). The store keeps the
  queue mirror (the body never crosses IPC) and persists it to localStorage
  for restart restore. Repeat/shuffle persist as preferences; queue editing
  awaits queue-edit commands; catalogue rows report the honest unavailable
  state until remote lands on the new path.
- `state/ui.ts` — side panel, fullscreen, drawer, banner flags (upstream file).
- `state/video.ts` — stub (video is a catalogue feature, honestly absent).
- `lib/queryClient.ts` + `pageRegistry.tsx` + `routes.tsx` — lazy routes with
  hover-preload; pages: `/home`, `/new`, `/search`, `/album/:id`, `/artist/:id`,
  `/playlist/:id(+/reorder)`, `/library/:section`, `/settings`.

### 5.4 Pages & components

| Path | What it does | Backed by |
|---|---|---|
| `pages/HomePage.tsx` | Recently added albums + songs shelves | `q.tracks()` → ShelfView |
| `pages/LibraryPage.tsx` | Songs/Albums/Artists/Playlists/Recently Played + import/rescan/progress/folders | `tracks_get`, `albums_get`, `artists_get`, `history_get`, `scan_*` events |
| `pages/AlbumPage.tsx` / `ArtistPage.tsx` | DetailHeader + TrackList; page tint from artwork (`usePageTint` + `useArtworkColor`) | `albums_get`, `album_tracks_get`, `artist_tracks_get` |
| `pages/PlaylistPage.tsx` / `ReorderPlaylistPage.tsx` | Create/rename/delete/play; dedicated drag-reorder page (original's pattern) with `lib/reorder.ts` (unit-tested) | playlist commands |
| `pages/SearchPage.tsx` | Library/YouTube Music segmented tabs; library: albums/artists shelves + songs; YouTube: `yt_search` rows, honest unavailable state | local index + sidecar |
| `pages/SettingsPage.tsx` | Playback (repeat/shuffle/crossfade pref), Library (folders, watch toggle), Sources (catalogue toggle + addon management + health dots), Lyrics (provider dialog), Integrations (honest not-implemented), About | settings + sources commands |
| `components/music/TrackList.tsx` | Virtualized track rows (album/playlist variants), hover play, ⋯ menus | `play.ts` → store |
| `components/music/TrackMenu.tsx` | Play Next / Add to Queue / Add to Playlist (picker + create) | playlist commands |
| `components/music/Artwork.tsx` | `art://` local / https remote, fade-in, retry | lib/artwork.ts |
| `lib/rows.ts` | Backend row types + Track mapping + query definitions | — |

### 5.5 Lyrics system (vendored, tests included)

```
useLyrics (react-query) ─► repository.ts ─► providers.ts (13 providers, raced)
                                │              ├─ parse.ts (TTML/LRC/QRC/A2/plain)
                                │              ├─ http.ts (fetch ⇄ Rust proxy_get allowlist)
                                │              └─ sources.ts + settings.ts (order/enable)
                                └─ result {lines, ttml, wordSynced}
LyricsPanel ─► <am-lyrics> + LyricClock (clock.test.ts) ─► word/line animation
```

Selection rules (upstream's): word-synced beats line-synced beats plain; priority
order honored; ISRC identify pass (2.5 s) before matching; Genius lazy.

---

## 6. The Kotlin sidecar (`../BitChord/sidecar/`)

Upstream checkout pinned to the **v1.8.1 tag**; `:shared` is untouched. The
sidecar is a Gradle module: JDK `HttpServer`, bound to **127.0.0.1 only**,
port from `--port` / `BITCHORD_SIDECAR_PORT` / 8765, one log line per request,
SIGTERM-clean.

| File | Role |
|---|---|
| `sidecar/build.gradle.kts` | Kotlin JVM 17 + fat jar task (`sidecar-all.jar`); supplies NewPipeExtractor (compileOnly upstream) itself. Built from `sidecar-overlay/` in this repo via `scripts/build-sidecar.sh`. |
| `src/main/kotlin/sidecar/Main.kt` | Routes: `/health`, `/search`, `/track/{id}`, `/track/{id}/stream[?prefer=aac]`, `/album/{id}`, `/playlist/{id}`, `/lyrics/track/{id}`. `prefer=aac` guarantees a progressive MP4/AAC via `resolveForDownload(requireM4a = true)`; without it, `StreamResolver.resolve` + mime sniff (one zero-byte ranged GET) + itag from the URL. |
| `src/main/kotlin/sidecar/Json.kt` | `Song`/`SearchResult`/`SongPage`/`LyricLine` → wire JSON (the contract in docs/sidecar-api.md). |

Runtime chain for one stream:
`YtMusicSource.stream → sidecar /track/{id}/stream?prefer=aac → StreamResolver.resolveForDownload → InnerTubeX client catalog (+ NewPipe fallback) → googlevideo URL → HttpRangeReader → Symphonia → rodio sink`.

**The sidecar hands Rust a URL, never audio bytes.** All six HTTP routes
(`/health`, `/search`, `/track`, `/album`, `/playlist`, `/lyrics`) answer
JSON; the stream route serializes the resolved googlevideo URL as a string
(`Main.kt:115`, `put("url", JsonPrimitive(stream.url))`). There is no
byte-proxying route. Rust opens the URL itself through `HttpRangeReader`
(512 KiB ranged GETs, moov seeks answered over the wire) — the JVM never
touches audio payloads.

### Sidecar measurements (M3, `#[ignore]` test `yt_measure.rs`)

Measured on the app's own spawn path (`YtMusicSource::launch`, macOS arm64,
dev jar, network live — 2026-10-31):

| Metric | Value |
|---|---|
| Spawn → ready (`launch()` incl. health poll) | 476 ms |
| Sidecar JVM RSS at ready | 79 MiB |
| Sidecar JVM RSS after 30 resolves + search | 147 MiB |
| Resolve latency (`/track/{id}/stream?prefer=aac`), 30 calls | cold 1728 ms, **p50 1837 ms, p95 2358 ms**, max 3172 ms |
| First play (cold resolve → ranged reader open → first 0.25 s decoded audio) | 4233 ms total (resolve 1822 + reader open 2412) |

Readings: JVM boot is cheap (~0.5 s incl. one 400 ms health-poll step), but
**every resolve costs ~1.8 s** — the sidecar does a fresh Innertube round
trip per stream, which is why the resolve-ahead service patches URLs and
prefetches the next track while the current one plays. The reader-open phase
(moov over ranged HTTP) is the other half of first-play latency. Percentiles
are nearest-rank over the sorted 30 samples. Test scope excludes Tauri IPC,
queue wiring and UI; the engine-path decode is exercised through the same
`HttpRangeReader` + `probe` the player uses.

---

## 7. The command surface (Tauri ⇄ React)

Groups (31 commands after M10): settings, library (folders/scan/tracks), playlists (CRUD + reorder), views (albums/artists/album_tracks/artist_tracks/history), lyrics proxy (`proxy_get`, host-allowlisted), catalogue (`yt_search`), sources (CRUD + health), bench marker, and `engine_command` — the one bridge into the task-based playback engine (`Command` over serde).

Events back: `playback_state` (every engine publish), `scan://progress`, `scan://done`. The legacy `player://event` broadcast is gone.
`playback_state` (M9: every change of the task-based engine's watch channel;
payload is the full `PlaybackState`).

---

## 8. Feature walkthroughs (how it works at runtime)

- **Import a folder** — dialog → `folder_add` (canonicalize, upsert folder row)
  → `scan_start` (spawn_blocking → rayon scan → progress events) → tracks +
  artwork cached → UI refreshes on `scan://done`.
- **Play a local track** — double-click → `player_load(ids, start)` → TrackRefs
  from DB → `QueueOp::Load` → engine opens the file → TrackChanged events →
  UI + OS media controls.
- **Play a YouTube row** — `player_play_remote` → sidecar resolves the start
  track (~1–2 s) → queue rows carry `sidecar://` placeholders → resolve-ahead
  patches the next 2 → engine's gapless preload opens them through
  `HttpRangeReader` → ranged fetches at line rate.
- **Lyrics** — TrackChanged → useLyrics query → raced providers → TTML into
  `<am-lyrics>` → LyricClock follows the interpolated playhead → sweep/word
  animation only when the data is word-timed.
- **Restart** — persist_state thread wrote the queue ≤0.8 s after every change;
  `restore_playback()` re-queues it paused at the saved position.
- **File changes on disk** — watcher debounce → scan → `scan://done` → views
  refresh; missing files marked (not deleted).

---

## 9. Platform notes

- **macOS** (current dev platform): verified end to end, incl. Now Playing
  integration and data-dir migration.
- **Windows**: no Windows host has run the build yet. Nothing is
  macOS-specific in the code paths (souvlaki handles SMTC; paths use
  `dirs_data_root()` which reads `%APPDATA%`); the sidecar needs a Java 17
  runtime on Windows. Pending item, tracked in REPORT.md.
- **Linux**: builds in principle (souvlaki/MPRIS), untested, unsupported
  officially.

---

## 10. Testing strategy

| Suite | Count | What it proves |
|---|---|---|
| `cargo test` (playback) | 6 | Range reader read/seek/length, decode of ranged-only audio, StreamCache disk replay + LRU eviction, metrics histogram bucketing, pipeline frame counting (frames ≠ samples). The legacy queue algebra (10 tests) retired with the old path in M10 |
| `cargo test` (library/persistence/sources) | 10 | Migration-2 round-trip, album/artist grouping rules, scanner e2e on a real WAV incl. rescan dedupe + missing marking, addon protocol vs mock server, registry ordering |
| `cargo test -p sources --test yt_sidecar -- --ignored` | 1 | The live path: sidecar launch → search → `prefer=aac` stream → 5 s decode through the ranged reader (needs network + built jar) |
| `cargo test -p sources --test yt_measure -- --ignored` | 1 | M3 sidecar measurements: spawn→ready, JVM RSS, resolve p50/p95 over 30 calls, first-play latency (resolve → ranged reader → first audio). Measurement only, no assertions beyond sanity |
| `cargo test` (app-core) | 21 | Watcher phases (1), playback-type transition law (6), playback task engine (14: lifecycle walk, refused commands, factory failure→Error→recovery, rate law, queue walking + queue_rev-on-any-change, volume clamp, **3 seeded-random 200-command sequences** (std LCG) asserting rev monotonicity + legal edges + rate rule, validator self-check, ClearQueue stops+empties+rebuilds, and 5 **paused-clock** deadline tests) — the old restore integration test retired with the old path |
| `pnpm test` (vitest) | 32 | Lyric parsers (15), LyricClock (10), repository selection (1), reorderMove (6) |
| `pnpm build` | — | tsc strict + production bundle |
| `cargo test -p library --test scan_timing -- --ignored` | — | M12 timing on 100k real synthetic files: batched insert, `list_tracks` (the search data path), `tracks_by_ids`, and a full unchanged rescan (stat compare only). Measured 2026-10-31: insert 9.8 s, list 0.97 s median, by-ids 11 ms median, rescan 1.41 s median |
| `scripts/bench/run.sh` | — | M1 baseline harness: release build, 5 runs of start→first-paint, memory footprint (app process; sidecar JVM when running), 60 s idle and 60 s playing CPU (median table). Env-gated autoplay hook, no behavior change. |
| M20 re-measurement | — | All four probes re-run at the M-series end (REPORT.md M20 section): paint 498→338 ms, idle CPU 0.74→0.06 %, playing 2.03→1.64 %, memory 49/49→31/39 MiB; stress 300 s stream_errors=0 with the M19 decode-off-callback layout; sidecar spawn/RSS ≈ unchanged, resolve variance 4× (network-dominated). |
| post-M10 security remediation | 8 | TAN-01 redirect-policy tests (4, local fixtures), TAN-02 enqueue-cap + TAN-10 non-finite-volume engine tests (3), TAN-03 decoder-panic containment test (1) — see REPORT.md remediation section |

Manual smoke: app launch, migration log, UI render (browser DOM capture).

### Output-device & sleep/wake behavior (M18)

Chosen behavior, all routed through engine `Command`s (the monitor thread
never mutates state):

- **Default device changes** (switch or removal while another device is
  default): `RebuildOutput` — the engine re-resolves the current entry and
  rebuilds the pipeline, then resumes **Playing** at the anchor position.
- **Active device removed** (headphones unplugged and nothing replaces the
  old default): `Pause` + `RebuildOutput` — playback pauses at the anchor
  and the stream is rebuilt on the remaining device; a later Play resumes
  at the anchor.
- **System sleeps**: `Pause`. **System wakes**: `RebuildOutput` — resumes
  at the anchor if it was playing, stays paused otherwise.
- **Nothing playing**: all three events are no-ops.
- Rebuild failure (e.g. no device at all) surfaces `Error(kind)`; recovery
  is any user-initiated Play/Next/Prev.

Detection: a 2 s poll of the default output device name (via cpal through
rodio — no new dependency) plus NSWorkspace will-sleep/did-wake
notifications (objc2-app-kit, promoted from the lockfile).

Post-M10 security remediation (audit at `edcb60e`, no new dependencies):
`proxy_get` re-validates every redirect hop against the same allowlist
predicate and double-checks the final response URL (`commands.rs`);
`Enqueue` refuses payloads above 10 000 tracks and non-finite volumes are
ignored (`playback_engine.rs`); the media pipeline contains decoder panics —
mid-decode panics on the cpal callback thread end the track instead of
aborting the process, and load-path panics reach the normal error ack
(`pipeline.rs`); `sources_get`/`source_reorder` (no callers) are
unregistered; `scan://progress` carries the folder id, not the path; a
`devCsp` mirrors production CSP plus the dev origin and HMR socket.
Debug end-to-end playback exercise (M7): `cargo run -p tansen --bin engine_demo`
— the app-core Engine + rodio `PipelineFactory` over local WAVs, printing every
published state and the `frames_played` counter through play/pause/resume/
seek/volume. The factory implementation is shared with the app shell
(`src/rodio_factory.rs`); M9 started the same engine inside the app.

---

## 11. UI ↔ backend sync inventory (M2)

Every IPC surface, its caller, and what it touches. **Hot** = fires ≥ 1/s
during normal playback or a running scan; **cold** = startup/user-action
frequency. Measured by inspection (grep over `src/` and `src-tauri/`) plus the
per-command counters below; the counters' first 10 s window is quoted where it
adds evidence.

### Commands (45 registered)

| Command | Caller | Hot/Cold | Reads | Writes |
|---|---|---|---|---|
| `settings_get` | SettingsPage (3 queries), lyrics settings | cold | settings table | — |
| `settings_set` | SettingsPage toggles (`watch_folders`, `catalogue_sources`) | cold | — | settings; watcher re-subscribes on `watch_folders` |
| `folder_add` / `folder_remove` | LibraryPage | cold | fs | folders |
| `folders_get` | LibraryPage, SettingsPage | cold | folders | — |
| `tracks_get` | HomePage/lib queries | cold (page mount) | tracks | — |
| `scan_start` | LibraryPage import/rescan | cold (emits hot events while scanning) | fs tree | tracks + artwork cache; emits `scan://progress`/`scan://done` |
| `player_load` | `player.playTracks` | cold (user) | queue | queue (`QueueOp::Load`) → TrackChanged events |
| `player_toggle` | store + Space key | hot (user) | — | engine playing |
| `player_pause` / `player_resume` / `player_stop` | **none registered callers** — reserved surface | — | — | engine playing |
| `player_next` / `player_previous` | store next/previous | hot (user; auto-advance is engine-internal) | queue | queue index |
| `player_seek` | scrubber, ⌘←→ | hot while dragging | — | engine position |
| `player_volume` | volume/mute + re-applied at startup | cold per change | — | engine volume |
| `player_repeat` | cycleRepeat + re-applied at startup | cold | queue | queue repeat |
| `queue_snapshot` | store `refresh()` (once at startup) | cold | queue | — |
| `queue_insert` / `queue_remove` / `queue_jump` / `queue_move` / `queue_clear` | Up Next actions, Play Next menus | cold (user) | queue | queue → QueueChanged event |
| `playlist_create` / `playlist_rename` / `playlist_delete` | TrackMenu, PlaylistPage, LibraryPage | cold | — | playlists |
| `playlists_get` | Sidebar, TrackMenu, Playlist, Library, Reorder pages | cold | playlists | — |
| `playlist_tracks_get` / `playlist_tracks_set` / `playlist_add_tracks` | Playlist + Reorder pages, TrackMenu | cold | playlist_tracks | playlist_tracks |
| `albums_get` / `artists_get` / `album_tracks_get` / `artist_tracks_get` | page queries | cold | tracks (SQL grouping) | — |
| `history_get` | Recently Played page | cold | play_history | — |
| `proxy_get` | lyrics `http.ts` (13 providers) | **hot** — 21 calls in the counters' first 10 s window; 24 h disk cache per URL | lyrics disk cache, network | lyrics cache files |
| `bench_marker` | main.tsx first-paint | once | — | — |
| `yt_search` | SearchPage YouTube tab | cold | sidecar → external | — |
| `player_play_remote` | store `playRemoteCommand` | cold | sidecar | queue (`sidecar://` placeholders) + resolve-ahead |
| `sources_get` / `source_reorder` | **no UI callers** — registered, reserved | — | sources_config | sources_config |
| `sources_get_with_health` | SettingsPage | cold | sources_config + live health checks | — |
| `source_add` / `source_remove` / `source_set_enabled` | SettingsPage | cold | manifest network | sources_config |

### Events and protocols

| Event | Emitter | Hot/Cold | Consumers | State written |
|---|---|---|---|---|
| `player://event` | main.rs forwarder ← EngineEvent broadcast | **hot** (10 Hz position while playing; state changes) | player store (queue/index/nowPlaying/status), playhead; souvlaki at 2 Hz | frontend store + playhead struct |
| `scan://progress` | `scan_start` forwarder | hot during scans only | LibraryPage progress UI | — |
| `scan://done` | `scan_start` + watcher forwarder | cold | LibraryPage view invalidation | — |
| `art://` protocol (not a command) | Artwork `<img>` src | hot per library view | webview imgs | — (reads art cache files) |

### Frontend internal sync (no IPC)

- Scrubber: rAF loop over `getPlayhead()` (engine ticks + interpolation) — hot, zero IPC.
- `localStorage` mirrors: `bitchord:player` (volume/muted/repeat/shuffle UI prefs), `bitchord:lyrics` (provider order/enabled/key). Re-applied to the engine once at startup (`player_volume`, `player_repeat`).
- **No polling**: no `setInterval` anywhere, no `refetchInterval` (queries use `staleTime: 5 min`); all `setTimeout` uses are one-shot retries/races. Verified by grep.

### Backend playback-state writers (not IPC)

`engine.rs` is the sole writer of in-memory playback state (invariant holds).
The DB mirror `playback_state` has exactly one writer (the persist_state
thread, event-driven, ≤0.8 s); `play_history` is written only by the history
thread; the resolve-ahead thread writes `Command::PatchUrl` + StreamCache
fills. OS media keys call AppState methods directly — not IPC, not counted by
the command counters.

> M10 superseded part of this inventory: the legacy playback/queue commands
> (16 of the 45) and the `player://event` broadcast are gone; `engine_command`
> and `playback_state` took their place. The table above stands as the M2
> snapshot.

### Per-command call counters (M2 instrumentation)

`src-tauri/src/metrics.rs`: every command invocation is counted in the
`invoke_handler` wrapper (std `AtomicU64` per name, mutex'd map — no new
dependencies). Debug builds only: `hit()` compiles to a no-op in release. A
debug-only thread logs one line per 10 s window listing commands with a
nonzero delta, target `metrics`:

```
INFO metrics: bench_marker +1 (total 1); player_repeat +1 (total 1);
player_volume +1 (total 1); playlists_get +1 (total 1); proxy_get +21 (total 21);
queue_snapshot +1 (total 1); tracks_get +1 (total 1)
```

That window is real startup data: `proxy_get` is by far the hottest command
(lyrics providers racing for the restored track), everything else is one-shot
startup. No behavior change — the wrapper adds one atomic increment (debug) or
an inlined no-op (release) per command call.

---

## 12. Audio reliability baseline (M4)

`crates/playback/src/metrics.rs` + `scripts/stress/run.sh`, debug builds only.

What rodio 0.21 exposes and what it does not:
- **Stream errors — exposed.** rodio forwards cpal's error callback through
  `OutputStreamBuilder::with_error_callback`; the engine routes it into
  `audio::record_stream_error`. CoreAudio never surfaces buffer underruns as
  cpal errors (verified below — even a 200 ms feeder gap counts zero), so the
  counter means "no device-level failure", not "provably glitchless".
- **Data-callback timing — not exposed.** cpal's render callback is owned by
  rodio; its timing is unreachable without replacing the output stack. The
  nearest proxy, used here: `MeasuredSource` times every decoder `next()` on
  rodio's feeder thread — the latency we control, and the thing that starves
  the real buffer when it exceeds it. Reported as a coarse ns histogram with
  quantized p50/p99.

The stress script (`scripts/stress/run.sh [seconds]`) plays a long local WAV
while an env-gated driver (`TANSEN_STRESS=1`, debug only) registers a 30-file
library, rescans it every 30 s, and runs library + history queries every 2 s
(and a catalogue search every 30 s when the sidecar answers). Every 10 s the
logger prints one `audio-metrics` line.

Measured, macOS arm64, debug build — **5-minute stress run (2026-10-31):**

| Metric | Value |
|---|---|
| cpal stream errors | **0** over the whole run |
| Decoder reads timed | 25,552,610 |
| p50 / p99 read latency | **<1 µs / <1 µs** (99.97 % of reads sub-µs) |
| Slowest read | **0.7 ms** |
| Histogram | <1µs 25,517,273 · 1–10µs 24,169 · 10–100µs 78 · 0.1–1ms 11,090 |
| Stress driver | 10 library rescans + continuous 2 s query rounds during playback |

Stall-hook validation (`TANSEN_STALL_MS`/`TANSEN_STALL_EVERY`, debug only):
- 40 ms sleep every ~0.6 s, 30 s: `>10ms` bucket 32, slowest 40.1 ms,
  stream_errors 0 — CoreAudio rode through.
- 200 ms sleep every 1 s, 15 s: slowest 200.0 ms, 8 reads >10 ms,
  **stream_errors still 0** — audible gaps are invisible to cpal's error
  callback on macOS, confirming the read histogram is the glitch signal.


## Source-layer inventory (S0) — read-only, 2026-11-01 at `edcb60e` + dirty tree (35 modified/untracked files)

1. **Catalogue trait** — `MusicSource` (`crates/sources/src/lib.rs:156-191`): `config_id`, `kind`, `display_name`, `health`, `search(query, limit, request)`, `stream(track_id, request)`, default `resolve(track_id, request) -> Option<ResolvedMedia>`. Implementors: `AddonSource` (`sources/src/addon.rs:218`), `YtMusicSource` (`yt.rs:380`); `SourceKind::{Addon, YouTube}` ranked.
2. **Sidecar HTTP surface** (`sidecar-overlay/sidecar/src/main/kotlin/sidecar/Main.kt`) — endpoints: `/health` (:54), `/search?q&filter=[songs|albums|artists|playlists|videos]` (:55,82-91), `/track/{id}` (:56), `/track/{id}/stream[?prefer=aac]` (:97,110), `/album/{id}`, `/playlist/{id}` (browse, :57-58), `/lyrics/track/{id}` (:59), 404 fallback (:60). Stream response: `{url, format(mime), kbps, itag?, loudnessDb?, container("mp4" when prefer=aac), expiresAt(now+6h)}` (:110-134; `STREAM_TTL_MS = 6h`, :163). `prefer=aac|m4a` ⇒ `StreamResolver.resolveForDownload(requireM4a=true)` progressive MP4/AAC; else `StreamResolver.resolve` + mime/container sniff (:110-127). Error shape: JSON `{error: "..."}` (400 missing q/browse id, 404 route, 502 upstream) — `errorJson` (:60,82). **Kotlin source IS in this repo** (`sidecar-overlay/`). No suggest/related endpoints. Quality/codec hints: only `prefer=aac`.
3. **Remote-track ingress** — frontend `Enqueue+Play` commands (`state/player.ts:202,262,281,293,495`) → `engine_command` (`commands.rs:308`) → engine bounded mpsc. Sidecar rows carry `videoId` but **no `play_remote` path exists anymore** (M10 removed it; `state.rs` has no playback surface). `ResolvedMedia {url, headers, expiry(unix s), length(Option, always None), mime}` (`sources/src/lib.rs:99-107`); `SourceStream` adds `below_request`, `duration_sec`, `mime`, `expiry_ms` (:75-92). `HttpRangeReader` (blocking test/legacy oracle) chunks 512 KiB (`remote.rs:16`); `PrefetchReader` HIGH_WATER 4 MiB, LOW_WATER 512 KiB, FETCH_CHUNK 512 KiB (`remote.rs:200-202`). Resolve-ahead: **NOT FOUND** (the `remote.rs` app-core module was deleted in M10). Expiry/403: 403/410 → `UrlDead` → engine one-retry re-resolve (`playback_engine.rs` M16); `prepare` refuses past-expiry (`rodio_factory.rs`).
4. **Persistence** — tables (migration 1, `persistence/src/lib.rs:82-154`): `settings`, `library_folders`, `tracks` (mtime_sec/size_bytes/art_dirty), `playlists`, `playlist_tracks`, `play_history`, `playback_state` (migration 2, no longer written — frontend mirror). Settings live in `settings` table (`repo` get/set) + sources config in `sources_config` setting key (`sources/src/registry.rs:24`). Registry fns: `load_configs/add_config/remove_config/set_enabled/build_active` — `reorder` still exists in registry but **`sources_get`/`source_reorder` commands are unregistered/deleted** (TAN-05; handler list has `sources_get_with_health` only, `main.rs:236`). No `sources` table — config is a settings row.
5. **Audio decode** — rodio 0.21.1 (`symphonia-all` feature, `Cargo.lock:3282-3285`), symphonia 0.5.5 (`all` features, lock 3849-3852). Supported extensions accepted by the scanner (`library/src/metadata.rs:15-18`): mp3, flac, m4a/mp4, aac, ogg/oga, **opus**, wav, aiff, wma, ape, wv, alac. Dynamic decode through rodio `Decoder`+Symphonia covers: Opus/WebM (via symphonia-all), AAC/M4A, FLAC/ALAC, MP3, Vorbis, WAV. Output sample rate: the `sample_rate` atomic = decoded source rate (`pipeline.rs:67,156`); `RingSource::sample_rate()` is hardcoded 44100 informational — output device rate handled by rodio's mixer (callback copies + applies volume only, M19).
6. **Frontend** — SearchPage lazy route (`pages/SearchPage.tsx:13`), `YtRow` local interface (:138) fed by `yt_search` invoke (:150); queue UI `shell/UpNext.tsx` (timeline `QueueEntry` types, `usePlayer.getState` actions); SettingsPage (`pages/SettingsPage.tsx:14`) with sources/lyrics sections; result rows typed via `types.ts` (`ScanProgress{folder_id}`, `Folder`, `QueueRow` in `lib/queueRows.ts`).
7. **Milestone/TAN state** — M11–M20 all documented as done in `REPORT.md` (:535-798). TAN statuses (REPORT.md:515-527): Fixed — 01,02,05,10,11,12; Partially fixed — 03 (supervision decision); Not fixed — 04 (UI verification), 06 (bench dependency decision), 08 (decision), 07/09 (accepted). HEAD `edcb60e`; working tree dirty with the M11–M20 implementation (35 files).
8. **Network surface** — outbound hosts: sidecar loopback `127.0.0.1:<port>` (`yt.rs:159,281`); `PROXY_ALLOWED_HOSTS` 16 single hosts (`commands.rs:148-165`), redirect policy re-validates per hop; CSP `img-src 'self' art: http://art.localhost data: blob:` + `connect-src 'self' ipc: http://ipc.localhost` (+ dev ws/localhost in devCsp) (`tauri.conf.json:25-26`); provider thumbnail hosts are not in CSP (imgs blocked → only local art/`art://` allowed).

**Unknowns (S0)**: (a) exact googlevideo CDN hosts are inside the sidecar's upstream client (opaque by legal guard); (b) `sources_config` value shape/serialization in `settings` not inventoried this pass; (c) whether `prefer=aac` always yields a container Symphonia decodes (depends on upstream resolution); (d) `playback_state` table still written by anything? — frontend mirror only, SQLite row legacy (verify in a later stage).

## S1/S2 — source contract, types, registry, quality (2026-11-01)

- S1: `MusicSource` extended (no new trait): `search(query, limit)`, `stream(key: &TrackKey, request)`, default `resolve(key, request)`; `TrackKey {source, id}` with `TRACK_ID_MAX_LEN = 1024`; `SourceKind::{Addon, Local, YouTube}` + rank; `StreamFormat` + `sample_rate/bit_depth/kbps_inferred`, `is_lossless(codec)` table (flac/alac/wav/aiff/ape/wv → true, known-lossy → false, unknown → None); `Container::{Progressive, Hls, Dash}`; `ResolvedMedia` + `format/container/source/below_request`; `SourceError::{Permanent, Transient, Timeout, Unsupported, Encrypted}`. Sidecar/addon adapters updated to new signatures with no behavior change; call sites (YtHandle, shell factory closures, tests) updated. 12 sources unit tests.
- S2: migration 3 seeds built-in `sources_config` (YouTube + Local, enabled) via the existing single writer; `AudioQuality::{DataSaver, Best, Lossless}` + `to_stream_request()` (Capped 64 / Best / Lossless); registry `validate_order`/`set_order(ids)` (bounded 64, reject unknown/duplicate/wrong-length) replaces the old delta reorder; `sources_get` re-added, `source_reorder(ids)`, `quality_get`/`quality_set` (strict enum) IPC; UI quality `<select>` in PlaybackSection following the Crossfade pattern, enable toggles already present in SourcesSection. Disabled sources: excluded from `build_active`; queued-track `SourceError::Disabled` propagation is deferred to the resolver (no source-keyed queue entries exist yet — recorded).

## S3 — resolver (2026-11-02)

`sources::resolver::Resolver`: `resolve(TrackKey, StreamRequest) -> Result<ResolvedMedia, SourceError>` — pinned source only (lookup returns None when disabled → `Disabled`); per-source timeout `RESOLVE_TIMEOUT = 15 s`; URL cache TTL = min(media expiry, `URL_CACHE_TTL = 1 h`); negative cache `NEGATIVE_CACHE_TTL = 10 min`; **single-flight per key** (once-cell map → N concurrent resolves make 1 upstream call); transient/timeout get exactly one sequential retry; permanent errors are cached negative. `invalidate(key)` drops both cache kinds. `resolve_blocking` (one-shot runtime) is the engine/factory facade — never on the audio thread. RodioFactory gains `with_resolver`, `register_track(EntryId, TrackKey)`, routes `prepare` through the resolver for keyed entries (library/bench fallback unchanged), and implements `PipelineFactory::invalidate` (new default method) which clears the key + resolver caches. Engine M16 UrlDead retry now calls `factory.invalidate(entry)` before the one re-resolve — integrated, not duplicated. Sequential only; no racing.

## S4 — decodability + container policy (2026-11-02)

**Feature set (verified from Cargo.lock):** symphonia 0.5.5 `all` includes
FLAC, MP3, AAC, ALAC, PCM/WAV, Vorbis, ADPCM + formats CAF/ISO-MP4/MKV/OGG/RIFF.
**NO `symphonia-codec-opus` and NO `symphonia-format-webm`** in the lockfile —
Opus and WebM are NOT decodable in this build. `decodable(StreamFormat, Container)`
(S4) reflects this exactly; HLS/DASH are always false. Unknown codecs are never
asserted decodable.

**Matrix (measured):**

| Codec | Container | Decodable |
|---|---|---|
| FLAC / ALAC / WAV / AIFF | Progressive | yes |
| MP3 | Progressive | yes |
| AAC / M4A / MP4 | Progressive (ISO-MP4) | yes |
| Vorbis / OGG | Progressive | yes |
| Opus | Progressive | **no** (no codec in build) |
| WebM | Progressive | **no** (no format in build) |
| any | HLS / DASH | no (policy) |

Verified: `decodable_table` unit test (playback's `codec_matrix` test was
reseated to the sources crate for layering); WAV decode probe passes (RMS
>0.005); resolver selection tests (best-decodable among candidates; all-
undecodable → `Unsupported`).

**Opus: STOP — decision required.** options: (a) add a `libopus`-based
decoder dependency (e.g. symphonia-codec-opus — a new dep, not in `all`);
(b) restrict the catalogue to AAC (sidecar `prefer=aac` default) and treat
Opus answers as `Unsupported`. Measured implication: the sidecar's `prefer=aac`
serves 131 kbps AAC; Opus from YouTube flows is typically ~130-160 kbps at
comparable audible quality. No quality delta measured (no Opus decode path to
compare), so (b) is the zero-new-dep path already in use. Recorded for the
owner. ffmpeg not installed → per-codec fixture generation NOT RUN (only the
self-generated WAV was exercised; the matrix itself is asserted structurally).

## S1–S4 completion pass (2026-11-02)

Verification-driven review of S1–S4 against the stage preambles found four gaps, now closed:
- S3: the resolver existed but was never wired into the shell — `main.rs` now constructs `sources::resolver::Resolver` with a catalogue-gated source lookup (disabled/unavailable kinds → Disabled) and builds `RodioFactory` via `with_resolver`.
- S1: `TRACK_ID_MAX_LEN` was declared but not enforced — `TrackKey::new(source, id)` now rejects over-cap ids (`SourceError::Permanent`); the yt adapter constructs via it.
- S2: IPC-order rejection was covered only implicitly — `oversize_order_is_rejected` + `unknown_duplicate_wrong_length_rejected` test the boundary explicitly (MAX_ORDER_IDS).
- S4: the optional backward-compatible codec-preference hint was missing — sidecar `/track/{id}/stream` accepts `prefer_codec` (absent = previous behavior; only aac/m4a honored) and the yt source surfaces it from non-Lossless requests.

## S5 — multi-source search + remote artwork (2026-11-02)

Sidecar search verified to expose title/artist(+artists)/album/durationSec/thumbnailUrl/videoId — no additive change needed. `sources::search`: `search_all(sources, query, limit)` — parallel fan-out over enabled sources (Local never searched), `SEARCH_TIMEOUT = 12 s`, per-source `SourceStatus {Ok, Timeout, Error{kind,message}}`, results ordered by kind rank, per-source (`SEARCH_PER_SOURCE_LIMIT = 10`) and total (`SEARCH_LIMIT_CAP = 30`) caps; overlong track ids dropped via `TrackKey::new` cap. Command `search_tracks(query, limit)`: trims, rejects empty / >200 chars / limit outside 1..=30; stale searches cancelled by atomic token (newer request wins; older result dropped). `fetch_artwork(url)`: Rust-side fetch for allowlisted image hosts (i.ytimg.com / lh3.googleusercontent.com / yt3.ggpht.com), 5 MiB size cap, content-type must start `image/`, disk-cached under `art/remote/<sha1(url)>` served by the existing `art://` handler; no CSP wildcard changes. Frontend: existing YouTube search tab now queries `search_tracks` (no redesign); status banners for failed sources. No new dependencies.

## S6 — remote track identity in queue (2026-11-02)

Queue entries now carry remote identity explicitly. Each queue entry is still
addressed by an integer `EntryId`; the engine keeps a `remote_keys: HashMap<
EntryId, TrackKey>` (S6) so a source-keyed entry can resolve fresh through the
resolver at load time. `PlaybackState` is untouched (no key/URL ever enters
it); the queue body stays inside the engine, display rows come from the DB
query.

**Schema decision — separate `remote_tracks` table, chosen.** The alternative
(a `source`/`key` column on `tracks`) would have forced nullable source
semantics onto library rows and rewritten the existing track insert/pl/upsert
paths — i.e. migrating existing track semantics. The separate table is purely
additive (migration 4), so no STOP was required. The upsert bounds every text
column (`REMOTE_MAX_STR = 512`, `REMOTE_MAX_KEY = 1024`; clipped, never
rejected).

**Command contract (S6).** `Command::Play { entry, remote }` and
`Command::Enqueue { tracks, remote, placement }` carry optional `RemoteKey =
(String, String)` — `(source, id)` — alongside local entries, without breaking
existing variants. `Enqueue.tracks` holds every entry id in queue order (local
library ids and remote row ids alike); `remote` maps the remote subset to
TrackKeys. The engine moves remote-only ids into the queue body too (they were
previously stored in the key map but never enqueued), keeps `PlaybackState`
small, and drops all remote keys on `ClearQueue` so a reused EntryId cannot
resolve as remote. The enqueue cap (`MAX_ENQUEUE_TRACKS = 10_000`) covers the
combined list and still refuses over-cap commands without touching state.

**The app layer upserts before commanding.** New IPC `remote_track_upsert`
(`commands.rs`) validates (source kind ∈ {youtube, addon}, key id non-empty
≤ 1024, finite duration ≤ 24 h, artwork URL https-only ≤ 512), writes through
the existing single writer (`state.db`), and returns the `remote_tracks` row
id — which becomes the EntryId. The frontend awaits this BEFORE sending
Enqueue/Play, so the mirror and the DB agree before the engine sees the
command.

**Queue query returns both kinds.** `queue_tracks_get(ids, remote_ids)`
(cap 10 000 combined) now returns unified `QueueDisplayRow`s
(`kind: local|remote`, title/artist/album/duration plus `art_path` for local,
`artwork_url`/`video_id`/`key_source` for remote) via
`repo::queue_rows_by_ids` — local ids through `tracks_by_ids` (missing rows
dropped), remote ids through the now-chunked `remote_tracks_by_ids`, each
group in input order.

**Restore resolves fresh.** The frontend localStorage mirror persists remote
rows as keys + display metadata only — never resolved stream URLs. On restore,
remote entries are re-upserted (idempotent — same key returns the same row id)
and re-seeded as `Enqueue` remote pairs; the engine resolves each fresh through
the resolver at prepare time.

**Frontend.** `state/player.ts` actions (`playTracks`/`playTrack`/
`playNext`/`addToQueue`/`jumpTo`/`togglePlay`) are remote-aware: local tracks
keep their library ids, remote tracks go through the upsert first; the queue
mirror entries carry `engineId` (the engine EntryId). Search rows (S5) are now
playable/enqueueable through the existing queue actions — `SearchPage.tsx`
tags them with `sourceKind` + `artworkUrl`; `TrackMenu` "Play Next / Add to
Queue" work for remote rows, while "Add to Playlist" stays local-only (playlist
schema is library-row-based). `refreshQueueRows` queries both kinds and merges
remote rows by `remote:<rowId>` — with remote artwork routed through the
allowlisted `fetch_artwork` before display.

**Files touched:** `crates/persistence/src/repo.rs` (+
`queue_rows_by_ids`, chunked `remote_tracks_by_ids`), `crates/persistence/
tests/views.rs` (+ queue-rows both-kinds test), `crates/app-core/src/playback_
engine.rs` (Enqueue body + combined cap, ClearQueue clears remote keys, fake
records keys, 4 new tests), `crates/app-core/src/playback_types.rs` (Enqueue
doc), `apps/desktop/src-tauri/src/commands.rs` (`queue_tracks_get` extended,
`remote_track_upsert` new), `.../main.rs` (command registered), `apps/desktop/
src/state/player.ts`, `apps/desktop/src/lib/queueRows.ts` (+ tests),
`apps/desktop/src/data/music/types.ts`, `apps/desktop/src/pages/SearchPage.tsx`,
`apps/desktop/src/components/music/TrackMenu.tsx`. Migration 4, the
`remote_tracks` upsert/query primitives, and the `RemoteKey` fields on
`Command` were already present in the S6 working tree and are verified here.
No new dependencies.

## S7 — matcher + fallback (2026-11-02)

**Pure matcher** (`sources/src/matcher.rs`, std only — no new dependency):
- `TrackMeta {title, artist, duration_sec}`; `normalize` folds case,
  punctuation (alphanumerics + spaces only), common Latin-1/Extended-A
  diacritics, and feat/ft credit suffixes (parentheticals and bare tokens).
- `matches(target, candidate)`: normalized title equal AND normalized artist
  equal AND marker masks equal (live/remix/remaster/acoustic, word-boundary
  token match) AND durations within `DURATION_TOLERANCE_SEC = 10` when both
  known (unknown never rejects).
- `queries(meta) -> Vec<String>`: `"title artist"`, then `"title"`
  (normalized) — the search queries for a fallback source.

**Resolver fallback ladder** (`sources/src/resolver.rs`):
- `Resolver::with_fallbacks(sources, fallbacks, meta)` — `Resolver::new`
  keeps its S3 behavior (empty fallback list, no metadata). `fallbacks` is
  a provider returning enabled sources in rank order; `meta` maps a
  `TrackKey` → target `TrackMeta` (`None` = no fallback attempted).
- On pinned failure (Permanent, or transient/timeout after the one retry),
  sources are tried sequentially (no racing), per-source `RESOLVE_TIMEOUT`,
  skipping the pinned source itself (same config id) and `SourceKind::Local`
  (no library lookup seam in the resolver yet — S12). A candidate counts
  only if `matches()` AND its `stream()`/resolve succeeds; the winning
  `ResolvedMedia.source` is recorded as the fallback source. No match
  anywhere → the original pinned error is returned.
- Shell wiring (`main.rs`): `fallbacks` builds enabled addons (user rank
  sorted) plus the memoized sidecar handle — lazily per trigger, never an
  eager JVM launch; `meta` reads the S6 `remote_tracks` table via
  `repo::get_remote_track_by_key` (the single writer's data).

**Files touched:** `crates/sources/src/matcher.rs` (+new, 6 tests),
`crates/sources/src/resolver.rs` (fallback ladder + 7 new tests),
`crates/persistence/src/repo.rs` (+`get_remote_track_by_key`),
`apps/desktop/src-tauri/src/main.rs` (resolver wiring). No new dependencies.

## S8 — resolve-ahead (2026-11-02)

**Measure first (existing M3 harness, `yt_measure.rs`, live network — ran
this session):**

| Metric | Value |
|---|---|
| Sidecar spawn → ready | 465 ms |
| JVM RSS ready → after 30 resolves | 78 → 144 MiB |
| Resolve ×30 | cold 918 ms, p50 992 ms, p95 1275 ms, max 1400 ms |
| Remote first-play | resolve 1017 ms + reader open 1126 ms + first 0.25 s decode = **2142 ms total**, RMS 0.2173 |

Resolve is ~47 % of remote first-play (1017/2142 ms); every track boundary
pays a fresh cold resolve when the S3 cache misses. **Measurable part of the
gap — implemented.** Track-boundary gap itself is not directly measurable by
the existing harnesses (needs GUI E2E); the resolve share of first-play is the
proxy. (Session variance note: resolve p50 992 ms vs 423 ms at M20 — the
documented 4× network-dominated variance, not a code change.)

**Implementation — event-driven, one in flight, no timers:**
- `PipelineFactory::prefetch_resolve(entry, key)` — new default no-op
  returning a boxed future. `RodioFactory` implements it: resolves the key
  through the S3 resolver (`resolve_blocking`, off the audio path), which
  populates the URL cache; a later `prepare` for that entry hits the cache
  instead of the network.
- The engine (`playback_engine.rs`) calls `maybe_prefetch` **only** after an
  accepted command or an auto-advance (queue/cursor change): Play/Next/Prev/
  Enqueue/ClearQueue and the end-of-track advance. No periodic timers
  anywhere; nothing runs while paused or idle. Local entries (no source key)
  are skipped — they resolve live and cheaply at prepare.
- Concurrency 1: at most one in-flight prefetch; if the queue changes and
  the new next-entry differs, the stale task is aborted before the new one
  starts. Same next-entry = no respawn.
- No new `Status` variants, events, or `PlaybackState` fields.

**Files touched:** `crates/app-core/src/playback_engine.rs` (trait method,
`maybe_prefetch`, wiring after commands + auto-advance, 3 new tests),
`apps/desktop/src-tauri/src/rodio_factory.rs` (real prefetch resolve). No new
dependencies. Frontend untouched.

## S9 — Subsonic/OpenSubsonic source (2026-11-02)

**Adapter (`crates/sources/src/subsonic.rs`)** — `SubsonicSource` implements
`MusicSource` with the shared REST API:
- health → `rest/ping` (auth-exercising), search → `rest/search3?query&songCount`
  (songs only, `isVideo` rows skipped), stream → `rest/stream?id&format=raw`
  (original format; lossless detected from Content-Type, e.g. flac/wav/aiff,
  vs lossy mp3/m4a/ogg). Artwork rows carry `rest/getCoverArt?id=` URLs.
- Auth is the token scheme: `u`/`t=md5(password+salt)`/`s`/`v=1.16.1`/`c=tansen`/
  `f=json` — the password never appears on the wire, and an API key works as
  the secret where the server supports one. The secret is fetched per request
  through the `SecretProvider` seam (`KeychainSecretProvider` default;
  `FixedSecretProvider` for tests).
- URL policy (`validate_base_url`): https default; http only with the explicit
  opt-in AND a loopback/private host; scheme allowlist http/https; embedded
  URL credentials rejected. Redirect policy is same-host AND same-scheme, ≤5
  hops. JSON responses capped at 4 MiB, stream probe at 64 KiB (chunked
  accumulation, not Content-Length trust). DRM/encrypted responses rejected:
  `x-subsonic-drm: true` or a content-type containing "encrypted" → error.

**Credentials (`crates/sources/src/keychain.rs`)** — macOS Keychain generic
passwords through `objc2-security` (the one new dependency approved for this
stage; the `security` CLI alternative was rejected because passing the secret
as an argv would expose it in the process list). Service
`app.tansen.desktop.subsonic`, account = username. Upsert (delete-then-add on
duplicate — SecItemUpdate is not bound by objc2-security), read, delete.
Non-macOS builds return a permanent error / `None`. The password NEVER touches
SQLite, logs, events or frontend state; the settings DB stores only
`username` + `allow_http` (non-secret fields on `SourceConfig`).

**Registry** — `SourceKind::Subsonic` (rank 2, between addon 0 and YouTube 3);
`build_active` constructs the source per config and warn-skips a config whose
URL fails policy (a bad Subsonic entry must not break other sources);
`upsert_config` for re-saving.

**Shell** — `subsonic_add` command: validates inputs, stores the password in
the Keychain FIRST, then saves the password-free config row;
`sources_get_with_health` for the settings list. Registered in `main.rs`.

**Settings UI** — Subsonic form on the existing Sources section (base URL,
username, password, allow-http checkbox). The password lives only in the
transient form field and is cleared on submit; nothing persists it in the
query cache or local storage. No bundled server list — the user enters their
own server.

**Dependencies:** `objc2-security` + `objc2-core-foundation` (macOS Keychain;
the approved addition) and `md-5` (token-scheme hashing, part of the approved
auth-scheme decision). No other new dependencies.

**Tests (`crates/sources/tests/subsonic_source.rs`)** — 8 integration tests
against dependency-free `TcpListener` mock servers: search3 round-trip with
token auth + video skip, stream sniffing + auth URL, ping ok/auth-failure
health mapping (server error code/message surfaced), redirect-to-other-host
rejected (target never fetched), oversize response rejected, DRM and
encrypted streams rejected, source-failure isolation (failing Subsonic +
healthy addon fan-out), and the S3/S7 fallback integration (pinned source
fails → ladder lands on Subsonic → stream URL) run through `resolve_blocking`
on a plain thread as the engine calls it.

## S10 — discovery pages (2026-11-02)

**Inventory (per S0):** the sidecar already exposed `/album/{id}` and
`/playlist/{id}` (both through `browseSongs`); `/artist` and `/suggest` were
missing — exactly those two were added, additively. `:shared` is untouched.

**Sidecar (overlay only):**
- `/artist/{id}` — same `browseJson` path as album/playlist (artist browse
  ids pass through unchanged; the generic layout-agnostic page walk collects
  the top-tracks shelf).
- `/suggest?q=` — upstream `searchTypeahead`, which is documented
  unauthenticated (strips the session cookie): no account sign-in, cookies
  or tokens cross this endpoint. Answers with the existing
  `searchResultsJson` shape (tracks + browse cards).
- Rebuilt `sidecar-all.jar` (JDK 17; the JBR's Java 25 breaks the Kotlin
  compiler's version parser) and route-checked it: `/suggest` without `q`
  and `/artist/` without an id both answer their validation errors, so the
  routes are live without any upstream call.

**Client (`crates/sources/src/yt.rs`) — inherent methods on
`YtMusicSource`, NOT on the source trait:**
- `suggest(query)` → `(Vec<DiscoveryTrack>, Vec<DiscoveryCard>)`,
  `album(id)`/`artist(id)`/`playlist(id)` → `DiscoveryPage` (header +
  first-page rows only; continuations never followed). `DiscoveryTrack`
  carries the S6 `TrackKey`.
- Caps: `DISCOVERY_MAX_ROWS` (50), `DISCOVERY_MAX_SUGGEST` (20); per-call
  deadline `DISCOVERY_TIMEOUT` (15 s) via `tokio::time::timeout` around each
  round trip.
- `valid_browse_id`: ASCII alphanumeric + `-` + `_`, length 1..=64 —
  enforced in the client and again at the IPC boundary.
- Fixture-driven bug fix on the shared `SidecarSong` parse struct: the
  wire names are camelCase (`durationSec`, `thumbnailUrl` per
  `sidecar/Json.kt songJson`), so the existing search path had been
  silently dropping durations and artwork. Renames added; search rows now
  carry both.

**Shell:** `YtHandle` blocking wrappers (`suggest`/`album`/`artist`/
`playlist`, the established `await_sync` pattern) + four Tauri commands
(`discovery_suggest`, `discovery_album`, `discovery_artist`,
`discovery_playlist`) — query validated/capped at the IPC boundary
(`SEARCH_MAX_QUERY`), ids re-checked via `valid_browse_id`, per-call
timeout in the client. Registered in `main.rs`.

**Frontend (minimal, no redesign):** `DiscoveryPage` at `/discover`
(sidebar entry, Compass icon) reusing `SearchField`, the search page's
remote-row → `Track` mapping and `TrackList`; suggest rows are playable
through the existing remote-play path. Browse cards render as plain chips —
remote album/artist/playlist pages do not exist and the local ones are
DB-bound, so no links (the commands exist for later wiring).

**Tests:** in-crate `discovery_tests` against saved fixture responses
(`crates/sources/tests/fixtures/discovery_suggest.json`,
`discovery_browse.json` — hand-authored wire shapes mirroring the sidecar's
JSON builders, sanitized: synthetic ids, no network captures, no secrets):
typed-row mapping with TrackKey, caps on tracks/cards and browse rows,
over-length id rejection, header preservation under caps, and
`valid_browse_id` charset/length/traversal cases.

## S11 — autoplay (2026-11-02)

**Inventory:** the sidecar had no related/radio endpoint; upstream
`:shared` exposes `YtMusicRepository.radio(videoId)` (the watch queue behind
the playing track). One additive sidecar route:

- `/radio/{videoId}` → `radioJson(songs)` = `{tracks:[songJson…]}` (the
  same row shape search uses). `:shared` untouched; `sidecar-all.jar`
  rebuilt (JDK 17) and smoke-checked: `/radio/` without id answers its
  validation error; a real id answered 50 related rows (live check — the
  network was available).

**Client (`crates/sources/src/yt.rs`, inherent on `YtMusicSource`, not the
trait):** `related(&TrackKey)` → `Vec<DiscoveryTrack>` (TrackKey rows),
capped `RELATED_MAX_ROWS` (25), the 15 s per-call deadline shared with the
other sidecar reads, and the same id validation (kind must be YouTube).

**Setting:** `autoplay` in the settings table, default off.
`autoplay_get`/`autoplay_set` commands (strict booleans) keep the stored
setting and an `AtomicBool` flag in step; the Settings → Playback screen
gains a checkbox row reusing the existing Row/checkbox components.

**Task (`apps/desktop/src-tauri/src/autoplay.rs`) — app crate, never the
Engine:**
- `QueueMirror` reconstructs only what autoplay needs from inputs the shell
  already sees: the queue body from every forwarded `Command` (Enqueue
  placement + remote-only append + ClearQueue, exactly the engine's arms) and
  the cursor from the published `PlaybackState` (covers media keys and
  auto-advance). `engine_command` forwards each command clone into an
  unbounded channel before sending to the engine.
- `run_autoplay` selects on the watch and the command channel only — no
  timers. The trigger (`consider_fetch`, shared by both arms): status
  Playing, cursor == last queue entry, flag on, not already fetched for that
  entry (once per entry). The command arm re-checks the current state after
  applying, closing the select! race where the watch event lands before the
  command does.
- Action: dedup related rows against every remote id already queued
  (`pick_related`), cap to `MAX_ENQUEUE_TRACKS − mirror.len()`, upsert each
  row through `remote_track_upsert` (the DB row id is the EntryId — the same
  identity path the frontend's own enqueues use), send
  `Command::Enqueue{Append}` via `Engine::send`, and emit
  `autoplay_enqueued` (the frontend's queue_rev refresh only resolves ids it
  already knows, so appended rows must be announced). `MAX_ENQUEUE_TRACKS`
  became `pub` — a visibility change, no engine logic touched.
- Local tail entries and non-YouTube tails skip the fetch (the radio
  endpoint is a sidecar read). Fetch/enqueue are best-effort: an error means
  "nothing to add", never a user-facing failure.

**Frontend:** a small `autoplay_enqueued` listener in the player store
adopts the announced rows into the queue mirror (remote artwork through the
allowlisted fetch, as everywhere); the toggle is the only new control. No
redesign.

**Known limit (recorded):** if the related fetch loses the race against the
tail track's end (engine one-shot deadline → Idle), the fetched rows still
land in the queue but nothing auto-starts; the next tail entry triggers
fresh. The fetch arms when the tail entry STARTS, so the whole track's
duration is the usual budget.

**Tests (`autoplay.rs`, 6):** the loop driven by a fake related fetch +
recording enqueue over a real watch channel — triggers once at the tail (a
second event for the same entry does nothing), not when off (and enabling
takes effect on the next event), paused tail does not fetch (resume is a new
event and fetches once), auto-advance into the appended tail fetches the
next entry, plus mirror semantics (append/insert-next/front/clear/
remote-only/is-tail) and `pick_related` dedup+cap. `Engine` tests unchanged.

## S12 — prefer-local (2026-11-02)

**Seam (smallest possible: one closure, no trait, no new dependency edge).**
`crates/sources/src/resolver.rs` gains a `LocalMatch` type —
`Arc<dyn Fn(&matcher::TrackMeta) -> Option<ResolvedMedia> + Send + Sync>` —
an optional field on `Resolver` set through a builder
(`with_local_match`); the existing constructors keep their signatures, so
every S3/S7 caller is untouched. No STOP needed: the closure's type uses
only types the sources crate already owns, and the shell (which already
depends on `persistence` + `sources` + `library`) supplies the actual
lookup — the crate graph is unchanged.

**Gate:** inside `resolve_upstream`, before any remote call, and only for
`StreamRequest::Lossless | StreamRequest::Best` — the DataSaver tier
(`Capped`) never consults local. Input metadata comes from the resolver's
existing S7 `MetaLookup` (the same title/artist/duration the fallback ladder
uses), matched against the library with the S7 matcher's exact rule
(`matcher::matches`: normalized title+artist equality, marker-word
agreement, ±10 s duration).

**The injected closure (shell, `main.rs`):** reads `db_ro` only (never the
writer, never any library write), finds the first track the matcher accepts,
and returns a `ResolvedMedia` only when the file **exists and is readable**
(a `File::open` probe — a stale row yields `None` and resolution proceeds
remotely). The media carries the absolute local path as `url`,
`SourceKind::Local`, and the track's codec as the `StreamFormat`. Zero
pipeline changes were needed: `playback::open_source` already plays
non-HTTP paths straight from disk (`File::open`), so the local media flows
through the same `pipeline.load(url)` call as a remote URL. Local hits
cache in the resolver like any other answer (one library scan per distinct
(key, request), not per playback).

**Tests (in-crate, `prefer_local_tests`, 4):** local hit preferred for both
Best and Lossless (no remote sources configured — success with the local
url proves the seam answered before remote); Capped skips local and hits
the remote path (Disabled with no remotes); a stale path (`None` from the
closure) falls back to the remote fake and records the remote call; no
local seam at all keeps the S3 behavior; the local hit is cached.

**Layering proof:** S12 touched only `crates/sources/src/resolver.rs` and
`apps/desktop/src-tauri/src/main.rs` — zero manifest edits. `cargo tree
-p sources -e normal --depth 1` shows exactly the S9-approved dependency
set (persistence, reqwest, serde, serde_json, thiserror, tokio, tracing,
async-trait, md-5, url, objc2-security, objc2-core-foundation) — no edge
added or removed. A full `cargo tree --workspace` diff against HEAD shows
only the S9–S11 approved additions already recorded (objc2 family, md-5,
url); nothing S12-shaped.

## S13 — measurement + audit stage (2026-11-02)

No feature code. `crates/sources/src/metrics.rs` (NEW): debug-only counters
(per-source search latency, resolve cold/warm + hit rate, ladder outcomes)
with a cfg(no-op) release twin; hooks in `search.rs` `search_source` and
`resolver.rs` `resolve`/`resolve_upstream`; dumped by the existing 10 s
metrics thread (`main.rs`). `TANSEN_BENCH_SEARCH` driver (debug-only,
env-gated) launches its own sidecar and drives 9 real searches, 4 cold + 2
warm resolves and both ladder outcomes through the real code paths — no
catalogue-setting dependency, no IPC registration. 2 counter unit tests;
workspace 144 passed / 0 failed / 4 ignored. Full numbers, the surface
inventory, the audits and the release-absence proof are in REPORT.md §S13.

## F0 — S13 issue resolution (2026-11-02)

Files changed:
- `deny.toml` (NEW): cargo-deny advisories config; three reasoned ignores
  for informational unmaintained/unsound transitive advisories (paste,
  proc-macro-error, glib). `cargo deny check advisories` = ok.
- `apps/desktop/package.json` + lockfile: happy-dom ^21.0.4, vitest ^5.1.0
  → `pnpm audit` clean; tests + build green.
- `crates/sources/src/keychain.rs`: `#[ignore]`d SecItem roundtrip test
  (runtime verification with cleanup; run explicitly with `-- --ignored`).
- `apps/desktop/src-tauri/src/commands.rs`: TAN-01 fix for `fetch_artwork` —
  `artwork_url_allowed` predicate + `artwork_client()` with a per-hop
  allowlist redirect policy (the client previously had reqwest's default
  follow-anywhere behavior); +1 regression test.
- `apps/desktop/src-tauri/src/main.rs`: debug-only seek-latency hook on the
  bench playback path; removed the stale duplicate TANSEN_BENCH_SEARCH
  driver block left by S13 (NO-DUPLICATION cleanup); driver launch-failure
  warn now carries the actual error.

Full resolution matrix, measurements and NOT RUN list: REPORT.md §F0.

## F1 — audio & macOS validation (2026-11-02)

Files changed:
- `crates/sources/src/yt.rs`: `prefer_codec_hint(request)` — every tier
  (including Lossless) now asks the sidecar for AAC. Fixes a real S4-era
  hole: the un-preferenced Lossless resolve returned Opus/WebM (itag 251,
  measured live) which no released symphonia decodes. +1 unit test.
- `crates/playback/src/pipeline.rs`: `hls_and_dash_manifests_fail_closed`
  test — real `.m3u8`/`.mpd` content through `open_source` errors
  gracefully (the documented, verified HLS/DASH limitation).

Decisions and the four-item status table: REPORT.md §F1. GUI automation and
wakeup measurement remain environment-blocked (documented with exact
commands); the headless bench + bench-search drivers remain the repeatable
real-backend smoke path.

## Final integration inventory (F0, read-only — 2026-11-02)

Fresh suite results this stage: `cargo build --workspace` clean; `cargo test --workspace` **147 passed / 0 failed / 5 ignored**; release build clean; frontend `pnpm test` **37/37** (5 files); `npm run build` ✓. Network: **available** (live sidecar + upstream probes answered throughout today).

### Capability matrix

| Capability | Status | Evidence | Gap |
|---|---|---|---|
| 1. YouTube Music search + audio resolution via sidecar | **Works-verified** | live probes (search p50 598–631 ms, resolve p50 832 ms, itags 140/251, radio 50 rows, first-audio 813 ms — REPORT §S13/F0/F1); `yt.rs` launch/get/stream; 147 tests incl. `subsonic_source`-style mocks | none functional; sidecar JVM not bundled (see infra) |
| 2. JioSaavn | **Missing (report only)** | `grep -rni 'jio\|saavn'` → one comment mention, `registry.rs:3` ("yet (QuickJS modules, JioSaavn, YouTube)"); no code/config/dependency/key anywhere | would need a new source via the S1 trait or QuickJS — not started, deliberately |
| 3. External addon/source support | **Partial** | `addon.rs` (manifest/search/stream protocol) + `subsonic.rs` (S9) with mock-server tests; settings UI add/remove/enable; registry ranks | QuickJS addon modules not implemented (the addon protocol's scripting half); live third-party addon server never exercised (mocks only) |
| 4. Unified search across sources | **Partial** | `search.rs::search_all` fan-out (rank order, caps 30/10, 12 s per-source timeout, stale-cancel `commands.rs` SEARCH_TOKEN) across addon+subsonic+yt; local search is a separate Library tab (`SearchPage` segmented control), `search.rs` deliberately excludes Local | local rows are not merged into the unified result list (two-tab UX by design) |
| 5. Source selection, ranking, enable/disable, fallback ladder | **Works-verified** | `registry.rs` (rank/enable/order caps 64), `sources_get_with_health` (per-kind health), `source_reorder`, S7 ladder (`resolver.rs::try_fallback` — skips pinned config + Local), live ladder timings 1054–1105 ms (F0) | none |
| 6. Track matching + metadata normalization | **Works-verified** | `matcher.rs` (normalize, marker words, ±10 s duration), used by the ladder AND prefer-local (S12, 4 tests) | matcher strictness rejects remasters by design (recorded, observed live in F0 bench) |
| 7. Stream resolution: cache/retry/pinning/resolve-ahead | **Works-verified** | `resolver.rs` cache (TTL min(expiry,1h), negative 10 min), M16 dead-URL single retry (engine), per-key single-flight pinning, S8 `maybe_prefetch` (1 in flight, aborted on queue change); live hit-rate counters (F0) | none measured |
| 8. Quality tiers, lossless, decodability, Opus | **Partial (fixed this stage)** | tiers work (`quality_get/set` → StreamRequest); F1 fixed the Lossless hole (always `prefer_codec=aac` — supply probe: no-prefer = itag 251 Opus/WebM); decodable: mp3/aac/m4a/mp4/flac/alac/wav/aiff/pcm/adpcm/vorbis/ogg YES; **opus/webm NO** (symphonia 0.5+0.6 ship no opus codec; `symphonia-codec-opus` absent from crates.io); HLS/DASH fail-closed (playback test) | opus undecodable — supply-side mitigated; revisit only with an upstream symphonia release |
| 9. Local library integration | **Works-verified** | scan (stat-skip + batched tx, 100k timing test), local views/search/playback, prefer-local seam (S12: db_ro + matcher + readability probe, 4 tests) | GUI-level prefer-local interaction unverified live (headless tests only) |

### Previous-roadmap stages existing in code

M1–M20 (bench, IPC counters, sidecar measure, audio reliability, playback types, engine, rodio factory, queue, app wiring, frontend migration, SQLite reads, scan perf, ResolvedMedia, supervisor, prefetch reader, 403/expiry retry, Now Playing, device/sleep, SPSC ring, re-measure) — all present. S0–S13 + F0/F1 source stages — all present (IMPLEMENTATION.md sections). Audit remediation TAN-01…TAN-12 per REPORT: **Fixed** TAN-01, 02, 05, 10, 11, 12; **Partially fixed** TAN-03; **Not fixed (Info, decision/accepted)** TAN-04 (CSP style-src inline), TAN-06 (bench_marker in release), TAN-07 (sidecar loopback auth), TAN-08 (jar integrity), TAN-09 (Musixmatch token).

### IPC surface (music-relevant)

40 registered commands (`main.rs` handler list): settings 2, folders 3, tracks/scan 2, playlists 7, views 5, history 1, proxy_get, bench_marker, engine_command, queue_tracks_get, remote_track_upsert, yt_search, search_tracks, fetch_artwork, discovery 4, sources 4 (`sources_get`, `sources_get_with_health`, `source_reorder`, `source_add`, `subsonic_add`, `source_remove`, `source_set_enabled`), quality 2, autoplay 2. Events emitted: `playback_state`, `autoplay_enqueued`, `scan://progress`, `scan://done`. Frontend listeners mirror those four (`state/player.ts`, `commands.rs` scan forwarder).

### Frontend components + data contracts

Pages: Home, New, Library, Search (+Library/YouTube tabs), Album, Artist, Playlist, ReorderPlaylist, Discovery, Settings. Music components: Artwork, DetailHeader, ExplicitBadge, FavoriteButton, Lockups, SearchField, ShelfView, States, TrackList, TrackMenu, links, play. Shell: AppShell, Sidebar, PlayerBar, Lcd, UpNext, FullScreenPlayer, LyricsPanel/Settings, VolumeControl, MeshBackdrop. Contracts: `data/music/types.ts` (18 interfaces/types) + `app_core::yt_handle` DTOs (RemoteTrack/CatalogRow/DiscoverySuggest/Page) + `sources::yt` DiscoveryTrack/Card/Page + `sources::registry::SourceConfig`.

### Test infrastructure

cargo: 147/0/5 across 7 crates — unit + integration incl. mock-TcpListener HTTP suites (subsonic, proxy, addon), engine law tests, fixture-driven parser tests (`tests/fixtures/`), perf/measure harnesses (`scripts/bench`, `scripts/stress`), `#[ignore]`d live tests (keychain SecItem roundtrip, yt_measure sidecar, yt_sidecar). Frontend: vitest 37/37 (5 files, happy-dom 21, vitest 5.1.0). UI automation: none (tauri-driver unsupported on macOS — verified; AppleScript blocked by Accessibility — `-1719`).

### Sidecar JVM discovery + bundling

`discover_jar` (`yt.rs`): `BITCHORD_SIDECAR_JAR` env → exe dir + parent + cwd + cwd.parent candidates incl. `resources/sidecar-all.jar` — packaged via `tauri.conf.json` `"resources": ["resources/sidecar-all.jar"]` (bundle resource). `discover_java`: `JAVA_HOME/bin/java` → PATH `java -version` exit-0 probe (the macOS stub at /usr/bin/java exits 1 and is rejected). **No bundled JRE** — a system JDK 17+ is required at runtime (documented gap for end-user distribution).

### Licenses

Workspace: **GPL-3.0-or-later** (`Cargo.toml:15`, all five crates + app crate inherit). Sidecar fat jar: **GPL-3.0 as a whole** — NewPipeExtractor v0.26.3 (GitHub API: GPL-3.0), innertubex-desktop v0.7.0 (pom: GPL-3.0), `:shared` (BitChord, GPL-3.0), plus MIT/Apache Kotlin libs — consistent with Tansen's GPL. `cargo deny check licenses`: **unconfigured** — no `[licenses]` section, so the default allowlist (no GPL) rejects everything workspace-side (506 informational rejects); needs a configured allowlist including GPL-3.0 before release packaging.

### Signing / notarization / entitlements / updater

None configured: `tauri.conf.json` has no signingIdentity, updater plugin, pubkey, or entitlements file — the app ships unsigned (no distribution story yet). Gap → release-packaging stage.

### Prioritized gap list → F1–F7 (proposal)

1. **F1 — QuickJS addon modules (`rquickjs`)**: closes the addon-source Partial (scripted third-party sources are the only missing source class). Highest value.
2. **F2 — Unified search merge**: fold local rows into `search_all` output (rank-aware). Small, isolated.
3. **F3 — Release packaging**: macOS signing/notarization/entitlements/updater + bundled-JRE decision + `deny.toml [licenses]` config. Blocks distribution, not features.
4. **F4 — Opus**: SKIP/reduce — deferred with evidence; act only when symphonia ships opus and rodio adopts it.
5. **F5 — GUI automation**: reduce to "if Accessibility is granted" (one osascript-driven smoke script); otherwise stays documented-blocked.
6. **F6 — Word-level lyrics timing** (existing roadmap): independent, UI-adjacent.
7. **F7 — TAN Info residuals**: SKIP — accepted-risk Info items (04/06/07/08/09), revisit only at release review; TAN-08 (jar pinning) is the only one worth bundling into F3 packaging.

## F2 — conformance suite (2026-11-02)

New: `crates/sources/tests/registry_conformance.rs` (reopen persistence,
disabled→Disabled); resolver tests (below-request rule, TTL expiry,
negative cache, pinned reuse); search tests (exact paused-clock timeout,
caps, `search_all_with_local` local+remote rank-grouped entry point,
`normalize_row`); commands test (stale-token); yt test (`join_artist`).
Small implementations the tests demanded: below-request flagging in
`resolve`, `LocalSearch` seam + `normalize_row` in `search.rs`,
`repo::search_local`, credited-artists join in `yt.rs`. 159/0/5; stress 5
min 0 underruns. Full named-test list per group: REPORT.md §F2.

## F3 — real-provider verification (2026-11-02)

Files changed:
- `crates/sources/src/yt.rs`: `expiresAt` rename on `StreamResponse` (the
  URL-TTL clamp had silently never engaged) + regression test.
- `crates/library/src/metadata.rs`: `mp4_codec()` — ALAC/AAC distinguished
  inside MP4 containers (was hardcoded "MP4"; ALAC files misclassified as
  lossy) + fixture-driven format test.
- `crates/playback/src/pipeline.rs`: missing-file + local-fixture decode
  tests.
- `crates/sources/tests/f2_live_providers.rs` (NEW, #[ignore]): the
  5-track live verification harness (search → resolve ×10 → first-audio →
  seek → decode → killed-sidecar recovery) with per-track error capture.

Full tables + classification: REPORT.md §F3. mp3/vorbis local NOT RUN (no
encoder); true network-off NOT RUN (host disruption) — dead-sidecar
simulation covers the Rust-side contract.

## F4 — addon contract (2026-11-02)

`crates/sources/src/addon.rs` extended to the F3 contract: manifest
version/capabilities, `?quality=LOSSLESS|HIGH|LOW`, encrypted → Encrypted,
manifest hls/dash → Unsupported, in-band error → Permanent, canonical
codec-based lossless, header allowlist, base-URL validation at
construction (Result; https default + per-addon http opt-in + no
userinfo), per-hop same-host redirects, 4 MiB body caps, id/query caps,
stream-URL host validation (private hosts only when the addon base is
local). Call sites updated (registry warn-skip, health reject, main.rs
ladder skip); `source_add` is https-only (http opt-in UI = decision
required). Wire-name fixes: sampleRate/bitDepth renames, url optional for
in-band errors. 14 new mock tests + 2 in-crate policy tests; 174/0/7.
Full list: REPORT.md §F4.

## F5 — BUG-001 closure + F4 gap report (2026-11-02)

Controlled bounded stress (multi-agent; report in REPORT.md §F5): skip-walk
regression suite 15/15 under killpg watchdogs (RSS ≤ 7.9 MiB); real-pipeline
failing-entry walks via engine_demo 5/5 (RSS peak ~92 MiB → ~20 MiB, no
growth); 120 s real-app soak (157.4 MiB plateau, 0 stream errors). Verdict
(Auditor): runaway mechanisms eliminated in the exercised paths, scoped.

F4 checklist: 5 PASS, 1 FAIL (quality setting dead for playback — factory
hard-codes Best, never updated), 6 NOT RUN/partial with named gaps:
integrated mixed-search-play scenario, live combined seek, restart/restore
scenario, composition test for disabled-source→skip, engine-level Decode
test, and the unwired `repo::search_local`. Stream-info Query still not
implemented. Fixes deliberately NOT made in this stage (recovery + gap
report only) — the quality dead-setting is the top-priority gap.

## F5 — minimum UI over backend capabilities (2026-11-02)

New: `stream_info_get` (the one sanctioned read-only query; RodioFactory
records the last successful ResolvedMedia per entry, evicts on release/
invalidate; fallback = registered key source ≠ answering source; camelCase
DTO + 2 tests). `search_tracks` now returns unified local+remote rows
(`search_all_with_local` + `repo::search_local` — the F2 dead code is
wired). Frontend: Sources search tab (rank-grouped sections, per-source
status line, local:{id} rows — also fixes the always-undefined st.state
read), source ▲/▼ reorder buttons, explicit Resolving…/Buffering… text in
Lcd, nerd-stats-gated stream-info line with fallback/below-request flags.
Event-driven only (entry-change fetch + token guard); streamInfo not
persisted. Audit fixes folded in: last_resolved eviction, fetch race
guard, duplicated main.rs setup block collapsed (was double-emitting
scan://done). 51/51 vitest (14 new), 184/0/7 cargo, tsc + builds clean.
Capability map + recorded risks (EntryId space collision, cross-kind
reorder no-op): REPORT.md §F5.

## F5-finish (2026-11-02)

- EntryId: `REMOTE_ID_OFFSET` (1<<40) split the remote/mint-id spaces;
  mint sites (remote_track_upsert, autoplay) add it, queue_tracks_get
  subtracts it. Collision impossible by construction.
- Quality: RodioFactory holds `Mutex<StreamRequest>`; `quality_set` applies
  to the factory (next resolve only); startup reads the stored setting.
- Reorder: ▲/▼ hidden across kind boundaries (per-kind rank is the real
  ordering).
- Docs: REPORT.md §F5-finish; checklist NOT RUN (no GUI session).

## F6 — non-visual UX pass (2026-11-03)

Owner brief empty → non-visual scope only. Contrast audit: two light-scheme
failures fixed (`--systemTertiary` 40→46%, new `--healthDot` token); full
WCAG table in REPORT.md §F6. Added missing VoiceOver labels (settings
checkboxes, lyrics checkboxes, Lcd nav button); reduced-motion guard
extended to transitions. First paint re-measured 425 ms (F4 baseline
449 ms — improved). Keyboard map documented. 10k virtualization verified
by construction (TrackList ≥ 80 rows); interactive checks NOT RUN.

## F8 — launch-freeze fix (2026-11-03)

FullScreenPlayer gated on `useUi.fullScreen` in addition to `nowPlaying`
(restored queues mounted the full-window overlay at boot and the close
button only cleared an unread flag). Regression test
shell/FullScreenPlayer.test.tsx (3 render-gate cases). Root cause + runtime
evidence: REPORT.md §F8.

## F9 — real-music-play fixes (2026-11-03)

- rodio_factory.rs: nested-runtime resolve fix (Handle-aware; the engine
  task's prepare previously panicked — remote play could never pass
  prepare). Prefetch path resolves async directly.
- pipeline.rs: open_source http branch sniffs the container (first 64
  bytes) and hints the probe; decodes googlevideo DASH-branded MP4.
  Debug-only sink peak meter.
- yt.rs: `yt_stream_host_allowed` (https + googlevideo.com suffix) + 2
  tests.
- e2e.rs (NEW, debug-only): TANSEN_E2E=1 self-test driver S1–S12.
- Full evidence, break map, scenario table: REPORT.md §F9.

## BUG-002: Settings page crash — `undefined is not an object (evaluating 't.name')` (2026-11-03)

**Root cause:** `SourceWithHealth` (commands.rs:536) serializes `config` with
`#[serde(flatten)]`, so the wire row is FLAT — `{config_id, kind, name,
base_url, enabled, rank, health:{state,...}}` — with no nested `config`
object. `SettingsPage.tsx` SourcesSection destructured `{ config, health }`,
so `config` was always `undefined` and the first `config.name` read crashed
the whole Settings route to the error boundary. This is why "the settings
folder looked empty": the page died while drawing the source list (the user's
DB actually has 2 valid sources: YouTube Music + Local Files). Latent since
M10 — any row in `sources_get_with_health` triggers it.

**Fix (frontend only, no backend/IPC/architecture change):**
- `apps/desktop/src/pages/SettingsPage.tsx` — SourcesSection now maps the
  whole row: `row.config_id/kind/name/base_url/enabled` top-level,
  `row.health.state` for the dot (SourceHealth stays nested, internally
  tagged). Reorder/enabler/remove buttons read the same flat fields.
  `SourceRow` type rewritten to the flat wire shape; added `SourceConfig`
  interface for the `source_add`/`subsonic_add` return.
- `apps/desktop/src/pages/SettingsPage.test.tsx` (NEW) — regression test:
  mounts the REAL SettingsPage with the exact flattened wire row and asserts
  both rows render with their enabled/disabled states (the old shape would
  crash this mount).

**Verification:** `tsc --noEmit` clean; vitest 9 files / 55 tests PASS
(incl. new test); `npm run build` OK — new bundle
`SettingsPage-ossRg8Vj.js` contains the old `{config:t,health:p}` pattern
**nowhere**; debug app launches, sidecar boots, exits cleanly on SIGTERM.

**Follow-up found by the release build:** `mod e2e;` was compiled in release
too, but it calls the cfg(debug_assertions) sink peak meter
(`drain_peak`) → release build broke with E0425. Gated the module:
`#[cfg(debug_assertions)] mod e2e;` (main.rs) — matching the stated design
("compiled out of release"). Release build then succeeds; bundles rebuilt:
Tansen.app 64.70 MiB + Tansen_0.3.0_aarch64.dmg 42.49 MiB. Shipped binary
verified: embedded frontend = fixed chunk (old destructure pattern absent).

## Home page redesign (2026-11-03, owner brief: Apple Music Radio look)
- HomePage.tsx only: red gradient hero card ("Tansen" lockup, real library
  counts, white "Start Listening" pill → shuffleList over the real library),
  above the existing album/song shelves. albumShelf now iterates newest-first
  (tracks_get is insertion-ordered; "Recently Added" was showing the oldest).
- No radio/station tiles — Tansen has no radio feature; shelves stay on real data.
- tsc clean, 55 vitest tests pass, vite build OK, debug app boots.
