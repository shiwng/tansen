# Tansen — Phase 1 Feasibility Spike (2026-11-04, macOS 27.2, Music 1.7.2)

Method: Design Lab route `/__lab/fullscreen-player` (dev-only, guarded by
`lab-route-grep.test.ts` + `scripts/ui-compare/check-release-lab.sh`) renders
the measured reference geometry (ui-replica/fullscreen-player-reference.md)
with three glass-capsule implementations side by side. Dev app pointed at the
route via a temporary `devUrl` override (reverted after the spike), captured
window-only at 2560×1640 (2×), interiors sampled with the toolkit.
Artifacts: `ui-replica/compare/spike/win-5348.png`.

## a. Translucent sidebar / window material — **CLOSABLE (native)**

tauri 2.12.5 supports `windowEffects` natively: NSVisualEffectView materials
(incl. `sidebar`, `WindowBackground`, `HudWindow`, `FullScreenUI`) inserted
BEHIND the WKWebView, plus **`LiquidGlassRegular`/`LiquidGlassClear`
(macOS 26+)** and `interactive` (macOS 27+) — this host is macOS 27.2, so the
real Liquid Glass material is available at the window layer without any new
dependency. Requires `transparent: true` (applied in tauri.conf.json for the
spike) and no `backgroundColor` (removed). `macOSPrivateApi` is a no-op in
tauri 2.12.1+ — not needed. Spike confirmed: app launches with the vibrancy
layer, no white flash, no crash. Exact sidebar-material pixel match is
measured in Batch 1 on the real sidebar (needs the AppShell sidebar region
made transparent).

## b. Floating glass player capsules — **CLOSABLE (tunable, not a capability gap)**

Reference (Music 1.7.2 full-screen player): capsule 36 pt tall, R 18, interior
`#76777a` over `#5b5c5f` backdrop (+26 lighten), 1 px rim `#f0f1f6` top.

| variant | interior sampled | vs backdrop | vs reference |
|---|---|---|---|
| CSS backdrop-filter blur(18px) sat(1.5) | `#6b6b6f` | +15 | −11 (too dark) |
| glisse (dark theme defaults) | `#3c3c3e` | −32 | wrong variant (dark glass) |
| glisse content-lens ("refract") | `#5b5c5f` | ±0 | invisible over a flat gradient |

Verdicts: the CSS capsule just needs a brighter fill/brightness term (+26 is a
tuning target, not a limit). glisse's dark-theme glass is var-themeable
(`--lg-glass`, `--lg-blur`). glisse refraction over a **flat gradient** adds
nothing (nothing to refract) — its content-lens path matters over busy content
(main window: tile grid behind glass), where on WKWebView it works via
`data-glisse-backdrop` (the live-backdrop Chromium path is correctly UA-gated
off). glisse 0.5.0: MIT, zero deps, ~25 KB gzip, styles auto-injected,
installed and approved by the owner.

## c. Spring / easing — **CLOSABLE (fully)**

`CSS.supports('animation-timing-function', 'linear(0, 1)')` → **true** in this
WKWebView. Real spring curves (Apple-style multi-segment linear()) are
available; cubic-bezier approximations not required.

## d. Scroll feel — NOT YET MEASURED (Batch 1/4)

Rubber-banding is WebKit-native; smoothness with the glass pill over a tile
grid gets measured with the frame-recorder once the shell batch lands.

## e. Context menus — deferred (Batch 3)

Native Tauri menu API vs HTML menus decided when the song lists land. glisse
has a Menu component but it is not portal-positioned (clips inside overflow
containers) — if used, needs an unclipped host layer.

## f. Text rendering parity — deferred (toolkit ready)

Both Music and Tansen render via CoreText; OCR-box diff pending first real
screen comparison.

## Escalation check

No critical item (a/b) is unclosable → no escalation; proceed to the owner
checkpoint and Batch 1.

## Decisions taken this spike (owner-visible)

1. `tauri.conf.json`: `transparent: true`, `backgroundColor` removed,
   `windowEffects: [sidebar]` — required for native vibrancy; permanent.
2. `glisse@0.5.0` added as a runtime dependency — owner-approved (MIT).
3. Design Lab route `/__lab/*` (dev-only, double-guarded) — permanent harness.
4. Temporary `devUrl` override used for the spike — REVERTED.
