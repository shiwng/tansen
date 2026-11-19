# Contributing to Tansen

Thank you for your interest in contributing to Tansen. Tansen is a local-first desktop music player built with Tauri 2, Rust, and React 19.

## Before You Start

- Read [docs/architecture.md](docs/architecture.md) to understand the crate boundaries and runtime invariants.
- Read [docs/feature-matrix.md](docs/feature-matrix.md) to check what is implemented, in progress, or intentionally out of scope.
- Check existing issues and pull requests before opening a new one.

## Architectural Invariants

All changes must preserve these core design rules:

1. **Single playback writer**: `crates/app-core` owns playback state and the queue. The frontend sends `Command`s and renders `playback_state` events.
2. **Real-time safe audio callback**: The `cpal`/`rodio` audio callback in `crates/playback` must never block, allocate, lock a mutex, or perform I/O. Audio bytes stay in Rust and never cross IPC.
3. **No Tauri dependency in core crates**: `crates/app-core`, `crates/playback`, `crates/library`, `crates/persistence`, `crates/sources`, and `crates/netguard` remain independent of Tauri.
4. **Security boundaries**:
   - Never log or return raw credential-bearing URLs (use `without_url()` on `reqwest` transport errors and redacted URLs in logs).
   - Store credentials only in the OS Keychain, never in SQLite, logs, events, or frontend state.
   - Validate all outbound URLs and redirects through `crates/netguard` and bound HTTP response bodies with `bounded_body.rs` or `json_limited`.
   - Keep YouTube Music access anonymous through the local loopback sidecar (`127.0.0.1` + per-launch `X-Sidecar-Token`).

## Local Development Setup

Prerequisites (macOS is the primary verified platform):

- Rust 1.90+ (`rustfmt` and `clippy` components)
- Node.js 22+ and `pnpm` 10+
- JDK 17 (only required if building the YouTube Music sidecar or packaging the bundle)

```bash
# Build the YouTube Music sidecar JAR + bundled JRE (required before packaging)
./scripts/build-sidecar.sh

# Install frontend dependencies
pnpm install --dir apps/desktop

# Run the desktop app in dev mode
cd apps/desktop && pnpm tauri dev
```

## Running Checks Before Opening a PR

```bash
# Rust formatting, linting, and tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets
cargo test --workspace

# Rust dependency and license checks
cargo audit
cargo deny check advisories licenses

# Frontend tests, type-checking, build, and audit
pnpm --dir apps/desktop test
pnpm --dir apps/desktop build
pnpm --dir apps/desktop audit --audit-level=high
```

## Pull Request Process

1. Fork the repository and create a focused branch from `main`.
2. Keep changes minimal and scoped to a single bug fix, feature, or documentation improvement.
3. Add or update unit/regression tests for any behavioral or security change.
4. Never commit `.env` files, `apps/desktop/src-tauri/google-oauth.env`, signing keys, or personal user data.
5. Fill out the pull request template with clear verification steps.

## Good First Issues

Looking for a place to start? Areas well suited for first-time contributors include:

- **Audio & DSP tests** (`crates/playback/src/dsp.rs`, `crates/playback/src/loudness.rs`): adding edge-case unit tests for EQ presets, loudness normalization, and crossfade curves.
- **Queue index handling** (`crates/app-core/src/playback_engine.rs`): improving queue remove/move operations when the same track appears multiple times in the queue.
- **Lyrics parser coverage** (`apps/desktop/src/lyrics/parse.ts`): adding LRC/TTML/QRC parsing edge-case fixtures in Vitest.
- **Cross-platform groundwork** (`crates/sources/src/keychain.rs`, `apps/desktop/src-tauri/tauri.windows.conf.json`): adding a Windows Credential Manager backend or validating Windows CI builds.
- **Documentation improvements** (`docs/`): clarifying setup steps, keyboard shortcuts, or source configuration guides.

## Security Disclosures

Please do not open public GitHub issues for suspected security vulnerabilities. Review [docs/security/SECURITY_AUDIT.md](docs/security/SECURITY_AUDIT.md) and report vulnerabilities privately through GitHub Security Advisories once enabled, or contact the maintainer directly.

## Licensing

Contributions must be compatible with the project's license (`GPL-3.0-or-later`; see the repository license notice).
