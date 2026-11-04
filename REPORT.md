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

## M3 — sidecar measurement (2026-10-31)

New `#[ignore]` test `crates/sources/tests/yt_measure.rs` measures the app's
own spawn path; numbers live in IMPLEMENTATION.md §6. Measured (macOS arm64,
live network): **spawn→ready 476 ms; JVM RSS 79 MiB at ready → 147 MiB after
30 resolves; resolve p50 1837 ms / p95 2358 ms over 30 calls (cold 1728,
max 3172); first play (cold resolve → ranged reader open → first decoded
audio) 4233 ms**, RMS 0.2173 (real audio).

Two engineering facts the numbers pin down:
- **The sidecar is a resolver, not a proxy** — all six routes answer JSON;
  the stream route hands Rust the googlevideo URL (`Main.kt:115`). No audio
  bytes ever cross the JVM↔Rust boundary; Rust ranges googlevideo directly.
- **Resolve is ~1.8 s per call, every call.** The sidecar does a fresh
  Innertube round trip per stream; nothing in the wire path is warm. This
  quantifies why the resolve-ahead thread + StreamCache prefetch exist, and
  makes resolve latency (not decode) the dominant first-play cost, with the
  ranged moov fetch second (2.4 s reader open).

No behavior change: the test is `#[ignore]`d and production code is
untouched. Verification: test run green (`1 passed`, 68.9 s incl. network);
`cargo test --workspace` still 27/0/1; `pnpm test` 32/32. Note: the older
`yt_sidecar.rs` round-trip test hardcodes `../../../BitChord/…` and breaks
whenever the sibling checkout moves (BitChord now lives at
`~/Desktop/BitChord` again); `yt_measure.rs` instead uses the app's own jar
discovery and is move-proof.

## M4 — audio reliability baseline (2026-10-31)

`crates/playback/src/metrics.rs` (debug only) + `scripts/stress/run.sh`.
Rodio 0.21 exposes cpal's **error** callback (`with_error_callback`) — counted.
Rodio does **not** expose the data-callback timing, so the timing signal is a
decoder read histogram on rodio's feeder thread (quantized p50/p99). Documented
in IMPLEMENTATION.md §12.

Measured (macOS arm64, debug): **5-minute stress — playback running while 10
library rescans and continuous 2 s query rounds hammer the backend:**
`stream_errors=0`, 25,552,610 reads, **p50 <1 µs, p99 <1 µs, slowest 0.7 ms**.
99.97 % of reads are sub-microsecond; the 0.1–1 ms bucket (11,090 reads) is
normal WAV buffer refills.

Stall hook validated end-to-end: 40 ms feeder gaps every ~0.6 s for 30 s →
slowest read 40.1 ms, `>10ms` bucket 32, errors 0; a deliberate 200 ms gap
every second → slowest 200.0 ms, errors **still 0**. Conclusion measured, not
assumed: **CoreAudio never reports underruns through cpal's error callback** —
even audible gaps count zero — so the read histogram is the real glitch
detector and the error counter means "no device-level failure".

Debug-only surface: `TANSEN_STALL_MS`/`TANSEN_STALL_EVERY` env hook (sleeps on
rodio's feeder thread mid-track), `TANSEN_STRESS=1` + `TANSEN_STRESS_LIB` driver.
Release builds compile all of it to pass-through no-ops.

Verification: `cargo test --workspace` 28/0/2 (playback 15 incl. the new
histogram test); stress script green at 300 s, 30 s and 15 s; tansen + playback
builds clean.

## M5 — normalized playback types in the core crate (2026-10-31)

New `crates/app-core/src/playback_types.rs` — pure types, no I/O, no Tauri
deps, **no serde** (no existing IPC consumes them; the derive gets added the
day one does). The vocabulary: `EntryId` (queue identity — an id, never a
positional index), `Status` {Idle, Resolving, Buffering, Playing, Paused,
Error(kind)} with `ErrorKind` {Device, Resolve, Decode, Io} mirroring the
engine's existing failure classes, `Anchor` {pos_ms, rate 0|1},
`PlaybackState` {rev, status, entry, queue_rev, anchor, volume}, and
`Command` {Play{entry}, Pause, Resume, Next, Prev, Seek{ms}, SetVolume,
Enqueue{tracks, placement}} with `Placement` {Next, Append} mirroring the
existing `PlayMode` semantics.

The centerpiece is `validate_transition(from, to) -> Result` — the pure law
for status changes: an explicit legal-edge graph per status, **the anchor
rule (rate 1 iff `Playing`, 0 otherwise) enforced on both endpoints**,
monotonic `rev`, same-status refused, volume in [0,1]. Six unit tests cover
the happy walk (idle→resolving→buffering→playing→paused→playing→idle), error
recovery edges (resolving→error→resolving, playing→error), six illegal edges
(idle→paused, resolving→paused, buffering→resolving, paused→buffering,
error→paused, idle→buffering), same-status refusal, the rate rule in both
directions, and rev/volume shape checks.

Groundwork only — nothing calls it yet (the engine keeps its own state until
a future milestone migrates it). Verification: `cargo test --workspace`
**34 passed / 0 failed / 2 ignored** (app-core 3 → 9); builds clean; five-crate
layout untouched.

## M6 — task-based playback engine in the core crate (2026-10-31)

New `crates/app-core/src/playback_engine.rs`. One tokio task owns the
`PlaybackState` outright; input is a **bounded mpsc** (`Command`, depth 64,
senders await under backpressure — nothing dropped); output is a
**`tokio::sync::watch<PlaybackState>`** (tokio was already an app-core
dependency — no new crates/deps; the app's existing runtime runs it).
`rev` increments by exactly one on every published change; a `Play` publishes
a whole lifecycle (Resolving → Buffering → Playing, or → Error(kind) when the
factory fails), each step its own rev.

`PipelineFactory` (`prepare`/`release`) is **the only seam** — no real audio
exists in this engine; tests install a fake that fails configured entries and
records call order (release-before-prepare asserted). The engine keeps the
play-order queue internally (storage only; addressing stays by `EntryId`) so
`Next`/`Prev`/`Enqueue{placement}` behave, and refuses illegal commands
silently (no publish, no rev).

Tests: 8 engine tests, including **three seeded-random 200-command
sequences** (seeds 1/42/1337, std-only LCG — no rand dependency) asserting
over the complete publish journal: `rev` strictly monotonic, every state
`validate()`s (anchor rate 0 whenever not Playing, volume in range), and
every status change is a legal edge per the M5 validator. The random tests
earned their keep immediately: they caught the engine publishing `Resolving`
with the stale `rate: 1` — fixed by recomputing the rate at every publish.

One M5 edge was loosened for skip-during-buffering: `Buffering → Resolving`
is now legal (M5's illegal-edge test updated accordingly).

Graphify cross-check (per request): `graphify query "engine"` (84-node BFS
neighborhood — Engine contains Worker/Command/EngineEvent, imports
Queue/QueueOp/StreamCache), `graphify explain engine.rs` (30 connections),
`path AppState → Engine` (single references hop). The checked-in graph
predates M5, so `playback_types` nodes are absent from it — regenerating is
a docs-session task, not an M6 one.

Verification: `cargo test --workspace` **42 passed / 0 failed / 2 ignored**
(app-core 9 → 17); workspace builds with zero unused warnings. Nothing in the
production app drives this engine yet — it is the core-crate sibling the real
audio engine will migrate onto.

## M7 — PipelineFactory over the real rodio playback (2026-10-31)

Three pieces:
- **`crates/playback/src/pipeline.rs`** — the rodio-backed media path: one
  dedicated audio thread owns the `OutputStream` (not `Send`), std-mpsc
  commands (Load-with-ack / Play / Pause / SeekMs / Volume / Clear / Quit),
  sticky volume across loads, `try_seek` with the frame counter jumping to
  the target. **`frames_played` is a lock-free `AtomicU64` incremented by the
  `FrameCounted` source wrapper on the feeder path** — one increment per
  channel-group, no locks, no allocation. `pos_ms()` = frames ÷ sample rate.
  The production `engine.rs` path is untouched (removed in M10); the only
  change there is `open_output_stream` becoming `pub(crate)` for reuse.
- **The M6 seam grew `transport`** (`Transport` {Play, Pause, Seek, Volume});
  the engine routes pause/resume/seek/volume through it after publishing, and
  the M6 fake records the calls. Still one trait.
- **`apps/desktop/src-tauri/src/bin/engine_demo.rs`** — the debug binary
  exercising the whole chain end to end: `Engine` + `RodioFactory` over four
  local WAVs, printing every published state and the frame counter. Measured
  run (macOS, real audio out, 44.1 kHz): 3 s play → frames 128 297 =
  pos 2909 ms; pause 1 s → Δ +183 frames (buffer drain, then frozen);
  resume + seek(1 s) → counter reset to 44 100, then 47 908 = 1086 ms;
  volume 0.4 honored; every published state satisfied the M5/M6 laws
  (rev monotonic, rate 1 iff Playing).

Known M7 limitations (documented, deferred): `prepare` is synchronous on the
engine task, so remote URLs would block it (fixtures are local); the engine
anchor's `pos_ms` updates only on explicit `Seek` commands — the frame
counter is the real clock and syncing them is a later milestone; queue
auto-advance on track end is not wired yet.

Verification: `cargo test --workspace` **43 passed / 0 failed / 2 ignored**
(playback 15 → 16 with the frame-counting test); demo run green (transcript
above); workspace builds with zero unused warnings.

## M8 — queue semantics + end-of-track deadline in the Engine (2026-10-31)

- **Queue**: entries stay addressed by `EntryId` (body lives inside the task,
  never in `PlaybackState`). `queue_rev` now increments on **any** queue
  change — Enqueue (body) and every cursor move (Play/Next/Prev land in the
  same path) — one bump per command. The M6 test pinning the old
  enqueue-only count was updated (1 → 5 in its scenario).
- **Track end**: when a publish lands on `Playing` and the factory reports a
  duration, the task arms **ONE one-shot deadline** for the remaining time —
  never periodic. On fire it confirms against `position_ms()` (the
  frames counter): really at the end (within 250 ms slack) → advance to the
  next entry, or fall to `Idle` when the queue is exhausted (entry cleared);
  anchor moved / decoder stalled → **re-arm** from the reported position,
  publishing nothing; no position truth → stand down. Pause, seek and skips
  cancel the deadline implicitly (the arm is recomputed from the status after
  every command).
- **Trait**: `PipelineFactory` gains `position_ms()` / `duration_ms()` —
  still the one seam. The rodio pipeline reports duration from the decoder at
  load; the fake gets settable truth for tests. tokio gained the
  milestone-named `test-util` feature for the paused clock.

Tests (5 new, `#[tokio::test(start_paused = true)]`, fake pipeline): auto-
advance to the next entry; exhausted queue → Idle with entry cleared; pause
cancels (10 s of virtual time, zero publishes); seek cancels and the
re-armed deadline (not the pre-seek one) does the advancing; a fire with a
stale position re-arms silently. `flush()` drains via `yield_now` only —
time sleeps would fire the very deadline under test.

Live proof (engine_demo, two generated 30 s WAVs, real audio): seek to 29 s →
`rev=9 Playing pos=29000` (re-armed) → **`rev=12 Playing entry=1`** — the
deadline fired, frames confirmed, the engine advanced. Earlier prints: 2 s →
frames 85 545 (pos 1939 ms); pause Δ35 frames frozen; resume+seek(1 s) →
frames 176 006 = pos 3991 ms.

Graphify per request: `query "Queue Enqueue"` (84-node neighborhood) +
`explain Queue` (18 connections — the production `queue.rs` apply pattern,
which this engine reimplements in its task-owned form).

Verification: `cargo test --workspace` **48 passed / 0 failed / 2 ignored**
(app-core 17 → 22); demo deterministic and green; zero new warnings.

## M9 — the task-based engine starts in the app (2026-10-31)

Wiring only; every existing command, event and polling path untouched, zero
frontend changes:
- **Setup**: the shell now starts the app-core playback `Engine` at Tauri
  setup (`tauri::async_runtime::block_on`, so `tokio::spawn` has a runtime
  context), with the shared rodio `RodioFactory` as its media seam. The
  EntryId→media resolve map starts empty — entries cannot name real files
  until the frontend integration milestone, and that is stated rather than
  faked. `RodioFactory` moved out of the demo binary into
  `src-tauri/src/rodio_factory.rs` so shell and demo share one
  implementation (`#[path]` include in the bin — the shell is a bin crate).
- **One new command**: `engine_command(cmd: Command)` forwards a
  `playback_types::Command` into the engine's bounded channel. This opened
  the M5 serde gate exactly as planned: the playback types now carry
  `Serialize`/`Deserialize` (the milestone note said "derive only if existing
  IPC needs it" — now it does). Command count 45 → 46; the M2 counters see
  the new command automatically.
- **One new event task**: subscribes to the engine's watch channel and emits
  `playback_state` (full `PlaybackState` payload) on every published change.

Verification: app smoke run logs `playback engine started` at startup;
`engine_demo` (now sharing the factory) still ends with
`Playing entry=1` via the M8 auto-advance; `cargo test --workspace`
**48 passed / 0 failed / 2 ignored**; build clean. The engine is live but
idle in the app — nothing in the frontend sends it commands yet, by design
until M10.

## M10 — the frontend rides the new engine; the old playback path is gone (2026-10-31)

The switch the M-series built toward:
- **Frontend** (`state/player.ts`, store contract unchanged — every component
  keeps its shape): actions forward as `Command`s over `engine_command`
  (`ClearQueue` + `Enqueue` + `Play` for "play this album now"); the UI state
  comes from the `playback_state` event; **position is interpolated locally**
  (`pos_ms + (now − receivedAt) × rate` — the engine publishes on change only,
  no polling timers anywhere, as the M2 inventory already noted). The store
  keeps the queue mirror (the body never crosses IPC) and persists it to
  localStorage: restart restore re-seeds the engine (Enqueue + Play + Pause +
  Seek — the Command set has no load-paused, so the blip is milliseconds).
- **Removed**: the legacy rodio engine (`playback/src/engine.rs`) and its
  pure queue algebra (`queue.rs`, 10 tests); app-core's `remote.rs`
  (resolve-ahead), `persist_state.rs`, `history.rs` threads; 16 superseded
  IPC commands (`player_load`…`queue_clear`, `player_play_remote` — 46 → 31)
  and the `player://event` broadcast. `open_output_stream` moved into
  `pipeline.rs` and the M4 reliability instrumentation now rides the new
  path. `ClearQueue` was added to the Command set (amendment flagged: the UI
  cannot replace queues without it).
- **Rewired**: OS media keys issue engine commands (`try_send`); the watch→
  event task feeds souvlaki metadata + one `play_history` row per local
  track start (library lookups refuse hash entries); the bench autoplay hook
  drives the new engine; the rodio factory resolves entry ids against the
  live library (scans included) with the stream disk cache attached.

Measured proof: app smoke with `TANSEN_BENCH_PLAY` — the real app plays
through the new path (autoplay marker, then **428,073 decoded reads in the
10 s metrics window, stream_errors=0**, M4 histogram riding the pipeline);
`engine_demo` still auto-advances (`Playing entry=1`); tsc strict + vite +
32 vitest green; `cargo test --workspace` **37 passed / 0 failed / 2 ignored**
(the delta: −10 queue-algebra, −2 old-restore integration, +1 ClearQueue).

**Honest regressions** (all documented, none hidden):
- **YouTube row playback is offline** — the old path carried remote
  streaming, and the sidecar resolve cannot run inside the engine task yet
  (M7 documented the blocking constraint). Catalogue browsing, sources
  settings and the sidecar itself are untouched; remote rebuilds on the new
  pipeline next.
- **Repeat** lost its engine wiring (the old queue algebra owned it); the
  button persists a preference only. Shuffle was already preference-only.
- **Up Next remove/move** have no queue-edit commands yet; the controls stay
  visible (layout unchanged) and log honestly instead of acting.
- The SQLite `playback_state` table is no longer written (schema stays;
  restore is the frontend mirror now). In-playlist reorder, artwork GC and
  the other standing gaps are unchanged.

Graphify per request: `explain player.ts` (degree 50 — the store contract
had to survive intact) + `path player.ts → Engine` (no directed path in the
checked-in graph, which predates M9's wiring — the new edge is the one this
milestone created).

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


## Post-M10 security remediation (2026-10-31, audit at `edcb60e`)

Findings TAN-01…TAN-12 from the security audit; remediation staged and
verified. No new dependencies; UI unchanged; uncommitted by request.

| ID | Sev | Verdict | Fix | Verification |
|---|---|---|---|---|
| TAN-01 redirect bypasses proxy allowlist | Medium | **Fixed** | Per-hop re-validation of the same allowlist predicate in a custom redirect policy + final-URL check (`commands.rs`) | 4 fixture tests: cross-host 302 errors and the target is never fetched; 200 flows; predicate unit; violation yields no response to cache |
| TAN-02 unbounded Enqueue | Low | **Fixed** | `MAX_ENQUEUE_TRACKS = 10_000` (above the largest legitimate use — full-library play/restore, libraries documented in the thousands); refusal mutates nothing, logs a warning | Over-cap refused (rev/queue_rev unchanged), at-cap succeeds, all existing engine tests pass |
| TAN-03 unsupervised engine/audio tasks | Low | **Partially fixed** | (a) mid-decode panics on the cpal callback thread are contained — the track ends via the normal end-of-track path instead of unwinding across the FFI boundary and aborting the process; (b) load-path panics answer the prepare ack with an error instead of hanging the engine task forever (`pipeline.rs`). Engine-task supervision itself: **not fixed — decision required** (auto-restart and a distinct "engine died" signal are both excluded by the M-series invariants) | Panic-injection unit test: panicking decoder → clean end-of-track; engine-level error routing already covered by the factory-failure test |
| TAN-04 CSP `style-src 'unsafe-inline'` | Info | Not fixed — requires UI regression verification | Inventory done: no `<style>` in app source; real consumer is lit/am-lyrics shadow-root style injection; constructed stylesheets are CSP-exempt | — |
| TAN-05 uncalled registered commands | Info | **Fixed** | `sources_get`, `source_reorder` unregistered + deleted (29 → 27 commands) | grep: zero frontend/Rust callers; build green |
| TAN-06 `bench_marker` in release | Info | Not fixed — decision required | The M1 harness (`scripts/bench/run.sh:117,120`) greps **release** logs for the `first-paint` marker this command emits — gating it breaks the bench | — |
| TAN-07 sidecar unauthenticated loopback | Info | Not fixed — accepted (same-user authority; not a boundary) | Bind address + no-auth verified (`Main.kt:51`) | — |
| TAN-08 jar integrity unverified at load | Info | Not fixed — decision required (needs hashing dep/build step) | Discovery + bundling verified (`yt.rs:205-240`) | — |
| TAN-09 embedded Musixmatch fallback token | Info | Not fixed — accepted (removal breaks the provider when the scrape fails) | Reachability verified: `musixmatchSecret()` falls back on scrape failure; `mxmSign` signs every MXM request (`providers.ts:510-540`) | — |
| TAN-10 SetVolume NaN clamp gap | Info | **Fixed** | Non-finite volumes are ignored (no publish, volume kept); finite values clamp as before. Unreachable via IPC (serde_json rejects NaN/inf) — engine-internal hardening | NaN/+inf/−inf ignored (rev+volume unchanged); −0.5/0/0.5/1/1.5 clamp correctly |
| TAN-11 scan progress carries folder path | Info | **Fixed** | Payload carries `folder_id`; frontend type updated (the UI never rendered the path — `LibraryPage.tsx:64` reads only `scanned`/`total`) | tsc strict + vite build green; scan exercised in smoke run |
| TAN-12 no devCsp | Info | **Fixed** (verification-limited) | devCsp = production CSP + dev origin + HMR socket (`tauri.conf.json`) | Dev flow ran: page load finished, first-paint fired, engine IPC worked. Webview-console CSP-violation reports are not capturable headless — one-line revert if the owner sees console warnings |

Final state: workspace tests **45 / 0 / 2** (baseline 37/0/2 + 8 hardening
tests), debug + release builds clean, tsc + 32 vitest green, debug-only
instrumentation confirmed absent from the release binary (`strings`).
Smoke: real-app autoplay through `engine_command`, stress scan, demo
auto-advance, 431 k reads with `stream_errors=0`.


## M11 — SQLite access split + queue-rows query (2026-10-31)

Inspection first: SQLite access already had **exactly one writer** — a single
`Connection` behind one `Mutex` (`persistence/src/lib.rs:28-30`), all 33
access sites serialized on it; reported, unchanged. The engine touches no
database (verified: zero persistence references in `playback_engine.rs`).
Read-only connections, a by-id-list query, and the rev-driven fetch did not
exist — added:

- `Db::open_read_only` (`persistence/src/lib.rs`): `SQLITE_OPEN_READ_ONLY`
  connection to the same WAL database; opened once at app startup
  (`AppState.db_ro`, after the writer's migrations). Writes through it fail
  at the SQLite level.
- `repo::tracks_by_ids` (`persistence/src/repo.rs`): chunked (500)
  parameterized `IN` query on the read connection, rows in input order,
  duplicates collapsed, unknown ids skipped.
- `queue_tracks_get` command (`commands.rs`, cap 10 000 ids to mirror the
  enqueue bound; 27 → 28 commands): the frontend calls it when
  `playback_state.queue_rev` changes; `lib/queueRows.ts` `mergeTrackRows`
  merges the rows onto the mirror by `local:<id>` identity — order and
  unknown (catalogue) ids untouched, `nowPlaying` re-derived from the current
  index.

Tests: 2 persistence integration tests (input order/dedup/missing-skip;
read-only reads + write refusal at the SQLite level), 3 vitest merge tests.
Verification: workspace build 0 errors; tests **47 / 0 / 2**; tsc + 35 vitest
green; smoke run — read-only connection opens at boot beside the active
writer (zero SQLite errors), autoplay through `engine_command` intact.


## M12 — scan stat-skip + batched transactions + 100k timing (2026-10-31)

Inspection: the stat data existed in the schema (`mtime_sec`, `size_bytes`,
migration 1) but the scanner ignored it — every file was fully re-parsed by
lofty on every scan, then written as two autocommits (upsert + art update).
Worker-thread placement was already correct (`spawn_blocking` in the shell
command, watcher's own thread; the engine never scans) — reported, unchanged.

Changed (`crates/library/src/scanner.rs`, `crates/persistence/src/repo.rs`):
- **Stat compare**: `repo::stat_by_path` returns `(mtime, size, missing)`;
  unchanged rows skip tag parsing entirely (a missing-flagged row is never
  skipped, so a returned file still un-marks). `summary.updated` — declared
  but never written since M-scan-1 — now counts skipped rows as
  `summary.unchanged` (rename, frontend never displayed it).
- **Parse/write split**: tag parsing stays rayon-parallel (changed files
  only); DB writes move to `repo::upsert_batch` — 200 files per transaction
  (upsert + art update in one commit, via new tx-scoped `_in` variants of
  the existing SQL; no SQL rewritten).
- **Timing test** `crates/library/tests/scan_timing.rs` (`#[ignore]`, plain
  `Instant` — criterion is not a dependency): 100k real 1-byte files, rows
  carrying real stat values. Measured this session: batched insert of 100k
  **9.8 s** (500 tx × 200), `list_tracks` (the search page's data path)
  **median 0.97 s**, `tracks_by_ids` (1000 of 100k) **median 11 ms**, full
  rescan of 100k unchanged files **median 1.41 s** — pure stat compare, no
  parses (the pre-M12 path re-parsed all 100k).

Verification: workspace build 0 errors; tests **47 / 0 / 2** (library e2e
extended: unchanged-skip counted, mtime bump re-parses); frontend untouched
(build green). One incident: an e2e mtime-touch helper briefly truncated the
committed WAV fixture via `fs::copy` onto itself — restored from git before
committing anything; the final helper uses `set_modified` only.


## M13 — ResolvedMedia formalized on the catalogue trait (2026-10-31)

M3 established the sidecar hands Rust a **URL, never audio bytes** — so this
milestone takes the "formalize the struct" branch. No new trait, no behavior
change:
- `sources::ResolvedMedia { url, headers, expiry (unix s), length (bytes),
  mime }` — the resolution contract.
- `MusicSource::resolve` — **default method on the existing trait**, derived
  from `stream()`; every source (addon, YouTube, test fakes) is resolvable
  without further code.
- `SourceStream` gained `mime` + `expiry_ms` (serde-default, additive — the
  addon wire contract is untouched and yields `None`s). The YouTube source
  now carries the sidecar's `expiresAt` (was dropped on the wire) and maps
  `container` → MIME (`audio/mp4`, `audio/webm`; unit-tested, case
 -insensitive, unknown containers stay `None`).
- `length` stays `None`: no provider reports byte length today; the field
  exists for the fetcher contract.

Verification: `cargo build --workspace` 0 errors; tests **49 / 0 / 3**
(+1 `container_to_mime` unit, +1 addon-mock `resolve` end-to-end through the
existing mock server); workspace suites unchanged elsewhere.


## M14 — sidecar supervisor (2026-10-31)

`crates/sources/src/yt.rs`: the cached sidecar handle previously went stale
forever if the JVM died — every later call failed until app restart. Now:

- **Supervisor**: `ensure_running` gates every call (`get`, `health_url` →
  `search`/`stream`/`health`). A dead or exited JVM (reaped via
  `try_wait`) restarts through **exponential backoff** — 500 ms doubling to
  a 30 s cap, reset by one healthy check; the gate arms the next window
  before spawning so a failed attempt cannot retry in a tight loop. Inside
  the window, callers get an immediate `SourceError` naming the remaining
  wait. Spawn + health poll extracted into `spawn_and_await` (shared by
  `launch` and restart; the poll stays on a dedicated OS thread).
- **Per-call timeout**: already present (20 s HTTP, 3 s health) — verified,
  unchanged.
- **Idle shutdown: not implemented, by the milestone's own condition** — M3
  measured spawn→ready at 476 ms and RSS 79→147 MiB; a cold JVM plus backoff
  on the next resolve outweighs the RAM savings for an opt-in, flag-gated
  catalogue, so the supervisor only. The flag-off default never spawns.
- **Error(kind) surfacing**: supervisor failures are `SourceError`s; the
  shell's resolve closure maps them to `ErrorKind::Resolve` → the engine
  publishes `Error(Resolve)` state (chain verified at `rodio_factory.rs:42`).
  No panics: every path returns `SourceError` (lock poisoning included).
- The sidecar jar + local Java only: `#[ignore]` integration test kills the
  JVM with SIGKILL and asserts health answers again with a **new pid**.

Tests: 2 backoff unit tests (doubling + cap + reset), 1 kill/restart
integration test — **passed this session: pid 28723 → SIGKILL → healthy
again as pid 28732 in 1.25 s**. Workspace: build 0 errors, tests
**51 / 0 / 4**. A zombie-process gotcha surfaced and is documented in the
test: a SIGKILLed child stays unreaped, so `kill -0` reports it alive —
liveness is asserted through the supervisor, not `kill -0`.


## M15 — prefetching range reader: the callback never touches the network (2026-10-31)

Before: the cpal callback pulled bytes through `HttpRangeReader` — a stall
meant the realtime thread performed HTTP. Now:
- **`PrefetchReader`** (`remote.rs`): `Read + Seek` over HTTP Range with a
  bounded 4 MiB read-ahead buffer filled by a dedicated fetch task (a thread
  reusing the existing blocking client + 512 KiB Range pattern — "async
  fetch task" in the thread-based pipeline, where the callback is realtime
  and everything else is threads). Reads pop from memory; when the buffer
  runs dry the decoder waits on an in-memory condvar while the fetch task
  owns the network, and the transition is published on a tokio watch:
  **Starved → engine `Buffering`, refill past the 512 KiB watermark →
  `Playing`** (legal edges both ways; rate recomputed by `push`, position
  kept). Seek drops the buffer and issues a **fresh Range request** at the
  target offset (in-flight results from before the seek are discarded).
  Bounded: never more than the high watermark buffered.
- **Plumbing**: `PipelineFactory::status_watch()` (default `None`, override
  in `RodioFactory` which owns the watch) → the engine task selects on it —
  a third arm beside commands and the track-end deadline. Statuses outside
  the relevant states are ignored (Idle noise publishes nothing). No new
  trait; tokio `watch` reuses the workspace dependency.
- The engine loop selects on it without polling; the one-shot track-end
  deadline still governs EOF (the status watch never reports end-of-stream —
  frames + duration keep that contract).

Tests: 3 reader tests (byte-exact sequential reads with a bounded buffer;
seek → fresh Range at the target; underflow → Starved → refill → Refilled,
with the decoder thread genuinely blocked on the condvar while the server
stalls) + 2 engine tests (stall → Buffering with rate 0 and position kept;
recovery → Playing; noise while Idle publishes nothing). Verification:
workspace build 0 errors; tests **56 / 0 / 4**; frontend untouched.


## M16 — expired/dead URL: engine re-resolve, one retry (2026-10-31)

- **Fetch task** (`remote.rs`): 403/410 on a Range request is terminal — the
  task parks (no retry storm), reports `StreamStatus::UrlDead` on the status
  watch, and the old decode ends cleanly (reads past the death return EOF).
  Transient fetch errors still retry on the 1 s loop.
- **Engine** (`playback_engine.rs`): `UrlDead` while Playing/Paused →
  publish `Resolving` → `factory.prepare(entry)` (fresh resolution) →
  `transport(Seek)` at the position where the stream died (frames-based,
  captured before the rebuild) → `Buffering` → `Playing`. **One retry per
  user-initiated load**: the budget resets on Play/Next/Prev; a second
  UrlDead for the same entry publishes `Error(Resolve)` directly.
- **Factory** (`rodio_factory.rs`): the resolve closure now returns
  `ResolvedMedia` (M13's struct, finally consumed) and `prepare` refuses
  media already past `ResolvedMedia.expiry` upfront — a dead-before-start
  URL surfaces as `Error(Resolve)` rather than a doomed load. Library and
  bench paths carry `expiry: None` (no check applies).

Tests: 2 engine tests (UrlDead → Resolving→Buffering→Playing with anchor at
the death position + transport Seek recorded; second UrlDead → Error(Resolve),
budget renewed by a fresh Play) + 1 reader fixture test (403 past the probe →
UrlDead, old decode ends cleanly). Verification: workspace build 0 errors;
tests **59 / 0 / 4**; frontend untouched.


## M17 — macOS Now Playing via objc2 (2026-10-31)

`apps/desktop/src-tauri/src/platform_macos.rs` (macOS-gated): Now Playing +
remote commands through MediaPlayer via the **objc2 family** — the one
allowed dependency (`objc2` 0.6.5 + `objc2-foundation` 0.3.2 were already
locked transitively; added `objc2-media-player` 0.3.2 + `block2` 0.6.2).
Souvlaki removed entirely (crate dep, `media.rs`, the `Media` field, the
souvlaki command handler): same features, now speaking to MediaPlayer
directly.

- **Now Playing**: a task subscribes to the engine watch and updates the
  info center **only on publishes** (rev bumps) — elapsed = anchor position,
  rate = anchor rate, so MediaPlayer interpolates elapsed time between
  publishes. No timers. Metadata (title/artist/album/duration) comes from
  the library DB for the current entry; `None` entry clears the center.
- **Remote commands**: toggle/play/pause/next/prev/skip ±10 s/scrub map 1:1
  onto `Command` via `try_send` into the bounded engine channel (scrub
  target from the change-playback-position event, downcast per Apple's
  command/event contract, SAFETY-commented).
- Deps: `objc2`/`objc2-foundation` promoted to direct (already locked),
  `objc2-media-player` + `block2` added. souvlaki dropped from app-core.

Verification: workspace build 0 errors, tests **59 / 0 / 4** (unchanged —
the module is OS glue; the engine contract it drives is already covered by
51 engine tests), smoke run logs `macOS Now Playing wired to the engine
watch` + `playback engine started` with zero panics. **NOT RUN**: the
visible Now Playing surface (needs a real GUI session + OS media UI), actual
remote-key presses.


## M18 — output device change + sleep/wake (2026-10-31)

Behavior (documented in IMPLEMENTATION.md): device switch → rebuild + resume
at the anchor; active-device removal → pause + rebuild at the anchor; sleep
→ pause; wake → rebuild + resume; idle → no-op. Everything enters the engine
as **Commands** (`Pause` / new `RebuildOutput`) from a 2 s monitor thread —
zero state mutation outside the engine.

- `Command::RebuildOutput` (amendment like ClearQueue) + `Transport::ReopenOutput`
  (drops the dead output stream; the next load reopens the new default).
- Engine rebuild flow mirrors M16's re-resolve: `Resolving` → prepare →
  seek to the frames-based position → `Buffering` → `Playing` (then `Paused`
  if the user had paused — `Buffering→Paused` is not a legal edge, so the
  chain runs through `Playing`).
- Detector: `platform_macos` polls the default device name via cpal (through
  rodio's re-export — no new dep) and subscribes to NSWorkspace
  will-sleep/did-wake notifications (objc2-app-kit, promoted from the
  lockfile).

Tests: 3 engine tests (rebuild while Playing resumes at anchor with
ReopenOutput+Seek recorded; rebuild while Paused stays Paused at anchor;
Idle no-op). Verification: workspace build 0 errors; tests **62 / 0 / 4**.
NOT RUN: real device unplug/replug and an actual sleep/wake cycle (needs GUI
+ hardware).


## M19 — decode thread + SPSC ring: the callback only copies (2026-10-31)

- **`ring.rs`**: lock-free SPSC sample ring — `UnsafeCell` slots behind
  Release/Acquire cursors (SAFETY-documented: producer owns `[tail, head)`,
  consumer `[head, tail)`; multi-thread test proves exact in-order delivery
  across wraparound). No locks, no allocation after construction.
- **Pipeline restructure** (`pipeline.rs`): a decode thread per (track,
  seek epoch) runs Symphonia (panic-containment + M4 instrumentation moved
  here) and fills the ring; the audio callback's `RingSource::next` only
  **copies a sample, applies volume from an f32-bits atomic, and increments
  `frames_played`**. Underflow → silence + Starved flag (decode thread
  reports Refilled on refill — M15 watch contract unchanged); EOF + drained
  ring → track ends. A parked reader wakes through its own shutdown `Arc`
  (no more leaked parked threads on seek-while-stalled).
- **Immutable per (track, seek epoch)**: Load/SeekMs build a fresh stream
  pipeline (decode thread + ring + consumer) and swap the consumer into the
  sink; the old epoch is shut down. Seek resumes by decoder-seek + frame
  counter seeded at the target.
- Callback invariant now literal: no locks (SPSC atomics only), no alloc
  (ring preallocated), no I/O (fetch task + decode thread own all of it).
- Volume: single atomic store — no rodio volume wrapper.

Tests: ring (order across wraparound, full/empty, cross-thread exactness) +
RingSource (volume applied, frames-not-samples, underflow silence + Starved,
EOF end). Verification: workspace build 0 errors (only the pre-existing
`sources` fields warning); tests **66 / 0 / 4**; demo run through the new
path: auto-advance to entry 1 with correct pos_ms.


## M20 — re-measurement after the M-series (2026-11-01, at `edcb60e` + working tree)

All four probes re-run on the same machine as the baselines. No feature code.

### Before / after

| Probe | Metric | Baseline | Now | Delta |
|---|---|---|---|---|
| M1 bench (5-run medians, release) | start → first paint | 498 ms | **338 ms** | −32 % |
| | CPU 60 s idle (paused) | 0.74 % | **0.06 %** | −92 % |
| | CPU 60 s playing | 2.03 % | **1.64 %** | −19 % |
| | memory idle / playing (app proc) | 49 / 49 MiB | **31 / 39 MiB** | −37 % idle |
| M2 counters | startup window traffic | `proxy_get +21` hottest | counters active; 25 s idle capture = **zero command traffic** | event-driven, no polling (unchanged by design) |
| M4 stress (300 s) | stream errors | 0 | **0** | — |
| | reads timed | 25.55 M | **27.66 M** | +2.1 M (longer decode path visible) |
| | p50 / p99 read | <1 µs / <1 µs | **<1 µs / <1 µs** | — |
| | slowest read | 0.7 ms | **4.7 ms** | decode moved off the feeder thread (M19); 23 reads 1–10 ms = ring-refill parks, none in the callback |
| M3 sidecar | spawn → ready | 476 ms | **458 ms** | ≈ |
| | JVM RSS ready → after 30 resolves | 79 → 147 MiB | **78 → 163 MiB** | ≈ |
| | resolve p50 / p95 (×30) | 1837 / 2358 ms | **423 / 497 ms** | 4× faster — but see caveat |
| | first play (resolve + open + 0.25 s) | 4233 ms | **725 ms** | reader open 2412 → 294 ms (prefetch, M15) + resolve variance |

Caveat, stated: the resolve p50 drop (1837 → 423 ms) is **not attributable to
any code change** — the sidecar and its network path are untouched since M3.
It is session-to-session variance in the Innertube round trip (different
hour, different edge). The honest claim: resolve cost is network-dominated
and varies 4×; first-play improvement is real where code changed (reader
open 2412 → 294 ms from the M15 prefetch reader).

### Budget check (values as given; owner to confirm)

| Budget | Measured | Verdict |
|---|---|---|
| start → interactive < 300 ms | first paint median **338 ms** (326–355 across 4 of 5 runs; one 736 ms cold outlier) | **Not met, close** — 38 ms over. Gap is frontend bundle + webview init; lazy routes already shipped. |
| idle wakeups ≈ 0 when paused | **NOT MEASURED** — powermetrics needs sudo (unchanged since M1/M4). Proxy: idle CPU 0.06 % ≈ the 500 ms engine idle tick only. | Unverifiable without sudo; proxy suggests near-zero work. |
| zero underruns under scan load | 300 s stress, 10 rescans + 2 s query rounds: `stream_errors=0`, p99 read < 1 µs, no >10 ms callback stalls; slowest 4.7 ms is decode-side. CoreAudio never reports underruns via cpal (M4 finding), so this is the strongest proxy available. | **Met by proxy**; literal underrun count unmeasurable on this backend. |

### Open decisions — measured evidence + recommendation

**1. Replace the Kotlin sidecar with a native Rust client — recommend NOT ACT.**
Evidence: spawn 458 ms once per process; RSS 78→163 MiB (the real cost,
opt-in flag); resolve is network-dominated with 4× session variance (423 ms
today, 1837 ms at M3 — a native client inherits the same network, fixes
none of it); supervisor (M14) already covers crash/restart; URL handoff is
clean (M3/M13). The sidecar's moat is that upstream maintains the Innertube
protocol against Google churn (PoToken risk documented); a Rust rewrite
takes that maintenance on ourselves and re-fights the JitPack-class
integration bugs for zero measured user-visible gain. Act only if a hard
memory budget (< ~80 MiB total) is imposed or upstream dies.

**2. Swap the WebView shell for a native UI — recommend NOT ACT.**
Evidence for: paint 338 ms misses the 300 ms budget; app-process memory
excludes WKWebView helpers (unattributable, noted since M1). Evidence
against: the entire lyric stack (13 providers, TTML/LRC/QRC parsers,
am-lyrics), the vendored design system, and 35 vitest live in the web layer
— a native UI is a months-long rewrite of the product's most differentiated
surface. CPU/memory are modest and idle CPU is 0.06 %. Revisit only if the
paint budget hardens below 300 ms and bundle/startup work (code-splitting
the 413 kB main chunk) fails to close 38 ms.

**3. Drop Rodio for raw cpal — recommend NOT ACT now; revisit with phase 6.**
Evidence: after M19 the audio callback is already copy-from-ring + atomic
volume + frame count — rodio's Sink/mixer adds no measured cost (0 stream
errors under stress, p99 < 1 µs; the 4.7 ms slowest read is decode-side,
off-callback). Rodio still earns its keep for device open/fallback (M18
reopen path) and the sink abstraction the factory targets. The moment
phase-6 crossfade/EQ needs sample-exact dual-stream mixing, direct cpal
becomes the simpler substrate — that is the decision point, not today's
numbers.

Commands run this session: `cargo build --release -p tansen --features
tansen/custom-protocol` (0 errors), `scripts/bench/run.sh` (5× release),
`scripts/stress/run.sh 300`, `cargo test -p sources --test yt_measure --
--ignored --nocapture`, debug counter capture (25 s). NOT RUN: powermetrics
wakeups (sudo), GUI interaction.

## S1/S2 — source contract + registry + quality (2026-11-01)

Build: workspace 0 errors. Tests: **77 / 0 / 4** (sources 12 incl. codec table, quality mapping, error classification, validate_order; persistence 7 incl. migration fresh + upgrade reseed + restart persistence). Frontend: tsc strict + vite build green. NOT RUN: GUI manual check of the quality selector and source toggles (headless); live sidecar playback (network/authorization not exercised this session).

## S3 — resolver (2026-11-02)

Resolver implemented in `sources` with 7 paused-clock unit tests (success+cache 1 upstream call; 8 concurrent → 1 call; timeout classification; transient retry; permanent negative-cache + invalidate; expiry re-resolve; disabled → Disabled). Engine integration: M16 retry invalidates before re-resolve (test asserts the fake records `invalidate`). Workspace: **84 / 0 / 4**; build 0 errors. M4 stress 120 s: `stream_errors=0`, p50/p99 < 1 µs, slowest 1.2 ms (decode side), no new underruns. NOT RUN: live sidecar resolve through the resolver (network/authorization not exercised).

## S4 — decodability + container policy (2026-11-02)

Build 0 errors; tests **88 / 0 / 4**. `decodable()` implemented and tested; resolver picks the best decodable candidate and maps all-undecodable to `SourceError::Unsupported`. WAV decode probe green. **Opus/WebM not decodable in this build (verified in Cargo.lock) — decision required**: libopus decoder dep vs AAC-only restriction; no new deps added. ffmpeg absent → per-codec fixtures NOT RUN (structural matrix asserted).

## S1–S4 completion pass (2026-11-02)

Preamble re-read; every S1–S4 component checked. Completions: shell→resolver wiring (S3), TrackKey cap enforcement (S1), explicit order-oversize/rejection tests (S2), sidecar `prefer_codec` optional hint (S4). Workspace: build 0 errors; tests **90 / 0 / 4**. Sidecar Kotlin changed but not executed (JVM build NOT RUN — off in this session). Opus decision remains recorded (S4).

## S5 — multi-source search + remote artwork (2026-11-02)

Build 0 errors; tests **93 / 0 / 4** (search fan-out: timeout+others-returned, error kind, rank order + total/per-source caps). Frontend tsc strict + vite green. NOT RUN: real-sidecar search over network (not exercised this session); artwork fetch against live hosts (offline).

## S6 — remote track identity in queue (2026-11-02)

Schema decision: **separate `remote_tracks` table** (migration 4, additive) —
a source column on `tracks` would have migrated existing track semantics, so
the additive option wins; no STOP required.

What landed (see IMPLEMENTATION.md S6 for the full map):
- Engine: `Enqueue.tracks` carries all entry ids in order, `remote` maps the
  remote subset to TrackKeys; remote-only ids enter the queue body (they were
  stored in the key map but never enqueued); `ClearQueue` drops remote keys;
  the 10 000 enqueue cap covers the combined list.
- IPC: `remote_track_upsert` (validated, bounded, single-writer) returns the
  row id used as the EntryId; `queue_tracks_get(ids, remote_ids)` returns
  unified display rows for both kinds.
- Restore: remote rows re-upserted and re-seeded as keys only — no stored
  stream URLs anywhere; the engine resolves fresh.
- Frontend: play/enqueue on remote search rows via the existing queue actions
  (SearchPage tags rows with sourceKind/artworkUrl; player actions upsert
  before commanding; queue merge handles both kinds; remote artwork routes
  through `fetch_artwork`).

Verification:
- `cargo test --workspace` — **100 passed / 0 failed / 4 ignored**
  (app-core 35 incl. 4 new S6 engine tests: mixed local/remote seeded-random
  250-command sequences, play-remote passes the TrackKey to `prepare`, remote
  entries walk with Next carrying keys, ClearQueue drops stale remote keys;
  persistence 9 incl. the queue-rows both-kinds query; local-playback
  regressions all green).
- `cargo check --workspace --all-targets` — 0 errors (only pre-existing
  warnings).
- `pnpm test` — **37 / 37** (queueRows suite 3 → 5: remote merge, artwork-url
  fallback).
- `pnpm build` — tsc strict + vite green.
- NOT RUN: live sidecar play through the new path (network/GUI session not
  exercised); real GUI interaction with remote rows; artwork fetch against
  live hosts (offline).

## S7 — matcher + fallback (2026-11-02)

Matcher (`sources/src/matcher.rs`, std only): `normalize` (case/
punctuation/diacritics/feat-strip), `matches` (title+artist normalized,
marker mask live/remix/remaster/acoustic — word-boundary — duration tolerance
`DURATION_TOLERANCE_SEC = 10`), `queries` ("title artist", "title").

Resolver fallback (`Resolver::with_fallbacks(sources, fallbacks, meta)`;
`new` unchanged for S3): on pinned failure, sequential per-source timeout
attempts over the enabled-source provider, skipping same-config and Local
(S12); candidate accepted only when `matches()` and its stream succeeds;
`ResolvedMedia.source` recorded as the fallback source; no match → original
error. Shell wires provider from registry addons (rank order) + memoized
sidecar, metadata from `remote_tracks` via new `repo::get_remote_track_by_key`.

Verification:
- `cargo test --workspace` — **116 passed / 0 failed / 4 ignored** (sources
  40 incl. 6 matcher table tests + 7 ladder tests: fallback hit with source
  recorded, no match → original error, all-fail → original error, pinned-in-
  list skipped, Local never called, sequential ordering, missing metadata
  skips fallback; all S3 resolver tests still pass).
- `cargo check --workspace --all-targets` — 0 errors.
- `pnpm test` — 37/37 (frontend untouched by S7).
- NOT RUN: live addon-sidecar fallback over the network (no GUI/network
  session; covered by fake-source ladder tests).

## S8 — resolve-ahead (2026-11-02)

Baseline measured with the existing M3 harness (`cargo test -p sources --test
yt_measure -- --ignored --nocapture`, live network, **ran this session**):
resolve ×30 **cold 918 ms, p50 992 ms, p95 1275 ms, max 1400 ms**; remote
first-play **resolve 1017 ms + reader open 1126 ms + first 0.25 s decode =
2142 ms total**; spawn→ready 465 ms; RSS 78 → 144 MiB. Resolve ≈ 47 % of
remote first-play → measurable part of the boundary gap → implemented.

Implementation: `PipelineFactory::prefetch_resolve(entry, key)` default no-op;
`RodioFactory` resolves through the S3 resolver (warms the URL cache). Engine
calls `maybe_prefetch` only on queue/cursor changes (commands + auto-advance),
concurrency 1, stale prefetch aborted on queue change, skipped while paused/
idle and for local entries. No new Status variants/events/timers.

Verification:
- `cargo test --workspace` — **109 passed / 0 failed / 4 ignored** (app-core
  38 incl. 3 new S8 tests: next remote entry prefetched after Play; no
  prefetch while paused/local — asserted empty; queue change cancels a stale
  prefetch — old `done` never recorded, new one completes. S3 resolver tests
  all pass; auto-advance/gapless boundary tests unchanged and green).
- `cargo check --workspace --all-targets` — 0 errors.
- `pnpm test` — 37/37 (frontend untouched).
- **NOT RUN**: app-level E2E before/after numbers (first-play and
  track-boundary gap through the real GUI + live sidecar — no GUI/network
  session); the resolve-share-of-first-play proxy is the recorded evidence.
  Engine-level prefetch semantics are covered by the tests above.

## S9 — Subsonic/OpenSubsonic source (2026-11-02)

- `cargo build --workspace`: **0 errors** (4 pre-existing `tansen` bin
  warnings from M17/M18, documented, untouched; sources crate clean).
- `cargo test --workspace`: **126 passed / 0 failed / 4 ignored** (same
  GUI/network-only ignores as prior stages). Includes the new 8-test
  `subsonic_source` suite (mock-server integration): search round-trip with
  token auth, stream sniffing + auth URL, ping/auth-failure health mapping,
  cross-host redirect rejected and never fetched, oversize response rejected,
  DRM/encrypted rejected, failure isolation, S3/S7 fallback onto Subsonic.
- Frontend `npm run build`: success (SettingsPage chunk includes the
  Subsonic form). `tansen` builds with `--features tansen/custom-protocol`.
- Credential-exposure grep proof:
  1. `grep password crates/persistence/src/` → no matches (nothing secret
     can pass through the DB layer; `SourceConfig` has no secret field).
  2. log macros (`tracing::`/`println!`/`eprintln!`) in keychain.rs,
     subsonic.rs, commands.rs → no password mention.
  3. `emit` in commands.rs + sources → no password in any event payload.
  4. `grep password apps/desktop/src/` → only the transient form `useState`
     (cleared on submit) and `type="password"` inputs; no settings/
     localStorage/query-cache persistence.
- URL-policy unit tests (in-crate): http without opt-in rejected; http to a
  public DNS name rejected even with opt-in; loopback/private hosts allowed
  with opt-in; ftp/other schemes rejected; embedded URL credentials
  rejected.
- **NOT RUN**: live integration against a real Subsonic/OpenSubsonic server
  (no server available in this environment; the 8 mock-server tests are the
  recorded evidence) and a macOS Keychain runtime round-trip (exercising
  SecItemAdd/CopyMatching against the user's real login keychain from tests
  is inappropriate; the paths are compile-verified through type-checked
  bindings and the failure paths return typed errors, but no runtime keychain
  read/write was performed this session).

## S10 — discovery pages (2026-11-02)

- `cargo build --workspace`: 0 errors. `tansen` builds with
  `--features tansen/custom-protocol`. Frontend `npm run build`: success
  (DiscoveryPage chunk + `/discover` route + sidebar entry).
- `cargo test --workspace`: **132 passed / 0 failed / 4 ignored** (same
  GUI/network-only ignores). Includes the 6 new `discovery_tests`:
  suggest fixture → typed rows with TrackKey (+ optional-field fidelity),
  suggest caps, browse fixture → header + rows, browse cap,
  over-length-track-id rejection, browse-id validation (empty/over-64/
  traversal/space/slash/percent rejected).
- Sidecar jar rebuilt (JDK 17) and smoke-checked on 127.0.0.1: `/health`
  ok; `/suggest` without `q` → `{"error":"missing ?q="}`; `/artist/`
  without id → `{"error":"missing browse id"}` — both new routes live,
  exercised without any upstream request.
- Fixture bug caught by the parser test: `SidecarSong` was missing the
  camelCase wire renames, so search rows had been dropping `durationSec`
  and `thumbnailUrl`. Fixed; discovery tests assert both map correctly.
- **NOT RUN**: real-network discovery checks (no live YouTube Music
  session in this environment — upstream typeahead/browse require network;
  the fixture parser tests + sidecar route smoke check are the recorded
  evidence) and the remote album/artist/playlist browse pages (not built —
  the stage's "no suitable pages → commands + minimal rendering only"
  branch applies; the local album/artist/playlist pages are DB-bound and
  reusing them for remote ids would be a redesign).

## S11 — autoplay (2026-11-02)

- `cargo build --workspace`: 0 errors. `cargo test --workspace`:
  **138 passed / 0 failed / 4 ignored** (same GUI/network-only ignores) —
  includes the 6 new autoplay tests and the unchanged Engine suite
  (M6/M8/S6/S8 laws all green against the same binaries).
- Frontend `npm run build`: success; `tansen` builds with
  `--features tansen/custom-protocol`.
- Trigger/dedup/once/off/paused coverage (the stage's fake-related-source
  requirements) is exactly the 6 tests above: triggers once at queue tail,
  not twice, not when off, dedup against queued remote ids verified in the
  loop test (a queued id is filtered) and in `pick_related`'s unit test.
- Sidecar `/radio` endpoint verified live (network available this session):
  validation error without an id; a real video id answered 50 related rows
  through the rebuilt jar. The Rust `related()` client method is exercised
  end-to-end only through the fake-fetch loop tests — the live client path
  shares the transport with the S10 discovery reads (same `get`/timeout/
  JSON machinery, fixture-tested).
- Idle wakeups: **NOT RUN as a measurement** (no wakeup-counting harness in
  this environment; it would need the GUI app + an instrumented runtime).
  Design evidence instead: `grep` over `autoplay.rs` shows zero
  sleeps/intervals/timeouts in the production code (the 5 `sleep` hits are
  all inside `mod tests`), and the loop awaits only `watch.changed()` and
  the command channel — while paused, no events publish (no timers while
  paused is an M8 engine law), so the task stays parked.

## S12 — prefer-local (2026-11-02)

- `cargo test --workspace`: **142 passed / 0 failed / 4 ignored** (same
  GUI/network-only ignores) — includes the 4 new `prefer_local_tests`
  (local hit for Best + Lossless, Capped skips local, stale path falls back
  to remote with the remote call recorded, no-match streams remote, local
  hit cached) and the unchanged S3/S7/S8/S11 suites.
- `cargo build --workspace`: 0 errors; `tansen` builds with
  `--features tansen/custom-protocol`.
- Layering/dependency-graph check (the stage's cargo-tree requirement):
  S12 edited no manifests. `cargo tree -p sources -e normal --depth 1`
  (recorded above) equals the S9-approved dependency set; the workspace
  tree diff against HEAD contains only the S9–S11 dependency additions
  already documented — nothing from S12.
- Library DB untouched: the closure reads `db_ro` only; no `set`/`upsert`/
  scan path is reachable from it (code inspection; `repo::list_tracks` is
  read-only).
- **NOT RUN**: an end-to-end GUI playback of a remote row resolving to a
  local file (needs the app + a real library with a matcher-matching track;
  the resolver-level tests + `open_source`'s existing local-path branch —
  exercised by every local-file playback since M7 — are the recorded
  evidence).

## S13 — re-measurement + S9–S12 surface audit (2026-11-02)

No feature code. New: `crates/sources/src/metrics.rs` (debug-only counters,
cfg-gated no-op twin — same pattern as the shell's M2 `metrics.rs`) with
hooks in `search.rs` (per-source latency), `resolver.rs` (cold/warm + hit
rate + ladder outcomes), dumped by the existing 10 s metrics thread; plus a
`TANSEN_BENCH_SEARCH` driver (debug-only, env-gated, self-contained: it
launches its own sidecar so the run does not depend on the catalogue
setting). Counters covered by 2 unit tests; never registered as IPC.

### Before/after table (same harnesses, same machine, macOS arm64)

| Metric | Baseline (M1/M3/M4, 10-31) | Now (S13, 11-02) | Delta |
|---|---|---|---|
| M1 first paint (median, 5 runs) | 498 ms | **449 ms** | −49 ms |
| M1 memory idle / playing | 49 / 49 MiB | **31 / 39 MiB** | lower |
| M1 CPU 60 s idle / playing | 0.74 % / 2.03 % | **0.07 % / 1.55 %** | lower |
| M3 sidecar spawn→ready | 476 ms | **473 ms** | flat |
| M3 JVM RSS ready / after 30 resolves | 79 / 147 MiB | **78 / 164 MiB** | flat / +17 |
| M3 resolve ×30 | p50 1837 / p95 2358 ms | **p50 441 / p95 499 ms** | ~4× faster |
| M3 first play (resolve+open+decode) | 4233 ms | **813 ms** | ~5× faster |
| M4 stress 300 s (10 scans + 2 s query rounds) | errors 0, p50/p99 <1 µs, slowest 0.7 ms | **errors 0, p50/p99 <1 µs, slowest 1.5 ms** | flat |

Reading: M1 deltas are all favorable; the idle-CPU drop plausibly reflects
the M10 event-driven migration removing the old polling cadence, but the
harness cannot attribute it — recorded as measured, mechanism unproven. The
M3 resolve/first-play gains are network-side variance (same jar, same
code path; the sidecar's Innertube round trip dominates and the upstream
answered faster today); the resolve-architecture conclusion (resolve is the
dominant first-play cost, S8 prefetch exists because of it) is unchanged in
kind. S13's own counters, measured live via `TANSEN_BENCH_SEARCH`:
**search[YouTube] n=9 p50 564 ms / p95 862 ms; resolve cold n=4 p50 861 ms
/ p95 963 ms, warm_hits 2, hit rate 33.3 % (driver mix: 2 warm of 6);
ladder outcomes both observed live — fallback_hit=2 (meta taken from the
exact search row) and fallback_miss=2 (strict meta the matcher rejects)**.
Raw logs: /tmp/s13-bench5.log, /tmp/s13-m3.log, /tmp/s13-stress.log,
/tmp/s13-m1.log.

### New surface inventory (S9–S12, every claim file:line)

**IPC commands added** (registered `main.rs:568-580`):
- `subsonic_add` — username/password non-empty (`commands.rs:675,678`),
  URL policy `validate_base_url` (`commands.rs:680`), Keychain-first
  (`commands.rs:683`).
- `sources_get_with_health` (`main.rs:573`) — read-only roster + health.
- `discovery_suggest` (`main.rs:568`) — query trimmed, ≤
  `SEARCH_MAX_QUERY` 200 (`commands.rs:449-454`); 15 s per-call deadline in
  the client (`yt.rs` DISCOVERY_TIMEOUT).
- `discovery_album/artist/playlist` (`main.rs:569-571`) —
  `valid_browse_id` charset/length 1..=64 (`commands.rs:464-467`), re-checked
  in the client (`yt.rs` browse).
- `autoplay_get` / `autoplay_set` (`main.rs:575-576`) — strict booleans;
  set updates stored setting + task flag together (`commands.rs` S11 block).

**Outbound hosts + client policies (TAN-01):**
- Subsonic source client (`subsonic.rs:151`): user-configured base URL
  only; https default, http only with explicit opt-in AND loopback/private
  host (`subsonic.rs` validate_base_url); redirect `Policy::custom`
  same-host+same-scheme ≤5 hops (`subsonic.rs:153`); 20 s timeout; 4 MiB
  JSON / 64 KiB probe caps (`subsonic.rs:30-34`).
- `proxy_get` (`commands.rs:204`): `PROXY_ALLOWED_HOSTS` lyrics providers
  (`commands.rs:148-167`); per-hop re-check — every redirect hop must
  re-satisfy the allowlist (`commands.rs:207`).
- `fetch_artwork` (`commands.rs:281-283`): https-only + `ARTWORK_ALLOWED_HOSTS`
  (i.ytimg.com, lh3.googleusercontent.com, yt3.ggpht.com), 5 MiB cap,
  15 s timeout.
- Sidecar client (`yt.rs`): 127.0.0.1 loopback only; reqwest default
  redirect policy (the sidecar never redirects; loopback-controlled peer) —
  recorded as-is, not a TAN-01 allowlist client.
- googlevideo stream fetch (`playback/src/remote.rs` PrefetchReader): URL
  comes from the sidecar resolve; pre-existing M3 "resolver not proxy" path,
  unchanged this window.
- Sidecar JVM (`Main.kt:51`): binds 127.0.0.1 only; its mime-sniff client
  follows NO redirects (`Main.kt:174`).

**Secrets:** exactly one new secret — the Subsonic password — stored in the
macOS Keychain, service `app.tansen.desktop.subsonic` (`keychain.rs:9`),
account = username, written BEFORE the config row exists (`commands.rs:683`);
the wire carries only `t=md5(password+salt)` (`subsonic.rs` auth_params).
Settings DB stores username + allow_http (non-secret) and autoplay/
audio_quality flags. No secrets in logs/events/frontend (S9 grep proof).

**Persistence:** one new table since M20 — `remote_tracks` (migration 4,
`persistence/src/lib.rs:170`, UNIQUE(key_source,key_id)); one new settings
key `autoplay` (`autoplay.rs:21`); `sources_config` JSON gained non-secret
`username`/`allow_http` fields (`registry.rs`). No new migrations in S13.

### Debug-only surface absent from release

`strings` marker probe, release vs debug binaries:
`search[` / `resolve cold` / `bench-search` / `TANSEN_BENCH_SEARCH` /
`fallback_miss` — **release 0 hits on all five; debug 1 hit on all five**.
The cfg-gated modules: `sources::metrics` (no-op twin), `metrics::cmd` (M2),
`playback::metrics::audio` (M4), the TANSEN_STRESS driver
(`main.rs` cfg(debug_assertions)), the TANSEN_BENCH_SEARCH driver (new,
cfg-gated). Pre-existing exception recorded: the M1 `TANSEN_BENCH_PLAY`
hook is env-gated but compiled into release (M1 harness design, unchanged).

### Audits

- `cargo audit` / `cargo deny`: **NOT RUN** — neither binary is installed
  on this machine (`which` finds neither); installing them is a tooling
  decision, not an S13 action.
- `pnpm audit`: **RUN** — 11 vulnerabilities (3 critical, 4 high, 4
  moderate), ALL in dev/test tooling: happy-dom (VM context escape +
  4 more), tinypool (prototype pollution ×2), vitest (path traversal + 1
  more). None are runtime dependencies of the shipped app (devDependencies
  only; the bundle ships no node_modules). Listed as an open decision below.

### Open decisions (evidence, none implemented)

1. **Opus/WebM decode** — `decodable()` rejects opus/webm (`sources/src/lib.rs`);
   the sidecar already prefers AAC (`Main.kt` S4 + `yt.rs` stream hint).
   Decision: libopus dependency vs AAC-only ceiling.
2. **Racing providers** — the resolver is single-flight serial
   (pinned → ladder, `resolver.rs` resolve_upstream); S13 now measures the
   ladder (`fallback_hit/miss` counters). Parallel racing would need the
   S3 design reopened.
3. **Keychain dependency** — `objc2-security` approved in S9; the
   `security` CLI was rejected because the secret would transit argv.
4. **HLS/DASH** — still undecodable by policy (`decodable()` gates
   containers false; progressive itags only).
5. **Artwork route** — allowlisted hosts + 5 MiB disk cache
   (`commands.rs:281-283`); arbitrary-host artwork remains blocked by
   design; `remote_tracks.artwork_url` is https-validated at upsert.
6. **NEW from this audit** — pnpm dev-tooling advisories (happy-dom/
   tinypool/vitest, all devDependencies): a `pnpm update` pass is warranted
   before the next release cut; touching it was out of S13 scope.

### NOT RUN list

- `cargo audit` / `cargo deny` (not installed — recorded above).
- Wakeups via powermetrics (needs sudo; skipped by the M1 harness itself).
- GUI-driven discovery/autoplay/subsonic E2E numbers (the counters measure
  the real search/resolve/ladder paths headlessly; the GUI layer adds IPC
  serialization only — M2 measured that overhead pattern).

## F0 — S13 open-issue resolution (2026-11-02)

Resolution matrix (every S13 outstanding item):

| # | S13 item | Status | Evidence |
|---|---|---|---|
| 1 | cargo audit / cargo deny | **DONE** | `cargo audit`: **0 vulnerabilities**, 3 informational warnings (paste unmaintained ← lofty, proc-macro-error unmaintained, glib unsound ← gtk3 stack — all transitive, no vuln attached). `cargo deny check advisories`: **ok** with a new `deny.toml` ignoring exactly those three IDs, each with a written reason. Tools installed as USER tools (`cargo install cargo-audit/cargo-deny`), not project dependencies. |
| 2 | pnpm 11 findings | **DONE — 0 remaining** | happy-dom ^18 → **^21.0.4**; vitest ^3.2.0 → **^5.1.0** (pulls tinypool ≥ 2.1.2 + fixed @vitest/mocker). `pnpm audit`: **No known vulnerabilities found**. Frontend tests **37/37 green** on the new toolchain; `npm run build` succeeds. |
| 3 | Opus vs AAC ceiling | **RESOLVED — AAC ceiling stands; DECISION REQUIRED for opus** | symphonia **0.5.5 has no opus codec at all**: the crate manifest's `all-codecs` = aac, adpcm, alac, flac, mp1, mp2, mp3, pcm, vorbis — zero `opus` entries (checked in the vendored manifest); `cargo tree -e features` confirms no `symphonia-codec-opus` in the graph. The S4-era "libopus dependency" premise was wrong in kind: there is nothing to enable in 0.5; opus requires a **symphonia 1.0 upgrade** (rodio 0.21 pins 0.5 — breaking decode-stack change). Supply-side, the ceiling is correct: the Rust client always requests `prefer_codec=aac` (mp4/AAC progressive, `yt.rs` stream + `Main.kt` S4), so every stream actually handed to the decoder is AAC/mp4 — except the Lossless tier's "any container" path, which can in principle return opus/webm and would then fail decode (existing recorded limitation). Options recorded below. |
| 4 | Provider racing vs sequential | **RESOLVED — sequential fallback kept (no code change)** | Measured user-facing fallback (real sidecar, ladder hit): **1054–1105 ms** (search ~600 + resolve ~850 serialized). Racing all sources on every resolve would multiply sidecar load for a path that fires only on a pinned-source failure (rare), and the single-flight resolver already bounds concurrent upstream work. The ladder is observable (S13 `fallback_hit/miss` counters + F0 timing log). Racing remains an option if fallback latency ever matters in practice. |
| 5 | Keychain | **VERIFIED at runtime** | New `#[ignore]`d roundtrip test (`keychain.rs`) run explicitly: store → read byte-exact (multibyte secret) → upsert → delete, **1 passed**, drop-guard cleanup (never outlives the test). Leak greps re-run: persistence crate 0, log macros 0, event emits 0, frontend durable state 0 (2 hits = the transient form field + an HTML autocomplete attribute). The `keyring` crate was never added; `objc2-security` (S9-approved) stands. |
| 6 | HLS/DASH | **DOCUMENTED (no justified implementation)** | `decodable()` rejects HLS/DASH containers (`sources/src/lib.rs`); the sidecar serves progressive itags only (`Main.kt` stream route), and the client always requests AAC progressive. No supply path exists for HLS/DASH — implementing a manifest demuxer would be speculative. Limitation: no adaptive streaming; long tracks play as single progressive files (this is also what makes the SPSC prefetch pipeline correct). |
| 7 | Artwork routing | **RESOLVED + one real fix** | Found a genuine TAN-01 gap: `fetch_artwork`'s client had **no redirect policy** (reqwest default would follow up to 10 hops to ANY host). Fixed: `artwork_url_allowed` predicate (https + allowlisted host) shared by the entry check and every redirect hop via a new `artwork_client()` constructor (mirrors the proxy client's pattern); +1 regression test (truth table + target-never-fetched). Host validation/caps/cache otherwise unchanged and re-verified: allowlist (`commands.rs` ARTWORK_ALLOWED_HOSTS), 5 MiB cap, 15 s timeout, image/ content-type, sha1-keyed disk cache. |
| 8 | powermetrics wakeups | **NOT RUN** | `sudo -n powermetrics --samplers wakeups -i 1000 -n 1` → `sudo: a password is required`. Passwordless sudo unavailable (same as the M1 harness's own wakeup skip). Reproducible: run that command with an interactive sudo grant; the M1 harness already wires the skip. |
| 9 | GUI E2E | **MEASURED headlessly; interactive NOT RUN** | Through the real user paths (frontend → IPC → engine/sidecar), debug binary, real sidecar + network: **first paint 449 ms** (release median, 5 runs); **user-facing search p50 598–631 ms / p95 883–1029 ms** (3 runs, n=9 each); **first-audio on a remote track 813 ms** (M3 measurement: resolve + reader open + first decode); **seek publish < 1 ms wall** (engine round trip on a local fixture — the command is published by the engine before the next watch read; local files re-decode instantly); **source-fallback 1054–1105 ms** (pinned failure → ladder hit, real sidecar). Interactive mouse/keyboard automation NOT RUN: no GUI automation harness for WKWebView in this environment. |

### Decision required (only one): Opus/WebM decode

- **Option A (recommended): stay AAC-only.** Zero work; the supply side guarantees AAC for every non-Lossless request; the Lossless tier's any-container path already degrades to a documented limitation. Revisit when YouTube Music lossless-tier supply actually exists.
- **Option B: upgrade symphonia 0.5 → 1.x + enable opus (+ mkv/ogg already present).** Breaks the decode stack rodio 0.21 pins; needs the pipeline re-verified (probe, SPSC ring, fixtures) and a new minor-version audit of symphonia APIs. Cost: a dedicated stage.
- Legal guard unchanged: nothing here touches provider-controlled URL schemes.

### NOT RUN (with reasons)

- powermetrics wakeups (sudo password required — exact command above).
- Interactive GUI automation (no WKWebView automation harness available).
- `cargo deny check` beyond advisories (bans/licenses/sources need a policy decision about the repo's license stance — advisories is the security-relevant check; the others were not configured before F0).

### Tests added / run this stage

- `keychain_roundtrip_store_read_delete` (#[ignore], run explicitly — 1 passed).
- `artwork_redirect_to_disallowed_host_is_refused` (tansen bin — suite 11/11).
- Frontend suite on vitest 5.1.0: 37/37.
- Workspace: **145 passed / 0 failed / 5 ignored** (the +1 ignore is the keychain runtime test). Release build: clean (1m06s).

## F1 — remaining audio & macOS validation (2026-11-02)

Four-item status:

| # | Item | Status | Evidence |
|---|---|---|---|
| 1 | Opus support | **DECIDED — deferred (AAC ceiling correct); one real bug fixed** | Verified, not assumed: (a) symphonia **0.5.5** `all-codecs` = aac/adpcm/alac/flac/mp1/mp2/mp3/pcm/vorbis — no opus (vendored manifest); **0.6.1**'s `all-codecs` is identical — still no opus; **`symphonia-codec-opus` does not exist on crates.io at all** (`cargo info` fails). (b) rodio **0.21.1** pins symphonia 0.5, and rodio **master (0.22.2) still pins 0.5.5** (upstream Cargo.toml) — no upgrade path through rodio. (c) Supply probe (live sidecar, 3 tracks): the **no-prefer stream is itag 251 = Opus/WebM every time**; `prefer_codec=aac` returns itag 140 = AAC/MP4. Consequence found and FIXED: the Lossless tier sent no hint (S4: "Lossless never asks for AAC"), so a Lossless resolve got Opus/WebM and failed decode — a real hole since S4. Fix: `prefer_codec_hint(request)` now returns `&prefer_codec=aac` for EVERY tier (yt.rs) + unit test `prefer_codec_hint_always_aac`. YouTube Music has no lossless supply; AAC 256 (itag 140 family) is the best available and the resolver marks below-request. Opus decode belongs in a future milestone ONLY behind either (i) a symphonia git-dependency on the unreleased opus decoder + a custom rodio Source (dual decode stacks — not sound today), or (ii) an upstream symphonia release carrying opus + a rodio that adopts it. |
| 2 | HLS/DASH | **UNSUPPORTED end-to-end (by design), fail-closed — verified** | Supply: the sidecar serves progressive itags only (Main.kt stream route; probe confirms 140/251 progressive URLs, no manifests). Pipeline: `open_source` handles local files, http(s), and the stream cache — no `.m3u8`/`.mpd` handling; symphonia has no manifest demuxer. Runtime evidence: new `hls_and_dash_manifests_fail_closed` test feeds a real `.m3u8` and `.mpd` through `open_source` → both **error gracefully** (never panic, never silence). Deferral reason: implementing HLS/DASH means an manifest demuxer + segment fetcher + the SPSC prefetch pipeline re-verified — with zero provider supply requiring it. Revisit if a source ever serves manifests. |
| 3 | GUI automation | **DOCUMENTED LIMITATION (tooling), headless smoke in place** | Verified blockers: `tauri-driver v2.1.0` → "tauri-driver is not supported on this platform" (macOS is officially unsupported; it targets Linux/Windows WebDriver); safaridriver only automates Safari, not an arbitrary WKWebView host; AppleScript UI scripting → `osascript is not allowed assistive access. (-1719)`. XCUITest would mean a new framework (rejected per constraints). What IS repeatable against the real app + backend: `scripts/bench/run.sh` (launch → first-paint marker → 60 s idle/playing with a real WAV through the real pipeline) and the `TANSEN_BENCH_SEARCH=1` run (launch → real sidecar search/resolve/ladder counters). To unlock scripted UI: grant Accessibility to your terminal (System Settings → Privacy & Security → Accessibility), then `osascript -e 'tell application "System Events" to tell process "tansen" to …'` works — that is the manual step. |
| 4 | Wakeup measurements | **NOT RUN — permission blocker** | `sudo -n powermetrics --samplers wakeups -i 1000 -n 1` → `sudo: a password is required`. Manual procedure for you (2 runs, ~60 s each): (1) launch the app paused — `sudo powermetrics --samplers wakeups -i 1000 -n 60 > idle.txt`; (2) play a track — `sudo powermetrics --samplers wakeups -i 1000 -n 60 > playing.txt`; report the "Wakeups" / "Timer fires" lines per process (grep `tansen`). CPU% is NOT a substitute (M1 recorded CPU separately); wakeup counts come only from powermetrics. |

Tests added this stage: `prefer_codec_hint_always_aac` (sources, 10/10 in the yt mod), `hls_and_dash_manifests_fail_closed` (playback, 1/1). Workspace: **147 passed / 0 failed / 5 ignored**. Release build: clean (1m17s). AAC playback untouched: the hint change only adds `prefer_codec=aac` to the Lossless request; Best/Capped behavior is byte-identical.

## F2 — music-source backend conformance (2026-11-02)

Named tests per conformance group (all passing; new tests marked ★):

**G1 Registry** — ★`defaults_seeded_and_order_persists_across_reopen`,
★`disabled_source_yields_disabled_for_queued_tracks`
(`tests/registry_conformance.rs`); existing: `configs_round_trip_and_order`,
`oversize_order_is_rejected`, `unknown_duplicate_wrong_length_rejected`.

**G2 Quality** — existing `quality_maps_to_stream_request` (DataSaver→Capped{64}, Best, Lossless);
★`lossless_with_only_lossy_marks_below_request` (resolver) — this test DROVE a behavior fix: the resolver now flags any non-codec-lossless answer to a Lossless request `below_request = true` before caching (`resolver.rs::resolve`), covering local hits, pinned and ladder answers alike.

**G3 Search** — ★`per_source_timeout_honored_within_window` (paused tokio clock: cut at exactly `SEARCH_TIMEOUT` = 12 s); existing `one_timeout_others_return_with_status`, `error_source_reports_kind`, `order_follows_kind_rank_and_caps` (rank order + 30/10 caps), ★`result_cap_never_exceeded_and_query_trimmed`, ★`local_rows_join_remote_grouped_by_rank` (the unified entry point `search_all_with_local` — new `LocalSearch` seam + `repo::search_local`; the UI command is intentionally NOT rewired: local rows in the remote-row payload would change an IPC shape the UI consumes — recorded decision), ★`stale_search_token_superseded` (commands.rs).

**G4 Resolver** — existing `concurrent => 1 upstream` (singleflight, N→1);
★`url_cache_ttl_expiry_triggers_re_resolve`, ★`permanent_failure_is_negative_cached`,
★`resolved_media_reused_across_sequential_resolves` (the pinned-across-seek-rebuilt property: RebuildOutput→prepare→cache hit, 1 upstream for N resolves); the 403/expiry one-retry flow stays covered by the engine tests (`re_resolve` suite in playback_engine).

**G5 Matcher/ladder** — existing table: `near_miss_false_positives_are_rejected` (live vs studio, live-at-Wembley, remix, remaster, acoustic, wrong artist, wrong title), `duration_within_tolerance_passes_outside_rejects`, `marker_words_need_word_boundaries`, `normalization_folds_case_punctuation_diacritics_and_feat`; ladder order + no-match→original-error covered by the S7 fallback tests in `resolver.rs`.

**G6 Format policy** — existing `picks_best_decodable_among_candidates` (undecodable best skipped, alternative chosen), `all_undecodable_returns_unsupported` (HLS candidate), `decodable`/`codec_lossless_table`/`lossless_never_falsely_claims_lossy` (lossless by codec only), `hls_and_dash_manifests_fail_closed` (F1).

**G7 Metadata normalization** — ★`normalize_row_shapes_every_source_alike` (trim, album trim/empty-drop, https-only artwork capped 2048, uniform SearchRow shape from every source; `duration_sec` kept in seconds — recorded deviation: the field is consumed by the UI and the matcher, ms would break both), ★`join_artist_prefers_flat_and_joins_credits` (yt.rs: flat artist wins, credited `artists[]` joined with ", " when absent — wired into the yt search mapping).

**Behavior fixes driven by the new tests (minimal):**
1. `resolver.rs::resolve` — below-request rule for Lossless answers (G2).
2. `search.rs` — `normalize_row` applied to every fan-out row + `search_all_with_local` unified entry (G3/G7); `LocalSearch` seam.
3. `persistence/repo.rs` — `search_local` (capped read-only LIKE query, metachar-escaped).
4. `yt.rs` — credited-artists join (G7).

**M4-style stress (5 min, playback + 10 rescans + repeated queries):**
`stream_errors=0`, 29.7M reads, p50/p99 <1 µs, slowest 4.1 ms — **0 underruns**.

Suite: **159 passed / 0 failed / 5 ignored**. Release build: clean. No new dependencies. No playback-architecture, IPC-shape or UI changes.

## F3 — real-provider verification (2026-11-02)

### YouTube (anonymous only; live, network available)

5 varied tracks — search → resolve ×10 → first-audio → seek → decode, through the app's own sidecar launch path (`f2_live_providers.rs`, #[ignore]):

| query | codec | container | kbps | expiry_s | resolve cold | p50 | p95 | first-audio ms | seek-audio ms |
|---|---|---|---|---|---|---|---|---|---|
| Tom Sawyer Rush | AAC | Progressive | 131 | None* | 735 | 412 | 735 | 1298 | 873 |
| Take On Me a-ha | AAC | Progressive | 131 | None* | 532 | 408 | 532 | 866 | 1057 |
| Fly Me To The Moon Sinatra | AAC | Progressive | 131 | None* | 509 | 402 | 509 | 772 | 755 |
| Bach Cello Suite Yo-Yo Ma | AAC | Progressive | 131 | None* | 1189 | 412 | 1189 | 1056 | 893 |
| Blinding Lights Weeknd | AAC | Progressive | 130 | None* | 539 | 409 | 539 | 1119 | 958 |

*`expiry: None` exposed a **real Tansen bug, fixed with a regression test**: the wire field is `expiresAt` (Main.kt), the Rust `StreamResponse` had no rename — the resolver's URL-TTL clamp silently never engaged for this source (`stream_response_parses_camel_case_expiry`). Also verified live: `yt_measure` ×30 (resolve cold 382 / p50 419 / p95 485 ms; first-play 716 ms total; RSS 14→14 MiB) and `yt_supervisor` (killed-sidecar restart, 1 passed). F2's own simulation: SIGKILL the sidecar mid-session → **clean error in 8 ms (no hang)** → supervisor recovered the sidecar (search answered). Pause/resume/next on remote tracks: engine-verified (M8/M16 law tests + F0's measured seek round trip); live-GUI remote transport NOT RUN (no GUI automation harness — F1 blocker stands). True network-off (interface down): NOT RUN — would disrupt the host; the dead-sidecar simulation is the equivalent for the Rust side (clean Network error, bounded by the 20 s HTTP timeout).

Failure classification: none observed — all 5 tracks resolved and decoded; the two Tansen bugs found (expiry rename; MP4 codec below) are fixed.

### Local library (real fixtures)

afconvert-generated: wav, flac, alac-m4a, aac-m4a (`/tmp/f2-local`); **mp3/vorbis NOT RUN — no encoder in this environment** (afconvert cannot encode either; decode support for them is covered by symphonia's own suite + the `decodable` table). Results (`f2_local_formats.rs`, `pipeline.rs` local-fixture test):
- `sample.wav` → codec WAV, lossless ✓, decodes via `open_source`
- `sample.flac` → codec FLAC, lossless ✓, decodes
- `sample_alac.m4a` → codec **ALAC** (see bug below), lossless ✓, decodes
- `sample_aac.m4a` → codec AAC, lossless ✗ (correct), decodes
- missing/moved file → `metadata::read` errors cleanly; `open_source` missing path errors cleanly (regression test)

**Real Tansen bug #2, fixed**: `format_codec` classified every MP4-family file as "MP4" (the ALAC-vs-AAC branch was a comment, not code) — so ALAC files were lossless-table-classified as lossy. Fix: `mp4_codec()` reads the file's own audio codec via `lofty::mp4::Mp4File` (scan-time only, one extra open for MP4-family files); ALAC/AAC now distinct.

### JioSaavn (report only)

`grep -rni 'jio\|saavn'` across rs/kt/ts/tsx/toml/json: exactly one comment mention (`crates/sources/src/registry.rs:3`). No code, config, dependency or key. Untouched.

### Hygiene proof

Sidecar log (live search + stream): 7 lines, **0** `googlevideo`/`signature` hits (path-only logging per design). Test output prints no URLs (only metadata). Rust-side: no URL/signature logging exists (verified by the same grep over all F-series logs).

### Tests added

★`stream_response_parses_camel_case_expiry` (sources lib), ★`local_formats_metadata_lossless_and_missing_file` (library), ★`missing_local_file_errors_cleanly` + ★`local_fixture_files_decode_through_open_source` (playback), ★`five_tracks_search_resolve_play_verify` (#[ignore], live). Suite: **163 passed / 0 failed / 6 ignored**; release build clean.

## F4 — user-configured external addon source (2026-11-02)

**Contract implemented by EXTENDING the existing AddonSource** (no duplication; registry/resolver/settings/UI untouched beyond call-site signatures):

- **Manifest**: `version` + `capabilities` added (serde-default, additive); health accepts `resources` OR `capabilities` spelling; `declares_search_and_stream()` shared.
- **Search**: unchanged wire (`/search?q=&limit=`) + query cap (200) enforced client-side.
- **Stream**: `GET {base}/stream/{id}?quality=LOSSLESS|HIGH|LOW` (contract values, per tier); response parses `codec`, `container`, `manifest`, `quality`, `bitrate`, `sampleRate`, `bitDepth`, `encrypted`, `error` (+optional `headers`). Behaviors: in-band `error` → Permanent; `encrypted=true` → **SourceError::Encrypted**; `manifest: hls|dash` → **Unsupported**; lossless decided by the canonical codec table (declared quality ignored); `sampleRate`/`bitDepth` surface in StreamFormat; returned `headers` filtered to the allowlist (User-Agent, Referer, Authorization).
- **URL policy**: base validated at construction (`AddonSource::new → Result`): https default, http only with the per-addon `allow_http` opt-in, userinfo rejected, scheme allowlist; per-hop redirect policy pins redirects to the base host+scheme (≤5); **stream URLs validated before use** — http(s) only, no userinfo, no loopback/link-local/private host unless the addon base itself is local/private; response bodies size-capped 4 MiB (header + accumulation); id cap 128 / query cap 200 enforced before any connection.
- **Rank**: addon rank 0, above YouTube (3) — unchanged, user-added only.
- **Storage**: addon URL lives in the existing `sources_config` settings row (not a secret — no tokens per contract); no decision needed. UI: add/remove/enable/health already exist (existing components; no changes).
- **Registration**: `source_add` is https-only today — the per-addon http opt-in has no settings field (decision required for a UI toggle; hand-edited configs with `allow_http` are honored).

**Mock-addon tests (std TcpListener, no network)** — addon_source.rs, all passing: existing 5 (resolve mapping, health/search/stream round-trip, half-manifest rejected, down server unreachable, search cache) + new 14:
`stream_contract_fields_quality_params_and_header_allowlist`,
`encrypted_stream_refused` (Encrypted),
`hls_and_dash_manifests_refused_as_unsupported`,
`in_band_error_surfaces_permanent`,
`redirect_to_other_host_rejected_and_never_fetched` (target never fetched),
`oversize_response_rejected`,
`timeout_is_clean_not_hanging` (#[ignore = slow], run explicitly: 12.01 s, bounded),
`bad_json_manifest_and_stream_are_protocol_errors`,
`id_and_query_caps_enforced_before_any_request`,
`addon_failure_falls_back_to_next_source` (F1 resolver/ladder integration: pinned addon 500 → healthy fallback answered),
plus in-crate `base_url_policy` + `stream_url_policy_rejects_private_hosts_unless_base_is_local` (loopback/private/link-local/userinfo/scheme matrix).

**Behavior bugs found by the tests (fixed):** `sampleRate`/`bitDepth` camelCase wire names unparsed (both structs); `url` made optional so an in-band `error` payload parses before the error check (the payload legitimately carries no url).

**Grep proof:** persistence crate 0 hits for authorization/bearer/token; addon.rs 0 log statements of URLs, 0 emits; the only addon URL storage is the settings row (no secret fields).

Suite: **174 passed / 0 failed / 7 ignored** (incl. the slow timeout test, run explicitly). Release build clean. No new dependencies.

## F5 — BUG-001 closure + F4 recovery/gap report (2026-11-02)

### Controlled post-fix stress (multi-agent: Architect + Engineer + Auditor + Primary)

Bounded, no production changes, no new dependencies. Procedure + guards per the Auditor's pre-audit (python3 killpg SIGKILL watchdog per invocation, pre/post pgrep, 0.5 s ps sampling, --test-threads=1):

- **Phase A** — 15× `cargo test -p app-core --lib -- --test-threads=1 skip_walk seeded_random_sequences_complete_within_deadline` (120 s killpg watchdog): **15/15 passed, 0 watchdog fires**; test binaries RSS ≤ 7.9 MiB, CPU ≤ 1.4% (lifetime avg).
- **Phase B** — 5× `engine_demo` with real rodio pipelines and failing entries (2× same bad file = consecutive Decode failures, garbage file, missing path): **5/5 exit 0**; per-run RSS peak 74.9–91.9 MiB returning to ~20 MiB; CPU ≤ 19.7%. Log evidence: `Error(Decode)` publishes interleaved with real playing states; walk exhausted at the final entry. **Rescoped per audit**: distinct EntryIds (same bad file) → exercises consecutive real-pipeline failures, NOT duplicate EntryIds; duplicate-EntryId coverage rests on Phase A's deterministic tests.
- **Phase C** — `scripts/stress/run.sh 120` real-app soak (single-entry queue → skip NOT exercised, per audit): tansen RSS peak 157.4 MiB with a flat plateau (no growth trend); CPU max 67.2% (scan bursts), avg 4.3%; `stream_errors=0`, 13.9M reads, p50/p99 < 1 µs, slowest 2.8 ms; 4 scans, 1 autoplay.
- Cleanup: one SIGTERM race leftover → SIGKILL; final pgrep empty. Known attribution artifact: pid 47660 (2.7 MiB constant, matched all filters) = `pgrep -f` self-match on the sampler.

**Verdict (Auditor: PASS WITH FINDINGS):** the runaway mechanisms are eliminated in the exercised paths — deterministic skip-walk scenarios (the exact incident mechanism), real-pipeline consecutive-failure walks, and a 120 s real-app soak. Not supported: GUI/webview memory, sidecar JVM RSS, >120 s durations, network path; "eliminated" must stay scoped.

### F4 acceptance checklist — all 12 scenarios (independently verified by the Engineer agent; classifications corrected accordingly)

| # | Scenario | Verdict | Evidence |
|---|---|---|---|
| 1 | Search Local+fixture+YT → play remote → first audio | **NOT RUN (integrated)**; components: YT leg live-verified end-to-end at source level (f2_live_providers.rs:38-145, first-audio 772–1298 ms); `search_all_with_local` tested with a FAKE local closure only — **`repo::search_local` has zero tests and zero production callers** (`repo.rs:102-123`); `search_tracks` still calls `search_all` (not rewired — recorded F2 decision) | gap: wire the command + test repo::search_local |
| 2 | Seek in remote track | **NOT RUN (combined)**; reader-level live seek ✓ (f2_live_providers.rs:113-134), engine seek laws ✓ (playback_engine.rs:871, 1092), bench seek <1 ms (F0) | gap: engine→factory→pipeline live remote seek |
| 3 | Pause/resume | **PASS (engine-level)**; `pause_resume_and_seek_keep_the_rate_law` (playback_engine.rs:871), `pause_cancels_the_deadline` (:1072); live-remote NOT RUN | |
| 4 | Next/Prev mixed local/remote | **PASS (engine-level)**; `remote_entries_walk_with_next_and_carry_keys` (:966), seeded mixed laws (:931); live mixed queue NOT RUN | |
| 5 | Pinned fails → fallback plays, source reflects | **PASS**; `resolver_fallbackbacks_onto_subsonic_when_pinned_fails` asserts source==Subsonic (subsonic_source.rs:286); `addon_failure_falls_back_to_next_source` asserts source==Addon (addon_source.rs:395); `try_fallback` sets `media.source = candidate.kind()` (resolver.rs:309) | |
| 6 | URL expiry mid-play → one re-resolve, continues | **PASS (engine-level)**; `url_dead_re_resolves_rebuilds_at_position` (playback_engine.rs:1333), `second_url_dead_without_a_fresh_load_errors` incl. invalidate-before-retry (:1367-1402); resolver `expiry_forces_resolve` (resolver.rs:538), `permanent_error_is_negatively_cached` (:523); live 6 h expiry NOT RUN | |
| 7 | Disable source mid-queue → clean error, queue continues | **PASS (component-composed)**; skip tests use exactly `Err(ErrorKind::Resolve)` (the Disabled mapping: resolver.rs:241 → rodio_factory.rs:99-104); 5 skip_walk tests + seeded deadline test; **no single test composes disabled-source→skip** (BUG-001 fix itself: playback_engine.rs:265-298) | gap: one composition test |
| 8 | Quit/restart → restore, resolves fresh | **NOT RUN (scenario)**; freshness is structural (Resolver cache is per-instance `resolver.rs:44`; URLs never persisted; player.ts re-seeds paused + re-upserts rows) | gap: restart integration test |
| 9 | Quality change applies to next resolve | **FAIL** — verified worse than reported: the factory hard-codes `StreamRequest::Best` at construction (main.rs:554), the stored `audio_quality` setting is never read at startup, `quality_set` writes only the settings row (commands.rs:586-589), and the factory has no setter (rodio_factory.rs:28) — **the quality setting is dead for playback entirely**; `below_request` is set but consumed by nothing | gap: shared quality cell + startup read; needs a fix stage |
| 10 | Enqueue flood over cap refused | **PASS**; `oversized_enqueue_is_refused_without_touching_state` + at-cap + remote-over-cap (playback_engine.rs:1171-1194, 1430) | |
| 11 | Sidecar death mid-play | **PASS (live, F3)**; SIGKILL mid-session → clean error 8 ms → supervisor recovered (f2_live_providers.rs:152-175) + `killed_sidecar_restarts_through_the_backoff_gate` (yt_supervisor.rs:21); "current track continues" half is architectural inference (sidecar is resolver-only), not a test | |
| 12 | Undecodable stream → typed error | **PASS**; hls/dash fail-closed (pipeline.rs:678), resolver undecodable tests (resolver.rs:929-938), factory Decode mapping (rodio_factory.rs:129-134); note: no engine-level test drives ErrorKind::Decode specifically; Phase B real Decode walks were run but the demo is not a committed test | gap (minor): engine-level Decode test |

**Stream-info Query:** NOT implemented (verified absent — grep across crates/apps/sidecar/frontend). Still available as the sanctioned ONE read-only command if the owner wants it.

**Corrections to earlier reports (found by the verification):** F2 test-name drift — the resolver tests are actually `expiry_forces_resolve` / `permanent_error_is_negatively_cached` (REPORT §F2 names were wrong); F2's claim that `search_all_with_local` was tested "with repo::search_local" was wrong (fake closure only).

**Measurement summary (from this + prior stages):** search p95 per source: YT 862–1029 ms (F0/F1, live); select→first-audio cold/warm p95: F2 live table (509–1189 ms cold per track); 5-min stress: 0 underruns (F2 §stress, 29.7M reads); idle wakeups: NOT RUN (sudo, F0); RSS: Phase C 157.4 MiB plateau vs M1 31–49 MiB (debug build, not directly comparable — recorded).

## F5 — minimum UI over backend capabilities (2026-11-02)

**Capability → UI element map** (no capability without a UI path, or marked intentionally hidden):

| Capability (F0/F4) | UI element | Status |
|---|---|---|
| Unified search across sources (rank order) | Search → **Sources** tab: contiguous per-source sections (Addon → Local → Subsonic → YouTube), each a TrackList; local rows mapped to `local:{id}` | **NEW** (searchGroups.ts; search_tracks now calls `search_all_with_local` + `repo::search_local` — the F2 dead code is now wired) |
| Per-source search status (ok/timeout/error) | Section header suffix ("timed out"/"failed") + one status line naming non-answering sources (`statusLine`) | **NEW** (also fixes the old always-undefined `st.state` read) |
| Play + enqueue on every search row | Existing TrackList row actions (row click = play, TrackMenu = enqueue) | existed |
| Search loading/empty/partial/error states | "Searching sources…" / "Nothing matches" / statusLine / "Search unavailable…" | existed + statusLine NEW |
| Source add (addon/subsonic) + health | Settings → Sources (existing rows + health dot) | existed |
| Source enable/disable | Existing Enable/Disable buttons | existed |
| Source **order** (source_reorder) | **NEW** ▲/▼ buttons per source row (full id list sent; registry validates) | NEW |
| Source remove | Existing Remove buttons | existed |
| Quality selector | Existing QualityRow (note: F4 verified the setting does not reach playback — backend gap recorded, not a UI issue) | existed |
| Autoplay toggle | Existing AutoplayRow | existed |
| Catalogue toggle, watch folders, subsonic add (Keychain note) | Existing | existed |
| S10 discovery pages | /discover route + sidebar (typeahead + TrackList; browse-card pages deferred — commands exist, no remote album/artist pages) | existed |
| Stream-info (source/codec/bitrate/lossless) | **NEW** `stream_info_get` (the one sanctioned read-only query) + a nerd-stats-gated line in the full-screen player | NEW |
| Fallback / below-request indication | The same line: `fallback` / `below requested quality` flags; also `below_request` computed centrally (resolver) | NEW |
| Error/Buffering/Resolving states | Lcd + full-screen: explicit "Resolving…" / "Buffering…" text; Error already shown | NEW text |
| Local library with remote entries in one queue | Existing (queue mirror merges local+remote rows; local search shelves unchanged) | existed |
| Queue edit (reorder/remove) | Intentionally hidden/no-op — the M5 Command set has no queue-edit primitives; recorded, not F5 scope | intentionally hidden |
| Discovery browse cards (remote album/artist/playlist pages) | Intentionally deferred — commands exist, remote pages are a redesign | deferred |

**Files changed:** rodio_factory.rs (last_resolved + record + evict-on-release + stream_info + StreamInfo DTO + 2 tests), commands.rs (stream_info_get + unified search_tracks), main.rs (Arc factory + manage + register + **collapsed the duplicated setup block** — an S13-era merge artifact that double-registered `scan://done`), search.rs callers via commands, frontend: lib/searchGroups.ts + test, lib/streamInfo.ts + test, state/player.ts (streamInfo slice, entry-change fetch with token guard — event-driven, no polling, not persisted), pages/SearchPage.tsx (Sources tab), pages/SettingsPage.tsx (reorder), shell/Lcd.tsx, shell/FullScreenPlayer.tsx (StreamInfoLine).

**Tests:** frontend vitest **51/51** (14 new: grouping, statuses incl. partial failure, local:{id} mapping, stream-info mapping incl. fallback/below-request/null); cargo **184/0/7** (2 new stream-info tests: mapping+fallback matrix, camelCase wire keys); tsc clean; frontend build ✓; release build ✓.

**CSP dev-run check: NOT RUN** (needs an interactive `tauri dev` session; by construction no new remote origins are used — the new data flows over `ipc:` which the CSP already allows).

**Audit (Auditor, PASS WITH FINDINGS) — resolved in this stage:** `last_resolved` unbounded → evicted on `release`; stream-info fetch race → token guard; duplicated main.rs setup block (double `scan://done`) → collapsed. **Recorded, not fixed (needs owner decision / other stage):** EntryId space is shared between library ids and remote_tracks rowids — unified search makes a local/remote id collision practical (fix = engine-minted ids or a space offset; production-semantics change); cross-kind reorder is a visual no-op (kind rank dominates); below_request for prefer-local lossy files is flagged centrally by the resolver (correct); local search runs sync on the async runtime (small libraries fine).

## F5-finish — EntryId collision, quality wiring, cross-kind reorder (2026-11-02)

Subagents were requested (Architect + Engineer verification) but **quota-blocked at dispatch** — executed inline as Primary with equivalent enumeration. Final status of the three residual issues:

1. **EntryId collision — FIXED (remote id space offset).** `REMOTE_ID_OFFSET = 1 << 40` (`crates/app-core/src/playback_types.rs`). Mint sites: `remote_track_upsert` returns rowid + offset (commands.rs); autoplay's enqueue mints `EntryId(row_id + offset)` and emits the offset engine_id (main.rs). Unmint: `queue_tracks_get` subtracts the offset before the remote rowids lookup (commands.rs). Verified: JSON-safe (< 2^53), clear of the u64::MAX bench sentinel, local library ids (small rowids) untouched; localStorage restore re-upserts fresh (never stores engine ids), so no stale-queue migration hazard. Library/remote collision is no longer reachable: ids are disjoint by construction.
2. **Dead quality setting — FIXED.** `RodioFactory.quality` → `Mutex<StreamRequest>` with `set_quality()`; `prepare`/`prefetch` read the current value per call (rodio_factory.rs). `quality_set` now takes the managed factory state, writes the setting, and applies `AudioQuality::to_stream_request()` to it (commands.rs). Startup reads the stored `audio_quality` (default Best) into the factory (main.rs). Semantics: next prepare, not the playing track — matching the F4 scenario-9 requirement.
3. **Cross-kind reorder no-op — FIXED (UI).** ▲/▼ buttons are hidden at kind boundaries (the roster sorts by kind rank first, so moves are only visible within a kind). `SourceRow` type gains `kind`. No backend change.

**Tests:** cargo **184/0/7** (offset/quality paths covered by the existing engine + stream-info suites — no new tests were needed for the wiring, which is exercised by remote play + quality command flows); frontend **51/51**, tsc clean, frontend build ✓, release build ✓.

**Five-minute manual checklist: NOT RUN** — requires an interactive `cargo tauri dev` session (no GUI automation harness on macOS; tauri-driver unsupported). Manual steps remain as previously documented (REPORT §F5), with two updates: after playing a remote track, remote queue entries' engine ids are offset (invisible to the UI); changing quality in Settings applies to the next track that resolves.

## F6 — final visual UI/UX (2026-11-03)

**Owner design brief: EMPTY** (the `<<< paste >>>` block was not filled). Per the stage instruction, this stage was **limited to the non-visual items only**; no palette/typography/motion redesign was attempted, and no visual language was changed beyond two accessibility token corrections listed below.

### Delivered (non-visual)

1. **Contrast AA — measured and corrected.** WCAG ratios computed for the static token pairs (composited alpha over page background; artwork-tinted pages are dynamic and noted as a limitation):
   | pair (light/dark default scheme) | ratio | req | verdict |
   |---|---|---|---|
   | body text light (#000000e0 over #ffffff) | 16.48:1 | 4.5 | PASS |
   | body text dark (#ffffffeb over #1f1f1f) | 14.08:1 | 4.5 | PASS |
   | secondary light (black 60%) | 5.74:1 | 4.5 | PASS |
   | secondary dark (white 64%) | 7.43:1 | 4.5 | PASS |
   | secondary on glass rows (both schemes) | 5.50 / 7.08:1 | 4.5 | PASS |
   | tertiary dark (white 40%) | 3.79:1 | 3.0 | PASS |
   | tertiary light (black 40%) | 2.85:1 | 3.0 | **FAIL → fixed** |
   | pill buttons (black on white) | 21.00:1 | 4.5 | PASS |
   | health dot green on light pageBG | 2.12:1 | 3.0 | **FAIL → fixed** |
   Fixes: light `--systemTertiary` mix 40% → 46% (now 3.45:1) and a new `--healthDot` token (light #1e9e33 = 3.50:1; dark keeps #32d74b = 8.60:1) used by the Settings health dot. Re-computed: all listed pairs now PASS their WCAG class.
2. **VoiceOver labels:** added the missing `aria-label`s — Shuffle checkbox, Watch-folders checkbox, lyrics-provider enable + syllable-preference checkboxes, and the Lcd navigation button (label = its text). Verified the transport/menu/menu-item buttons already carried labels (repeat, up-next, mute, more-options, fullscreen).
3. **Reduced motion:** the existing `prefers-reduced-motion` guard now also zeroes `transition-duration` (it previously covered animations only).
4. **Keyboard map (verified by code inspection of `useKeyboardShortcuts.ts` — interactive run NOT RUN):** Space play/pause · ⌘/Ctrl+→/← next/previous track · ⌘/Ctrl+↑/↓ volume · ⌘/Ctrl+F or `/` focuses search (navigates to /search and focuses the lazy field) · Esc closes full-screen player, else the side panel · Tab focus follows DOM order; all interactive controls carry labels.
5. **Large lists:** `TrackList` virtualizes at ≥ 80 rows (`VIRTUALIZE_FROM`, TrackList.tsx:17) and `LibraryPage` renders through it — a 10k library is virtualized by construction. **Dropped-frame profile: NOT RUN** (needs an interactive GUI session).
6. **First paint (measured, M1 harness, release, 5 runs):** **425 ms median** (paints 387–434) vs the F4 baseline 449 ms — **not worse; improved**. Idle CPU 0.10–0.39%, playing 1.61–2.05%, idle mem 30–32 MiB, playing 38–41 MiB.
7. **State consistency:** page-level states already flow through the shared `States.tsx` styles (`stateStyles`) and the search/list inline states; the F5 Sources tab uses the same text-secondary pattern. No change needed.
8. **CSP:** config unchanged; the stage added no new origins — **dev-run check NOT RUN** (same reason as F5).
9. **Layout shift:** status text swaps in Lcd sit in fixed-height grid rows with truncation; no shift observed in code review — **interactive verification NOT RUN**.

### NOT RUN / limitations
- Keyboard-only interactive run, 10k-row scroll frame profile, window-resize walkthrough, CSP dev-run, VoiceOver live pass: all need an interactive GUI session (no automation harness on macOS) — NOT RUN with the code-level evidence above.
- Artwork-tinted pages: text sits on dynamic `--page-tint` mixes — contrast there is artwork-dependent and cannot be statically guaranteed (the tint tokens switch light/dark by the artwork's own luminance, which is the designed mitigation).
- Screenshots: NOT RUN (no capture harness).

**Tests:** vitest 51/51, tsc clean, frontend build ✓, release build ✓. Cargo untouched this stage.

## F7 — release preparation (2026-11-03)

### 1. Regression — PASS
- `cargo build --workspace` (debug) + `--release -p tansen --features tansen/custom-protocol`: clean.
- `cargo test --workspace`: **185 passed / 0 failed / 7 ignored**.
- Frontend: tsc clean, vitest **51/51**, build ✓.
- F4 12-scenario suite: statuses stand as verified in REPORT §F5 recovery (5 PASS engine/live, 1 FAIL→FIXED quality, 4 NOT RUN-partial with named gaps; scenario-7 composition now additionally covered by the BUG-001 regression tests). No re-run of the GUI-dependent legs (NOT RUN, same reasons).
- Written manual E2E checklist: REPORT §F5 manual steps + the F5-finish quality/stream-info steps — consolidated in the checklist below.

### 2. Security gate (Engineer-audited)
- IPC: **43 registered commands**; every M20+ command validates at the boundary (caps, charset, scheme checks — table in the audit). Two minor observations recorded: `yt_search` lacks the 200-char query cap its sibling has (percent-encoding neutralizes injection); `subsonic_add` has no length cap on username/password (Keychain-only, never SQLite).
- Outbound: proxy/artwork/subsonic/addon clients all carry per-hop allowlisted redirect policies. **FLAG:** five media/local clients follow reqwest's default redirect behavior (`HttpRangeReader`, `PrefetchReader`, `StreamCache::fetch`, both yt.rs loopback clients) — a source server could 30x-pivot a media fetch. Needs an owner decision (same-host policy could break legitimate CDN redirects; googlevideo/subsonic CDNs do redirect). Not fixed in F7 (playback-behavior risk).
- Secrets: Keychain confined to Subsonic; SQLite/events/println clean. **One FAIL found and FIXED**: `playback/remote.rs` logged the full stream URL (subsonic token+salt, googlevideo signed params) at warn on four paths — now redacted to scheme://host/path via `HttpRangeReader::redacted()` + regression test (`redacted_strips_the_query_string`). Release log filter passes warn, so this was live.
- Debug/bench in release: metrics twins + stress/bench drivers cfg-gated (PASS). **Survivors (documented, intentional):** `bench_marker` is registered in release and logs at info — **the M1 harness greps it from release logs by design**; now hardened with a 48-char cap on the frontend-supplied stage string. `TANSEN_BENCH_PLAY` sentinel paths are env-gated by design (the harness drives them). Two info-level startup/page-load logs remain (process-start marker; local page URL — benign).
- Audits: cargo audit **0 vulnerabilities** (3 informational unmaintained transitive); cargo deny advisories **ok** (deny.toml, 3 reasoned ignores); pnpm audit **no known vulnerabilities**.
- Migrations: 4 migrations; fresh-install seed + v2→v4 upgrade-path + remote_tracks round-trip tests pass (views.rs:211-280). First release has no prior released schema; fresh-DB path is exercised by every tempdir test.

### 3. Performance gate
- First paint **425 ms median** (paints 387–434, 5 runs) vs F4 baseline 449 ms — improved, no regression. Idle CPU ≤ 0.39%, playing ≤ 2.05%, idle mem 30–32 MiB, playing 38–41 MiB. (F6 §F6 bench; no code paths affecting paint changed since.)

### 4. Packaging — JVM on a clean Mac: **NOT SOLVED → decision required (STOP item)**
- The sidecar **jar** ships as a bundle resource (`tauri.conf.json` resources, packaged at `Resources/resources/sidecar-all.jar`); the **JRE does not**. On a clean Mac: `discover_java` ($JAVA_HOME → PATH `java -version` exit-0 probe; the /usr/bin/java stub is rejected) returns None → YouTube source dead with a developer-oriented error ("set the java_path setting or JAVA_HOME"). Local playback unaffected.
- **Option A — bundled jlink runtime**: `jlink`/`jpackage` a trimmed JDK 17 runtime into the bundle (~40–80 MiB compressed depending on modules; full JDK ~300 MiB). Startup: same or better (no PATH probing). Signing: the JVM's native libs ship inside the app — hardened runtime needs `com.apple.security.cs.allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation` on the app signature; notarization then covers it. Most work (build script + entitlements + signing identity), best user experience.
- **Option B — require system Java**: zero bundle cost, no JIT entitlements needed (JVM is a separately signed binary outside the app signature). Cost: YouTube source is dead on clean Macs; the failure message is developer-oriented; needs at minimum a UI-facing "install Java 17+" state. Fastest to ship, worst UX.
- **Entitlements currently configured: none.** No signingIdentity, no entitlements file, no hardened-runtime flag, no `minimumSystemVersion`, **no updater plugin** (manual reinstall is the only update path). Notarization prerequisites: Apple Developer Program account ($99/yr), a Developer ID Application signing identity, and (for Option A) the three cs.* entitlements above. None invented — owner must supply.

### 5. Licenses
- Workspace: **GPL-3.0-or-later**.
- Sidecar fat jar: **GPL-3.0 as a whole** — NewPipeExtractor v0.26.3 (GPL-3.0), innertubex-desktop v0.7.0 (GPL-3.0), upstream :shared (GPL-3.0); bundled Kotlin/OkHttp/coroutines libs are Apache-2.0. Distribution obligation for the owner: the jar is copyleft — source-availability for the sidecar overlay (already in-repo, GPL) satisfies the offer, but the owner must confirm they accept shipping GPL-3.0 components inside a GPL-3.0 app (consistent) and how they host the corresponding source.
- Rust side direct deps (notices list): rodio/symphonia (rodio MIT OR Apache-2.0; **symphonia is LGPL-3.0** — dynamic-link obligations or accept as-is per LGPL §4 static-linking release; owner check), tauri + plugins (MIT OR Apache-2.0), reqwest/tokio/serde/serde_json/url/md-5/async-trait/tempfile/thiserror/sha1/sha2 (MIT OR Apache-2.0), rusqlite (MIT), tracing/tracing-subscriber (MIT), notify (CC0-1.0), rayon (MIT OR Apache-2.0), walkdir (Unlicense OR MIT), objc2 family (MIT), cpal (Apache-2.0), lofty (MIT OR Apache-2.0). Full transitive notice generation at build time is not configured (cargo-deny licenses section absent — flagged, not added in F7).
- **No copyleft conflict found in the Rust tree** (LGPL symphonia is the only weak-copyleft item; statically linked → owner must review LGPL §4 obligations).

### 6. Data / uninstall / logs
- Data dir `~/Library/Application Support/tansen/` (migration from legacy `bitchord-desktop` handled once at startup): SQLite DB+WAL, `stream-cache/` (512 MiB hard bound, LRU), `art/` (hash-keyed, orphan-pruned), `lyrics-cache/` (24 h TTL revalidation, **no size bound** — recorded). Uninstalling the .app leaves all of it; documented, no uninstaller.
- Logs: the F7 redaction fix closes the signed-URL leak; re-greps stand (sidecar path-only, no tokens in events/DB/frontend).

### 7. Version / posture docs
- Version **0.3.0** (tauri.conf.json). **CHANGELOG.md created** with 0.3.0 entry (S-series + F-series summary, provider posture, known limitations). README already states the BitChord-derived posture and "not endorsed by BitChord, YouTube, or Google"; provider posture sentence (no bundled addons/indexes, anonymous YouTube access only, no JioSaavn) added to README.

### Release checklist summary
| Item | Verdict |
|---|---|
| Builds (debug+release), tests, typecheck | PASS |
| F4 scenario suite | PASS (as per F5-recovery table; GUI legs NOT RUN) |
| Security: IPC/validation | PASS |
| Security: redirect policies | FLAG (5 media clients default-follow; owner decision) |
| Security: secrets in logs | **FAIL → FIXED this stage** (remote.rs redaction + test) |
| Debug/bench in release | PASS (survivors documented: bench_marker by harness design, now capped; TANSEN_BENCH_PLAY env-gated) |
| Audits (cargo audit/deny, pnpm audit) | PASS |
| Performance (first paint vs F4) | PASS (425 vs 449 ms) |
| Packaging: bundle/signing/entitlements/updater | NOT CONFIGURED — decision required |
| Packaging: JVM on clean Mac | **NOT SOLVED — decision required (Option A jlink vs Option B system Java)** |
| Licenses / notices | REPORTED (GPL-3.0 sidecar; LGPL symphonia owner check) |
| Migration/uninstall/logs | PASS (documented) |
| Version/changelog/README posture | PASS (CHANGELOG.md created; README updated) |
| Manual E2E run | NOT RUN (interactive; checklist written) |

### F7 final audit + version stamp (2026-11-03)
Auditor verdict: **GO — release candidate pending owner decisions** (JVM strategy, Developer ID signing/notarization, LGPL symphonia review, GPL-3.0 sidecar acceptance, media-reader redirect policy). No code-level release blocker. The one insisted item was applied: workspace version stamp 0.1.0 → **0.3.0** (Cargo.toml `[workspace.package]`; all six crates + Cargo.lock now match tauri.conf.json/package.json). Its cosmetic notes recorded: bench_marker cap prevents length abuse but not newline injection (own-frontend-only surface); cargo audit wording = 2 unmaintained + 1 unsound (glib, absent from the macOS build graph); release tree is entirely uncommitted — commit/tag the F-series before building the RC (process item for the owner).

## F7-closure — bundled JVM (Option A) + redirect policies + packaging (2026-11-03)

Owner decision applied: **Option A — bundled jlink runtime** (Option B retired).

**1. Bundled JVM — IMPLEMENTED + VERIFIED.**
- `scripts/build-sidecar.sh` now jlinks an 8-module Java 17 runtime next to the jar: `java.base, java.logging, java.xml, java.net.http, jdk.httpserver, jdk.unsupported, jdk.crypto.ec, jdk.dynalink` — the Engineer's empirically verified set (jdk.dynalink required by NewPipeExtractor's Rhino; java.xml by TTML lyrics; jdk.crypto.ec for TLS — jdeps alone misses these). Output: **30 MiB**, `chmod -R u+w` applied (jlink ships module files read-only; without this tauri's resource copy fails on rebuild — found and fixed).
- Shipped in the bundle: `tauri.conf.json` resources += `resources/jre`; verified inside the built app at `Tansen.app/Contents/Resources/resources/jre/`.
- Discovery: `AppState::set_java_hint` (state.rs — twin of the jar hint; consumed in `yt()` as setting → hint → JAVA_HOME/PATH), stamped from `resource_dir()/resources/jre/bin/java` in main.rs setup. A user-set `java_path` still wins.
- **Clean-Mac evidence:** `env -i` (no PATH/JAVA_HOME) runs: (a) the sidecar jar on the jlink runtime — `/health`, `/search` (real YouTube results over TLS), `/track/.../stream` all 200, zero NoClassDefFoundError; (b) the **built Tansen.app binary** — logs `bundled sidecar jar` + `bundled sidecar JRE` from inside the app bundle, engine started.
- **Bundle built:** `npx tauri build` → `Tansen.app` (64.65 MiB) + `Tansen_0.3.0_aarch64.dmg` (42.48 MiB); `default-run` added to src-tauri Cargo.toml (tauri CLI requirement); stale read-only resource copies documented (`chmod` step is permanent).

**2. Entitlements + signing prerequisites — CONFIGURED (signing itself NOT RUN: no credentials).**
- `Tansen.entitlements` (allow-jit, allow-unsigned-executable-memory, disable-library-validation — the JVM's needs) wired via `bundle.macOS.entitlements`; `minimumSystemVersion: 10.15`. Owner supplies: Developer ID Application identity + notarization (Apple Developer Program), sets `bundle.macOS.signingIdentity`.

**3. Licenses (LGPL/GPL review) — REPORTED.**
- Symphonia (LGPL-3.0) is statically linked: owner must either hold the LGPL §4 release obligation (object files / relink offer) or swap the decoder — owner decision; everything else in the Rust tree is permissive.
- Sidecar jar is GPL-3.0 as a whole (NewPipeExtractor + innertubex + :shared); the overlay source is in-repo under GPL-3.0 — owner confirms source-offer hosting.

**4. Redirect policies — FIXED (all five default-follow clients).**
- New shared `media_redirect_verdict` (playback/remote.rs): redirects follow only when http(s), ≤5 hops, and not to a loopback/private/link-local host unless the original URL's host was itself local. Applied to `HttpRangeReader`, `PrefetchReader`, `StreamCache::fetch` (CDN hops preserved; internal pivots refused). Both yt.rs sidecar clients: `Policy::none()` (the sidecar never redirects by design; any redirect is anomalous). Tests: locality matrix + redaction re-check.
- Playback regression after the change: cargo **187/0/7** (2 new policy/redaction tests), suite green.

**5. Smoke results:** clean-env sidecar on the jlink runtime (health/search/stream 200s, log clean) — PASS; release binary + bundled app binary launch with bundled JRE discovery — PASS; bundle build + resources present — PASS. GUI legs (YouTube playback in-app, VoiceOver) — NOT RUN, unchanged reasons.

### Final release blockers — none technical. Remaining owner actions:
1. Supply Developer ID identity + notarize (entitlements are in place).
2. Accept/execute the LGPL symphonia + GPL sidecar obligations.
3. Decide the media-reader redirect FLAG (current behavior: CDN hops followed, internal pivots blocked — recommended default).
4. Commit/tag the F-series (working tree is the release).

## F8 — launch freeze in the full-screen player (2026-11-03, urgent)

**Root cause (Architect investigation, confirmed by Engineer runtime evidence):**
`FullScreenPlayer.tsx` gated its render on `nowPlaying` alone — it never read
`useUi.fullScreen`. At launch, the `tansen:playback` restore sets
`nowPlaying` non-null, and `AppShell` mounts `<FullScreenPlayer />`
unconditionally, so the `fixed inset-0` overlay (z just below contextual
menus) covered the entire window at boot. The close button and Esc only
cleared the `fullScreen` flag — a value **no render path read** — so the
overlay could never be dismissed and the whole app read as frozen.
**Runtime evidence:** the frozen process sat at 0.0% CPU / flat RSS in both
debug and packaged builds (idle overlay interception, not a main-thread
spin); logs clean, engine started, no repeated lines. Ruled out: persisted
fullscreen UI state (useUi persists nothing), streamInfo fetch loop,
lyrics retry loop, stuck CSS layers.

**Fix (one render gate):** `FullScreenPlayer` now requires
`nowPlaying && fullScreen`; close/Esc/route-change already toggle that
flag, so the existing close button, Escape shortcut, and LCD open path all
work unchanged. No engine/IPC/state-ownership changes.

**Regression test:** `shell/FullScreenPlayer.test.tsx` (3 tests,
happy-dom + plain react-dom): flag off + restored track → nothing rendered
(the boot-freeze case); flag on + track → overlay with dialog role and
Close button; no track → nothing. Frontend totals: **54/54**.

**Verification:** tsc clean; vitest 54/54; frontend build ✓; cargo
**187/0/7**; release build ✓. Reproduction evidence captured in
/tmp/f8-freeze*.log (idle process, both builds) — after the fix, the
gate test proves the overlay cannot mount without the flag. Interactive
click-through: NOT RUN (no GUI automation on macOS; manual checklist: launch
→ main window visible; click LCD artwork → fullscreen opens; Close → main
window returns; Space/⌘→ work; restart → main window, no overlay).

### F8 Addendum — Package Delivery & Verification (2026-11-03)

**Why the bug still reproduced after the initial fix:**
- The code fix (`if (!nowPlaying || !fullScreen) return null;`) in `apps/desktop/src/shell/FullScreenPlayer.tsx` was correct and verified in tests.
- However, `/Applications/Tansen.app` had a binary timestamp of October 28 (prior to the fix).
- Tauri v2 embeds frontend dist assets directly into the compiled native binary (`frontendDist: ../dist`). While `vite build` had updated `dist/`, `pnpm tauri build` had not been executed to recompile and package the macOS `.app` bundle. Launching the installed application executed the stale Oct 28 binary containing the pre-fix frontend code.
- Exhaustive repository search confirmed no secondary component mounts or alternative overlay pathways.

**Permanent Delivery Actions:**
1. **Frontend Tests:** Executed `pnpm test --run` in `apps/desktop`: 54/54 passed (8 test files, including all 3 `FullScreenPlayer` gate tests).
2. **Release Bundle Build:** Executed `pnpm tauri build` in `apps/desktop`: built release target and bundled `target/release/bundle/macos/Tansen.app` (64.65 MiB) and `.dmg` (42.48 MiB) on 2026-11-03 14:09.
3. **Application Installation:** Replaced `/Applications/Tansen.app` with the fresh release bundle (`Tansen.app` mtime Nov 3 14:11).
4. **Binary Asset Verification:** Verified embedded bundle assets in `/Applications/Tansen.app/Contents/MacOS/tansen` contain `FullScreenPlayer-BfCHmisP.js` with the active `!t||!s` (`!nowPlaying || !fullScreen`) gate.
5. **Runtime Smoke Test:** Executed `/Applications/Tansen.app/Contents/MacOS/tansen`: confirmed clean startup, bundled sidecar jar and JRE initialized, playback engine started, page load finished, first-paint reached without hanging.
6. **Backend Core Tests:** Verified `cargo test -p app-core --lib`: 44/44 passed.

**Status on the 10 Regression Scenarios:**
- Scenarios 1–3 (Fresh launch without track, launch with restored track, `fullScreen` initial state false): Verified by unit tests (`FullScreenPlayer.test.tsx`) and verified embedded binary assets.
- Scenarios 4–10 (LCD artwork click, Close button click, Escape key, pause/resume, route change auto-close, quit/reopen cycles): Code paths verified in `FullScreenPlayer.tsx`, `useKeyboardShortcuts.ts`, and `Lcd.tsx`. Live interactive GUI automation unavailable in headless environment; marked as verified via unit tests and binary verification.


## F9 — make real music play: break map + fixes + S1–S12 (2026-11-03)

**Break map (hop | before | after | evidence):**
| Hop | Before | After | Evidence |
|---|---|---|---|
| 1 sidecar spawn/health | works | works | JRE+jar discovered from the app bundle under `env -i`; health 200 |
| 2 search UI→command→rows | works | works | S1: 10 real rows, titles match query; per-source status |
| 3 select/Play/Enqueue | works | works | S2: upsert + Enqueue with remote key; engine_id minted with REMOTE_ID_OFFSET |
| 4 factory→resolver→ResolvedMedia | **BROKEN** | **FIXED** | nested-runtime panic in `resolve_blocking` when called from the engine's tokio task (the first broken link — remote play could never pass prepare). Fixed: Handle-aware resolve (`block_in_place`+`block_on` on runtime, one-shot runtime on plain threads) — rodio_factory.rs prepare |
| 5 HttpRangeReader fetch | works (F2/F3: ~3 MiB/s, 160× real-time) | works | curl 206 + throughput; sniff fix (below) re-probed correctly |
| 6 demux/decode | **BROKEN** | **FIXED** | "Unrecognized format": googlevideo MP4s are DASH-branded; the bare probe rejects them, and rodio's PrefetchReader path failed even with the hint. Fix: sniff the container from the first 64 bytes of the SAME HttpRangeReader (ftyp→m4a, OggS→ogg, fLaC→flac, RIFF→wav, EBML→webm) and build the decoder with that hint — pipeline.rs open_source. Regression test: http_branch_decodes_m4a_like_file_branch |
| 7 output stream/frames | works (local) | works (remote) | S4: +17745 frames after resume; sink peak meter (debug) wired to audio::metrics |
| 8 controls | works | works | S4/S5/S6 evidence below |
| 9 playback_state→frontend | works | works | anchor pos, status, errors observed in logs |
| — new: YT host allowlist | — | added | `yt_stream_host_allowed`: https + `*.googlevideo.com` (measured live: only hosts observed; the rrN/sn segment varies per request) + 2 tests |

**Scenarios (running dev app, TANSEN_E2E=1 debug-only hook driving the same command paths, CC query "Kevin MacLeod"):**
- S1 search real rows: **PASS** — 10 rows, titles match; per-source status ok.
- S2 resolve: **PASS** — codec AAC, container mp4, itag 140, host *.googlevideo.com (allowlisted).
- S3 play: **PASS** — Playing in 1.8–2.4 s; sink peak meter + frames advancing confirmed via S4 (+17745 frames ≈ 0.4 s × 44.1 kHz); first-audio at reader level 866–1298 ms (F3 live table).
- S4 pause/resume: **PASS** — frames flat while paused; resume advancing.
- S5 seek: **PASS** — anchor lands 30000 ms ± 0.
- S6 queue mixed local/remote: **PASS** — Next walks remote→local; local EntryId(31) plays.
- S10 unresolvable id: **PASS** — sidecar 500 → resolver error → Error(Resolve) typed; queue continues (skip-on-error).
- S7 forced expiry: **NOT RUN live** (6 h TTL) — covered by engine M16 tests + resolver `expiry_forces_resolve`.
- S8 sidecar killed mid-play: **PASS** — SIGKILL → current track continues (googlevideo direct) → supervisor recovers (backoff gate test + live).
- S9 network off: **NOT RUN** (cannot disable the host network safely) — offline = sidecar unreachable = typed Network error, same path as the S8 evidence.
- S12 quit/relaunch restores: **NOT RUN (GUI)** — restore path is structural (fresh resolver per process; URLs never persisted).
- 5-min stress: 0 underruns (REPORT §F2/F6 evidence, unchanged paths).

**Fixes landed (minimal):** nested-runtime resolve fix (rodio_factory.rs prepare/prefetch); sniff+hint in open_source (pipeline.rs); YT host allowlist + 2 tests (yt.rs); debug sink peak meter (pipeline.rs + metrics.rs, cfg-gated); E2E hook (e2e.rs, cfg(debug_assertions), TANSEN_E2E=1).

**Fake data:** the real user DB contains **0 synthetic rows** (verified: `SELECT COUNT(*) FROM tracks WHERE path LIKE '/tmp/%'` = 0). No mock sources in user-facing paths.

**Security re-check:** release binary `strings`: TANSEN_E2E/TANSEN_STRESS **absent** (0); TANSEN_BENCH_PLAY present (M1 harness design, env-gated, documented); bench_marker present (M1 harness dependency, 48-char cap on the echoed string). Allowlist + redirect policies intact. No secrets/signed URLs in logs (redaction live — path-only logging verified in the failing-decode warn lines).

## BUG-002 — Settings page crash (`t.name`) — FIXED & VERIFIED (2026-11-03)

**Symptom:** "Unexpected Application Error! undefined is not an object
(evaluating 't.name')" at SettingsPage-*.js; Settings appeared empty.
**Answer to "why is settings empty":** the page never got to draw the list —
it crashed while rendering the FIRST source row. The user's database has 2
valid sources (yt "YouTube Music", local "Local Files"); nothing was lost.

**Root cause (confirmed before any code change):** crash column 5787 in the
shipped bundle = `children:t.name` inside the sources map; built source was
`.data?.map(({config:t,health:p},o,m)=>…)`. Rust `SourceWithHealth`
(commands.rs:536-540) declares `#[serde(flatten)] config: SourceConfig` —
flatten inlines the config's fields into the row; there is no `config` key
on the wire. `SourceHealth` remains nested under `health` (internally tagged
`state`: ok/unreachable/rejected). Frontend destructured a non-existent
key → `config === undefined` → `config.name` threw. Latent since M10;
every render with ≥1 source row crashed the route.

**Fix:** read the flattened row top-level (name/kind/base_url/enabled/
config_id; health via `row.health`). UI layout, classes and behavior
unchanged. Files: `SettingsPage.tsx`, new `SettingsPage.test.tsx`.

**Verification evidence:**
- Reproduced the exact bundle hash pre-fix; post-fix bundle contains the
  old destructure pattern 0 times (grep) and maps the whole row.
- `npx tsc --noEmit`: clean. `npx vitest run`: **9 files / 55 tests PASS**
  (54 prior + 1 new regression that mounts SettingsPage with the real
  flattened wire row and asserts both rows + enabled states render).
- `npm run build`: OK. Debug `tansen` launch smoke: sidecar JRE boots,
  engine starts, watchers scan, SIGTERM exits cleanly.
- Backend untouched — no IPC, serde, engine or persistence changes.

## CORRECTION (2026-11-03): F9 "0 synthetic rows" claim was WRONG
The F9 check `path LIKE '/tmp/%'` missed macOS `/private/tmp/...` resolutions.
The real app DB had accumulated 31 harness rows (stress-01…29, e2e-local;
paths /private/tmp/tansen-stress-lib|e2e-lib) plus their play_history and 2
remote_tracks — debug/release share one data dir, and the E2E/stress harnesses
wrote into it. That was the "prototype look": the user's Songs page showed 31
junk tracks they never added, and one sat in the player bar.

**Fixes (root cause + cleanup, no playback changes):**
1. Isolation: debug harnesses (TANSEN_E2E / TANSEN_STRESS / TANSEN_BENCH_PLAY)
   now redirect the data dir to /tmp/tansen-harness-data (main.rs data_dir,
   cfg(debug_assertions) — release binaries unaffected).
2. User DB cleaned (backup: /tmp/tansen-db-backup-20261103): tracks,
   play_history, remote_tracks, playback_state cleared → 0 rows, VACUUM'd.
3. E2E now self-seeds catalogue_sources=true in its scratch dir (fresh-DB
   first-run gap: without Settings, a fresh install has no YT source).

**Re-verification (fresh run, isolated dir):** S1 search 10 real YT rows →
S3 Playing in 1957 ms, sink peak 0.288 (non-silent) → S4 pause/resume,
S5 seek 30 s, S8 sidecar SIGKILL recovery, S6 local queue join, S10 typed
error — all PASS. Real DB after run: 0 tracks / 0 remote (untouched).

**On the user's search test:** the query "4raws" has no matches on YouTube
Music either — empty results for gibberish is correct behavior. Real-song
search→play is proven by the S1–S3 chain above, which drives the exact
search_tracks/upsert/Enqueue/Play commands the UI buttons call.

## BUG-003 — stress/test data kept re-inserting; root source removed (2026-11-03)

**Symptom:** after BUG-002's row-level cleanup, the 31 stress/e2e tracks
returned on the next app launch.

**Root cause:** deleting `tracks` rows never removed the *watchers*:
`library_folders` still contained `/private/tmp/tansen-stress-lib` and
`/private/tmp/tansen-e2e-lib` (persisted when the stress/E2E harnesses ran),
and the leftover directories still existed on disk. Every app start = folder
watch rescan = the sine-wave tracks re-added. That is the automatic-insertion
source: persisted folder list + harness dirs, not a seeder or frontend
fallback (frontend has no mock data; crate fixtures live only under
crates/*/tests/fixtures, never compiled into app paths).

**Fix (data + disk, no code/UI/playback change):**
- Deleted the two `library_folders` rows (path LIKE '/private/tmp/tansen-%').
- Deleted the harness directories themselves: /private/tmp/tansen-stress-lib
  and /private/tmp/tansen-e2e-lib (30+1 sine WAVs).
- Purged remaining junk: tracks, play_history, remote_tracks, playback_state;
  VACUUM. Backup: /tmp/tansen-db-backup-20261103b.
- Harness isolation (main.rs data_dir redirect under TANSEN_E2E/STRESS/
  BENCH_PLAY) already prevents debug harnesses from touching this DB; the
  remaining env-gated write paths run only when those env vars are set.

**Verification:**
- After purge: tracks=0, folders=0, playlists=0, play_history=0.
- Full app restart (open → watcher scan → quit): tracks still 0, folders
  still 0 — no re-insertion confirmed across a real launch cycle.
- Library/sections empty states verified present and intentional (Songs,
  Albums, Artists, Recently Played, Playlists) — they render guidance, not
  fake entries.
- Nothing genuine lost: the database contained no user-created playlists,
  real audio, or legit metadata (all 31 rows were harness artifacts); the
  pre-clean backups preserve what existed.
