# Security Policy

Tansen is a local-first desktop music player. Security is a design constraint,
not an afterthought: the full internal audit (methodology, findings register,
accepted risks, verification matrix) lives in
[docs/security/SECURITY_AUDIT.md](docs/security/SECURITY_AUDIT.md) and
[docs/security/SECURITY_FINDINGS.md](docs/security/SECURITY_FINDINGS.md).

## Supported Versions

| Version | Supported |
|---|---|
| 0.3.x | ✅ |

Earlier builds were release candidates and are not supported.

## Reporting a Vulnerability

**Please do not open a public GitHub issue for a suspected vulnerability.**

Use GitHub's private vulnerability reporting on this repository
(**Security → Report a vulnerability**), which creates a private security
advisory visible only to maintainers. Include:

1. Affected component (crate/module path or file) and version/commit.
2. A reproduction: exact steps, or a minimal proof of concept.
3. Impact as you understand it, including preconditions.
4. Whether it is exploitable from the default local-only configuration
   (remote sources are off by default).

You will get an initial response within 7 days. Please give maintainers a
reasonable window (typically 90 days) to fix and release before public
disclosure; we are happy to coordinate on credit and timing.

## Scope Notes

- **In scope**: the Rust workspace (`crates/`, `apps/desktop/src-tauri/`), the
  frontend bundle (`apps/desktop/src/`), the Kotlin sidecar overlay
  (`sidecar-overlay/`), build scripts, and CI workflows.
- **Out of scope**: vulnerabilities in upstream dependencies that are already
  tracked by RustSec/GitHub advisories without a Tansen-specific angle,
  social engineering of end users, and attacks requiring local malware already
  running as the same user (documented residual risks are listed in the
  register, e.g. R-01, R-03, R-09).
- Keychain is used for all stored credentials; `localStorage` holds
  user-supplied lyrics-provider API keys only (see TSN-2026-14 for the threat
  model around that choice).

## Hardening Invariants for Contributors

- Credentials only in the OS Keychain — never SQLite, logs, events, or frontend state.
- Never log or return raw credential-bearing URLs.
- Validate outbound URLs/redirects via `crates/netguard`; bound HTTP response bodies (`bounded_body.rs`, `json_limited`).
- The audio callback must never block, allocate, lock, or perform I/O.
- The bundled sidecar stays anonymous, loopback-only, and token-gated.
