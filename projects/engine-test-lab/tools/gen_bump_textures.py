"""Regenerate scene 12's normal-map demo pair.

    python tools/gen_bump_textures.py

Writes `assets/sprites/bumps128.png` (diffuse) and `bumps128_n.png` (normal).

Scene 12 puts two rows of the same diffuse side by side: the top row shades
against a flat normal, the bottom against this normal map. The whole scene is
that comparison, so the pair has one job - make the difference obvious.

The previous pair did not. The normal map was four big hemispheres with flat
gaps between them, so **57% of the surface was perfectly flat** and shaded
exactly like the row it was being compared against. Worse, the diffuse was an
unrelated fine woven pattern, so nothing in the albedo told you where the bumps
were: the shading read as vague light variation rather than as a sculpted
surface.

Two decisions follow from that:

* **A dense grid of nearly-touching hemispheres.** Little flat area is left, so
  almost every pixel of the bottom row shades differently from the top.
* **A near-uniform diffuse.** It is tempting to draw the bumps into the albedo
  too, but the top row uses the same texture - painted bumps would make the
  *flat* row look bumpy as well and blunt the very comparison the scene exists
  for. All the visible structure has to come from the lighting, so the albedo
  is a flat warm tone with a whisper of noise to keep it from reading as a
  solid rectangle.
"""

import math
import random
import struct
import zlib

SIZE = 128
GRID = 5                      # bumps per axis
SPACING = SIZE / GRID         # 25.6 px
RADIUS = SPACING * 0.5        # nearly touching
BASE = (206, 176, 136)        # warm tan, matching the old palette
NOISE = 6                     # +/- per channel, enough to see, not to distract


def write_png(path, rows):
    """Minimal RGBA PNG writer - no Pillow, so the tool runs anywhere."""
    raw = b"".join(b"\x00" + bytes(row) for row in rows)

    def chunk(tag, data):
        c = tag + data
        return struct.pack(">I", len(data)) + c + struct.pack(">I", zlib.crc32(c))

    header = struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0)
    png = (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )
    with open(path, "wb") as f:
        f.write(png)


def bump_normal(x, y):
    """The unit normal of the hemisphere grid at pixel (x, y).

    Between bumps the surface is flat, which is what the small remaining gaps
    at the grid's diagonals are - there is no way to tile circles without them,
    and they read as the mortar between cobbles.
    """
    cx = (math.floor(x / SPACING) + 0.5) * SPACING
    cy = (math.floor(y / SPACING) + 0.5) * SPACING
    dx = (x + 0.5 - cx) / RADIUS
    dy = (y + 0.5 - cy) / RADIUS
    d2 = dx * dx + dy * dy
    if d2 >= 1.0:
        return 0.0, 0.0, 1.0
    # A hemisphere's normal at (dx, dy) is just (dx, dy, sqrt(1 - d2)).
    return dx, dy, math.sqrt(1.0 - d2)


def main():
    random.seed(12)  # scene 12; reruns must not churn the file
    diffuse, normal = [], []
    for y in range(SIZE):
        drow, nrow = [], []
        for x in range(SIZE):
            nx, ny, nz = bump_normal(x, y)
            nrow += [
                int(round((nx * 0.5 + 0.5) * 255)),
                int(round((ny * 0.5 + 0.5) * 255)),
                int(round((nz * 0.5 + 0.5) * 255)),
                255,
            ]
            jitter = random.randint(-NOISE, NOISE)
            drow += [min(255, max(0, c + jitter)) for c in BASE] + [255]
        diffuse.append(drow)
        normal.append(nrow)

    import os

    sprites = os.path.join(os.path.dirname(__file__), "..", "assets", "sprites")
    write_png(os.path.join(sprites, "bumps128.png"), diffuse)
    write_png(os.path.join(sprites, "bumps128_n.png"), normal)
    print(f"wrote bumps128.png and bumps128_n.png ({GRID}x{GRID} hemispheres)")


if __name__ == "__main__":
    main()
