"""Standing props for Visual Showcase 2 - trees, lamps, barrels, crystals -
drawn as pixel-art cards that a stage billboard stands upright, as HD-2D
draws its foliage. Each returns a `Canvas` whose bottom row is where it
touches the ground."""

import numpy as np

from chars import selective_outline
from pixel import Canvas, ramp, rng, value_noise


def _blob_cluster(c, blobs, pal, seed, light=(-0.6, -0.8)):
    """Overlapping leaf clumps, each shaded by its own facing toward the
    light with ordered dither - a canopy."""
    ys, xs = np.mgrid[0:c.h, 0:c.w]
    n = value_noise(c.w, c.h, 4, seed)
    for (cx, cy, r) in blobs:
        d = np.hypot(xs + 0.5 - cx, ys + 0.5 - cy) / r
        m = d <= 1.0 + (n - 0.5) * 0.35
        facing = ((xs - cx) * light[0] + (ys - cy) * light[1]) / r
        v = 0.45 + 0.4 * facing - 0.25 * d + (n - 0.5) * 0.4
        bayer = np.tile(np.array([[0, 2], [3, 1]]) / 4.0, (c.h // 2 + 1, c.w // 2 + 1))[:c.h, :c.w]
        idx = np.clip(np.floor(v * (len(pal) - 1) + bayer), 0, len(pal) - 1).astype(int)
        c.rgba[m, :3] = pal[idx][m]
        c.rgba[m, 3] = 1
        c.height[m] = np.clip(1 - d, 0, 1)[m]


def oak(seed=21, palette=None, w=56, h=72):
    pal = palette if palette is not None else ramp("#16301e", "#1e4a26", "#2e6a30", "#4a8a38", "#6aa844", "#94c45a")
    c = Canvas(w, h)
    bark = ramp("#2a1c16", "#40281c", "#5a3a26", "#74503a")
    # Trunk, flaring at the root.
    for y in range(h - 30, h):
        t = (y - (h - 30)) / 30
        half = 3 + t * t * 4
        x0, x1 = int(w / 2 - half), int(w / 2 + half)
        c.rect(x0, y, x1, y + 1, bark[1])
        c.rect(x0, y, x0 + 1, y + 1, bark[3])
        c.rect(x1 - 2, y, x1, y + 1, bark[0])
    r = rng(seed)
    blobs = [(w / 2, h * 0.36, 17), (w / 2 - 13, h * 0.45, 12), (w / 2 + 13, h * 0.44, 12), (w / 2, h * 0.2, 12)]
    for _ in range(4):
        blobs.append((w / 2 + r.uniform(-14, 14), h * r.uniform(0.15, 0.5), r.uniform(6, 9)))
    _blob_cluster(c, blobs, pal, seed)
    selective_outline(c)
    return c


def pine(seed=22, palette=None, w=36, h=80, snow=False):
    pal = palette if palette is not None else ramp("#0e2420", "#163a2c", "#1e5034", "#2e6a3c", "#468446")
    c = Canvas(w, h)
    bark = ramp("#2a1c16", "#4a3020")
    c.rect(w // 2 - 2, h - 12, w // 2 + 2, h, bark[1])
    tiers = 5
    for i in range(tiers):
        top = 2 + i * (h - 16) / tiers
        bottom = top + (h - 16) / tiers * 1.5
        for y in range(int(top), int(min(bottom, h - 10))):
            t = (y - top) / (bottom - top)
            half = 3 + t * (6 + i * 2.6)
            for x in range(int(w / 2 - half), int(w / 2 + half) + 1):
                side = (x - w / 2) / max(half, 1)
                v = 0.55 - side * 0.35 + t * 0.15 - 0.35 * (t > 0.85)
                k = int(np.clip(v, 0, 0.999) * len(pal))
                c.put(x, y, pal[k], height=1 - abs(side))
            if snow and t < 0.4:
                # A cap of snow on each tier, lit on its left, thinning out.
                reach = half * (1.0 - t * 1.6)
                for x in range(int(w / 2 - half), int(w / 2 + reach) + 1):
                    side = (x - w / 2) / max(half, 1)
                    tone = 0.97 if side < 0 else 0.84
                    c.put(x, y, np.array([tone * 0.94, tone * 0.97, tone]), height=1.0)
    selective_outline(c)
    return c


def bush(seed=23, palette=None, w=28, h=18, berries=False):
    pal = palette if palette is not None else ramp("#16301e", "#245026", "#3a702e", "#5a903a", "#80b04a")
    c = Canvas(w, h)
    _blob_cluster(c, [(w * 0.3, h * 0.62, 8), (w * 0.62, h * 0.55, 9), (w * 0.48, h * 0.38, 7)], pal, seed)
    if berries:
        r = rng(seed)
        for _ in range(6):
            c.put(int(r.integers(4, w - 4)), int(r.integers(4, h - 4)), np.array([0.85, 0.2, 0.25]))
    selective_outline(c)
    return c


def lamp_post(lit=False, w=14, h=56):
    """An iron lamp post; `lit` draws only the glass, glowing, for an
    additive card over it."""
    c = Canvas(w, h)
    iron = ramp("#16141c", "#2a2832", "#46424e")
    if lit:
        c.rect(4, 6, w - 4, 14, np.array([1.0, 0.82, 0.5]))
        c.rect(5, 7, w - 5, 13, np.array([1.0, 0.95, 0.8]))
        return c
    c.rect(w // 2 - 1, 12, w // 2 + 1, h - 3, iron[1])
    c.rect(w // 2 - 1, 12, w // 2, h - 3, iron[2])
    c.rect(w // 2 - 3, h - 4, w // 2 + 3, h, iron[0])
    c.rect(3, 4, w - 3, 6, iron[0])
    c.rect(4, 6, w - 4, 14, np.array([0.35, 0.3, 0.22]))
    c.rect(3, 14, w - 3, 15, iron[0])
    c.rect(w // 2 - 1, 1, w // 2 + 1, 4, iron[1])
    selective_outline(c)
    return c


def barrel(w=14, h=18):
    c = Canvas(w, h)
    wood = ramp("#2e1c12", "#4a2e1c", "#6a4428", "#8a5e38", "#a87a4c")
    iron = ramp("#1a1a20", "#3a3a44")
    for x in range(w):
        t = (x + 0.5) / w
        bulge = 1 - (2 * t - 1) ** 2
        v = 0.25 + 0.55 * (1 - abs(t - 0.35) * 1.6) + 0.1 * bulge
        k = int(np.clip(v, 0, 0.999) * len(wood))
        c.rect(x, 1, x + 1, h, wood[k], height=bulge)
        if x % 3 == 0:
            c.rect(x, 1, x + 1, h, wood[max(0, k - 1)], height=bulge * 0.8)
    for y in (3, h - 4):
        c.rect(0, y, w, y + 1, iron[1], height=1.0)
    c.rect(1, 0, w - 1, 2, wood[1], height=0.6)
    selective_outline(c)
    return c


def crate(w=16, h=16):
    c = Canvas(w, h)
    wood = ramp("#3a2614", "#5a3e22", "#7a5a32", "#9a7644")
    c.rect(0, 0, w, h, wood[1], height=0.4)
    for y in range(2, h - 2, 3):
        c.rect(2, y, w - 2, y + 2, wood[2], height=0.6)
    c.rect(0, 0, w, 2, wood[3], height=1.0)
    c.rect(0, h - 2, w, h, wood[0], height=1.0)
    c.rect(0, 0, 2, h, wood[3], height=1.0)
    c.rect(w - 2, 0, w, h, wood[0], height=1.0)
    for i in range(w - 4):
        c.put(2 + i, 2 + i * (h - 4) // (w - 4), wood[3], height=1.0)
    selective_outline(c)
    return c


def crystal(seed=24, w=24, h=34, color=("#1a2a5a", "#2a4a9a", "#4a7ad8", "#8ab4f4", "#d8ecff")):
    """A cluster of crystal spires, lit from inside: the card is drawn unlit
    and glows."""
    pal = ramp(*color)
    c = Canvas(w, h)
    r = rng(seed)
    spires = [(w / 2, h - 1, 5, h - 3)]
    for _ in range(4):
        spires.append((w / 2 + r.uniform(-8, 8), h - 1, r.uniform(2.5, 4), r.uniform(h * 0.35, h * 0.75)))
    for (bx, by, half, tall) in sorted(spires, key=lambda s: -s[3]):
        lean = r.uniform(-0.25, 0.25)
        for y in range(int(by - tall), int(by)):
            t = (by - y) / tall
            hw = half * (1 - t ** 3) if t < 0.85 else half * (1 - t) * 4
            cx = bx + lean * (by - y)
            for x in range(int(cx - hw), int(cx + hw) + 1):
                side = (x - cx) / max(hw, 0.5)
                v = 0.4 + 0.45 * t - 0.4 * side
                c.put(x, y, pal[int(np.clip(v, 0, 0.999) * len(pal))], height=1 - abs(side))
            c.put(int(cx - hw * 0.3), y, pal[-1])
    selective_outline(c)
    return c


def stalagmite(seed=25, w=14, h=34):
    pal = ramp("#141220", "#24202e", "#383242", "#4e4658", "#6a6072")
    c = Canvas(w, h)
    for y in range(h):
        t = y / h
        half = 1 + t ** 1.5 * (w / 2 - 1)
        for x in range(int(w / 2 - half), int(w / 2 + half) + 1):
            side = (x - w / 2) / max(half, 1)
            v = 0.5 - side * 0.4 + 0.1 * np.sin(y * 0.9)
            c.put(x, y, pal[int(np.clip(v, 0, 0.999) * len(pal))], height=1 - abs(side))
    selective_outline(c)
    return c


def torch_bracket(w=8, h=16):
    c = Canvas(w, h)
    iron = ramp("#16141c", "#34303c", "#504a58")
    c.rect(w // 2 - 1, 5, w // 2 + 1, h, iron[1])
    c.rect(1, 3, w - 1, 6, iron[2])
    c.rect(2, 0, w - 2, 3, np.array([0.35, 0.2, 0.1]))
    selective_outline(c)
    return c


def shrine_lantern(w=16, h=34):
    """A stone lantern: base, post, a firebox with a dark opening."""
    pal = ramp("#2a2a2e", "#46464c", "#62626a", "#808088", "#9e9ea6")
    c = Canvas(w, h)

    def stone(x0, y0, x1, y1):
        for x in range(x0, x1):
            side = (x - (x0 + x1) / 2) / ((x1 - x0) / 2)
            v = 0.55 - side * 0.35
            c.rect(x, y0, x + 1, y1, pal[int(np.clip(v, 0, 0.999) * len(pal))], height=1 - abs(side))

    stone(2, h - 4, w - 2, h)
    stone(5, h - 16, w - 5, h - 4)
    stone(3, h - 24, w - 3, h - 16)
    c.rect(5, h - 22, w - 5, h - 18, np.array([0.12, 0.08, 0.06]))
    stone(0, h - 28, w, h - 24)
    stone(5, h - 31, w - 5, h - 28)
    stone(7, h - 34, w - 7, h - 31)
    selective_outline(c)
    return c


def fence(w=32, h=14):
    c = Canvas(w, h)
    wood = ramp("#3a2614", "#5a3e22", "#7a5a32", "#9a7644")
    for x in (2, w // 2, w - 4):
        c.rect(x, 0, x + 3, h, wood[2], height=1.0)
        c.rect(x, 0, x + 1, h, wood[3])
    for y in (3, 8):
        c.rect(0, y, w, y + 2, wood[1], height=0.8)
        c.rect(0, y, w, y + 1, wood[2])
    selective_outline(c)
    return c


def signpost(w=20, h=30):
    c = Canvas(w, h)
    wood = ramp("#3a2614", "#5a3e22", "#7a5a32", "#9a7644")
    c.rect(w // 2 - 1, 6, w // 2 + 2, h, wood[1])
    c.rect(1, 4, w - 1, 13, wood[2], height=0.8)
    c.rect(1, 4, w - 1, 5, wood[3])
    for y in (7, 10):
        c.rect(4, y, w - 4, y + 1, wood[0])
    selective_outline(c)
    return c


def table(w=30, h=14):
    c = Canvas(w, h)
    wood = ramp("#2e1c12", "#4a2e1c", "#6a4428", "#8a5e38")
    c.rect(0, 0, w, 4, wood[2], height=1.0)
    c.rect(0, 0, w, 1, wood[3])
    c.rect(0, 3, w, 4, wood[0])
    for x in (2, w - 5):
        c.rect(x, 4, x + 3, h, wood[1], height=0.8)
    # A mug and a candle on it.
    c.rect(6, -3 + 3, 9, 1, np.array([0.55, 0.4, 0.25]))
    selective_outline(c)
    return c


def grass_tuft(seed=26, w=24, h=20, palette=None):
    """Tall blades fanning out of one root - the foreground a lens blurs."""
    pal = palette if palette is not None else ramp("#16301e", "#24482a", "#3a6a32", "#58903c", "#7cb04a", "#a4cc62")
    c = Canvas(w, h)
    r = rng(seed)
    blades = sorted(((r.uniform(-1.0, 1.0), r.uniform(0.35, 1.0)) for _ in range(18)), key=lambda b: b[1])
    for lean, tall in blades:
        top = h * (1 - tall)
        base_x = w / 2 + lean * 4 + r.uniform(-1.5, 1.5)
        for y in range(int(top), h):
            t = (h - y) / (h - top)
            x = base_x + lean * (w / 2 - 4) * t ** 1.6
            half = 0.7 * (1 - t) + 0.2
            for px in range(int(x - half), int(x + half) + 1):
                side = (px - x) / max(half, 0.5)
                v = 0.25 + 0.6 * t - 0.25 * side
                c.put(px, y, pal[int(np.clip(v, 0, 0.999) * len(pal))], height=1 - abs(side))
    selective_outline(c)
    return c


def banner(seed=27, w=20, h=64, cloth=("#3a0c10", "#6a1418", "#9a2420", "#c4402a"), trim="#d8a84a"):
    """A war banner on a pole, its cloth torn ragged at the foot."""
    c = Canvas(w, h)
    wood = ramp("#2a1c12", "#4a3020", "#6a4a30")
    c.rect(2, 2, 4, h, wood[1], height=1.0)
    c.rect(2, 2, 3, h, wood[2])
    c.rect(1, 0, 5, 3, np.array([0.75, 0.6, 0.3]), height=1.0)
    c.rect(2, 4, w - 1, 6, wood[1], height=0.8)
    pal = ramp(*cloth)
    r = rng(seed)
    n = value_noise(w, h, 4, seed)
    rag = [int(h * 0.62 + r.uniform(-5, 7)) for _ in range(w)]
    for x in range(4, w - 1):
        t = (x - 4) / (w - 5)
        for y in range(6, rag[x]):
            fold = 0.5 + 0.35 * np.sin(x * 0.9 + y * 0.12)
            v = fold - 0.25 * t + (n[y, x] - 0.5) * 0.3
            c.put(x, y, pal[int(np.clip(v, 0, 0.999) * len(pal))], height=fold)
    tr = ramp(trim)[0]
    for x in range(4, w - 1):
        c.put(x, 8, tr, height=1.0)
    # An emblem: a diamond in the trim colour.
    cx, cy = (w + 3) // 2, int(h * 0.32)
    for dy in range(-5, 6):
        for dx in range(-(5 - abs(dy)) // 2, (5 - abs(dy)) // 2 + 1):
            c.put(cx + dx, cy + dy, tr, height=1.0)
    selective_outline(c)
    return c


def mug(w=6, h=7):
    c = Canvas(w, h)
    c.rect(0, 1, 4, h, np.array([0.55, 0.38, 0.22]))
    c.rect(0, 1, 4, 2, np.array([0.95, 0.9, 0.75]))
    c.rect(4, 2, 6, 5, np.array([0.45, 0.3, 0.18]))
    selective_outline(c)
    return c
