# Tansen Security Audit

| Field | Value |
|---|---|
| Audit ID | `TANSEN-SEC-2026-11` |
| Date | 2026-11-13 |
| Target | Tansen desktop music player (Tauri 2 + Rust + React) |
| Repository | Tansen local worktree (absolute path redacted) |
| Branch | `ui-replica` |
| HEAD | `b250adef5e47423081b411dd038b494c4ff82f18` |
| Worktree | Audit snapshot: 85 modified, 62 untracked, 6 deleted tracked files. Re-verified 2026-11-19 in a larger uncommitted worktree (116 modified, 55 untracked, 54 deleted, 2 renamed entries). |
| Reports | [`SECURITY_FINDINGS.md`](./SECURITY_FINDINGS.md) |
| Audit type | Source-assisted, coverage-led application security audit with local runtime verification and regression remediation |

> **Bottom line.** The application has a sound local-first security architecture
> and the high-risk network paths were materially hardened during this audit. One
> conditional High-severity redirect/SSRF finding was remediated and
> regression-tested; the remaining High-impact concern is the unsigned,
> unnotarized macOS distribution. The former SQLite advisory is fixed and the
> supply-chain inputs are digest-pinned; a small set of documented residual
> risks remains. This report does **not** claim that Tansen is "100% secure" or
> that it has zero vulnerabilities.

> **Revision 2026-11-17.** The audit below was performed on 2026-11-13 against a
> worktree whose `commands.rs` and `repo.rs` were monoliths. This revision
> re-verified the findings against the current tree: the SQLite advisory is
> fixed (`rusqlite 0.41`, `libsqlite3-sys 0.39.1`), `cargo audit` reports 0
> vulnerabilities, `cargo deny check licenses` passes, CI actions are pinned to
> commit SHAs, the JitPack sidecar artifacts are SHA-256-pinned, the sidecar
> token comparison is constant-time, and the frontend suite passes 143/143.
> Verdicts and matrices below have been updated; local scratch paths are gone.
> The only open finding left is the ad-hoc-signed, unnotarized bundle.

> **Revision 2026-11-19 (pre-publication re-run).** `cargo clippy --workspace
> --all-targets`, `cargo test --workspace` (262 passed, 0 failed, 9 ignored),
> `cargo audit` (0 vulnerabilities; 4 informational warnings), `cargo deny check
> advisories licenses`, the frontend suite (143/143), the production build, and
> `pnpm audit --audit-level=high` all pass. `cargo fmt --all -- --check` initially
> failed on pre-existing drift (71 files, 920 hunks); a formatting-only
> `cargo fmt --all` was applied, the gate now passes, and clippy plus the full
> suite were re-run afterwards. TSN-2026-17/18 regression tests were run
> explicitly and pass.

---

## 1. Executive summary

Tansen is a local-first desktop music player. Playback state and audio decoding
live in Rust; the React webview is a presentation and control surface; Google
authentication is optional and uses Authorization Code + PKCE with a loopback
redirect. The audit reviewed the current worktree rather than only `HEAD`
because most security-relevant code is untracked or modified.

**What was confirmed and fixed**

- A download redirect chain could pivot a user-initiated download into loopback
  or private services, downgrade `https` to `http`, and carry source headers
  across hosts. Fixed with a manual, per-hop validated chain, DNS
  resolve-validate-pin, host-scoped headers, and redacted errors
  (`TSN-2026-01`, High).
- Media readers and the stream cache trusted the initial hostname and redirect
  targets without DNS validation. Fixed with initial-host pinning and a shared
  redirect verdict (`TSN-2026-02`, Medium).
- Subsonic credentials, signed googlevideo URLs, and provider keys leaked
  through `reqwest` transport errors and logs. Fixed with `without_url()` at
  every converted error and redacted log lines (`TSN-2026-03`, Medium).
- Whole-body consumers (lyrics proxy, Spotify pages, radio JSON/artwork,
  sidecar discovery) had no response-size cap. Fixed with an 8 MiB
  Content-Length + chunk-accumulation cap (`TSN-2026-04`, Medium).
- An `https` addon could redirect its media stream to plaintext `http` while
  carrying an authorization header. Fixed by refusing the downgrade
  (`TSN-2026-05`, Medium).
- Radio station hostnames reached the engine without DNS validation. Fixed by
  resolving the host before the engine opens the stream (`TSN-2026-06`,
  Medium).
- CI used default token permissions and persisted checkout credentials. Fixed
  with read-only permissions and `persist-credentials: false`
  (`TSN-2026-07`, Low).

**What remains**

- The release bundle is ad-hoc/linker-signed, has no TeamIdentifier or sealed
  resources, and `spctl` rejects it (`TSN-2026-09`, Medium, open). Developer ID
  signing and notarization require owner credentials/approval.
- ~~`libsqlite3-sys 0.30.1` is affected by RUSTSEC-2026-0459.~~ Fixed on
  2026-11-17: `rusqlite 0.41` resolves `libsqlite3-sys 0.39.1`, and `cargo
  audit` reports 0 vulnerabilities (`TSN-2026-08`, remediated).
- ~~GitHub Actions and JitPack coordinates are mutable tags.~~ Fixed: every
  action is pinned to a full commit SHA, and `scripts/build-sidecar.sh`
  verifies hardcoded SHA-256 digests for the JitPack jar and pom before use
  (`TSN-2026-15`, remediated).
- The sidecar JAR is not hash-verified at runtime (`TSN-2026-13`, Low,
  accepted); the JVM entitlements widen the hardened-runtime surface (R-03).
- Media-reader redirect hops still have a DNS check-then-connect race (R-01);
  Google's trusted-zone bodies are not capped (R-02); sidecar reads are.
- The frontend persists a user-supplied PaxSenix key in `localStorage`
  (`TSN-2026-14`, Low, accepted) and ships a Musixmatch fallback secret
  (`TSN-2026-16`, Low, accepted). Neither is a Tansen or Google credential.

**Verification (re-run 2026-11-19).** `cargo test --workspace`: **262 passed,
0 failed, 9 ignored**; `cargo clippy --workspace --all-targets` passes; `pnpm
build` (including `tsc --noEmit`) passes; frontend tests are **143 passed of
143** across 30 files. `cargo audit` reports **0 vulnerabilities** and four
informational warnings; `cargo deny check advisories licenses` passes. `pnpm
audit --audit-level=high` reports 0 known vulnerabilities. A custom secret scan of 329
product and report files, `dist`, the packaged app, and 32 commits found no
source or history leak of the Google client secret; the single binary hit is the
expected public desktop-client secret.

**Posture.** The security architecture is coherent and the sensitive paths are
defended in depth. The product is **adequate for a development/local-first
build**, but **not yet release-grade** because of the unsigned distribution and
the residual network/entitlement risks. None of those is a reason to describe
the app as "secure" without qualification.

---

## 2. Scope, method, and constraints

### 2.1 In scope

- All tracked files and all relevant non-ignored untracked files in the current
  worktree: Rust backend and workspace crates, React/TypeScript frontend,
  Tauri configuration, capabilities and entitlements, SQLite schema and
  migrations, build scripts, CI, lockfiles, documentation claims, tests, icons,
  and bundled resources.
- The security-relevant behavior reachable from the running macOS app: IPC
  commands, custom `art:` protocol, OAuth loopback listener, sidecar HTTP,
  outbound provider requests, local library scanning, downloads, and Keychain.
- The machine-readable ledger records 300 entries: 21 full-file reviews, 224
  targeted reviews, 28 scanned files, 21 binary/generated artifacts, and 6
  deleted tracked files. Category A–L are covered; N is the extra artifact
  category.

### 2.2 Out of scope

- Attacks against Google, YouTube Music, Subsonic servers, Radio Browser,
  Musixmatch, PaxSenix, Spotify, or any external infrastructure. No external
  target was touched.
- OS/kernel/root compromise, physical access, malicious signed applications,
  and supply-chain compromise of Apple/Google infrastructure.
- DRM, licensing, and terms-of-service questions.
- Line-by-line decompilation of the bundled JRE or the 18 MB sidecar JAR. Their
  provenance, configuration, and hashes were assessed; only the Kotlin overlay
  was reviewed as source.
- Windows runtime behavior. Shared Rust/TypeScript code was reviewed, but the
  built and exercised artifact is macOS.
- A full penetration test of the Google OAuth consent screen or provider APIs.

### 2.3 Method (Cloudflare six-phase workflow, adapted)

1. **Reconnaissance.** Built the file inventory, read the architecture and
   configuration, mapped trust boundaries, and captured prior audit evidence
   (the dated remediation log and work log that existed at audit time; both
   have since been removed from the repository).
2. **Coverage-led hunting.** Assigned each file to a category (A–L, plus N for
   artifacts), recorded the review level, and hunted by sink class: IPC, SQL,
   filesystem, process execution, network/redirects, secrets, auth, frontend
   sinks, platform, dependencies, CI, and tests.
3. **Candidate validation.** For each candidate: source-to-sink trace,
   preconditions, attacker capability, impact, and a way to disprove it.
   Speculation was rejected; defense-in-depth gaps were recorded as hardening,
   not vulnerabilities.
4. **Structured output.** Confirmed, accepted, open, and rejected records are in
   [`SECURITY_FINDINGS.md`](./SECURITY_FINDINGS.md).
5. **Independent verification.** Regression tests, the full Rust/frontend
   suites, builds, package audits, a custom secret scan, runtime Google
   sign-in/refresh/sign-out, and macOS bundle inspection.
6. **Reporting.** This report, the findings register, and the coverage manifest.

### 2.4 Constraints and authorization

- Only local/adversarial testing was performed. No production account, real
  user file, or external service was attacked.
- The running Tansen app was reused. No duplicate instance, preview window, or
  prototype was created. The app was closed before final report assembly; no
  restart was performed without approval.
- No `/Applications` install, bundle replacement, file deletion, credential
  rotation, macOS setting change, commit, push, tag, release, or publication was
  performed.
- The worktree was already dirty. The audit changed security-relevant files in
  place and did not revert unrelated user work.
- Builds and tests ran on the host, not in an OS-enforced sandbox. This bounds
  the exploitability claims (see §13).

---

## 3. System and trust architecture

### 3.1 Components

| Layer | Components | Security role |
|---|---|---|
| Presentation | React 19, React Router, Zustand, TanStack Query, vendored lyrics components | Renders UI and issues IPC commands. Treated as a constrained but not fully trusted surface. CSP and a minimal capability set limit what it can do. |
| IPC | 82 `#[command]`-annotated handlers across `commands/`, `downloads.rs`, `radio.rs`, `google_auth_commands.rs` | The only path from webview to native capabilities. Inputs are validated at the boundary; state lives in Rust. |
| Core | `app-core`, `library`, `persistence`, `playback`, `sources`, `netguard` | Domain logic, queue/engine state, SQLite access, source adapters, shared host/redirect policy. |
| Audio | rodio/cpal pipeline, SPSC ring, DSP, loudness | Decodes and plays audio in Rust. The real-time callback does not block, allocate, lock, or perform I/O; audio bytes never cross IPC. |
| Sidecar | Kotlin `YtMusic` helper + bundled JRE | Resolves YouTube Music search/stream metadata. Binds `127.0.0.1`, requires a per-launch token, and is anonymous (no account/cookies). |
| Storage | SQLite WAL database, art cache, download folder, `localStorage` | Local library, history, queue restore, artwork, downloads, preferences. |
| Secrets | macOS Keychain (`app.tansen.desktop` domains) | Google tokens and Subsonic credentials. |
| Network | `reqwest` clients, `netguard`, redirect verdicts, bounded bodies | Provider access, SSRF/DNS-rebinding checks, body caps, error redaction. |
| Distribution | Tauri bundle, entitlements, CI, sidecar build script | macOS app packaging, JVM entitlements, reproducible sidecar provenance. |

### 3.2 Primary data flows

```
WebView --invoke--> Rust command --validate--> source/resolver
                                        |--> local SQLite / library scan
                                        |--> Subsonic / addon / radio / lyrics provider
                                        |--> bundled sidecar (127.0.0.1 + token)
                                        '--> playback engine --> audio device
                                              (audio bytes stay in Rust)

Browser --OAuth consent--> 127.0.0.1 loopback listener
        --> PKCE code exchange --> JWKS/claims validation
        --> macOS Keychain (tokens) + SQLite (profile only)

Provider --> Rust artwork fetcher --> art cache --> art:// protocol --> WebView
```

The webview receives metadata, state, and artwork, never audio bytes. The Rust
engine is the source of truth for playback state; the frontend mirrors it and
sends commands. This preserves the project invariant that the engine owns
playback and the audio callback is real-time safe.

### 3.3 Trust boundaries

| # | Boundary | What crosses | Controls |
|---|---|---|---|
| B1 | WebView ↔ Rust IPC | Commands, settings, search queries, file paths, URLs | Tauri capabilities (no shell/fs/http plugin), CSP, command input validation, no ambient authority in the webview, audio isolation. |
| B2 | Rust ↔ macOS Keychain | Google tokens, Subsonic password | OS Keychain ACL; separate service names; no token serialization to the frontend; sign-out deletes. |
| B3 | Rust ↔ sidecar | Search/stream metadata over loopback HTTP | `127.0.0.1` bind, per-launch 128-bit token, no account/cookies, body caps on discovery. |
| B4 | Rust ↔ external providers | URLs, queries, media, metadata, artwork | HTTPS defaults, host allowlists, redirect verdicts, DNS resolve/pin, body caps, error redaction. |
| B5 | Rust/OS ↔ local files | Library paths, art cache, downloads, SQLite/WAL | User-selected folders, canonicalized art root, sanitized download names, app-owned DB, same-user file permissions. |
| B6 | Build/CI ↔ shipped artifact | Rust crates, npm packages, actions, JitPack jars, JRE, sidecar | Lockfiles, pinned sidecar commit, read-only CI token, hash recording; **no Developer ID signing/notarization yet**. |
| B7 | App ↔ OS security model | Entitlements, Gatekeeper, Keychain | JVM entitlements, hardened runtime (not yet complete), ad-hoc signature. |

### 3.4 Architecture assessment

- **Strong separation.** The audio engine, network, and secrets do not live in
  the webview. The frontend can request actions but cannot read the Keychain,
  open arbitrary files, spawn processes, or receive audio buffers.
- **Narrow but broad IPC.** 82 command handlers are a large surface for a
  desktop app. They were reviewed by sink class; the ones that accept URLs or
  paths validate them. The custom commands are not individually
  permission-gated by Tauri capabilities, so an XSS in the webview would have
  the same command authority as the UI. CSP and the absence of dynamic HTML
  sinks are therefore security controls, not just presentation choices.
- **The sidecar is a local helper, not a trust boundary against the user.**
  Loopback plus a per-launch random token raises the bar against other local
  processes; it is not a defense against a same-user attacker, and it is not
  designed to be.
- **The network layer is now defensively layered.** Host allowlists,
  scheme checks, DNS validation, per-hop redirect verdicts, body caps, and
  redacted errors are present on the reviewed paths. Residual gaps are the
  media-reader DNS race and trusted-zone body caps, not the absence of a policy.
- **Distribution is the weakest layer.** The app's runtime controls are
  materially better than its packaging: an ad-hoc signature and no
  notarization mean users cannot verify the publisher, and Gatekeeper blocks
  the app on a clean machine.

---

## 4. Threat model

### 4.1 Assets

| Asset | Why it matters | Primary threats |
|---|---|---|
| Google access/refresh tokens | Account access within the granted scopes | Log/error leakage, Keychain theft, XSS-driven IPC abuse, malicious local process |
| Subsonic username/password/token | Access to the user's music server | Error/log leakage, plaintext transport, Keychain theft |
| Local library paths and metadata | Privacy (what the user listens to and stores) | Path traversal, over-broad scanning, log leakage, local file read |
| Playback state, queue, history | Integrity and privacy | IPC abuse, local database tampering, crash/DoS |
| User's local network and loopback services | SSRF pivot target | Malicious source/addon URL or redirect, DNS rebinding |
| App bundle and sidecar JAR | Code integrity | Unsigned distribution, same-user file write (build inputs are now digest-pinned) |
| Third-party API keys (PaxSenix, Musixmatch fallback) | Provider abuse, not user account access | Frontend extraction, `localStorage` read, bundled secret |
| User's files and downloads | Data integrity/availability | Malicious download content, filename traversal, disk exhaustion |

### 4.2 Actors and capabilities

| Actor | Capability assumed |
|---|---|
| Remote media/addon/Subsonic/radio/lyrics/artwork server | Can return arbitrary JSON, URLs, redirects, headers, and bodies; may be malicious or compromised. |
| Addon author | Can choose the base URL and stream URL; can attach allowlisted headers. |
| Active network attacker | Can observe/modify plaintext HTTP; can attempt DNS rebinding; cannot break TLS. |
| Local unprivileged process (same user) | Can read the user's files, environment, and logs; can call loopback services; can write the app's data directory. |
| Same-user file writer | Can replace the JAR/bundle, craft a DB/WAL pair, or edit `localStorage`; already stronger than a remote attacker. |
| Supply-chain attacker | Can compromise a dependency, action tag, JitPack coordinate, or upstream sidecar source. |
| Malicious media/metadata | Can exploit decoder/metadata parser bugs; not fuzzed in this audit. |
| User misconfiguration | Can add an HTTP source, a malicious addon, or an unsafe download folder. |
| Root/admin/physical attacker | Out of scope; can already bypass app controls. |

### 4.3 Entry points and controls

| Entry point | Examples | Control |
|---|---|---|
| IPC commands | search, library, playlists, downloads, settings, auth, radio | Boundary validation, allowlists, typed deserialization, state ownership in Rust. |
| Custom `art:` protocol | artwork bytes from the cache | Canonicalized file/root containment; no arbitrary filesystem serving. |
| OAuth loopback | `127.0.0.1:<random>/callback` | State check before code exchange, PKCE verifier, nonce, short-lived listener. |
| Sidecar HTTP | `/search`, `/stream`, `/health`, `/home` | Loopback + per-launch token; local process only. |
| Provider responses | JSON, HTML, images, media, redirects | Scheme/host allowlists, redirect verdicts, DNS checks, body caps, redaction. |
| Imported files | audio metadata, artwork, folder paths | Path confinement, metadata parser use, no execution. |
| SQLite | library, history, downloads, settings | Parameterized values; internal identifiers only. |
| Downloads | media URL, filename, destination | Redirect/DNS policy, sanitized names, user-selected folder. |
| CI/build | actions, crates, npm, JitPack, JRE | Lockfiles, read-only token, pinned sidecar commit, hash records. |

### 4.4 Assumptions

- The OS Keychain and the user's macOS session are not compromised.
- TLS certificate validation is not disabled; the app does not pin certificates.
- The user trusts the sources/addons they configure. The audit still treats a
  malicious source as an attacker because a compromised server is realistic.
- Google's OAuth endpoints and JWKS behave as documented.
- The bundled JRE and sidecar JAR are the artifacts built from the recorded
  provenance; runtime hash verification is not implemented (TSN-2026-13).
- No multi-user tenancy exists on the desktop; the same-user boundary is the
  practical security boundary.

---

## 5. Coverage ledger

The audit produced an authoritative per-file ledger as an audit artifact; it is
not part of the repository. It was generated from the 284-path product
inventory captured during reconnaissance, then merged with current
tracked/untracked product files discovered afterwards.
It excludes reference/design trees, agent state, installed dependencies, build
output, VCS metadata, and the audit deliverables themselves.

| Category | Name | Primary | Secondary | Review focus |
|---|---|---:|---:|---|
| A | Authentication and authorization | 7 | 7 | OAuth, Keychain, scopes, sign-out, session restore |
| B | Tauri IPC, capabilities, command surface | 24 | 49 | Command validation, capabilities, window/event surface |
| C | Injection and unsafe execution | 0 | 27 | SQL, process execution, HTML/JS sinks, path injection |
| D | Filesystem and local data | 8 | 27 | Art serving, downloads, library paths, SQLite |
| E | Network, providers, redirects | 24 | 49 | Host allowlists, redirects, DNS, body caps, errors |
| F | Secrets and privacy | 0 | 43 | Keychain, env, logs, localStorage, bundle |
| G | Cryptography and token validation | 0 | 20 | PKCE, JWKS, claims, HMAC, randomness |
| H | Dependencies and supply chain | 20 | 41 | Lockfiles, audits, licenses, CI, sidecar provenance |
| I | Application logic and abuse resistance | 13 | 115 | Queue caps, volume/NaN, state invariants, DoS |
| J | macOS platform security | 1 | 26 | Entitlements, bundle, Keychain, remote commands |
| K | Frontend security | 102 | 149 | CSP, sinks, routing, storage, provider calls |
| L | Tests and verification tooling | 60 | 60 | Regression tests, scanners, harnesses, fixtures |
| M | Documentation and claims | 20 | 20 | Secret scan, stale security claims, accuracy |
| N | Binary artifacts and generated assets | 21 | 21 | JAR/JRE/icons: hash, provenance, not line review |

**Totals:** 300 ledger entries; 273 source/config files (21 full, 224 targeted,
28 scanned); 21 binary/generated artifacts; 6 deleted tracked files.
Categories C, F, and G have no primary entries because no file is solely about
them; they are represented as secondary tags on the files that carry those
concerns. This is intentional, not a coverage gap.

**Review-level meaning**

- **Full:** substantive read of a security-critical path (auth, IPC, network,
  shell/process, platform config).
- **Targeted:** sink/boundary review with architecture context; not every line.
- **Scanned:** automated or manifest/claim scan (lockfiles, docs, generated
  schemas, assets).
- **Artifact:** binary; hash/provenance assessed, not line-reviewed.
- **Not applicable:** deleted or absent; nothing remained to review.

**Exclusions and treatments**

- `screenshot-to-code/`, `ui-study/`, `ui-reference/`, `ui-replica/`,
  `graphify-out/`, `floorplan/`: reference/design/generated trees, not imported
  or shipped.
- `.agents/`, `.freebuff/`, `.opencode/`, `.zcode/`: agent state, not product
  code.
- `node_modules/`: installed dependencies, audited through the lockfile.
- `target/`, `apps/desktop/dist/`: generated build output; `dist` and the
  packaged app were still secret-scanned.
- `.git/`: VCS metadata.
- `docs/security/`: the audit deliverables themselves.

Artifact records: sidecar JAR 18,289,719 bytes, SHA-256
`38bec046ae89d890c157c5614c4fa21bb31548ca6116d64dcacf12bdff6c08c0`;
JRE directory 30,468,482 bytes. The JAR's upstream source is pinned by commit;
the JRE is a binary dependency and is not line-reviewed.

---

## 6. Findings and remediation

The full register with preconditions, traces, impact, evidence, tests, and
residual risk is [`SECURITY_FINDINGS.md`](./SECURITY_FINDINGS.md). The summary:

| ID | Finding | Severity | Status |
|---|---|---|---|
| TSN-2026-01 | Download redirects could pivot local/private, downgrade, and forward headers | High | remediated |
| TSN-2026-02 | Media readers trusted initial hosts/redirects without DNS validation | Medium | remediated |
| TSN-2026-03 | Credential/signed URLs leaked via transport errors and logs | Medium | remediated |
| TSN-2026-04 | Whole-body consumers had no response-size bound | Medium | remediated |
| TSN-2026-05 | Addon media could downgrade HTTPS→HTTP with authorization | Medium | remediated |
| TSN-2026-06 | Radio station host not resolved before stream open | Medium | remediated |
| TSN-2026-07 | CI default token permissions and persisted credentials | Low | remediated |
| TSN-2026-08 | `libsqlite3-sys 0.30.1` / RUSTSEC-2026-0459 | Low | remediated |
| TSN-2026-09 | Bundle ad-hoc signed, not notarized, fails `spctl` | Medium | open |
| TSN-2026-10 | Sidecar token `==` and environment handoff | Low | remediated (constant-time compare) |
| TSN-2026-11 | Stale "zero vulnerabilities" claims in docs | Info | remediated |
| TSN-2026-12 | Google desktop client secret embedded in binary | Info | accepted |
| TSN-2026-13 | Sidecar JAR not runtime-hash-verified | Low | accepted |
| TSN-2026-14 | PaxSenix key in `localStorage` | Low | accepted |
| TSN-2026-15 | Mutable action tags / JitPack coordinates | Low | remediated |
| TSN-2026-16 | Hardcoded Musixmatch fallback secret | Low | accepted |
| TSN-2026-17 | Local download copy accepted untracked filesystem paths / bypassed `local_files_enabled` | Medium | remediated |
| TSN-2026-18 | Remote download transfer had no per-track maximum size cap | Low | remediated |
| TSN-2026-19 | `.gitignore` missing broad `.env`, key, and database patterns | Low | remediated |

Counts: **14 remediated** (TSN-2026-17/TSN-2026-18 added 2026-11-19; both Rust regression tests pass locally), **4 accepted**, **1 open**, **0 critical**. The one
High finding is conditional on a malicious/compromised source configuration and
is fixed in the current worktree. The remaining open item needs Developer ID
credentials and owner approval, not a code change.

### 6.1 Remediation by file

| File | Change | Regression evidence |
|---|---|---|
| `apps/desktop/src-tauri/src/downloads.rs` | Manual max-5 redirect chain; `download_redirect_verdict`; per-hop DNS resolve-validate-pin; host-scoped custom headers; no `https`→`http`; local/private refusal; redacted errors | `download_redirects_cannot_pivot_local_or_downgrade`, `download_network_errors_drop_the_credential_query` |
| `crates/playback/src/remote.rs` | Initial-host DNS pinning; shared `media_redirect_verdict`; DNS check on redirect hops; redacted logs/errors for range, prefetch, and stream paths | `redirect_verdict_refuses_downgrade_private_and_long_chains`, `media_host_locality`, IPv6 classification regression, `redacted_strips_the_query_string` |
| `crates/playback/src/cache.rs` | Initial-host pinning; shared redirect verdict; URL-redacted fetch errors | `fetch_errors_drop_the_credential_query`, `fetch_caches_and_serves_replays_from_disk` |
| `crates/sources/src/subsonic.rs` | `transport_error` strips the URL from credential-bearing requests | `transport_errors_drop_the_credential_query` |
| `crates/sources/src/addon.rs` | HTTPS→HTTP stream downgrade refused; transport/non-2xx errors drop the endpoint | `stream_url_policy_*` tests incl. the downgrade assertion |
| `crates/sources/src/yt.rs` | Signed googlevideo URLs stripped from `fetch_prefix` errors; sidecar discovery JSON capped | `json_limited` path; workspace suite |
| `apps/desktop/src-tauri/src/bounded_body.rs` | 8 MiB text cap with Content-Length precheck and chunk accumulation cap | `content_length_over_cap_is_refused_before_reading`, `chunked_body_over_cap_is_refused_while_accumulating` |
| `apps/desktop/src-tauri/src/commands/` | Capped bodies for lyrics proxy, Spotify embed/pathfinder; URL-redacted artwork/proxy errors (the former monolith is now split into these modules) | Command tests + workspace suite |
| `apps/desktop/src-tauri/src/radio.rs` | Station host resolve/validate before `resolve` returns; capped directory/artwork bodies | `radio_source_returns_the_url_it_was_keyed_with`, `station_urls_must_be_public_http` |
| `.github/workflows/ci.yml` | `permissions: contents: read` + `actions: read`; `persist-credentials: false` | Workflow review; no CI run in this audit |

### 6.2 What was not changed during the audit

- The Google auth flow was not reworked: PKCE/state/nonce/JWKS/claims and
  Keychain storage were already correct and runtime-verified. It remains
  optional and uses exactly `openid email profile`.
- The playback engine and audio callback were not modified. The remedy for
  network findings stayed in the source/transport layer.
- No dependency was upgraded during the audit solely to silence
  CVE-2026-119375; the upgrade to `rusqlite 0.41` landed afterwards
  (2026-11-17) and is verified in TSN-2026-08.
- No warning was suppressed: `deny.toml` never ignored the advisory. Both
  `cargo deny check advisories` and `check licenses` pass now that the advisory
  is fixed and the license allowlist exists.
- The stale frontend test was fixed separately; the suite is 143/143 at this
  revision.
- No commit, push, tag, release, install, or credential rotation was performed.

---

## 7. Verification and test evidence

### 7.1 Build and test results

| Check | Result |
|---|---|
| `cargo test --workspace` | **262 passed, 0 failed, 9 ignored** across 27 test binaries (re-run 2026-11-19) |
| `cargo clippy --workspace --all-targets` | Passed with warnings; no denied lint, no compile error (re-run 2026-11-19) |
| `pnpm build` (`tsc --noEmit` + Vite) | Passed |
| `pnpm test` | **143 passed of 143**, 30 files (re-run 2026-11-19) |
| `cargo audit --json` | **0 vulnerabilities**, 4 informational warnings (re-run 2026-11-19) |
| `cargo deny check advisories` | Passes; two informational unmaintained advisories (`paste`, `proc-macro-error`) are ignored with reasons |
| `cargo deny check licenses` | Passes against the `[licenses]` allowlist |
| `pnpm audit --audit-level=high` | 0 known vulnerabilities (re-run 2026-11-19) |
| Secret scan | 329 product/report files + `dist` + bundle + 32 commits: 0 source/history leaks; 1 expected binary client-secret hit; 0 standard token/private-key patterns |
| macOS bundle | Ad-hoc/linker-signed; no TeamIdentifier; no sealed resources; `codesign --verify --deep --strict` exit 1; `spctl` exit 1 |

The frontend suite is green in the current tree: the Albums empty-state test no
longer expects an `Import folder` button in that header (the button was removed
there), so the 2026-11-13 drift is gone.

### 7.2 Runtime Google authentication evidence

Earlier in the audit, a real Google sign-in completed in the running app as
the owner's Google test account. The following were observed:

- The profile persisted and the sidebar reflected the signed-in state.
- A Keychain generic-password item exists under service
  `app.tansen.desktop.google`, for that account. The secret was
  not printed.
- Session restore, a forced token refresh, sign-out cleanup, and re-sign-in
  were exercised.
- The token exchange and ID-token validation run in Rust; tokens are not
  serialized to the frontend.
- Sign-out revokes the token (best effort, detached), deletes the Keychain item,
  and clears the profile settings.

The app was closed before final report assembly. No restart, reinstall, or
account switch to a different identity was performed without approval. The
Keychain item remains intact; the Google session was not destroyed.

### 7.3 Scanner evidence

**`cargo audit` (2026-11-13 snapshot, for the historical record)**

```
VULN  RUSTSEC-2026-0459  libsqlite3-sys 0.30.1  (CVE-2026-119375 / GHSA-72w2-rjgg-q729)
WARN  unmaintained       paste 1.0.15
WARN  unmaintained       proc-macro-error 1.0.4
WARN  unsound            glib 0.18.5
```

**`cargo audit` (2026-11-19, current tree)**

```
0 vulnerabilities
WARN  unmaintained  chrono 0.4.45
WARN  unmaintained  paste 1.0.15
WARN  unmaintained  proc-macro-error 1.0.4
WARN  unsound       glib 0.18.5
```

`cargo deny check advisories` now passes. Two informational unmaintained
advisories (`paste`, `proc-macro-error`) are ignored in `deny.toml` with
reasons; `chrono` and `glib` are not ignored and do not fail the check at the
configured scope. The former failure was RUSTSEC-2026-0459 on
`libsqlite3-sys → rusqlite → persistence → app-core/library/playback/sources/tansen`,
which the `rusqlite 0.41` upgrade resolved.

**`pnpm audit`:** 213 total dependencies, 0 info/low/moderate/high/critical
advisories at the audit date.

**Secret scan:** the Google client secret was read from the mode-600,
gitignored env file and used as a literal; it was never printed. The scan
covered 329 product and report files, the production `dist`, the packaged app (including
large binaries via streaming), and all 32 commits (`git grep` with a mode-600
pattern file, not an argv literal). Results: 0 source/history hits; exactly 1
hit in `Tansen.app/Contents/MacOS/tansen` (the expected embedded public desktop
secret); 0 `ya29.`/refresh-token/private-key/AWS/GitHub/Slack/OpenAI/Google-API
patterns. The hardcoded Musixmatch fallback secret was found by source review,
not by the generic patterns (TSN-2026-16).

---

## 8. Dependency and supply-chain assessment

### 8.1 Rust dependencies

- **Lockfile and audit.** `Cargo.lock` is present and was the audit input.
  `cargo audit` now reports **0 vulnerabilities** and four informational
  warnings (see §7.3); the 2026-11-13 snapshot had one, fixed below. The workspace has 0 packages with a missing license field. The 7
  `GPL-3.0-or-later` entries are the workspace's own crates; the remaining
  licenses are permissive or compatible (MIT/Apache-2.0/BSD/ISC/MPL-2.0/
  Unicode/Zlib/CC0, with two dual `MIT OR Apache-2.0 OR LGPL-2.1-or-later`
  entries). This is an inventory, not legal advice.
- **Resolved advisory.** RUSTSEC-2026-0459 was a local file-format issue:
  SQLite before 3.54.0 could return uninitialized page-cache memory when opening
  a crafted DB plus sibling WAL. `crates/persistence/Cargo.toml` now uses
  `rusqlite 0.41`, resolving `libsqlite3-sys 0.39.1`; `cargo audit` is clean.
  Tansen still opens only its own database at `Application Support/…/tansen.db`,
  with no user-selected SQLite path.
- **Warnings.** `paste` and `proc-macro-error` are unmaintained macro/build
  dependencies; `chrono` is flagged unmaintained upstream; `glib` is an unsound
  GTK/Linux transitive advisory not reachable in the shipped macOS path. They
  are not counted as vulnerabilities.
- **Deny configuration.** `deny.toml` has an explicit `[licenses]` allowlist;
  `cargo deny check licenses` and `cargo deny check advisories` both pass. The
  header no longer claims a result the tree does not produce (TSN-2026-11).

### 8.2 Frontend dependencies

`apps/desktop/pnpm-lock.yaml` records 213 dependencies. `pnpm audit` reports no
known vulnerabilities at the audit date. The notable runtime dependencies are
React 19, React Router 8, TanStack Query/Virtual, Zustand, `lucide-react`,
`lenis`, `glisse`, and the vendored `@uimaxbai/am-lyrics`. The lyrics package
renders into a shadow root; the app's CSP constrains script and network access,
and app code has no dynamic HTML sink beyond the static logo. Dependency
behavior was not fuzzed.

### 8.3 CI and build supply chain

- **Strengths:** CI token permissions are read-only (`contents: read`,
  `actions: read`), checkout credentials are not persisted, the sidecar upstream
  source is pinned to commit `7d956f211b8a46a87184fd3ff87432b51a111dfa` in both
  `scripts/build-sidecar.sh` and CI, and the script refuses a dirty/incorrect
  checkout.
- **Weaknesses (resolved):** GitHub Actions are now referenced by full commit
  SHAs, and `scripts/build-sidecar.sh` verifies hardcoded SHA-256 digests for
  the JitPack jar and pom before Gradle sees them. The JitPack coordinate stays
  tag-based, so the digest constants must be updated with the sidecar version
  (TSN-2026-15).
- **Artifacts:** the sidecar JAR hash is recorded
  (`38bec046…f6c08c0`), but the runtime does not verify it
  (TSN-2026-13). The JRE is a bundled binary; its size is recorded and its JVM
  entitlements are documented. Neither was line-reviewed.
- **No SBOM** was generated. The lockfiles and the audit's artifact records
  are the current inventory; an SBOM would improve release provenance.

### 8.4 Distribution

The macOS bundle is ad-hoc/linker-signed, has no TeamIdentifier, no sealed
resources, and no notarization. `codesign --verify --deep --strict` and `spctl`
both fail. This is the largest release-integrity gap (TSN-2026-09). The JVM
entitlements (`allow-jit`, `allow-unsigned-executable-memory`,
`disable-library-validation`) are required by the bundled JRE but widen the
hardened-runtime surface (R-03); they should be reviewed as part of the signing
work, not removed blindly.

---

## 9. Privacy and secrets assessment

### 9.1 Data inventory

| Data | Where it lives | Purpose | Recipients / retention |
|---|---|---|---|
| Google `sub`, name, email, picture | SQLite `settings` table | Show the signed-in profile | Google only; deleted on sign-out |
| Google access/refresh token, expiry | macOS Keychain, service `app.tansen.desktop.google` | Session restore and refresh | Google token endpoint; Keychain item deleted on sign-out |
| Subsonic username/password | macOS Keychain, Subsonic service | Server authentication | The user's Subsonic server; removed when the source is removed |
| Library folders, track metadata, playlists, history, listening events, likes/follows | SQLite `tansen.db` | Local library and recommendations | Local only; no telemetry; user-controlled deletion/retention |
| Artwork cache | App data art directory | Offline artwork | Local only; rebuilt from providers |
| Downloads | User-selected download folder | Offline playback | Local only; user-managed |
| Queue, volume, appearance, locale, lyrics preferences | `localStorage` | Restore the UI | Local only |
| PaxSenix API key | `localStorage` | Optional lyrics provider | Sent to `api.paxsenix.org` when that provider is used |
| Search queries | In-memory cache / provider requests | Search | YouTube Music (via sidecar), Subsonic, addons, Radio Browser, lyrics providers |
| Radio discoveries/artwork | In-memory/disk cache (10 min / art cache) | Radio UI | Radio Browser and station artwork hosts |
| Logs | Local stderr/Console | Diagnostics | Local only; no remote shipping; URLs/tokens redacted on reviewed paths |

### 9.2 Token and secret handling

- **Google tokens** are stored only in the Keychain. `GoogleTokens` has no
  `Debug` derive and is not serialized to the frontend. The profile (not the
  tokens) is persisted in SQLite. Sign-out revokes, deletes, and clears.
- **OAuth scopes** are exactly `openid email profile`. There is no Drive, Gmail,
  contacts, or offline-access scope beyond the refresh token that the flow
  requests for session restore.
- **Desktop client secret** is embedded in the backend binary by `build.rs`
  from a mode-600 gitignored env file. This is expected for a Google desktop
  client and is not a server-side secret (TSN-2026-12). It is not in the
  frontend, resources, logs, or Git history.
- **Subsonic credentials** go to the Keychain first; the password is not written
  to SQLite. Public hosts require HTTPS; HTTP is allowed only for
  loopback/private hosts with explicit opt-in.
- **Sidecar token** is a per-launch 128-bit random value passed in the child
  environment and required in `X-Sidecar-Token`. It is not persisted or logged.
- **PaxSenix key** is user-entered and stored in `localStorage` (TSN-2026-14).
- **Musixmatch fallback secret** is a hardcoded third-party client secret in
  the frontend bundle (TSN-2026-16); it is not a Tansen credential.

### 9.3 Minimization and third parties

The app requires no account for local playback. Google sign-in is optional and
requests only identity scopes. YouTube Music access through the sidecar is
anonymous: no account sign-in, cookies, or tokens cross that endpoint. There is
no analytics, crash reporting, advertising SDK, or telemetry endpoint in the
Rust, TypeScript, or CI code. The third parties that can receive data are the
user's own Subsonic server and addons, Google (OAuth), YouTube Music (search/
stream resolution), Radio Browser (station search), artwork hosts, and the
lyrics providers the user enables. The lyrics providers receive track title/
artist (and the PaxSenix key when selected) over HTTPS. This is inherent to the
feature and is recorded as R-10.

### 9.4 Retention and deletion

Sign-out deletes the Google Keychain item and profile settings. Source removal
deletes the Subsonic Keychain credential. Library/history/download data is
user-controlled local data; there is no remote account to delete. The audit did
not delete or modify user data. A future privacy note should state the
`localStorage` and lyrics-cache retention explicitly.

---

## 10. macOS platform security

| Control | State | Evidence |
|---|---|---|
| Tauri capabilities | Minimal: core/events/window drag/dialog only; no shell/fs/http/updater | `capabilities/default.json` |
| WebView CSP | `default-src 'self'`; IPC-only connect; art/data/blob images; no remote script | `tauri.conf.json` |
| Custom art protocol | Canonicalized file and root; traversal/symlink tests | `main.rs:774-790`, tests |
| Keychain | Generic-password items, separate services, OS ACL | `keychain.rs` |
| Entitlements | JVM JIT/unsigned executable memory/library validation disabled | `Tansen.entitlements:5-14` |
| Code signature | Ad-hoc/linker-signed; no TeamIdentifier; no sealed resources | `codesign -dv --verbose=4` |
| Notarization / Gatekeeper | Not notarized; `spctl` rejects | `spctl --assess` exit 1 |
| Hardened runtime | Incomplete until proper signing; entitlements present | TSN-2026-09 / R-03 |
| Sidecar isolation | Loopback + per-launch token; no account/cookies | `Main.kt:50-77`; `yt.rs:514-534` |

The JVM entitlements are a deliberate trade-off: the bundled JRE needs JIT and
unsigned executable memory, and disabling library validation lets it load its
own native libraries. They should be scoped as tightly as the JRE allows during
the signing work. They are not evidence of a vulnerability by themselves.

---

## 11. Residual and accepted risks

| ID | Risk | Severity | Decision / next action |
|---|---|---|---|
| R-01 | Media-reader redirect hops are DNS-checked then re-resolved by `reqwest`; a validation failure is treated as allow. | Low | Accepted. Per-hop pinning would need playback verification; the downloader already pins every hop. |
| R-02 | Google JWKS/token bodies are not size-capped. Sidecar health/search/stream reads now go through `json_limited`. | Low | Accepted for the trusted Google endpoint; cap it if the issuer becomes configurable. |
| R-03 | JVM entitlements widen the hardened-runtime surface. | Medium | Review with signing/notarization; do not remove blindly. |
| R-04 | Bundle is ad-hoc signed / not notarized (TSN-2026-09). | Medium | Open; needs Developer ID credentials and owner approval. |
| R-05 | Mutable action tags / JitPack coordinates (TSN-2026-15). | Low | Closed: full SHAs in CI and SHA-256-pinned JitPack artifacts. |
| R-06 | Google account switching to a different identity was not runtime-verified. | Info | Verification gap; test with a second account before release. |
| R-07 | Builds/tests ran on the host, not in an OS sandbox. | Info | Methodology limitation; see §13. |
| R-08 | Sidecar JAR is not runtime-hash-verified (TSN-2026-13). | Low | Accepted; record the SHA-256 in release notes and verify at packaging. |
| R-09 | Local data is protected by user file permissions, not app-level encryption. | Low | Accepted; local-first app, no multi-user boundary; FileVault is the OS control. |
| R-10 | Lyrics providers receive track metadata and the user's PaxSenix key when selected. | Low | Disclose in the privacy note; keep providers optional. |
| R-11 | User data deletion/retention for `localStorage` and lyrics caches is not described in one place. | Info | Documentation improvement; no code change required for this audit. |

The resolved SQLite advisory (TSN-2026-08) and sidecar token comparison
(TSN-2026-10), plus the client secret (TSN-2026-12), PaxSenix key (TSN-2026-14),
and Musixmatch secret (TSN-2026-16), are detailed in the findings register.

---

## 12. False positives and rejected hypotheses

The detailed table is in [`SECURITY_FINDINGS.md` §5](./SECURITY_FINDINGS.md#5-rejected-candidates-and-false-positives).
The main rejected candidates were:

- The Google desktop client secret in the binary as a critical leak. It is a
  public native-client identifier, not a server-side secret; PKCE and strict
  token validation carry the security.
- The `cargo deny` license failure as a license violation. It is a missing
  allowlist policy; 0 packages lack a license field and the GPL entries are the
  workspace's own crates.
- The `glib` unsound advisory as a macOS vulnerability. It is a GTK/Linux
  transitive path not shipped in the macOS build.
- `dangerouslySetInnerHTML` in `LogoMark.tsx` as XSS. The input is a static SVG
  constant; CSP blocks inline script.
- Path traversal in `serve_art`, SQL injection in internal-identifier helpers,
  and the sidecar being unauthenticated. The first two are controlled and
  tested; the sidecar now requires a per-launch token, with the remaining
  comparison/environment notes recorded as Low.
- CSP `style-src 'unsafe-inline'` as script injection. It permits inline styles,
  not scripts; the consumer is the vendored lyrics shadow DOM.
- `paste`/`proc-macro-error` warnings as vulnerabilities. They are unmaintained
  build/macro dependencies with no attached vulnerability.
- Radio HTTP streams as a vulnerability. They are public audio with no
  credentials; the app still refuses local/private hosts and HLS.

---

## 13. Limitations

1. **No OS-enforced sandbox.** Builds, tests, and runtime checks ran on the host
   with the user's permissions. `sandbox-exec` is available, but no profile was
   established. Exploitability claims are bounded by source review and unit/
   integration tests, not live exploitation.
2. **No fuzzing.** Decoder/metadata parsers (Symphonia, lofty), JSON parsers,
   and the OAuth/sidecar parsers were not fuzzed. A malicious audio file or
   parser bug remains a residual risk outside this audit's evidence.
3. **Sidecar/JRE not decompiled.** The 18 MB JAR and 30 MB JRE were assessed by
   provenance and hash, not line-by-line. Only the Kotlin overlay source was
   reviewed.
4. **Frontend suite state at audit time.** One pre-existing, untracked UI test
   failed; it has since been fixed with the Albums empty-state change and the
   suite is 143/143.
5. **Account switching to a different Google identity is unverified.** The same
   account was re-selected after sign-out. The flow sends
   `prompt=select_account`; test with a second identity before release.
6. **The app was closed at final assembly.** Runtime auth evidence was collected
   earlier; no restart was performed without approval. The Keychain session was
   preserved.
7. **No Developer ID signing/notarization.** The bundle is ad-hoc signed and
   `spctl` rejects it. This is an open distribution gap (TSN-2026-09).
8. **Advisory database snapshot.** `cargo audit` and `pnpm audit` reflect the
   databases at the audit date; new advisories can appear.
9. **License allowlist, not legal advice.** `deny.toml` now has an explicit
   `[licenses]` allowlist and `cargo deny check licenses` passes; that is a
   tooling control, not a legal opinion.
10. **Custom secret scan, not a full entropy scanner.** It uses the live literal
    plus common token/private-key patterns; it is not equivalent to gitleaks,
    semgrep, or a commercial secret scanner (none were installed). The
    Musixmatch secret was found by source review, showing the limit.
11. **Coverage depth varies.** 224 files received targeted sink review, not a
    full line-by-line read. 28 files were scanned by manifest/claim checks.
    Binary artifacts were hashed, not reviewed.
12. **macOS only.** The built/tested artifact is macOS; Windows runtime behavior
    was not exercised.
13. **No external penetration test.** Google, YouTube, Subsonic, Radio Browser,
    and lyrics providers were not attacked; that was out of scope and not
    authorized.

---

## 14. Evidence-based scorecard

Grades are relative to a local-first desktop application in active development,
not to a regulated multi-tenant service.

| Domain | Rating | Evidence |
|---|---|---|
| Authentication and authorization | **Strong** | PKCE S256, strict state/nonce, JWKS RS256, issuer/audience/expiry/`email_verified`, exact scopes, Keychain-only tokens, sign-out deletion. Account switching unverified. |
| IPC and capability surface | **Strong** | 82 handlers reviewed by sink class; minimal Tauri capabilities; no shell/fs/http plugin; CSP; state owned by Rust. |
| Network, SSRF, and redirects | **Adequate** | Initial-host pinning and per-hop verdicts on downloads/media/cache; host allowlists; DNS validation; body caps; redacted errors. Residual media-hop race and trusted-zone bodies. |
| Injection and unsafe execution | **Strong** | Parameterized SQL; internal identifiers; argument-vector `open`/`java`; no shell; no dynamic HTML; no `eval`. |
| Secrets and privacy | **Adequate** | Keychain tokens, no telemetry, no cookies, redacted logs, exact scopes. Public desktop client secret, PaxSenix `localStorage`, Musixmatch fallback, sidecar env token. |
| Filesystem and local data | **Strong** | Canonicalized art root; sanitized download names; app-owned DB; user-selected folders; traversal tests. |
| Availability and abuse resistance | **Adequate** | 8 MiB body caps, queue/volume caps, bounded downloads; some trusted-zone bodies uncapped; no fuzzing. |
| Dependency and supply chain | **Adequate** | Lockfiles, clean `cargo audit`, `deny` license allowlist, read-only CI token, SHA-pinned actions, SHA-256-pinned JitPack artifacts. No runtime JAR verification. |
| macOS platform and distribution | **Weak** | JVM entitlements and minimal capabilities are deliberate; the bundle is ad-hoc signed, unnotarized, and rejected by Gatekeeper. |
| Verification and regression coverage | **Adequate** | 260 Rust tests, 143/143 frontend, build, clean audits, secret scan, runtime auth. No fuzzing and no sandbox. |
| **Overall** | **Adequate for a development/local-first build; not release-grade** | Strong runtime architecture and materially improved network defenses, offset by unsigned distribution and residual network/entitlement risks. |

The scorecard is not a percentage and is not a guarantee. It is a judgment
grounded in the evidence in this report and the findings register.

---

## 15. Commands and tool versions

### 15.1 Tool versions

| Tool | Version |
|---|---|
| rustc | 1.99.1 (58cf8f9c3 2026-10-13) |
| cargo | 1.99.1 (5f94df478 2026-08-27) |
| cargo-audit | 0.22.2 |
| cargo-deny | 0.20.2 |
| node | v22.23.4 |
| pnpm | 10.18.0 |
| macOS | 27.2 |
| codesign / spctl | System tools (macOS 27.2) |
| sandbox-exec | Available; no profile established |

### 15.2 Principal commands

```sh
# Inventory and coverage
git ls-files; git ls-files --others --exclude-standard; git ls-files -d
python3 "$TANSEN_AUDIT_DIR/make_coverage.py"   # audit scratch dir, not in the repo

# Rust
cargo test --workspace
cargo test -p sources addon
cargo test -p tansen radio
cargo test -p playback
cargo test -p tansen bounded_body
cargo audit --json
cargo deny check advisories
cargo deny check licenses
cargo metadata --format-version 1 --all-features

# Frontend
cd apps/desktop
pnpm test
pnpm build

# Frontend dependency audit
pnpm audit --json

# Secrets
python3 "$TANSEN_AUDIT_DIR/secret_scan.py"

# macOS bundle
APP=target/release/bundle/macos/Tansen.app
codesign -dv --verbose=4 "$APP"
codesign --verify --deep --strict "$APP"
spctl --assess --type execute --verbose=2 "$APP"
/usr/libexec/PlistBuddy -c 'Print :CFBundleShortVersionString' "$APP/Contents/Info.plist"

# Keychain metadata only (the secret was not printed)
security find-generic-password -s app.tansen.desktop.google
```

### 15.3 Evidence locations

The audit's scratch directory was a temporary local path and is not reproduced
here. Its artifacts were: `cargo-test-final3.log` (Rust log), `cargo-audit-final.json`,
`cargo-deny-final.log`, `cargo-metadata.json`, `secret-scan-final.json`,
`make_coverage.py` (coverage generator), and the Cloudflare audit-skill
`README.md`. Any of them can be regenerated in a directory of your choosing
with the commands in §15.2.

---

## 16. Change inventory and repository state

### 16.1 Files changed by this audit's remediation

- `apps/desktop/src-tauri/src/downloads.rs`
- `apps/desktop/src-tauri/src/commands/` (the former `commands.rs`)
- `apps/desktop/src-tauri/src/bounded_body.rs`
- `apps/desktop/src-tauri/src/radio.rs`
- `crates/playback/src/remote.rs`
- `crates/playback/src/cache.rs`
- `crates/sources/src/subsonic.rs`
- `crates/sources/src/addon.rs`
- `crates/sources/src/yt.rs`
- `.github/workflows/ci.yml`
- Test-only mock repair: `apps/desktop/src/pages/LibraryPage.albums.test.tsx`,
  `apps/desktop/src/pages/LibraryPage.recent.test.tsx`
- Audit deliverables: `docs/security/SECURITY_AUDIT.md`,
  `docs/security/SECURITY_FINDINGS.md`

Several of these files were already untracked or modified in the worktree
before the audit. Git cannot attribute line-level provenance for untracked
files; the audit's changes are identified by the controls and tests in §6.1.

### 16.2 Repository state (final)

- Branch `ui-replica`, HEAD `b250adef…`.
- 85 modified tracked files, 62 untracked non-ignored entries, 6 deleted
  tracked files. The worktree was dirty before the audit; unrelated user work
  was preserved.
- `apps/desktop/src-tauri/google-oauth.env` is mode `600`, gitignored, and was
  not committed. It must stay that way.
- The Keychain Google item remains under `app.tansen.desktop.google` for
  the signed-in test account. The app was closed during final assembly;
  no restart/install was performed.
- The only remaining Tansen app bundle is
  `target/release/bundle/macos/Tansen.app` (version `0.3.0`), ad-hoc signed.
  `/Applications` has no Tansen installed.
- No commit, push, tag, release, publication, credential rotation, deletion of
  user data, or macOS setting change was performed.

---

## 17. Conclusion

The audit confirmed a coherent local-first design and materially improved its
weakest network paths. Eleven findings are remediated in the current tree,
including the conditional High-severity download redirect/SSRF issue, the
SQLite advisory, the sidecar token comparison, and the supply-chain pins. The
remaining risk is concentrated in release integrity (unsigned/unnotarized
bundle) and a small set of documented residuals (media-hop DNS race,
trusted-zone body caps, JVM entitlements, embedded third-party secrets).

Tansen should be described as **hardened for development and local use**, not as
"secure" without qualification. Before a public release, the owner should:

1. Sign with a Developer ID, enable the hardened runtime, seal resources,
   notarize, and staple the bundle (TSN-2026-09 / R-03).
2. Verify the sidecar JAR hash at packaging and record it in release notes
   (TSN-2026-13).
3. Test Google account switching with a second identity and document the
   `localStorage`/lyrics-cache retention (R-06, R-11).
4. Re-run the coverage ledger and the full test suite after any of the above.

Already closed since the audit: the SQLite advisory (TSN-2026-08), the
SHA-pinned Actions and sidecar digests (TSN-2026-15), and the stale frontend
test.

No audit can prove the absence of vulnerabilities. This one is bounded by the
scope, the evidence, and the limitations in §13; residual risk remains.
