# Tansen

A Rust + React desktop music player for macOS and Windows, named after the
legendary musician. Its architecture, design and features follow
[BitChord](https://github.com/kushagrasinghx/BitChord) (a YouTube Music client)
and borrow from its GPL code under the same license. **Not affiliated with or
endorsed by BitChord, YouTube, or Google.**

Status: **local-first player + YouTube Music streaming**. The UI is the
upstream web player's design (vendored, GPL-3.0): glass sidebar, player pill
with LCD seek line, full-screen player with mesh backdrop, Up Next, and the
full lyrics system. Local library, playlists, queue persistence, and real
playback work end to end; YouTube Music streams through a local Kotlin sidecar
behind a Settings toggle. Feature status: [docs/feature-matrix.md](docs/feature-matrix.md).
Work log and what's-not-working: [REPORT.md](REPORT.md).
How everything is built and connected: [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Stack

- **Tauri 2** shell, **Rust** core, **React 19 + TypeScript + Tailwind 4** UI
- **rodio + Symphonia** — decode/playback (MP3, FLAC, MP4/AAC, OGG/Opus, WAV, AIFF, WavPack, APE); remote streams play through a ranged-HTTP reader (no full download)
- **rusqlite** (bundled SQLite, WAL) — settings, library, playlists, history, recoverable queue
- **lofty** — tags + embedded artwork; **souvlaki** — OS media keys (Now Playing / SMTC / MPRIS)
- YouTube Music via a **Kotlin sidecar** (`../BitChord/sidecar/`): upstream's GPL client served over 127.0.0.1 HTTP, spawned and supervised by the app (`docs/sidecar-api.md`). Off by default — the Settings → Sources toggle.
- No backend server; no telemetry.

## Run (dev)

Prereqs: Rust 1.85+, Node 20+, pnpm. For YouTube Music: JDK 17 (Gradle uses
`JAVA_HOME`) — the sidecar jar builds from pinned upstream sources inside this
repo's overlay and is packaged as a Tauri resource:

```bash
./scripts/build-sidecar.sh        # clones/checks out BitChord v1.8.1, builds sidecar-all.jar
pnpm install --dir apps/desktop
cd apps/desktop && pnpm tauri dev # jar is discovered via cwd/exe/resource paths
```

`tauri build` bundles the jar into the .app (`Contents/Resources/resources/`),
where the packaged app finds it without any dev checkout. Enable YouTube Music
in Settings → Sources. Library features need no sidecar. CI (`.github/workflows/ci.yml`)
runs the same steps on macOS; there is no Windows workflow yet — see REPORT.md.

## Build

```bash
cd apps/desktop && pnpm tauri build   # macOS Tansen.app/.dmg (Windows .msi on Windows)
```

Data lives in the OS app-data dir (`~/Library/Application Support/tansen` on
macOS; pre-rename dirs migrate automatically). SQLite WAL mode; schema
migrations are versioned in `crates/persistence/src/lib.rs`.

## Layout

```
apps/desktop/       React UI (src) + Tauri shell (src-tauri)
crates/playback/    audio engine (local + ranged-HTTP streaming) + pure queue model
crates/library/     scanner, lofty metadata, artwork cache, watcher support
crates/persistence/ SQLite schema + repositories
crates/sources/     MusicSource trait, addon adapter, YouTube sidecar source
crates/app-core/    state + services (no Tauri dependency)
docs/               upstream audit, architecture, feature matrix, plan, sidecar API
sidecar-overlay/    Kotlin sidecar module (source of truth; built vs pinned upstream)
scripts/            build-sidecar.sh — pinned-tag sidecar build into Tauri resources
tests/fixtures/     generated audio fixtures for tests
```

## Testing

```bash
cargo test                                # Rust tests: queue algebra, scanner e2e, range-reader streaming, persistence
cargo test -p sources --test yt_sidecar -- --ignored --nocapture   # live YouTube round-trip (needs jar + network)
pnpm --dir apps/desktop test              # vitest: lyric parsers, LyricClock, repository selection, reorder
pnpm --dir apps/desktop build             # tsc strict + vite production build
```

The queue model (`crates/playback/src/queue.rs`) and the scanner path are the
regression core; playback itself needs an output device and is smoke-tested by
launching the app. The sidecar round-trip is the YouTube canary.

## Architecture

See [IMPLEMENTATION.md](IMPLEMENTATION.md) for the full end-to-end explanation
(stack, every module and file, how they connect at runtime). Short version: the
audio engine owns a dedicated thread (rodio's output stream is not `Send`); the
UI talks to it through commands and `EngineEvent`s only; the queue is a pure,
tested model; app-core has no Tauri dependency; scanning and streaming never
block the UI; the sidecar is created/owned by `crates/sources` and killed on
drop.

## Reference audit

[docs/audit-bitchord.md](docs/audit-bitchord.md) documents the original Kotlin
app (repo @ v1.8.1 + supplied APK, same version) — modules, playback internals,
source resolution, lyrics pipeline, downloads, auth, integrations — with
file/line evidence, and what each part means for this port.

## License

GPL-3.0-or-later — matching upstream so logic and UI translations are
license-compatible. The UI is **vendored from BitChord's own GPL web player**
(`BitChord/web/app/src` — tokens, shell, track list, lyrics system); those
files keep their upstream structure and this project adds nothing that would
restrict them. Upstream: BitChord by Kushagra Singh and contributors. The
Tansen name and the app mark (three sound bars) are this project's own; no
BitChord branding is used. Known asset flag: the lyrics font (SF Pro Display
woff2) is Apple-proprietary and ships upstream without its own license — kept
for design parity, flagged here for distribution review. DM Sans (Google, OFL)
is self-hosted for the UI face.

## Known limitations (honest list)

- YouTube streaming needs the sidecar jar + Java 17, and stays **off by
  default** (`catalogue_sources` setting). It resolves today without a BotGuard
  token — observed, re-tested by the round-trip test, not guaranteed.
- HLS/DASH manifests are not supported (progressive streams only); word-level
  lyric timings are not yet exported by the sidecar.
- Crossfade/EQ/speed/sleep timer and the phase-7 integrations
  (scrobbling, Discord, Listen Together, downloads) are designed but not
  implemented — see the feature matrix.
- Windows build is untested (no Windows host in this environment); macOS is the
  verified platform.
- Icons are placeholders; art theming and animations land with phase 5/6 polish.
