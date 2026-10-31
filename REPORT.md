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
