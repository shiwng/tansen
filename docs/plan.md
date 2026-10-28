# Implementation plan and milestones

Each milestone ends in something runnable and verifiable. "Verified" means the
check listed here was actually run, not that the code compiles.

## M1 — Phase 2 + 3 vertical slice ✅ (this delivery)

Runnable desktop app: navigation shell (Home/Search/Library/Settings), SQLite
persistence (settings, folders, tracks, playlists, history), folder import →
scan (lofty metadata + artwork cache) → library table with filter/sort, real
audio playback (rodio + symphonia) with transport, seek, volume, repeat,
gapless-advance queue, `art://` artwork protocol, OS media keys (souvlaki),
track-error surfacing, keyboard shortcuts.

- Verified by: `cargo test` (13 tests: queue algebra ×8, scanner end-to-end on
  a real WAV, metadata/art units), `pnpm build` (tsc strict + vite), app
  launch smoke test with logs clean.
- Known scope cuts: playlist UI, queue reorder drag, queue persistence across
  restarts, window-state restore — all next.

## M2 — Finish phase 3

1. Queue persisted across restarts; restore position.
2. Playlist UI (create, add tracks, reorder, play) + History page.
3. Scan watcher (`notify`) for automatic re-index; prune-orphan artwork GC.
4. Album/artist grouping views (port `LocalGroups` stable-id concept).
- Verify: restart persistence test; playlist CRUD integration test; scan of a
  1k-file fixture under a time budget.

## M3 — Phase 4 sources

1. `sources` crate: `MusicSource` trait (health/search/stream + `StreamRequest`
   tiers), `SourceRegistry` with persisted order, `SourceResolver` with racing
   `bestAcross` + fallback rules (port of the documented ordering, no Kotlin).
2. Addon HTTP adapter first (manifest/search/stream), with health checks in
   Settings → Sources screen.
3. QuickJS module adapter (`rquickjs`), two-export contract, sandboxed `fetch`.
4. YouTube Music: feasibility spike for BotGuard PoToken on desktop
   (WebView minter). **If it cannot be made to work, the feature is documented
   as blocked, never simulated.**
- Verify: integration test against a local mock addon server (search →
  stream → play); resolver unit tests for rank/fallback/quality rules.

## M4 — Phase 5 lyrics + artwork

1. `lyrics` crate: provider trait, raced selection (word > line, priority),
   ISRC identify pass, TTML/LRC/QRC parsers (GPL-compatible translation of the
   pure algorithms), LRCLIB provider first, disk cache.
2. Now-playing lyrics screen with `LyricClock`-style smoothed clock; word
   animation only for word-timed data.
3. Artwork palette: median-cut quantizer (port of the specified algorithm) →
   dynamic theming; animated artwork only where a provider supplies it.
- Verify: parser round-trip tests (fixtures committed), clock unit tests,
  provider test against LRCLIB live (gated, tolerated failure).

## M5 — Phase 6 advanced audio

1. Crossfade: second sink + equal-power gain ramps, handoff at curve midpoint.
2. EQ: biquad SVF chain as a rodio Source wrapper; presets; limiter after EQ.
3. Speed (`set_speed`), skip-silence wrapper, sleep timer UI.
4. Measure: gapless seam < 5 ms, crossfade within ±50 ms of target, seek
   accuracy on mp3/flac/m4a/opus/wav.
- Verify: automated audio-tap test (loopback or rendered checksums) + manual
  listening checklist documented in `docs/`.

## M6 — Phase 7 integrations (individually gated)

Scrobbling (Last.fm auth flow + ListenBrainz token), Discord RPC (opt-in,
user-token gateway), Listen Together client (protocol documented; works with
the upstream Go server or a self-hosted one), downloads with taggers only for
sources whose terms permit it. Each adapter: config screen, clear "blocked"
state when credentials/infrastructure are unavailable, no fake data.

## M7 — Phase 8 release

Windows CI build (SMTC verified), macOS notarization + dmg, MSI, DB migration
test matrix (user_version upgrades), resource-cleanup audit, accessibility
pass, README + docs final, signed release builds.

## Risks (tracked)

- **PoToken/BotGuard on desktop** — biggest risk; spike in M3 before any YTM
  UI work; fallback = addon/module sources only.
- **souvlaki macOS main-thread constraint** — handled (attach in setup).
- **rodio OutputStream not Send** — handled (stream owned by audio thread).
- **Windows SMTC / wry WebView2** — verify in M7; no code path assumes macOS.
