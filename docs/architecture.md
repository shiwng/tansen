# Architecture

Tansen is a Tauri 2 application: Rust owns state, audio and background work; React/TypeScript owns presentation. The mapping from the original Kotlin app is in `docs/audit-bitchord.md` §10; this document describes what exists now and what lands where.

## Workspace layout

```
tansen/
├── apps/desktop/
│   ├── src/              React UI (pages, components, zustand stores)
│   └── src-tauri/        Tauri shell: commands, event forwarding, art protocol, media keys
├── crates/
│   ├── app-core/         Application state + services (no Tauri dependency)
│   ├── playback/         Audio engine (rodio/symphonia) + pure queue model
│   ├── library/          Folder scanning, metadata (lofty), artwork cache
│   └── persistence/      SQLite schema, migrations, repositories (rusqlite)
├── docs/                 Audit, architecture, feature matrix
└── tests/                (integration tests as phases add them)
```

The proposed `frontend/` top-level directory was merged into `apps/desktop/src` — one npm package, one build. `sources`, `lyrics`, `audio-processing` and `integrations` crates are introduced with their phases, not pre-created.

## Vendored UI (upstream web player)

The desktop UI is the upstream web player's design, vendored file-by-file from
`BitChord/web/app/src` (GPL-3.0, same as this project): `styles/tokens.css`,
`styles/global.css`, the shell (`AppShell`, `Sidebar`, `PlayerBar`, `Lcd`,
`SidePanel`, `UpNext`, `FullScreenPlayer`, `LyricsPanel`, `LyricsSettings`,
`useScrubber`, `useKeyboardShortcuts`, `MeshBackdrop`), the music components
(`TrackList`, `Artwork`, `Lockups`, `DetailHeader`, `ShelfView`, `Menu`, …),
`state/ui.ts`, and the entire `src/lyrics/*` system with its tests. Two layers
were rewritten for the desktop:

1. **`state/player.ts`** — same field/action contract as upstream's store, but
   backed by Tauri commands + `player://event`s instead of YouTube decks.
   `getPlayhead()` interpolates the 10 Hz engine ticks; `getAudio()` returns
   null (consumers use the playhead).
2. **`lyrics/http.ts`** — the upstream browser-extension bridge becomes a
   host-allowlisted Rust `proxy_get` command (reqwest), so CORS-less lyric
   hosts work without any extension.

Catalogue-only features (video surface, motion artwork, canvas, NerdStats,
favorites, party sync) are stripped at the import sites, not stubbed to
pretend.

## Data flow

```
React (zustand stores)
   │  invoke()                      ▲  emit("player://event", …)
   ▼                                │
Tauri commands (src-tauri/commands.rs)
   │
app-core::AppState ──── playback::Engine ─── audio thread (rodio sink)
   │        │              │    ▲
   │        │              │    └── EngineEvent broadcast
   │        │              └── spawn_event_forwarder (src-tauri): events → webview + OS media keys (souvlaki)
   ▼        ▼
persistence   library::Scanner (rayon, spawn_blocking)
(SQLite)         └── lofty metadata + art:// protocol for cached artwork
```

- **Playback never touches the UI thread.** The engine owns a dedicated OS thread because rodio's `OutputStream` is not `Send`; everything else talks to it over an mpsc channel. State changes flow back as `EngineEvent`s on a tokio broadcast channel.
- **The queue model is pure** (`playback/src/queue.rs`): `Queue` + `QueueOp` + `QueueEffect`, fully unit-tested without an audio device. The engine applies effects; the UI receives snapshots. UI and engine can never disagree about order.
- **app-core has no Tauri dependency**, so services are testable and a future CLI/other shell could reuse them. The shell only translates commands/events and registers the `art://` protocol (serves cached artwork bytes to the webview without exposing the filesystem).
- **Scanning is bounded** (rayon's default pool) and runs in `spawn_blocking`; progress is forwarded as events. Metadata/artwork for unchanged files is skipped via a path+mtime content hash.

## Original-module mapping

| BitChord (Kotlin) | Here (Rust/TS) | Status |
|---|---|---|
| `shared` Models | `persistence::models` + `src/types.ts` | slice |
| `QueueCoordinator`/`QueueTimeline`/`QueueWindow` | `playback::queue` (simplified; window/tiers later) | slice core |
| `PlaybackService` (Media3) | `playback::engine` (rodio) + `src-tauri` event forwarder | slice |
| `CrossfadeController`, DSP chain, `AudioCache` | `audio-processing` crate | phase 6 |
| `MusicSource`/`SourceResolver`/`SourceRegistry` | `sources` crate (same 3-method interface, racing resolver) | phase 4 |
| ModuleManager (QuickJS) / AddonClient (HTTP) | `sources::addon` (HTTP first), QuickJS via `rquickjs` | phase 4 |
| `LocalMediaRepository` (MediaStore) | `library` scanner (walkdir + lofty) | slice |
| `LyricsRepository` + parsers | `lyrics` crate (port pure parsing + racing selection) | phase 5 |
| `ArtworkPalette`/`PaletteQuantizer` | artwork theming in UI (port quantizer math) | phase 5 |
| `AppSettings` | `settings` table + Settings page (keys added per phase) | slice → |
| `Downloads`/`DownloadService`/taggers | `integrations` or `sources` support | phase 7 |
| `ScrobbleManager`/LastFM/ListenBrainz, kizzy Discord, ListenTogether | `integrations` crate (one adapter module each) | phase 7 |
| Compose `sharedUi` screens | React pages/components | slice → |

## Concurrency rules (from the brief, enforced here)

1. Scanning, artwork extraction and future lyrics fetches never block UI: `spawn_blocking` / rayon / dedicated threads.
2. Bounded concurrency: rayon pool for scans; tokio semaphores for future network fan-out (BitChord's racing-resolver pattern translates directly).
3. Single SQLite connection behind a mutex; transactions keep multi-statement writes atomic. Long work stays outside the lock.
4. Errors are surfaced (`TrackError` events, scan error counts, command `Result` → JS rejects); nothing is swallowed silently.
