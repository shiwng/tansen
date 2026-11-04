# Changelog

## 0.3.0 — 2026-11-03

First release-candidate line: local-first music player with optional
catalogue streaming.

### Player
- Task-based playback engine (single writer, bounded command channel,
  event-driven state), gapless local playback via rodio/symphonia.
- SPSC decode ring + prefetch pipeline; end-of-track auto-advance;
  seek/pause/resume; URL-expiry single re-resolve; skip-on-error so one
  broken entry cannot stall the queue (BUG-001 hardening).
- Stream cache (512 MiB bound), resolve-ahead prefetch, fallback ladder
  across sources with track matching (normalized title/artist, marker
  words, duration window).
- Now Playing (macOS system media integration), device change and
  sleep/wake recovery.

### Sources
- Local library: scanner (stat-skip + batched transactions), unified
  search, prefer-local resolution for Lossless/Best tiers.
- YouTube Music via a local Kotlin sidecar (anonymous access only; the
  sidecar jar ships with the app). AAC supply by default.
- Addon servers (manifest/search/stream contract) and Subsonic/
  OpenSubsonic (credentials in the macOS Keychain, token auth on the
  wire).
- Discovery typeahead; autoplay adds related tracks at queue end
  (off by default).

### Security
- Per-client redirect policies with host allowlists for lyrics/artwork
  proxies; same-host redirects for source API clients; stream URLs are
  never logged (redacted to scheme://host/path); credentials only in the
  macOS Keychain; no secrets in SQLite, logs, events or frontend state.

### Known limitations
- YouTube Music runs on a bundled 30 MiB jlink Java 17 runtime — no
  system Java required.
- Opus/WebM decode unsupported (symphonia 0.5/0.6 ship no Opus codec);
  AAC supply is used instead.
- HLS/DASH manifests unsupported (progressive streams only).
- Quality setting applies to newly resolving tracks.
- No auto-updater; notarization/signing not yet configured.
