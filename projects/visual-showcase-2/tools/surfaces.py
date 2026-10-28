"""Scenery surfaces for Visual Showcase 2: tiles drawn at art resolution
with a height field each, for a normal map. Every function returns a
`Canvas`; `gen_art.py` writes them four times larger.

The palettes are ramps chosen dark to light with a hue shift - shadows go
cooler and lights warmer - which is most of what makes pixel shading read as
painted rather than as a brightness slider.
"""

import numpy as np

from pixel import Canvas, ramp, rng, shade, value_noise


def cobbles(seed=1, size=32, palette=None):
    """Rounded stones in staggered rows, dark mortar between."""
    pal = palette if palette is not None else ramp("#2a2430", "#4a4048", "#6a5e60", "#8a7c74", "#a8988a", "#c4b4a0")
    c = Canvas(size, size)
    r = rng(seed)
    mortar = ramp("#1c1820", "#2a2428")
    c.rect(0, 0, size, size, mortar[1], height=0.0)
    rows = 4
    rh = size // rows
    noise = value_noise(size, size, 8, seed + 7)
    ys, xs = np.mgrid[0:size, 0:size]
    for row in range(rows):
        off = (row % 2) * 4
        x = -off
        while x < size:
            w = int(r.integers(6, 11))
            cx, cy = x + w / 2, row * rh + rh / 2
            rx, ry = w / 2 - 0.6, rh / 2 - 0.7
            for wrap in (-size, 0, size):
                d = ((xs + 0.5 - (cx + wrap)) / rx) ** 2 + ((ys + 0.5 - cy) / ry) ** 2
                m = d <= 1.0
                dome = np.sqrt(np.clip(1 - d, 0, 1))
                tone = float(r.random()) * 0.35 + 0.35
                # Lit from the upper left: brighter there, darker low right.
                light = tone + 0.3 * dome - 0.25 * np.clip((xs - cx - wrap) / rx + (ys - cy) / ry, -1, 1) * 0.5
                light = light + (noise - 0.5) * 0.25
                vals = np.clip(light, 0, 1)
                c.rgba[m, :3] = shade(np.where(m, vals, 0), pal)[m]
                c.rgba[m, 3] = 1
                c.height[m] = 0.3 + 0.7 * dome[m]
            x += w
    return c


def grass(seed=2, size=32, flowers=True, palette=None):
    pal = palette if palette is not None else ramp("#1e3a2a", "#2a5a32", "#3e7a38", "#5a9a40", "#7ab44a", "#a0cc60")
    c = Canvas(size, size)
    n = value_noise(size, size, 16, seed) * 0.6 + value_noise(size, size, 4, seed + 1) * 0.4
    c.rgba[:, :, :3] = shade(n * 0.8 + 0.1, pal, 0.8)
    c.rgba[:, :, 3] = 1
    c.height[:] = n * 0.4
    r = rng(seed + 3)
    # Blades: short bright strokes with a dark root.
    for _ in range(size * 3):
        x, y = int(r.integers(0, size)), int(r.integers(1, size))
        c.put(x, y, pal[-1] if r.random() < 0.4 else pal[-2], height=0.8)
        c.put(x, (y + 1) % size, pal[1], height=0.2)
    if flowers:
        for col in ("#f0e070", "#f0f0f0", "#e07090"):
            for _ in range(3):
                x, y = int(r.integers(0, size)), int(r.integers(0, size))
                c.put(x, y, np.array([int(col[i:i + 2], 16) / 255 for i in (1, 3, 5)]), height=1.0)
    return c


def dirt(seed=3, size=32, palette=None):
    pal = palette if palette is not None else ramp("#3a2a22", "#5a4030", "#7a5a40", "#9a7652", "#b8946a")
    c = Canvas(size, size)
    n = value_noise(size, size, 8, seed) * 0.7 + value_noise(size, size, 2, seed + 1) * 0.3
    c.rgba[:, :, :3] = shade(n, pal, 0.9)
    c.rgba[:, :, 3] = 1
    c.height[:] = n * 0.5
    r = rng(seed + 4)
    for _ in range(18):
        x, y = int(r.integers(0, size)), int(r.integers(0, size))
        c.put(x, y, pal[-1], height=0.9)
        c.put(x + 1, y + 1, pal[0], height=0.1)
    return c


def plaster(seed=4, size=32, palette=None):
    pal = palette if palette is not None else ramp("#8a7a68", "#a8987e", "#c4b496", "#dccca8", "#ece0bc")
    c = Canvas(size, size)
    n = value_noise(size, size, 8, seed) * 0.6 + value_noise(size, size, 2, seed + 2) * 0.4
    c.rgba[:, :, :3] = shade(n * 0.6 + 0.3, pal, 0.7)
    c.rgba[:, :, 3] = 1
    c.height[:] = n * 0.35
    return c


def timber(seed=5, size=32, plaster_pal=None, wood=None):
    """Plaster in a timber frame: posts at both edges, a rail across the
    middle and a brace - a tile that repeats into a half-timbered wall."""
    c = plaster(seed, size, plaster_pal)
    wd = wood if wood is not None else ramp("#2a1a14", "#3e2a1e", "#5a3e28", "#74523a")
    n = value_noise(size, size, 4, seed + 9)

    def beam(mask):
        vals = 0.3 + 0.5 * n
        c.rgba[mask, :3] = shade(np.where(mask, vals, 0), wd, 0.6)[mask]
        c.height[mask] = 0.9

    ys, xs = np.mgrid[0:size, 0:size]
    beam((xs < 3) | (xs >= size - 1))
    beam((ys >= size // 2 - 1) & (ys < size // 2 + 2))
    beam((ys < 2))
    # A brace across the lower half.
    t = (xs - 3) / (size - 4)
    beam((ys >= size // 2 + 2) & (np.abs(ys - (size // 2 + 2 + t * (size // 2 - 2))) < 1.6) & (xs >= 3))
    # A shadowed edge under each beam's lower side.
    return c


def bricks(seed=6, size=32, palette=None, rows=6):
    pal = palette if palette is not None else ramp("#3a3038", "#5a4a4a", "#786458", "#968070", "#b09a84")
    c = Canvas(size, size)
    mortar = ramp("#2a2428", "#3e363a")
    c.rect(0, 0, size, size, mortar[1], height=0.0)
    r = rng(seed)
    rh = size // rows
    n = value_noise(size, size, 4, seed + 5)
    for row in range(rows):
        off = (row % 2) * (size // 4)
        for col in range(-1, 3):
            x0 = col * (size // 2) + off + 1
            x1 = x0 + size // 2 - 1
            y0, y1 = row * rh + 1, row * rh + rh
            tone = float(r.random()) * 0.3 + 0.35
            for x in range(x0, x1):
                xx = x % size
                for y in range(y0, y1):
                    v = tone + (n[y, xx] - 0.5) * 0.3
                    if y == y0:
                        v += 0.2
                    if y == y1 - 1 or x == x1 - 1:
                        v -= 0.2
                    c.rgba[y, xx, :3] = pal[int(np.clip(v, 0, 0.999) * len(pal))]
                    c.rgba[y, xx, 3] = 1
                    c.height[y, xx] = 0.8 if y > y0 else 0.95
    return c


def roof(seed=7, size=32, palette=None, rows=4):
    """Overlapping tiles in rows: each row's bottom edge casts a dark line
    and the tiles round off at their lower ends."""
    pal = palette if palette is not None else ramp("#3a1418", "#5e2020", "#8a3226", "#ac4a30", "#c86a3e")
    c = Canvas(size, size)
    rh = size // rows
    n = value_noise(size, size, 4, seed)
    ys, xs = np.mgrid[0:size, 0:size]
    for row in range(rows):
        off = (row % 2) * 2
        y0 = row * rh
        tw = 4
        for x in range(size):
            k = ((x + off) % tw)
            for y in range(y0, y0 + rh):
                t = (y - y0) / rh
                v = 0.25 + 0.6 * t + (n[y, x] - 0.5) * 0.2
                if k == 0:
                    v -= 0.3
                if k == 1:
                    v += 0.12
                if y == y0 + rh - 1:
                    v -= 0.15
                c.rgba[y, x, :3] = pal[int(np.clip(v, 0, 0.999) * len(pal))]
                c.rgba[y, x, 3] = 1
                c.height[y, x] = t * 0.9 + (0.0 if k == 0 else 0.1)
    return c


def planks(seed=8, size=32, palette=None, boards=4, vertical=False):
    pal = palette if palette is not None else ramp("#2e1c14", "#4a3020", "#68462e", "#86603e", "#a07a52")
    c = Canvas(size, size)
    r = rng(seed)
    grain = value_noise(size, size, 2, seed + 3)
    bw = size // boards
    for b in range(boards):
        tone = float(r.random()) * 0.25 + 0.35
        for i in range(bw):
            for j in range(size):
                y, x = (j, b * bw + i) if vertical else (b * bw + i, j)
                v = tone + (grain[y, x] - 0.5) * 0.35
                if i == 0:
                    v += 0.15
                if i == bw - 1:
                    v = 0.05
                c.rgba[y, x, :3] = pal[int(np.clip(v, 0, 0.999) * len(pal))]
                c.rgba[y, x, 3] = 1
                c.height[y, x] = 0.7 if i < bw - 1 else 0.1
        # A nail pair at the board's end.
        y = b * bw + bw // 2
        for x in (2, size - 3):
            if vertical:
                c.put(y, x, pal[0], height=0.5)
            else:
                c.put(x, y, pal[0], height=0.5)
    return c


def rock(seed=9, size=32, palette=None):
    pal = palette if palette is not None else ramp("#16141e", "#262432", "#3a3646", "#524c5c", "#6e6676", "#8c8292")
    c = Canvas(size, size)
    n = value_noise(size, size, 16, seed, 4)
    cracks = value_noise(size, size, 8, seed + 1)
    h = n * 0.8 + 0.2 * np.abs(cracks - 0.5)
    # Lit from the upper left: the height's slope shades it.
    dx = np.roll(h, -1, 1) - np.roll(h, 1, 1)
    dy = np.roll(h, -1, 0) - np.roll(h, 1, 0)
    v = 0.45 + (h - 0.5) * 0.6 - (dx + dy) * 3.0
    c.rgba[:, :, :3] = shade(np.clip(v, 0, 1), pal, 0.8)
    c.rgba[:, :, 3] = 1
    c.height[:] = h
    return c


def snow(seed=10, size=32, palette=None):
    pal = palette if palette is not None else ramp("#7a8aa8", "#9eaec8", "#c0cce0", "#dce4f0", "#f2f6fc")
    c = Canvas(size, size)
    n = value_noise(size, size, 16, seed) * 0.7 + value_noise(size, size, 4, seed + 1) * 0.3
    c.rgba[:, :, :3] = shade(n * 0.5 + 0.45, pal, 0.6)
    c.rgba[:, :, 3] = 1
    c.height[:] = n * 0.5
    r = rng(seed + 2)
    for _ in range(10):
        c.put(int(r.integers(0, size)), int(r.integers(0, size)), np.array([1.0, 1.0, 1.0]), height=0.8)
    return c


def water(seed=11, size=32, palette=None):
    pal = palette if palette is not None else ramp("#10243a", "#163450", "#1e4a68", "#2a6280", "#4a8aa4", "#8ac0d0")
    c = Canvas(size, size)
    ys, xs = np.mgrid[0:size, 0:size]
    n = value_noise(size, size, 8, seed)
    ripple = np.sin((xs * 0.4 + n * 6) + ys * 1.1) * 0.5 + 0.5
    v = 0.25 + n * 0.35 + (ripple > 0.92) * 0.4
    c.rgba[:, :, :3] = shade(np.clip(v, 0, 1), pal, 0.5)
    c.rgba[:, :, 3] = 1
    c.height[:] = ripple * 0.3
    return c


def window(lit=False, size=16):
    """A window: a timber frame, four panes. `lit` draws only the panes, in
    warm light, for an additive quad laid over the dark one."""
    c = Canvas(size, size + 4)
    wood = ramp("#2a1a14", "#4a3020", "#6a4a30")
    if not lit:
        c.rect(0, 0, size, size + 4, wood[1], height=0.9)
        c.rect(0, size + 2, size, size + 4, wood[2], height=1.0)
        c.rect(2, 2, size - 2, size, np.array([0.08, 0.09, 0.14]), height=0.2)
        c.rect(size // 2 - 1, 2, size // 2 + 1, size, wood[0], height=0.8)
        c.rect(2, size // 2, size - 2, size // 2 + 1, wood[0], height=0.8)
        c.put(3, 3, np.array([0.3, 0.34, 0.45]))
    else:
        warm = np.array([1.0, 0.72, 0.36])
        for x0, x1 in ((2, size // 2 - 1), (size // 2 + 1, size - 2)):
            for y0, y1 in ((2, size // 2), (size // 2 + 1, size)):
                c.rect(x0, y0, x1, y1, warm)
                c.rect(x0, y0, x1, y0 + 1, warm * 0.8)
    return c


def door(size=16):
    c = Canvas(size, size * 2)
    wood = ramp("#2a1a10", "#4a2e1c", "#6a4428", "#865a36")
    c.rect(0, 0, size, size * 2, wood[0], height=1.0)
    for i, x in enumerate(range(2, size - 2, 3)):
        c.rect(x, 2, x + 3, size * 2, wood[1 + i % 3], height=0.6)
        c.rect(x + 2, 2, x + 3, size * 2, wood[0], height=0.4)
    c.rect(2, 8, size - 2, 10, wood[0], height=0.9)
    c.rect(2, size * 2 - 10, size - 2, size * 2 - 8, wood[0], height=0.9)
    c.put(size - 5, size, np.array([0.85, 0.7, 0.3]), height=1.0)
    return c
