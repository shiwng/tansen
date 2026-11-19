# Tansen Security Findings Register

| Field | Value |
|---|---|
| Audit ID | `TANSEN-SEC-2026-11` |
| Audit date | 2026-11-13 |
| Repository | Tansen local worktree (absolute path redacted) |
| Branch | `ui-replica` |
| HEAD | `b250adef5e47423081b411dd038b494c4ff82f18` |
| Worktree | Audit snapshot: 85 modified, 62 untracked, 6 deleted tracked files. Re-verified 2026-11-19 in a larger uncommitted worktree (116 modified, 55 untracked, 54 deleted, 2 renamed entries). |
| Methodology | Cloudflare security-audit workflow (reconnaissance, coverage-led hunting, adversarial validation, structured records, independent verification, target-neutral reporting), adapted for a local-first Tauri desktop app |
| Companion report | [`SECURITY_AUDIT.md`](./SECURITY_AUDIT.md) |

This register records what was **confirmed** with a source trace, what was
**remediated and regression-tested**, what is **accepted residual risk**, and
what was **rejected** after adversarial review. It does not claim that the
application is vulnerability-free. Line numbers refer to the current worktree
at the date above; several files are untracked or modified, so they are not
stable commit coordinates.

> **Revision 2026-11-17 — re-verified against the current worktree.** This
> register was first written on 2026-11-13, before `commands.rs` and `repo.rs`
> were split into `commands/` and `repo/`. The SQLite advisory is now fixed
> (`rusqlite 0.41` / `libsqlite3-sys 0.39.1`), the sidecar compares its token
> with `MessageDigest.isEqual`, CI actions are pinned to full commit SHAs, and
> the stale Albums test is fixed (143/143 frontend tests pass). Those items are
> marked `remediated` below. Evidence that pointed at monolith line numbers now
> names the owning module, and local machine paths have been removed.

> **Revision 2026-11-19 (pre-publication re-run).** The complete local suite was
> re-run: clippy, tests (262 passed, 0 failed, 9 ignored), `cargo audit`,
> `cargo deny check advisories licenses`, the frontend tests (143/143), the
> production build, and the high-severity dependency audit all pass.
> `cargo fmt --all -- --check` initially failed on pre-existing drift (71 files);
> a formatting-only `cargo fmt --all` was applied and the gate now passes, with
> clippy and the full test suite re-run afterwards. TSN-2026-17/18 regression
> tests pass explicitly.

## Verdict and severity vocabulary

| Term | Meaning |
|---|---|
| `confirmed` | A complete source-to-sink trace exists, the preconditions are stated, and the impact is bounded. Remediated findings have a regression test or a reproduced control. |
| `needs_validation` | An exact unresolved fact blocks a severity or exploitability decision. No `needs_validation` item is counted as a vulnerability. |
| `rejected` | A candidate was disproved or is a defense-in-depth observation, not a boundary failure. |
| `remediated` | The code now enforces the control and the test suite exercises it. |
| `accepted` | The risk is understood, the owner decision is recorded, and the rationale is in the residual register. |
| `open` | Real work remains; the item is not a code-level vulnerability in the current build or requires an external decision/credential. |

| Severity | Basis |
|---|---|
| Critical | Remote code execution or full credential/session compromise with no meaningful precondition. None found. |
| High | A boundary failure that gives an attacker a materially stronger capability than the user already has (for example, a source server pivoting a download into loopback services), with a realistic precondition. |
| Medium | Limited confidentiality/integrity/availability impact, or a High-impact issue that needs a strong precondition (malicious source, active network attacker, crafted local file). |
| Low | Defense-in-depth, same-user/local-only impact, or a narrow information leak. |
| Info | Documentation, compatibility, or accepted design property with no direct exploit path. |

## 1. Summary

| ID | Finding | Severity | Verdict | Status | Confidence |
|---|---|---|---|---|---|
| TSN-2026-01 | Download worker followed redirects without per-hop validation or DNS pinning | High | confirmed | remediated | High |
| TSN-2026-02 | Media readers trusted initial hosts and redirect targets without DNS validation | Medium | confirmed | remediated | High |
| TSN-2026-03 | Credential-bearing and signed URLs leaked through transport errors and logs | Medium | confirmed | remediated | High |
| TSN-2026-04 | Whole-body HTTP consumers had no response-size bound | Medium | confirmed | remediated | High |
| TSN-2026-05 | Addon media could downgrade `https` to `http` while carrying an authorization header | Medium | confirmed | remediated | High |
| TSN-2026-06 | Radio station hostnames were not resolved before the engine opened the stream | Medium | confirmed | remediated | High |
| TSN-2026-07 | CI workflow used default token permissions and persisted checkout credentials | Low | confirmed | remediated | High |
| TSN-2026-08 | `libsqlite3-sys 0.30.1` is affected by RUSTSEC-2026-0459 / CVE-2026-119375 | Low | confirmed | remediated | High |
| TSN-2026-09 | Release bundle is ad-hoc/linker-signed, not notarized, and fails `spctl` | Medium | confirmed | open | High |
| TSN-2026-10 | Sidecar token is compared with `==` and passed through the environment | Low | confirmed | remediated | High |
| TSN-2026-11 | Project documents still claim `cargo audit` found zero vulnerabilities | Info | confirmed | remediated | High |
| TSN-2026-12 | Google desktop OAuth client secret is embedded in the shipped binary | Info | confirmed | accepted | High |
| TSN-2026-13 | Bundled sidecar JAR is not hash-verified at runtime | Low | confirmed | accepted | High |
| TSN-2026-14 | User-supplied PaxSenix API key is stored in `localStorage` | Low | confirmed | accepted | High |
| TSN-2026-15 | GitHub Actions and JitPack coordinates are mutable tags, not immutable digests | Low | confirmed | remediated | Medium |
| TSN-2026-16 | Hardcoded Musixmatch fallback secret ships in the frontend bundle | Low | confirmed | remediated | High |
| TSN-2026-17 | `downloads_add` / `copy_local` accepted arbitrary local filesystem paths and skipped `local_files_enabled` | Medium | confirmed | remediated | High |
| TSN-2026-18 | Remote download transfer had no per-track maximum byte cap | Low | confirmed | remediated | High |
| TSN-2026-19 | `.gitignore` lacked broad `.env`, key, and local database exclusions | Low | confirmed | remediated | High |

Findings by status: **15 remediated** (TSN-2026-17 and TSN-2026-18 were added on 2026-11-19 and their Rust unit tests pass locally in `cargo test --workspace`; TSN-2026-16 was remediated on 2026-11-19), **3 accepted**, **1 open**, **0 critical**.
The High finding is conditional on a malicious or compromised source/addon
configuration; it is not exploitable from the default local-only library. The
remaining open item (TSN-2026-09) needs Developer ID credentials and owner
approval, not a code change.

---

## 2. Remediated findings

### TSN-2026-01 — Download redirects could pivot into local services and downgrade transport

- **Severity:** High (conditional). **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `apps/desktop/src-tauri/src/downloads.rs`.
- **Preconditions:** The user has configured a remote source (Subsonic server or
  addon) and starts a download; the source is malicious, compromised, or its
  response is tampered with. The default local-only library is not affected.
- **Source-to-sink trace (pre-remediation):** a source returned a media URL →
  the download worker built a `reqwest` client with the default redirect policy
  → a `3xx Location` could point at `http://127.0.0.1:*`, an RFC1918 address, or
  a plaintext `http://` URL → the worker issued the follow-up request and wrote
  the response into the download directory. Caller-supplied source headers could
  be attached to the redirected request, and transport errors included the full
  URL (including Subsonic `u`/`t`/`s` query values).
- **Impact:** blind server-side request forgery against services bound to
  loopback/LAN; plaintext downgrade of a media fetch; possible exposure of source
  authorization headers to a redirect target; local files written from an
  attacker-selected response. Response bytes are not returned to the source
  server, so this is not a general-purpose exfiltration primitive.
- **Evidence (current control):**
  - `download_redirect_verdict` (`downloads.rs:109-130`) rejects more than five
    hops, non-HTTP(S) schemes, `https`→`http`, and local/private targets unless
    the original URL was already local.
  - `download_client` (`downloads.rs:133-151`) resolves the target host with
    `netguard::resolve_validated` and pins the client to the validated address;
    it is rebuilt for every hop.
  - `send_download` (`downloads.rs:153-199`) follows redirects manually, keeps
    custom headers on the original host only, and redacts transport URLs with
    `without_url()`.
  - Regression tests: `download_redirects_cannot_pivot_local_or_downgrade`
    (`downloads.rs:1229`) and `download_network_errors_drop_the_credential_query`
    (`downloads.rs:1247`).
- **Remediation:** manual redirect chain with per-hop validation, DNS
  resolve-validate-pin, no cross-host custom headers, no downgrade, bounded hop
  count, redacted errors.
- **Residual risk:** a malicious source can still make the app download
  attacker-chosen public content into the user's download folder; that is
  inherent to a download feature. Redirects to public hosts remain allowed so
  legitimate CDNs keep working.

### TSN-2026-02 — Media readers trusted initial hosts and redirect targets without DNS validation

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `crates/playback/src/remote.rs` (`HttpRangeReader`,
  `PrefetchReader`, streaming reader), `crates/playback/src/cache.rs`.
- **Preconditions:** A malicious addon, Subsonic server, radio-directory entry,
  or compromised metadata returns a URL whose hostname resolves to loopback or
  private space, or a redirect to such a target; the user plays or pre-caches the
  track.
- **Source-to-sink trace (pre-remediation):** a resolved media URL entered the
  range reader or cache → `reqwest` followed redirects with its default policy →
  the client only classified the URL's literal host, not the DNS answer or the
  redirect target → the request could reach a local service. The prior audit had
  already fixed an IPv6 literal classification bug; DNS names remained the gap.
- **Impact:** blind SSRF and local-service interaction from the media pipeline;
  a local response could be written into the stream cache. Audio bytes are not
  sent back to the source, so the impact is bounded.
- **Evidence (current control):**
  - `media_redirect_verdict` (`remote.rs:50-80`) rejects non-HTTP(S) schemes,
    `https`→`http`, local/private targets for public sources, and resolves the
    hop to reject a local DNS answer.
  - `HttpRangeReader::new` (`remote.rs:103-136`) and `PrefetchReader::new`
    (`remote.rs:470-500`) resolve and pin the initial host, then use the shared
    verdict for every redirect.
  - `StreamCache::fetch` (`cache.rs:79-118`) pins the initial host and applies
    the same verdict per hop.
  - Regression tests: `redirect_verdict_refuses_downgrade_private_and_long_chains`
    (`remote::redirect_policy_tests`), `media_host_locality`, the IPv6
    classification regression, and `fetch_errors_drop_the_credential_query`
    (`cache.rs:217`).
- **Remediation:** initial-host DNS resolution and pinning; shared redirect
  verdict with DNS validation; cache uses the same policy; transport errors
  redacted.
- **Residual risk:** for media readers the redirect verdict resolves the hop and
  then `reqwest` resolves again when connecting, so a DNS-rebinding attacker
  could answer differently between the check and the connection. A validation
  DNS failure is treated as "allow" in that helper. The downloader does not have
  this gap because it rebuilds and pins a client per hop. See `R-01` in the
  residual register.

### TSN-2026-03 — Credential-bearing and signed URLs leaked through transport errors and logs

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `crates/sources/src/subsonic.rs`, `crates/sources/src/addon.rs`,
  `crates/sources/src/yt.rs`, `crates/playback/src/cache.rs`,
  `crates/playback/src/remote.rs`,   `apps/desktop/src-tauri/src/downloads.rs`,
  `apps/desktop/src-tauri/src/commands/` (the former `commands.rs`),
  `apps/desktop/src-tauri/src/bounded_body.rs`.
- **Preconditions:** Any network failure (DNS, connect, TLS, reset, timeout) while
  a request is in flight. Subsonic URLs carry `u`, `t` (token), and `s` (salt);
  googlevideo URLs carry signed query parameters; addon and artwork URLs may
  carry provider keys in the path or query.
- **Source-to-sink trace:** `reqwest::Error`'s `Display` includes the full URL →
  the call site converted it with `e.to_string()` → the string was returned over
  IPC, rendered in the UI, or written to a `tracing` log. The Subsonic token is
  replayable until its server-side window expires; a signed media URL is
  replayable until its expiry.
- **Impact:** credential and signed-URL disclosure to anyone who can read logs or
  the UI, and to crash/feedback artifacts if a user shares them.
- **Evidence (current control):**
  - `SubsonicSource::transport_error` (`subsonic.rs:256-258`) uses
    `without_url()`; test `transport_errors_drop_the_credential_query`
    (`subsonic.rs:493`) asserts the token and username are absent.
  - Download worker error paths use `without_url()`
    (`downloads.rs:179`, `:620`, `:660`); test
    `download_network_errors_drop_the_credential_query` (`downloads.rs:1247`).
  - Addon transport and non-2xx errors drop the endpoint
    (`addon.rs:306`, `:336`).
  - Cache fetch drops the URL (`cache.rs:117`); test at `cache.rs:217`.
  - Range/prefetch/stream paths redact logs and errors
    (`remote.rs:152-158`, `:243-247`, `:662-664`); the existing
    `redacted_strips_the_query_string` test covers the log redactor.
  - `fetch_prefix` drops signed googlevideo URLs (`yt.rs:1117-1130`); the
    bounded-body error path also uses `without_url()` (`bounded_body.rs:25`).
- **Remediation:** URL stripping at every converted transport error; a generic
  addon HTTP-status message; the existing redacted-URL logger retained.
- **Residual risk:** protocol-level errors from the sidecar may still include
  upstream text. No credential-bearing URL was observed in that text during the
  audit. New clients must use the same `without_url()` convention; this is a
  review checklist item, not a compiler-enforced rule.

### TSN-2026-04 — Whole-body HTTP consumers had no response-size bound

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `apps/desktop/src-tauri/src/commands/` (lyrics proxy,
  Spotify embed/pathfinder), `apps/desktop/src-tauri/src/radio.rs` (directory and
  artwork), `crates/sources/src/yt.rs` (sidecar discovery).
- **Preconditions:** An allowlisted lyrics/artwork provider, the radio directory,
  or a compromised provider returns an oversized or endless chunked body while
  the user opens lyrics, radio, or the home feed.
- **Source-to-sink trace:** `Response::text()` / `Response::bytes()` buffer the
  whole body before returning → the app's memory grows with the response → the
  process can be killed or the UI can stall.
- **Impact:** denial of service against the desktop app; no code execution.
- **Evidence (current control):**
  - `bounded_body::bytes` checks `Content-Length` first and then enforces the cap
    while accumulating chunks, because a chunked response can omit or lie about
    the header. `TEXT_MAX_BYTES` is 8 MiB (`bounded_body.rs:9-40`).
  - Wired into the lyrics proxy, Spotify embed and pathfinder (the `commands/`
    modules after the split), radio directory
    (`radio.rs:219`), and radio artwork (`radio.rs:427`; 5 MiB artwork cap).
  - Sidecar discovery uses `json_limited` with `Content-Length` and chunk caps
    (`yt.rs:687-700`).
  - Regression tests: `content_length_over_cap_is_refused_before_reading` and
    `chunked_body_over_cap_is_refused_while_accumulating`
    (`bounded_body::tests`).
- **Remediation:** shared capped-body helpers; per-call caps; chunked responses
  cannot bypass the bound.
- **Residual risk:** Google JWKS/token responses still use unbounded
  `.json()`/`.text()`. That endpoint is Google's authenticated OAuth service, so
  the risk is Low and recorded as `R-02`. The sidecar's health, search and
  stream reads now all go through `json_limited`.

### TSN-2026-05 — Addon media could downgrade HTTPS to HTTP with an authorization header

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `crates/sources/src/addon.rs`.
- **Preconditions:** An addon with an `https` base returns an `http` stream URL;
  the stream request may carry one of the allowlisted headers (`authorization`,
  `referer`, `user-agent`).
- **Source-to-sink trace:** `GET /stream/{id}` response → `validate_stream_url`
  accepted any `http(s)` URL → the engine opened it. An active network attacker
  could read or replace the audio and, if `authorization` was attached, capture
  the header. The same issue allowed a public `https` addon to direct media at a
  plaintext endpoint.
- **Impact:** media integrity/confidentiality loss under an active network
  attacker; possible addon credential disclosure.
- **Evidence (current control):** `validate_stream_url` now rejects an `https`
  base whose stream is not `https` (`addon.rs:268-274`); local addons (the only
  `http` base allowed) still serve `http` streams. Regression asserts the
  downgrade is refused (`addon::url_policy_tests::stream_url_policy_*`).
- **Remediation:** scheme-consistency check on the stream URL; existing locality
  and userinfo checks retained.
- **Residual risk:** an `http` addon base is still plaintext by explicit user
  opt-in. The addon UI exposes the opt-in only for Subsonic; the model default is
  `false`.

### TSN-2026-06 — Radio station hostnames were not resolved before opening the stream

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `apps/desktop/src-tauri/src/radio.rs`.
- **Preconditions:** The radio directory (or a tampered response) supplies a
  station URL whose hostname resolves to loopback/private space (`localtest.me`,
  a rebinding name, or a compromised DNS answer); the user plays the station.
- **Source-to-sink trace:** station JSON → `station_url_allowed` checked only the
  URL text (public host/scheme, no `.m3u8`) → `RadioSource::resolve` returned the
  URL → the engine opened it → the media pipeline contacted the local service.
- **Impact:** blind SSRF from the media pipeline; no audio bytes are returned to
  the directory server.
- **Evidence (current control):** `RadioSource::resolve` parses the URL and calls
  `netguard::resolve_validated` on a blocking thread before returning the media
  (`radio.rs:468-492`). The regression test uses an IP literal so it stays
  offline and documents the hostname path.
- **Remediation:** resolve and validate the station hostname off the async
  runtime before the engine opens the stream.
- **Residual risk:** the validated address is not pinned for the initial radio
  connection, so the same DNS TOCTOU as `R-01` applies. Radio streams are public
  and carry no credentials; the impact is blind local access.

### TSN-2026-07 — CI used default token permissions and persisted checkout credentials

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated.
- **Components:** `.github/workflows/ci.yml`.
- **Preconditions:** A compromised action/dependency or a malicious change runs in
  CI with the default `GITHUB_TOKEN`; the checkout step leaves the token in the
  local Git config.
- **Source-to-sink trace:** workflow job → default token permissions (write for
  same-repo pushes in older defaults) → any step could push or modify repository
  state; `actions/checkout` persisted the credential in `.git/config`.
- **Impact:** repository integrity compromise from a CI supply-chain failure.
- **Evidence (current control):** top-level `permissions: contents: read` and
  `actions: read` (`ci.yml:18-21`); `persist-credentials: false` on both checkout
  steps (`ci.yml:27-29`, `:32-41`).
- **Remediation:** least-privilege workflow token; no persisted checkout
  credential.
- **Residual risk (now closed):** actions were referenced by mutable tags and
  the sidecar overlay used JitPack tag coordinates. Both are now digest-pinned
  (full SHAs, hardcoded SHA-256 for the JitPack artifacts); see TSN-2026-15.

---

## 3. Resolved, open and accepted findings

### TSN-2026-08 — `libsqlite3-sys 0.30.1` / SQLite before 3.54.0 (RUSTSEC-2026-0459)

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated (2026-11-17).
- **Advisory:** RUSTSEC-2026-0459, `CVE-2026-119375`, `GHSA-72w2-rjgg-q729`.
  CVSS 4.0: `AV:L/AC:L/AT:N/PR:N/UI:P/VC:H/VI:N/VA:N/SC:N/SI:N/SA:N`.
- **Affected path at audit time:** `libsqlite3-sys 0.30.1` → `rusqlite 0.32.1`
  → `persistence` → `app-core`, `library`, `playback`, `sources`, `tansen`. The
  `bundled` feature compiled the vulnerable SQLite source.
- **Resolution (2026-11-17):** `crates/persistence/Cargo.toml` now pins
  `rusqlite = "0.41"`, which resolves `libsqlite3-sys 0.39.1` in `Cargo.lock` —
  past the fixed 0.39.0. `cargo audit` over 548 crates reports **0
  vulnerabilities** and four informational warnings (`chrono`, `paste`,
  `proc-macro-error`, `glib`). `cargo deny check licenses` passes. The
  accepted-risk rationale below is kept as the historical record of why the
  upgrade could wait until the persistence layer was next touched.
- **Preconditions:** A less-trusted party can write a crafted database file *and*
  its sibling `-wal` file into the app's data directory; the user then opens the
  database, and an ordinary query returns uninitialized page-cache memory.
- **Impact:** local confidentiality (memory disclosure) per the advisory. It is
  not remotely reachable, and it is not triggered by audio files, metadata, or
  network responses.
- **Why it was accepted at audit time:** Tansen opens only its own database at
  `Application Support/…/tansen.db` (`app-core/src/state.rs`). No command
  accepts a user-selected SQLite path, and no downloaded media is opened as a
  database. An attacker who can write the app's database and WAL has same-user
  file write, which already permits stronger compromise (replacing the app,
  reading the Keychain through the user's session, or editing the library).
- **Evidence then and now:** at audit time `cargo audit --json` reported 1
  vulnerability and `cargo deny check advisories` failed on the same advisory;
  `Cargo.lock` listed `libsqlite3-sys 0.30.1`. After the upgrade, both are
  clean.
- **Residual control:** the app still opens only its own database; if a feature
  ever imports a user-supplied SQLite file, re-check the advisory database at
  that time.

### TSN-2026-09 — Release bundle is ad-hoc/linker-signed and not notarized

- **Severity:** Medium. **Confidence:** High. **Verdict:** confirmed. **Status:** open.
- **Components:** `target/release/bundle/macos/Tansen.app`.
- **Preconditions:** The user downloads the bundle from a channel that is not
  integrity-protected, or an attacker can replace it before first launch.
- **Evidence:** `codesign -dv --verbose=2` reports
  `flags=0x20002(adhoc,linker-signed)`, `TeamIdentifier=not set`; the resource
  envelope is absent (`Sealed Resources=none`, `Info.plist=not bound`);
  `codesign --verify --deep --strict` exits 1 with "code has no resources but
  signature indicates they must be present"; `spctl --assess --type execute`
  exits 1. The bundle version is `0.3.0`.
- **Impact:** Gatekeeper rejects the app on a clean machine; users cannot verify a
  publisher identity; tampering after build is not detectable. This is a
  distribution-integrity gap, not a runtime code vulnerability.
- **Remediation (requires owner action):** Developer ID Application signing,
  hardened runtime, a complete resource seal, notarization, and stapling. The
  JVM entitlements are already in `Tansen.entitlements`; they should be reviewed
  with the signing identity. Do not remove them blindly: the bundled JRE needs
  JIT and unsigned executable memory.
- **Status rationale:** signing credentials and `/Applications` installation were
  explicitly out of scope without approval. The running build was not replaced.

### TSN-2026-10 — Sidecar token comparison and environment exposure

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated (2026-11-17); environment handoff remains accepted.
- **Components:** `sidecar-overlay/sidecar/src/main/kotlin/sidecar/Main.kt`,
  `crates/sources/src/yt.rs`.
- **Evidence:** the sidecar requires `X-Sidecar-Token`. The comparison is now
  `MessageDigest.isEqual` over the UTF-8 bytes (`Main.kt`, `authorized()`), so
  it is constant-time. The launcher generates a 128-bit token from the OS
  CSPRNG and passes it in the child environment (`yt.rs`, sidecar launch). The
  server binds `127.0.0.1` only.
- **Impact:** a same-user process that can read the app's environment or memory
  can call the local sidecar. A timing side channel against Kotlin string
  equality over loopback was not practical, but the pre-remediation comparison
  was not constant-time; it now is.
- **Residual (accepted):** the token still crosses the child environment, so a
  same-user process that can read `/proc`-style process state (or the parent's
  environment) can call the helper. Loopback plus a per-launch random token is a
  reasonable local boundary for a bundled helper that never handles account
  credentials; the token is not persisted, logged, or sent over the network. If
  the sidecar ever handles credentials or becomes multi-user, move the handoff
  to a pipe/file descriptor.

### TSN-2026-11 — Stale "zero vulnerabilities" claims in project documents

- **Severity:** Info. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated (2026-11-17).
- **Evidence:** the dated work log that existed at audit time claimed
  `cargo audit`: **0 vulnerabilities** in its baseline table and security gate;
  `deny.toml`'s header repeated the claim. After RUSTSEC-2026-0459 entered the
  advisory database, that statement was no longer true. The work log has since
  been removed from the repository.
- **Impact:** misleading assurance for maintainers and users; no runtime impact.
- **Remediation:** this register and `SECURITY_AUDIT.md` now carry the re-run
  results, and `deny.toml` has never ignored the advisory. The SQLite upgrade
  described in TSN-2026-08 means `cargo audit` is clean again, so the original
  claim is no longer contradicted by the tree.

### TSN-2026-12 — Google desktop OAuth client secret is embedded in the binary

- **Severity:** Info. **Confidence:** High. **Verdict:** confirmed. **Status:** accepted.
- **Evidence:** `build.rs:25-31` reads `TANSEN_GOOGLE_CLIENT_SECRET` from the
  environment or the mode-600 gitignored `google-oauth.env` and embeds it with
  `cargo:rustc-env`. The packaged backend binary contains one literal instance;
  the frontend `dist`, bundled resources, logs, and Git history contain none
  (secret scan: 329 product and report files, 32 commits, 0 source/history hits; 1 expected
  binary hit).
- **Impact:** anyone with the app can extract the secret.
- **Why accepted:** Google's OAuth 2.0 for native apps treats the desktop client
  secret as a non-confidential identifier, not a server-side secret. The app uses
  Authorization Code + PKCE S256, a loopback redirect, strict `state`/`nonce`,
  JWKS signature verification, and Keychain-only token storage. There is no
  server-side trust in the secret. The env file is mode `600` and gitignored; it
  must never be committed. If Google changes the client type or policy, revisit.

### TSN-2026-13 — Bundled sidecar JAR is not hash-verified at runtime

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** accepted.
- **Evidence:** `apps/desktop/src-tauri/resources/sidecar-all.jar` is a bundled
  artifact (18,548,947 bytes; SHA-256
  `eedbb8e3fdc645a48e77a5e1b5c8ce9ca74312e36f135015de0578775d8aca69`, rebuilt
  2026-11-19). The build
  script pins upstream BitChord commit `7f45c768a461fd7e68bf4613f71e8a767e643d4c`
  (the v1.9.1 tag — re-pinned on 2026-11-19 from v1.8.1 `7d956f21…`, which the
  overlay no longer compiles against; the previously recorded jar was built
  from a newer upstream checkout, so the old pin understated its provenance)
  and refuses a dirty checkout; the runtime discovers and launches the JAR
  without comparing a hash. The `resources/` directory is gitignored, so the
  binary is reproducible from the pinned source, not reviewed as a tracked file.
- **Impact:** an attacker who can replace the JAR in the app bundle or the
  per-user resources directory can execute code in the sidecar. That attacker
  already has same-user/bundle write access, which is a stronger position than a
  remote attacker.
- **Why accepted:** the artifact is launched from the app's own resources, the
  distributed bundle is the integrity boundary (and is currently unsigned; see
  TSN-2026-09), and runtime hashing would require a build-time manifest and a
  key-management decision. Record the hash in release notes and verify it during
  packaging. Do not treat the JAR as line-reviewed source; only the overlay and
  provenance were reviewed.

### TSN-2026-14 — User-supplied PaxSenix API key is stored in `localStorage`

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** accepted.
- **Evidence:** `apps/desktop/src/lyrics/settings.ts:36,47` persists
  `paxsenixKey` in `localStorage` alongside lyric preferences. The key is sent to
  `api.paxsenix.org` for the user-selected provider
  (`lyrics/repository.ts:135-140`).
- **Impact:** a same-user process, a profile backup, or an XSS in the webview can
  read the key. It is a third-party API key, not a Tansen or Google credential.
- **Why accepted:** the key is user-entered, optional, and scoped to a lyrics
  provider; the app has no dynamic HTML sink in its own code, CSP blocks remote
  script/connect, and the only `dangerouslySetInnerHTML` is a static logo. A
  future hardening step is to move it to the Keychain, but that adds a native
  round-trip to a non-sensitive preference store.

### TSN-2026-15 — Mutable GitHub Action tags and JitPack coordinates

- **Severity:** Low. **Confidence:** Medium. **Verdict:** confirmed. **Status:** remediated (2026-11-17).
- **Evidence at audit time:** `.github/workflows/ci.yml` used `actions/checkout@v4`,
  `actions/setup-java@v4`, `android-actions/setup-android@v3`,
  `dtolnay/rust-toolchain@stable`, `pnpm/action-setup@v4`, and
  `actions/setup-node@v4`.
- **Resolution:** every action in `ci.yml` is now pinned to a full commit SHA
  with the version in a trailing comment (`actions/checkout@11d5960a… # v4.4.0`,
  `actions/setup-java@cf277c60… # v4.9.1`, `android-actions/setup-android@9fc6c4e9…
  # v3.2.2`, `dtolnay/rust-toolchain@39008827… # stable branch`,
  `actions/cache@0057852b… # v4.3.0`, `pnpm/action-setup@fc06bc12… # v4.4.0`,
  `actions/setup-node@49933ea5… # v4.4.0`). The JitPack artifacts
  (`innertubex-desktop-v0.7.0.jar` and its `.pom`) are fetched through
  `fetch_verified` in `scripts/build-sidecar.sh`, which compares a hardcoded
  SHA-256 and refuses a mismatch before the file reaches `~/.m2`. The JitPack
  coordinate is still tag-based, but the accepted bytes are digest-pinned.
- **Residual:** keep the digests current when the sidecar version moves; a
  Renovate/Dependabot review is still the low-effort guard for action SHAs.

### TSN-2026-16 — Hardcoded Musixmatch fallback secret in the frontend bundle

- **Severity:** Low. **Confidence:** High. **Verdict:** confirmed. **Status:** remediated (2026-11-19).
- **Evidence:** `apps/desktop/src/lyrics/providers.ts` defined
  `MXM_FALLBACK_SECRET` and used it when the live Musixmatch web-player secret
  scrape failed. It was bundled into the frontend `dist`
  and extractable by anyone with the app. This was already recorded as accepted
  in the prior audit (TAN-09); it is repeated here because it is a real embedded
  third-party secret and the standard token-pattern scan does not classify it.
- **Impact:** abuse of Musixmatch's web API under the shared key, potentially
  rate-limiting or blocking the provider for all Tansen users; no Tansen user
  credential, Google token, or local file is exposed.
- **Remediation:** the constant and both fallback call sites were removed from
  `providers.ts` (commit "Prepare Tansen v0.3.0 for public launch"). The
  provider now reads Musixmatch's web-client signing secret from Musixmatch's
  own public web bundle at request time; when that fetch fails,
  `musixmatchSecret()` returns null, `mxmGet()` returns null, and the lyrics
  repository falls through to the next configured source. No key can be rotated
  by Tansen (it is Musixmatch's own client secret, publicly derivable from
  their web app), so removal — rather than rotation — is the correct fix. The
  residual is a graceful degradation: the Musixmatch source returns no lyrics
  when their bundle cannot be read, which the source list already communicates
  per provider. The literal remains in local Git history (commit `0c93870`);
  the repository has never been pushed, the value is a publicly derivable
  third-party web-client constant and not a Tansen credential, so no history
  rewrite is performed. Do not present it as a Tansen secret.

---

## 4. Verified controls (not findings)

These were reviewed and are working; they are listed so the reader can see what
was tested rather than assumed.

| Control | Evidence |
|---|---|
| OAuth Authorization Code + PKCE S256, loopback redirect, strict `state` | `google_auth.rs:267-352`; state mismatch is rejected before code exchange; `random_urlsafe` uses 32 OS-random bytes. |
| ID-token signature and claims | `google_auth.rs:168-260`: JWKS RS256 verification, issuer, audience, expiry (60 s skew), mandatory nonce, `email_verified`; tests cover each rejection. |
| Exact Google scopes | `google_auth.rs:36`: `openid email profile`. No Drive/Gmail/contacts scope. |
| Token custody | Tokens are a non-`Debug` struct (`google_auth.rs:111-118`), stored in the macOS Keychain under `app.tansen.desktop.google` (`keychain.rs:157-163`), never serialized to the frontend; sign-out revokes (best effort), deletes the Keychain item and profile settings (`google_auth_commands.rs:44-48, 209-216`). |
| Keychain item present at runtime | `security find-generic-password -s app.tansen.desktop.google` returned the item for the signed-in test account; the secret was not printed. |
| Artwork path confinement | `serve_art` canonicalizes the file and the art root and rejects out-of-root paths (`main.rs:774-790`); traversal and symlink regression tests exist. |
| Tauri capability surface | `capabilities/default.json` grants only `core:default`, events, window dragging, and dialog; no shell, fs, http, or updater plugin capability. |
| WebView CSP | `default-src 'self'`; `connect-src` only self/IPC; `img-src` self/art/data/blob; no remote script. `style-src 'unsafe-inline'` is retained for the vendored lyrics shadow DOM, not for app-authored HTML. |
| Frontend injection sinks | Only `LogoMark.tsx` uses `dangerouslySetInnerHTML` with a static SVG constant; no `window.open`, dynamic `innerHTML`, `eval`, `new Function`, remote navigation, or cookies in app code. |
| Command execution | Only the platform browser opener with an argument-vector OAuth URL (`google_auth_commands.rs`, `open_system_browser`) and the sidecar `java -jar` launch (`yt.rs`); no shell interpolation and no debug kill helper remains after `e2e.rs` was deleted. |
| SQL identifiers | All interpolated table/order names are internal constants in the `repo/` modules (keyset and list builders); values are parameterized. No user-controlled SQL identifier. |
| Subsonic plaintext policy | `http` is allowed only for loopback/private hosts with explicit opt-in; public DNS names must be `https` (`subsonic.rs:69-84`, tests 469-478). |
| Download filenames | Control characters and path separators are replaced; names are trimmed and bounded to 140 chars (`downloads.rs:1029-1053`; test 1221). |
| Sidecar network boundary | Binds `127.0.0.1`, requires a per-launch 128-bit token header; the Rust launcher sets it (`Main.kt:58-61`; `yt.rs:514-534`). |
| Sidecar account boundary | No YouTube account sign-in, cookies, or tokens cross the sidecar; streams are resolved anonymously (`Main.kt` header comment; `yt.rs:729`). |
| Secret hygiene | Custom scan of 329 product and report files, `dist`, the packaged app, and 32 Git commits: 0 source/history hits for the client secret; exactly 1 expected hit in the backend binary; 0 token/private-key patterns. The env file is mode `600` and gitignored. |
| No telemetry | No Sentry/PostHog/Segment/Mixpanel/Amplitude/crash-reporter dependency or endpoint found in Rust, TypeScript, or CI. Listening history stays in the local SQLite database. |

---

## 5. Rejected candidates and false positives

| Candidate | Why rejected |
|---|---|
| "Critical: Google client secret is in the binary." | The desktop OAuth client secret is not a server-side secret under Google's native-app model; PKCE, loopback redirect, and strict token validation carry the security. It is still not placed in source, frontend, resources, logs, or Git. Severity would change only if the app started trusting a backend that uses the secret as an authenticator. |
| "`cargo deny check licenses` fails, so the project has a license violation." | This was true only before the allowlist existed. `deny.toml` now has an explicit `[licenses]` allowlist and the check passes; `cargo metadata` shows 0 packages without a license field. The 7 `GPL-3.0-or-later` entries are the workspace's own crates; the rest are permissive/compatible. |
| "`glib 0.18.5` is unsound, so the app is vulnerable." | The advisory is on a GTK/Linux transitive stack not shipped in the macOS build; `cargo deny` reports `advisory-not-detected` for the macOS target. It remains an informational unmaintained/unsound warning for Linux builds, not a macOS runtime finding. |
| "`dangerouslySetInnerHTML` in `LogoMark.tsx` is XSS." | The input is a static, source-controlled SVG string; no user, provider, or network data reaches it. CSP also blocks inline script. |
| "`serve_art` is path-traversable." | The handler canonicalizes both the requested file and the art root and rejects escapes; regression tests cover raw and percent-encoded `..`. |
| "SQL injection in `count`/`set_liked_in`." | The interpolated identifiers are compile-time string constants from call sites; all values use bound parameters. There is no user-controlled identifier path. |
| "The sidecar is unauthenticated on loopback (prior TAN-07)." | The launcher now sets a per-launch 128-bit token and every route checks it; the remaining comparison/environment notes are TSN-2026-10. Standalone dev runs without a token are an explicit development mode. |
| "CSP `style-src 'unsafe-inline'` allows script injection." | It allows inline *styles*, not scripts; `default-src 'self'` blocks inline/external script. The consumer is the vendored lyrics shadow-root styling, not app-authored HTML. Recorded as an accepted Info residual from the prior audit (TAN-04). |
| "`paste` and `proc-macro-error` are vulnerabilities." | Both are unmaintained build/macro dependencies with no attached vulnerability. They are informational warnings, not exploitable findings. |
| "`pnpm audit` found nothing, so the frontend is clean." | That is evidence, not proof; the frontend was also reviewed for sinks and the lockfile is part of the ledger. The statement is bounded to the advisory database at the audit date. |
| "A radio `http://` stream is a vulnerability." | Radio streams are public and carry no credentials; plaintext audio is a confidentiality choice the user makes by selecting a station. The app still refuses local/private station hosts and HLS. |
| "`paxsenixKey` in `localStorage` is a Tansen credential leak." | It is an optional third-party lyrics key entered by the user, not a Tansen/Google token. It is a Low accepted risk (TSN-2026-14), not a session compromise. |

## 6. Residual risk register

| ID | Residual risk | Severity | Status / owner action |
|---|---|---|---|
| R-01 | Media-reader redirect hops are DNS-checked and then re-resolved by `reqwest`; a validation failure is treated as allow. A rebinding attacker could win the race. | Low | Accepted for now; per-hop client pinning would require playback-behavior verification. The downloader already pins every hop. |
| R-02 | Google JWKS/token responses are not size-capped; the sidecar discovery reads now use `json_limited`. | Low | Accepted for the trusted Google endpoint; bound it if the issuer ever becomes configurable. Sidecar discovery JSON is capped. |
| R-03 | JVM entitlements (`allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`) widen the hardened-runtime attack surface. | Medium | Required by the bundled JRE; review with the signing/notarization work. Do not remove blindly. |
| R-04 | Release bundle is ad-hoc signed/not notarized (`TSN-2026-09`). | Medium | Open; needs Developer ID credentials and owner approval. |
| R-05 | Mutable action tags and JitPack coordinates (`TSN-2026-15`). | Low | Closed: actions are SHA-pinned and the JitPack jar/pom bytes are SHA-256-pinned in `scripts/build-sidecar.sh`. |
| R-06 | Google account switching to a *different* identity was not runtime-verified; the same account was re-selected after sign-out. | Info | Verification gap, not a code finding. The flow sends `prompt=select_account`; test with a second account before release. |
| R-07 | Builds/tests ran on the host, not in an OS-enforced sandbox (`sandbox-exec` is available but no profile was established). Exploitability claims are bounded by source review and unit tests, not live exploitation. | Info | Methodology limitation; see `SECURITY_AUDIT.md` §Limitations. |
| R-08 | Sidecar JAR is not runtime-hash-verified (`TSN-2026-13`). | Low | Accepted; record the SHA-256 in release notes and verify at packaging. |
| R-09 | Downloaded files and the library database are protected by normal user file permissions, not app-level encryption. | Low | Accepted: local-first app with no multi-user boundary; FileVault is the OS control. |
| R-10 | Lyrics providers receive track title/artist (and the user's PaxSenix key when selected) over HTTPS. | Low | Privacy disclosure inherent to the feature; document in the privacy note and keep providers optional. |

## 7. Verification matrix

| Check | Result |
|---|---|
| `cargo test --workspace` | **262 passed, 0 failed, 9 ignored** (27 test binaries; re-run 2026-11-19). |
| `cargo test -p sources addon` | 3 URL-policy tests pass, including the new https-downgrade assertion. |
| `cargo test -p tansen radio` | 4 tests pass; the radio source test uses an IP literal to stay offline. |
| `cargo test -p playback` | 28 tests pass, including the new redirect-verdict test and the cache credential-redaction test. |
| `cargo test -p tansen bounded_body` | 3 tests pass, including Content-Length and chunked over-cap rejection. |
| `cargo test -p tansen downloads` | 10 tests pass, including `local_library_path_rejects_untracked_filesystem_paths` and `download_size_cap_rejects_oversized_transfers` (TSN-2026-17/18). |
| `cargo clippy --workspace --all-targets` | Passes with warnings; no denied lint and no compile error (re-run 2026-11-19). |
| `pnpm build` (includes `tsc --noEmit`) | Passes. |
| `pnpm test` | **143 passed of 143** across 30 test files (re-run 2026-11-19; the stale Albums empty-state expectation was removed when `Import folder` left that header). |
| `cargo audit` | **0 vulnerabilities** over 548 crates; 4 informational warnings (`chrono`, `paste`, `proc-macro-error` unmaintained, `glib` unsound). The SQLite advisory is gone with `libsqlite3-sys 0.39.1`. |
| `cargo deny check advisories` | Passes. Two informational unmaintained advisories (`paste`, `proc-macro-error`) are ignored in `deny.toml` with reasons; neither is a vulnerability. |
| `cargo deny check licenses` | Passes against the explicit `[licenses]` allowlist in `deny.toml`. |
| `pnpm audit --audit-level=high` | 0 known vulnerabilities (re-run 2026-11-19). |
| Secret scan | 329 product/report files + `dist` + packaged app + 32 commits; 0 source/history hits; 1 expected binary hit (public desktop client secret); 0 token/private-key patterns. |
| macOS bundle | Ad-hoc/linker-signed; `TeamIdentifier=not set`; `Sealed Resources=none`; `codesign --verify --deep --strict` exit 1; `spctl` exit 1. |
| Runtime Google auth | Real sign-in succeeded earlier in the audit as the owner's Google test account; profile persisted; Keychain item present; session restore, forced refresh, sign-out cleanup, and re-sign-in verified. The app was closed before final report assembly; no restart was performed. |

## 8. Coverage notes

The audit produced a machine-readable ledger of **300 entries** (21 full-file
reviews, 224 targeted reviews, 28 scanned files, 21 binary/generated artifacts,
6 deleted tracked files). It was an audit artifact and is not part of the
repository. Categories A–L were covered
by primary and secondary labels; C (injection), F (secrets/privacy), and G
(cryptography) appear only as secondary categories because no file is solely
about them. `N` is the extra binary-artifact category.

Excluded from the ledger, with reasons in the manifest: generated/reference
trees (`screenshot-to-code/`, `ui-study/`, `ui-reference/`, `ui-replica/`,
`graphify-out/`, `floorplan/`), agent state (`.agents/`, `.freebuff/`,
`.opencode/`, `.zcode/`), installed dependencies (`node_modules/`), build output
(`target/`, `apps/desktop/dist/`), VCS metadata, and the audit deliverables
themselves. `dist` and the packaged app were still secret-scanned.

## 9. Reproduction and references

- `docs/security/SECURITY_AUDIT.md` — architecture, threat model, dependency and
  privacy assessment, scorecard, and limitations.
- Final Rust log: `$TANSEN_AUDIT_DIR/cargo-test-final3.log` (the audit scratch
  directory was a temporary local path, removed from this report).
- `cargo audit` JSON: `$TANSEN_AUDIT_DIR/cargo-audit-final.json`.
- `cargo deny` log: `$TANSEN_AUDIT_DIR/cargo-deny-final.log`.
- Secret scan result: `$TANSEN_AUDIT_DIR/secret-scan-final.json`.
- The 2026-11-17 re-verification was run from the repository root with
  `cargo audit`, `cargo deny check licenses`, `pnpm test`, and `pnpm build`; no
  scratch paths are needed to reproduce it.

No external infrastructure, production account, or real user file was attacked.
All tests were local/adversarial. No commit, push, tag, release, install, or
credential rotation was performed.
