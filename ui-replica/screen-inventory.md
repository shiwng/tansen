# Music.app screen inventory (live, 2026-11-04)

Environment: macOS 27.2 (26B123) · Music 1.7.2 · built-in Liquid Retina 2560×1664
at 2× · dark appearance · default (blue) accent · Music window 1280×820 at (80,60),
same size as Tansen's window.

Capture method: window-only `screencapture -o -l129`, frames in
`ui-replica/captures/` (gitignored). The sidebar footer shows the signed-in
account — mask before attaching any image.

## Reachable now

| Screen | How | Capture | Notes |
|---|---|---|---|
| Songs (library, empty) | sidebar row 9 | `music-live-dark.png` | empty state: centred note glyph, "Add Music to Your Library", one button |
| Search / Browse | sidebar row 1 | `music-dark-search-empty.png` | promo banner + Browse Categories grid (5 wide), scope control at top right |
| Home | sidebar row 2 | not captured | click did not register; retry with `input.swift click` |

Sidebar rows (same for all screens): Search, Home, Radio · **Library**:
Recently Added, Artists, Albums, Songs · **Store**: iTunes Store · **Playlists**:
All Playlists · account footer at the bottom.

## Geometry measured from AX (points, exact)

| Element | Frame | Derived |
|---|---|---|
| window | 80,60 1280×820 | default size |
| toolbar | 80,60 1280×52 | 52 pt tall, spans full width |
| search field | 616,67 421×38 | rel x 536, 7 pt below toolbar top; 421×38; centred in the *content* area |
| scope control (radio group) | 1092,67 260×38 | right inset 24 pt; 3 segments |
| sidebar scroll area | 80,112 208×711 | sidebar 208 pt wide, below toolbar |
| sidebar row | 80,112 208×32 | rows 32 pt tall |
| sidebar group header row | 80,221 208×19 | headers 19 pt; 13 pt above, 16 pt below |
| sidebar row label | 134,119 137×18 | text inset 54 pt from sidebar left, 18 pt tall box |
| sidebar header label | 95,222 190×16 | inset 15 pt |
| sidebar footer (account) | 80,823 208×50 | 50 pt; hidden in Tansen by design |
| content scroll area | 288,112 1072×686 | content starts at 288 (208 sidebar + splitter), 686 pt tall |
| floating player pill | 474,807 700×54 | rel x 394, bottom inset 19 pt, 700×54 |

Traffic lights: 16×16 at rel (18,18), (41,18), (64,18).

## Blocked (needs owner input)

- Album / playlist / artist pages, queue, lyrics, playing/paused/loading states:
  the library is empty and there is no subscription catalogue content, so these
  screens have no reachable instance. Either the owner adds local files to the
  Music library, or captures these screens.
- Light appearance: needs the owner to switch macOS appearance (agent must not).
- Hover / pressed / focused / inactive-window states: not yet captured; hover is
  reachable with `input.swift move`, inactive needs another app focused.
- Music's own Settings window: not opened yet (read-only is allowed).

## Light mode, second pass (2026-11-04, macOS 27.2, Music 1.7.2)

Window 1280×820 at (80,60), light appearance, default blue accent, 2× display.
Coordinates below are **window-relative points** (AX frame minus the window
origin), which is what the web view will use.

| Element | Window-relative frame | Note |
|---|---|---|
| toolbar | 0,0 1280×52 | exists only on Search/Browse; library pages get a compact "Find in X" instead |
| search field | 536,7 421×38 | radius 19, white fill, hairline border, blue 3 pt focus ring |
| scope control | 1012,7 260×38 | right edge 8 pt from the window edge; 3 segments |
| sidebar | 0,52 208×711 | rows from y 52; 32 pt each |
| selection pill | 10,276 187.5×32 | selected page row (Songs), radius 8, fill `#d7d7d6` |
| sidebar label | 54,· | 13 pt |
| group header | 15,· | 11 pt, `#878786`, 19 pt row, 13/16 pt gaps |
| content | 208,52 1072×711 | `#ffffff`, no divider against the sidebar |
| player pill | 394,747 700×54 | radius 27, `#fefefe`, no border, 19 pt bottom inset |
| pill transport | 403/431/459/495/523 | 28/28/36/28/28 pt, centred on y 760 (play 756) |
| pill right controls | 975 (lyrics), 1012 (queue), 1049 (volume) | 36×36 each |
| settings sheet | screen (530,281) 650×436 | layer-8 panel, id 1656; General tab 436 tall, Advanced 445 |

Toolbar modes, measured: with Search/Browse selected the window carries the
search field + scope control; on Recently Added/Artists/Albums/Songs/Playlists
those controls are absent and a trailing "Find in <page>" appears instead.
Same toolbar height in both modes.

Empty state (all library pages): 60 pt gray note glyph, "Add Music to Your
Library" 17 pt semibold `#808080` at y 414, subtitle 13 pt `#808080` at y 456,
bordered button 148×26 at (670,483) labelled "Browse Apple Music".

Settings sheet tabs (read-only): General, Playback, Files, Advanced —
icon-over-label buttons 55–64×56 at y 313 (abs), selected one ringed blue.
Form: white, 1 pt `#e5e5e5` separators, checkboxes `#3375f0`, popup "Medium",
Cancel/OK 68×26 at the bottom right, help "?" 26×26 at the bottom left.
Closed with **Cancel**; nothing was changed or saved.

### New captures (gitignored)

`music-light-{home,search,search-page,search-focus,search-suggest,search-results,radio,recent,artists,albums,songs,playlists,contextmenu}.png`,
`music-light-settings-{general,playback,files,advanced}.png`,
`tansen-light-home.png`.

### State measurements

| State | Result |
|---|---|
| sidebar hover | **no visual change** — image diff of two captures = no differing pixel |
| sidebar selected | `#d7d7d6` pill, x 10→197.5, full 32 pt row |
| search field focused | 3 pt blue ring, fill stays `#fdfeff` |
| context menu (right-click on sidebar row) | **no menu appeared** — synthetic right-click produced nothing; NOT MEASURED |
| transport disabled (no track) | `#bcbcbc` glyphs on the pill |
