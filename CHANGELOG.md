# Changelog

All notable changes to Tansen are documented in this file.
Format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/).

## [0.3.1] - 2026-11-26

Security fixes from the review of the 0.3.0 build. No feature changes.

### Installation
- Download `Tansen_0.3.1_aarch64.dmg` and `SHA256SUMS.txt` from [GitHub Releases](https://github.com/shiwng/tansen/releases/tag/v0.3.1). The DMG is built for **macOS 11+ on Apple Silicon (arm64)**.
- Verify the disk image with `shasum -a 256 -c SHA256SUMS.txt`, then drag Tansen to `Applications`.
- The build is still unsigned and unnotarized. On first launch use **System Settings, Privacy and Security, Open Anyway**, or run `xattr -cr /Applications/Tansen.app` once before opening.

### Security
- **Outbound fetches refuse the ranges that are never public.** `netguard` classified only RFC 1918, loopback, link local and IPv6 unique local addresses. Carrier grade NAT (`100.64.0.0/10`), the IANA protocol, test net and benchmarking blocks, multicast and the reserved block are now local too, and the IPv4 address carried inside NAT64 (`64:ff9b::/96`) and 6to4 (`2002::/16`) addresses is judged by its own range. Every client that asks `netguard` inherits the fix, so a source URL, a stream read or an artwork fetch cannot pivot into the host network through an IPv6 transition address.
- **Radio station artwork connects to the address it resolved.** The host name was checked for "local" and then resolved a second time by the HTTP client, so a publisher's name that resolves to a private address once passed the check. The connection is now pinned to the validated address, and redirects are refused rather than followed to a second, unpinned host.
- **A login startup folder is refused as a downloads destination.** A download folder pointed at `LaunchAgents`, `LaunchDaemons`, `etc/cron.*` or the XDG and Windows autostart folders would place a downloaded file where the session runs it without the user opening it. A destination that turned into a symlink is refused as well.
- **The sidecar jar and the JVM are only started when another account cannot rewrite them.** Both are executed as the user, and their paths come from settings, so a file that is a symlink, not a regular file, or writable by group or other is refused with the path in the error.

## [0.3.0] - 2026-11-21

First public release. Local-first music player for macOS with optional
catalogue streaming. (The 0.3.0 release-candidate line was dated 2026-11-03;
the public release adds the hardening and community work below.)

### Installation
- Download `Tansen_0.3.0_aarch64.dmg` and `SHA256SUMS.txt` from [GitHub Releases](https://github.com/shiwng/tansen/releases/tag/v0.3.0). The DMG is built for **macOS 11+ on Apple Silicon (arm64)**.
- Verify the disk image with `shasum -a 256 -c SHA256SUMS.txt`, then drag Tansen to `Applications`.
- The v0.3.0 build is **unsigned and unnotarized** (no Apple Developer ID signature and no notarization ticket). On first launch, open **System Settings → Privacy & Security → Open Anyway**, or run `xattr -cr /Applications/Tansen.app` once before opening the app.

### Player
- Task-based playback engine (single writer, bounded command channel, event-driven state), gapless local playback via `rodio`/`Symphonia`.
- SPSC decode ring + prefetch pipeline; end-of-track auto-advance; seek/pause/resume; URL-expiry single re-resolve; skip-on-error so one broken entry cannot stall the queue.
- Stream cache (512 MiB bound), resolve-ahead prefetch, fallback ladder across sources with track matching.
- DSP EQ, crossfade, loudness normalization, skip-silence; macOS Now Playing and media-key integration.
- Full-screen player with synced (word-timed) lyrics, mesh backdrop, Up Next queue management; LRC/TTML/QRC parsing.
- Offline downloads with a persistent queue; quality ladder (low/medium/high/lossless).
- Now Playing (macOS system media integration), device change and sleep/wake recovery.

### Sources
- Local library: parallel scanner (stat-skip + batched transactions), unified search, prefer-local resolution for Lossless/Best tiers.
- YouTube Music via a local Kotlin sidecar (anonymous access only; sidecar JAR + bundled 30 MiB `jlink` Java 17 runtime).
- Addon servers (manifest/search/stream contract) and Subsonic/OpenSubsonic (credentials in macOS Keychain, token auth on the wire).
- Internet Radio browsing via Radio Browser with public-host DNS validation.
- Discovery typeahead, browse categories, and optional autoplay.
- Google Sign-In (OAuth 2.0 + PKCE, Keychain-only tokens) and public Spotify playlist import matched to YouTube Music.

### Added
- First downloadable macOS Apple Silicon DMG (`Tansen_0.3.0_aarch64.dmg`), containing the unsigned, unnotarized app plus the pinned sidecar JAR and bundled Java 17 runtime; requires macOS 11+ (`TSN-2026-09`).
- Cross-platform preparation: macOS-only `objc2`/`block2` crates gated under `[target.'cfg(target_os = "macos")'.dependencies]`, Windows window override (`apps/desktop/src-tauri/tauri.windows.conf.json`), cross-platform OAuth browser launch, and Windows headless `java.exe` spawning.
- Open-source community and CI files: `LICENSE` (GPL-3.0-or-later), `SECURITY.md`, `CODE_OF_CONDUCT.md`, `CONTRIBUTING.md`, GitHub issue/PR templates, and expanded CI checks (`rustfmt`, `clippy`, `cargo audit`, `cargo deny`, `pnpm audit`).

### Security
- Enforced indexed local-library containment and `local_files_enabled` gating in `downloads_add` / `copy_local` (`TSN-2026-17`), plus a 1 GiB per-track remote download size cap (`TSN-2026-18`).
- Expanded `.gitignore` with broad `.env`, signing-key, and local SQLite exclusions (`TSN-2026-19`).
- Upgraded `rusqlite` to `0.41` (`libsqlite3-sys 0.39.1`), resolving `RUSTSEC-2026-0459` / `CVE-2026-119375` (`TSN-2026-08`).
- Switched sidecar `X-Sidecar-Token` verification to constant-time `MessageDigest.isEqual` (`TSN-2026-10`) and bounded all sidecar JSON reads with `json_limited`.
- Pinned all GitHub Actions in `.github/workflows/ci.yml` to immutable commit SHAs and added hardcoded SHA-256 verification for JitPack artifacts in `scripts/build-sidecar.sh` (`TSN-2026-15`).
- Added per-hop redirect validation, DNS resolve-validate-pin for downloads/media/cache/radio, 8 MiB response body caps (`bounded_body.rs`), HTTPS downgrade refusal for addons, and URL-redacted transport errors (`TSN-2026-01` through `TSN-2026-07`).
- Removed the embedded Musixmatch fallback signing secret from the lyrics provider; the secret is read from Musixmatch's own public web bundle at request time and the provider degrades gracefully when unavailable (`TSN-2026-16`).

### Performance
- Switched track resolution in `main.rs` to point lookups (`repo::get_track`) and duration-window queries (`repo::tracks_in_duration_window`).
- Preloaded file stat maps once before parallel Rayon scanning in `crates/library/src/scanner.rs`, eliminating per-file SQLite mutex contention.

### Known Limitations
- Opus/WebM decode and HLS/DASH manifests unsupported (progressive streams and AAC supply used instead).
- Quality setting applies to newly resolving tracks; playback speed and sleep timer not implemented.
- The published artifact is Apple Silicon-only (arm64); Intel, Windows, and Linux builds are untested.
- No auto-updater; Apple Developer ID signing and notarization not yet configured. The app in the v0.3.0 DMG is unsigned and unnotarized (`TSN-2026-09`).
