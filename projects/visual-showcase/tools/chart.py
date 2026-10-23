"""The sea chart of 29 Atlas of the Deep, decided once.

`gen_textures.py` paints it and `gen_scenes.py` labels it, so both read the
same land from here: islands where a seeded noise field rises past a level,
named by size, with harbours on their coasts. Pure numpy, deterministic.
"""

import numpy as np

SIZE = (2048, 1280)            # the chart texture, pixels
WORLD = (2560.0, 1600.0)       # what it covers, world units
LAND = 0.6                     # the field's level at the coast
_RNG = np.random.default_rng(2929)


def _value_noise(w, h, cells, rng):
    """Smooth value noise, `cells` lattice cells across the width."""
    gw, gh = cells + 2, int(cells * h / w) + 3
    grid = rng.random((gh, gw))
    ys = np.linspace(0, gh - 2.0001, h)
    xs = np.linspace(0, cells, w)
    x0, y0 = np.floor(xs).astype(int), np.floor(ys).astype(int)
    fx, fy = xs - x0, ys - y0
    sx, sy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a = grid[y0[:, None], x0[None, :]]
    b = grid[y0[:, None], x0[None, :] + 1]
    c = grid[y0[:, None] + 1, x0[None, :]]
    d = grid[y0[:, None] + 1, x0[None, :] + 1]
    top = a + (b - a) * sx[None, :]
    bot = c + (d - c) * sx[None, :]
    return top + (bot - top) * sy[:, None]


_FIELD = None


def field():
    """The height of the sea floor, 0..1; land where it is over `LAND`.
    Falls away toward the chart's edges so the land sits inside the frame."""
    global _FIELD
    if _FIELD is None:
        w, h = SIZE
        rng = np.random.default_rng(2929)
        f = np.zeros((h, w))
        for cells, k in ((5, 1.0), (11, 0.5), (23, 0.25), (47, 0.12)):
            f += k * _value_noise(w, h, cells, rng)
        f = (f - f.min()) / (f.max() - f.min())
        yy, xx = np.mgrid[0:h, 0:w]
        ex = np.minimum(xx, w - 1 - xx) / (w * 0.12)
        ey = np.minimum(yy, h - 1 - yy) / (h * 0.12)
        edge = np.clip(np.minimum(ex, ey), 0, 1)
        _FIELD = f * (0.55 + 0.45 * edge)
    return _FIELD


def to_world(px, py):
    w, h = SIZE
    return (round((px / w - 0.5) * WORLD[0], 1), round((0.5 - py / h) * WORLD[1], 1))


def islands(step=8, min_cells=6):
    """The islands, largest first: (cells, centroid px, centroid py, extent
    px), found by flood fill on a grid of `step` pixels."""
    f = field()[::step, ::step]
    land = f > LAND
    h, w = land.shape
    seen = np.zeros_like(land)
    out = []
    for y in range(h):
        for x in range(w):
            if not land[y, x] or seen[y, x]:
                continue
            stack, cells = [(y, x)], []
            seen[y, x] = True
            while stack:
                cy, cx = stack.pop()
                cells.append((cy, cx))
                for ny, nx in ((cy + 1, cx), (cy - 1, cx), (cy, cx + 1), (cy, cx - 1)):
                    if 0 <= ny < h and 0 <= nx < w and land[ny, nx] and not seen[ny, nx]:
                        seen[ny, nx] = True
                        stack.append((ny, nx))
            if len(cells) >= min_cells:
                ys = np.array([c[0] for c in cells]) * step
                xs = np.array([c[1] for c in cells]) * step
                out.append((len(cells), float(xs.mean()), float(ys.mean()), float(max(np.ptp(xs), np.ptp(ys))), cells))
    out.sort(key=lambda i: -i[0])
    return out


ISLAND_NAMES = ["Wrenholm", "Saltmarsh", "Ember Isle", "Tarrow", "Gull Rock", "Lantern Cay",
                "Hollow Reef", "Brine Point", "Corran", "The Teeth"]
TOWN_NAMES = ["Port Ashby", "Kettlewick", "Marrow Bay", "Fennick", "Quay of Lamps", "Holloway",
              "Stillwater", "Gannet's Rest", "Tidesend", "Oakharbour", "Weir", "Saltcote"]


def places():
    """Every label: (kind, name, world x, world y, size hint)."""
    out = []
    f = field()
    step = 8
    names = iter(ISLAND_NAMES)
    towns = iter(TOWN_NAMES)
    for n, cx, cy, extent, cells in islands(step)[:len(ISLAND_NAMES)]:
        x, y = to_world(cx, cy)
        out.append(("isle", next(names), x, y, extent))
        # Harbours: coastal cells, the ones furthest apart.
        coast = []
        for (gy, gx) in cells:
            py, px = gy * step, gx * step
            ring = f[max(py - step, 0):py + step + 1, max(px - step, 0):px + step + 1]
            if (ring < LAND).any():
                coast.append((px, py))
        if not coast or n < 40:
            continue
        coast.sort()
        for px, py in (coast[0], coast[-1])[: 2 if n > 120 else 1]:
            name = next(towns, None)
            if name is None:
                break
            x, y = to_world(px, py)
            out.append(("town", name, x, y, 0))
    return out


def deepest(k=2):
    """The `k` deepest open-water points, far apart: where the sea names go."""
    f = field()
    w, h = SIZE
    picks = []
    blur = f[::16, ::16]
    order = np.dstack(np.unravel_index(np.argsort(blur.ravel()), blur.shape))[0]
    for gy, gx in order:
        py, px = gy * 16, gx * 16
        if px < w * 0.12 or px > w * 0.88 or py < h * 0.15 or py > h * 0.85:
            continue
        if all(abs(px - a) + abs(py - b) > w * 0.35 for a, b in picks):
            picks.append((px, py))
        if len(picks) == k:
            break
    return [to_world(px, py) for px, py in picks]
