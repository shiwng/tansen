# Measurement toolkit (dev only)

Scripts live in `scripts/ui-compare/`, never ship, and are not imported by the app.
Raw captures land in `ui-replica/captures/` (gitignored — they can show the owner's
account name; mask the sidebar footer before any image leaves the folder).

| Tool | What it does | Notes |
|---|---|---|
| `find-window.swift <owner>` | CGWindowList window id + bounds | layer 0 only, on-screen windows |
| `capture.sh <out.png> [owner]` | window-only capture (`screencapture -o -l<id>`), settles by waiting for two identical frames | 2× Retina PNG |
| `ax-dump.swift <pid> [depth]` | recursive AX tree: role, subrole, title, desc, value, frame | frames are screen points (1×) |
| `ocr-boxes.swift <img>` | Vision text + boxes in image px | boxes are line bounds, not cap height |
| `window-set.sh <pid> x y w h` | AX position/size | same size for Music and Tansen |
| `input.swift move/click/scroll/key` | CGEvent real input (hover needs a real move) | 60 ms between events |
| `pixel_tools.py color/edges/ssim/maskrect` | flat-region colour, run-length edges, masked SSIM + heatmap, privacy box | Pillow 11.3, numpy 2.0 in `ui-replica/.venv` |

## Accuracy check against known Tansen values (2026-11-04)

| Known value (CSS) | Measured | Delta |
|---|---|---|
| sidebar 208 px | 416 device px at 2× = 208.0 pt (edge run `0-416`) | 0.0 pt |
| player pill height 50 px | 98 device px = 49.0 pt (border antialiasing eats 1 px/side) | ≤ 1 pt |
| content background | `#1f1f1f`, stable across repeated samples | ±1 level |
| OCR: sidebar labels | "Search/Home/Radio/Library/Recently Added…" boxes aligned to AX text x=134 pt (Music) within 1 pt | ≤ 1 pt |

So: geometry from AX is exact (points), pixels are 2×, colour sampling is sRGB as
captured (no ICC conversion needed on this display — PNG has no embedded profile and
the capture is already sRGB), OCR is good to ~1 pt. Frame-time precision for motion
needs a recorder, which is **NOT BUILT**: `ffmpeg` is absent and installing it needs
owner approval; the Phase-4 alternative is an AVFoundation/ScreenCaptureKit Swift
recorder.

## Privacy

Only window captures are taken (`-l<id>`), never the full screen. The Music sidebar
footer shows the signed-in account; `pixel_tools.py maskrect` covers it before any
image is attached to a report.
