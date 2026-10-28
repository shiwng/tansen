# BitChord Desktop

A Rust + React desktop music player for macOS and Windows, re-implementing the
architecture and experience of [BitChord](https://github.com/kushagrasinghx/BitChord)
(a YouTube Music client) with a local-first core. **Not affiliated with or
endorsed by BitChord, YouTube, or Google.**

Status: **M2 + phase-5 lyrics data layer** — the UI is the upstream web player's
design (vendored, GPL-3.0): glass sidebar, player pill/LCD, full-screen player
with mesh backdrop, Up Next, and the full lyrics system. Local library,
playlists, queue persistence and real playback work end to end. Catalogue
discovery is gated on the PoToken spike. Feature status:
[docs/feature-matrix.md](docs/feature-matrix.md).

## Stack

- **Tauri 2** shell, **Rust** core, **React 19 + TypeScript + Tailwind 4** UI
- **rodio + Symphonia** — decode/playback (MP3, FLAC, MP4/AAC, OGG/Opus, WAV, AIFF, WavPack, APE)
- **rusqlite** (bundled SQLite) — settings, library, playlists, history
- **lofty** — tags + embedded artwork; **souvlaki** — OS media keys
- No backend server; no telemetry.

## Run (dev)

Prereqs: Rust 1.85+, Node 20+, pnpm.

```bash
pnpm install --dir apps/desktop
cd apps/desktop && pnpm tauri dev
```

## Build

```bash
cd apps/desktop && pnpm tauri build   # macOS .app/.dmg (Windows .msi on Windows)
```

Data lives in the OS app-data dir (`bitchord-desktop`); SQLite WAL mode; schema
migrations are versioned in `crates/persistence/src/lib.rs`.

## Layout

```
apps/desktop/       React UI (src) + Tauri shell (src-tauri)
crates/playback/    audio engine + pure queue model (unit-tested)
crates/library/     scanner, lofty metadata, artwork cache
crates/persistence/ SQLite schema + repositories
crates/app-core/    state + services (no Tauri dependency)
docs/               BitChord audit, architecture, feature matrix, plan
tests/fixtures/     generated audio fixtures for tests
```

## Testing

```bash
cargo test                                # 15 Rust tests: queue algebra, scanner e2e (real WAV), persistence views
pnpm --dir apps/desktop test              # 26 vitest tests: lyric parsers, LyricClock, repository selection rules
pnpm --dir apps/desktop build             # tsc strict + vite production build
```

The queue model (`crates/playback/src/queue.rs`) and the scanner path are the
regression core; playback itself needs an output device and is smoke-tested by
launching the app.

## Architecture

See [docs/architecture.md](docs/architecture.md). Short version: the audio
engine owns a dedicated thread (rodio's output stream is not `Send`); the UI
talks to it through commands and `EngineEvent`s only; the queue is a pure,
tested model; app-core has no Tauri dependency; scanning is bounded (rayon)
and never blocks the UI.

## Reference audit

[docs/audit-bitchord.md](docs/audit-bitchord.md) documents the original
Kotlin app (repo @ v1.8.1 + supplied APK, same version) — modules, playback
internals, source resolution, lyrics pipeline, downloads, auth, integrations —
with file/line evidence, and what each part means for this port.

## License & attribution

GPL-3.0-or-later — matching upstream so logic and UI translations are
license-compatible. The UI is **vendored from BitChord's own GPL web player**
(`BitChord/web/app/src` — tokens, shell, track list, lyrics system); those
files keep their upstream structure and this project adds nothing that would
restrict them. Upstream: BitChord by Kushagra Singh and contributors.
Known asset flag: the lyrics font (SF Pro Display woff2) is Apple-proprietary
and ships upstream without its own license — kept for design parity, flagged
here for distribution review. DM Sans (Google, OFL) is self-hosted for the UI
face.

## Known limitations (honest list)

- YouTube Music / JioSaavn streaming is **not implemented**: web clients
  require Google's BotGuard PoToken; a desktop feasibility spike is scheduled
  before any catalogue UI (see plan M3). Nothing here fakes it.
- Lyrics, scrobbling, Discord, Listen Together, downloads: designed but not
  started — marked 📋 in the feature matrix, not simulated.
- Windows build untested (no Windows host in this environment); SMTC wiring is
  in place behind `#[cfg]`-free souvlaki APIs.
- Icons are placeholders; art theming and animations land in phase 5.
