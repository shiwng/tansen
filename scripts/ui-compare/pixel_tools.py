#!/usr/bin/env python3
"""Pixel measurement for the UI replica. Dev-only; never bundled.

Commands:
  color  <img> <x> <y> [w] [h]      mean sRGB colour of a rect (hex)
  edges  <img> <y> [axis]           run-length segments along a row/col
  ssim   <a> <b> <out.png> [mask]   SSIM + diff heatmap + side-by-side
  maskrect <img> <out> x y w h      opaque box (privacy masking)
All coordinates are image pixels unless --pt SCALE is given.
"""
import sys
import numpy as np
from PIL import Image, ImageDraw


def load(path):
    im = Image.open(path)
    if im.mode in ("RGBA", "LA"):
        bg = Image.new("RGB", im.size, (0, 0, 0))
        bg.paste(im, mask=im.split()[-1])
        im = bg
    return im.convert("RGB")


def color(path, x, y, w=1, h=1):
    a = np.asarray(load(path))[y:y + h, x:x + w].reshape(-1, 3)
    return tuple(int(round(v)) for v in a.mean(axis=0))


def runs(arr, tol=2):
    """Run-length segments of near-constant colour along arr (n,3)."""
    out, start, ref = [], 0, arr[0]
    for i in range(1, len(arr)):
        if np.abs(arr[i].astype(int) - ref.astype(int)).max() > tol:
            out.append((start, i, tuple(int(v) for v in ref)))
            start, ref = i, arr[i]
    out.append((start, len(arr), tuple(int(v) for v in ref)))
    return out


def edges(path, pos, axis="x", tol=2, minlen=2):
    a = np.asarray(load(path))
    line = a[pos, :, :] if axis == "x" else a[:, pos, :]
    return [r for r in runs(line, tol) if r[1] - r[0] >= minlen]


def ssim(a, b, out, mask=None):
    """Masked SSIM (grayscale) + diff heatmap + side-by-side, all written to out."""
    A, B = np.asarray(load(a), dtype=np.float64), np.asarray(load(b), dtype=np.float64)
    if A.shape != B.shape:
        raise SystemExit(f"size mismatch {A.shape} vs {B.shape}")
    if mask:
        mx, my, mw, mh = (int(v) for v in mask.split(","))
        A[my:my + mh, mx:mx + mw] = 0
        B[my:my + mh, mx:mx + mw] = 0
    ga = A.mean(axis=2)
    gb = B.mean(axis=2)
    c1, c2 = (0.01 * 255) ** 2, (0.03 * 255) ** 2
    mua, mub = ga.mean(), gb.mean()
    va, vb = ga.var(), gb.var()
    cov = ((ga - mua) * (gb - mub)).mean()
    score = ((2 * mua * mub + c1) * (2 * cov + c2)) / ((mua ** 2 + mub ** 2 + c1) * (va + vb + c2))
    d = np.abs(A - B).max(axis=2)
    heat = np.zeros((*d.shape, 3), dtype=np.uint8)
    heat[..., 0] = np.clip(d * 4, 0, 255)
    heat[..., 1] = np.clip(d * 1.5, 0, 120)
    ia, ib, ih = Image.fromarray(A.astype(np.uint8)), Image.fromarray(B.astype(np.uint8)), Image.fromarray(heat)
    comb = Image.new("RGB", (ia.width * 3, ia.height))
    for i, im in enumerate((ia, ib, ih)):
        comb.paste(im, (i * ia.width, 0))
    comb.save(out)
    print(f"ssim={score:.4f} mean_abs_diff={d.mean():.2f} p99={np.percentile(d,99):.1f} out={out}")


def maskrect(path, out, x, y, w, h):
    im = load(path)
    ImageDraw.Draw(im).rectangle([x, y, x + w - 1, y + h - 1], fill=(24, 24, 26))
    im.save(out)


if __name__ == "__main__":
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == "color":
        print("#%02x%02x%02x" % color(args[0], *(int(v) for v in args[1:5])))
    elif cmd == "edges":
        for s, e, c in edges(args[0], int(args[1]), args[2] if len(args) > 2 else "x"):
            print(f"{s}-{e} ({e-s}px) #%02x%02x%02x" % c)
    elif cmd == "ssim":
        ssim(args[0], args[1], args[2], args[3] if len(args) > 3 else None)
    elif cmd == "maskrect":
        maskrect(args[0], args[1], *(int(v) for v in args[2:6]))
    elif cmd == "dark":
        # darkest pixel + 2nd-percentile colour in a box: text colour behind antialiasing
        im = load(args[0]).convert("RGB")
        x, y, w, h = (int(v) for v in args[1:5])
        px = list(im.crop((x, y, x + w, y + h)).getdata())
        lum = sorted(px, key=lambda c: 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2])
        print("min #%02x%02x%02x  p2 #%02x%02x%02x" % (lum[0] + lum[max(0, len(lum) // 50)]))
    else:
        raise SystemExit(__doc__)
