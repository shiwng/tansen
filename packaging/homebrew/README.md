# Homebrew Cask — `tansen`

Target one-command installation:

```bash
brew install --cask tansen
```

`brew search tansen` (verified 2026-11-19) returns no existing cask, so the
name is free. Getting there requires the following sequence, in order:

## Release → Cask sequence

1. **Signed, notarized DMG** — *currently blocked*: `security find-identity -v
   -p codesigning` finds **0 valid Developer ID identities** on the build
   machine. The app is only ad-hoc signed (`TSN-2026-09`). Once an Apple
   Developer ID Application certificate is available:

   ```bash
   export APPLE_CERTIFICATE_SIGNING_IDENTITY="Developer ID Application: <name> (<team-id>)"
   export APPLE_ID="<apple-id-email>"
   export APPLE_PASSWORD="<app-specific-password>"
   export APPLE_TEAM_ID="<team-id>"
   pnpm --dir apps/desktop tauri build
   codesign --verify --deep --strict src-tauri/target/release/bundle/macos/Tansen.app
   spctl --assess --type execute src-tauri/target/release/bundle/macos/Tansen.app   # must say "accepted"
   xcrun stapler validate src-tauri/target/release/bundle/dmg/Tansen_0.3.0_aarch64.dmg
   ```

   Keep the hardened-runtime entitlements in `Tansen.entitlements`: the bundled
   JRE needs `allow-jit` and `allow-unsigned-executable-memory` (R-03; do not
   remove blindly).

2. **GitHub release** — publish `v0.3.0` with the DMG, SHA-256 checksums, and
   release notes; record the sidecar JAR SHA-256 in the notes (TSN-2026-13).

3. **Fill in the cask** — put the release URL and the real `sha256` of the
   DMG into [`tansen.rb`](tansen.rb).

4. **Tap** — create a personal tap repo `shiwng/homebrew-tap` with
   `Casks/tansen.rb`, then:

   ```bash
   brew tap shiwng/tap
   brew install --cask tansen            # via the tap
   brew install --cask shiwng/tap/tansen # explicit form
   ```

5. **homebrew/cask PR (optional, later)** — a top-level `brew install --cask
   tansen` requires a PR to [`Homebrew/homebrew-cask`](https://github.com/Homebrew/homebrew-cask)
   with a real notarized release and audit passing (`brew audit --cask`).
   Do this only after the tap install has been verified on a clean Mac.

## Verification rule

No installation instruction may be published as "working" until it has been
run on a clean macOS install (or VM) and the result recorded. Until step 1
exists, this document and the cask template stay aspirational and the README
says so.
