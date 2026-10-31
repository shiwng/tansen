# Tansen — Work Report & Roadmap

Where the project stands as of **2026-10-29**. Companion documents:
`IMPLEMENTATION.md` (how everything is built and connected),
`docs/feature-matrix.md` (per-feature status), `docs/plan.md` (milestones),
`docs/audit-bitchord.md` (upstream reference).

This file doubles as the roadmap: section 1 is how we got here, section 2 is
what is reached and verified, sections 3–5 are the road still ahead, and
section 6 is how any of it can be re-verified.

---

## 1. The road so far

| Session | Date | Commit | What landed |
|---|---|---|---|
| 1 | 2026-10-27 | `685b426` | **Foundation.** Audit of upstream BitChord (repo + APK v1.8.1 — same version, full report in `docs/audit-bitchord.md`). Phase-2+3 vertical slice: Tauri shell, navigation, SQLite (WAL + migrations), folder import → scan (lofty metadata, artwork cache), rodio playback engine, pure queue model, media keys. |
| 2 | 2026-10-28 | `0c93870` | **UI parity.** UI re-based on BitChord's own GPL **web player** (tokens, glass shell, TrackList, FullScreenPlayer, Up Next). Player store mirroring the upstream contract over Tauri. **Lyrics system vendored**: 13 providers, TTML/LRC/QRC parsers, LyricClock — 26 upstream tests pass against the vendored code. Rust: queue persistence (`playback_state`), playlist rename/delete, albums/artists/history views, queue Move. |
| 3 | 2026-10-28 | `50421d7` | **M2 closed.** Add-to-Playlist picker, drag reorder page, library watcher (notify + `watch_folders`), restart-persistence integration tests, artwork page tint. |
| 4 | 2026-10-28 | `7518803` | **M3 started.** `sources` crate — `MusicSource` trait with upstream tier semantics (Lossless/Capped/Best, Ok/Unreachable/Rejected health), addon HTTP adapter (SharedCalls dedup, 429 quiet period), SQLite registry, Settings → Sources screen; mock-server integration tests. |
| 5 | 2026-10-28 | `7f70fdc` | **YouTube Music via Kotlin sidecar.** Upstream checkout pinned at v1.8.1 + additive `sidecar/` module (nothing in `:shared` changed). `YtMusicSource` supervises the JVM. Round-trip proof: search → AAC stream → 5 s decode, RMS 0.126. |
| 6 | 2026-10-28 | `60085ba`, `51823a3` | **Named + relocated.** Renamed to **Tansen** (product, bundle id `app.tansen.desktop`, crate/npm names, data-dir migration `bitchord-desktop` → `tansen`, original three-bars app mark replacing BitChord's logo). Project moved to `~/Desktop/Tansen`, later `~/Downloads/Tansen` (current), with the upstream checkout as sibling. |
| 7 | 2026-10-28 | `e79d57d` | **Remote streaming playback.** `HttpRangeReader` (512 KiB ranged chunks — googlevideo throttles unbounded reads), engine queue entries dispatch by scheme, resolve-ahead thread swaps placeholder URLs before the gapless preload, YouTube Music tab in Search, play/queue on remote rows, catalogue toggle in Settings. Round-trip decodes through the ranged-HTTP engine path in **4.5 s** (was 142 s downloading). |
| 8 | 2026-10-28 | `facf82a` | **Playback robustness.** Engine pending-load machine: next/natural-end onto an unresolved placeholder holds the transition up to 10 s instead of skipping. `TrackRef.video_id` + restore re-placeholdering for expired stream URLs (integration-tested). Frontend queue rows carry real video ids. |
| 9 | 2026-10-29 | `d792bd6` | **Reproducible builds.** Sidecar module sources moved into the repo (`sidecar-overlay/`), `scripts/build-sidecar.sh` builds from pinned upstream v1.8.1 (fresh clone or clean checkout; mavenLocal pre-seeded via resumed curl), jar packaged as a Tauri bundle resource, packaged-app discovery verified in a real `Tansen.app` (log line from `Contents/Resources/resources/`), macOS CI workflow added. |
| 11 | 2026-10-31 | working tree (uncommitted) | **Music engine hardening from the Spotifast study.** `StreamCache` disk cache (512 MiB LRU) for streamed tracks + resolve-ahead prefetch of the next track; engine device-loss fallback (degraded start, RetryOutput, honest TrackError, no queue burn); tick cadence 100 ms audible / 500 ms idle; 24 h lyrics disk cache in the Rust proxy. See "Spotifast reference study" below. |
| 10 | 2026-10-29 | working tree (uncommitted) | **App icon.** Real macOS-style icon rendered from scratch (1024² squircle, indigo gradient, three-bars mark), full set generated via `pnpm tauri icon` (icns/ico/PNGs), `bundle.icon` references icns+ico, bundle + `Info.plist` verified. Uncommitted — pending review. |

**Test totals:** 25 Rust tests + 32 vitest tests, all green; `tsc` strict +
production bundle clean; app smoke-launched every session. `pnpm tauri build`
produces `Tansen.app` (30.7 MiB) + dmg (22.7 MiB) with the sidecar jar inside.

**Project location:** currently `~/Downloads/Tansen` with the pinned upstream
checkout at `~/Downloads/BitChord` (sibling — the sidecar builds against its
`:shared`). Moves cleanly; paths in docs use relative references.

---

## 2. Roadmap point reached — what works (verified)

| Area | Status | Evidence |
|---|---|---|
| Library import/scan/rescan | ✅ reached | End-to-end test on a real WAV; rescan adds 0 duplicates; missing files marked, not deleted; watcher integration test |
| Metadata + artwork | ✅ reached | Tags, codec/bitrate/sample-rate/bit-depth stored; content-hash artwork cache; `art://` protocol |
| Local playback | ✅ reached | Play/pause/seek/volume/next/prev/repeat; queue ops incl. drag-move; gapless pre-append advance; errors surface and auto-advance |
| Queue persistence | ✅ reached | Survives restart paused at saved position; dead local files dropped; remote entries kept and re-resolved (integration test) |
| Playlists | ✅ reached | Create/rename/delete/play/append; drag reorder page; Add-to-Playlist from any track menu |
| History + Recently Played | ✅ reached | One row per local track start |
| Albums/Artists views + page tint | ✅ reached | SQL groupings with album_artist fallback; pages tint from artwork |
| Search (library) | ✅ reached | Albums/artists shelves + songs |
| UI design parity | ✅ reached | Vendored upstream web-player design (glass sidebar, player pill/LCD, full-screen player, Up Next), tokens verbatim, DM Sans self-hosted |
| Lyrics | ✅ reached (data layer) | 13 providers raced in priority (word > line > plain), ISRC identify, TTML/LRC/QRC parsers, provider settings — 26 upstream tests pass against the vendored code |
| YouTube Music catalogue | ✅ reached, behind flag | Sidecar supervises upstream's Kotlin client; search → `prefer=aac` progressive stream → engine decodes through the ranged reader, RMS 0.126, 4.5 s end-to-end; off by default |
| Streaming playback | ✅ reached | 512 KiB ranged reader; MP4 moov seeks over the wire; remote rows in queue/Up Next; resolve-ahead keeps URLs fresh; transition race fixed |
| OS media keys (macOS) | ✅ reached | Now Playing metadata + 2 Hz position; play/pause/seek from keys |
| Settings | ✅ reached | Every visible control wired or explicitly labeled "not yet implemented" |
| Watch folders | ✅ reached | Auto-rescan, 2 s debounce, initial scan, toggle honored |
| Reproducible sidecar builds | ✅ reached | Pinned-tag build script from repo overlay; jar bundled + discovered inside packaged `.app` (verified) |
| App icon | ✅ reached | Original artwork; full format set; icns verified down to per-chunk artwork match; bundle + Info.plist agree |
| Data migration | ✅ reached | `bitchord-desktop` → `tansen` verified live |

---

## 3. Road ahead — planned, in dependency order

| Milestone | What it covers | Where it starts |
|---|---|---|
| **Phase 4 completion** — sources | QuickJS addon modules (`rquickjs`), racing source resolver (`bestAcross`, lossless upgrades, cross-source matching) once a second real source exists to race | `docs/plan.md` M3 |
| **Phase 5 completion** — lyrics UI polish | Word-level timings over the sidecar wire (upstream model has them), lyrics on all pages, per-track offset | M4 |
| **Phase 6** — advanced audio | Crossfade (two-sink equal-power) + Automix transition planning, 10-band EQ + loudness, playback speed, skip silence, sleep timer, audio cache; measured transitions | M5 |
| **Phase 7** — integrations | Downloads + taggers, Last.fm/ListenBrainz scrobbling, Discord RPC, Listen Together client; each individually gated and verified | M6 |
| **Phase 8** — release | Windows verification (real host/CI), signing + notarization, migration test matrix, accessibility pass, final docs | M7 |

## 4. Known gaps on the reached plateau

| Area | Status | Notes |
|---|---|---|
| In-playlist inline reorder | 🟨 | Dedicated reorder page done; drag inside the list itself pending |
| Word-level lyric timings over the sidecar | 🟨 | Upstream model has them; sidecar exports line-level only |
| Dynamic page tint on search/home | 🟨 | Album/playlist pages tinted; others pending |
| Artwork orphan GC | 🟨 | `prune_orphans` exists; not scheduled |
| First-play latency | 🟨 | Uncached remote start still resolves in 1–2 s; StreamCache + next-track prefetch (2026-10-31) removes repeat-play and queued-next latency |
| Library search is client-side | 🟨 | Fine at thousands of tracks; SQL FTS when libraries grow |
| SF Pro Display lyrics font | 🟨 | Apple-proprietary face shipped by upstream; flagged for distribution review |
| Single sidecar handle per process | 🟨 | One JVM; re-launched on next use after a crash; no watchdog/health UI |

## Spotifast reference study (2026-10-31)

Reference: `~/Downloads/spotifast-main` (Rust + egui, one crate, ~138 kLOC).
Floorplan: `spotifast-main/floorplan/` (subsystem-level map; per-file
descriptions intentionally skipped for a reference codebase).

**Delivery lifecycle traced (file → mechanism):**
- Search: `src/api/client.rs` — authenticated rate-limited Spotify Web API transport (`api.spotify.com/v1/search`), semaphore-capped concurrency, `Retry-After` honored. Metadata from `src/api/models.rs` (image picker picks smallest sufficient artwork).
- Delivery: `src/player.rs` — librespot session/Spirc owns the actual fetch+decode+buffer internally; Spotifast's job is config (bitrate, normalisation, gapless), event folding into `LocalState` snapshots, and engine replacement with resume (`interrupted()` / `resume_point()`, `src/player.rs:580-708`).
- Output: `src/sink.rs` — custom rodio sink over their `fastframe-audio` device layer: device opens lazily on first write, reopens after failure, follows the system default, paused = zero audio work. This is the pattern Tansen adopted (degraded start + RetryOutput).
- Caching: librespot `Cache` (audio_cache_dir + limit from settings) for Spotify audio; `src/images.rs` ArtLoader = disk-cached artwork with LRU scan; lyrics sha1-disk-cached in `src/lyrics.rs` (Spotify transcription → LRCLIB, exact match then duration-ranked).
- Local files: `src/local_tags.rs` — Symphonia reads cover + lyrics from ID3/FLAC/Vorbis; `.lrc` sidecars beside the file.
- Failure recovery: sink reopens the device; engine replacement carries `Heard` state; session drops resume from `interrupted()`.

**What Tansen adopted (own implementation):** bounded stream disk cache + next-track prefetch (fixes first-play latency and gives gapless-from-disk), degraded engine start + RetryOutput instead of crash-or-skip, 24 h lyrics response cache in the Rust proxy, dynamic tick cadence.

**What Tansen already did better:** strict crate layering (Spotifast is one 138 kLOC crate with 10–33 kLOC god-files `app.rs`/`backend.rs`), pure unit-tested queue model, SQLite with versioned migrations, process-isolated sidecar instead of an in-process SDK, 57 automated tests incl. a live network canary.

## M1 baseline metrics (2026-10-31)

`scripts/bench/run.sh`: release build with the frontend embedded
(`custom-protocol`), 5 runs, medians. macOS (darwin 27, arm64), machine-local —
these are baselines to diff future changes against, not absolutes.

| Metric (5 runs, release) | Median |
|---|---|
| Process start → first paint | 498 ms |
| Memory idle — app process | 49 MiB |
| Memory playing — app process | 49 MiB |
| CPU 60 s idle (paused) | 0.74 % |
| CPU 60 s playing | 2.03 % |
| Wakeups | skipped (powermetrics needs sudo) |

Raw: paints 459–517 ms; idle CPU 0.72–1.23 % (one warm-run outlier); playing
CPU 1.99–2.04 %; idle memory 31–49 MiB (one run captured before the web view
helpers settled); playing memory 46–49 MiB. Memory covers the app process
(the sidecar JVM starts lazily and was not up during these runs); WKWebView
helper processes are launchd-spawned and excluded — their footprint sits in
separate processes. The "playing" phase is a generated 75 s WAV driven by an
env-gated autoplay hook (`TANSEN_BENCH_PLAY`); normal runs are unaffected.

Two real release bugs the harness exposed, both fixed:
- CSP had no `connect-src ipc: http://ipc.localhost` — every `invoke()` failed
  silently in release builds (dev mode worked because Tauri relaxes CSP for
  the dev server).
- `cargo build --release` without the `custom-protocol` feature pointed the
  binary at devUrl — a "release" run was silently a dev-config run.

Reading: idle ≈ 0.7 % CPU matches the 500 ms idle tick cadence; playing adds
decode and lands ≈ 2 %; memory is flat idle→playing (the StreamCache is
empty on this path — the local fixture does not fill it; remote tracks fill
it up to the 512 MiB bound).

## M2 — UI↔backend sync inventory + command counters (2026-10-31)

Full inventory table in `IMPLEMENTATION.md` §11. What the pass found, measured:

- **No polling anywhere**: zero `setInterval`, zero `refetchInterval`; the UI
  is purely event-driven off `player://event` (10 Hz position) plus one-shot
  retries. The rAF scrubber reads the interpolated playhead without IPC.
- **`proxy_get` is the hottest command**: 21 calls in the counters' first
  10 s window (lyrics providers racing for the restored track). The 24 h
  disk cache absorbs repeat fetches.
- **Registered but caller-less commands** (reserved surface, honestly
  marked): `player_pause`, `player_resume`, `player_stop`, `sources_get`,
  `source_reorder`.
- Playback-state invariant re-verified by inspection: the engine is the sole
  writer of in-memory playback state; `playback_state` (DB) has exactly one
  writer (persist_state thread); media keys bypass IPC entirely.

Instrumentation: `metrics.rs` counts every command invocation in the
`invoke_handler` wrapper (std atomics, no new deps), debug builds only
(no-op in release), logged per 10 s window under the `metrics` log target.

Verification: `cargo build` clean (tansen crate zero warnings; 2 pre-existing
`sources` dead-code warnings noted, untouched); `cargo test --workspace`
27 passed / 0 failed / 1 ignored; `pnpm test` 32/32. Counter smoke run
(debug + custom-protocol, 15 s): startup window log captured (the excerpt
quoted in IMPLEMENTATION.md §11). rustfmt is not installed on this
toolchain (never used in this repo); code follows the file's existing style.

## 5. Blocked (with reasons — never simulated)

| Area | Reason |
|---|---|
| HLS / DASH streaming | rodio/Symphonia decode progressive files only; manifest support needs a demuxer change. Sidecar serves progressive itags. |
| Signed-in YouTube features (home, likes, playlists CRUD, history) | Sidecar is anonymous-only by design; cookie plumbing is a future decision. |
| PoToken/BotGuard guarantee | Today's client path resolves without a token (observed, re-tested); no WebView minter exists on desktop. If Google flips the switch, streaming breaks until a minter is built — the round-trip test is the canary. |
| YouTube-caption lyric providers | Need Innertube outside the sidecar; not wired. |
| Favorites / like status | No rating data model; button absent rather than fake. |
| Chromecast | No desktop Cast SDK equivalent; upstream feature is Android/GMS-bound. |
| Discord RPC, Last.fm/ListenBrainz, Listen Together, downloads | Designed (protocols documented in the audit); phase 7. |
| Windows build + CI job | **Untested** — no Windows host here; no CI job added for a platform that has never run the toolchain. Code paths are cross-platform (souvlaki/SMTC, `%APPDATA%`); needs a real pass + Java 17 runtime note for the sidecar. |
| macOS signing/notarization | `.app` + `.dmg` bundle verified; signing is phase 8. |

## Provider & API integrity (verified 2026-10-31)

| Provider | Use | Auth | Caching | Terms notes |
|---|---|---|---|---|
| YouTube Music / googlevideo | Search, stream resolution, progressive AAC | Anonymous (sidecar; no PoToken observed on this path) | Ranged-HTTP disk cache, personal use | Unofficial API via upstream's own client; no DRM bypassed — progressive itags only. ToS risk acknowledged; round-trip test is the canary. |
| LRCLIB (`lrclib.net`) | Lyrics (plain + LRC) | None, no key | 24 h disk cache — LRCLIB's docs explicitly permit caching | Open API, fair-use rate limits. |
| Musixmatch (`apic.musixmatch.com`) | Word/line lyrics | Self-provisioned token (upstream pattern) | 24 h disk cache, `token.get` excluded | Token-scraping is upstream behavior; unofficial. |
| KuGou | Line lyrics | None | 24 h disk cache | Unofficial mobile endpoints. |
| BetterLyrics / BiniLyrics / LrcRed / Unison / LyricsPlus / PaxSenix | Word/line lyrics | Some need a user-supplied PaxSenix key | 24 h disk cache (binimum/boidu/unison/lyricsplus hosts) | Third-party mirrors of Apple/QQ timings; availability not guaranteed. |
| Apple Music (`amp-api`, token scrape) | Catalogue search for lyrics matching | Scraped web JWT | **Never cached** (token freshness) | Unofficial; excluded from cache rules. |
| Genius | Plain-text lyrics | None | 24 h disk cache | HTML scraping; unofficial. |
| Addon servers | User-configured | Per-server | SharedCalls dedup only | The user's own servers; the app imposes no third-party terms. |

No DRM, paywall, or access control is bypassed anywhere: only progressive itags the upstream client itself exposes, and only providers the user enables.

## 6. How to re-verify (from a clean checkout)

```bash
# sidecar jar (requires JDK 17; clones upstream at the pinned tag itself)
./scripts/build-sidecar.sh

# Rust tests: queue algebra, range reader + ranged decode, scanner e2e,
# groupings, persistence round-trips, watcher, addon mock server, registry
cargo test --workspace

# live YouTube canary (network): search → prefer=aac → 5 s decode, RMS 0.126
cargo test -p sources --test yt_sidecar -- --ignored --nocapture

# frontend: lyric parsers, LyricClock, repository selection, reorderMove
pnpm --dir apps/desktop install && pnpm --dir apps/desktop test
pnpm --dir apps/desktop build

# M1 baseline metrics: 5-run release measurement, medians table
scripts/bench/run.sh

# packaged app: Tansen.app + dmg, sidecar jar inside Resources
pnpm --dir apps/desktop tauri build
```

CI (`.github/workflows/ci.yml`) runs the same steps on macOS on every push.
Last full run locally: 27 Rust (+1 ignored live canary) + 32 vitest green;
M1 bench 5× release run recorded (2026-10-31). Bundle inspection (icon icns
per-chunk match, sidecar jar sha256 match): 2026-10-29.
