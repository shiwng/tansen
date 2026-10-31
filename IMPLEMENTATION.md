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
| `src/lib.rs` | `Db` (one `Connection` behind a `Mutex`), WAL + foreign-key pragmas, ordered migrations keyed on `PRAGMA user_version`. Migration 1: settings, library_folders, tracks, playlists, playlist_tracks, play_history. Migration 2: playback_state (the recoverable queue). |
| `src/models.rs` | `Track` (a library row), `TrackRef` (what the queue/player need: id, title, artist, album, duration, art path, path), `Folder`. |
| `src/repo.rs` | Typed queries: settings get/set, folders, track upsert-by-path (rescan-idempotent), mark/purge missing, playlist CRUD + append + reorder, history insert. |
| `src/views.rs` | Read models: `albums()` / `artists()` groupings (SQL, with the album_artist-fallback rule), `album_tracks()`, `artist_tracks()`, `history()`, and `playback_state` save/load/clear. |
| `tests/views.rs` | Album/artist grouping rules + playback_state round-trip. |

Connection rules: short transactions only; scanning/artwork/decoding never hold
the lock; `user_version` migrations are append-only.

### 3.2 `playback` — audio engine + queue model

| File | Role |
|---|---|
| `src/queue.rs` | **Pure** queue state: `Queue`, `QueueOp` (Load/Insert/RemoveAt/Move/Jump/Next/Prev/SetRepeat/Clear), `QueueEffect` (what the engine must do), `QueueSnapshot` (what the UI sees). Repeat modes, history for Previous, index tracking through moves. Fully unit-tested without audio. |
| `src/engine.rs` | The audio thread. Owns rodio `OutputStream` (not `Send` — it must be created and kept on this thread; `Sink` is Send+Sync). Applies `QueueOp`s, emits `EngineEvent`s on a tokio broadcast channel, ticks at 10 Hz for position/gapless, preloads the next decoder ~2 s before track end (gapless), detects the handoff by a position drop, pauses/plays/seek/volume, `PatchUrl` for remote rows. |
| `src/remote.rs` | `HttpRangeReader`: `Read + Seek` over HTTP using 512 KiB `Range` chunks (googlevideo throttles unbounded reads; ranges run at line rate). MP4 `moov` seeks answered over the wire. Must be created/dropped off the async runtime (the audio thread qualifies). |
| `src/cache.rs` | `StreamCache`: bounded (512 MiB LRU) disk cache for streamed tracks, sha1-of-URL keyed, complete-file-only. The engine plays cached copies from disk; the resolve-ahead service prefetches the next track into it. |
| `src/probe.rs` | `decode_rms(reader, window)`: Symphonia decode without a device — the test oracle for "this stream contains sound". |

Event → consumer map:

```
EngineEvent (broadcast, 128 slots)
 ├─► src-tauri forwarder ─► webview "player://event"  (UI store updates)
 ├─► src-tauri forwarder ─► souvlaki (Now Playing metadata + 2 Hz position)
 ├─► history.rs thread    ─► play_history rows (local tracks only)
 ├─► persist_state thread ─► playback_state table (≤0.8 s after change, or ≥2 s position drift)
 └─► resolve-ahead thread ─► Command::PatchUrl (remote placeholders → real URLs)
```

Commands enter via `Engine::send`/`command_tx` (an mpsc the audio thread
drains with a 100 ms timeout so ticks still run). Decoders erase to
`Box<dyn Source<Item = f32> + Send>` so local and remote tracks share one queue.

### 3.3 `library` — folders to rows

| File | Role |
|---|---|
| `src/metadata.rs` | lofty extraction → `Track` (title falls back to filename), codec naming, extension whitelist. |
| `src/art.rs` | Embedded picture → `art/<sha256(path+mtime)>.<ext>` content-hash cache; unchanged files never rewrite; `prune_orphans`. |
| `src/scanner.rs` | walkdir over registered folders (bounded by rayon) → upsert → mark-missing (kept, not deleted) → artwork. Progress callback every 25 files. |

### 3.4 `sources` — the catalogue abstraction

| File | Role |
|---|---|
| `src/lib.rs` | The `MusicSource` trait (`health` / `search` / `stream`), `StreamRequest` (Lossless / Capped / Best — upstream semantics), `StreamFormat` (lossless/atmos/bitrate + `is_better_than` comparison), `SourceHealth` (Ok / Unreachable / **Rejected** — a down server is retryable, a refusing one is not), `CatalogTrack`, `SourceKind` (Addon / YouTube, ranked). |
| `src/addon.rs` | HTTP addon adapter: `manifest.json` / `search` / `stream/{id}` with SharedCalls-style dedup (10/10/5 min) and a 429 quiet period. Integration-tested against a dependency-free TcpListener mock. |
| `src/registry.rs` | `SourceConfig` list persisted in settings (`sources_config`), kind+rank ordering, `build_active()` builds enabled addon sources; the YouTube source joins only when `catalogue_sources` is on and the sidecar launches. |
| `src/yt.rs` | `YtMusicSource`: spawns `java -jar sidecar-all.jar --port <free>`, health-polls (400 ms × 20 s) on a scoped OS thread (blocking client — never inside a runtime), kills the JVM on drop. Java discovery rejects the macOS stub (`/usr/bin/java` runs but exits 1 — success means exit 0). Translates the sidecar JSON contract to the trait. |
| `tests/addon_source.rs`, `tests/yt_sidecar.rs` | Mock-server integration tests; the ignored-by-default round-trip (network) proves search → stream → decode 5 s via the engine's ranged reader. |

### 3.5 `app-core` — the application brain

| File | Role |
|---|---|
| `src/state.rs` | `AppState`: owns Db, Engine, Scanner, Media, scan_events channel, cached `YtHandle`. Every service the UI drives lives here (folders/tracks/scan/playlists/settings/play_remote/…). `restore_playback()` re-loads the saved queue paused at the saved index (dead local files dropped, remote entries kept). `migrate_legacy_data_dir()` renames pre-0.3.0 `bitchord-desktop` → `tansen` once. |
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
| `src/main.rs` | Tracing init → legacy data-dir migration → Builder: dialog plugin, `art://` protocol (serves cached artwork bytes, CORS header so the canvas can read pixels), setup (AppState::open, restore, media keys on the main thread, event forwarders), 30+ registered commands. The lyrics proxy disk-caches cacheable provider responses for 24 h (token endpoints excluded). |
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
  (flat fields the shell reads) but is backed by Tauri: actions are `invoke`
  calls; `player://event`s mutate it; `getPlayhead()` interpolates the 10 Hz
  engine ticks. Remote detection: any track whose `videoId` is not `local:`
  routes play/next/queue to `player_play_remote`.
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

---

## 7. The command surface (Tauri ⇄ React)

Groups: settings, library (folders/scan/tracks), playback (load/toggle/seek/volume/repeat/next/prev/stop), queue (snapshot/insert/move/remove/jump/clear), playlists (CRUD + reorder), views (albums/artists/album_tracks/artist_tracks/history), lyrics proxy (`proxy_get`, host-allowlisted), catalogue (`yt_search`, `player_play_remote`), sources (CRUD + health).

Events back: `player://event`, `scan://progress`, `scan://done`.

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
| `cargo test` (playback) | 14 | Queue algebra (moves/repeat/next-prev), placeholder patching, range reader read/seek/length, decode of ranged-only audio, StreamCache disk replay + LRU eviction |
| `cargo test` (library/persistence/sources) | 10 | Migration-2 round-trip, album/artist grouping rules, scanner e2e on a real WAV incl. rescan dedupe + missing marking, addon protocol vs mock server, registry ordering |
| `cargo test -p sources --test yt_sidecar -- --ignored` | 1 | The live path: sidecar launch → search → `prefer=aac` stream → 5 s decode through the ranged reader (needs network + built jar) |
| `cargo test` (app-core) | 3 | Watcher phases, restart persistence incl. remote-entry restore |
| `pnpm test` (vitest) | 32 | Lyric parsers (15), LyricClock (10), repository selection (1), reorderMove (6) |
| `pnpm build` | — | tsc strict + production bundle |
| `scripts/bench/run.sh` | — | M1 baseline harness: release build, 5 runs of start→first-paint, memory footprint (app process; sidecar JVM when running), 60 s idle and 60 s playing CPU (median table). Env-gated autoplay hook, no behavior change. |

Manual smoke: app launch, migration log, UI render (browser DOM capture).
