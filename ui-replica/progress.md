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
