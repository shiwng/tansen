# Token proposal (M1) — measured values only

Status: PROPOSED. M3 maps these onto the existing `styles/tokens.css` custom
properties (184 of them) and verifies each against the reference. Nothing here
is applied to the app yet.

## Spacing / layout

| Token (proposed) | Value | Source (reference.md row) |
|---|---|---|
| `--sidebar-width` | 208 pt | Sidebar panel width |
| `--sidebar-top-inset` | 52 pt | Sidebar top inset |
| `--sidebar-row-height` | 32 pt | Sidebar row height |
| `--sidebar-section-height` | 19 pt | Section header row |
| `--content-inset` | 34 pt | Content left inset |
| `--shelf-card` | 180 pt | Shelf card |
| `--shelf-card-total-height` | 217 pt | Shelf card |
| `--shelf-gap` | 20.5 pt | Shelf card pitch − card |
| `--player-pill-width` | 700 pt | Player pill |
| `--player-pill-height` | 54 pt | Player pill |
| `--transport-button` | 28 pt | Transport buttons |
| `--play-button` | 36 pt | Play button |
| `--traffic-light` | 16 pt | Traffic lights |
| `--traffic-light-inset` | 18 pt | Traffic light x/y |

## Colour (proposed, dark / light)

| Token | Dark | Light | Caveat |
|---|---|---|---|
| `--content-bg` | `#1f1f22` | `#ffffff` | flat, stable |
| `--sidebar-material-tint` | `#26262b` | `#ededec` | sampled through vibrancy; use tint + backdrop blur, not a fixed fill |
| `--nav-selected-bg` | `#434347` | `#d7d7d6` | flat |
| `--player-pill-tint` | `#212121` | `#b1b1b1` | translucent material |

## Not yet tokenised (blocked on measurement)

- Type scale (size/weight/tracking/line height), title and caption styles.
- Hairlines/separators, hover and pressed fills, focus ring.
- Radii (card, pill, artwork) — visually 6–10 pt but unmeasured.
- Shadows and the pill's inner border.

Rule: no token enters the app until it has a measured row in `reference.md`.
