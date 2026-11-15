#!/usr/bin/env python3
"""Trace the two raster cuts of the mark into SVG path data (run once; output is committed as mark.json).

  trace.py GLYPH.png FACETS.png > mark.json

Both PNGs are 1024x1024 RGBA with the same outer silhouette. The glyph is one colour with two cut-outs;
the faceted cut uses the five brand tones. Needs: numpy, pillow, potracer.
"""
import json, math, sys
import numpy as np
import potrace
from PIL import Image

TONES = {"mid": "#F77848", "hi": "#FF956B", "lo": "#AC3712", "sh": "#7A2A12", "glint": "#FFF0E5"}

# The drawing has the head about 20 degrees off square to the shank. Stand the nail upright, shear the
# head level about the point, and lay it back at the brand angle, so the nail is one rigid, straight
# shape at any rotation.
TILT, LEVEL, POINT_X = math.radians(49.5), 0.366, 1.6

def straight(p):
    c, s = math.cos(TILT), math.sin(TILT)
    x, y = p.x - 512, p.y - 512
    ux, uy = x * c + y * s, -x * s + y * c
    uy += LEVEL * (ux - POINT_X)
    return f"{ux * c - uy * s + 512:.1f} {ux * s + uy * c + 512:.1f}".replace(".0", "")

def path(mask, tol=0.6):
    bm = potrace.Bitmap(~mask)  # potracer treats False as ink
    pl = bm.trace(turdsize=12, alphamax=1.0, opttolerance=tol)
    out = []
    f = straight
    for c in pl:
        out.append("M" + f(c.start_point))
        for s in c.segments:
            out.append(("L" + f(s.c) + "L" + f(s.end_point)) if s.is_corner else ("C" + f(s.c1) + " " + f(s.c2) + " " + f(s.end_point)))
        out.append("Z")
    return "".join(out)

glyph = np.array(Image.open(sys.argv[1]).convert("RGBA"))
facets = np.array(Image.open(sys.argv[2]).convert("RGBA"))
res = {"viewBox": "0 0 1024 1024", "glyph": path(glyph[..., 3] > 127)}
inside = facets[..., 3] > 127
rgb = facets[..., :3].astype(int)
pal = np.array([[int(h[i:i + 2], 16) for i in (1, 3, 5)] for h in TONES.values()])
near = np.argmin(((rgb[:, :, None, :] - pal[None, None]) ** 2).sum(-1), axis=-1)
res["silhouette"] = path(inside)
res["facets"] = {name: path(inside & (near == i)) for i, name in enumerate(TONES) if name != "mid"}
res["tones"] = TONES
json.dump(res, sys.stdout, indent=1)
