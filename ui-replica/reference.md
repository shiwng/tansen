# Reference: real Apple Music on this Mac (measured)

Device: macOS 27.0.1 (26A434), Music.app 1.7 (CFBundleShortVersionString).
Window: 1280×820 pt at screen position (80, 60). Captures at 2× Retina
(2560×1640 px). All values below are logical points unless stated.

Method tags: `AX` = System Events accessibility tree (`ui-replica/ax/`),
`PX` = pixel measurement (`scripts/ui-compare/measure.py`), `EYE` = screenshot
review (cross-check only, never the source of a number).

## Captures

| File | Appearance | State | Notes |
|---|---|---|---|
| `ui-reference/music-dark-default.png` | dark | Home, idle, no track | sidebar + Home shelves + player pill |
| `ui-reference/music-light-default.png` | light | same | same window/scroll position |

Unreachable this session (NOT RUN, reasons): Search page, album / playlist /
artist pages, queue (Up Next), lyrics, playing/paused, hover + selected list
rows, minimum window size, inactive window.
Reasons: `AXPress` on the sidebar rows returns `missing value` (System Events
cannot press Music's outline rows), and the app has no owned library/account
content to open — Home shows only Apple's own stations.

## Geometry (AX, dark and light identical)

| Element | Value | Unit | Notes |
|---|---|---|---|
| Window content | 1280×820 | pt | frame (80,60) |
| Sidebar panel width | 208 | pt | `AXScrollArea` at x=0 |
| Sidebar top inset | 52 | pt | first row y=52 |
| Sidebar row height | 32 | pt | 208×32, pitch 32 |
| Section header row | 19 | pt | `Library`, `Store`, `Playlists` |
| Sidebar/content splitter | 0 | pt | collapsed, at x=208 |
| Content area | 1072×686 | pt | x=208, y=52 |
| Content left inset | 34 | pt | title/heading x=34 from content edge |
| Page title block ("Home") | 94×40 | pt | y=50 from window top |
| Section heading | 116×19 | pt | e.g. "Non-Stop Music" |
| Shelf card | 180×217 | pt | artwork 180×180 + 37 caption |
| Shelf card pitch | 200.5 | pt | gap ≈ 20.5 |
| Shelf row 2 top | y=367 | pt | content-relative |
| Promo banner | 1072×420 | pt | full content width |
| Player pill | 700×54 | pt | centred, y=747 from window top |
| Pill inner padding | 9 | pt | inner 682×54 at +9 |
| Transport buttons | 28×28 | pt | shuffle/prev/next/repeat |
| Play button | 36×36 | pt | 8 pt larger than transport |
| Traffic lights | 16×16 | pt | x=18, 41, 64; y=18 |

Traffic lights sit **inside** the sidebar, 18 pt from the window's top-left:
the title bar is overlaid on the content (full-height sidebar), not a separate
strip. No toolbar row exists in this state.

## Colours (PX, sRGB after conversion)

| Surface | Dark | Light | Method |
|---|---|---|---|
| Content background | `#1f1f22` | `#ffffff` | flat sample, dominant colour |
| Sidebar background | `#26262b` | `#ededec` | flat sample (vibrancy over wallpaper) |
| Selected sidebar row | `#434347` | `#d7d7d6` | flat sample |
| Player pill | `#212121` | `#b1b1b1` | sample (translucent material) |
| Card caption text | not measured | not measured | needs type overlay |

The light sidebar `#ededec` and the pill values are **material-dependent**
(they sample the desktop behind the window). Tokens must therefore use
`backdrop-filter` + tint, not a fixed hex, and the comparison protocol has to
tolerate material variance.

## Typography

NOT MEASURED yet. The 40 pt title block suggests the macOS large-title style,
but size, weight, tracking and line height need the overlay method (render
candidate CSS, diff against the capture) at M3. Do not guess them now.

## Structure (EYE, cross-checked against AX)

Sidebar order, top to bottom:

1. Search, Home, Radio (no section header)
2. `Library` header, with a right-aligned `Done` button (34×16 at x=168)
3. Recently Added, Artists, Albums, Songs, Genres, Composers, Music Videos, TV & Movies
   — the first four carry a red checkmark accessory, the last four a grey square
4. `Store` header: iTunes Store
5. `Playlists` header: All Playlists
6. Account row pinned to the bottom (avatar 24 pt circle + name)

Content (Home): large title `Home`; shelf `Non-Stop Music` with 180 pt square
cards (`Bollywood`, `Tamil`, `Punjabi`, `Hip-Hop`, `Pop`, …), each with a
`music radio` wordmark inside the artwork and a two-line caption (title,
`Radio Station`); shelf `Live Radio Stations` (same card size); promo banner
below; horizontally scrollable with visible chevron buttons at both edges.

Player pill: shuffle · previous · play · next · repeat on the left cluster,
Apple glyph in the centre, lyrics · queue · volume on the right, no now-playing
text because nothing is playing.

## Deltas to Tansen already visible (for M4/M5)

- Tansen sidebar: Search, Home, Radio · Library: Recently Added Artists/Albums/
  Songs · Settings · Playlists. Apple: single `Recently Added`, `Artists`,
  `Albums`, `Songs`, `Genres`, `Composers`, `Music Videos`, `TV & Movies`,
  plus `Store` and an account row. Tansen has no Genres/Composers/Music Videos/
  TV & Movies/Store backend; those must be hidden, not mocked.
- Tansen has no traffic-light overlay, no centred now-playing cluster, no
  section-header `Done` affordance.
- Apple's row pitch is 32 pt; Tansen's sidebar rows are 36 pt (`h-9`).
