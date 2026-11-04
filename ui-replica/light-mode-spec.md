# Music.app 1.7.2 — light mode specification

Measured live on this machine. **All geometry below is window-relative points**
(origin = window top-left, y down), which is what a Tauri web view also uses.
Pixel colours are sRGB, sampled from 2× window captures; every value is the
median of a flat region, not a guess.

> Correction to earlier notes: AX frames are *screen* points. The sidebar rows
> that the Phase-1 notes listed at y=112 are at **y=52 inside the window**.
> Everything in this file has been converted.

## 1. Environment (measured)

| Item | Value |
|---|---|
| macOS | 27.2 (build 26B123) |
| Music.app | 1.7.2 |
| Appearance | **Light** (`AppleInterfaceStyle` unset) |
| Accent | default blue, measured `#3478f6` (OK button fill), `#3375f0` (checkbox) |
| Display | built-in Retina, 2560×1664, 2× |
| Window | 1280×820 at screen (80,60); min size not yet probed |
| Window frame | unified: traffic lights inside the content, 16 pt at (18,18)/(41,18)/(64,18) |
| Capture | `screencapture -o -l<id>` → 2560×1640 PNG = exactly 2× points |

## 2. Screen map (what exists, light mode)

| Screen | Reachable | Reference capture |
|---|---|---|
| Home (Listen Now) | yes | `captures/music-light-home.png` |
| Browse (search page, scope=Apple Music) | yes | `captures/music-light-search-page.png` |
| Search: focused + suggestions | yes, typed query `u2` | `captures/music-light-search-suggest.png` |
| Search: full results | yes | `captures/music-light-search-results.png` |
| Radio | yes | `captures/music-light-radio.png` |
| Recently Added / Artists / Albums / Songs / Playlists | yes (empty state) | `captures/music-light-{recent,artists,albums,songs,playlists}.png` |
| Settings sheet, 4 tabs | yes, read-only, closed with **Cancel** | `captures/music-light-settings-{general,playback,files,advanced}.png` |
| Album / artist / playlist detail, queue, lyrics, now-playing, mini player | **no** — needs library content or a subscription | — |

No library content is installed in Music, so every library page renders the same
empty state and no track-level UI is reachable. That is why the row, queue,
lyrics and now-playing specs are absent rather than guessed.

## 3. Geometry

### Frame
| Element | Points (x, y, w, h) |
|---|---|
| Window | 1280 × 820 |
| Sidebar | x 0, y 52 → 763; width **208** |
| Toolbar | y 0, h **52** (above sidebar and content) |
| Content | x **208**, y 52, w **1072**, h 711 |
| Player pill | w **700**, h **54**, x **394** (centred in the content column: 208 + (1072−700)/2 = 394), y 747, bottom inset **19** |
| Traffic lights | 16 × 16 at (18,18), (41,18), (64,18) |

### Sidebar
| Element | Value |
|---|---|
| Row height | **32** (outline y 52 → 763, rows from 52) |
| Row hit area | full 208 width |
| Selected pill | x **10 → 197.5** (10 pt inset each side), full row height 32, radius **8** |
| Row label x | **54** (OCR 108 device px) |
| Row icon | 16 × 16, centred at x ≈ 30 |
| Group header height | **19** |
| Header label x | **15** (OCR 14.5) |
| Gaps | 13 above and 16 below each group header (rows at 52/84/116 · header 161 · rows 180/212/244/276) |
| Groups | (none) Search, Home, Radio · **Library** · **Store** · **Playlists** |

### Library pages
| Element | Value |
|---|---|
| Empty-state glyph | centred, ≈ 60 × 60, y ≈ 380 |
| Title | `Add Music to Your Library`, 17 pt semibold, top y **414** (OCR) |
| Subtitle | top y **456** (OCR), 13 pt |
| Button | `Browse Apple Music`, **148 × 26** at x **670**, y **483** (AX), radius 13; centred on the content column (670 + 74 = 744) |

### Toolbar, two modes
1. **Search / Browse**: centred search field **421 × 38** at x 536 (centred in the
   content column: 208 + (1072−421)/2 = 533.5, measured 536 → +2.5), radius 19;
   scope control **260 × 38** at x 1012 → its right edge is **8 pt** from the
   window edge (AX: abs x 1092, window x 80, width 260, window width 1280).
2. **Library pages**: no global search field and no scope control; a compact
   trailing "Find in <page>" control sits at the right of the toolbar instead.

### Player pill controls (AX, points, window-relative)
| Control | Frame (x is window-relative) |
|---|---|
| Shuffle | 28 × 28 at x 403, y 760 |
| Previous | 28 × 28 at x 431, y 760 |
| Play/Pause | 36 × 36 at x 459, y 756 |
| Next | 28 × 28 at x 495, y 760 |
| Repeat | 28 × 28 at x 523, y 760 |
| LCD / now-playing | 406 × 54 at x 560, y 747 |
| Lyrics | 36 × 36 at x 975, y 756 |
| Playing Next | 36 × 36 at x 1012, y 756 |
| Volume | 36 × 36 at x 1049, y 756 |

Pill spans x 394 → 1094 (700 wide). Raw AX is in `screen-inventory.md`.

## 4. Colour (measured, light)

| Token | Value | Where measured |
|---|---|---|
| Content background | `#ffffff` | flat run 416→2560 device px |
| Sidebar background | `#ededec` | flat run 72→415; **vibrancy — takes wallpaper tint** |
| Sidebar selected row | `#d7d7d6` | 64 pt tall run inside the pill; ≈ `#3c3c431a` over `#ededec` (token predicts `#dbdbdc`, 4 levels off) |
| Sidebar hover | **none** | pointer over a row changes **0 pixels** (image diff bbox = none) |
| Sidebar label | `#000000` | glyph core |
| Sidebar group header | `#878786` | glyph core |
| Sidebar icon | accent blue, stroke `#006bef`…`#3478f6` | 2 px runs at icon edges |
| Divider sidebar/content | **none** — the colour step *is* the boundary | run ends exactly at 416 |
| Toolbar / field fill | `#fdfdfd`…`#fefefe` | inner field |
| Search field border | `#d8d8d8`-class 1 pt hairline + **blue focus ring** when focused | crop `/tmp/toolbar-crop.png` equivalent |
| Placeholder text | `#7f7f7e` | darkest in placeholder box |
| Scope control | outer pill, 1 pt `#d8d8d8`-class border, white fill | 1:1 crop |
| Scope selected segment | `#e3e3e3`-class rounded pill, black label | crop; mid-sample `#bcbcbc` is the label edge |
| Content title ("Browse Categories", "Top Results") | `#000000`, 13 pt semibold | glyph core |
| Empty-state title | `#808080` | glyph core, 17 pt semibold |
| Empty-state subtitle | `#808080` | glyph core, 13 pt |
| Pill background | `#fefefe` with a soft `#faf9f9` gradient at the bottom and **no border** | vertical runs 1494–1623 device px |
| Pill disabled controls | `#bcbcbc` | transport glyphs with no track loaded |
| Settings sheet background | `#ffffff` | flat |
| Settings separators | `#e5e5e5`, 1 pt | 2 device px lines |
| Settings tab bar underline | `#c2c2c2` | 8 pt band under the tab strip |
| Accent (buttons, checkboxes, selected tab ring) | `#3478f6` / `#3375f0` | OK fill, checkbox fill |
| Cancel button label | `#242424` | glyph core |

Not inverted dark mode: light mode's sidebar is a *lighter* surface than the
content only because of vibrancy; the content is the brightest surface at pure
white, which is the opposite relationship to dark mode (`#232324` sidebar over
`#1f1f1f` content).

## 5. Typography

* System font only — SF Pro Text / SF Pro Display as the OS supplies them. No
  bundled face is used (the app is a native AppKit app).
* Sizes are **INFERRED** from OCR line-box heights and Apple's scale, not read
  from the app (AX exposes no font attributes here):

| Role | Size / weight | Evidence |
|---|---|---|
| Large page title (`Home`, `Radio`, `Recently Added`) | 26 pt / bold | OCR box 26–35 device px tall; matches `--large-title` 26 |
| Empty-state title | 17 pt / semibold | box 61 device px incl. descenders |
| Section heading (`Browse Categories`, `Top Results`) | 13 pt / semibold | box 23–30 device px |
| Sidebar row label | 13 pt / regular | box 22–23 device px |
| Sidebar group header | 11 pt / regular, gray | box 19–20 device px |
| Scope segment label | 13 pt / regular (selected: medium) | box 30 device px |
| Card title / subtitle | 13 / 11 pt | results cards |
| Pill track title / artist | 12 / 11 pt | LCD block |
| Settings form label | 13 pt | 26 device px boxes |

## 6. Components

### Sidebar
* Flat 208 pt column, `#ededec`, no divider, no hover feedback.
* Rows 32 pt tall; 10 pt horizontal inset for the selection pill (radius 8).
* Icons 16 pt, accent blue, label 54 pt from the window edge (icon then 13 pt label gap).
* Group headers 11 pt gray, 15 pt from the edge, 13 pt above / 16 pt below.
* Account footer is a 130 × 28 button at the bottom (`Shiwang Kumar`) — Tansen
  legitimately omits it (no account) and must keep its own Settings entry.

### Toolbar
* 52 pt tall, same material as the window (no separate bar, no divider line).
* Search field: 421 × 38, radius 19, hairline border, white fill, leading
  magnifier, placeholder is the service name (`Apple Music`), clear button
  appears once text is entered.
* Focus: 3 pt blue ring (`#3478f6` at 50–60% alpha) around the pill.
* Scope: 260 × 38 outer pill with 1 pt border, three segments, the active one on
  a light-gray rounded pill.

### Empty state (all library pages)
* Centred stack: 60 pt gray music-note glyph, 17 pt semibold gray title,
  13 pt gray subtitle, then a 148 × 26 bordered pill button (`Browse Apple Music`).
* Everything is gray; nothing is blue except the button's hover state (not measured).

### Player pill
* 700 × 54, radius 27, `#fefefe` translucent material, soft shadow, 19 pt from
  the bottom, centred in the content column.
* Disabled transport glyphs are `#bcbcbc` when nothing is loaded.
* No divider between controls and the LCD block.

### Settings sheet (read-only reference for Tansen's Settings)
* 650 × 436 (General) / 445 (Advanced), modal sheet over the main window.
* Tab strip: 4 icon-over-label buttons, 55–64 × 56, selected one ringed in blue.
* Grouped form: white, 1 pt `#e5e5e5` separators, 13 pt labels, checkboxes on
  the left column, popup button `Medium`, footer Cancel/OK 68 × 26.

## 7. Interaction states

| Component | Default | Hover | Focus | Pressed | Selected | Disabled |
|---|---|---|---|---|---|---|
| Sidebar row | transparent | **same as default** (0 px diff, measured) | — | — | `#d7d7d6` pill, 10/10 inset | — |
| Search field | white, hairline | — | blue 3 pt ring (measured) | — | — | — |
| Scope segment | white | — | — | — | `#e3e3e3` pill | — |
| Toolbar search (library pages) | absent → compact "Find in X" | — | — | — | — | — |
| Transport buttons | `#bcbcbc` when empty | — | — | — | — | measured |
| Context menu | **NOT MEASURED** — synthetic right-click produced no menu | | | | | |
| Track row / queue / lyrics | **NOT REACHABLE** — empty library | | | | | |

## 8. Motion

Not yet measured (no frame-accurate recorder; see `light-motion.md`). Static
captures show no cross-fade between page switches — the content is replaced in
one frame at this sampling rate, so nothing can be claimed about duration.

## 9. What Tansen can and cannot take from this

Legitimate to take: geometry, colour relationships, typography roles, the
two-mode toolbar, empty-state composition, the selection pill, the pill player
layout, settings form structure.

Must **not** be taken: Apple wordmark, `Apple Music`/`iTunes Store` segment
labels, store promo banners, category artwork, catalogue imagery, the account
footer, service names. Tansen's sources are its own (YouTube Music / local
library), so the scope control must name those instead.
