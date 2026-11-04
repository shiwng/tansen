# Apple Music full-screen player — reference measurements (Not Playing idle, dark)

Source: `~/.zcode/cli/image-cache/sess_3b0aec4a-e444-48c3-8a53-7f27fc46023d/image-9378115c9c3c45552d6047903b8e3ce9.png`
Measured 2026-11-04 with `scripts/ui-compare/pixel_tools.py` conventions (scratch scripts in `/private/tmp/lab-measure/`).

## Scale determination

| Fact | Value |
|---|---|
| Image size | 2784 × 1864 px (window floats on black surround; surround is not part of the window) |
| Window bbox in image | x 112–2672, y 76–1716 → **2560 × 1640 px** |
| Live Music window (AX, this machine) | 1280 × 820 **pt** (`find-window.swift Music` → `80 60 1280 820`) |
| Scale | **2× Retina** (2560/1280 = 1640/820 = 2.0, exact) |
| All pt values below | window-relative points = (image px − window origin) / 2, window origin = (112, 76) |

## Main table

| Element | Value | px @2× (image coords) | pt (window-relative) | Method |
|---|---|---|---|---|
| Window size | 1280 × 820 pt | 2560 × 1640 | 1280 × 820 | non-black bbox; cross-checked vs live window AX frame |
| Window corner radius | 15 pt | 30 | 15 | circle fit on corner offsets (R=30 fits dy≥2 exactly) |
| Window edge rim | ~1 px lighter line inside all 4 edges, #6b6c6f | 2 px | ~1 pt | row/col scans at window edges |
| Window outer shadow | none visible (hard cut to black surround) | — | — | pixel scan around window |
| Background colour (mid) | #5b5c5f | — | — | 5×5 flat samples ×3 spots (200,600)(2000,500)(1500,1550) |
| Background gradient | very subtle vertical: #5c5d60 top → #5a5b5e bottom (~2 RGB levels, ≈1.5%) | — | full height | 40 px patch means, 6×6 grid + vertical profile |
| Artwork tile bbox | (189, 194.5) 262.5 × 262.5 pt | (490, 465) 525 × 525 | 189, 194.5, 262.5 × 262.5 | threshold bbox + edge transitions |
| Tile corner radius | 10 pt | 20 | 10 | offsets fit: R=20 matches dy 3–15 |
| Tile fill | #262629 | — | — | 30×30 mean, flat ±1 |
| Tile drop shadow | soft, reach ~25–30 px, slightly stronger below/right (#545557 at edge → bg over ~25 px) | 25–30 | 12–15 | falloff scans on 4 sides |
| Tile glyph (music note) | 88 × 108 pt, colour #626267, center 9 pt left of tile center (optical) | (646, 620) 176 × 216 | 88 × 108 | threshold bbox, mode colour |
| "Not Playing" label | bbox x 141–228, y 542.5–559.5 pt; cap height 12.5 pt → **SF Pro ~18 pt Bold** (stem 5 px ≈ 2.5 pt); white #ffffff | x 394–568, y 1161–1195; cap 25 px | cap 12.5 | column scan of "N" (cap top 1163 → baseline 1188), lum>200 core |
| Left-aligned to scrubber | label left edge = scrubber left edge (±1 px) | 394 vs 392 | 141 vs 140 | bbox compare |
| Scrubber track | x 140–500 pt (w 360), y 597–602.5 pt (h 6), colour **#898a8d**, fully rounded caps, **no filled portion** (uniform along axis) | x 392–1112 (720), y 1270–1281 (12) | 360 × 6 | column profile (12 px flat run), row samples every 90 px |
| Time labels "--:--" | left at x 140.5 pt, right ends x 499.5 pt; y 614–619.5 pt; ~**11 pt** (colon 11 px); colour #8f9093 | left x 393–436, right x 1068–1111, y 1304–1315 | — | threshold bbox + colon column scan |
| Transport: shuffle | center (154.75, 656.5) pt, 19.5 × 16 pt, #c8c9cc (enabled) | (421.5, 1389) 39 × 32 | — | threshold bbox, core mean |
| Transport: rewind | center (233, 656.5) pt, 30 × 17 pt, #a1a1a3 (disabled) | (578, 1389) 60 × 34 | — | same |
| Transport: play | center (321.5, 656.25) pt, 21 × 23.5 pt (taller than wide), #a1a1a3 | (755, 1388.5) 42 × 47 | — | same |
| Transport: forward | center (407, 656.5) pt, 30 × 17 pt, #a1a1a3 | (926, 1389) 60 × 34 | — | same |
| Transport: repeat | center (485, 656.75) pt, 18 × 15.5 pt, #cacacd (enabled) | (1082, 1389.5) 36 × 31 | — | same |
| Transport spacing (center-to-center) | 78.25 / 88.5 / 85.5 / 78 pt; symmetric about column center x=320 pt (rewind/forward ±87 pt, shuffle/repeat ±165 pt) | 156.5 / 177 / 171 / 156 px | — | icon centers |
| Lyrics placeholder "Play a song to see lyrics here." | bbox x 850.5–1017, y 436.5–447.5 pt; center (933.75, 442) pt; cap 8.5 pt → **~12 pt**; colour core #dfe0e3 (brightest #e8e8ec — dimmer than the white title) | x 1813–2146, y 949–971; cap 17 px | — | threshold bbox + stem scan |
| Top-left capsule (X + artwork toggle in ONE capsule) | (101, 8) 75 × 36 pt, stadium R 18 pt | (314, 92) 150 × 72 | — | lum>105 bbox; no bg gap between icons (single capsule) |
| Capsule fill | #76777a (vs bg #5b5c5f behind → glass lightens ≈ +27 lum) | — | — | interior mean away from icons |
| Capsule rim | 1 px near-white top #f0f1f6 and bottom #f5f7fd; 1 px dark #2a2a2c at left/right tangent points | 1 px | 0.5 pt | edge profiles at x=389 / y=128 |
| X (close) icon | 13 × 13 pt at center (121, 26.5) pt, white #fafafb, stroke ~2 pt | (341, 116) 26 × 26 | — | threshold bbox |
| Artwork-toggle icon | 21.5 × 17.5 pt at center (155.75, 26.25) pt, white #fefefe (rounded-rect outline + arrow + filled rounded square front) | (402, 111) 43 × 35 | — | threshold bbox |
| Traffic lights | Ø 14 pt, centers (26, 26), (49, 25.75), (72, 26) pt; spacing 23 pt; red #e2685b, yellow #f4c32c, green #62bd2e (look dimmed — window not key) | Ø 28, centers (164,128)(210,127.5)(256,128) | — | colour blob bbox |
| Volume capsule | (1092, 8) 180 × 36 pt, stadium R 18 pt; fill #77787b; same rim treatment | (2296, 92) 360 × 72 | — | lum>105 run at y=128 |
| Volume track | x 1107–1221 pt (w 114), y 23–28.5 pt (h 6), rounded caps | x 2326–2554 (228), y 122–133 (12) | — | threshold runs |
| Volume filled portion | #e4e4e5, x 1107–1172 pt (left of knob) | 2326–2456 | — | row runs |
| Volume unfilled portion | #919294, x 1196–1221 pt (right of knob) | 2504–2554 | — | row runs |
| Volume knob | white #ffffff pill/ellipse 24 × 13 pt, center (1184, 26) pt | 48 × 26, center (2480, 128) | — | lum>240 bbox (rim rows excluded) |
| Speaker icon | 22 × 15.5 pt, center (1245, 25.75) pt, #e4e4e5 (body + 2 arcs) | (2580, 112) 44 × 31 | — | threshold bbox y 104–154 |
| Bottom-right capsule | (1196.5, 773) 75 × 35.5 pt, stadium R 18 pt; fill #76777a; same rims | (2505, 1622) 150 × 71 | — | col scan x=2580 (edges 1622/1693) |
| Lyrics button (active) | light circle Ø ≈ 30 pt, #dededf, center (716, 291.25) pt | Ø 60, center (2544, 1658.5) | — | row scan y=1658 |
| Lyrics bubble glyph | ~17.5 × 18.5 pt dark #5a5b5e bubble + white quote marks, inside circle | ~35 × 37 | — | pixel grid samples |
| Queue icon | ~17 × 12.5 pt (3 dot+bar rows), white #f7f8f8, right of circle | (2601, 1648) 31 × 22 (thr) / ~34 × 25 true | — | threshold bbox |

## Glass / material notes (item 10)

- All three capsules share one material: frosted light fill ≈ **#76777a–#78797c** over a #5b5c5f background (lightens what is behind by ≈ +26/255; no backdrop detail exists here to sample blur radius against).
- Rim: 1 px near-white highlight along top **and** bottom arcs (#f0f1f6 top, #f5f7fd bottom), turning into a 1 px dark line **#2a2a2c** at the extreme left/right tangent points — i.e. a light ring strongest at top/bottom, dark at the sides.
- Interior is not perfectly flat: lum 119–121 mid, rising to ~129–133 within ~8 px of the bottom rim (bottom-lit edge).
- No drop shadows under any capsule (bg directly below each capsule is unchanged 91–94 lum).
- The window itself has a 1–2 px #6b6c6f rim highlight on all four inside edges; no outer shadow in this capture (black surround is a hard cut).
- The artwork tile is the only element with a drop shadow (soft, ~25–30 px reach, down-right bias).
- Traffic lights read ~15–20 % desaturated vs the usual macOS hues — consistent with the window not being key at capture time.

## Background gradient verdict (item 11)

Not one flat colour, but nearly: a monotonic vertical darkening of ~2 RGB levels (#5c5d60 → #5a5b5e over 820 pt, ≈1.5 % luminance). Horizontal variation ≤1 level away from UI. For a rebuild: `background: linear-gradient(#5c5d60, #5a5b5e)` or flat #5b5c5f is within 1 level.

## Raw sample points (image px, sRGB hex)

| Purpose | Point(s) | Value |
|---|---|---|
| bg spot 1 / 2 / 3 | (200,600) (2000,500) (1500,1550) | #5b5c5f / #5c5c60 / #5b5b5e |
| bg vertical profile (x 2150–2350, 40 px patches) | y 300/500/700/900/1100/1300/1500 | #5c5d60 ×3 → #5b5c5f → #5b5b5e → #5a5b5e ×2 |
| tile fill | (530,505), 30×30 at (505,480) | #262629 |
| tile glyph core | mean of lum>80 in tile | #626267 (n=14773) |
| scrubber | (480,1276) (750,1276) (1110,1276), col x=700 y1270–1281 | #898a8d–#8a8a8e, 12 px tall |
| time labels | core mean y1304–1315 | #8f9093 |
| transport cores | lum>150 means | shuffle #c8c9cc, rewind #a1a1a3, play #a0a1a3, forward #a1a1a3, repeat #cacacd |
| title core / brightest | lum>200 mean / max in (394–568, 1161–1195) | #fcfcfd / #ffffff |
| lyrics core / brightest | lum>180 mean / max in (1813–2146, 949–971) | #dfe0e3 / #e8e8ec |
| capsule fill | (318–344, 100–158) and (448–460, 100–158) | #78797c / #747578 |
| capsule rim top / bottom / side | (389, 92) / (389, 163) / (313, 128) | #f0f1f6 / #f5f7fd / #2a2a2c |
| volume track filled / unfilled | (2400–2440, 124–132) / (2510–2545, 124–132) | #e4e4e5 / #919294 |
| volume knob | (2480, 115–141) | #ffffff |
| speaker icon core | lum>170 in (2580–2624, 112–143) | #e4e4e5 |
| traffic lights | (164,128) (210,128) (256,128) | #e2685b / #f4c32c / #62bd2e |
| bottom-right capsule fill | (2590–2600, 1650–1666) | #747578 |
| lyrics circle / bubble / quotes | row y=1658 x2514–2574 / (2530,1650) / inside bubble | #dededf / #5a5b5e / white |
| window rim | (1392, 76) (1392, 1714) (112, 900) | #6b6c70 / #6b6c6e / #6c6c6e |
| tile shadow (below, x=750) | y 994/1000/1006/1012/1018 | #545557 → #5a5b5f (bg by ~1024) |

Scratch scripts: `/private/tmp/lab-measure/probe1..9.py` (venv: `ui-replica/.venv`, `python3 -I`). App source untouched.
