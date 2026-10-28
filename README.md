# BitChord Desktop

A Rust + React desktop music player for macOS and Windows, re-implementing the
architecture and experience of [BitChord](https://github.com/kushagrasinghx/BitChord)
(a YouTube Music client) with a local-first core. **Not affiliated with or
endorsed by BitChord, YouTube, or Google.**

Status: **phase 3 vertical slice** — local library + real playback work end to
end; catalogue discovery, lyrics and integrations are staged behind
[docs/plan.md](docs/plan.md). Feature status: [docs/feature-matrix.md](docs/feature-matrix.md).

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
cargo test                        # queue algebra, scanner end-to-end (real WAV), units
pnpm --dir apps/desktop build     # tsc strict + vite production build
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

## License

GPL-3.0-or-later. BitChord is GPL-3.0; behavioral translation of its logic
must carry a compatible license, so this project adopts GPL-3.0. Upstream
attribution: BitChord by Kushagra Singh and contributors. Third-party
dependency licenses are listed in `docs/audit-bitchord.md` §10 and Cargo/npm
metadata; no proprietary artwork or branding is copied.

## Known limitations (honest list)

- YouTube Music / JioSaavn streaming is **not implemented**: web clients
  require Google's BotGuard PoToken; a desktop feasibility spike is scheduled
  before any catalogue UI (see plan M3). Nothing here fakes it.
- Lyrics, scrobbling, Discord, Listen Together, downloads: designed but not
  started — marked 📋 in the feature matrix, not simulated.
- Windows build untested (no Windows host in this environment); SMTC wiring is
  in place behind `#[cfg]`-free souvlaki APIs.
- Icons are placeholders; art theming and animations land in phase 5.
