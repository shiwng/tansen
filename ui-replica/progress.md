# Tansen UI replica — progress

Milestone status, inventory and known deviations. Reference measurements live in
`reference.md` (M1), final tokens in `tokens.md` (M3), REA notes in
`study-notes.md` (M1).

## M0 — gate + inventory (read-only)

### Environment (measured)

| Item | Value | How |
|---|---|---|
| OS | macOS 27.0.1 (build 26A434) | `sw_vers` |
| Music.app | 1.7 | `defaults read /System/Applications/Music.app/Contents/Info CFBundleShortVersionString` |
| Design generation | UNKNOWN — must be read from a real screenshot (glass/material era) | blocked, see permissions |
| Tauri | crate `tauri = "2"`, `@tauri-apps/api` 2.12.4 | Cargo.toml, node_modules |
| Frontend | React 19, react-router 8, zustand 5, TanStack Query 5 | package.json |
| Styling | Tailwind CSS 4 via `@tailwindcss/vite`; 184 CSS custom properties in `styles/tokens.css` (280 lines); `styles/global.css` 414 lines | grep |
| Icons | hand-authored SVG (`NavIcons.tsx`, `Transport.tsx`), `lucide-react` 0.545 for three glyphs (Radio, Settings, ListMusic) | source |
| Tests | vitest 5 + happy-dom, 12 files / 63 tests; `tsc --noEmit`; `vite build` | package.json |

### Permissions (blocker for M1 measurement)

| Capability | State | Evidence |
|---|---|---|
| Screen Recording | **DENIED** | `screencapture -x /tmp/_permtest.png` → `could not create image from display` |
| Accessibility (AX element tree) | **DENIED** | `osascript … process "tansen" … UI element` → `osascript is not allowed assistive access. (-1719)`; process listing works, element access does not |

Owner action to unblock M1: System Settings → Privacy & Security → Screen
Recording → enable for the terminal/host running this agent, then quit and
relaunch it. Same for Accessibility if AX frame measurement is wanted.
Alternative: place PNGs in `ui-reference/` named
`<screen>-<light|dark>-<state>.png`, plus a note of the window size.

Without screenshots there is no `reference.md`, no pixel measurement and no
comparison metrics. Visual review itself is possible for this agent (images can
be read), but there must be images.

### Gate: playback + covers (mission depends on both)

| Check | Result | Evidence |
|---|---|---|
| Real Play button produces sound | **PASS** (owner-confirmed; 2026-11-04 run) | log: 20× `remote track upsert` → `cmd=ClearQueue` → `cmd=Enqueue` → `cmd=Play`; stream reads 0→122,138,589, `stream_errors=0`; `pmset` audio-out assertion |
| Engine state reaches `Playing` with frames advancing | PARTIAL | `Playing` implied by the audio path; the engine's frame counter is not exposed outside the debug E2E driver — audio-read growth used as proxy |
| Covers render everywhere | **PASS** (owner-confirmed) | 126 cached covers in `~/Library/Application Support/tansen/art/remote`; DOM `naturalWidth` NOT MEASURED (no devtools in the WKWebView) |

Gate passes on owner-observed behaviour; the two DOM-level proofs (frames,
`naturalWidth`) stay NOT RUN with the reasons above.

### Frontend inventory

Shell (`src/shell/`): `AppShell` (sidebar + scrolling `main#scrollable-page` +
floating `PlayerBar` pill + lazy `SidePanel` + lazy `FullScreenPlayer`),
`Sidebar`, `MobileTopBar` (<484px drawer), `Lcd` (now-playing display),
`useScrubber`, `VolumeControl`, `UpNext`, `LyricsPanel`, `MeshBackdrop`,
`useKeyboardShortcuts`. No toolbar, no traffic lights: Tauri renders the default
window title bar. `tauri.conf.json` sets no `titleBarStyle`, `hiddenTitle`,
`transparent`, `windowEffects` or `macOSPrivateApi`; window 1280×820, min
960×620, `theme: "Dark"`.

Routes (`pageRegistry.tsx`): `/home`, `/new`, `/search`, `/album/:id`,
`/artist/:id`, `/playlist/:id`, `/playlist/:id/reorder`, `/library/:section`,
`/settings`, `/discover`, `/radio`; unknown → `NotFoundPage`.

Sidebar items (current): Search, Home, Radio · Library: Recently Added
Artists/Albums/Songs · Settings · Playlists: All Playlists + playlists ·
"Account" footer text.

Stores: `state/player.ts` (queue mirror, status, now-playing; IPC via
`engine_command`/`playback_state`), `state/ui.ts` (side panel, full screen,
drawer, install banner), `state/video.ts`.

Data/backed surfaces (existing IPC only): `discovery_home`,
`discovery_explore`, `discovery_search`, `discovery_suggest`,
`discovery_continuation`, `discovery_album/artist/playlist`, `discovery_radio`,
`playlists_get`, `playlist_*`, `tracks_get`, `albums_get`, `artists_get`,
`folders_get`, `folder_add/remove`, `scan_start`, `settings_*`, `quality_*`,
`autoplay_*`, `sources_get_with_health`, `source_*`, `subsonic_add`,
`fetch_artwork`, `stream_info_get`, `engine_command`.

### Screen → Apple Music map, and gaps

| Apple Music (macOS) surface | Tansen today | Backend? |
|---|---|---|
| Window chrome, traffic lights, toolbar with back/forward + centred now-playing | none: default Tauri title bar, no toolbar, no history buttons | UI only |
| Sidebar (Search, Listen Now, Browse, Radio; Library; Playlists) | sidebar with Search/Home/Radio + recently-added library + playlists; no Listen Now/Browse | Search/Home/Radio yes; Listen Now personalisation + Browse **none** |
| Now-playing bar | floating pill below content | yes |
| Song list (album/playlist) | `TrackList` with index→play, hover, explicit badge, duration | yes |
| Album / playlist / artist pages | `AlbumPage`, `PlaylistPage`, `ArtistPage` (remote albums/artists/playlists + local) | yes |
| Search with suggestions + filters + top result | `SearchPage` + typeahead, filter tabs, load-more | yes |
| Queue (Up Next) panel | `UpNext` side panel, remove/reorder | yes |
| Lyrics panel | `LyricsPanel` (timing currently broken: still reads retired `getAudio()`) | yes (providers) |
| Full-screen player | `FullScreenPlayer` | yes |
| Music videos | none | **none** |
| Store / Apple account / cloud library / Connect / AirPlay picker | none | **none** — hidden, never mocked |
| Listen Now personalisation | none | **none** — `HomePage` shows anonymous Home/Explore shelves instead |

Rule for every gap: hide the element or show an honest empty state; never mock.

### Plan (one milestone at a time, owner says "continue" after each)

| M | Scope | Files it will touch (expected) |
|---|---|---|
| M1 | Reference capture + REA study | `ui-reference/`, `ui-replica/reference.md`, `study-notes.md`, `scripts/ui-compare/` |
| M2 | Window chrome: overlay title bar, hidden title, native traffic lights, drag regions, sidebar material, inactive state, min/default size | `tauri.conf.json`, `main.rs` window setup (window options only), `AppShell`, sidebar CSS |
| M3 | Tokens + type scale + base controls from measurements | `styles/tokens.css`, `global.css`, `Menu`, `SearchField`, controls; dev-only component sheet route |
| M4 | Sidebar + navigation + history | `Sidebar.tsx`, `PlayerBar`/toolbar host, routes |
| M5 | Toolbar + now-playing display | `Lcd.tsx`, `PlayerBar.tsx`, `useScrubber.ts` |
| M6 | Song list rows/columns/states | `TrackList.tsx` |
| M7 | Album/playlist/artist pages | `AlbumPage`, `PlaylistPage`, `ArtistPage`, `DetailHeader` |
| M8 | Search | `SearchPage`, `SearchField` |
| M9 | Queue + lyrics panels | `UpNext`, `SidePanel`, `LyricsPanel` |
| M10 | Home/Browse shelves | `HomePage`, `ShelfView`, `Lockups` |
| M11 | Interaction + accessibility polish | shell-wide |
| M12 | Full audit + performance + deviations report | all |

### Known deviations / risks

- WebView (WKWebView) cannot reproduce native vibrancy/glass blur exactly;
  `backdrop-filter` is an approximation. Size of the gap measured at M2/M12.
- Custom window effects in Tauri 2 may require `macOSPrivateApi` (blocks Mac App
  Store distribution). Verified against the Tauri docs before use; if required,
  the trade-off goes to the owner as a decision, not a silent choice.
- `useKeyboardShortcuts` must be reconciled with Music.app's real menu
  shortcuts at M11 (read from the menu bar, not from memory).
- Every reference screenshot stays in the gitignored `ui-reference/` and
  `ui-replica/compare/`; no Apple asset, font or icon is ever copied.

## M1 — reference capture + REA (started 2026-11-04)

Unblocked: Screen Recording and Accessibility now work (`screencapture` returns
a 2560×1640 PNG; System Events reads Music's AX tree).

Captured (2×, window 1280×820 at (80,60)): Home dark, Home light.
Measured: `reference.md` (geometry from AX, colours from pixels). Proposed
tokens: `tokens.md`. Study: `study-notes.md` (Musish AGPL-3.0, tauri-controls
and Cider-2 unlicensed — concepts only, no code).

Blocked states: Search, album/playlist/artist, queue, lyrics, playing/paused,
hover/selected rows, minimum size, inactive window. `AXPress` on Music's
sidebar rows returns `missing value`, and there is no library content on this
account to open. Typography not measured yet (needs the overlay method).

Open risks: sidebar/capsule colours are vibrancy samples, so tokens need
material tint + blur, and comparisons must tolerate material variance.

## U0–U4 — browse/search screen restyle (2026-11-04, second pass)

Reference: the owner's attached dark "Search / Browse" screenshot. The expected
file `ui-reference/01-search-browse-dark.png` is **missing**, so pixel
measurement, the diff heatmap and SSIM are NOT RUN. Values below are read from
the attached image by eye and cross-checked against the earlier AX-measured
Apple Music numbers in `reference.md`; exact-pixel verification stays pending.

Applied in this pass:

- Window chrome: `titleBarStyle: Overlay`, `hiddenTitle: true`,
  `trafficLightPosition {18,18}`, `backgroundColor #1f1f22`; sidebar content
  inset 52 pt below the lights. No title strip.
- Sidebar: 208 pt wide, rows 28 pt / radius 6 pt, accent-tinted 20 px glyphs,
  groups Search·Home·Radio / Library (Recently Added, Artists, Albums, Songs) /
  Tansen (Settings) / Playlists (All Playlists). Account footer removed.
- Toolbar: centred search field (600 pt max, 32 pt tall, dark fill, key-colour
  focus ring) and a capsule scope control listing the real enabled sources plus
  the Library scope, wired to `/search?q=&scope=`.
- Browse page: idle `/search` shows the Browse Categories grid (5 columns,
  16:9, radius 8, 17 pt gaps) and a real-notice banner (solid accent blue, mark,
  divider, two lines, white action) shown only when no catalogue source is
  enabled. Tiles are genre shortcuts that run real searches — no catalogue
  artwork, no fabricated data.
- Player: pill narrowed to 600 pt, height 50 pt, glass material retained.
- Gutter 29 pt.

Deviations / gaps: Settings page not restyled yet (functional, unstyled against
the new tokens); promo banner has no Apple-style promo copy by design; category
tiles are shortcuts, not a real categories endpoint; exact reference measurement
and the visual comparison are pending (screen capture unavailable to the agent
host: `screencapture` -> "could not create image from display").

## Part A — Settings crash fixed (2026-11-04)

Cause: `Toolbar`'s `sources_get` and `SettingsPage`'s
`sources_get_with_health` shared the React Query key `['sources']`, so Settings
could receive rows without `health` and throw on `health.state`. Key split
(`['sources','config']`), health treated as optional with an honest
"Status unavailable" label, and every Settings section now sits in the existing
`AppError` boundary (new inline mode) with Retry. Regression tests: health
absent, rejected reason, inline boundary. Installed-app check passed
(Settings opens, Shuffle persists across relaunch, search returns real results
with covers). Part B milestones not started.

## Phase 0 + 1 — capability gate and toolkit (2026-11-04)

Phase 0 PASSED. Images can be read; window capture returns 2560×1640 (2×) PNGs and
the AX tree of Music reads normally (earlier sessions recorded both as denied).
Environment: macOS 27.2 (26B123), Music 1.7.2, 2560×1664 built-in Retina at 2×,
dark, default accent. Tansen baseline re-verified: installed app opens Settings,
search returns real YouTube Music rows with covers (screenshots in the Part A pass).

Phase 1 built `scripts/ui-compare/` (find-window, capture with settle, ax-dump,
ocr-boxes, window-set, input, pixel_tools) and validated it against Tansen's known
CSS: sidebar 208 pt measured 208.0 pt; pill 50 pt measured 49.0 pt; flat colour
sampling stable within 1 level; OCR aligned to AX text within 1 pt. See
`ui-replica/toolkit.md`. Motion recording NOT BUILT: no ffmpeg; a ScreenCaptureKit
recorder is the Phase-4 plan.

First live recon in `ui-replica/screen-inventory.md`: Music's toolbar is 52 pt, the
search field is 421×38 centred in the content area (not the window), sidebar rows
are 32 pt, group headers 19 pt, and the player pill is 700×54 with a 19 pt bottom
inset. Tansen currently differs: 29 pt rows, 600×32 search field centred in the
window, 600×50 pill at 20 pt. These become S1–S5 work.

Blocking item for typography: Tansen still bundles DM Sans **and** SF Pro Display
WOFF2 and uses them for UI and lyrics. The brief forbids bundling SF, and Music
renders SF Pro Text; S1 must switch the UI stack to `-apple-system, system-ui` and
remove the bundled faces (lyrics font decision needed).

## Phase 2 — light-mode research (2026-11-04)

Owner switched macOS to Light appearance; Music 1.7.2 re-measured from scratch.
No app code changed in this phase.

### Produced

- `ui-replica/light-mode-spec.md` — full measured light spec (geometry, colour,
  typography, components, states, what may and may not be copied).
- `ui-replica/light-mode-comparison.md` — 18 ranked Tansen deviations + the
  10 milestones (M1 light tokens/window … M10 motion) + verification recipe.
- `ui-replica/light-motion.md` — honest "NOT MEASURED" note, method, and what a
  recorder must capture. No timings invented.
- `ui-replica/screen-inventory.md` — light-mode section with window-relative
  frames and the new capture list.
- Toolkit additions: `scripts/ui-compare/sheet.py` (contact sheets),
  `scripts/ui-compare/ui-click.sh` (window-relative click that activates the app
  first — plain clicks on a background window only focus it), `input.swift type`
  (kept: unicode typing is unreliable in Music; use `System Events` keystroke),
  `pixel_tools.py dark <img> x y w h` (glyph-core colour in a box).

### Headline numbers (light)

Content `#ffffff`, sidebar `#ededec` (vibrancy), selection pill `#d7d7d6` at
x 10→197.5 in a 32 pt row, group headers 11 pt `#878786` at x 15, labels 13 pt
at x 54; toolbar 52; search field 421×38 at x 536 (centred in the content
column) with a blue focus ring; scope 260×38 with its right edge 8 pt from the
window edge; content starts at x 208 with **no divider line**; pill 700×54 at
x 394, bottom 19, `#fefefe`, radius 27; empty-state text `#808080`, button
148×26 `#242424`; accent `#3478f6`; settings sheet 650×436, separators `#e5e5e5`.

### Findings that change the plan

1. **Tansen cannot render light today**: `tauri.conf.json` pins
   `"theme": "Dark"`, so the light branch in `tokens.css` is dead code. M1 must
   remove the pin first, then re-measure everything.
2. Music has **two toolbar modes** (global search+scope on Search/Browse, compact
   "Find in page" on library pages). Tansen has one. Adopting both is a behaviour
   change and needs the owner's call.
3. The sidebar has **no hover state** (0-pixel diff), so Tansen's
   `hover:bg-quinary` on nav rows is a deviation, not a fidelity gain.
4. Light mode is not the dark palette inverted: content is the *brightest*
   surface (`#ffffff`) and the sidebar is darker (`#ededec`).

## M1 — theme unpin + system fonts (2026-11-04)

Code state (already in the working tree from the earlier pass, verified this
session): `tauri.conf.json` no longer pins `"theme": "Dark"` — the window
follows the macOS appearance, so `prefers-color-scheme: dark` in tokens.css is
live; tokens.css is light-first with `color-scheme` per branch; the bundled
`dm-sans-variable-latin.woff2`, `sf-pro-display-bold.woff2` and
`sf-pro-display-regular.woff2` are deleted, `assets/fonts/` is gone, the
`@font-face` block was removed from global.css, and both the UI
(`--font-family`) and lyrics (`LYRIC_FONT`) use `-apple-system, system-ui`.

Checks run this session: `tsc --noEmit` 0 errors; vitest 15 files / 70 tests
pass (lyrics test logs a 504/CORS warning against the real network — test still
passes; two Vite servers exit slowly, known vitest noise); `cargo check
--workspace` clean (5 pre-existing warnings). `pnpm tauri dev` launched, page
load Finished confirmed in the log.

BLOCKED: pixel verification. Screen Recording is denied to the current agent
host — `screencapture -x /tmp/_permtest.png` → `could not create image from
display`, and window capture fails for both the dev Tansen and Music. Same
regression as the U0–U4 note. Owner action: re-grant Screen Recording to the
app hosting this agent, then relaunch it. Until then light-mode colour/font
verification stays PENDING OWNER.

Known deviation deferred to M2: `backgroundColor: "#1f1f22"` in
tauri.conf.json is static, so a light-mode launch shows a dark flash before
CSS paints. Runtime appearance-aware background needs M2 window work.

Next: M2 window chrome (overlay title bar behaviour, traffic-light inset,
drag regions, min/default size, launch background per appearance). Stop for
`continue` after the M1 report.

## UI REPLICA v3 — Phase 1 feasibility spike (2026-11-04, branch ui-replica)

Branch `ui-replica` created per brief rule 6; baseline commit d325d06 = full
working-tree snapshot (engine M10 + security run-2 remediation); M0 commit
ba714fa = spike. Music 1.7.2, macOS 27.2, capture permission WORKING.

Key facts (details in ui-replica/feasibility.md):
- tauri 2.12.5 supports native windowEffects incl. LiquidGlassRegular/Clear
  (macOS 26+) and `interactive` (27+) — applied `transparent: true` +
  `windowEffects: [sidebar]`, removed `backgroundColor`. macOSPrivateApi not
  needed (no-op since 2.12.1). Traffic lights cannot be hidden via config
  (tao supports it; a ~10-line objc2 addition to platform_macos.rs would).
- glisse 0.5.0 installed (owner-approved, MIT, zero deps, ~25 KB gzip). On
  WKWebView: frost + lit rim for arbitrary backdrops, FULL content-lens
  refraction for content marked data-glisse-backdrop (live-backdrop path is
  Chromium-only, UA-gated). Over a flat gradient refraction adds nothing —
  the player capsules are a tuning problem (CSS +15 vs reference +26 lighten;
  rim dimmer), not a capability gap.
- linear() easing SUPPORTED in this WKWebView → real spring curves.
- Reference: owner-supplied full-screen player screenshot measured to
  ui-replica/fullscreen-player-reference.md (all geometry in pt; capsules
  36/R18, artwork 262.5, transport centers, gradient bg #5c5d60→#5a5b5e).
- Design Lab: /__lab/* dev-only, double-guarded (vitest grep test +
  scripts/ui-compare/check-release-lab.sh on dist; both green).

Blockers: Music.app AX reads denied to the host until ZCode restart
(granted but not trusted; osascript -1719) → live AX-driven capture recipes
BLOCKED; capture-only works (base state captured). Escalation: none (no
critical spike item unclosable).

Next (after owner "continue"): Batch 1 foundation (tokens, window chrome,
native-feel hygiene, sidebar vibrancy region), live captures once the host
restart restores AX, then Batch 2 shell with the glass player.

## Batch 1 — FOUNDATION (2026-11-04, commit 59a91cb on ui-replica)

Theme system: tokens now attribute-driven — `:root[data-theme='dark']`
replaces the dark media query; inline bootstrap in index.html sets the
attribute before first paint; `shell/chrome.ts` (new) syncs it with the OS
(follow-system, like Music), listens `tauri://focus`/`tauri://blur` for the
inactive state (`data-window-inactive` → `.chrome-dim` at 0.6), and guards
the default context menu in PROD builds only. Both token branches verified
by captures without touching the owner's appearance: dark (system) + forced
light (temporary sync suspend, reverted). glisse + tokens now agree on
data-theme.

Vibrancy: body transparent; main content column owns `--pageBG`
(`bg-page bg-clip-content` — the bg-clip-content is essential: the column is
inset-0 and would otherwise paint over the sidebar). Sidebar dropped
`glass-material` — native Sidebar material shows (#444446 sampled over the
current wallpaper, wallpaper-dependent; boundary at x208 clean). Found and
fixed during verification: solid dark selection #434347 was invisible on
today's material — replaced with white-alpha overlay #ffffff1c (the correct
model over varying vibrancy; renders #59595a, verified).

Chrome: drag regions on Toolbar root + a 52 pt sidebar strip
(`data-tauri-drag-region`; `core:window:allow-start-dragging` added to
capabilities); hygiene: select-none on the three chrome roots, arrow cursor
on buttons/links/scrubbers (Music-native), img user-drag none, tap-highlight
global, PROD-only context-menu guard (text fields exempt).

Verified: dark branch (sidebar #444446 / content #1f1f22 / pill #212121 /
selection #59595a), light branch (content #ffffff / pill #fefefe), inactive
dim (glyph #f0f0f0→#9f9fa0 + material #444446→#28282b native), gates (vitest
16/72, tsc 0, build ✓, release-lab gate ✓, cargo check ✓).

NOT RUN / deferred (blockers + reasons):
- Drag + selection + interactive regression (search/Play/settings): synthetic
  input blocked — CGEvent post and AX both untrusted until the owner
  restarts the host (grant is ON; trust cache stale). input.swift gained a
  `drag` subcommand for next time.
- Material/page theme coupling: vibrancy follows the OS; under the app's
  follow-OS design they always pair. If an in-app theme override is ever
  wanted, NSWindow appearance sync becomes a new item.
- Light sidebar selection exactness: alpha over wallpaper-dependent material;
  logged, not guessed.
- Viewport zoom lock: omitted (no demonstrated requirement).
- Separator #e5e5e5 refinement: omitted (systemGray5 is Apple's real palette
  value; consumer split not established).
- Batch-2 geometry tokens: deliberately not added (consumed when sidebar/
  toolbar/pill geometry lands).

## ADDENDUM + CHANGE ORDER 1 absorbed (2026-11-04)

Addendum results (all measured, artifacts in ui-replica/compare/addendum/):
1. MATERIAL PROOF: PASS. Citations — LiquidGlassRegular/Clear: tauri-utils
   2.10.2 src/lib.rs:118-121; applied via window-vibrancy 0.8.1
   src/macos/liquid_glass.rs (NSGlassEffectView, behind the webview);
   `interactive` macOS 27+: tauri-utils config.rs:1892-1896;
   macOSPrivateApi no-op: tauri-utils config.rs:3375-3379 ("No-op in Tauri
   2.12.1+"). Liveness: sidebar material sampled #444446 at (215,165),
   #28282b at (80,60), #28282b inactive — changes with position and state.
   A/B at Music's exact position (80,60), dark, same wallpaper: sidebar
   #28282b vs Music #262628 (Δ2 — WINNER); hudWindow #282829 (Δ2, wrong
   semantics); LiquidGlassRegular #202020 (Δ6); windowBackground #1f1f20
   (Δ7); fullScreenUI #28282b (Δ2, ties sidebar). Kept: sidebar material.
   Corner radius/shadow: window-only captures exclude shadows (UNKNOWN —
   needs screen-region capture, forbidden); radius/traffic-light positions
   pending pixel fit (Batch 2). Theme pin: mechanism exists (Tauri window
   theme setting) but "stays dark under a light system" needs the system in
   light = OWNER SESSION. UNKNOWN.
2. GLASS PILL: CSS-only tuned wins. Music re-measured separately: main pill
   (fresh capture, dark) fill #2a2a2a over content #1f1f20 (+11 → alpha
   model, token fixed to #ffffff0d); fullscreen capsule from the owner's
   screenshot (36/R18, +26, rim #f0f1f6). Four-backdrop A/B (dark/mid/light/
   busy tile grid + refraction probe line): CSS lift +54/+21/light/blurred
   mix — all correct; glisse pass-through on this WKWebView (interior =
   backdrop exactly, sharp stripes on busy — no frost/fill/rim) after 3
   tuning attempts (style, className, wrapper). DECISION: glisse REMOVED
   (package.json + lockfile + wrapper + imports; dist grep 0). CSS-only is
   the glass implementation; parameters tunable per surface in WS-C.
3. SCROLL/PERF: Lab /__lab/scroll built (200 tiles, fixed pill, rAF-gap
   harness + on-screen report). Runs pending the input-event unblock for
   scripted verification; harness itself runs on open. glisse decision made
   moot by removal.
4. GLISSE VETTING: 0kzh <me@kelvinzhang.ca>; 0.5.0 published 2026-10-30 (5
   days old at install — below the 14-day preference, flagged); MIT; no
   install/postinstall scripts; lockfile pin glisse@0.5.0; zero runtime
   deps; styles via JS <style> injection (CSP style-src 'unsafe-inline'
   already required). pnpm audit: no known vulnerabilities. REMOVED anyway
   per the measured decision rule; NOTICE reverted (dependency gone).
5. TEXT PARITY: Lab /__lab/text built (sidebar strings at measured
   geometry). OCR diff pending live Music capture with AX (input events
   blocked until host restart). UNKNOWN → owner session.
6. REFERENCE RECONCILIATION: ui-reference/01-search-browse-dark.png DOES
   NOT EXIST (never delivered) — marked obsolete/missing. Fresh Music
   captures live in ui-reference/captures/ (music-base-state.png) and
   compare/addendum/ (music-main-fresh.png).
7. GIT/RELEASE HYGIENE: tauri.conf.json diff vs ed3c797 = exactly
   (−theme Dark, +transparent, +windowEffects sidebar, +devCsp, +macOS
   bundle keys, +jre resource); devUrl override gone (diff clean); CSP
   content unchanged; lab gates green; release build + launch check pending
   (batch end).

CHANGE ORDER 1 mapping:
- R1 live appearance: data-theme + matchMedia sync (B1) — live switching
  VERIFIED via forced-light capture; owner toggle pending. No forced dark
  anywhere (grep clean; tauri theme key removed). No-flash bootstrap in
  index.html.
- R2 accent: dark keyColor family switched pink→blue #0a84ff (INFERRED from
  macOS systemBlue dark — live verify needs a selected Music row in the
  owner session). Measured today: Music's UNSELECTED sidebar icons are gray
  (#464648), not accent — accent applies to selected/active; matches R2's
  intent.
- R3/R4/R5/R6 (WS-B/C/D/E): queued in the brief's order WS-A→C→B→D→E.
  WS-A is ~done (B1 + this addendum); WS-C glass parameters next
  (CSS-only), then WS-B fonts/icons/alignment, WS-D scroll gates, WS-E
  Settings matrix.
- glisse decision: REMOVED — satisfies the dependency-review default (no
  measured gap after tuning; pass-through on WebKit). Recorded here as the
  approval reference trail (owner added it in Phase 1; removed on measured
  evidence per Change Order 1's own default rule).
- OWNER SESSION checklist (one request, when WS-A verification runs):
  static wallpaper + windows open; Light→Dark toggle; accent change;
  Reduce Transparency; Reduce Motion.

Gates after all changes: vitest 16/72, tsc 0, build ✓, dist glisse-grep 0,
cargo check ✓ (earlier this session). Dev window state: Music restored
(untouched); Tansen dev instance running for the next milestone.
