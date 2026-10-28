# Tansen — Work Report

Status as of **2026-10-28**, HEAD `e79d57d`. Companion documents:
`IMPLEMENTATION.md` (how everything is built and connected),
`docs/feature-matrix.md` (per-feature status), `docs/plan.md` (milestones).

---

## 1. Timeline of work

| Date | Commit | What landed |
|---|---|---|
| Session 1 (2026-10-27) | `685b426` | Audit of upstream BitChord (repo + APK v1.8.1, same version — full report in `docs/audit-bitchord.md`). Phase-2+3 vertical slice: Tauri shell, navigation, SQLite (WAL + migrations), folder import → scan (lofty metadata, artwork cache), rodio playback engine, pure queue model, media keys. |
| Session 2 | `0c93870` | UI re-based on BitChord's own GPL **web player** (tokens, glass shell, TrackList, FullScreenPlayer, Up Next). Player store mirroring the upstream contract over Tauri. **Lyrics system vendored**: 13 providers, TTML/LRC/QRC parsers, LyricClock — 26 upstream tests pass. Rust: queue persistence (`playback_state`), playlist rename/delete, albums/artists/history views, queue Move. M2 + UI parity. |
| Session 3 | `50421d7` | M2 closed: Add-to-Playlist picker, drag reorder page, library watcher (notify + `watch_folders`), restart-persistence tests, artwork page tint. |
| Session 4 | `7518803` | M3 start: `sources` crate — `MusicSource` trait with upstream tier semantics, addon HTTP adapter (SharedCalls dedup, 429 quiet period), SQLite registry, Settings → Sources screen; mock-server integration tests. |
| Session 5 | `7f70fdc` | **YouTube Music via Kotlin sidecar** (upstream checkout pinned at v1.8.1 + additive `sidecar/` module; nothing in `:shared` changed). `YtMusicSource` supervises the JVM. Round-trip proof: search → AAC stream → 5 s decode, RMS 0.126. |
| Session 6 | `60085ba`, `51823a3` | Renamed to **Tansen** (product, bundle id, crate/npm names, data-dir migration `bitchord-desktop` → `tansen`, original app mark replacing BitChord's logo). Project moved to `~/Desktop/Tansen` with `~/Desktop/BitChord` as sibling. |
| Session 7 | `e79d57d` | **Remote streaming playback**: `HttpRangeReader` (512 KiB ranged chunks — googlevideo throttles unbounded reads), engine queue entries dispatch by scheme, resolve-ahead thread swaps placeholder URLs before the gapless preload, YouTube Music tab in Search, play/queue on remote rows, catalogue toggle in Settings. Round-trip now decodes through the ranged-HTTP engine path in **4.5 s** (was 142 s downloading). |

Test totals: **24 Rust tests + 32 vitest tests**, all green; `tsc` strict +
production bundle clean; app smoke-launched after every session.

---

## 2. What works (verified)

| Area | Status | Evidence |
|---|---|---|
| Library import/scan/rescan | ✅ | End-to-end test on a real WAV; rescan adds 0 duplicates; missing files marked, not deleted; watcher integration test |
| Metadata + artwork | ✅ | Tags, codec/bitrate/sample-rate/bit-depth stored; content-hash artwork cache; `art://` protocol |
| Local playback | ✅ | Play/pause/seek/volume/next/prev/repeat; queue ops incl. drag-move; gapless pre-append advance; errors surface and auto-advance |
| Queue persistence | ✅ | Survives restart paused at saved position; dead files dropped; remote entries kept (integration test) |
| Playlists | ✅ | Create/rename/delete/play/append; drag reorder page; Add-to-Playlist from any track menu |
| History + Recently Played | ✅ | One row per local track start |
| Albums/Artists views + page tint | ✅ | SQL groupings with album_artist fallback; pages tint from artwork |
| Search (library) | ✅ | Albums/artists shelves + songs |
| UI design parity | ✅ | Vendored upstream web-player design (glass sidebar, player pill/LCD, full-screen player, Up Next), tokens verbatim, DM Sans self-hosted |
| Lyrics | ✅ data layer | 13 providers raced in priority (word > line > plain), ISRC identify, TTML/LRC/QRC parsers, provider settings — 26 upstream tests pass against the vendored code |
| YouTube Music (catalogue) | ✅ behind flag | Sidecar supervises upstream's Kotlin client; search → `prefer=aac` progressive stream → engine decodes through the ranged reader, RMS 0.126, **4.5 s end-to-end**; catalogue flag off by default |
| Streaming playback | ✅ | 512 KiB ranged reader; MP4 moov seeks over the wire; remote rows in queue/Up Next; resolve-ahead keeps URLs fresh |
| OS media keys (macOS) | ✅ | Now Playing metadata + 2 Hz position; play/pause/seek from keys |
| Settings | ✅ | Every visible control wired or explicitly labeled "not yet implemented" |
| Watch folders | ✅ | Auto-rescan, 2 s debounce, initial scan, toggle honored |
| Data migration | ✅ | `bitchord-desktop` → `tansen` verified live on this machine |

---

## 3. What does not work yet (planned, not faked)

| Area | Status | Notes |
|---|---|---|
| Crossfade / Automix | 📋 | Preference stored; two-sink equal-power fade + upstream's transition planning land in phase 6 |
| Equalizer / loudness / skip silence | 📋 | Phase 6 DSP chain |
| Playback speed, sleep timer | 📋 | Phase 6 |
| QuickJS addon modules | 📋 | Addon HTTP adapter done; module runtime via `rquickjs` next |
| Racing source resolver (lossless upgrades, cross-source matching) | 📋 | Documented in audit; ports once a second real source exists to race |
| Playlist reorder from Up Next only | 🟨 | In-playlist drag exists as its own page; inline reorder pending |
| Playlist persistence of queue | 🟨 | Queue persists; playlist add-from-queue pending |
| Word-level lyric timings over the sidecar | 🟨 | Upstream model has them; sidecar exports line-level only — add when the desktop lyrics UI needs them |
| In-queue row artwork for remote tracks | 🟨 | https thumbnails pass through; queue snapshot rows carry them (verify visual pass pending) |
| Dynamic page tint on search/home | 🟨 | Album/playlist pages tinted; others pending |
| Artwork orphan GC | 📋 | `prune_orphans` exists; not scheduled yet |
| Scan watcher on Windows | 📋 | notify works cross-platform; untested |

## 4. What is blocked (with reasons — never simulated)

| Area | Reason |
|---|---|
| HLS / DASH streaming | Upstream phone handles manifests via media3; rodio/Symphonia decode progressive files only. Sidecar serves progressive itags; manifest support needs a demuxer change. |
| Word-synced lyric **providers needing Innertube** (YouTube captions) | No Innertube client outside the sidecar yet; the sidecar's lyric route uses the provider race but YouTube-caption sources fail there the same way. |
| Signed-in YouTube features (home, likes, playlists CRUD, history) | Sidecar is anonymous-only by design; cookie plumbing is a future decision. |
| PoToken/BotGuard guarantee | Today's client path resolves without a token (observed, re-tested); no WebView minter exists on desktop. If Google flips the switch, streaming breaks until a minter is built — the round-trip test is the canary. |
| Favorites / like status | No rating data model yet; the button is absent rather than fake. |
| Chromecast | No desktop Cast SDK equivalent; upstream feature is Android/GMS-bound. |
| Discord RPC, Last.fm/ListenBrainz, Listen Together, downloads | Designed (protocols documented in the audit), implementation scheduled phase 7. |
| Windows build | **Untested** — no Windows host in this environment. Code paths are cross-platform (souvlaki/SMTC, `%APPDATA%` data dir); needs a CI or manual pass, plus a Java 17 runtime note for the sidecar. |
| macOS signing/notarization | Placeholder icons and unsigned dev builds; release packaging is phase 8. |

## 5. Known gaps / debt worth naming

1. **Manual-next race on remote rows** — a next pressed before resolve-ahead
   finishes skips the track cleanly (error path) instead of waiting. Widen the
   ahead window or add fetch-to-cache.
2. **First-play latency** — start track resolve is 1–2 s; a fetch-to-cache
   layer doubles as the fix for (1).
3. **Library search is client-side** — fine at thousands of tracks; move to SQL
   FTS when libraries grow.
4. **SF Pro Display lyrics font** — Apple-proprietary face shipped by upstream
   without its own license; kept for parity, flagged for distribution review.
5. **Windows verification** — highest-priority platform debt; tracked in plan M7.
6. **Single sidecar handle per process** — one JVM for the whole app; a crashed
   sidecar is re-launched on next use, but there is no watchdog/health UI yet.

## 6. Verification summary

- `cargo test` — 24 tests, 0 failures (queue algebra, range reader + ranged decode, scanner e2e, groupings, persistence round-trips, watcher, addon mock server, registry).
- `cargo test -- --ignored` (network) — sidecar round-trip: search → `prefer=aac` → 5 s decode via ranged HTTP, RMS 0.126, 4.5 s total.
- `pnpm test` — 32 tests (lyric parsers, LyricClock, repository selection, reorderMove).
- `pnpm build` — tsc strict + Vite production bundle.
- Manual: app launch + data-dir migration logs; UI structure verified via REA browser DOM capture (2026-10-28).
