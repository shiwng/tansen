"""Tile window captures into one contact sheet for triage.

Usage: python sheet.py out.png <in1.png> [in2.png ...]   (2 columns, 520 px wide each)
Labels are the file names. Measurement always uses the full-resolution capture,
never this sheet.
"""

import sys

from PIL import Image, ImageDraw

COL_W, COLS, PAD, LABEL = 520, 3, 8, 14

out, paths = sys.argv[1], sys.argv[2:]
thumbs = []
for p in paths:
    im = Image.open(p).convert("RGB")
    h = round(im.height * COL_W / im.width)
    thumbs.append((p.rsplit("/", 1)[-1], im.resize((COL_W, h), Image.LANCZOS)))

rows = (len(thumbs) + COLS - 1) // COLS
row_h = max(t.height for _, t in thumbs) + LABEL + PAD
sheet = Image.new("RGB", (COLS * (COL_W + PAD) + PAD, rows * row_h + PAD), "#808080")
d = ImageDraw.Draw(sheet)
for i, (name, t) in enumerate(thumbs):
    x = PAD + (i % COLS) * (COL_W + PAD)
    y = PAD + (i // COLS) * row_h
    d.text((x + 2, y), name, fill="#ffffff")
    sheet.paste(t, (x, y + LABEL))
sheet.save(out)
print(out, sheet.size)
