# Architecture

Tansen is a Tauri 2 desktop app: a React webview, a Rust host process, and —
for YouTube Music only — a local Kotlin sidecar JVM. There is no backend
service and no telemetry.

```
React UI (apps/desktop/src)
  │  Tauri commands + `playback_state` / `scan://*` / `downloads_changed` events
  ▼
Tauri shell (apps/desktop/src-tauri) — IPC validation, app wiring, OS integration
  ▼
app-core — state, playback task, watcher, autoplay, sidecar handle
  ▼
playback · library · sources · persistence · netguard
```

`app-core` has no Tauri dependency, which keeps the playback and library logic
testable in plain Rust. The shell is the only layer that knows about windows,
webviews, dialogs, and OS media integration.

## Workspace layout

| Path | Role |
|---|---|
| `apps/desktop/src` | React shell, pages, components, player mirror, lyrics, styles |
| `apps/desktop/src-tauri` | Tauri host: command surface, wiring, downloads, autoplay, Now Playing, radio, Spotify import |
| `crates/playback` | rodio/Symphonia pipeline, ranged-HTTP reader, SPSC ring, DSP, metrics |
| `crates/library` | folder scanning, lofty metadata, artwork extraction |
| `crates/persistence` | SQLite schema, migrations, repositories, grouped views |
| `crates/sources` | `MusicSource` trait, registry, search fan-out, resolver, addons, Subsonic, YouTube sidecar source |
| `crates/app-core` | `AppState`, task-based playback engine, playback types, library watcher, sidecar handle |
| `crates/netguard` | outbound URL checks and DNS/IP validation shared by media fetches |
| `sidecar-overlay` | Kotlin sidecar module; built against the pinned upstream shared module |
| `scripts/build-sidecar.sh` | the only supported sidecar build path |
| `tests/fixtures` | generated audio fixtures used by scanner and format tests |

## Playback path

1. The UI sends one serialized `Command` through the `engine_command` Tauri
   command. Commands are `Play`, `Pause`, `Resume`, `Next`, `Prev`, `Seek`,
   `SetVolume`, `Enqueue`, `Remove`, `Move`, `ClearQueue`, `RebuildOutput`,
   and `SetRepeat`.
2. `app_core::playback_engine::Engine` owns the queue and the current
   `PlaybackState` inside one task. The command channel is bounded
   (`COMMAND_BUFFER`); backpressure is preferred over dropping commands.
3. The task publishes every state change on a `watch` channel. The shell
   forwards it to the webview as the `playback_state` event; the UI renders
   only from that event.
4. Media work goes through the `PipelineFactory` seam. The real
   implementation is `rodio_factory::RodioFactory`; engine tests install a
   fake so queue and transition laws are tested without an audio device.
5. `crates/playback/src/pipeline.rs` runs a decode thread that fills an SPSC
   sample ring. The audio callback only pops samples, applies volume, EQ,
   loudness, and fades, and increments the frame counter — no locks,
   allocations, or I/O on the callback thread.

The queue body never crosses IPC on every change. The frontend keeps an
ordered mirror and re-reads display metadata from `queue_tracks_get` only when
the engine publishes a new `queue_rev`. The mirror persists in `localStorage`;
startup re-seeds the engine from it, paused at the saved position, and remote
URLs are resolved fresh.

### Local and remote media

- Local files are opened directly by the pipeline.
- Remote media uses `crates/playback/src/remote.rs`: HTTP Range reads with a
  bounded read-ahead buffer, a redirect policy that cannot downgrade HTTPS or
  pivot to private hosts, and re-resolution when a URL expires (403/410).
- `crates/sources` resolves a `TrackKey` to `ResolvedMedia` through the
  resolver: per-key single-flight, a negative cache, one transient retry, and
  an optional ranked fallback ladder. Resolve-ahead warms the next queue entry
  off the audio path.

## Library path

`library::scanner` walks configured folders in two phases: a parallel
stat/parse phase that reads tags only for changed or missing files, then a
batched write phase that commits chunks in one transaction. Files that vanish
are marked `missing`, not deleted.

`app_core::watcher` watches the same folders with a debounce, skips scans while
a manual scan is running, and publishes `scan://progress` / `scan://done`
through the shell. The watcher is gated by the `watch_folders` setting.

Artwork is extracted into a content-hash cache under app data and served to the
webview through the path-scoped `art://` protocol.

## Catalogue sources

`crates/sources/src/lib.rs` defines the `MusicSource` trait and the quality
tiers. `registry.rs` stores the user's source roster and builds only the
enabled sources. `search.rs` fans out searches and injects the local-library
seam; `resolver.rs` turns a key into media; `matcher.rs` provides strict
metadata matching for fallback candidates.

Built-in adapters:

- **YouTube Music** (`yt.rs`) — talks to the Kotlin sidecar. The JVM is
  spawned and supervised by Rust, bound to `127.0.0.1`, and killed when the app
  exits.
- **Local** — backed by `crates/persistence`.
- **Subsonic / OpenSubsonic** (`subsonic.rs`) — credentials live in the macOS
  Keychain; requests use the server's REST API.
- **HTTP addons** (`addon.rs`) — manifest validation, search, and stream
  resolution with a per-host quiet period after HTTP 429.

The catalogue is off by default and enabled in Settings → Sources.

## Sidecar

`sidecar-overlay/sidecar` is the source of truth for the Kotlin module. It
builds against the pinned upstream `:shared` module, so it is not a standalone
Gradle project. `scripts/build-sidecar.sh` checks out the pinned upstream
commit, applies the overlay, builds `sidecar-all.jar`, and copies it into the
Tauri resources directory. The HTTP contract is in
[`docs/sidecar-api.md`](sidecar-api.md).

## Persistence

`crates/persistence/src/lib.rs` owns the schema and forward-only migrations.
Repositories are split by domain under `repo/`:

- `settings.rs` — key/value settings
- `tracks.rs` — folders, track metadata, scan batches, search
- `playlists.rs` — playlists, ordered membership, remote membership, queue rows
- `downloads.rs` — the persistent download queue
- `recommendations.rs` — listening events, likes, follows, dismissals

`views.rs` contains the grouped album/artist/history queries. Writes go
through the single WAL writer; query-heavy paths can open a read-only
connection that SQLite refuses to write through.

## Downloads

The download manager (`apps/desktop/src-tauri/src/downloads.rs`) keeps its
queue in SQLite and survives restarts. Subsonic servers, direct addon
streams, and local-file copies can be downloaded; YouTube Music and Spotify
audio cannot, and the UI says so. Status changes are pushed to the frontend
as `downloads_changed`.

## Frontend

- `src/shell` — sidebar, player pill, full-screen player, Up Next, lyrics
  panel; search, menus, dialogs, and the full-screen player own their keys.
- `src/pages` — library, albums, artists, playlists, search, discovery,
  radio, settings, reorder, Spotify import.
- `src/state/player.ts` — the player mirror and the only command sender.
- `src/state/audioSettings.ts` — crossfade, EQ, loudness, output depth.
- `src/lyrics` — the vendored provider/parser/clock system.

Routing is a local lazy registry in `pageRegistry.tsx`, mounted by a
`createBrowserRouter` in `routes.tsx`; there is no server-side router.

## Platform integration

- **Now Playing / media keys** — `platform_macos.rs` uses the MediaPlayer
  framework through `objc2`. Updates ride engine publishes; remote commands map
  onto engine `Command`s.
- **Sleep/wake and device changes** — the shell watches the default output
  device and system sleep state, then sends `RebuildOutput`.
- **Radio** — directory rows are validated by `radio.rs` and `netguard` before
  they reach the engine; stations play as live, unseekable streams.
- **Autoplay** — an app-crate task appends related tracks when the queue tail
  starts. It is off by default and never runs in the engine itself.

## Concurrency rules

- The engine task is the sole owner of playback state; callers send commands,
  they do not mutate state directly.
- The audio callback never blocks, locks, allocates, or performs I/O.
- Scanning and streaming run off the UI thread.
- The sidecar is a child process owned by the app and shut down on exit.
- Outbound URLs pass through `netguard`/allowlists before any fetch.

## Testing

```bash
cargo test --workspace
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
```

The queue/transition laws, scanner pipeline, persistence repositories, source
registry/search, resolver fallback, range reader, and download queue all have
unit or integration coverage. Live network and sidecar suites are `#[ignore]`d:

```bash
cargo test -p sources --test live_providers -- --ignored --nocapture
cargo test -p sources --test yt_measure -- --ignored --nocapture
cargo test -p sources --test yt_supervisor -- --ignored --nocapture
cargo test -p library --test scan_timing -- --ignored --nocapture
```
