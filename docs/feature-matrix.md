# Feature parity matrix

Statuses: ✅ implemented & tested · 🟨 partial (works, scope-bounded) · 📋 planned (phase) · 🚫 blocked (documented reason).

Legend: phase numbers refer to `docs/plan.md`.

## Playback

| Feature | Status | Notes |
|---|---|---|
| Play / pause / stop / next / previous | 🟨 P3 slice | works; previous = history-aware restart, tested in queue model |
| Seek | 🟨 P3 slice | rodio `try_seek`, drag-to-seek in UI |
| Volume | 🟨 P3 slice | engine + UI slider + keyboard |
| Persistent queue, reorder | 🟨 P3 slice | queue snapshot API + panel (add/remove/jump/clear); reorder drag + persistence across restarts planned |
| Gapless playback | 🟨 P3 slice | pre-append ahead of track end, position-drop handoff detection; measured transition validation pending (P6) |
| Crossfade 0–12 s | 📋 P6 | two-sink equal-power mix (BitChord uses two ExoPlayers) |
| Playback speed | 📋 P6 | rodio `set_speed` exists; UI pending |
| Skip silence | 📋 P6 | needs a silence-trimming source wrapper |
| Repeat / shuffle | 🟨 P3 slice | repeat off/all/one wired; shuffle planned |
| Codec/bitrate/sample-rate/bit-depth reporting | 🟨 P3 slice | stored at scan (lofty), shown in library; NerdStats-style panel planned |
| Native media keys (macOS/Windows) | 🟨 P3 slice | souvlaki wired (Now Playing / SMTC); Windows build pending test |
| Background playback recovery / error skip | 🟨 P3 slice | unopenable track → `TrackError` event + auto-advance |

## Library

| Feature | Status | Notes |
|---|---|---|
| Import folders | ✅ P3 slice | dialog picker → canonical path → scan; tested end-to-end on a real WAV |
| Metadata extraction | ✅ P3 slice | lofty: tags, duration, codec, bitrate, sample rate, bit depth, channels |
| Artwork (embedded) | 🟨 P3 slice | extracted to content-hash cache, served via `art://` |
| Search / sort / filter | 🟨 P3 slice | client-side; server-side query when libraries grow |
| Missing-file detection, rescan without duplicates | ✅ P3 slice | tested: second scan adds 0, missing marked not deleted |
| Playlists + history persistence | 🟨 P3 slice | schema + repos + commands exist; playlist UI pending |
| SMB / WebDAV sources | 📋 P4/P7 | BitChord uses SMBJ/PROPFIND; Rust: `pavao`/`reqwest_dav` candidates |

## Discovery & sources

| Feature | Status | Notes |
|---|---|---|
| Source interface (`health`/`search`/`stream`, quality tiers) | 📋 P4 | shape mirrors `MusicSource.kt`; racing resolver plan in audit §3 |
| Addon HTTP sources (manifest/search/stream) | 📋 P4 | plain JSON protocol, cheapest first win |
| QuickJS module sources | 📋 P4 | `rquickjs`; same two-export contract |
| YouTube Music catalogue + auth | 🚫 until PoToken spike | web clients need BotGuard tokens; requires WebView or external minter — spike first, else blocked (documented, not simulated) |
| JioSaavn | 📋 P4 | undocumented public API, same risk class as YTM |
| Fallback/upgrade/substitution rules | 📋 P4 | logic documented; port after ≥2 real sources exist to test against |

## Lyrics

| Feature | Status | Notes |
|---|---|---|
| Provider interface + priority + raced fetch | 📋 P5 | port of `LyricsRepository` selection rules |
| Plain + line-synced LRC (LRCLIB first) | 📋 P5 | open API, no credentials |
| Word-level TTML/QRC parsing | 📋 P5 | parsers are pure logic (GPL-compatible translation) |
| ISRC identification & matching | 📋 P5 | 2.5 s budget, LRU — port as specified |
| Synchronized lyrics UI | 📋 P5 | `LyricClock` algorithm is fully specified + unit-tested upstream |
| Caching, fallback, unavailable states | 📋 P5 | |
| Translation | 🚫 P5+ | depends on Google endpoints; optional, off by default |

## Audio features

| Feature | Status | Notes |
|---|---|---|
| Equalizer (10-band) | 📋 P6 | BitChord: bilinear SVF chain on float blocks — port math, wrap rodio source |
| Loudness normalization | 📋 P6 | requires per-track loudness data (YTM provides; local = ReplayGain scan) |
| Sleep timer | 📋 P6 | trivial once P3 controls exist |
| Audio cache | 📋 P4/P6 | local files need none; network sources get an LRU disk cache |
| Listening statistics | 🟨 P3 slice | play history rows recorded; aggregation UI pending |

## Downloads & integrations

| Feature | Status | Notes |
|---|---|---|
| Downloads + tagging | 📋 P7 | only where source terms permit; BitChord's taggers are portable logic |
| Last.fm / ListenBrainz scrobbling | 📋 P7 | documented protocols (MD5-signed posts / token API) |
| Discord Rich Presence | 📋 P7 | user-token gateway; no official API — document risk, gate behind opt-in |
| Listen Together | 📋 P7 | protocol + Go reference server exist and are documented |
| Spotify import | 📋 P7 | embed-page scrape + matching; private GraphQL needs user session |
| Chromecast | 🚫 | no desktop-cast SDK equivalent; reassess only with a concrete use case |

## Platform

| Feature | Status | Notes |
|---|---|---|
| macOS build + launch | ✅ P2/P3 | dev build verified on arm64; packaging (dmg) in P8 |
| Windows build | 📋 P8 | cross-compile check pending; SMTC via souvlaki configured |
| Installers / updates | 📋 P8 | tauri bundler + updater plugin |
