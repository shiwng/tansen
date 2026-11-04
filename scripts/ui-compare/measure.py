#!/usr/bin/env python3
"""Dev-only pixel measurement for the UI replica. Never bundled with the app.

Usage: measure.py <png> [--logical]
Prints: image size, logical size, column/row boundaries, colour samples,
and the dominant flat colours per region. Values in logical points (2x Retina).
"""
import sys
from collections import Counter
from PIL import Image

def main(path):
    im = Image.open(path).convert("RGB")
    scale = im.width // 1280 if im.width >= 2560 else 1
    print(f"file {path}")
    print(f"pixels {im.width}x{im.height} scale {scale}x logical {im.width//scale}x{im.height//scale}")

    px = im.load()

    def at(x, y):
        return px[x * scale, y * scale]

    def hexc(c):
        return "#%02x%02x%02x" % c

    # Sidebar/content boundary: first x where the row colour changes and stays.
    for y in (300, 500, 650):
        base = at(20, y)
        edge = None
        for x in range(120, 400):
            if sum(abs(a - b) for a, b in zip(at(x, y), base)) > 12:
                edge = x
                break
        print(f"sidebar_edge y={y} x={edge} (sidebar {hexc(base)} -> {hexc(at(200, y)) if edge else '-'})")

    # Header vertical boundary: first y where the content column changes colour.
    for x in (700, 1000):
        base = at(x, 10)
        edge = None
        for y in range(20, 200):
            if sum(abs(a - b) for a, b in zip(at(x, y), base)) > 12:
                edge = y
                break
        print(f"content_top_edge x={x} y={edge} top {hexc(base)} below {hexc(at(x, 140))}")

    samples = {
        "sidebar_bg": (100, 600),
        "sidebar_selected_bg": (100, 100),
        "content_bg": (700, 700),
        "window_bg_top": (700, 20),
        "player_pill": (700, 760),
        "player_pill_btn": (560, 760),
    }
    for name, (x, y) in samples.items():
        print(f"colour {name} {hexc(at(x, y))}")

    # Dominant colours in the sidebar strip and the content area.
    for name, box in (("sidebar", (0, 0, 200, 700)), ("content", (300, 350, 1200, 700))):
        c = Counter()
        for x in range(box[0], box[2], 4):
            for y in range(box[1], box[3], 4):
                c[at(x, y)] += 1
        top = ", ".join(f"{hexc(k)} x{v}" for k, v in c.most_common(3))
        print(f"dominant {name}: {top}")

if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "ui-reference/music-dark-default.png")
