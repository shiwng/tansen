# Tansen vs Music.app — light mode comparison

Reference: `light-mode-spec.md` (measured). Tansen values are read from source
(`styles/tokens.css`, `shell/Sidebar.tsx`, `shell/Toolbar.tsx`,
`src-tauri/tauri.conf.json`) and from a live capture in light appearance.

## 0. Baseline: Tansen does not render light at all

Live evidence, 2026-11-04, macOS in Light appearance:

| Sample | Tansen | Music (light) |
|---|---|---|
| Sidebar | `#232324` | `#ededec` |
| Content | `#1f1f1f` | `#ffffff` |

Cause: `apps/desktop/src-tauri/tauri.conf.json` sets `"theme": "Dark"` on the
window, so the web view's `prefers-color-scheme` is always dark and the light
branch already present in `tokens.css` never applies. Capture:
`captures/tansen-light-home.png`.

## 1. Deviations (ranked, all measurable)

| # | Item | Music light (measured) | Tansen today (source) | Action |
|---|---|---|---|---|
| 1 | Appearance | follows the OS | forced dark (`theme: "Dark"`) | **M1** remove the pin, verify both branches |
| 2 | Font | system SF Pro Text/Display | bundled `DM Sans` + `Tansen SF Pro Display` WOFF2 | **M1** switch to `-apple-system, system-ui`, drop both `@font-face` blocks and the `.woff2` files |
| 3 | Toolbar height | 52 | 60 (`h-[60px]`) | **M3** |
| 4 | Search field | 421 × 38, centred in the **content column**, radius 19, hairline border, blue 3 pt focus ring | 600 × 32, centred in the window | **M3** |
| 5 | Scope control | 260 × 38 outer pill, 1 pt border, selected segment on a `#e3e3e3` pill, 8 pt from the window edge | 24 pt pills on a `bg-segmented` track, `right-5` | **M3** |
| 6 | Toolbar modes | two: global search+scope (Search/Browse) vs compact "Find in X" (library pages) | one global toolbar on every page | **M3** (behaviour change — confirm with owner) |
| 7 | Sidebar rows | 32 tall, no 1 px gap, radius 8, pill inset 10/10 | 28 (`h-7` + `mb-px`), radius 6, no inset | **M2** |
| 8 | Sidebar label x | 54 | 48 (12 pad + 8 + 20 icon + 8 gap) | **M2** |
| 9 | Sidebar icon | 16 pt, accent blue, centred at x 30 | 20 pt, `--keyColor` tint | **M2** |
| 10 | Group header | 11 pt regular, `#878786`, x 15, 13 above / 16 below | 12 pt semibold, x 20 | **M2** |
| 11 | Sidebar material | `#ededec` vibrancy over wallpaper | `.glass-material` (#f5f5f78c light token) — likely close once light is enabled; re-measure | **M1** |
| 12 | Selection | `#d7d7d6` | `--navSidebarSelectedState` #3c3c431a | **M1** compare after light is on |
| 13 | Hover in sidebar | none | `hover:bg-quinary` | **M2** remove if Music truly has none (measured 0 px) |
| 14 | Pill | 700 × 54, x 394, bottom 19, `#fefefe`, radius 27, no border | 600 × 50, bottom 20 | **M7** |
| 15 | Pill disabled glyphs | `#bcbcbc` | not measured in Tansen | **M7** |
| 16 | Empty states | glyph + 17 pt gray title + 13 pt subtitle + bordered 148 × 26 button | library pages use their own copy | **M5** |
| 17 | Accent | `#3478f6` | `--keyColor` #d60017 (red) in light tokens | **M1/M9** decide: keep Tansen's own accent or match the OS accent |
| 18 | Settings | 650 pt sheet, 4 icon tabs, grouped form, 26 pt Cancel/OK | in-page route, six sections | **M9** (layout only; Tansen's page stays, no sheet) |

## 2. Deliberately not copied

* Apple wordmark, "Apple Music" / "iTunes Store" strings, store banners, category
  artwork, the account footer, the subscription call to action.
* Tansen's scope names come from its real sources (Local library + YouTube Music),
  so the control has 2 segments, not 3. Do not invent a third.
* The `Browse …` empty-state button must point at Tansen's own search, not a store.

## 3. Milestones

1. **M1 — light tokens + window.** Unpin the window theme, verify every token in
   light, remove the bundled fonts, fix `.glass-material` for light, re-sample.
2. **M2 — sidebar.** 32 pt rows, 10 pt pill inset, radius 8, 54 pt labels,
   16 pt accent icons, 11 pt gray headers, no hover fill.
3. **M3 — toolbar + search.** 52 pt, 421 × 38 field centred in the content
   column, two toolbar modes, scope pill with hairline border.
4. **M4 — Home + Browse.** Real data only; no artwork placeholders.
5. **M5 — library pages + song tables.** Empty state composition, row metrics.
6. **M6 — album / artist / playlist.** Blocked until Music has content to measure.
7. **M7 — player pill.** 700 × 54 at 19 pt, control rhythm, disabled colours.
8. **M8 — queue + lyrics.** Blocked on measurable reference.
9. **M9 — menus, popovers, settings.**
10. **M10 — motion, a11y, polish.**

## 4. Verification recipe (per milestone)

1. `bash scripts/ui-compare/window-set.sh <tansen-pid> 80 60 1280 820` and the
   same for Music (`2445`) — identical frame, both in light.
2. Same screen and state in both; capture with `scripts/ui-compare/capture.sh`.
3. Compare: `pixel_tools.py edges` for edges, `color` for surfaces,
   `ssim a.png b.png out.png mask.png` for the overall diff (mask the sidebar
   footer and any account name).
4. Record numbers, not adjectives. "Looks identical" is not a result.
