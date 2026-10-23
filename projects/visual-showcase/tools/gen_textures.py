r"""Write the showcase's procedural textures into assets/sprites/.

Everything here is generated rather than drawn, so a texture can be retuned
by changing a number and re-running - and so the showcase measures what the
*renderer* can do with plain inputs, not what a painter can hide.

FX textures are white with the shape in alpha: the sprite's colour tints
them, so one glow serves every light in every scene. Remember colours in
this engine are linear (CLAUDE.md, "Colours are linear") - a texture is
uploaded as sRGB, which is why these are pure white rather than grey.

Keep generated art **out of `assets/sprites/` packing**: the project's asset
pipeline has atlas packing off, so these load as they are.

Re-run after editing:
    python tools/gen_textures.py      (from the project directory)
"""
import math
from pathlib import Path

import numpy as np
from PIL import Image

OUT = Path(__file__).resolve().parent.parent / "assets" / "sprites"
RNG = np.random.default_rng(7)


def save_rgba(path, rgb, alpha):
    """rgb: HxWx3 in 0..1 (or a scalar), alpha: HxW in 0..1."""
    h, w = alpha.shape
    img = np.empty((h, w, 4), dtype=np.float64)
    img[..., :3] = rgb
    img[..., 3] = alpha
    data = (np.clip(img, 0.0, 1.0) * 255.0 + 0.5).astype(np.uint8)
    path.parent.mkdir(parents=True, exist_ok=True)
    Image.fromarray(data, "RGBA").save(path)
    print(f"wrote {path.relative_to(OUT.parent.parent)}  {w}x{h}")


def radius(size_w, size_h=None):
    """Normalised distance from the centre: 0 at the middle, 1 at the edge."""
    size_h = size_h or size_w
    y, x = np.mgrid[0:size_h, 0:size_w].astype(np.float64)
    nx = (x + 0.5) / size_w * 2.0 - 1.0
    ny = (y + 0.5) / size_h * 2.0 - 1.0
    return nx, ny, np.sqrt(nx * nx + ny * ny)


def smoothstep(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


# ── tileable value noise ────────────────────────────────────────────────

def tile_noise(size, cells):
    """Smooth value noise that wraps at the edges (so a tile has no seam)."""
    grid = RNG.random((cells, cells))
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    fx = x / size * cells
    fy = y / size * cells
    x0 = np.floor(fx).astype(int) % cells
    y0 = np.floor(fy).astype(int) % cells
    x1 = (x0 + 1) % cells
    y1 = (y0 + 1) % cells
    tx = smoothstep(0.0, 1.0, fx - np.floor(fx))
    ty = smoothstep(0.0, 1.0, fy - np.floor(fy))
    a = grid[y0, x0] * (1 - tx) + grid[y0, x1] * tx
    b = grid[y1, x0] * (1 - tx) + grid[y1, x1] * tx
    return a * (1 - ty) + b * ty


def fbm(size, base_cells, octaves):
    total = np.zeros((size, size))
    amp, norm = 1.0, 0.0
    cells = base_cells
    for _ in range(octaves):
        total += tile_noise(size, cells) * amp
        norm += amp
        amp *= 0.5
        cells *= 2
    return total / norm


# ── FX ──────────────────────────────────────────────────────────────────

def glow():
    """A light's halo: a bright core and a long tail, which is what makes an
    additive sprite read as light rather than as a disc."""
    _, _, r = radius(256)
    core = np.exp(-(r * r) / 0.012)
    tail = np.clip(1.0 - r, 0.0, 1.0) ** 2.6
    a = np.clip(core * 0.55 + tail * 0.75, 0.0, 1.0) * smoothstep(1.0, 0.92, r)
    save_rgba(OUT / "fx" / "glow.png", 1.0, a)


def soft_dot():
    """The particle default: a round, soft-edged dot."""
    _, _, r = radius(64)
    a = smoothstep(1.0, 0.0, r) ** 1.6
    save_rgba(OUT / "fx" / "soft_dot.png", 1.0, a)


def ember():
    """A tiny hot core with a halo, for sparks that should bloom."""
    _, _, r = radius(32)
    a = np.clip(np.exp(-(r * r) / 0.05) + 0.35 * smoothstep(1.0, 0.0, r) ** 2, 0.0, 1.0)
    save_rgba(OUT / "fx" / "ember.png", 1.0, a)


def spark():
    """A streak, long on x, for motion-stretched sparks and rain."""
    nx, ny, _ = radius(128, 16)
    a = smoothstep(1.0, 0.0, np.abs(ny)) ** 1.5 * smoothstep(1.0, 0.2, np.abs(nx))
    save_rgba(OUT / "fx" / "spark.png", 1.0, a)


def ring():
    _, _, r = radius(256)
    a = np.exp(-((r - 0.82) ** 2) / 0.004) * smoothstep(1.0, 0.95, r)
    save_rgba(OUT / "fx" / "ring.png", 1.0, a)


def rift():
    """Through the Window's portal, torn rather than round: a ragged blob
    with cracks running out of it (`fx/rift_mask.png`, the `sprite_mask`
    texture - only its alpha is read) and a glow hugging that same edge
    (`fx/rift_rim.png`, drawn over it). A shape mask could not be this: the
    outline is painted."""
    from PIL import ImageFilter
    w, h = 360, 504
    rng = np.random.default_rng(28)
    nx, ny, r = radius(w, h)
    theta = np.arctan2(ny, nx)
    # The edge wanders: a few slow lobes and a fine tear.
    edge = 0.72 * np.ones_like(theta)
    for k, amp in ((2, 0.06), (3, 0.05), (5, 0.035), (9, 0.022), (17, 0.014), (31, 0.008)):
        edge += amp * np.sin(k * theta + rng.random() * 6.283)
    body = smoothstep(edge + 0.012, edge - 0.012, r)
    # Cracks: thin wedges reaching out past the edge, tapering.
    for _ in range(7):
        a0 = rng.random() * 6.283
        reach = 0.12 + rng.random() * 0.14
        bend = (rng.random() - 0.5) * 1.2
        out = np.clip((r - edge) / reach, 0.0, 1.0)
        along = theta - (a0 + bend * out)
        along = (along + np.pi) % (2 * np.pi) - np.pi
        width = 0.05 * (1.0 - out) ** 1.4
        crack = smoothstep(width + 0.004, width - 0.004, np.abs(along) * r) * (r > edge - 0.02) * (out < 1.0)
        body = np.maximum(body, crack)
    save_rgba(OUT / "fx" / "rift_mask.png", 1.0, body)

    def blur(a, s):
        img = Image.fromarray((np.clip(a, 0, 1) * 255).astype(np.uint8))
        return np.asarray(img.filter(ImageFilter.GaussianBlur(s)), dtype=np.float64) / 255.0

    outer = blur(body, 9) * (1.0 - body)
    inner = body * (1.0 - blur(body, 3))
    rim = np.clip(outer * 2.6 + inner * 2.2, 0.0, 1.0)
    save_rgba(OUT / "fx" / "rift_rim.png", 1.0, rim)


def smoke():
    """A lumpy puff: soft disc eaten by noise, so a crowd of them reads as
    smoke rather than as a crowd of discs."""
    size = 128
    _, _, r = radius(size)
    n = fbm(size, 4, 4)
    a = smoothstep(1.0, 0.1, r) * smoothstep(0.25, 0.75, n + 0.35 * (1.0 - r))
    save_rgba(OUT / "fx" / "smoke.png", 1.0, a ** 1.3)


def flame():
    """A teardrop, tall on y, bright at the bottom - the shape of one tongue
    of fire. Anchored at its base by the scene."""
    w, h = 64, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    nx = (x + 0.5) / w * 2.0 - 1.0
    t = (y + 0.5) / h            # 0 at the top (tip), 1 at the bottom (base)
    width = np.clip(np.sin(np.clip(t, 0, 1) * np.pi * 0.92) ** 0.7, 0.02, 1.0) * (0.35 + 0.65 * t)
    body = smoothstep(1.0, 0.35, np.abs(nx) / width)
    a = body * smoothstep(0.0, 0.35, t) * smoothstep(1.0, 0.9, t)
    save_rgba(OUT / "fx" / "flame.png", 1.0, a)


# ── environment ─────────────────────────────────────────────────────────

def flagstones():
    """A tileable flagstone floor and its normal map.

    The stones are a Voronoi pattern over jittered points, repeated across
    the texture's edges so it tiles: each pixel belongs to its nearest
    point, and the mortar runs where the nearest two are about equally far
    (F2 - F1 is small). Irregular stones are what make a floor read as
    laid stone; a square grid reads as bathroom tile.

    Height comes from each stone's own lift and tilt, bevelled at its edge,
    plus a little noise; the normal map is that height's gradient, in the
    convention the lit sprite pass reads (+y up, green = up).
    """
    size = 512
    cells = 5
    step = size / cells
    pts = []
    for j in range(cells):
        for i in range(cells):
            pts.append(((i + 0.5 + RNG.uniform(-0.38, 0.38)) * step,
                        (j + 0.5 + RNG.uniform(-0.38, 0.38)) * step))
    n = len(pts)

    y, x = np.mgrid[0:size, 0:size].astype(np.float32)
    f1 = np.full((size, size), 1e9, dtype=np.float32)
    f2 = np.full((size, size), 1e9, dtype=np.float32)
    owner = np.zeros((size, size), dtype=np.int32)
    ox = np.zeros((size, size), dtype=np.float32)
    oy = np.zeros((size, size), dtype=np.float32)
    for k, (px, py) in enumerate(pts):
        for wx in (-size, 0, size):
            for wy in (-size, 0, size):
                dx = x - (px + wx)
                dy = y - (py + wy)
                d = np.sqrt(dx * dx + dy * dy)
                nearer = d < f1
                f2 = np.where(nearer, f1, np.minimum(f2, d))
                f1 = np.where(nearer, d, f1)
                owner = np.where(nearer, k, owner)
                ox = np.where(nearer, dx, ox)
                oy = np.where(nearer, dy, oy)
    from_edge = (f2 - f1).astype(np.float64) * 0.5
    gap = 2.5
    inside = from_edge > gap
    bevel = smoothstep(gap, gap + 9.0, from_edge)

    lift = RNG.uniform(-0.08, 0.08, n)
    tilt_x = RNG.uniform(-0.12, 0.12, n)
    tilt_y = RNG.uniform(-0.12, 0.12, n)
    stone_tone = RNG.uniform(0.80, 1.0, n)
    stone_hue = RNG.uniform(-0.04, 0.04, n)
    face = fbm(size, 8, 5)
    height = 0.75 + lift[owner] + tilt_x[owner] * ox / step + tilt_y[owner] * oy / step
    height = np.where(inside, height * bevel + (face - 0.5) * 0.08, 0.0)
    tone = np.where(inside, stone_tone[owner] * (0.78 + 0.22 * bevel), 0.30)

    grit = fbm(size, 32, 3)
    stain = fbm(size, 4, 3)
    base = np.stack([0.47 + stone_hue[owner], 0.44 + stone_hue[owner] * 0.5,
                     0.41 - stone_hue[owner] * 0.5], axis=-1)
    base = np.where(inside[..., None], base, np.array([0.30, 0.28, 0.27]))
    rgb = base * (tone * (0.9 + 0.2 * grit) * (0.85 + 0.3 * stain))[..., None]
    save_rgba(OUT / "env" / "flagstones.png", rgb, np.ones((size, size)))

    strength = 4.0
    dx = (np.roll(height, -1, axis=1) - np.roll(height, 1, axis=1)) * strength
    dy = (np.roll(height, -1, axis=0) - np.roll(height, 1, axis=0)) * strength
    # Image rows run down the screen, so a height rising downward is a
    # surface facing *up* (+y) in world space: flip dy's sign accordingly.
    nx_ = -dx
    ny_ = dy
    nz_ = np.ones_like(dx)
    length = np.sqrt(nx_ * nx_ + ny_ * ny_ + nz_ * nz_)
    normal = np.stack([nx_ / length, ny_ / length, nz_ / length], axis=-1) * 0.5 + 0.5
    # Normal maps are data, not colour: written straight, never gamma'd.
    save_rgba(OUT / "env" / "flagstones_n.png", normal, np.ones((size, size)))


def pillar_top():
    """A column seen from above: a disc with a rim, shaded as if lit from
    nowhere in particular. Drawn unlit (an occluder shaded its own sprite when this was made)."""
    _, _, r = radius(128)
    n = fbm(128, 6, 3)
    shade = 0.55 + 0.25 * n
    rim = smoothstep(0.78, 0.9, r) * smoothstep(1.0, 0.92, r)
    rgb = np.stack([shade * 0.62, shade * 0.58, shade * 0.55], axis=-1)
    rgb = rgb * (1.0 - 0.45 * rim[..., None])
    a = smoothstep(1.0, 0.96, r)
    save_rgba(OUT / "env" / "pillar_top.png", rgb, a)


def moon_shaft():
    """Moonlight through a four-pane window, as it lands on the floor: a
    parallelogram sheared away from the window, bright near it and fading
    with distance, with the mullions as dark bars. Used as a `lightmap`,
    which adds into the light buffer - black adds nothing, so the alpha
    is irrelevant and the brightness is the whole texture."""
    size = 512
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    u = x / size
    v = y / size                       # 0 at the window (top), 1 far away
    shear = 0.38 * v                   # the shaft leans as it travels
    lx = (u - 0.5 + shear - 0.19) / (0.26 + 0.18 * v)
    inside = smoothstep(0.52, 0.44, np.abs(lx))
    pane_x = np.abs(((lx + 0.5) * 2.0) % 1.0 - 0.5)       # 0 at a mullion
    pane_y = np.abs(((v * 2.2) % 1.0) - 0.5)
    mullion = smoothstep(0.02, 0.06, pane_x) * smoothstep(0.015, 0.05, pane_y)
    along = smoothstep(0.0, 0.08, v) * (1.0 - v) ** 1.3
    edge = smoothstep(0.98, 0.85, v)
    dust = 0.85 + 0.15 * fbm(size, 8, 3)
    light = inside * mullion * along * edge * dust
    rgb = np.stack([light * 0.55, light * 0.7, light * 1.0], axis=-1)
    save_rgba(OUT / "fx" / "moon_shaft.png", rgb, np.ones((size, size)))


# ── 03 Crypt: surfaces with normal maps ─────────────────────────────────
#
# Every one of these is a colour texture plus a height field, and the normal
# map is the height's gradient - the same convention as `flagstones` (+y up,
# green = up; written straight, since a normal map is data, not colour).
# A shape that is round in the world (a column, a barrel) also bends its
# normals across its width, so a light passing it shades it as round.

def normals_from_height(height, strength, wrap=True, base=None):
    """The normal map of a height field, as 0..1 RGB. `wrap` for a texture
    that tiles; `base` is an HxWx3 normal to bend (a cylinder's), before
    the bumps are added to it."""
    if wrap:
        dx = (np.roll(height, -1, axis=1) - np.roll(height, 1, axis=1)) * strength
        dy = (np.roll(height, -1, axis=0) - np.roll(height, 1, axis=0)) * strength
    else:
        gy, gx = np.gradient(height)
        dx, dy = gx * 2.0 * strength, gy * 2.0 * strength
    n = np.stack([-dx, dy, np.ones_like(dx)], axis=-1)
    if base is not None:
        n = n + base - np.array([0.0, 0.0, 1.0])
    n = n / np.linalg.norm(n, axis=-1, keepdims=True)
    return n * 0.5 + 0.5


def cylinder_normal(u):
    """Normals across a round thing: u runs -1..1 over its width. Pulled in
    a little from the silhouette, or the very edge faces sideways and
    catches no light from the front at all."""
    s = np.clip(u * 0.88, -0.999, 0.999)
    return np.stack([s, np.zeros_like(s), np.sqrt(1.0 - s * s)], axis=-1)


def bricks():
    """A tileable wall of stone bricks: running bond, mortar sunk well below
    the faces, each brick its own lift and tilt, edges chipped by noise.
    It is the texture a normal-map demo is judged on, so the relief is
    deep - a torch walking along it should rake every course."""
    size, bh, bw, mortar = 256, 32, 64, 3.0
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    row = (y // bh).astype(int)
    sx = (x + (row % 2) * (bw // 2)) % size
    col = (sx // bw).astype(int)
    lx, ly = sx % bw, y % bh
    d = np.minimum(np.minimum(lx, bw - 1 - lx), np.minimum(ly, bh - 1 - ly)) - mortar * 0.5
    chip = fbm(size, 16, 3)
    d = d - np.clip(chip - 0.58, 0.0, 1.0) * 10.0            # bites out of the edges
    inside = d > 0.0
    bevel = smoothstep(0.0, 6.0, d)

    per_row = size // bw
    idx = row * per_row + col
    n = (size // bh) * per_row
    lift = RNG.uniform(-0.10, 0.10, n)
    tilt_x = RNG.uniform(-0.25, 0.25, n)
    tilt_y = RNG.uniform(-0.25, 0.25, n)
    tone = RNG.uniform(0.72, 1.0, n)
    hue = RNG.uniform(-0.05, 0.05, n)
    face = fbm(size, 12, 5)
    pits = smoothstep(0.70, 0.78, fbm(size, 24, 2))           # small pocks
    height = (0.8 + lift[idx] + tilt_x[idx] * (lx / bw - 0.5) + tilt_y[idx] * (ly / bh - 0.5)) * bevel
    height = height + (face - 0.5) * 0.18 - pits * 0.12
    height = np.where(inside, height, 0.05 + (face - 0.5) * 0.05)

    grime = fbm(size, 4, 4)
    base = np.stack([0.50 + hue[idx], 0.45 + hue[idx] * 0.4, 0.39 - hue[idx] * 0.6], axis=-1)
    shade = tone[idx] * (0.80 + 0.20 * bevel) * (0.85 + 0.3 * face) * (0.8 + 0.35 * grime) * (1.0 - 0.2 * pits)
    rgb = np.where(inside[..., None], base * shade[..., None], np.array([0.20, 0.18, 0.16]) * (0.8 + 0.4 * face)[..., None])
    save_rgba(OUT / "env" / "bricks.png", rgb, np.ones((size, size)))
    save_rgba(OUT / "env" / "bricks_n.png", normals_from_height(height, 5.0), np.ones((size, size)))


def column():
    """A fluted stone column shaft, tiling vertically: round across its
    width (a cylinder normal), eight shallow flutes, and a drum joint every
    128 pixels."""
    w, h = 96, 256
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w * 2.0 - 1.0
    ang = np.arcsin(np.clip(u, -0.999, 0.999))                 # around the shaft
    flutes = 0.5 + 0.5 * np.cos(ang * 8.0 * 2.0)                # ridges between flutes
    joint = smoothstep(0.0, 4.0, np.minimum(y % 128, 127 - y % 128))
    rough = fbm(256, 10, 5)[:, :w]
    height = 0.12 * flutes + 0.25 * joint + (rough - 0.5) * 0.1
    tone = (0.82 + 0.18 * flutes) * (0.7 + 0.3 * joint) * (0.85 + 0.3 * rough)
    rgb = np.stack([0.40 * tone, 0.37 * tone, 0.33 * tone], axis=-1)
    save_rgba(OUT / "env" / "column.png", rgb, np.ones((h, w)))
    save_rgba(OUT / "env" / "column_n.png", normals_from_height(height, 4.0, base=cylinder_normal(u)),
              np.ones((h, w)))


def barrel():
    """A barrel standing up: staves round across it, bulging at the middle,
    two iron hoops near each end with rivets."""
    w, h = 112, 144
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    t = (y + 0.5) / h
    half = 0.84 + 0.16 * np.sin(np.pi * t)                     # the bulge
    u = ((x + 0.5) / w * 2.0 - 1.0) / half
    alpha = smoothstep(1.0, 0.97, np.abs(u)) * smoothstep(0.0, 0.015, t) * smoothstep(1.0, 0.985, t)
    ang = np.arcsin(np.clip(u, -0.999, 0.999))
    stave = (ang / np.pi * 9.0 + 0.5) % 1.0
    groove = smoothstep(0.0, 0.08, np.minimum(stave, 1.0 - stave))
    grain = fbm(256, 8, 5)[(y * 0.4).astype(int) % 256, (x * 2.0).astype(int) % 256]
    hoop_c = [0.13, 0.24, 0.76, 0.87]
    hoop = np.zeros_like(t)
    for c in hoop_c:
        hoop = np.maximum(hoop, smoothstep(0.040, 0.028, np.abs(t - c)))
    rivet = np.zeros_like(t)
    for c in hoop_c:
        for a in np.linspace(-1.2, 1.2, 5):
            rivet = np.maximum(rivet, smoothstep(0.035, 0.015, np.hypot((ang - a) * 0.35, t - c)))
    height = 0.3 * groove + (grain - 0.5) * 0.12 + 0.35 * hoop + 0.2 * rivet
    wood = np.stack([0.42, 0.31, 0.21]) * ((0.7 + 0.3 * groove) * (0.75 + 0.5 * grain))[..., None]
    iron = np.stack([0.24, 0.23, 0.23]) * (0.8 + 0.4 * grain + 0.6 * rivet)[..., None]
    rgb = np.where((hoop > 0.5)[..., None], iron, wood)
    save_rgba(OUT / "env" / "barrel.png", rgb, alpha)
    save_rgba(OUT / "env" / "barrel_n.png", normals_from_height(height, 3.5, wrap=False, base=cylinder_normal(u)),
              alpha)


def crate():
    """A wooden crate: a bevelled frame, horizontal planks, one diagonal
    brace and a nail at each corner."""
    s = 128
    y, x = np.mgrid[0:s, 0:s].astype(np.float64)
    edge = np.minimum(np.minimum(x, s - 1 - x), np.minimum(y, s - 1 - y))
    frame = edge < 14
    frame_h = 0.6 + 0.3 * smoothstep(0.0, 4.0, edge) * smoothstep(14.0, 10.0, edge)
    plank = ((y - 14) / ((s - 28) / 4.0)) % 1.0
    plank_h = 0.3 * smoothstep(0.0, 0.06, np.minimum(plank, 1.0 - plank))
    diag = np.abs((x - 14) - ((s - 1 - y) - 14)) / np.sqrt(2.0)
    brace = (diag < 9) & ~frame
    brace_h = 0.5 + 0.2 * smoothstep(9.0, 5.0, diag)
    nails = np.zeros_like(x)
    for cx in (7, s - 8):
        for cy in (7, s - 8):
            nails = np.maximum(nails, smoothstep(3.5, 1.5, np.hypot(x - cx, y - cy)))
    grain = fbm(s, 8, 5)[(y * 0.5).astype(int) % s, (x * 1.0).astype(int) % s]
    height = np.where(frame, frame_h, np.where(brace, brace_h, plank_h)) + (grain - 0.5) * 0.1 + 0.25 * nails
    tone = np.where(frame, 0.85, np.where(brace, 0.95, 0.75 + 0.25 * smoothstep(0.0, 0.1, np.minimum(plank, 1 - plank))))
    rgb = np.stack([0.48, 0.38, 0.27]) * (tone * (0.75 + 0.5 * grain))[..., None]
    rgb = np.where((nails > 0.3)[..., None], np.array([0.30, 0.29, 0.28]), rgb)
    save_rgba(OUT / "env" / "crate.png", rgb, np.ones((s, s)))
    save_rgba(OUT / "env" / "crate_n.png", normals_from_height(height, 3.0, wrap=False), np.ones((s, s)))


def chain():
    """An iron chain hanging down, tiling vertically: links alternate
    face-on (a ring) and edge-on (a bar), each rounded in its normals."""
    w, h, pitch = 32, 128, 32
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    cx = x - (w - 1) / 2.0
    height = np.zeros((h, w))
    for k in range(h // pitch + 1):
        cy = y - k * pitch
        if k % 2 == 0:                      # face-on ring
            r = np.hypot(cx / 10.0, cy / 18.0)
            height = np.maximum(height, np.clip(1.0 - np.abs(r - 1.0) / 0.28, 0.0, 1.0) ** 0.6)
        else:                               # edge-on link
            height = np.maximum(height, np.clip(1.0 - np.hypot(cx / 3.5, cy / 19.0), 0.0, 1.0) ** 0.6)
    alpha = smoothstep(0.02, 0.15, height)
    rust = fbm(128, 8, 3)[:h, :w]
    rgb = np.stack([0.30 + 0.10 * rust, 0.28 + 0.03 * rust, 0.27 - 0.03 * rust], axis=-1) * (0.6 + 0.6 * height)[..., None]
    save_rgba(OUT / "env" / "chain.png", rgb, alpha)
    save_rgba(OUT / "env" / "chain_n.png", normals_from_height(height, 2.2), alpha)


# ── 04 Stained Glass ────────────────────────────────────────────────────

def pew():
    """A church pew seen from above: three seat planks along its length, a
    raised backrest along the top edge, carved ends. Wood, normal-mapped."""
    w, h = 320, 48
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    end = np.minimum(x, w - 1 - x)
    back = y < 14
    back_h = 0.9 * smoothstep(0.0, 3.0, y) * smoothstep(14.0, 10.0, y)
    seat_y = (y - 14) / ((h - 14) / 3.0)
    plank = seat_y % 1.0
    seat_h = 0.45 * smoothstep(0.0, 0.08, np.minimum(plank, 1.0 - plank)) * smoothstep(h - 1.0, h - 4.0, y)
    cap = end < 12
    cap_h = 0.95 * smoothstep(0.0, 3.0, end) * smoothstep(0.0, 3.0, np.minimum(y, h - 1 - y))
    grain = fbm(256, 8, 5)[(y * 0.7).astype(int) % 256, (x * 0.35).astype(int) % 256]
    height = np.where(cap, cap_h, np.where(back, back_h, seat_h)) + (grain - 0.5) * 0.08
    tone = np.where(cap, 0.75, np.where(back, 0.82, 0.95 - 0.2 * (plank < 0.06)))
    rgb = np.stack([0.56, 0.38, 0.22]) * (tone * (0.75 + 0.5 * grain))[..., None]
    alpha = smoothstep(0.0, 1.5, np.minimum(end, np.minimum(y, h - 1 - y)) + 0.5)
    save_rgba(OUT / "env" / "pew.png", rgb, alpha)
    save_rgba(OUT / "env" / "pew_n.png", normals_from_height(height, 3.0, wrap=False), alpha)


def carpet():
    """A runner down the aisle, tiling vertically: deep red, a gold border
    down each side, a diamond lattice, and a weave that catches low light."""
    w, h = 128, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = np.abs((x + 0.5) / w * 2.0 - 1.0)
    border = (u > 0.78) & (u < 0.9)
    edge = u >= 0.9
    dx = np.abs(((x - w / 2) / 32.0) % 1.0 - 0.5)
    dy = np.abs((y / 32.0) % 1.0 - 0.5)
    lattice = smoothstep(0.06, 0.02, np.abs(dx + dy - 0.5))
    weave = 0.5 + 0.5 * np.sin(x * 1.9) * np.sin(y * 1.9)
    wear = fbm(128, 4, 4)
    red = np.array([0.46, 0.05, 0.06])
    gold = np.array([0.62, 0.45, 0.14])
    rgb = np.where(border[..., None], gold, red)
    rgb = np.where(edge[..., None], red * 0.55, rgb)
    rgb = np.where((lattice > 0.5)[..., None] & (u < 0.78)[..., None], gold * 0.8, rgb)
    rgb = rgb * (0.82 + 0.18 * weave + 0.2 * (wear - 0.5))[..., None]
    height = 0.15 * weave + 0.25 * border + 0.12 * lattice * (u < 0.78)
    save_rgba(OUT / "env" / "carpet.png", rgb, np.ones((h, w)))
    save_rgba(OUT / "env" / "carpet_n.png", normals_from_height(height, 2.0), np.ones((h, w)))


def shaft():
    """A beam of light in dusty air, long on x: bright where it enters,
    fading with distance, soft across. Drawn additive, tinted per pane."""
    w, h = 512, 64
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    t = (x + 0.5) / w
    v = np.abs((y + 0.5) / h * 2.0 - 1.0)
    a = smoothstep(1.0, 0.25, v) * smoothstep(0.0, 0.04, t) * (1.0 - t) ** 1.4
    a = a * (0.8 + 0.2 * fbm(256, 6, 3)[(y * 4).astype(int) % 256, (x * 0.5).astype(int) % 256])
    save_rgba(OUT / "fx" / "shaft.png", 1.0, a)


# ── 05 Nebula ───────────────────────────────────────────────────────────

def warped_fbm(size, cells, octaves, warp):
    """fbm sampled through another fbm - the domain warp that turns blobs
    into the filaments and curls a nebula is made of. Tiles, as fbm does."""
    base = fbm(size, cells, octaves)
    wx = fbm(size, cells, 3)
    wy = fbm(size, cells, 3)
    y, x = np.mgrid[0:size, 0:size]
    sx = (x + (wx - 0.5) * warp * size).astype(int) % size
    sy = (y + (wy - 0.5) * warp * size).astype(int) % size
    return base[sy, sx]


def nebula_clouds():
    """Three cloud shapes, white in alpha so a sprite's colour tints them:
    a wispy veil, a dense knot with a bright heart, and a dust lane that is
    drawn with multiply to carve dark rivers through the glow."""
    size = 512
    _, _, r = radius(size)
    edge = smoothstep(1.0, 0.35, r)

    veil = warped_fbm(size, 4, 6, 0.9)
    a = np.clip((veil - 0.42) * 2.6, 0.0, 1.0) ** 1.4 * edge
    save_rgba(OUT / "fx" / "nebula_veil.png", 1.0, a)

    knot = warped_fbm(size, 3, 6, 0.6)
    heart = np.exp(-(r * r) / 0.12)
    a = np.clip((knot - 0.35) * 2.2 + heart * 0.6, 0.0, 1.0) ** 1.2 * edge
    save_rgba(OUT / "fx" / "nebula_knot.png", 1.0, a)

    lane = warped_fbm(size, 4, 5, 0.5)
    lane = (lane + np.roll(lane, 1, 0) + np.roll(lane, -1, 0) + np.roll(lane, 1, 1) + np.roll(lane, -1, 1)) / 5.0
    a = smoothstep(0.48, 0.72, lane) * edge
    save_rgba(OUT / "fx" / "nebula_dust.png", 1.0, a)


def star_fields():
    """Two tileable star fields - many faint, few bright - for parallax
    layers. White in alpha; the brightest have a tiny halo."""
    size = 512
    for name, count, bright in (("stars_far", 1400, 0.55), ("stars_near", 260, 1.0)):
        a = np.zeros((size, size))
        xs = RNG.integers(0, size, count)
        ys = RNG.integers(0, size, count)
        mag = RNG.random(count) ** 3 * bright
        for x, y, m in zip(xs, ys, mag):
            a[y, x] = max(a[y, x], 0.25 + m)
            if m > 0.45:
                for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                    a[(y + dy) % size, (x + dx) % size] = max(a[(y + dy) % size, (x + dx) % size], m * 0.45)
        save_rgba(OUT / "fx" / f"{name}.png", 1.0, np.clip(a, 0.0, 1.0))


def star_flare():
    """A bright star as a camera sees it: a hot core, a soft halo and four
    diffraction spikes."""
    size = 256
    nx, ny, r = radius(size)
    core = np.exp(-(r * r) / 0.002)
    halo = np.exp(-(r * r) / 0.03) * 0.5
    spikes = (np.exp(-(ny * ny) / 0.00008) + np.exp(-(nx * nx) / 0.00008)) * np.clip(1.0 - r, 0.0, 1.0) ** 2.5
    a = np.clip(core + halo + spikes * 0.9, 0.0, 1.0) * smoothstep(1.0, 0.9, r)
    save_rgba(OUT / "fx" / "star_flare.png", 1.0, a)


def planet():
    """A planet: oceans, continents and ice caps from fbm, and a sphere in
    its normals so a star lights it with a day side and a night side."""
    size = 512
    nx, ny, r = radius(size)
    inside = r < 1.0
    nz = np.sqrt(np.clip(1.0 - r * r, 0.0, 1.0))
    # Sample the noise on the sphere, not the disc: longitude and latitude of
    # the point under each pixel, so land crowds together toward the limb.
    lon = np.arctan2(nx, np.maximum(nz, 1e-4))
    latr = np.arcsin(np.clip(-ny, -1.0, 1.0))
    grid = fbm(size, 5, 6)
    gx = ((lon / np.pi * 0.5 + 0.5) * (size - 1)).astype(int) % size
    gy = ((latr / np.pi + 0.5) * (size - 1)).astype(int) % size
    land = grid[gy, gx]
    lat = np.abs(latr) / (np.pi / 2)
    ocean = np.array([0.08, 0.20, 0.42])
    shore = np.array([0.55, 0.50, 0.34])
    green = np.array([0.22, 0.36, 0.16])
    ice = np.array([0.88, 0.92, 0.96])
    is_land = land > 0.52
    rgb = np.where(is_land[..., None], green * (0.8 + 0.4 * land[..., None]), ocean * (0.8 + 0.4 * land[..., None]))
    rgb = np.where((np.abs(land - 0.52) < 0.012)[..., None], shore, rgb)
    rgb = np.where((lat > 0.72 + (land - 0.5) * 0.5)[..., None], ice, rgb)
    cloud_grid = fbm(size, 8, 5)
    clouds = smoothstep(0.55, 0.7, cloud_grid[gy, (gx + size // 7) % size])
    rgb = rgb * (1.0 - clouds[..., None] * 0.7) + clouds[..., None] * 0.9
    alpha = smoothstep(1.0, 0.985, r)
    save_rgba(OUT / "env" / "planet.png", rgb, alpha)
    # The sphere, bent a little by the land's height.
    n = np.stack([nx, -ny, nz], axis=-1)
    bump = normals_from_height(np.where(is_land, land, 0.52), 1.5, wrap=False) * 2.0 - 1.0
    n = n + np.stack([bump[..., 0], bump[..., 1], np.zeros_like(nz)], axis=-1) * 0.15
    n = n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    save_rgba(OUT / "env" / "planet_n.png", np.where(inside[..., None], n * 0.5 + 0.5, 0.5), alpha)


def moon():
    """A grey, cratered moon: rims raised, floors sunk, in the albedo and in
    the normals, on a sphere."""
    size = 256
    nx, ny, r = radius(size)
    nz = np.sqrt(np.clip(1.0 - r * r, 0.0, 1.0))
    height = (fbm(size, 6, 5) - 0.5) * 0.3
    for _ in range(26):
        cx, cy = RNG.uniform(-0.8, 0.8, 2)
        cr = RNG.uniform(0.05, 0.22)
        d = np.hypot(nx - cx, ny - cy) / cr
        height += np.where(d < 1.0, -0.5 * (1.0 - d * d), 0.0) + 0.35 * np.exp(-((d - 1.0) ** 2) / 0.02)
    tone = 0.55 + 0.35 * np.clip(height, -0.6, 0.6)
    rgb = np.stack([tone * 0.98, tone * 0.96, tone * 0.93], axis=-1)
    alpha = smoothstep(1.0, 0.985, r)
    save_rgba(OUT / "env" / "moon.png", rgb, alpha)
    n = np.stack([nx, -ny, nz], axis=-1)
    bump = normals_from_height(height, 1.2, wrap=False) * 2.0 - 1.0
    n = n + np.stack([bump[..., 0], bump[..., 1], np.zeros_like(nz)], axis=-1) * 0.25
    n = n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    save_rgba(OUT / "env" / "moon_n.png", np.where((r < 1.0)[..., None], n * 0.5 + 0.5, 0.5), alpha)



# ── 07 Spellcraft ───────────────────────────────────────────────────────
#
# Their own generator, so adding them does not shift what RNG gives the
# textures above (every earlier texture stays byte-identical).
SPELL_RNG = np.random.default_rng(1707)


def _supersampled(size, draw, scale=3):
    """Draw with PIL at `scale` times the size and shrink, for smooth edges.
    `draw(d, s)` gets an ImageDraw and the scale factor. Returns alpha 0..1."""
    from PIL import ImageDraw
    big = Image.new("L", (size[0] * scale, size[1] * scale), 0)
    draw(ImageDraw.Draw(big), scale)
    return np.asarray(big.resize(size, Image.LANCZOS), dtype=np.float64) / 255.0


def _glyph_strokes(rng):
    """A rune: three to five strokes between points of a 3 x 3 grid, never
    the same stroke twice - which reads as writing rather than as noise."""
    pts = [(x, y) for y in range(3) for x in range(3)]
    want = int(rng.integers(3, 6))
    strokes = set()
    while len(strokes) < want:
        a, b = rng.choice(len(pts), 2, replace=False)
        if abs(pts[a][0] - pts[b][0]) <= 1 or abs(pts[a][1] - pts[b][1]) <= 1 or rng.random() < 0.2:
            strokes.add((min(a, b), max(a, b)))
    return [(pts[a], pts[b]) for a, b in strokes]


def rune_ring():
    """The outer summoning circle: two thin rings with a band of runes
    between them, and tick marks. White in alpha; the scene colours it."""
    size = 1024
    c = size / 2
    rng = SPELL_RNG

    def draw(d, s):
        cx = c * s

        def circle(r, w):
            d.ellipse([cx - r * s, cx - r * s, cx + r * s, cx + r * s], outline=255, width=int(w * s))
        circle(500, 5)
        circle(470, 3)
        circle(392, 3)
        circle(372, 6)
        n = 36
        for i in range(n):
            ang = 2 * np.pi * i / n
            # The rune stands on the ring: its "down" points at the centre.
            gh, gw = 52.0, 34.0
            for (x0, y0), (x1, y1) in _glyph_strokes(rng):
                pts = []
                for gx, gy in ((x0, y0), (x1, y1)):
                    rr = 404 + (2 - gy) / 2 * gh
                    tt = ang + (gx - 1) / 2 * gw / rr
                    pts.append((cx + np.cos(tt) * rr * s, cx + np.sin(tt) * rr * s))
                d.line(pts, fill=255, width=int(4 * s))
            # A tick between runes on the outer band.
            tt = ang + np.pi / n
            d.line([(cx + np.cos(tt) * 474 * s, cx + np.sin(tt) * 474 * s),
                    (cx + np.cos(tt) * 496 * s, cx + np.sin(tt) * 496 * s)], fill=255, width=int(3 * s))
    a = _supersampled((size, size), draw)
    save_rgba(OUT / "fx" / "rune_ring.png", 1.0, a)


def rune_seal():
    """The inner circle: a seven-pointed star in a ring, small circles on
    its points, and a ring of dots - geometry, where the outer ring is
    writing."""
    size = 1024
    c = size / 2

    def draw(d, s):
        cx = c * s

        def circle(x, y, r, w):
            d.ellipse([x - r * s, y - r * s, x + r * s, y + r * s], outline=255, width=int(w * s))
        circle(cx, cx, 480, 6)
        circle(cx, cx, 300, 3)
        k = 7
        pts = [(cx + np.cos(2 * np.pi * i / k - np.pi / 2) * 470 * s,
                cx + np.sin(2 * np.pi * i / k - np.pi / 2) * 470 * s) for i in range(k)]
        for i in range(k):
            d.line([pts[i], pts[(i + 3) % k]], fill=255, width=int(5 * s))
            circle(pts[i][0], pts[i][1], 26, 4)
        for i in range(56):
            t = 2 * np.pi * i / 56
            x, y = cx + np.cos(t) * 330 * s, cx + np.sin(t) * 330 * s
            d.ellipse([x - 5 * s, y - 5 * s, x + 5 * s, y + 5 * s], fill=255)
        circle(cx, cx, 90, 5)
    a = _supersampled((size, size), draw)
    save_rgba(OUT / "fx" / "rune_seal.png", 1.0, a)


def bolt():
    """One segment of a lightning bolt: flat along x, so it stretches to any
    length, with a hard core and a soft sheath across y."""
    w, h = 16, 32
    _, ny, _ = radius(w, h)
    a = np.clip(np.exp(-(ny * ny) / 0.012) + 0.45 * smoothstep(1.0, 0.0, np.abs(ny)) ** 2, 0.0, 1.0)
    save_rgba(OUT / "fx" / "bolt.png", 1.0, a)


def shard():
    """An ice shard: a long, thin diamond, bright along its spine."""
    w, h = 96, 24
    nx, ny, _ = radius(w, h)
    width = np.clip(1.0 - np.abs(nx), 0.0, 1.0) ** 0.8
    body = smoothstep(1.0, 0.7, np.abs(ny) / np.maximum(width, 1e-3))
    spine = np.exp(-(ny * ny) / 0.02) * width
    save_rgba(OUT / "fx" / "shard.png", 1.0, np.clip(body * 0.7 + spine * 0.6, 0.0, 1.0))


# Where the staff's crystal sits in mage.png (pixels from the top-left of a
# 256 x 384 image); the scene puts its glow there.
MAGE_SIZE = (256, 384)
MAGE_STAFF_TIP = (214, 34)


def mage():
    """A robed, hooded figure with a staff, facing right, in a normal-mapped
    sprite so the spells' own lights shape it: domed cloth from a blurred
    silhouette, folds from wavy stripes, a dark hollow under the hood."""
    from PIL import ImageFilter
    w, h = MAGE_SIZE

    def robe(d, s):
        # Robe: shoulders to a flared hem, a little forward-leaning.
        d.polygon([(92 * s, 148 * s), (150 * s, 142 * s), (178 * s, 200 * s), (196 * s, 372 * s),
                   (54 * s, 376 * s), (72 * s, 250 * s)], fill=255)
        # Hood: a rounded cowl with a point swept back.
        d.polygon([(96 * s, 158 * s), (84 * s, 110 * s), (70 * s, 64 * s), (108 * s, 76 * s),
                   (140 * s, 82 * s), (162 * s, 112 * s), (160 * s, 150 * s)], fill=255)
        d.ellipse([90 * s, 72 * s, 166 * s, 156 * s], fill=255)
        # The arm reaching to the staff.
        d.polygon([(140 * s, 170 * s), (168 * s, 160 * s), (214 * s, 196 * s), (206 * s, 214 * s),
                   (160 * s, 204 * s)], fill=255)

    def staff(d, s):
        d.line([(214 * s, 60 * s), (206 * s, 380 * s)], fill=255, width=int(8 * s))
        # The claw that holds the crystal.
        d.polygon([(200 * s, 64 * s), (228 * s, 64 * s), (232 * s, 40 * s), (222 * s, 56 * s),
                   (214 * s, 44 * s), (206 * s, 56 * s), (196 * s, 40 * s)], fill=255)

    def hood_hollow(d, s):
        d.ellipse([130 * s, 96 * s, 164 * s, 146 * s], fill=255)

    body = _supersampled((w, h), robe)
    rod = _supersampled((w, h), staff)
    hollow = _supersampled((w, h), hood_hollow) * body
    alpha = np.clip(np.maximum(body, rod), 0.0, 1.0)

    def blur(a, r):
        img = Image.fromarray((np.clip(a, 0, 1) * 255).astype(np.uint8))
        return np.asarray(img.filter(ImageFilter.GaussianBlur(r)), dtype=np.float64) / 255.0

    # Height: the silhouette blurred into a dome, plus vertical cloth folds.
    dome = blur(body, 14)
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    folds = np.sin(x * 0.19 + np.sin(y * 0.02) * 2.0 + SPELL_RNG.random() * 6.0) * smoothstep(170, 360, y)
    height = dome + folds * 0.05 * body - hollow * 0.4 + blur(rod, 2) * 0.6
    normal = normals_from_height(height, 6.0, wrap=False)

    robe_col = np.array([0.20, 0.16, 0.36])
    trim = smoothstep(356, 370, y) * body                 # a pale hem
    rgb = robe_col * (0.75 + 0.25 * (folds[..., None] * 0.5 + 0.5))
    rgb = rgb * (1.0 - hollow[..., None] * 0.85)
    rgb = rgb * (1.0 - trim[..., None]) + np.array([0.70, 0.58, 0.30]) * trim[..., None]
    wood = np.array([0.30, 0.20, 0.12])
    only_rod = np.clip(rod - body, 0.0, 1.0)[..., None]
    rgb = rgb * (1.0 - only_rod) + wood * only_rod
    save_rgba(OUT / "env" / "mage.png", rgb, alpha)
    save_rgba(OUT / "env" / "mage_n.png", normal, alpha)


# ── 08 Underwater ───────────────────────────────────────────────────────
#
# Their own generator too, so nothing above shifts.
SEA_RNG = np.random.default_rng(808)


def _sea_noise(size, cells, octaves, rng):
    """Tileable value-noise fbm on `rng`, so these textures leave RNG alone."""
    total = np.zeros((size, size))
    amp, norm, c = 1.0, 0.0, cells
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    for _ in range(octaves):
        grid = rng.random((c, c))
        fx, fy = x / size * c, y / size * c
        x0, y0 = np.floor(fx).astype(int) % c, np.floor(fy).astype(int) % c
        x1, y1 = (x0 + 1) % c, (y0 + 1) % c
        tx, ty = smoothstep(0.0, 1.0, fx - np.floor(fx)), smoothstep(0.0, 1.0, fy - np.floor(fy))
        a = grid[y0, x0] * (1 - tx) + grid[y0, x1] * tx
        b = grid[y1, x0] * (1 - tx) + grid[y1, x1] * tx
        total += (a * (1 - ty) + b * ty) * amp
        norm += amp
        amp *= 0.5
        c *= 2
    return total / norm


def sand():
    """Seabed sand: wind-less ripples bent by noise, speckled grains. Tiles."""
    size = 512
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    warp = _sea_noise(size, 4, 3, SEA_RNG)
    # Bands across the floor, not along it: on a floor seen at an angle the
    # ripples run side to side.
    ripples = np.sin((y / size * 18 + warp * 3.0) * 2 * np.pi) * 0.5 + 0.5
    grains = SEA_RNG.random((size, size))
    height = ripples ** 1.5 * 0.8 + _sea_noise(size, 16, 3, SEA_RNG) * 0.3 + grains * 0.05
    base = np.array([0.74, 0.66, 0.50])
    shade = 0.78 + 0.22 * height[..., None] + (grains[..., None] - 0.5) * 0.08
    save_rgba(OUT / "env" / "sand.png", base * shade, np.ones((size, size)))
    save_rgba(OUT / "env" / "sand_n.png", normals_from_height(height, 2.5), np.ones((size, size)))


def rock():
    """A barnacled boulder: a lumpy disc with a domed height, lichen spots."""
    size = 256
    nx, ny, r = radius(size)
    lump = _sea_noise(size, 5, 4, SEA_RNG)
    edge = r + (lump - 0.5) * 0.35
    alpha = smoothstep(0.95, 0.9, edge) * (ny > -0.2 + (lump - 0.5) * 0.3)   # flat-ish top cut
    alpha = smoothstep(0.95, 0.88, edge)
    dome = np.sqrt(np.clip(1.0 - np.clip(edge, 0, 1) ** 2, 0.0, 1.0))
    height = dome + (_sea_noise(size, 20, 3, SEA_RNG) - 0.5) * 0.25
    spots = smoothstep(0.62, 0.7, _sea_noise(size, 12, 2, SEA_RNG))
    rgb = np.array([0.33, 0.36, 0.38]) * (0.7 + 0.3 * height[..., None])
    rgb = rgb * (1 - spots[..., None]) + np.array([0.35, 0.45, 0.30]) * spots[..., None]
    save_rgba(OUT / "env" / "rock.png", rgb, alpha)
    save_rgba(OUT / "env" / "rock_n.png", normals_from_height(height * 1.2, 3.0, wrap=False), alpha)


def kelp():
    """One segment of kelp, 32 x 96, anchored at its base: a stem with a
    ruffled blade either side. Stacked in a chain, each turned a little from
    the one below, they bend like a strand."""
    w, h = 32, 96
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w * 2 - 1
    t = (y + 0.5) / h
    # Near-constant width with a ruffle, so stacked segments join into one
    # blade instead of pinching at every joint.
    blade_w = 0.72 + 0.1 * np.sin(t * 2 * np.pi * 3) + 0.05 * np.sin(t * 40)
    blade = smoothstep(blade_w, blade_w - 0.15, np.abs(u))
    stem = smoothstep(0.18, 0.08, np.abs(u))
    a = np.clip(np.maximum(blade * 0.85, stem), 0, 1)
    rgb = np.array([0.30, 0.52, 0.18]) * (0.6 + 0.4 * (1 - np.abs(u)))[..., None]
    rgb = rgb + stem[..., None] * np.array([0.1, 0.12, 0.0])
    save_rgba(OUT / "env" / "kelp.png", rgb, a)


def fish():
    """A small fish, side on, facing right: silver belly, dark back, a fork
    tail. 64 x 32."""
    w, h = 64, 32
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w
    v = (y + 0.5) / h * 2 - 1
    body_h = np.clip(np.sin(np.clip((u - 0.18) / 0.8, 0, 1) * np.pi) ** 0.7 * 0.8, 0, 1)
    body = smoothstep(body_h, body_h - 0.1, np.abs(v)) * (u > 0.16)
    tail = (u < 0.24) * smoothstep(0.02, 0.0, np.abs(np.abs(v) - (0.24 - u) * 3.0) - 0.12)
    a = np.clip(body + tail, 0, 1)
    back = smoothstep(0.1, -0.5, v)
    rgb = np.array([0.82, 0.86, 0.88]) * (1 - back[..., None]) + np.array([0.20, 0.30, 0.38]) * back[..., None]
    eye = ((u - 0.84) ** 2 * 16 + (v + 0.1) ** 2) < 0.012
    rgb = np.where(eye[..., None], 0.05, rgb)
    save_rgba(OUT / "env" / "fish.png", rgb, a)


def jelly():
    """A jellyfish bell and trailing tendrils, white in alpha: it glows in
    whatever colour the scene gives it."""
    w, h = 128, 192
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w * 2 - 1
    t = (y + 0.5) / h
    bell_r = np.sqrt(u ** 2 + ((t - 0.3) / 0.28) ** 2)
    bell = smoothstep(1.0, 0.85, bell_r) * (t < 0.34)
    rim = np.exp(-((bell_r - 0.9) ** 2) / 0.004) * (t < 0.34)
    tend = np.zeros_like(u)
    for k, off in enumerate((-0.55, -0.25, 0.0, 0.25, 0.55)):
        cx = off + 0.12 * np.sin(t * 12 + k)
        tend += np.exp(-((u - cx) ** 2) / 0.0015) * smoothstep(0.3, 0.36, t) * smoothstep(1.0, 0.5, t)
    a = np.clip(bell * 0.55 + rim * 0.9 + tend * 0.7, 0, 1)
    save_rgba(OUT / "fx" / "jelly.png", 1.0, a)


def bubble():
    """A bubble: a thin bright rim, a highlight up and left, clear inside."""
    size = 64
    nx, ny, r = radius(size)
    rim = np.exp(-((r - 0.82) ** 2) / 0.006)
    hl = np.exp(-(((nx + 0.3) ** 2 + (ny + 0.35) ** 2)) / 0.02)
    a = np.clip(rim * 0.8 + hl + 0.08 * (r < 0.82), 0, 1) * smoothstep(1.0, 0.92, r)
    save_rgba(OUT / "fx" / "bubble.png", 1.0, a)


def vfade():
    """White, opaque at the top and clear at the bottom: laid over the far
    edge of a floor, in the water's colour, it melts the floor into it."""
    w, h = 8, 256
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    save_rgba(OUT / "fx" / "vfade.png", 1.0, smoothstep(1.0, 0.0, t) ** 1.6)


def water_bg():
    """The water column: pale teal under the surface to near-black depths."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.25, 0.62, 0.70])
    mid = np.array([0.05, 0.22, 0.36])
    deep = np.array([0.01, 0.05, 0.10])
    a = smoothstep(0.0, 0.45, t)[..., None]
    b = smoothstep(0.45, 1.0, t)[..., None]
    rgb = top * (1 - a) + mid * a
    rgb = rgb * (1 - b) + deep * b
    save_rgba(OUT / "env" / "water_bg.png", rgb, np.ones((h, w)))


# ── 09 Ten Thousand Lights ──────────────────────────────────────────────
LANTERN_RNG = np.random.default_rng(909)


def night_sky():
    """Deep navy overhead to a warm violet glow at the horizon."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.01, 0.02, 0.07])
    mid = np.array([0.06, 0.05, 0.18])
    low = np.array([0.35, 0.16, 0.24])
    a = smoothstep(0.0, 0.6, t)[..., None]
    b = smoothstep(0.6, 1.0, t)[..., None]
    rgb = top * (1 - a) + mid * a
    rgb = rgb * (1 - b) + low * b
    save_rgba(OUT / "env" / "night_sky.png", rgb, np.ones((h, w)))


def hills():
    """A ridge line of wooded hills, tileable sideways, with a normal map so
    the lanterns' light catches the slopes."""
    w, h = 1024, 256
    x = np.arange(w) / w
    ridge = np.zeros(w)
    for k, amp in ((2, 0.22), (5, 0.1), (13, 0.05), (31, 0.02)):
        ph = LANTERN_RNG.random() * 2 * np.pi
        ridge += amp * np.sin(x * 2 * np.pi * k + ph)
    ridge = 0.55 + ridge                      # fraction of the height that is hill
    y = (np.arange(h) / h)[:, None]
    depth = ridge[None, :] - (1.0 - y)          # > 0 inside the hill (the bottom `ridge` of it)
    alpha = smoothstep(-0.004, 0.004, depth)
    trees = _sea_noise(256, 32, 3, LANTERN_RNG)
    tex = trees[(np.arange(h)[:, None] % 256), (np.arange(w)[None, :] // 4 % 256)]
    height = np.clip(depth * 4.0, 0, 1) ** 0.5 + tex * 0.25
    rgb = np.array([0.10, 0.10, 0.14]) * (0.7 + 0.3 * tex[..., None])
    save_rgba(OUT / "env" / "hills.png", rgb, alpha)
    save_rgba(OUT / "env" / "hills_n.png", normals_from_height(height, 3.0, wrap=False), alpha)


def lantern():
    """A paper sky lantern, white in alpha: a rounded body brightest low down
    where the flame is, a soft halo around it."""
    w, h = 48, 64
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w * 2 - 1
    v = (y + 0.5) / h * 2 - 1
    body = smoothstep(1.0, 0.85, np.maximum(np.abs(u) / (0.48 + 0.1 * (v + 1) / 2), np.abs(v) / 0.62))
    flame = np.exp(-((u ** 2) / 0.02 + ((v - 0.35) ** 2) / 0.04))
    halo = np.clip(1.0 - np.sqrt(u ** 2 + v ** 2), 0, 1) ** 2
    a = np.clip(body * (0.55 + 0.45 * (v + 1) / 2) + flame * 0.6 + halo * 0.25, 0, 1)
    save_rgba(OUT / "fx" / "lantern.png", 1.0, a)


# ── 10 Clay ─────────────────────────────────────────────────────────────
#
# Primitives seen from above, as normal maps: the shape is all in the
# normals, the colour is plain white, so the light and the ambient
# occlusion do all the work - a clay render.


def _write_normal(name, n, alpha):
    n = n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    save_rgba(OUT / "env" / f"{name}_n.png", n * 0.5 + 0.5, alpha)


def clay_box():
    """The top of a box with rounded, bevelled edges: flat in the middle,
    turning outward over the last few percent of each side."""
    size = 256
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    u = (x + 0.5) / size * 2 - 1
    v = (y + 0.5) / size * 2 - 1
    bevel = 0.1
    # Each side's slope: 0 in the flat middle, rising toward 0.9 at the edge.
    sx = np.clip((np.abs(u) - (1 - bevel)) / bevel, 0, 1) * np.sign(u)
    sy = np.clip((np.abs(v) - (1 - bevel)) / bevel, 0, 1) * np.sign(v)
    n = np.stack([sx * 0.9, -sy * 0.9, np.ones_like(u)], axis=-1)
    _write_normal("clay_box", n, np.ones((size, size)))


def clay_sphere():
    """A sphere seen from above: a white disc, and a hemisphere in its
    normals."""
    size = 256
    nx, ny, r = radius(size)
    alpha = smoothstep(1.0, 0.985, r)
    nz = np.sqrt(np.clip(1.0 - r * r, 0.0, 1.0))
    save_rgba(OUT / "env" / "clay_disc.png", 1.0, alpha)
    _write_normal("clay_sphere", np.stack([nx, -ny, np.maximum(nz, 0.05)], axis=-1), alpha)


def clay_cylinder():
    """A cylinder seen from above: a flat round top with a rounded rim."""
    size = 256
    nx, ny, r = radius(size)
    alpha = smoothstep(1.0, 0.985, r)
    rim = np.clip((r - 0.86) / 0.14, 0, 1)
    k = rim * 0.95 / np.maximum(r, 1e-4)
    _write_normal("clay_cylinder", np.stack([nx * k, -ny * k, np.ones_like(r)], axis=-1), alpha)


# ── 11 Trails ─────────────────────────────────────────────────────────────
#
# Textures a `trail` stretches along its ribbon: u runs head to tail, v
# across. Deterministic - no random numbers, so no RNG of their own.


def ribbon_soft():
    """A soft ribbon: a hot core fading to nothing at both edges."""
    w, h = 16, 64
    v = (np.mgrid[0:h, 0:w][0].astype(np.float64) + 0.5) / h * 2 - 1
    a = np.exp(-(v / 0.42) ** 2) * 0.75 + np.exp(-(v / 0.12) ** 2) * 0.25
    save_rgba(OUT / "fx" / "ribbon_soft.png", 1.0, np.clip(a, 0, 1))


def ribbon_slash():
    """A blade's slash: a bright, fairly hard-edged band with fine streaks
    running along it, the way a swing smears a lit edge."""
    w, h = 16, 96
    v = (np.mgrid[0:h, 0:w][0].astype(np.float64) + 0.5) / h * 2 - 1
    band = smoothstep(1.0, 0.7, np.abs(v))
    streaks = 0.7 + 0.3 * np.cos(v * 23.0) * np.cos(v * 9.0)
    core = np.exp(-(v / 0.18) ** 2)
    save_rgba(OUT / "fx" / "ribbon_slash.png", 1.0, np.clip(band * streaks * 0.8 + core * 0.4, 0, 1))



# ── 12 Pixel Perfect ──────────────────────────────────────────────────────
#
# Real pixel art: every texture is a grid of palette indices, drawn at 1
# texel per art pixel (the scene shows each at 4x4 world units), with hard
# edges, 1-pixel outlines and ordered dithering - no smooth gradient
# anywhere. They live in `sprites/pixel/`, which the project samples
# nearest-neighbour while every other texture is filtered
# (`texture_filter_overrides` in project.jengine).
#
# The water is **palette-cycled**: its red channel is an index 0..7 into
# the eight colours in its own bottom row, and the `palette_cycle` graph
# (tools/gen_shaders.py) turns the index with time - the waterfall flows
# without a single frame of animation, as it did on 16-bit consoles.

PIXEL_RNG = np.random.default_rng(1212)


def _hex(c):
    c = c.lstrip("#")
    return [int(c[i:i + 2], 16) / 255.0 for i in (0, 2, 4)]


PX_SKY = [_hex(c) for c in ("#1d1a3b", "#2c2657", "#48306f", "#723f82", "#a3528a",
                            "#d46f7f", "#f19a72", "#ffc98a")]
PX_CLOUD = [_hex(c) for c in ("#7a4f86", "#c1708c", "#f2a48f", "#ffd9ae")]
PX_MOUNT = [_hex(c) for c in ("#3b2f63", "#4f3f78", "#6a5690", "#cbb9e6")]
PX_ROCK = [_hex(c) for c in ("#140f1f", "#2b2135", "#453446", "#654b55", "#8b675f", "#b38b6d")]
PX_GRASS = [_hex(c) for c in ("#1d3a2c", "#2f5d3a", "#4b8a44", "#80c05a")]
# Mostly mid blues with one bright highlight: cycled, each column reads as
# dashes of light running down it.
PX_WATER = [_hex(c) for c in ("#1f4a8a", "#2a66b3", "#2a66b3", "#3a86d6",
                              "#5aa6ea", "#d8f2ff", "#8ccaf6", "#3a86d6")]
PX_WOOD = [_hex(c) for c in ("#140f1f", "#4a2c2a", "#6f4432", "#9a6440")]
PX_ROOF = [_hex(c) for c in ("#140f1f", "#6e2338", "#a33a44", "#d1604e")]
PX_STONE = [_hex(c) for c in ("#140f1f", "#4d4a66", "#716f8c", "#a3a1ba")]
PX_WINDOW = _hex("#ffd36b")
PX_SAIL = [_hex(c) for c in ("#140f1f", "#c9b79c", "#efe3c8")]

BAYER4 = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) / 16.0


def _px_save(name, rgb, alpha):
    """Pixel art: exact colours, alpha only 0 or 1."""
    save_rgba(OUT / "pixel" / name, rgb, (alpha > 0.5).astype(np.float64))


def _outline(mask):
    """The 1-pixel ring just inside `mask`'s edge (4-neighbour)."""
    m = mask.astype(bool)
    inner = m.copy()
    inner[1:, :] &= m[:-1, :]
    inner[:-1, :] &= m[1:, :]
    inner[:, 1:] &= m[:, :-1]
    inner[:, :-1] &= m[:, 1:]
    return m & ~inner


def _paint(idx, palette):
    """Index grid -> rgb; -1 stays black (and transparent by alpha)."""
    pal = np.array(palette + [[0.0, 0.0, 0.0]])
    return pal[np.where(idx < 0, len(palette), idx)]


def pixel_sky():
    """Dawn: eight bands, ordered-dithered into each other."""
    w, h = 352, 180
    y = (np.arange(h)[:, None] + 0.5) / h * np.ones((1, w))
    t = np.clip((y - 0.02) / 0.9, 0, 1) * (len(PX_SKY) - 1)
    x_i, y_i = np.meshgrid(np.arange(w), np.arange(h))
    idx = np.floor(t + BAYER4[y_i % 4, x_i % 4] - 0.5 + 0.5).astype(int)
    idx = np.clip(idx, 0, len(PX_SKY) - 1)
    # A few stars in the dark top.
    stars = (PIXEL_RNG.random((h, w)) < 0.004) & (y < 0.3)
    rgb = _paint(idx, PX_SKY)
    rgb[stars] = _hex("#e8e0ff")
    _px_save("pp_sky.png", rgb, np.ones((h, w)))


def pixel_cloud(name, w, h, seed):
    """A flat-bottomed cumulus of overlapping circles, lit from the upper
    left, its shade bands dithered."""
    r = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    inside = np.zeros((h, w), bool)
    light = np.zeros((h, w))
    for _ in range(7):
        cx, cy = r.uniform(w * 0.15, w * 0.85), r.uniform(h * 0.35, h * 0.75)
        rad = r.uniform(h * 0.25, h * 0.5)
        d = np.hypot(x - cx, (y - cy) * 1.3)
        inside |= d < rad
        light = np.maximum(light, np.where(d < rad, ((cx - x) + (cy - y)) / rad * 0.5 + 0.5, 0))
    inside &= y < h * 0.82
    level = np.clip(light * 3.2 + (BAYER4[y.astype(int) % 4, x.astype(int) % 4] - 0.5) * 0.9, 0, 3)
    idx = np.where(inside, np.floor(level).astype(int), -1)
    idx[_outline(inside) & (y > h * 0.5)] = 0
    _px_save(name, _paint(idx, PX_CLOUD), inside.astype(float))


def pixel_mountains():
    """Far range, tileable sideways: two ridges, snow on the high one."""
    w, h = 352, 110
    x = np.arange(w)
    def ridge(amps, base):
        r = np.full(w, base, float)
        for k, a in amps:
            r += a * np.sin(x / w * 2 * np.pi * k + PIXEL_RNG.random() * 6.28)
        return np.round(r).astype(int)
    back = ridge([(3, 12), (7, 5), (17, 2)], 26)
    front = ridge([(2, 8), (5, 6), (13, 2)], 44)
    y = np.arange(h)[:, None]
    idx = np.full((h, w), -1)
    idx[y >= back[None, :]] = 2
    idx[(y >= back[None, :]) & (y < back[None, :] + 1)] = 3
    idx[y >= front[None, :]] = 1
    idx[(y >= front[None, :]) & (y < front[None, :] + 1)] = 2
    idx[y >= front[None, :] + 14] = 0
    _px_save("pp_mountains.png", _paint(idx, PX_MOUNT), (idx >= 0).astype(float))


def pixel_cliff(name, w, h, facing_right, seed):
    """A cliff face in strata, lit from the upper left, grass on top and a
    dark outline. `facing_right`: the open side (the waterfall's) is on the
    right, so the edge profile runs down that side."""
    r = np.random.default_rng(seed)
    y, x = np.mgrid[0:h, 0:w]
    # The open edge: where the rock ends, per row, wandering in steps.
    edge = np.empty(h)
    e = w * 0.78
    for row in range(h):
        if row % 6 == 0:
            e = np.clip(e + r.integers(-3, 4), w * 0.6, w - 2)
        edge[row] = e
    top = 10 + np.round(3 * np.sin(np.arange(w) / 9.0 + seed)).astype(int)
    xx = x if facing_right else (w - 1 - x)
    solid = (xx < edge[:, None]) & (y >= top[None, :] if facing_right else y >= top[None, ::-1])
    # Strata: horizontal bands with a stepped wobble, shaded by depth into
    # the rock and by height.
    band = ((y + np.round(2 * np.sin(xx / 7.0 + y / 23.0))) // 7) % 3
    shade = np.clip(3 + (band == 0) - (band == 2) - (xx > edge[:, None] - 6) * 1
                    - (y > h * 0.75) * 1, 1, 5).astype(int)
    cracks = (r.random((h, w)) < 0.02)
    shade = np.where(cracks, 1, shade)
    idx_rock = np.where(solid, shade, -1)
    rgb = _paint(idx_rock, PX_ROCK)
    # Grass cap: 3 pixels, a lighter lip, tufts hanging over the edge.
    ttop = top if facing_right else top[::-1]
    grass = solid & (y < ttop[None, :] + 3)
    rgb[grass] = np.array(PX_GRASS[2])
    rgb[solid & (y == ttop[None, :])] = np.array(PX_GRASS[3])
    tufts = solid & (y >= ttop[None, :] + 3) & (y < ttop[None, :] + 6) & (r.random((h, w)) < 0.35)
    rgb[tufts] = np.array(PX_GRASS[1])
    out = _outline(solid)
    rgb[out] = np.array(PX_ROCK[0])
    _px_save(name, rgb, solid.astype(float))


def _palette_row(rgb, alpha, palette):
    """Append the eight cycling colours as the texture's last row."""
    h, w = alpha.shape
    row = np.zeros((1, w, 3))
    row[0, :len(palette)] = np.array(palette)
    return np.concatenate([rgb, row]), np.concatenate([alpha, np.ones((1, w))])


def pixel_waterfall():
    """Index art (red = index / 8) in vertical streaks that run down as the
    palette turns, with the palette as the last row."""
    w, h = 36, 112
    y, x = np.mgrid[0:h, 0:w]
    # Each column (sometimes a pair) on its own phase, so the highlights fall
    # as separate dashes rather than lining up into bands.
    phase = PIXEL_RNG.integers(0, 8, w)
    pair = PIXEL_RNG.random(w) < 0.4
    phase[1:][pair[1:]] = phase[:-1][pair[1:]]
    idx = ((y // 3 + phase[None, :]) % 8)
    # The lip at the top and the edges are darker: indices 0..2 only.
    edge = (x < 2) | (x >= w - 2)
    idx = np.where(edge, idx % 3, idx)
    rgb = np.zeros((h, w, 3))
    rgb[..., 0] = (idx + 0.5) / 8.0
    rgb, a = _palette_row(rgb, np.ones((h, w)), PX_WATER)
    save_rgba(OUT / "pixel" / "pp_waterfall.png", rgb, a)


def pixel_pool():
    """The pool: an ellipse of ripples, cycled with the same palette."""
    w, h = 150, 26
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    inside = ((x - w / 2) / (w / 2)) ** 2 + ((y - h / 2) / (h / 2)) ** 2 < 1.0
    ripple = np.floor(x / 4 + 2.2 * np.sin(y * 0.9 + x * 0.07) + np.abs(x - w / 2) * 0.12)
    idx = (ripple % 8).astype(int)
    idx = np.where(np.abs(x - w / 2) < 16, np.minimum(idx + 3, 7), idx)   # foam under the fall
    rgb = np.zeros((h, w, 3))
    rgb[..., 0] = (idx + 0.5) / 8.0
    rgb, a = _palette_row(rgb, inside.astype(float), PX_WATER)
    save_rgba(OUT / "pixel" / "pp_pool.png", rgb, (a > 0.5).astype(float))


def pixel_hut():
    """A small wooden hut, red roof, one warm window."""
    w, h = 26, 24
    idx = np.full((h, w), -1)
    rgb = np.zeros((h, w, 3))
    # Walls.
    walls = np.zeros((h, w), bool)
    walls[11:24, 3:23] = True
    rgb[walls] = np.array(PX_WOOD[2])
    for yy in range(12, 24, 3):
        rgb[yy, 3:23] = np.array(PX_WOOD[1])
    # Roof: a triangle.
    roof = np.zeros((h, w), bool)
    for yy in range(0, 12):
        half = yy + 2
        roof[yy, max(0, 13 - half):min(w, 13 + half)] = True
    rgb[roof] = np.array(PX_ROOF[2])
    for yy in range(0, 12):
        half = yy + 2
        rgb[yy, max(0, 13 - half):13] = np.array(PX_ROOF[3])
    # Door and window.
    rgb[16:24, 6:11] = np.array(PX_WOOD[1])
    rgb[14:19, 14:20] = np.array(PX_WINDOW)
    rgb[16, 14:20] = np.array(PX_WOOD[3])
    rgb[14:19, 16] = np.array(PX_WOOD[3])
    solid = walls | roof
    rgb[_outline(solid)] = np.array(PX_WOOD[0])
    _px_save("pp_hut.png", rgb, solid.astype(float))


def pixel_windmill():
    """A stone tower (its cap is the hub) and, separately, the four sails -
    a sprite the scene turns, so rotation can be seen staying on the grid."""
    w, h = 18, 34
    y, x = np.mgrid[0:h, 0:w]
    half = 5 + (y * 3) // h
    tower = (np.abs(x - w // 2) <= half) & (y >= 4)
    cap = (np.abs(x - w // 2) <= 6) & (y < 6)
    solid = tower | cap
    shade = np.where(x < w // 2, 3, 2)
    rgb = _paint(np.where(solid, shade, -1), PX_STONE)
    rgb[cap] = np.array(PX_ROOF[1])
    rgb[(y % 6 == 0) & tower] = np.array(PX_STONE[1])
    rgb[20:26, 7:11] = np.array(PX_WOOD[1])
    rgb[_outline(solid)] = np.array(PX_STONE[0])
    _px_save("pp_windmill.png", rgb, solid.astype(float))

    n = 41
    y, x = np.mgrid[0:n, 0:n] - n // 2
    arm = ((np.abs(x) <= 1) & (np.abs(y) <= 19)) | ((np.abs(y) <= 1) & (np.abs(x) <= 19))
    # A sail beside each arm, on its trailing side.
    sail = (((x >= 2) & (x <= 6) & (y >= 5) & (y <= 18))
            | ((y <= -2) & (y >= -6) & (x >= 5) & (x <= 18))
            | ((x <= -2) & (x >= -6) & (y <= -5) & (y >= -18))
            | ((y >= 2) & (y <= 6) & (x <= -5) & (x >= -18)))
    solid = arm | sail
    rgb = np.zeros((n, n, 3))
    rgb[sail] = np.array(PX_SAIL[2])
    rgb[sail & ((x + y) % 3 == 0)] = np.array(PX_SAIL[1])
    rgb[arm] = np.array(PX_WOOD[2])
    rgb[_outline(solid)] = np.array(PX_SAIL[0])
    _px_save("pp_sails.png", rgb, solid.astype(float))


def pixel_pine():
    """A pine, three tiers, outlined."""
    w, h = 15, 26
    y, x = np.mgrid[0:h, 0:w]
    tiers = np.zeros((h, w), bool)
    for top, bottom in ((0, 10), (6, 17), (12, 23)):
        rows = (y >= top) & (y < bottom)
        half = (y - top) * 0.75 + 1
        tiers |= rows & (np.abs(x - w // 2) <= half)
    trunk = (np.abs(x - w // 2) <= 1) & (y >= 22)
    solid = tiers | trunk
    shade = np.where(x < w // 2 + ((y % 5) - 2) * 0.3, 2, 1)
    rgb = _paint(np.where(tiers, shade, -1), PX_GRASS)
    rgb[trunk] = np.array(PX_WOOD[1])
    rgb[_outline(solid)] = np.array(PX_GRASS[0])
    _px_save("pp_pine.png", rgb, solid.astype(float))


def pixel_dot():
    """One white pixel: mist and spray, drawn as whole art pixels."""
    _px_save("pp_dot.png", np.ones((1, 1, 3)), np.ones((1, 1)))



# ── 13 Day & Night ────────────────────────────────────────────────────────
#
# A small town from above: grass, roofs, tree crowns and a pond, all
# normal-mapped so a sun sweeping over them lights one slope of a roof and
# then the other. Own RNG, so every earlier texture stays byte-identical.

TOWN_RNG = np.random.default_rng(1313)


def town_grass():
    """Tileable meadow grass: blades of noise at two scales, darker clumps."""
    size = 256
    n1 = _sea_noise(size, 8, 4, TOWN_RNG)
    n2 = _sea_noise(size, 48, 2, TOWN_RNG)
    shade = 0.7 + 0.35 * n1 + 0.25 * (n2 - 0.5)
    rgb = np.stack([0.20 * shade, 0.42 * shade, 0.14 * shade], axis=-1)
    save_rgba(OUT / "env" / "town_grass.png", rgb, np.ones((size, size)))
    save_rgba(OUT / "env" / "town_grass_n.png", normals_from_height(n1 * 0.6 + n2 * 0.4, 1.2), np.ones((size, size)))


def town_roof():
    """A gable roof from above: two slopes meeting at a ridge along x, in
    rows of tiles, eaves darker. White-ish in colour (the scene tints each
    roof) with its slopes in the normal map."""
    w, h = 192, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    v = (y + 0.5) / h * 2 - 1                     # -1 top eave .. 1 bottom eave
    u = (x + 0.5) / w * 2 - 1
    rows = (np.abs(v) * 7.0) % 1.0
    course = np.floor(np.abs(v) * 7.0)
    stagger = ((x + (course % 2) * 8) // 16) % 2
    tile = 0.82 + 0.12 * smoothstep(0.0, 0.25, rows) + 0.05 * stagger + 0.06 * (TOWN_RNG.random((h, w)) - 0.5)
    ridge = 1.0 - 0.25 * smoothstep(0.08, 0.0, np.abs(v))
    eave = 1.0 - 0.35 * smoothstep(0.85, 1.0, np.abs(v)) - 0.25 * smoothstep(0.9, 1.0, np.abs(u))
    shade = tile * ridge * eave
    save_rgba(OUT / "env" / "town_roof.png", np.stack([shade] * 3, axis=-1), np.ones((h, w)))
    # The slopes: +y normal on the top half (facing up the screen), -y on
    # the bottom half; each tile row a small step.
    ny = np.where(v < 0, 1.0, -1.0) * (0.75 + 0.2 * (1 - smoothstep(0.0, 0.3, rows)))
    nz = np.ones_like(ny) * 0.8
    nx = np.zeros_like(ny) + 0.35 * np.sign(u) * smoothstep(0.88, 1.0, np.abs(u))   # hipped ends
    n = np.stack([nx, ny, nz], axis=-1)
    n = n / np.linalg.norm(n, axis=-1, keepdims=True)
    save_rgba(OUT / "env" / "town_roof_n.png", n * 0.5 + 0.5, np.ones((h, w)))


def town_tree():
    """A tree crown from above: a dome of leafy clumps."""
    size = 160
    nx, ny, r = radius(size)
    clumps = _sea_noise(size, 10, 3, TOWN_RNG)
    edge = 0.93 + 0.07 * (clumps - 0.5) * 2
    alpha = smoothstep(edge, edge - 0.04, r)
    dome = np.sqrt(np.clip(1 - r * r, 0, 1))
    height = dome * 0.8 + clumps * 0.5
    shade = 0.55 + 0.45 * clumps
    rgb = np.stack([0.24 * shade, 0.50 * shade, 0.18 * shade], axis=-1)
    save_rgba(OUT / "env" / "town_tree.png", rgb, alpha)
    save_rgba(OUT / "env" / "town_tree_n.png", normals_from_height(height, 4.0, wrap=False), alpha)


def town_pond():
    """The pond: an irregular round shape in alpha (the scene colours it
    with the sky), and the same shape full of stars for the night."""
    size = 256
    nx, ny, r = radius(size)
    wob = _sea_noise(size, 4, 2, TOWN_RNG)
    shape = r / (0.9 + 0.12 * (wob - 0.5))
    alpha = smoothstep(1.0, 0.97, shape)
    rim = smoothstep(0.8, 1.0, shape) * alpha
    save_rgba(OUT / "env" / "town_pond.png", np.stack([1.0 - 0.3 * rim] * 3, axis=-1), alpha)
    stars = np.zeros((size, size))
    for _ in range(90):
        sx, sy = TOWN_RNG.integers(0, size, 2)
        stars[sy, sx] = TOWN_RNG.uniform(0.4, 1.0)
    save_rgba(OUT / "env" / "town_pond_stars.png", 1.0, stars * alpha * (1 - rim))


def town_lamp():
    """A street lamp from above: an iron cap with a glass ring."""
    size = 40
    nx, ny, r = radius(size)
    alpha = smoothstep(1.0, 0.9, r)
    glass = smoothstep(0.55, 0.7, r) * smoothstep(0.95, 0.8, r)
    rgb = np.stack([0.12 + 0.8 * glass, 0.11 + 0.7 * glass, 0.10 + 0.45 * glass], axis=-1)
    save_rgba(OUT / "env" / "town_lamp.png", rgb, alpha)



# ── 14 Toon & Outlines ────────────────────────────────────────────────────
#
# Cel-shaded shapes: white (the scene tints each), smooth normal maps that
# toon lighting steps into bands, and a transparent margin round every
# silhouette for the ink outline to draw in.

TOON_RNG = np.random.default_rng(1414)
TOON_PAD = 16          # texels of margin for the outline


def _toon_save(name, height, alpha, rgb=None, strength=3.0):
    if rgb is None:
        rgb = np.ones(alpha.shape + (3,))
    save_rgba(OUT / "toon" / f"{name}.png", rgb, alpha)
    save_rgba(OUT / "toon" / f"{name}_n.png", normals_from_height(height, strength, wrap=False), alpha)


def _dome(size, cx, cy, rx, ry):
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    d = ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2
    return np.sqrt(np.clip(1 - d, 0, 1)), d < 1


def toon_blob():
    """A round creature: an egg of a body on two stubby feet."""
    size = 192
    h = np.zeros((size, size))
    inside = np.zeros((size, size), bool)
    for cx, cy, rx, ry, k in ((96, 88, 70, 64, 1.0), (70, 150, 22, 16, 0.4), (122, 150, 22, 16, 0.4)):
        dome, m = _dome(size, cx, cy, rx, ry)
        h = np.maximum(h, dome * k * max(rx, ry) / 64)
        inside |= m
    alpha = inside.astype(float)
    _toon_save("toon_blob", h * 30, alpha, strength=1.0)


def toon_eye():
    """An eye: white, a black pupil, a glint."""
    size = 32
    nx, ny, r = radius(size)
    alpha = smoothstep(1.0, 0.9, r)
    pupil = np.hypot(nx - 0.15, ny - 0.1) < 0.45
    glint = np.hypot(nx - 0.28, ny + 0.05) < 0.14
    rgb = np.ones((size, size, 3))
    rgb[pupil] = 0.05
    rgb[glint] = 1.0
    ring = (r > 0.8) & (r < 1.0)
    rgb[ring] = 0.05
    save_rgba(OUT / "toon" / "toon_eye.png", rgb, alpha)


def toon_rock():
    """A boulder: a lumpy dome."""
    size = 192
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    ang = np.arctan2(y - 100, x - 96)
    wob = 1 + 0.08 * np.sin(ang * 3 + 0.5) + 0.05 * np.sin(ang * 7 + 1.3)
    d = np.hypot((x - 96) / 76, (y - 100) / 60) / wob
    inside = d < 1
    h = np.sqrt(np.clip(1 - d * d, 0, 1)) + 0.15 * _sea_noise(size, 6, 2, TOON_RNG)
    _toon_save("toon_rock", h * 25 * inside, inside.astype(float), strength=1.5)


def toon_bush():
    """A bush: a clump of round leaf-balls."""
    size = 192
    h = np.zeros((size, size))
    inside = np.zeros((size, size), bool)
    for cx, cy, r in ((70, 110, 44), (122, 110, 44), (96, 72, 46), (60, 70, 32), (132, 70, 32)):
        dome, m = _dome(size, cx, cy, r, r)
        h = np.maximum(h, dome * r / 46)
        inside |= m
    _toon_save("toon_bush", h * 30, inside.astype(float), strength=1.0)


def toon_ground():
    """Tileable meadow: long gentle swells, which toon lighting cuts into
    contour-like patches of light and shade."""
    size = 512
    h = _sea_noise(size, 3, 2, TOON_RNG)
    save_rgba(OUT / "toon" / "toon_ground.png", np.ones((size, size, 3)), np.ones((size, size)))
    save_rgba(OUT / "toon" / "toon_ground_n.png", normals_from_height(h, 18.0), np.ones((size, size)))


# ── 15 Still Water ────────────────────────────────────────────────────────
#
# A mountain lake at dusk, everything above the waterline - the water draws
# its own reflection from what is there. Its own RNG, so re-running leaves
# every earlier scene's art alone.

LAKE_RNG = np.random.default_rng(1515)


def lake_sky():
    """Indigo overhead to a peach glow on the horizon."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.04, 0.05, 0.16])
    mid = np.array([0.22, 0.17, 0.38])
    low = np.array([0.98, 0.62, 0.45])
    a = smoothstep(0.0, 0.7, t)[..., None]
    b = smoothstep(0.62, 1.0, t)[..., None]
    rgb = top * (1 - a) + mid * a
    rgb = rgb * (1 - b) + low * b
    save_rgba(OUT / "env" / "lake_sky.png", rgb, np.ones((h, w)))


def _ridge(w, peaks, jag, rng):
    """A ridge line, 0..1 of the height, tileable sideways."""
    x = np.arange(w) / w
    r = np.zeros(w)
    for k, amp in peaks:
        r += amp * np.sin(x * 2 * np.pi * k + rng.random() * 2 * np.pi)
    # Jagged: folded noise gives sharp peaks rather than rolling hills.
    for k, amp in jag:
        r += amp * np.abs(np.sin(x * np.pi * k + rng.random() * 2 * np.pi))
    return r


def lake_mountains():
    """A far range with snow on its peaks, hazier toward its foot."""
    w, h = 1024, 320
    ridge = 0.5 + _ridge(w, ((2, 0.16), (3, 0.08)), ((9, 0.10), (23, 0.04), (57, 0.015)), LAKE_RNG)
    ridge = np.clip(ridge, 0.15, 0.97)
    y = 1.0 - (np.arange(h) / h)[:, None]           # 0 at the bottom, 1 at the top
    inside = ridge[None, :] - y
    alpha = smoothstep(-0.003, 0.003, inside)
    below_peak = np.clip(inside / 0.22, 0, 1)
    snow = (1.0 - smoothstep(0.1, 0.35, below_peak)) * smoothstep(0.62, 0.75, ridge)[None, :]
    tex = _sea_noise(256, 16, 4, LAKE_RNG)
    grain = tex[np.arange(h)[:, None] % 256, (np.arange(w)[None, :] // 2) % 256]
    snow = np.clip(snow * (0.6 + 0.8 * grain), 0, 1)
    rock = np.array([0.36, 0.38, 0.55]) * (0.85 + 0.3 * grain[..., None])
    rgb = rock * (1 - snow[..., None]) + np.array([0.92, 0.88, 0.95]) * snow[..., None]
    haze = smoothstep(0.35, 0.0, y)[..., None]
    rgb = rgb * (1 - 0.45 * haze) + np.array([0.85, 0.6, 0.6]) * 0.45 * haze
    save_rgba(OUT / "env" / "lake_mountains.png", rgb, alpha)


def lake_pines():
    """A treeline of pines: stacked tiers of triangles, tileable sideways."""
    w, h = 1024, 220
    alpha = np.zeros((h, w))
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    x = 0.0
    while x < w:
        tall = LAKE_RNG.uniform(0.45, 0.98) * h
        half = tall * LAKE_RNG.uniform(0.16, 0.22)
        for off in (-w, 0, w):
            cx = x + off
            depth = h - yy                                     # 0 at the bottom
            t = depth / tall                                   # 0 foot, 1 tip
            tiers = 1.0 - ((t * 5.0) % 1.0) * 0.35             # a notch per tier
            width = half * (1.0 - t) * tiers
            inside = (np.abs(xx - cx) < width) & (t >= 0) & (t <= 1)
            alpha = np.maximum(alpha, inside.astype(np.float64))
        x += LAKE_RNG.uniform(0.35, 0.8) * half * 2
    alpha[h - 30:, :] = 1.0                                    # the shore under them
    shade = 0.8 + 0.2 * _sea_noise(256, 16, 3, LAKE_RNG)[yy.astype(int) % 256, xx.astype(int) % 256]
    rgb = np.array([0.05, 0.09, 0.08]) * shade[..., None]
    save_rgba(OUT / "env" / "lake_pines.png", rgb, alpha)


def lake_cabin():
    """A log cabin, dark against the trees; its windows are cut out, for a
    warm glow sprite behind them to show through."""
    w, h = 256, 200
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    wall = (xx > 30) & (xx < 226) & (yy > 92) & (yy < h)
    roof = (yy > 20) & (yy <= 96) & (np.abs(xx - 128) < (yy - 20) * 1.55 + 4)
    chimney = (xx > 170) & (xx < 190) & (yy > 18) & (yy < 70)
    window = ((xx > 58) & (xx < 94) | (xx > 162) & (xx < 198)) & (yy > 118) & (yy < 152)
    door = (xx > 110) & (xx < 144) & (yy > 124)
    alpha = ((wall | roof | chimney) & ~window).astype(np.float64)
    logs = 0.8 + 0.2 * np.sin(yy * 0.9)
    rgb = np.where(roof[..., None], np.array([0.08, 0.06, 0.07]), np.array([0.16, 0.10, 0.07]) * logs[..., None])
    rgb = np.where(door[..., None], np.array([0.09, 0.06, 0.05]), rgb)
    save_rgba(OUT / "env" / "lake_cabin.png", rgb, alpha)


def lake_pier():
    """A jetty seen side-on: a plank deck on posts, the posts ending at the
    sprite's bottom edge, which is placed on the waterline."""
    w, h = 512, 96
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    deck = (yy > 18) & (yy < 34)
    posts = np.zeros((h, w), bool)
    for px in range(14, w, 70):
        posts |= (np.abs(xx - px) < 6) & (yy >= 30)
    rail_posts = np.zeros((h, w), bool)
    for px in range(14, w, 140):
        rail_posts |= (np.abs(xx - px) < 3) & (yy < 22)
    rail = (yy > 2) & (yy < 7)
    alpha = (deck | posts | rail_posts | rail).astype(np.float64)
    grain = 0.75 + 0.25 * np.sin(xx * 0.31 + np.sin(yy * 0.7) * 2)
    planks = 0.85 + 0.15 * ((xx // 22) % 2)
    rgb = np.array([0.30, 0.20, 0.13]) * (grain * np.where(deck, planks, 1.0))[..., None]
    rgb = np.where(posts[..., None], rgb * 0.6, rgb)
    save_rgba(OUT / "env" / "lake_pier.png", rgb, alpha)


def lake_reeds():
    """Reeds and cattails for the near shore, tileable sideways."""
    w, h = 512, 160
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    alpha = np.zeros((h, w))
    for _ in range(90):
        x0 = LAKE_RNG.uniform(0, w)
        tall = LAKE_RNG.uniform(0.35, 1.0) * h
        lean = LAKE_RNG.uniform(-0.35, 0.35)
        t = (h - yy) / tall
        cx = x0 + lean * (h - yy) * t
        for off in (-w, 0, w):
            blade = (np.abs(xx - cx - off) < 1.6 * (1.1 - t)) & (t >= 0) & (t <= 1)
            alpha = np.maximum(alpha, blade.astype(np.float64))
        if LAKE_RNG.random() < 0.25:
            ty, tx = h - tall * 0.9, x0 + lean * tall * 0.81
            for off in (-w, 0, w):
                head = ((xx - tx - off) / 3.5) ** 2 + ((yy - ty) / 11.0) ** 2 < 1.0
                alpha = np.maximum(alpha, head.astype(np.float64))
    rgb = np.array([0.04, 0.07, 0.05])
    save_rgba(OUT / "env" / "lake_reeds.png", rgb, alpha)


def lake_boat():
    """A rowing boat side-on, a figure in it and a lantern on a pole; its
    waterline is the texture's bottom edge."""
    w, h = 200, 110
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    wl = h - 1                                          # the waterline row
    # Hull: a shallow crescent, sheer rising to both ends.
    u = (xx - w / 2) / (w * 0.44)
    top = wl - 22 - 12 * u ** 2
    bottom = wl - 2 + 6 * (1 - u ** 2)
    hull = (np.abs(u) < 1) & (yy > top) & (yy < np.minimum(bottom, wl))
    # The figure, hunched at the oars.
    head = (xx - 92) ** 2 + (yy - (wl - 58)) ** 2 < 8 ** 2
    body = ((xx - 94) / 11) ** 2 + ((yy - (wl - 38)) / 16) ** 2 < 1
    oar = (np.abs((yy - (wl - 34)) - (xx - 94) * 0.55) < 1.6) & (xx > 94) & (xx < 150)
    # A lantern pole at the stern.
    pole = (np.abs(xx - 36) < 1.5) & (yy > wl - 78) & (yy < wl - 20)
    arm = (yy > wl - 79) & (yy < wl - 76) & (xx > 24) & (xx < 38)
    alpha = (hull | head | body | oar | pole | arm).astype(np.float64)
    plank = 0.85 + 0.15 * (((yy - top) // 5) % 2)
    rgb = np.where(hull[..., None], np.array([0.20, 0.12, 0.08]) * plank[..., None], np.array([0.05, 0.05, 0.07]))
    save_rgba(OUT / "env" / "lake_boat.png", rgb, alpha)


# ── 16 Festival Night ─────────────────────────────────────────────────────
#
# A harbour town across the water, and boats moored in front of it. The
# windows are a second texture - white in alpha - drawn additively over the
# silhouette, so they glow and the water mirrors them.

HARBOUR_RNG = np.random.default_rng(1616)


def harbour_town():
    """Gabled houses, a tower and a church spire, tileable sideways; and the
    lit windows in them, as their own texture."""
    w, h = 1024, 280
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    body = np.zeros((h, w), bool)
    windows = np.zeros((h, w))
    x = 0
    spire_at = w * 0.62
    while x < w:
        bw = int(HARBOUR_RNG.integers(46, 110))
        bh = int(HARBOUR_RNG.integers(70, 170))
        if abs(x + bw / 2 - spire_at) < 60:
            bw, bh = 70, 150
        top = h - bh
        wall = (xx >= x) & (xx < x + bw) & (yy >= top)
        kind = HARBOUR_RNG.integers(0, 3)
        cx = x + bw / 2
        if kind == 0:        # pitched gable
            roof = (yy >= top - bw * 0.45) & (yy < top) & (np.abs(xx - cx) < (yy - (top - bw * 0.45)) / 0.45 * 0.5)
        elif kind == 1:      # stepped gable
            steps = np.floor(np.abs(xx - cx) / (bw / 8))
            roof = (np.abs(xx - cx) < bw / 2) & (yy >= top - (4 - steps) * 12) & (yy < top)
        else:                # flat, with a chimney
            roof = (xx > x + bw * 0.7) & (xx < x + bw * 0.8) & (yy >= top - 20) & (yy < top)
        body |= wall | roof
        # Windows: a grid on the facade, some lit.
        for wy in range(top + 14, h - 26, 26):
            for wx in range(x + 8, x + bw - 14, 18):
                if HARBOUR_RNG.random() < 0.42:
                    lit = HARBOUR_RNG.uniform(0.55, 1.0)
                    windows[wy:wy + 12, wx:wx + 8] = np.maximum(windows[wy:wy + 12, wx:wx + 8], lit)
        x += bw + int(HARBOUR_RNG.integers(-6, 4))
    # The church: a spire over the square tower.
    spire = (np.abs(xx - spire_at) < (yy - 10) * 0.09) & (yy >= 10) & (yy < h - 150)
    body |= spire
    cross = ((np.abs(xx - spire_at) < 1.5) & (yy >= 0) & (yy < 12)) | ((np.abs(xx - spire_at) < 5) & (yy >= 3) & (yy < 6))
    body |= cross
    windows = np.where(body, windows, 0.0)
    rgb = np.array([0.045, 0.05, 0.085]) * (0.9 + 0.1 * np.sin(xx * 0.05))[..., None]
    save_rgba(OUT / "env" / "harbour_town.png", rgb, body.astype(np.float64))
    save_rgba(OUT / "env" / "harbour_windows.png", 1.0, windows)


def harbour_boats():
    """Moored boats: hulls on the waterline (the texture's bottom), masts and
    rigging above, tileable sideways."""
    w, h = 1024, 220
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    alpha = np.zeros((h, w))
    x = 40.0
    while x < w - 80:
        length = HARBOUR_RNG.uniform(70, 150)
        hull_h = length * 0.16
        cx = x + length / 2
        u = (xx - cx) / (length / 2)
        v = (h - yy) / hull_h                                   # 0 at the waterline
        hull = (np.abs(u) < 1.0) & (v >= 0) & (v < 1.0) & (np.abs(u) < 0.75 + 0.25 * v)
        mast_h = length * HARBOUR_RNG.uniform(0.9, 1.3)
        mast = (np.abs(xx - cx) < 1.6) & (yy > h - hull_h - mast_h) & (yy < h - hull_h)
        top = np.array([cx, h - hull_h - mast_h])
        rig = np.zeros((h, w), bool)
        for end in (cx - length * 0.45, cx + length * 0.45):
            # A stay from the masthead to each end of the hull.
            t = np.clip((yy - top[1]) / (h - hull_h - top[1]), 0, 1)
            lx = top[0] + (end - top[0]) * t
            rig |= (np.abs(xx - lx) < 0.7) & (yy >= top[1]) & (yy < h - hull_h)
        alpha = np.maximum(alpha, (hull | mast | rig).astype(np.float64))
        x += length + HARBOUR_RNG.uniform(30, 120)
    rgb = np.array([0.035, 0.04, 0.06])
    save_rgba(OUT / "env" / "harbour_boats.png", rgb, alpha)


# ── 17 Murmuration ────────────────────────────────────────────────────────
#
# Starlings over winter fields at sunset. The birds are white in alpha - the
# flock's colour tints them - and point right, the way a boid faces.

MURM_RNG = np.random.default_rng(1717)


def _bird(size, wing_span, wing_sweep, body_len, tail):
    """A bird seen from below, pointing right: a body, swept wings, a tail."""
    y, x = np.mgrid[0:size, 0:size].astype(np.float64)
    u = (x + 0.5) / size * 2 - 1          # along the body, + is the head
    v = (y + 0.5) / size * 2 - 1          # across it
    body = (u / body_len) ** 2 + (v / 0.13) ** 2 < 1.0
    head = ((u - body_len * 0.85) / 0.16) ** 2 + (v / 0.11) ** 2 < 1.0
    # Wings: a broad root at the body's middle tapering to a swept point,
    # the leading edge bowed forward like a starling's.
    a = np.abs(v) / wing_span                                  # 0 at the root, 1 at the tip
    lead = 0.16 - wing_sweep * a ** 1.6 * 0.6
    trail = lead - 0.42 * np.clip(1.0 - a, 0.0, None) ** 0.8 - 0.02
    wing = (a < 1.0) & (u < lead) & (u > trail)
    tail_mask = (u < -body_len * 0.7) & (u > -body_len * 0.7 - tail) & (np.abs(v) < 0.05 + (-body_len * 0.7 - u) * 0.6)
    return (body | head | wing | tail_mask).astype(np.float64)


def murm_birds():
    save_rgba(OUT / "fx" / "starling.png", 1.0, _bird(32, 0.92, 0.55, 0.42, 0.22))
    save_rgba(OUT / "fx" / "falcon.png", 1.0, _bird(96, 0.95, 0.9, 0.5, 0.3))


def murm_sky():
    """Blue overhead through rose to a molten band on the horizon."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    stops = [(0.0, (0.16, 0.22, 0.42)), (0.45, (0.55, 0.42, 0.58)), (0.75, (0.96, 0.58, 0.45)),
             (0.9, (1.0, 0.72, 0.38)), (1.0, (1.0, 0.82, 0.5))]
    rgb = np.zeros((h, w, 3))
    for (t0, c0), (t1, c1) in zip(stops, stops[1:]):
        k = np.clip((t - t0) / (t1 - t0), 0, 1)[..., None]
        seg = (t >= t0)[..., None] & (t <= t1)[..., None]
        rgb = np.where(seg, np.array(c0) * (1 - k) + np.array(c1) * k, rgb)
    save_rgba(OUT / "env" / "murm_sky.png", rgb, np.ones((h, w)))


def murm_clouds():
    """Long, thin evening clouds, tileable sideways: white in alpha."""
    w, h = 1024, 256
    n = _sea_noise(256, 8, 5, MURM_RNG)
    yy, xx = np.mgrid[0:h, 0:w]
    tex = n[(yy // 1) % 256, (xx // 4) % 256]            # stretched sideways
    bands = np.exp(-((yy / h - 0.5) / 0.22) ** 2)
    alpha = smoothstep(0.48, 0.78, tex) * bands
    save_rgba(OUT / "env" / "murm_clouds.png", 1.0, alpha)


def _branch(alpha, x, y, angle, length, width, depth):
    """Draw a branch and its children into `alpha` (y down)."""
    import math
    steps = int(length)
    for i in range(steps):
        t = i / max(steps, 1)
        px = x + math.cos(angle) * i
        py = y - math.sin(angle) * i
        r = max(0.6, width * (1 - 0.6 * t))
        x0, x1 = int(px - r - 1), int(px + r + 2)
        y0, y1 = int(py - r - 1), int(py + r + 2)
        if x1 < 0 or y1 < 0 or x0 >= alpha.shape[1] or y0 >= alpha.shape[0]:
            continue
        yy, xx = np.mgrid[max(y0, 0):min(y1, alpha.shape[0]), max(x0, 0):min(x1, alpha.shape[1])]
        d = np.hypot(xx - px, yy - py)
        alpha[yy, xx] = np.maximum(alpha[yy, xx], np.clip(r - d + 0.5, 0, 1))
    if depth == 0:
        return
    ex = x + math.cos(angle) * length
    ey = y - math.sin(angle) * length
    for _ in range(int(MURM_RNG.integers(2, 4))):
        a = angle + MURM_RNG.uniform(-0.75, 0.75)
        _branch(alpha, ex, ey, a, length * MURM_RNG.uniform(0.6, 0.78), width * 0.62, depth - 1)


def murm_tree():
    """A bare winter tree, branching to twigs."""
    import math
    w, h = 384, 384
    alpha = np.zeros((h, w))
    _branch(alpha, w / 2, h - 1, math.pi / 2, 120, 9.0, 6)
    save_rgba(OUT / "env" / "murm_tree.png", np.array([0.06, 0.05, 0.07]), alpha)


def murm_fields():
    """Low rolling fields with hedgerows, tileable sideways."""
    w, h = 1024, 200
    x = np.arange(w) / w
    ridge = 0.55 + 0.1 * np.sin(x * 2 * np.pi * 2 + 1.0) + 0.05 * np.sin(x * 2 * np.pi * 7 + 2.0)
    yy = 1.0 - (np.arange(h) / h)[:, None]
    alpha = smoothstep(-0.004, 0.004, ridge[None, :] - yy)
    hedge = _sea_noise(256, 32, 3, MURM_RNG)[(np.arange(h)[:, None]) % 256, (np.arange(w)[None, :] // 2) % 256]
    bumps = (ridge[None, :] + 0.05 * hedge - yy)
    alpha = np.maximum(alpha, smoothstep(-0.004, 0.004, bumps) * smoothstep(0.62, 0.7, hedge))
    rgb = np.array([0.10, 0.07, 0.10]) * (0.85 + 0.15 * hedge[..., None])
    save_rgba(OUT / "env" / "murm_fields.png", rgb, alpha)


# ── 18 Graveyard Watch ────────────────────────────────────────────────────
#
# A chapel graveyard at night, in the same 320x180 pixel art as Pixel
# Perfect - but lit, and lit the way a palette-limited game had to be: the
# light comes in four levels with the steps between them dithered (the
# scene's `toon_lighting.dither`). So the art is flat, mid-bright colour:
# the lighting does all the shading.

RETRO_RNG = np.random.default_rng(1818)

RT_SKY = [_hex(c) for c in ("#06051a", "#0b0b28", "#111436", "#181c46", "#212858")]
RT_STONE = [_hex(c) for c in ("#0e0b16", "#3d3a56", "#5a5775", "#807d9c", "#a9a7c4")]
RT_MOSS = [_hex(c) for c in ("#2d4a33", "#4f7a45", "#78a85a")]
RT_DIRT = [_hex(c) for c in ("#0e0b16", "#4d3a3a", "#6b5146", "#8d6d57")]
RT_IRON = [_hex(c) for c in ("#0e0b16", "#3a3850", "#5d5a78")]
RT_GLASS = [_hex(c) for c in ("#8c2f45", "#2f5d9c", "#d9a441")]
RT_FLAME = [_hex(c) for c in ("#ffe7a0", "#ffb445", "#e8612d")]
RT_GHOST = [_hex(c) for c in ("#8fd8e0", "#d8f6f2")]


def retro_sky():
    """Night sky in five bands, ordered-dithered into each other, and stars."""
    w, h = 352, 140
    y = np.arange(h)[:, None] * np.ones((1, w))
    t = y / h * (len(RT_SKY) - 1)
    band = np.floor(t).astype(int)
    frac = t - band
    xx = np.arange(w)[None, :] * np.ones((h, 1))
    thr = BAYER4[(y.astype(int) % 4), (xx.astype(int) % 4)]
    idx = np.clip(band + (frac > thr), 0, len(RT_SKY) - 1)
    rgb = np.array(RT_SKY)[idx]
    for _ in range(140):
        sx, sy = RETRO_RNG.integers(0, w), RETRO_RNG.integers(0, int(h * 0.8))
        rgb[sy, sx] = _hex("#cfd6ff") if RETRO_RNG.random() < 0.8 else _hex("#fff2b8")
    for _ in range(8):
        sx, sy = RETRO_RNG.integers(2, w - 2), RETRO_RNG.integers(2, int(h * 0.6))
        for dx, dy in ((0, 0), (1, 0), (-1, 0), (0, 1), (0, -1)):
            rgb[sy + dy, sx + dx] = _hex("#ffffff") if (dx, dy) == (0, 0) else _hex("#8f98d8")
    _px_save("rt_sky.png", rgb, np.ones((h, w)))


def retro_moon():
    size = 20
    y, x = np.mgrid[0:size, 0:size] + 0.5
    r = np.hypot(x - size / 2, y - size / 2)
    disc = r < size / 2 - 0.5
    rgb = np.zeros((size, size, 3))
    rgb[disc] = _hex("#f1efd6")
    for cx, cy, cr in ((7, 7, 2.2), (13, 11, 2.8), (8, 14, 1.6)):
        rgb[disc & (np.hypot(x - cx, y - cy) < cr)] = _hex("#c9c6a8")
    shade = disc & (x + y > size * 1.35)
    rgb[shade] = rgb[shade] * 0.85
    _px_save("rt_moon.png", rgb, disc.astype(float))


def retro_hills():
    """Far hills with a line of pines on them."""
    w, h = 352, 44
    xs = np.arange(w)
    ridge = (18 + 5 * np.sin(xs / 29.0) + 3 * np.sin(xs / 11.0 + 1.3)).astype(int)
    alpha = np.zeros((h, w))
    rgb = np.zeros((h, w, 3))
    for x in range(w):
        alpha[ridge[x]:, x] = 1
    for x in range(2, w - 2, 5):
        tall = int(RETRO_RNG.integers(4, 10))
        top = ridge[x] - tall
        for k in range(tall):
            half = k // 3
            alpha[top + k, max(0, x - half):x + half + 1] = 1
    rgb[:] = _hex("#141a33")
    _px_save("rt_hills.png", rgb, alpha)


def retro_chapel():
    """A stone chapel front: gable, bell turret, an arched door and two
    stained windows."""
    w, h = 120, 92
    rgb = np.zeros((h, w, 3))
    solid = np.zeros((h, w), bool)
    wall_top = 40
    solid[wall_top:, 8:112] = True
    for yy in range(0, 34):                         # gable
        half = int(yy * 1.55) + 2
        solid[wall_top - 34 + yy, max(8, 60 - half):min(112, 60 + half)] = True
    solid[0:14, 55:65] = True                       # bell turret
    solid[0:3, 54:66] = True
    # Stone: brick courses, each a shade picked per brick.
    for yy in range(h):
        row = yy // 4
        off = 4 if row % 2 else 0
        for xx in range(w):
            if not solid[yy, xx]:
                continue
            brick = (xx + off) // 8
            k = 2 + int((row * 7 + brick * 13) % 3 == 0) - int((row * 5 + brick * 3) % 4 == 0)
            rgb[yy, xx] = RT_STONE[k]
            if yy % 4 == 0 or (xx + off) % 8 == 0:
                rgb[yy, xx] = RT_STONE[1]
    # The gable's coping and a moss line along the foot.
    rgb[wall_top - 1, 8:112] = RT_STONE[4]
    for xx in range(8, 112):
        if RETRO_RNG.random() < 0.6:
            rgb[h - 1 - int(RETRO_RNG.integers(0, 3)), xx] = RT_MOSS[int(RETRO_RNG.integers(0, 3))]
    # Arched door.
    door = np.zeros((h, w), bool)
    for yy in range(58, h):
        for xx in range(51, 69):
            dy = yy - 64
            if dy >= 0 or (xx - 60) ** 2 + (dy * 1.5) ** 2 < 81:
                door[yy, xx] = True
    rgb[door] = RT_DIRT[2]
    for xx in range(52, 68, 4):
        rgb[door[:, xx], xx] = RT_DIRT[1]
    rgb[_outline(door)] = RT_DIRT[0]
    # Stained windows: arched, three panes of colour, iron leading.
    for cx in (30, 90):
        win = np.zeros((h, w), bool)
        for yy in range(50, 74):
            for xx in range(cx - 6, cx + 6):
                dy = yy - 56
                if dy >= 0 or (xx - cx + 0.5) ** 2 + (dy * 1.3) ** 2 < 36:
                    win[yy, xx] = True
        ys, xs = np.nonzero(win)
        for yy, xx in zip(ys, xs):
            rgb[yy, xx] = RT_GLASS[((yy // 5) + (xx - cx + 6) // 4) % 3]
        rgb[win & ((np.arange(w)[None, :] - cx) % 4 == 0)] = RT_IRON[1]
        rgb[_outline(win)] = RT_IRON[0]
    # Turret opening and bell.
    rgb[4:11, 58:62] = RT_IRON[0]
    rgb[7:10, 59:61] = _hex("#b99a4a")
    rgb[_outline(solid)] = RT_STONE[0]
    _px_save("rt_chapel.png", rgb, solid.astype(float))
    # The windows alone, for the glow drawn over them.
    glow = np.zeros((h, w, 3))
    glow_a = np.zeros((h, w))
    for cx in (30, 90):
        for yy in range(50, 74):
            for xx in range(cx - 6, cx + 6):
                dy = yy - 56
                if dy >= 0 or (xx - cx + 0.5) ** 2 + (dy * 1.3) ** 2 < 36:
                    glow[yy, xx] = rgb[yy, xx]
                    glow_a[yy, xx] = 1.0 if (xx - cx) % 4 else 0.0
    _px_save("rt_chapel_glass.png", glow, glow_a)


def retro_ground():
    """Grass over dirt, tileable sideways."""
    w, h = 64, 34
    rgb = np.zeros((h, w, 3))
    rgb[:] = RT_DIRT[2]
    for yy in range(h):
        for xx in range(w):
            n = (xx * 7 + yy * 13 + (xx * yy) % 5) % 9
            if n == 0:
                rgb[yy, xx] = RT_DIRT[3]
            elif n == 4:
                rgb[yy, xx] = RT_DIRT[1]
    for xx in range(w):
        depth = 3 + (xx * 5 % 7 == 0) + (xx * 3 % 11 == 0)
        rgb[0:depth, xx] = RT_MOSS[1]
        rgb[0, xx] = RT_MOSS[2] if xx % 3 else RT_MOSS[1]
        rgb[depth, xx] = RT_MOSS[0]
    for _ in range(18):
        sx, sy = RETRO_RNG.integers(0, w - 2), RETRO_RNG.integers(8, h - 2)
        rgb[sy, sx:sx + 2] = RT_STONE[3]
        rgb[sy + 1, sx:sx + 2] = RT_STONE[1]
    _px_save("rt_ground.png", rgb, np.ones((h, w)))


def retro_graves():
    shapes = {
        "rt_grave_round.png": (12, 16, "round"),
        "rt_grave_cross.png": (11, 17, "cross"),
        "rt_grave_obelisk.png": (8, 21, "obelisk"),
    }
    for name, (w, h, kind) in shapes.items():
        solid = np.zeros((h, w), bool)
        if kind == "round":
            solid[4:, 1:w - 1] = True
            for yy in range(0, 5):
                for xx in range(w):
                    if (xx - (w - 1) / 2) ** 2 + ((yy - 5) * 1.2) ** 2 < ((w - 2) / 2) ** 2:
                        solid[yy, xx] = True
        elif kind == "cross":
            solid[:, 4:7] = True
            solid[4:7, 0:w] = True
            solid[h - 3:, 2:9] = True
        else:
            for yy in range(h):
                half = 1 + yy * 3 // h
                solid[yy, w // 2 - half:w // 2 + half] = True
            solid[h - 3:, 0:w] = True
        rgb = np.zeros((h, w, 3))
        rgb[solid] = RT_STONE[3]
        rgb[solid & (np.arange(w)[None, :] > w * 0.6)] = RT_STONE[2]
        if kind == "round":
            rgb[7, 3:w - 3] = RT_STONE[1]
            rgb[9, 4:w - 4] = RT_STONE[1]
        rgb[h - 1, :][solid[h - 1]] = RT_MOSS[1]
        rgb[_outline(solid)] = RT_STONE[0]
        _px_save(name, rgb, solid.astype(float))


def retro_tree():
    """A dead tree, one-pixel twigs."""
    import math
    w, h = 50, 70
    solid = np.zeros((h, w), bool)

    def line(x0, y0, x1, y1, thick):
        n = int(max(abs(x1 - x0), abs(y1 - y0))) + 1
        for i in range(n + 1):
            t = i / max(n, 1)
            x = x0 + (x1 - x0) * t
            y = y0 + (y1 - y0) * t
            for dx in range(-thick // 2, thick - thick // 2):
                xi, yi = int(round(x)) + dx, int(round(y))
                if 0 <= xi < w and 0 <= yi < h:
                    solid[yi, xi] = True

    def branch(x, y, ang, length, thick, depth):
        x1 = x + math.cos(ang) * length
        y1 = y - math.sin(ang) * length
        line(x, y, x1, y1, thick)
        if depth > 0:
            for _ in range(2):
                branch(x1, y1, ang + RETRO_RNG.uniform(-0.8, 0.8), length * 0.7, max(1, thick - 1), depth - 1)

    branch(25, h - 1, math.pi / 2, 24, 4, 5)
    rgb = np.zeros((h, w, 3))
    rgb[solid] = RT_DIRT[1]
    rgb[solid & (np.arange(w)[None, :] < 25)] = RT_DIRT[2]
    rgb[_outline(solid)] = RT_DIRT[0]
    _px_save("rt_tree.png", rgb, solid.astype(float))


def retro_fence():
    w, h = 32, 16
    solid = np.zeros((h, w), bool)
    for x in range(1, w, 4):
        solid[3:, x] = True
        solid[1, x] = True
        solid[2, x - 1:x + 2] = True
    solid[6, :] = True
    solid[13, :] = True
    rgb = np.zeros((h, w, 3))
    rgb[solid] = RT_IRON[2]
    rgb[solid & (np.arange(h)[:, None] > 8)] = RT_IRON[1]
    _px_save("rt_fence.png", rgb, solid.astype(float))


def retro_small():
    """The torch (bracket, then flame), the lantern and the ghost."""
    # Bracket: lit; the flame is its own sprite, unlit.
    br = np.zeros((6, 5), bool)
    br[2:, 2] = True
    br[1, 1:4] = True
    rgb = np.zeros((6, 5, 3))
    rgb[br] = RT_IRON[2]
    _px_save("rt_bracket.png", rgb, br.astype(float))
    fl = np.array([[0, 0, 1, 0, 0],
                   [0, 1, 1, 0, 0],
                   [0, 1, 2, 1, 0],
                   [1, 2, 3, 2, 1],
                   [1, 2, 3, 2, 1],
                   [0, 1, 2, 1, 0]])
    rgb = np.zeros((6, 5, 3))
    for k, c in ((1, RT_FLAME[2]), (2, RT_FLAME[1]), (3, RT_FLAME[0])):
        rgb[fl == k] = c
    _px_save("rt_flame.png", rgb, (fl > 0).astype(float))
    ln = np.array([[0, 1, 1, 1, 0],
                   [1, 0, 0, 0, 1],
                   [1, 2, 3, 2, 1],
                   [1, 3, 3, 3, 1],
                   [1, 2, 3, 2, 1],
                   [1, 1, 1, 1, 1]])
    rgb = np.zeros((6, 5, 3))
    rgb[ln == 1] = RT_IRON[2]
    rgb[ln == 2] = RT_FLAME[1]
    rgb[ln == 3] = RT_FLAME[0]
    _px_save("rt_lantern.png", rgb, (ln > 0).astype(float))
    w, h = 12, 15
    gh = np.zeros((h, w), bool)
    for yy in range(h):
        for xx in range(w):
            if yy < 7:
                gh[yy, xx] = (xx - 5.5) ** 2 + ((yy - 6) * 1.1) ** 2 < 30
            else:
                gh[yy, xx] = 1 <= xx <= 10 and not (yy == h - 1 and xx % 3 == 0) and not (yy == h - 2 and xx % 3 == 1 and yy > 12)
    rgb = np.zeros((h, w, 3))
    rgb[gh] = RT_GHOST[1]
    rgb[_outline(gh)] = RT_GHOST[0]
    rgb[5:7, 3:5] = _hex("#141a33")
    rgb[5:7, 7:9] = _hex("#141a33")
    _px_save("rt_ghost.png", rgb, gh.astype(float))


# ── 19 God Rays ───────────────────────────────────────────────────────────
#
# A forest at morning: a bright sky behind a canopy full of holes, and trunks
# at three depths. Everything but the sky is a silhouette - the shafts are
# the light the sky throws through the gaps (`light_shafts`).

GR_RNG = np.random.default_rng(1919)


def gr_sky():
    """Pale gold overhead to a warm white glow low down."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.72, 0.80, 0.86])
    mid = np.array([0.98, 0.90, 0.72])
    low = np.array([1.0, 0.82, 0.58])
    a = smoothstep(0.0, 0.55, t)[..., None]
    b = smoothstep(0.55, 1.0, t)[..., None]
    rgb = top * (1 - a) + mid * a
    rgb = rgb * (1 - b) + low * b
    save_rgba(OUT / "env" / "gr_sky.png", rgb, np.ones((h, w)))


def gr_haze():
    """The forest's depth below the canopy: clear at the top, where the sky
    shows through the leaves, thickening to haze. White, tinted in the scene;
    it stays below the shafts' threshold, so only the sky above casts them."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    alpha = smoothstep(0.18, 0.5, t)
    save_rgba(OUT / "env" / "gr_haze.png", 1.0, alpha)


def gr_canopy():
    """A mass of leaves hanging from the top, full of holes and thinning to
    ragged clumps below, tileable sideways: white in alpha, tinted per layer.
    The holes are the point - each is a shaft once the sky shows through."""
    w, h = 1024, 420
    yy, xx = np.mgrid[0:h, 0:w]
    def spread(n):
        return (n - n.min()) / max(n.max() - n.min(), 1e-6)     # the full 0..1
    big = spread(_sea_noise(256, 8, 4, GR_RNG))[(yy // 2) % 256, (xx // 4) % 256]
    mid = spread(_sea_noise(256, 20, 4, GR_RNG))[yy % 256, (xx // 2) % 256]
    leaf = spread(_sea_noise(256, 40, 2, GR_RNG))[yy % 256, xx % 256]
    y = yy / h
    density = 0.74 - 0.8 * y + 0.55 * (big - 0.5) + 0.45 * (mid - 0.5) + 0.3 * (leaf - 0.5)
    alpha = smoothstep(0.45, 0.52, density)
    save_rgba(OUT / "env" / "gr_canopy.png", 1.0, alpha)


def gr_trunk():
    """A straight trunk, bark-streaked, with a couple of branch stubs."""
    w, h = 96, 640
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    half = 22 + 8 * (yy / h) ** 2               # wider at the foot
    body = np.abs(xx - w / 2) < half
    for by, side in ((140, 1), (260, -1), (380, 1)):
        stub = (np.abs((yy - by) + side * (xx - w / 2) * 0.8) < 5) & (np.abs(xx - w / 2) < 46)
        body |= stub & (side * (xx - w / 2) > 0)
    bark = 0.96 + 0.04 * np.sin(xx * 0.9 + np.sin(yy * 0.05) * 3.0)
    save_rgba(OUT / "env" / "gr_trunk.png", np.stack([bark] * 3, -1), body.astype(np.float64))


def gr_floor():
    """The forest floor with ferns, tileable sideways: white in alpha."""
    import math
    w, h = 1024, 220
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    ground = 0.62 + 0.04 * np.sin(xx / w * 2 * np.pi * 3) + 0.02 * np.sin(xx / w * 2 * np.pi * 11)
    alpha = (1.0 - yy / h < ground).astype(np.float64)
    for _ in range(38):
        x0 = GR_RNG.uniform(0, w)
        base = h * (1 - 0.6)
        for k in range(7):
            ang = math.radians(20 + k * 20) + GR_RNG.uniform(-0.1, 0.1)
            length = GR_RNG.uniform(50, 95)
            for t in np.linspace(0, 1, 60):
                px = x0 + math.cos(ang) * length * t
                py = base - math.sin(ang) * length * t + (t ** 2) * length * 0.35
                r = 3.5 * (1 - t) + 0.8
                for off in (-w, 0, w):
                    d = np.hypot(xx - px - off, yy - py)
                    alpha = np.maximum(alpha, (d < r).astype(np.float64))
    save_rgba(OUT / "env" / "gr_floor.png", 1.0, alpha)


def gr_leaf():
    """A falling leaf, white in alpha."""
    size = 24
    y, x = np.mgrid[0:size, 0:size] + 0.5
    u, v = (x - size / 2) / (size / 2), (y - size / 2) / (size / 2)
    leaf = (u / 0.95) ** 2 + (v / 0.45) ** 2 < 1.0
    stem = (np.abs(v) < 0.08) & (u > 0.6)
    save_rgba(OUT / "fx" / "gr_leaf.png", 1.0, (leaf | stem).astype(np.float64))


# ── 20 Storm ──────────────────────────────────────────────────────────────
#
# A village street at night in a storm: a heavy sky, cloud banks that roll
# across it, houses whose roofs the rain lands on, a lamp, cobbles and
# puddles. The houses are silhouettes - the street is lit by its lamp and its
# windows, and the lightning lights the rest.

ST_RNG = np.random.default_rng(2020)


def st_sky():
    """Near black overhead to a bruised grey-blue low down."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.05, 0.06, 0.09])
    low = np.array([0.20, 0.23, 0.30])
    a = smoothstep(0.0, 1.0, t)[..., None] ** 1.5
    save_rgba(OUT / "env" / "st_sky.png", top * (1 - a) + low * a, np.ones((h, w)))


def st_clouds():
    """A bank of storm cloud, tileable sideways: lit from above, dark
    beneath, ragged top and bottom."""
    w, h = 1024, 360
    yy, xx = np.mgrid[0:h, 0:w]

    def spread(n):
        return (n - n.min()) / max(n.max() - n.min(), 1e-6)
    big = spread(_sea_noise(256, 6, 5, ST_RNG))[(yy // 2) % 256, (xx // 4) % 256]
    fine = spread(_sea_noise(256, 24, 3, ST_RNG))[yy % 256, (xx // 2) % 256]
    y = yy / h
    # A band thickest in the middle, its edges eaten by the noise.
    band = 1.0 - np.abs(y - 0.5) * 2.0
    dens = band * 0.9 + 0.6 * (big - 0.5) + 0.25 * (fine - 0.5)
    alpha = smoothstep(0.25, 0.55, dens)
    # Lit from above: bright tops, dark undersides, the billows shaded.
    shade = 0.35 + 0.55 * (1.0 - y) * (0.6 + 0.4 * big) + 0.1 * fine
    rgb = np.stack([shade * 0.92, shade * 0.95, shade * 1.05], -1)
    save_rgba(OUT / "env" / "st_clouds.png", rgb, alpha)


def _st_house(name, w, h, roof, windows, chimney=None):
    """A house silhouette: walls, a gable roof `roof` px tall, window holes
    left clear (the windows are their own lit sprites) - white in alpha."""
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    wall = (yy >= roof) & (xx >= 8) & (xx < w - 8)
    # Gable: a triangle over the walls, eaves past them by 8 px.
    half = w / 2
    ridge = np.abs(xx - half) / half * roof               # 0 at the ridge
    gable = (yy >= ridge) & (yy < roof + 4)
    alpha = (wall | gable).astype(np.float64)
    if chimney:
        cx, cw, ch = chimney
        # From above the slope where it stands, down into the roof.
        at = int(abs(cx + cw / 2 - half) / half * roof)
        alpha[max(0, at - ch):at + 20, cx:cx + cw] = 1.0
    for (x0, y0, x1, y1) in windows:
        alpha[y0:y1, x0:x1] = 0.0
    # Siding and roof tiles, faint - they show only in the lightning.
    rgb = np.ones((h, w, 3)) * 0.55
    rgb[yy < roof] *= (0.75 + 0.25 * (np.sin(yy[yy < roof] * 0.9) > 0))[..., None]
    rgb[yy >= roof] *= (0.85 + 0.15 * (np.sin(xx[yy >= roof] * 0.5) > 0.6))[..., None]
    save_rgba(OUT / "env" / f"st_{name}.png", rgb, alpha)


def st_houses():
    _st_house("house_a", 300, 440, 150, [(60, 210, 110, 280), (190, 210, 240, 280), (60, 320, 110, 390)],
              chimney=(210, 30, 40))
    _st_house("house_b", 400, 320, 130, [(70, 180, 140, 240), (260, 180, 330, 240), (170, 200, 230, 320)])
    _st_house("house_c", 240, 380, 120, [(90, 170, 150, 240), (90, 280, 150, 340)], chimney=(40, 26, 30))


def st_window():
    """A lit window: warm glass with a cross of glazing bars."""
    w, h = 64, 80
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    glow = 1.0 - 0.35 * (yy / h)
    rgb = np.stack([glow, glow * 0.78, glow * 0.45], -1)
    bars = (np.abs(xx - w / 2) < 3) | (np.abs(yy - h * 0.45) < 3)
    rgb[bars] *= 0.15
    save_rgba(OUT / "env" / "st_window.png", rgb, np.ones((h, w)))


def st_street():
    """Wet cobbles, tileable sideways: dark stone, bright wet edges."""
    w, h = 512, 160
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    row = (yy // 20).astype(int)
    off = (row % 2) * 16
    cx = ((xx + off) % 32) - 16
    cy = (yy % 20) - 10
    stone = 1.0 - smoothstep(10.0, 14.0, np.maximum(np.abs(cx) * 0.9, np.abs(cy) * 1.4))
    jitter = _sea_noise(256, 32, 2, ST_RNG)[yy.astype(int) % 256, xx.astype(int) % 256]
    v = 0.18 + 0.22 * stone * (0.7 + 0.6 * jitter)
    # Wet: a sheen along each stone's top edge.
    sheen = smoothstep(-10.0, -6.0, cy) * (1 - smoothstep(-6.0, -3.0, cy)) * stone
    v = v + 0.25 * sheen
    rgb = np.stack([v * 0.9, v * 0.95, v * 1.1], -1)
    # Fades into the dark with distance (up the texture).
    alpha = np.ones((h, w))
    save_rgba(OUT / "env" / "st_street.png", rgb, alpha)


def st_puddle():
    """A puddle seen low: a flat ellipse, bright at its far rim."""
    w, h = 256, 48
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    u, v = (xx - w / 2) / (w / 2), (yy - h / 2) / (h / 2)
    r = np.sqrt(u * u + v * v)
    alpha = smoothstep(1.0, 0.85, r)
    rim = smoothstep(0.6, 0.95, r) * (v < 0)
    val = 0.55 + 0.45 * rim
    save_rgba(OUT / "env" / "st_puddle.png", np.stack([val * 0.85, val * 0.92, val], -1), alpha)


def st_lamp():
    """A street lamp: post, arm and lantern, white in alpha."""
    w, h = 90, 360
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    post = (np.abs(xx - 20) < 4 + 3 * (yy / h) ** 3) & (yy > 40)
    base = (np.abs(xx - 20) < 10) & (yy > h - 20)
    arm = (np.abs(yy - 44) < 3) & (xx > 20) & (xx < 64)
    hang = (np.abs(xx - 62) < 2) & (yy > 44) & (yy < 56)
    cage = (np.abs(xx - 62) < 13) & (yy > 56) & (yy < 92) & ((np.abs(xx - 62) > 10) | (yy < 60) | (yy > 88))
    cap = (np.abs(xx - 62) < 16 - (58 - yy) * 0.9) & (yy > 48) & (yy <= 58)
    alpha = (post | base | arm | hang | cage | cap).astype(np.float64)
    save_rgba(OUT / "env" / "st_lamp.png", 1.0, alpha)


def st_bolt():
    """A lightning bolt: a jagged main channel with forks, a white core in
    a soft glow - drawn additive and bright."""
    import math
    w, h = 320, 720
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    core = np.zeros((h, w))

    def channel(x0, y0, x1, y1, width, depth):
        pts = [(x0, y0), (x1, y1)]
        disp = abs(x1 - x0) * 0.2 + abs(y1 - y0) * 0.18
        for _ in range(6):
            nxt = [pts[0]]
            for a, b in zip(pts, pts[1:]):
                mx, my = (a[0] + b[0]) / 2, (a[1] + b[1]) / 2
                nxt += [(mx + ST_RNG.uniform(-disp, disp), my), b]
            pts, disp = nxt, disp * 0.55
        for a, b in zip(pts, pts[1:]):
            n = max(2, int(math.hypot(b[0] - a[0], b[1] - a[1]) / 2))
            for t in np.linspace(0, 1, n):
                px, py = a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t
                y0i, y1i = max(0, int(py) - 10), min(h, int(py) + 11)
                x0i, x1i = max(0, int(px) - 10), min(w, int(px) + 11)
                d = np.hypot(xx[y0i:y1i, x0i:x1i] - px, yy[y0i:y1i, x0i:x1i] - py)
                core[y0i:y1i, x0i:x1i] = np.maximum(core[y0i:y1i, x0i:x1i], smoothstep(width + 1.5, width * 0.3, d))
        if depth > 0:
            for _ in range(2):
                i = ST_RNG.integers(len(pts) // 4, len(pts) * 3 // 4)
                fx, fy = pts[i]
                channel(fx, fy, fx + ST_RNG.uniform(-110, 110), fy + ST_RNG.uniform(90, 220), width * 0.55, depth - 1)
    channel(w * 0.5, 0, w * 0.45, h - 10, 2.6, 2)
    # A glow around the channel: blurred by box filters.
    glow = core.copy()
    for _ in range(3):
        k = 9
        pad = np.pad(glow, k, mode="edge")
        cs = np.cumsum(np.cumsum(pad, 0), 1)
        glow = (cs[2 * k:, 2 * k:] - cs[:-2 * k, 2 * k:] - cs[2 * k:, :-2 * k] + cs[:-2 * k, :-2 * k])[:h, :w] / (2 * k) ** 2
    alpha = np.clip(core + glow * 1.6, 0, 1)
    rgb = np.stack([0.85 + 0.15 * core, 0.9 + 0.1 * core, np.ones_like(core)], -1)
    save_rgba(OUT / "fx" / "st_bolt.png", rgb, alpha)


def st_drop():
    """A raindrop: a thin soft streak, drawn stretched along its fall."""
    w, h = 8, 32
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    u, v = (xx - w / 2) / (w / 2), (yy - h / 2) / (h / 2)
    a = smoothstep(1.0, 0.2, np.abs(u)) * smoothstep(1.0, 0.3, np.abs(v))
    save_rgba(OUT / "fx" / "st_drop.png", 1.0, a)


# ── 21 The Creature ──────────────────────────────────────────────────────
#
# A big cat's skin - one texture, three bands: the body (back to belly, the
# long way nose to tail), near legs, far legs - and a golden afternoon behind
# it. `tools/creature.py` lays the skin's UVs over these bands.

CR_RNG = np.random.default_rng(2121)


def cr_skin():
    w, h = 512, 256
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    u = xx / (w - 1)
    rgb = np.zeros((h, w, 3))
    back = np.array([0.50, 0.29, 0.12])
    flank = np.array([0.82, 0.52, 0.24])
    belly = np.array([0.95, 0.88, 0.72])
    # Body: rows 0..127, v from the back (0) to the belly (0.5).
    v = np.clip(yy / 128.0, 0, 1)[..., None]
    a = smoothstep(0.0, 0.45, v)
    b = smoothstep(0.62, 0.95, v)
    body = back * (1 - a) + flank * a
    body = body * (1 - b) + belly * b
    # Stripes down the flanks, torso only, wobbling and tapering.
    noise = _sea_noise(256, 16, 3, CR_RNG)[(yy.astype(int)) % 256, (xx.astype(int) // 2) % 256]
    stripe = np.sin(u * 150 + noise * 4.0)
    torso = smoothstep(0.30, 0.36, u) * (1 - smoothstep(0.66, 0.72, u))
    reach = 0.25 + 0.45 * _sea_noise(256, 24, 2, CR_RNG)[0, (xx.astype(int)) % 256]
    stripes = smoothstep(0.55, 0.8, stripe) * torso * (v[..., 0] < reach)
    # Rings on the tail, and its dark tip.
    rings = smoothstep(0.6, 0.85, np.sin(u * 130)) * (u < 0.28) * (u > 0.04)
    tip = (u < 0.045).astype(np.float64)
    dark = np.clip(np.maximum(np.maximum(stripes, rings * 0.9), tip), 0, 1)[..., None]
    body = body * (1 - 0.8 * dark) + np.array([0.12, 0.07, 0.04]) * 0.8 * dark
    # A pale muzzle under the jaw.
    muzzle = (smoothstep(0.9, 0.95, u) * smoothstep(0.55, 0.8, v[..., 0]))[..., None]
    body = body * (1 - muzzle) + belly * muzzle
    rgb[:128] = body[:128]
    # Legs: u from hip (0) to toe (1), v across; near legs rows 128..191.
    lu = u[128:192]
    leg = flank * (1 - smoothstep(0.55, 0.95, lu))[..., None] + belly * smoothstep(0.55, 0.95, lu)[..., None]
    edge = np.abs((yy[128:192] - 160) / 32.0)[..., None]
    leg = leg * (1 - 0.12 * edge)
    rgb[128:192] = leg
    rgb[192:256] = leg * 0.55
    # A leg's top fades into the body it overlaps, so the thigh has no seam.
    alpha = np.ones((h, w))
    alpha[128:] = smoothstep(0.0, 0.22, u[128:])
    save_rgba(OUT / "env" / "cr_skin.png", rgb, alpha)


def cr_ear():
    """A cat's ear: a rounded triangle, a dark tip."""
    s = 64
    yy, xx = np.mgrid[0:s, 0:s].astype(np.float64) + 0.5
    y = 1 - yy / s                      # 0 at the bottom (the anchor)
    half = 0.5 * (1 - y) ** 0.8
    inside = np.abs(xx / s - 0.5) < half
    alpha = inside.astype(np.float64) * smoothstep(0.0, 0.05, y)
    col = np.array([0.72, 0.44, 0.20]) * (1 - smoothstep(0.55, 0.8, y))[..., None] + \
        np.array([0.12, 0.07, 0.04]) * smoothstep(0.55, 0.8, y)[..., None]
    save_rgba(OUT / "env" / "cr_ear.png", col, alpha)


def cr_sky():
    """A golden afternoon: blue overhead, warm at the horizon."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.38, 0.58, 0.82])
    low = np.array([1.0, 0.86, 0.60])
    a = smoothstep(0.0, 1.0, t)[..., None] ** 1.4
    save_rgba(OUT / "env" / "cr_sky.png", top * (1 - a) + low * a, np.ones((h, w)))


def _cr_ridge(name, w, h, amp, base, cells, rgb, haze):
    """A tileable line of hills, white in alpha, flat colour."""
    xx = np.arange(w)
    row = _sea_noise(256, cells, 4, CR_RNG)[64]
    # Sampled between the noise's texels, so the skyline is smooth.
    n = np.interp(xx * 256 / w, np.arange(257), np.append(row, row[0]))
    n = (n - n.min()) / max(n.max() - n.min(), 1e-6)
    top = h * (1 - base - amp * n)
    yy = np.arange(h)[:, None]
    alpha = smoothstep(-1.0, 1.0, yy - top[None, :])
    col = np.array(rgb)[None, None, :] * (1 - haze * (1 - yy / h))[..., None]
    save_rgba(OUT / "env" / name, col * np.ones((h, w, 1)), alpha)


def cr_hills():
    # Tall, solid below the skyline, so a dip in the ground never shows sky.
    _cr_ridge("cr_hills_far.png", 1024, 600, 0.22, 0.62, 6, (0.55, 0.62, 0.72), 0.2)
    _cr_ridge("cr_hills_mid.png", 1024, 600, 0.16, 0.68, 9, (0.40, 0.48, 0.30), 0.1)


# ── 22 Menus & HUD ────────────────────────────────────────────────────────
#
# Eight item icons in one strip - sword, shield, two potions, a gem, a bow, a
# scroll and a crown - painted with PIL at four times size and scaled down.


def mh_icons():
    from PIL import Image, ImageDraw
    S, N = 64, 8
    big = Image.new("RGBA", (S * N * 4, S * 4), (0, 0, 0, 0))
    d = ImageDraw.Draw(big)

    def c(*rgb, a=255):
        return tuple(int(v * 255) for v in rgb) + (a,)

    def at(i, pts):
        return [(i * S * 4 + x * 4, y * 4) for x, y in pts]

    outline = c(0.08, 0.06, 0.05)
    # 0 sword: a blade up the diagonal, a guard, a grip.
    d.polygon(at(0, [(14, 50), (46, 10), (54, 10), (54, 18), (22, 54)]), fill=c(0.85, 0.88, 0.92), outline=outline, width=8)
    d.line(at(0, [(18, 38), (30, 50)]), fill=c(0.85, 0.65, 0.25), width=24)
    d.line(at(0, [(10, 58), (20, 48)]), fill=c(0.45, 0.28, 0.15), width=20)
    # 1 shield: a kite, a rim, a boss.
    d.polygon(at(1, [(12, 10), (52, 10), (52, 30), (32, 58), (12, 30)]), fill=c(0.20, 0.38, 0.75), outline=c(0.95, 0.78, 0.30), width=14)
    d.ellipse(at(1, [(26, 22), (38, 34)]), fill=c(0.95, 0.78, 0.30))
    # 2, 3 potions: a round flask, a neck, a cork, a gleam.
    for i, liquid in ((2, c(0.90, 0.20, 0.25)), (3, c(0.25, 0.45, 0.95))):
        d.ellipse(at(i, [(12, 22), (52, 60)]), fill=c(0.85, 0.90, 0.95, a=150), outline=outline, width=8)
        d.pieslice(at(i, [(14, 24), (50, 58)]), 0, 180, fill=liquid)
        d.rectangle(at(i, [(26, 8), (38, 24)]), fill=c(0.85, 0.90, 0.95, a=170), outline=outline, width=6)
        d.rectangle(at(i, [(25, 4), (39, 11)]), fill=c(0.55, 0.38, 0.20))
        d.ellipse(at(i, [(19, 30), (25, 38)]), fill=c(1, 1, 1, a=200))
    # 4 gem: a cut stone with facets.
    d.polygon(at(4, [(32, 6), (54, 26), (32, 58), (10, 26)]), fill=c(0.20, 0.80, 0.50), outline=outline, width=8)
    d.polygon(at(4, [(32, 6), (42, 26), (22, 26)]), fill=c(0.55, 0.95, 0.75))
    d.line(at(4, [(10, 26), (54, 26)]), fill=c(0.10, 0.45, 0.30), width=6)
    # 5 bow: an arc and a string.
    d.arc(at(5, [(10, 6), (50, 58)]), -80, 80, fill=c(0.60, 0.38, 0.20), width=22)
    d.line(at(5, [(34, 8), (34, 56)]), fill=c(0.92, 0.90, 0.85), width=5)
    # 6 scroll: a sheet between two rolls, a few lines.
    d.rectangle(at(6, [(16, 14), (48, 50)]), fill=c(0.95, 0.88, 0.70), outline=outline, width=6)
    for y in (22, 30, 38):
        d.line(at(6, [(21, y), (43, y)]), fill=c(0.55, 0.45, 0.35), width=5)
    for y in (10, 50):
        d.rounded_rectangle(at(6, [(10, y - 4), (54, y + 4)]), radius=12, fill=c(0.80, 0.68, 0.48), outline=outline, width=6)
    # 7 crown (and coin): gold points, gems.
    d.polygon(at(7, [(8, 48), (8, 20), (20, 34), (32, 14), (44, 34), (56, 20), (56, 48)]), fill=c(1.0, 0.80, 0.25), outline=outline, width=8)
    for x, col in ((20, c(0.9, 0.2, 0.25)), (32, c(0.2, 0.45, 0.95)), (44, c(0.2, 0.8, 0.5))):
        d.ellipse(at(7, [(x - 4, 38), (x + 4, 46)]), fill=col)
    img = big.resize((S * N, S), Image.LANCZOS)
    img.save(OUT / "env" / "mh_icons.png")
    print(f"wrote {OUT / 'env' / 'mh_icons.png'}  {S * N}x{S}")


# ── 23 Tilt-Shift Diorama ─────────────────────────────────────────────────
#
# A small town from above: the ground painted whole from `tools/diorama.py`'s
# layout (grass, fields, the river, roads and their bridges, a park), and
# roofs, trees, cars and a boat as sprites that stand on it.

TD_RNG = np.random.default_rng(2323)


def td_ground():
    import diorama as td
    from PIL import Image, ImageDraw, ImageFilter
    W_, H_ = td.GROUND_W, td.GROUND_H
    yy, xx = np.mgrid[0:H_, 0:W_].astype(np.float64)
    wx, wy = xx - W_ / 2, H_ / 2 - yy                          # world coordinates
    n1 = _sea_noise(256, 12, 4, TD_RNG)[(yy // 4).astype(int) % 256, (xx // 4).astype(int) % 256]
    n2 = _sea_noise(256, 48, 2, TD_RNG)[yy.astype(int) % 256, xx.astype(int) % 256]
    grass = np.array([0.36, 0.56, 0.22])
    rgb = grass * (0.85 + 0.25 * n1[..., None] + 0.08 * n2[..., None])
    # Mown stripes in the park and the lawns.
    rgb *= (1 + 0.03 * np.sign(np.sin(wx / 14.0)))[..., None]
    # Fields west of the river: crop rows in patches of colour.
    rx = np.vectorize(td.river_x)(wy[:, :1])[:, 0][:, None] * np.ones((1, W_))
    west = wx < rx - td.RIVER_W / 2 - 60
    patch = ((np.floor((wy + 600) / 170) + np.floor((wx + 900) / 150)) % 3).astype(int)
    crops = np.array([[0.62, 0.58, 0.26], [0.42, 0.52, 0.20], [0.55, 0.42, 0.24]])[patch]
    rows = 0.88 + 0.12 * (np.sin(wy / 5.0 + patch * 1.3) > 0)
    rgb = np.where(west[..., None], crops * rows[..., None] * (0.92 + 0.12 * n1[..., None]), rgb)
    # The river: banks of sand, water darker in the middle.
    d = np.abs(wx - rx)
    bank = d < td.RIVER_W / 2 + 10
    water = d < td.RIVER_W / 2
    rgb = np.where(bank[..., None], np.array([0.72, 0.64, 0.46]), rgb)
    depth = np.clip(1 - d / (td.RIVER_W / 2), 0, 1)[..., None]
    ripple = (0.04 * np.sin(wy / 9.0 + np.sin(wx / 13.0) * 2) + 0.03 * n2)[..., None]
    blue = np.array([0.30, 0.52, 0.68]) * (1 - 0.35 * depth) + ripple
    rgb = np.where(water[..., None], blue, rgb)
    # The park: paler grass, a sand path through it, a pond.
    px0, py0, px1, py1 = td.PARK
    park = (wx > px0) & (wx < px1) & (wy > py0) & (wy < py1)
    rgb = np.where(park[..., None], rgb * np.array([1.06, 1.08, 1.0]), rgb)
    path = park & (np.abs(wy - 18 - 12 * np.sin(wx / 60.0)) < 7)
    rgb = np.where(path[..., None], np.array([0.80, 0.72, 0.52]), rgb)
    cx, cy, ax, ay = td.POND
    e = ((wx - cx) / ax) ** 2 + ((wy - cy) / ay) ** 2
    rgb = np.where((e < 1.25)[..., None], np.array([0.62, 0.58, 0.44]), rgb)
    rgb = np.where((e < 1.0)[..., None], np.array([0.28, 0.50, 0.66]) * (0.8 + 0.2 * e)[..., None], rgb)
    img = Image.fromarray((np.clip(rgb, 0, 1) * 255).astype(np.uint8), "RGB")
    dr = ImageDraw.Draw(img)

    def px(x, y):
        return (x + W_ / 2, H_ / 2 - y)
    asphalt, walk, paint = (80, 82, 88), (176, 172, 162), (236, 232, 214)
    half = td.ROAD_W / 2
    # Roads: sidewalk, asphalt, dashes; the horizontals run over the river
    # on bridges.
    for y in td.ROADS_Y:
        dr.rectangle([px(-W_ / 2, y + half + 6), px(W_ / 2, y - half - 6)], fill=walk)
        dr.rectangle([px(-W_ / 2, y + half), px(W_ / 2, y - half)], fill=asphalt)
    for x in td.ROADS_X:
        dr.rectangle([px(x - half - 6, H_ / 2), px(x + half + 6, -H_ / 2)], fill=walk)
        dr.rectangle([px(x - half, H_ / 2), px(x + half, -H_ / 2)], fill=asphalt)
    for y in td.ROADS_Y:
        dr.rectangle([px(-W_ / 2, y + half), px(W_ / 2, y - half)], fill=asphalt)
        rxc = td.river_x(y)
        dr.rectangle([px(rxc - td.RIVER_W / 2 - 24, y + half + 10), px(rxc + td.RIVER_W / 2 + 24, y - half - 10)],
                     fill=(150, 128, 104))
        dr.rectangle([px(rxc - td.RIVER_W / 2 - 24, y + half + 4), px(rxc + td.RIVER_W / 2 + 24, y - half - 4)],
                     fill=(96, 92, 90))
        x = -W_ / 2
        while x < W_ / 2:
            if all(abs(x - rx_) > half + 14 for rx_ in td.ROADS_X):
                dr.rectangle([px(x, y + 1.5), px(x + 14, y - 1.5)], fill=paint)
            x += 30
    for x in td.ROADS_X:
        y = -H_ / 2
        while y < H_ / 2:
            if all(abs(y - ry) > half + 14 for ry in td.ROADS_Y):
                dr.rectangle([px(x - 1.5, y + 14), px(x + 1.5, y)], fill=paint)
            y += 30
    img = img.filter(ImageFilter.GaussianBlur(0.6))
    img.save(OUT / "env" / "td_ground.png")
    print(f"wrote {OUT / 'env' / 'td_ground.png'}  {W_}x{H_}")


def td_sprites():
    """Roofs (six styles), trees (three), cars (five colours), a boat and a
    soft block for shadows - all top-down, drawn at 4x and scaled."""
    from PIL import Image, ImageDraw

    def save(img, name, size):
        img.resize(size, Image.LANCZOS).save(OUT / "env" / name)

    roofs = [((0.72, 0.30, 0.20), (0.52, 0.20, 0.14)), ((0.40, 0.46, 0.56), (0.28, 0.32, 0.40)),
             ((0.35, 0.33, 0.33), (0.24, 0.22, 0.22)), ((0.46, 0.52, 0.30), (0.32, 0.37, 0.20)),
             ((0.58, 0.40, 0.26), (0.42, 0.28, 0.18)), ((0.80, 0.52, 0.30), (0.60, 0.36, 0.20))]
    for i, (lit, dark) in enumerate(roofs):
        w, h = 96 * 4, 72 * 4
        img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        c = lambda t: tuple(int(v * 255) for v in t) + (255,)
        # A gable, ridge along the width: the top half catches the light.
        d.rectangle([8, 8, w - 8, h // 2], fill=c(lit))
        d.rectangle([8, h // 2, w - 8, h - 8], fill=c(dark))
        for yy in range(16, h - 8, 18):
            d.line([(8, yy), (w - 8, yy)], fill=c(tuple(v * 0.85 for v in (lit if yy < h // 2 else dark))), width=3)
        d.line([(8, h // 2), (w - 8, h // 2)], fill=c(tuple(min(1, v * 1.25) for v in lit)), width=8)
        d.rectangle([8, 8, w - 8, h - 8], outline=c((0.12, 0.10, 0.09)), width=6)
        if i % 2 == 0:
            d.rectangle([w * 0.7, h * 0.18, w * 0.7 + 40, h * 0.18 + 40], fill=c((0.45, 0.40, 0.38)),
                        outline=c((0.15, 0.12, 0.1)), width=5)
        save(img, f"td_roof_{i}.png", (96, 72))
    greens = [(0.24, 0.46, 0.18), (0.30, 0.52, 0.20), (0.20, 0.38, 0.22)]
    for i, g in enumerate(greens):
        S = 64 * 4
        img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        c = lambda t, a=255: tuple(int(max(0, min(1, v)) * 255) for v in t) + (a,)
        rng = np.random.default_rng(100 + i)
        d.ellipse([12, 12, S - 12, S - 12], fill=c(tuple(v * 0.8 for v in g)))
        for _ in range(9):
            r = rng.uniform(30, 55)
            x, y = rng.uniform(60, S - 60), rng.uniform(60, S - 60)
            d.ellipse([x - r, y - r, x + r, y + r], fill=c(g))
        for _ in range(7):
            r = rng.uniform(14, 26)
            x, y = rng.uniform(70, S - 90), rng.uniform(70, S - 90)
            d.ellipse([x - r, y - r, x + r, y + r], fill=c(tuple(v * 1.25 for v in g)))
        save(img, f"td_tree_{i}.png", (64, 64))
    cars = [(0.85, 0.20, 0.18), (0.20, 0.45, 0.85), (0.95, 0.80, 0.25), (0.92, 0.92, 0.90), (0.25, 0.60, 0.40)]
    for i, col in enumerate(cars):
        w, h = 32 * 4, 16 * 4
        img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        c = lambda t: tuple(int(max(0, min(1, v)) * 255) for v in t) + (255,)
        d.rounded_rectangle([4, 4, w - 4, h - 4], radius=18, fill=c(col), outline=c((0.1, 0.1, 0.1)), width=4)
        d.rounded_rectangle([w * 0.52, 10, w * 0.72, h - 10], radius=8, fill=c((0.15, 0.2, 0.28)))   # windscreen
        d.rounded_rectangle([w * 0.22, 12, w * 0.5, h - 12], radius=8, fill=c(tuple(v * 1.12 for v in col)))
        d.rectangle([w * 0.14, 12, w * 0.2, h - 12], fill=c((0.15, 0.2, 0.28)))
        save(img, f"td_car_{i}.png", (32, 16))
    w, h = 40 * 4, 18 * 4
    img = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    d.polygon([(8, 8), (w * 0.7, 8), (w - 6, h / 2), (w * 0.7, h - 8), (8, h - 8)], fill=(235, 230, 220, 255),
              outline=(40, 35, 30, 255))
    d.rectangle([w * 0.25, 22, w * 0.55, h - 22], fill=(150, 90, 60, 255))
    save(img, "td_boat.png", (40, 18))
    # A soft-edged block: shadows under houses.
    S = 32
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    u, v = np.abs(xx / S - 0.5) * 2, np.abs(yy / S - 0.5) * 2
    a = smoothstep(1.0, 0.7, np.maximum(u, v))
    save_rgba(OUT / "env" / "td_block.png", 1.0, a)


# ── 24 Collapse ───────────────────────────────────────────────────────────
#
# A courtyard at night: a wall of dark stone behind (it takes the shadows),
# a tower of pale blocks, a boulder, braziers. Blocks and wall carry normal
# maps so the fire rakes across them.

CL_RNG = np.random.default_rng(2424)


def cl_wall():
    """Coursed stone, tileable: dark blocks, deep mortar."""
    S = 512
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
    course = 64
    row = (yy // course).astype(int)
    off = (row % 2) * 64
    bx = ((xx + off) % 128) - 64
    by = (yy % course) - course / 2
    face = 1 - smoothstep(26.0, 31.0, np.maximum(np.abs(bx) * 0.5, np.abs(by)))
    n1 = _sea_noise(256, 16, 4, CL_RNG)[(yy.astype(int)) % 256, (xx.astype(int)) % 256]
    n2 = _sea_noise(256, 48, 2, CL_RNG)[(yy.astype(int) * 2) % 256, (xx.astype(int) * 2) % 256]
    tone = 0.85 + 0.3 * _sea_noise(256, 4, 1, CL_RNG)[(row * 37) % 256, (((xx + off) // 128).astype(int) * 53) % 256]
    height = face * (0.8 + 0.2 * n1) + 0.08 * n2
    v = (0.22 + 0.18 * n1 + 0.06 * n2) * tone * (0.35 + 0.65 * face)
    rgb = np.stack([v * 1.0, v * 0.95, v * 0.9], -1)
    save_rgba(OUT / "env" / "cl_wall.png", rgb, np.ones((S, S)))
    save_rgba(OUT / "env" / "cl_wall_n.png", normals_from_height(height, 3.0), np.ones((S, S)))


def cl_block():
    """One dressed stone block, bevelled, chipped: pale sandstone."""
    w, h = 128, 64
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    u, v = np.abs(xx + 0.5 - w / 2) / (w / 2), np.abs(yy + 0.5 - h / 2) / (h / 2)
    edge = np.maximum(u * w / (w - 10), v * h / (h - 10))
    bevel = 1 - smoothstep(0.82, 1.0, edge)
    n1 = _sea_noise(256, 20, 4, CL_RNG)[(yy.astype(int) * 2) % 256, (xx.astype(int) * 2) % 256]
    height = bevel * (0.85 + 0.15 * n1)
    tone = 0.62 + 0.25 * n1
    rgb = np.stack([tone * 1.0, tone * 0.88, tone * 0.70], -1) * (0.55 + 0.45 * bevel)[..., None]
    save_rgba(OUT / "env" / "cl_block.png", rgb, np.ones((h, w)))
    save_rgba(OUT / "env" / "cl_block_n.png", normals_from_height(height, 4.0, wrap=False), np.ones((h, w)))


def cl_boulder():
    """A round boulder: grey, pitted, with a sphere's normals."""
    S = 96
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    x, y = (xx - S / 2) / (S / 2), (yy - S / 2) / (S / 2)
    r = np.sqrt(x * x + y * y)
    alpha = smoothstep(1.0, 0.94, r)
    n1 = _sea_noise(256, 12, 4, CL_RNG)[(yy.astype(int) * 2) % 256, (xx.astype(int) * 2) % 256]
    v = 0.45 + 0.25 * n1
    save_rgba(OUT / "env" / "cl_boulder.png", np.stack([v, v * 0.97, v * 0.93], -1), alpha)
    z = np.sqrt(np.clip(1 - r * r, 0, 1))
    base = np.stack([x, -y, np.maximum(z, 0.05)], -1)
    save_rgba(OUT / "env" / "cl_boulder_n.png", normals_from_height(n1, 2.0, wrap=False, base=base), alpha)


def cl_brazier():
    """An iron bowl on a stand, white in alpha (tinted dark)."""
    w, h = 64, 96
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    x = xx - w / 2
    bowl = (yy < 34) & (yy > 14) & (np.abs(x) < 28 - (yy - 14) * 0.5)
    rim = (yy > 12) & (yy < 17) & (np.abs(x) < 30)
    stem = (np.abs(x) < 4) & (yy >= 34) & (yy < 86)
    foot = (yy >= 84) & (np.abs(x) < 20 - (yy - 84) * 0.2)
    alpha = (bowl | rim | stem | foot).astype(np.float64)
    save_rgba(OUT / "env" / "cl_brazier.png", 1.0, alpha)


# ── 25 Cloth & Rope ───────────────────────────────────────────────────────
#
# A gorge in the mountains late in the day: a rope bridge across it, flags
# and banners at either end. The cloth and the rope are textures laid over
# meshes the engine bends every frame, so each is drawn flat and square.

CB_RNG = np.random.default_rng(2525)


def cb_sky():
    """Late afternoon: a deep blue overhead going to peach at the horizon."""
    w, h = 8, 512
    t = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    top = np.array([0.22, 0.34, 0.62])
    mid = np.array([0.62, 0.62, 0.74])
    low = np.array([1.0, 0.78, 0.56])
    a = smoothstep(0.0, 0.7, t)[..., None]
    b = smoothstep(0.55, 1.0, t)[..., None]
    rgb = top * (1 - a) + mid * a
    rgb = rgb * (1 - b) + low * b
    save_rgba(OUT / "env" / "cb_sky.png", rgb, np.ones((h, w)))


def _cb_peaks(name, w, h, amp, base, rgb, haze, seed_row):
    """Jagged tileable peaks: noise plus a sharpened ridge, white-capped at
    the tips when `rgb` is light."""
    xx = np.arange(w)
    n = np.zeros(w)
    for cells, k in ((5, 1.0), (11, 0.45), (29, 0.18)):
        row = _sea_noise(256, cells, 1, CB_RNG)[seed_row]
        n += k * np.interp(xx * 256 / w, np.arange(257), np.append(row, row[0]))
    n = (n - n.min()) / max(n.max() - n.min(), 1e-6)
    n = n ** 1.6   # sharper peaks, wider valleys
    top = h * (1 - base - amp * n)
    yy = np.arange(h)[:, None]
    alpha = smoothstep(-1.0, 1.0, yy - top[None, :])
    depth = np.clip((yy - top[None, :]) / (h * 0.5), 0, 1)
    col = np.array(rgb)[None, None, :] * (1 - haze * depth)[..., None]
    snow = smoothstep(18.0, 0.0, yy - top[None, :]) * smoothstep(0.55, 0.85, n)[None, :]
    col = col * (1 - snow[..., None] * 0.6) + snow[..., None] * np.array([0.92, 0.9, 0.95]) * 0.6
    save_rgba(OUT / "env" / name, col, alpha)


def cb_peaks():
    _cb_peaks("cb_peaks_far.png", 1024, 520, 0.55, 0.3, (0.56, 0.58, 0.74), 0.15, 40)
    _cb_peaks("cb_peaks_near.png", 1024, 520, 0.42, 0.34, (0.30, 0.30, 0.42), 0.35, 150)


def cb_cliff():
    """Layered rock, tileable: strata across, cracks down, with a normal map."""
    S = 512
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
    n1 = _sea_noise(256, 8, 4, CB_RNG)[(yy.astype(int) // 2) % 256, (xx.astype(int) // 2) % 256]
    n2 = _sea_noise(256, 32, 3, CB_RNG)[yy.astype(int) % 256, xx.astype(int) % 256]
    warp = yy + 22 * n1
    strata = 0.5 + 0.5 * np.sin(warp / S * 2 * np.pi * 9)
    ledge = smoothstep(0.75, 0.95, strata)
    height = 0.55 * n1 + 0.25 * n2 + 0.35 * ledge
    tone = 0.36 + 0.22 * n1 + 0.1 * n2 + 0.08 * strata
    rgb = np.stack([tone * 1.0, tone * 0.88, tone * 0.76], -1)
    save_rgba(OUT / "env" / "cb_cliff.png", rgb, np.ones((S, S)))
    save_rgba(OUT / "env" / "cb_cliff_n.png", normals_from_height(height, 5.0), np.ones((S, S)))


def cb_grass():
    """A fringe of grass along a cliff top, tileable across; white in alpha
    below the blades so it laps over the rock's edge."""
    w, h = 512, 64
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    alpha = np.zeros((h, w))
    rgb = np.zeros((h, w, 3))
    base = h * 0.62
    for _ in range(420):
        x0 = CB_RNG.uniform(0, w)
        tall = CB_RNG.uniform(10, 34)
        lean = CB_RNG.uniform(-8, 8)
        t = np.clip((base - yy) / tall, 0, 1)
        cx = x0 + lean * t ** 2
        dx = np.minimum(np.abs(xx - cx), w - np.abs(xx - cx))
        blade = (dx < (1 - t) * 2.2 + 0.4) & (yy <= base) & (yy >= base - tall)
        shade = 0.55 + 0.45 * t
        g = np.array([0.36, 0.52, 0.22]) * CB_RNG.uniform(0.8, 1.15)
        rgb[blade] = g * shade[blade][:, None]
        alpha[blade] = 1.0
    turf = yy > base - 3
    rgb[turf] = np.array([0.24, 0.3, 0.14])
    alpha[turf] = 1.0
    save_rgba(OUT / "env" / "cb_grass.png", rgb, alpha)


def _cb_wood(w, h, along_x, dark):
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    a, b = (xx, yy) if along_x else (yy, xx)
    n = _sea_noise(256, 12, 3, CB_RNG)[(b.astype(int) * 3) % 256, (a.astype(int) // 3) % 256]
    grain = 0.5 + 0.5 * np.sin(b * 1.3 + n * 7.0)
    tone = (0.42 + 0.14 * grain + 0.1 * n) * dark
    return np.stack([tone * 1.0, tone * 0.72, tone * 0.48], -1), grain * 0.3 + n * 0.4


def cb_plank():
    w, h = 96, 24
    rgb, height = _cb_wood(w, h, True, 1.0)
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    edge = np.maximum(np.abs(xx + 0.5 - w / 2) / (w / 2), np.abs(yy + 0.5 - h / 2) / (h / 2))
    bevel = 1 - smoothstep(0.8, 1.0, edge)
    for nx in (10, w - 10):   # a nail at each end, where the ropes tie
        d = np.hypot(xx - nx, yy - h / 2)
        rgb[d < 2.2] = [0.2, 0.2, 0.22]
    rgb = rgb * (0.6 + 0.4 * bevel)[..., None]
    save_rgba(OUT / "env" / "cb_plank.png", rgb, np.ones((h, w)))
    save_rgba(OUT / "env" / "cb_plank_n.png", normals_from_height(height * bevel + bevel, 3.0, wrap=False),
              np.ones((h, w)))


def cb_post():
    w, h = 32, 256
    rgb, height = _cb_wood(w, h, False, 0.85)
    xx = np.arange(w)[None, :] + 0.5
    u = (xx - w / 2) / (w / 2)
    rgb = rgb * (0.55 + 0.45 * np.sqrt(np.clip(1 - u * u, 0, 1)))[..., None]
    save_rgba(OUT / "env" / "cb_post.png", rgb, np.ones((h, w)))
    base = np.stack([u * np.ones((h, 1)), np.zeros((h, w)), np.sqrt(np.clip(1 - u * u, 0.05, 1)) * np.ones((h, 1))], -1)
    save_rgba(OUT / "env" / "cb_post_n.png", normals_from_height(height, 1.5, wrap=False, base=base), np.ones((h, w)))


def cb_flag():
    """A long pennant: red, a gold sun at the hoist, a swallowtail cut in
    the fly. Laid over the cloth grid: u runs hoist to fly, v top to bottom."""
    w, h = 256, 128
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    u, v = xx / w, yy / h
    cut = u > 0.8 + 0.2 * np.abs(v - 0.5) * 2   # the swallowtail's notch
    alpha = (~cut).astype(np.float64)
    weave = 0.94 + 0.06 * _sea_noise(256, 64, 2, CB_RNG)[(yy.astype(int)) % 256, (xx.astype(int)) % 256]
    rgb = np.stack([0.78 * weave, 0.14 * weave, 0.12 * weave], -1)
    band = u < 0.1
    rgb[band] = np.array([0.95, 0.9, 0.78]) * weave[band][:, None]
    d = np.hypot(xx - 70, yy - 64)
    rays = 0.5 + 0.5 * np.cos(np.arctan2(yy - 64, xx - 70) * 12)
    sun = (d < 18) | ((d < 30) & (rays > 0.7))
    rgb[sun] = np.array([0.98, 0.76, 0.26])
    stripe = (np.abs(v - 0.14) < 0.025) | (np.abs(v - 0.86) < 0.025)
    rgb[stripe & ~band] = np.array([0.98, 0.76, 0.26])
    save_rgba(OUT / "env" / "cb_flag.png", rgb, alpha)


def cb_banner():
    """A tall hanging banner: deep blue, a gold border and a mountain, a
    point at the bottom with a fringe."""
    w, h = 128, 320
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    u, v = xx / w, yy / h
    point = v > 0.84 + 0.16 * (1 - np.abs(u - 0.5) * 2)
    alpha = (~point).astype(np.float64)
    fringe = point & (v < 0.84 + 0.16 * (1 - np.abs(u - 0.5) * 2) + 0.04) & ((xx.astype(int) // 3) % 2 == 0)
    alpha[fringe] = 1.0
    weave = 0.93 + 0.07 * _sea_noise(256, 64, 2, CB_RNG)[(yy.astype(int)) % 256, (xx.astype(int)) % 256]
    rgb = np.stack([0.1 * weave, 0.16 * weave, 0.42 * weave], -1)
    border = (u < 0.07) | (u > 0.93) | (v < 0.05)
    edge_v = 0.84 + 0.16 * (1 - np.abs(u - 0.5) * 2)
    border |= (v > edge_v - 0.05) & ~point
    gold = np.array([0.95, 0.74, 0.28])
    rgb[border] = gold
    rgb[fringe] = gold * 0.9
    # A mountain with a snow cap, and a sun over it.
    peak = (v > 0.28 + np.abs(u - 0.5) * 0.9) & (v < 0.62)
    rgb[peak] = np.array([0.86, 0.84, 0.9])
    cap = peak & (v < 0.36 + np.abs(u - 0.5) * 0.9 - 0.04)
    rgb[cap] = np.array([1.0, 1.0, 1.0])
    sun = np.hypot(u - 0.5, (v - 0.18) * h / w) < 0.1
    rgb[sun] = gold
    save_rgba(OUT / "env" / "cb_banner.png", rgb, alpha)


def cb_rope():
    """Twisted rope along u, soft at its two edges (v)."""
    w, h = 256, 16
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    v = (yy / h - 0.5) * 2
    twist = 0.5 + 0.5 * np.sin((xx * 0.9 + yy * 1.6))
    round_ = np.sqrt(np.clip(1 - v * v, 0, 1))
    tone = (0.35 + 0.3 * twist) * (0.45 + 0.55 * round_)
    rgb = np.stack([tone * 1.0, tone * 0.82, tone * 0.58], -1)
    save_rgba(OUT / "env" / "cb_rope.png", rgb, smoothstep(1.0, 0.8, np.abs(v)))


def cb_walker():
    """A traveller walking right: hood, cloak, a pack and a staff."""
    from PIL import Image, ImageDraw
    S = 4
    w, h = 56, 88
    im = Image.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)

    def P(pts):
        return [(x * S, y * S) for x, y in pts]

    cloak = (70, 58, 64, 255)
    d.line(P([(44, 10), (38, 86)]), fill=(92, 66, 42, 255), width=3 * S)            # staff
    d.polygon(P([(14, 26), (26, 22), (28, 50), (14, 52)]), fill=(96, 70, 48, 255))    # pack
    d.polygon(P([(22, 20), (36, 20), (40, 60), (34, 72), (20, 72), (18, 56)]), fill=cloak)
    d.polygon(P([(24, 70), (29, 70), (26, 86), (20, 86)]), fill=(46, 38, 40, 255))    # legs
    d.polygon(P([(30, 70), (34, 70), (38, 86), (32, 86)]), fill=(52, 42, 44, 255))
    d.ellipse(P([(22, 6), (38, 24)]), fill=(84, 66, 70, 255))                          # hood
    d.ellipse(P([(30, 12), (37, 20)]), fill=(222, 178, 142, 255))                      # face
    d.line(P([(34, 36), (42, 40)]), fill=cloak, width=4 * S)                           # arm to staff
    im = im.resize((w, h), Image.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(OUT / "env" / "cb_walker.png", a[..., :3], a[..., 3])


# ── 29 Atlas of the Deep ──────────────────────────────────────────────────
#
# An old sea chart on parchment: depth bands and contour lines in the sea,
# inked coasts, hatched shallows, hills on the land, a lat/long grid and a
# compass rose. The land comes from `chart.py`, which `gen_scenes.py` reads
# to put the names on it. No lettering here: every word is world text.


def ad_chart():
    import chart
    from PIL import Image, ImageDraw
    w, h = chart.SIZE
    f = chart.field()
    land = f > chart.LAND
    rng = np.random.default_rng(29)
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    stain = _sea_noise(256, 6, 3, rng)[(yy.astype(int) // 8) % 256, (xx.astype(int) // 8) % 256]
    grain = _sea_noise(256, 64, 2, rng)[yy.astype(int) % 256, xx.astype(int) % 256]
    paper = np.array([0.94, 0.88, 0.74])
    rgb = paper[None, None, :] * (0.9 + 0.08 * stain + 0.03 * grain)[..., None]
    # The sea: tinted toward teal with depth, in bands.
    depth = np.clip((chart.LAND - f) / chart.LAND, 0, 1)
    band = np.floor(depth * 5) / 5
    sea = np.array([0.62, 0.76, 0.76])
    k = (0.18 + 0.5 * band)[..., None]
    rgb = np.where(land[..., None], rgb, rgb * (1 - k) + sea * k)
    # Contour lines where the floor crosses a depth.
    for level in (0.48, 0.38, 0.28):
        near = np.abs(f - level) < 0.0035
        rgb[near & ~land] = rgb[near & ~land] * 0.55 + np.array([0.2, 0.36, 0.4]) * 0.45
    # Shallows hatched: diagonal strokes just off the coast.
    shallow = (~land) & (f > chart.LAND - 0.05)
    hatch = ((xx + yy).astype(int) % 7) < 1
    rgb[shallow & hatch] = rgb[shallow & hatch] * 0.6 + np.array([0.25, 0.4, 0.42]) * 0.4
    # The land: warm ochre, darker inland where it rises, stippled hills.
    land_c = np.array([0.86, 0.76, 0.55])
    rise = np.clip((f - chart.LAND) / 0.2, 0, 1)
    lc = land_c[None, None, :] * (1.0 - 0.18 * rise)[..., None] * (0.93 + 0.07 * stain)[..., None]
    rgb = np.where(land[..., None], lc, rgb)
    hills = land & (f > chart.LAND + 0.1) & (rng.random((h, w)) < 0.06)
    rgb[hills] = rgb[hills] * 0.5 + np.array([0.35, 0.28, 0.18]) * 0.5
    # The coast in ink: land with sea a pixel or two away.
    from numpy.lib.stride_tricks import sliding_window_view as win
    pad = np.pad(land, 2, mode='edge')
    any_sea = ~win(pad, (5, 5)).all(axis=(2, 3))
    coast = land & any_sea
    rgb[coast] = np.array([0.24, 0.17, 0.11])
    img = Image.fromarray((np.clip(rgb, 0, 1) ** (1 / 1.0) * 255).astype(np.uint8), 'RGB')
    d = ImageDraw.Draw(img)
    ink = (96, 72, 50)
    # The graticule, and a frame.
    for gx in range(0, w, 128):
        d.line([(gx, 0), (gx, h)], fill=(150, 125, 95), width=1)
    for gy in range(0, h, 128):
        d.line([(0, gy), (w, gy)], fill=(150, 125, 95), width=1)
    d.rectangle([6, 6, w - 7, h - 7], outline=ink, width=4)
    d.rectangle([16, 16, w - 17, h - 17], outline=ink, width=1)
    # A compass rose in the open sea.
    import math
    cx, cy, r = int(w * 0.1), int(h * 0.16), 100
    for i in range(32):
        a = i * math.pi / 16
        rr = r if i % 4 == 0 else (r * 0.62 if i % 2 == 0 else r * 0.42)
        d.line([(cx, cy), (cx + rr * math.sin(a), cy - rr * math.cos(a))], fill=ink, width=2 if i % 4 == 0 else 1)
    for i in range(4):
        a = i * math.pi / 2
        p = [(cx + r * math.sin(a), cy - r * math.cos(a)),
             (cx + 18 * math.sin(a + math.pi / 2), cy - 18 * math.cos(a + math.pi / 2)),
             (cx, cy), (cx + 18 * math.sin(a - math.pi / 2), cy - 18 * math.cos(a - math.pi / 2))]
        d.polygon(p, fill=(150, 60, 45) if i == 0 else ink)
    d.ellipse([cx - r - 8, cy - r - 8, cx + r + 8, cy + r + 8], outline=ink, width=2)
    d.ellipse([cx - 20, cy - 20, cx + 20, cy + 20], outline=ink, width=2)
    arr = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(OUT / "env" / "ad_chart.png", arr, np.ones((h, w)))



# ── 31 The Storyteller ─────────────────────────────────────────────────

STORY_RNG = np.random.default_rng(31)


def st31_desk():
    """Dark walnut planks, tiling: long grain along x, a seam every plank."""
    n = 512
    y, x = np.mgrid[0:n, 0:n].astype(np.float64)
    grain = fbm(n, 4, 5)
    streak = np.sin((y / n * 6.0 + grain * 3.0) * np.pi * 8.0) * 0.5 + 0.5
    plank = (y % 128) / 128.0
    seam = smoothstep(0.02, 0.0, plank) + smoothstep(0.98, 1.0, plank)
    base = np.array([0.24, 0.15, 0.09])
    light = np.array([0.36, 0.23, 0.13])
    t = np.clip(0.35 + streak * 0.35 + (grain - 0.5) * 0.6, 0, 1)[..., None]
    rgb = base * (1 - t) + light * t
    rgb *= (1.0 - 0.55 * np.clip(seam, 0, 1))[..., None]
    save_rgba(OUT / "story" / "desk.png", rgb, np.ones((n, n)))


def st31_book():
    """An open book from above: two cream pages with fibre, a soft gutter
    shadow down the middle, the stacked page edges below, a leather cover
    peeking out round it. Text is drawn on it by the scene."""
    w, h = 1200, 700
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w
    v = (y + 0.5) / h
    # The cover: a rounded rectangle a little bigger than the pages.
    def rounded(cx, cy, hw, hh, r):
        dx = np.maximum(np.abs(x - cx) - (hw - r), 0)
        dy = np.maximum(np.abs(y - cy) - (hh - r), 0)
        return smoothstep(r + 1.0, r - 1.0, np.sqrt(dx * dx + dy * dy))
    cover = rounded(w / 2, h / 2, w / 2 - 4, h / 2 - 4, 26)
    leather = np.array([0.30, 0.10, 0.08]) * (0.8 + 0.4 * fbm_rect(w, h, 3, 4))[..., None]
    # The pages: each a slightly curved sheet, higher at the outer edge.
    page_l = rounded(w * 0.255, h * 0.48, w * 0.235, h * 0.43, 10)
    page_r = rounded(w * 0.745, h * 0.48, w * 0.235, h * 0.43, 10)
    pages = np.maximum(page_l, page_r)
    paper = np.array([0.93, 0.87, 0.72])
    fibre = fbm_rect(w, h, 40, 3)
    rgb_p = paper * (0.94 + 0.08 * fibre)[..., None]
    # Gutter: dark in the middle, a page's curve lighter towards the edges.
    gutter = np.exp(-((u - 0.5) / 0.035) ** 2)
    curve = 1.0 - 0.10 * (1 - np.abs(u - 0.5) * 2) ** 3
    rgb_p *= ((1.0 - 0.45 * gutter) * curve)[..., None]
    # Age at the edges.
    edge = np.minimum(np.minimum(u, 1 - u), np.minimum(v, 1 - v))
    rgb_p *= (0.86 + 0.14 * smoothstep(0.0, 0.08, edge))[..., None]
    # Stacked page edges peeking below each page.
    stack = np.maximum(rounded(w * 0.255, h * 0.495, w * 0.235, h * 0.43, 10),
                       rounded(w * 0.745, h * 0.495, w * 0.235, h * 0.43, 10)) * (1 - pages)
    lines = 0.75 + 0.25 * np.sin(y * 2.2)
    rgb_s = np.array([0.80, 0.74, 0.60]) * lines[..., None]
    rgb = leather * cover[..., None]
    rgb = rgb * (1 - stack[..., None]) + rgb_s * stack[..., None]
    rgb = rgb * (1 - pages[..., None]) + rgb_p * pages[..., None]
    save_rgba(OUT / "story" / "book.png", rgb, np.clip(cover, 0, 1))


def fbm_rect(w, h, cells, octaves):
    """`fbm` stretched to a w x h rectangle (not tiling)."""
    n = max(w, h)
    big = fbm(n, cells, octaves)
    return big[:h, :w]


def st31_illustration():
    """An ink vignette for the facing page: a moon over reeds and a
    lantern-lit hill, in brown ink with a hatched shadow - the alpha is the
    ink, so the page shows through."""
    w, h = 400, 420
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    ink = np.zeros((h, w))
    cx, cy = w / 2, h / 2
    # A round frame.
    r = np.sqrt((x - cx) ** 2 + (y - cy) ** 2)
    frame_r = min(w, h) * 0.46
    ink = np.maximum(ink, smoothstep(2.2, 0.8, np.abs(r - frame_r)))
    ink = np.maximum(ink, smoothstep(1.2, 0.3, np.abs(r - frame_r + 7)) * 0.7)
    inside = r < frame_r - 9
    # The moon, a crescent.
    m1 = np.sqrt((x - cx - 70) ** 2 + (y - cy + 90) ** 2) < 42
    m2 = np.sqrt((x - cx - 86) ** 2 + (y - cy + 100) ** 2) < 38
    ink = np.maximum(ink, (m1 & ~m2) * 0.95 * inside)
    # A hill, hatched.
    hill = y > cy + 40 - 70 * np.exp(-((x - cx + 40) / 120) ** 2) + 10 * np.sin(x / 23)
    hatch = (np.sin((x + y) * 0.55) > 0.6) * 0.75
    ink = np.maximum(ink, hill * inside * hatch)
    ridge = np.abs(y - (cy + 40 - 70 * np.exp(-((x - cx + 40) / 120) ** 2) + 10 * np.sin(x / 23))) < 1.6
    ink = np.maximum(ink, ridge * inside * 1.0)
    # Reeds: slanted strokes at the bottom.
    for k in range(38):
        bx = cx - frame_r + 20 + k * (2 * frame_r - 40) / 38 + STORY_RNG.normal(0, 3)
        top = cy + 40 + STORY_RNG.random() * 60
        lean = STORY_RNG.normal(0, 0.18)
        yy = np.clip(y, top, h)
        line = np.abs(x - (bx + (yy - top) * lean)) < 1.1 + (y - top) / 160
        ink = np.maximum(ink, line * (y > top) * inside)
    # A lantern on the hilltop: a little diamond.
    lx, ly = cx - 40, cy - 38
    ink = np.maximum(ink, (np.abs(x - lx) + np.abs(y - ly) < 10) * 0.9)
    save_rgba(OUT / "story" / "illustration.png", np.array([0.30, 0.17, 0.08]), np.clip(ink, 0, 1))


def _supersample_mask(w, h, inside, k=4):
    """A shape test on a k-times grid, averaged: antialiased alpha."""
    yy, xx = np.mgrid[0:h * k, 0:w * k].astype(np.float64)
    m = inside((xx + 0.5) / k, (yy + 0.5) / k).astype(np.float64)
    return m.reshape(h, k, w, k).mean(axis=(1, 3))


def st31_icons():
    """Two inline icons for the rich text: an old brass key and a red heart,
    each a 64 px square, drawn in ink so they sit in the line like words."""
    n = 64

    def key(x, y):
        bow = np.abs(np.sqrt((x - 18) ** 2 + (y - 32) ** 2) - 11) < 4
        shaft = (np.abs(y - 32) < 3.5) & (x > 26) & (x < 58)
        bits = ((x > 46) & (x < 51) & (y > 32) & (y < 44)) | ((x > 53) & (x < 58) & (y > 32) & (y < 41))
        return bow | shaft | bits
    a = _supersample_mask(n, n, key)
    yy, xx = np.mgrid[0:n, 0:n].astype(np.float64)
    shade = 0.75 + 0.25 * (1 - yy / n)
    rgb = np.array([0.72, 0.52, 0.16]) * shade[..., None]
    save_rgba(OUT / "story" / "key.png", rgb, a)

    def heart(x, y):
        u = (x - 32) / 24
        v = (32 - y) / 24 + 0.25
        return (u * u + v * v - 1) ** 3 - u * u * v ** 3 < 0
    a = _supersample_mask(n, n, heart)
    rgb = np.array([0.78, 0.12, 0.14]) * (0.8 + 0.2 * (1 - yy / n))[..., None]
    save_rgba(OUT / "story" / "heart.png", rgb, a)


def st31_candle():
    """A cream wax candle, round-shaded, with drips and a black wick."""
    w, h = 64, 200
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x + 0.5) / w * 2 - 1
    body = (np.abs(u) < 0.62) & (y > 18)
    drips = (np.abs(u) < 0.72) & (y > 18) & (y < 18 + 30 * (0.5 + 0.5 * np.sin(u * 9)) ** 3)
    a = (body | drips).astype(np.float64)
    a *= smoothstep(0.62, 0.55, np.abs(u)) + drips * 1.0
    a = np.clip(a, 0, 1)
    shade = np.clip(1.0 - u * u * 0.9 - (u > 0) * u * 0.3, 0.2, 1)
    rgb = np.array([0.92, 0.86, 0.70]) * shade[..., None]
    wick = (np.abs(u) < 0.06) & (y > 4) & (y <= 20)
    rgb[wick] = [0.06, 0.05, 0.05]
    a = np.maximum(a, wick.astype(np.float64))
    save_rgba(OUT / "story" / "candle.png", rgb, a)



# ── 32 Mapmaker ────────────────────────────────────────────────────────

MM_TILE = 48


def _mm_corner_field(mask, t):
    """Corner values of a dual-grid tile (`tilemap::autotile` bit order:
    1 top-left, 2 top-right, 4 bottom-left, 8 bottom-right), blended
    bilinearly - which is what makes two neighbours agree along their shared
    edge whatever their other corners are."""
    y, x = np.mgrid[0:t, 0:t].astype(np.float64)
    u = (x + 0.5) / t
    v = (y + 0.5) / t
    tl, tr, bl, br = (mask >> 0) & 1, (mask >> 1) & 1, (mask >> 2) & 1, (mask >> 3) & 1
    return (tl * (1 - u) * (1 - v) + tr * u * (1 - v) + bl * (1 - u) * v + br * u * v)


def mm32_terrain():
    """The Mapmaker's tileset: three terrains (sand, grass, rock) over the
    sea (`mm32_water`, a backdrop under the map), a
    row of sixteen dual-grid tiles each - tile `m` of a row is the terrain
    over the corners in mask `m`; tile 0 is never drawn. Every edge is the
    0.5 line of the corner field plus noise that repeats every tile, so any
    tile meets any other seamlessly."""
    t = MM_TILE
    rng = np.random.default_rng(32)
    rows = 3
    sheet = np.zeros((rows * t, 16 * t, 4))

    def noise(cells, octaves):
        return fbm(t, cells, octaves)

    # Every tile of a terrain shares its texture, so anything at the scale
    # of a tile repeats on a grid; keep the grain fine and the blotches faint.
    n_edge = [noise(2, 3) for _ in range(rows)]
    n_tex = [noise(6, 4) for _ in range(rows)]
    n_fine = [noise(12, 2) for _ in range(rows)]
    y, x = np.mgrid[0:t, 0:t].astype(np.float64)
    for row in range(rows):
        for m in range(1, 16):
            f = _mm_corner_field(m, t) + (n_edge[row] - 0.5) * 0.42
            inside = smoothstep(0.47, 0.53, f)
            depth = np.clip((f - 0.5) * 3.0, 0.0, 1.0)      # 0 at the edge, 1 well in
            tex = n_tex[row]
            fine = n_fine[row]
            if row == 0:    # sand: warm, grainy, wet near the edge, foam outside it
                base = np.array([0.88, 0.79, 0.58])
                rgb = base * (0.9 + 0.16 * fine[..., None])
                wet = 1.0 - smoothstep(0.0, 0.35, depth)
                rgb = rgb * (1.0 - 0.22 * wet[..., None])
                foam = smoothstep(0.32, 0.45, f) * (1.0 - inside) * (0.55 + 0.45 * fine)
                rgb = rgb * inside[..., None] + np.array([0.95, 0.97, 1.0]) * (1 - inside[..., None])
                a = np.clip(inside + foam * 0.85, 0, 1)
            elif row == 1:  # grass: green, tufted, a darker lip and a soft shadow outside
                base = np.array([0.36, 0.58, 0.27])
                rgb = base * (0.94 + 0.1 * tex[..., None]) * (0.88 + 0.22 * fine[..., None])
                lip = 1.0 - smoothstep(0.0, 0.25, depth)
                rgb = rgb * (1.0 - 0.3 * lip[..., None])
                shadow = smoothstep(0.36, 0.47, f) * (1.0 - inside) * 0.35
                rgb = rgb * inside[..., None]
                a = np.clip(inside + shadow, 0, 1)
            else:           # rock: a grey plateau, cracked, with a dark cliff rim
                base = np.array([0.55, 0.51, 0.47])
                crack = smoothstep(0.48, 0.5, fine) * (1 - smoothstep(0.5, 0.52, fine))
                rgb = base * (0.92 + 0.12 * tex[..., None]) * (0.9 + 0.16 * fine[..., None]) \
                    * (1.0 - 0.3 * crack[..., None])
                top = smoothstep(0.0, 0.35, depth)
                rgb = rgb * (0.78 + 0.22 * top[..., None])
                rim = smoothstep(0.30, 0.47, f) * (1.0 - inside)
                rgb = rgb * inside[..., None] + np.array([0.22, 0.19, 0.17]) * (1 - inside[..., None])
                a = np.clip(inside + rim * 0.9, 0, 1)
            sheet[row * t:(row + 1) * t, m * t:(m + 1) * t, :3] = rgb
            sheet[row * t:(row + 1) * t, m * t:(m + 1) * t, 3] = a
    save_rgba(OUT / "terrain" / "terrain.png", sheet[..., :3], sheet[..., 3])


def mm32_water():
    """The sea under the Mapmaker's island: a seamless 256 px tile, much
    larger than a map cell, so its pattern does not march with the grid."""
    t = 256
    tex = fbm(t, 4, 5)
    swell = fbm(t, 9, 3)
    base = np.array([0.15, 0.34, 0.56])
    rgb = base * (0.82 + 0.32 * tex[..., None])
    rgb = rgb + np.array([0.10, 0.15, 0.16]) * smoothstep(0.64, 0.78, swell)[..., None] * 0.8
    save_rgba(OUT / "terrain" / "water.png", rgb, np.ones((t, t)))


def mm32_shade():
    """A shade that fades in towards its bottom edge, for under a caption."""
    y = (np.arange(128) + 0.5) / 128
    a = np.tile(smoothstep(0.0, 1.0, y)[:, None] ** 1.5, (1, 4))
    save_rgba(OUT / "terrain" / "shade.png", 1.0, a)


def mm32_cursor():
    """The brush: a soft white ring."""
    _, _, r = radius(64)
    a = np.exp(-((r - 0.85) ** 2) / 0.004) * 0.9
    save_rgba(OUT / "terrain" / "cursor.png", 1.0, a)

# ── 33 Hillside Hamlet ─────────────────────────────────────────────────
# Isometric art: a cell is a 96 x 48 diamond, a block rises HH_LIFT above
# it. Every tile is drawn bottom-aligned on its cell, so the image of a
# block or a house is the diamond plus what stands on it.

HH_W, HH_H, HH_LIFT = 96, 48, 32


def _hh_diamond_st(w, h, top=0.0):
    """Each pixel's place in a diamond whose top vertex is at (w / 2, top):
    `s` runs down-right along a cell's column axis, `t` down-left along its
    row axis, both 0..1 inside."""
    y, x = np.mgrid[0:int(h), 0:int(w)].astype(np.float64)
    px = (x + 0.5) - w / 2.0
    py = (y + 0.5) - top
    s = py / HH_H + px / HH_W
    t = py / HH_H - px / HH_W
    return s, t


def _hh_inside(s, t, soft=0.0):
    """Coverage of the diamond. Hard by default: two neighbours each half
    transparent along their shared edge let the ground through as a seam."""
    m = np.minimum(np.minimum(s, 1 - s), np.minimum(t, 1 - t))
    if soft <= 0.0:
        return (m >= 0.0).astype(np.float64)
    return smoothstep(-soft, soft, m)


def _hh_grass(s, t, rng, shade=1.0):
    n = _hh_noise(s, t, 5, rng) * 0.6 + _hh_noise(s, t, 13, rng) * 0.4
    base = np.array([0.36, 0.60, 0.24])
    rgb = base * (0.86 + 0.26 * n[..., None])
    tuft = smoothstep(0.66, 0.74, _hh_noise(s, t, 21, rng))
    rgb = rgb * (1.0 - 0.18 * tuft[..., None])
    return rgb * shade


def _hh_noise(s, t, cells, rng):
    """Value noise on the diamond's own axes, wrapping every cell, so two
    neighbouring tiles meet without a seam."""
    grid = rng.random((cells, cells))
    fs = np.mod(s, 1.0) * cells
    ft = np.mod(t, 1.0) * cells
    i0 = np.floor(fs).astype(int) % cells
    j0 = np.floor(ft).astype(int) % cells
    i1, j1 = (i0 + 1) % cells, (j0 + 1) % cells
    a = smoothstep(0, 1, fs - np.floor(fs))
    b = smoothstep(0, 1, ft - np.floor(ft))
    top = grid[j0, i0] * (1 - a) + grid[j0, i1] * a
    bot = grid[j1, i0] * (1 - a) + grid[j1, i1] * a
    return top * (1 - b) + bot * b


def hh33_ground():
    """The ground tileset, one row of 96 x 48 diamonds: grass, grass with
    flowers - and then two autotiled terrains, sixteen tiles each in corner
    order (`tilemap::autotile` - in an isometric map bit 1 is the top
    vertex, 2 the right, 4 the left, 8 the bottom): a dirt lane and a pond."""
    rng = np.random.default_rng(33)
    sheet = np.zeros((HH_H, 34 * HH_W, 4))
    s, t = _hh_diamond_st(HH_W, HH_H)
    inside = _hh_inside(s, t)
    grass = _hh_grass(s, t, np.random.default_rng(1))
    sheet[:, 0:HH_W, :3] = grass
    sheet[:, 0:HH_W, 3] = inside
    fl = grass.copy()
    dots = _hh_noise(s, t, 17, np.random.default_rng(5))
    for colour, lo in (((0.95, 0.85, 0.30), 0.80), ((0.95, 0.55, 0.70), 0.83), ((0.95, 0.95, 0.98), 0.86)):
        m = smoothstep(lo, lo + 0.02, dots) * (1 - smoothstep(lo + 0.025, lo + 0.03, dots))
        fl = fl * (1 - m[..., None]) + np.array(colour) * m[..., None]
        dots = _hh_noise(s, t, 17, rng)
    sheet[:, HH_W:2 * HH_W, :3] = fl
    sheet[:, HH_W:2 * HH_W, 3] = inside
    edge_n = _hh_noise(s, t, 3, np.random.default_rng(9))
    fine = _hh_noise(s, t, 15, np.random.default_rng(11))
    for k, kind in enumerate(("lane", "pond")):
        for m in range(1, 16):
            tl, tr_, bl, br = m & 1, (m >> 1) & 1, (m >> 2) & 1, (m >> 3) & 1
            # The dual tile is centred on a vertex, so its own four vertices
            # are the centres of the four cells sharing that vertex.
            u, v = s, t
            f = tl * (1 - u) * (1 - v) + tr_ * u * (1 - v) + bl * (1 - u) * v + br * u * v
            f = f + (edge_n - 0.5) * 0.30
            ins = smoothstep(0.47, 0.53, f) * inside
            depth = np.clip((f - 0.5) * 3.0, 0, 1)
            if kind == "lane":
                rgb = np.array([0.62, 0.48, 0.32]) * (0.88 + 0.2 * fine[..., None])
                rut = 1 - smoothstep(0.0, 0.3, depth)
                rgb = rgb * (1 - 0.18 * rut[..., None])
                a = ins
            else:
                rgb = np.array([0.20, 0.42, 0.62]) * (0.9 + 0.15 * fine[..., None])
                rgb = rgb * (0.75 + 0.25 * smoothstep(0.0, 0.6, depth)[..., None])
                glint = smoothstep(0.78, 0.84, _hh_noise(s, t, 7, rng)) * smoothstep(0.2, 0.6, depth)
                rgb = rgb + np.array([0.35, 0.40, 0.40]) * glint[..., None]
                bank = smoothstep(0.36, 0.47, f) * (1 - ins) * inside
                rgb = rgb * ins[..., None] + np.array([0.30, 0.42, 0.18]) * (1 - ins[..., None])
                a = np.clip(ins + bank * 0.85, 0, 1)
            x0 = (2 + k * 16 + m) * HH_W
            sheet[:, x0:x0 + HH_W, :3] = rgb
            sheet[:, x0:x0 + HH_W, 3] = a
    save_rgba(OUT / "hamlet" / "ground.png", sheet[..., :3], sheet[..., 3])


def _hh_faces(img, lift, top_rgb, left_rgb, right_rgb, top_alpha, w=HH_W, h=HH_H):
    """A block: the diamond on top, its two front faces dropping `lift`."""
    H, W = img.shape[:2]
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cx = w / 2.0
    # Left face: below the edge from the left vertex (0, h/2) to the bottom
    # vertex (w/2, h), down by lift.
    edge_l = h / 2 + (x / cx) * (h / 2)
    left = (x < cx) & (y >= edge_l) & (y < edge_l + lift)
    edge_r = h - ((x - cx) / cx) * (h / 2)
    right = (x >= cx) & (y >= edge_r) & (y < edge_r + lift)
    img[left, :3] = left_rgb[left] if np.ndim(left_rgb) == 3 else left_rgb
    img[left, 3] = 1
    img[right, :3] = right_rgb[right] if np.ndim(right_rgb) == 3 else right_rgb
    img[right, 3] = 1
    m = top_alpha > 0.01
    img[m, :3] = top_rgb[m]
    img[m, 3] = top_alpha[m]


def hh33_blocks():
    """The hill: grass-topped earth blocks, one 96 x 80 tile, and a stone
    one for the hill's crown."""
    H = HH_H + HH_LIFT
    sheet = np.zeros((H, 2 * HH_W, 4))
    s, t = _hh_diamond_st(HH_W, H)
    inside = _hh_inside(s, t, 0.006)
    y, x = np.mgrid[0:H, 0:HH_W].astype(np.float64)
    strata = 0.85 + 0.15 * np.sin(y * 0.9 + np.sin(x * 0.2) * 2)
    for k in range(2):
        img = np.zeros((H, HH_W, 4))
        if k == 0:
            top = _hh_grass(s, t, np.random.default_rng(1), 1.05)
            earth = np.array([0.50, 0.36, 0.24])
        else:
            top = np.array([0.62, 0.60, 0.56]) * (0.85 + 0.25 * _hh_noise(s, t, 9, np.random.default_rng(3)))[..., None]
            earth = np.array([0.46, 0.44, 0.42])
        left = earth[None, None, :] * strata[..., None] * 0.95
        right = earth[None, None, :] * strata[..., None] * 0.68
        _hh_faces(img, HH_LIFT, top, left, right, inside)
        # A grass lip over the faces, and a dark line where the faces meet.
        lip = (img[..., 3] > 0) & (inside < 0.5)
        yy = y + 0.5
        lipband = np.abs(np.where(x < HH_W / 2, HH_H / 2 + (x + 0.5) / (HH_W / 2) * (HH_H / 2),
                                  HH_H - (x + 0.5 - HH_W / 2) / (HH_W / 2) * (HH_H / 2)) - yy) < 4
        if k == 0:
            img[lip & lipband, :3] = top.mean(axis=(0, 1)) * 0.8
        seam = (np.abs(x + 0.5 - HH_W / 2) < 0.8) & (yy > HH_H)
        img[seam, :3] *= 0.6
        sheet[:, k * HH_W:(k + 1) * HH_W] = img
    save_rgba(OUT / "hamlet" / "blocks.png", sheet[..., :3], sheet[..., 3])


def _hh_shadow(img, cx, cy, rx, ry, strength):
    """A soft contact shadow, under whatever is drawn later."""
    H, W = img.shape[:2]
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    d = ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2
    a = (1 - smoothstep(0.4, 1.0, d)) * strength
    img[..., :3] = img[..., :3] * (1 - a[..., None]) * (img[..., 3:4] > 0) + 0.0
    img[..., 3] = np.maximum(img[..., 3], a)


def _hh_polygon(pts, w, h, scale=4):
    """A polygon's coverage as a boolean mask (drawn 4x and sampled)."""
    from PIL import ImageDraw
    m = Image.new("L", (w * scale, h * scale), 0)
    ImageDraw.Draw(m).polygon([(px * scale, py * scale) for px, py in pts], fill=255)
    m = np.asarray(m.resize((w, h), Image.BOX), dtype=np.float64) / 255.0
    return m > 0.5


def _hh_house(rng, wall, roof, H):
    """A cottage on one cell: a box of walls, a pitched roof along the row
    axis, a door and a window on the front faces."""
    img = np.zeros((H, HH_W, 4))
    base = H - HH_H                     # the cell's top vertex, in the image
    _hh_shadow(img, HH_W / 2, base + HH_H / 2 + 4, 46, 22, 0.45)
    inset = 10.0
    w, h = HH_W - 2 * inset, (HH_W - 2 * inset) / 2
    wall_h = 40.0
    y, x = np.mgrid[0:H, 0:HH_W].astype(np.float64) + 0.5
    cx = HH_W / 2
    # Footprint diamond, inset.
    top_y = base + inset / 2 + (HH_H - h) / 2 - inset / 2
    left_v = (inset, top_y + h / 2)
    bot_v = (cx, top_y + h)
    right_v = (HH_W - inset, top_y + h / 2)
    # Front-left wall: from left vertex to bottom vertex, up wall_h.
    def face(p0, p1, height, x, y):
        tt = (x - p0[0]) / (p1[0] - p0[0])
        ey = p0[1] + tt * (p1[1] - p0[1])
        return (tt >= 0) & (tt <= 1) & (y <= ey) & (y >= ey - height), tt, (ey - y) / height
    fl, tl, hl = face(left_v, bot_v, wall_h, x, y)
    fr, trr, hr = face(bot_v, right_v, wall_h, x, y)
    grain = 0.9 + 0.1 * np.sin(y * 1.7)
    img[fl, :3] = (np.array(wall) * 0.95)[None, :] * grain[fl][:, None]
    img[fl, 3] = 1
    img[fr, :3] = (np.array(wall) * 0.72)[None, :] * grain[fr][:, None]
    img[fr, 3] = 1
    # A door on the right face, a window on the left.
    door = fr & (np.abs(trr - 0.45) < 0.12) & (hr < 0.55)
    img[door, :3] = np.array([0.30, 0.18, 0.10])
    win = fl & (np.abs(tl - 0.5) < 0.13) & (np.abs(hl - 0.55) < 0.17)
    img[win, :3] = np.array([0.95, 0.80, 0.45])
    frame = fl & (np.abs(tl - 0.5) < 0.15) & (np.abs(hl - 0.55) < 0.2) & ~win
    img[frame, :3] = np.array([0.35, 0.24, 0.16])
    # Roof: a ridge along the left-right axis, over the wall tops.
    ridge_h = 34.0
    eave = 6.0
    lv = (left_v[0] - eave, left_v[1] - wall_h + 2)
    bv = (bot_v[0], bot_v[1] - wall_h + eave)
    rv = (right_v[0] + eave, right_v[1] - wall_h + 2)
    tv = (cx, top_y - wall_h - eave + 2)
    mid_front = ((lv[0] + bv[0]) / 2, (lv[1] + bv[1]) / 2 - ridge_h)
    mid_back = ((tv[0] + rv[0]) / 2, (tv[1] + rv[1]) / 2 - ridge_h)

    def tri_fill(pts, colour, shade_fn=None):
        inside = _hh_polygon(pts, img.shape[1], img.shape[0])
        c = np.array(colour)[None, None, :] * np.ones_like(img[..., :3])
        if shade_fn is not None:
            c = c * shade_fn[..., None]
        img[inside, :3] = c[inside]
        img[inside, 3] = 1
    shingle = 0.88 + 0.12 * (np.mod(y * 0.5 + x * 0.25, 2.0) > 1.0)
    # Gable at the left end (a triangle on the left wall's top edge), the
    # front slope, the back slope's visible sliver.
    tri_fill([lv, bv, mid_front], np.array(wall) * 0.9)
    tri_fill([mid_front, mid_back, rv, bv], np.array(roof) * 0.78, shingle)
    tri_fill([mid_front, mid_back, tv, lv], np.array(roof) * 1.0, shingle)
    # Chimney.
    ch = (np.abs(x - (cx + 14)) < 4) & (y > mid_back[1] - 14) & (y < mid_back[1] + 10)
    img[ch, :3] = np.array([0.45, 0.30, 0.25])
    img[ch, 3] = 1
    return img


def _hh_tree(rng, H, tone):
    img = np.zeros((H, HH_W, 4))
    base = H - HH_H
    cx, cy = HH_W / 2, base + HH_H / 2
    _hh_shadow(img, cx + 6, cy + 4, 34, 15, 0.45)
    y, x = np.mgrid[0:H, 0:HH_W].astype(np.float64) + 0.5
    trunk = (np.abs(x - cx) < 4 + (y - (cy - 40)) * 0.05) & (y > cy - 46) & (y < cy + 2)
    img[trunk, :3] = np.array([0.36, 0.24, 0.15]) * (0.8 + 0.2 * (x[trunk] < cx))[:, None]
    img[trunk, 3] = 1
    blobs = [(0, -70, 28), (-16, -56, 22), (17, -54, 22), (-6, -92, 20), (10, -84, 19), (0, -48, 18)]
    for bx, by, r in blobs:
        d = np.sqrt((x - cx - bx) ** 2 + (y - cy - by) ** 2)
        m = d < r
        light = np.clip(1.0 - ((x - cx - bx + r * 0.4) ** 2 + (y - cy - by + r * 0.5) ** 2) / (r * r * 1.8), 0, 1)
        leaf = 0.85 + 0.15 * np.sin(x * 1.3 + np.cos(y * 1.1) * 3)
        col = np.array(tone)[None, None, :] * (0.62 + 0.5 * light[..., None]) * leaf[..., None]
        img[m, :3] = col[m]
        img[m, 3] = 1
    return img


def _hh_well(H):
    img = np.zeros((H, HH_W, 4))
    base = H - HH_H
    cx, cy = HH_W / 2, base + HH_H / 2
    _hh_shadow(img, cx, cy + 3, 30, 14, 0.4)
    y, x = np.mgrid[0:H, 0:HH_W].astype(np.float64) + 0.5
    ring = (((x - cx) / 22) ** 2 + ((y - cy + 6) / 11) ** 2 < 1)
    wall = ring & (y > cy - 6)
    wall2 = (np.abs(x - cx) < 22) & (y > cy - 6) & (y < cy + 10) & (((x - cx) / 22) ** 2 + ((y - cy - 10) / 11) ** 2 < 1.0001 + 1)
    stone = np.array([0.62, 0.60, 0.56])
    body = ((np.abs(x - cx) < 22) & (y >= cy - 6) & (y < cy + 6 + 11 * np.sqrt(np.clip(1 - ((x - cx) / 22) ** 2, 0, 1)))) | ring
    img[body, :3] = stone * (0.75 + 0.25 * (x[body] < cx))[:, None]
    img[body, 3] = 1
    water = (((x - cx) / 17) ** 2 + ((y - cy + 6) / 8) ** 2 < 1)
    img[water, :3] = np.array([0.12, 0.22, 0.32])
    for px in (cx - 20, cx + 20):
        post = (np.abs(x - px) < 2.5) & (y > cy - 48) & (y < cy + 2)
        img[post, :3] = np.array([0.40, 0.27, 0.16])
        img[post, 3] = 1
    roof = (np.abs(x - cx) < 30 - (cy - 44 - y) * 0.9) & (y > cy - 64) & (y < cy - 44)
    img[roof, :3] = np.array([0.55, 0.22, 0.16]) * (0.8 + 0.2 * (x[roof] < cx))[:, None]
    img[roof, 3] = 1
    return img


def _hh_fence(H):
    img = np.zeros((H, HH_W, 4))
    base = H - HH_H
    y, x = np.mgrid[0:H, 0:HH_W].astype(np.float64) + 0.5
    # Along the cell's bottom-left edge (left vertex to bottom vertex).
    p0, p1 = (0, base + HH_H / 2), (HH_W / 2, base + HH_H)
    wood = np.array([0.56, 0.40, 0.24])
    for k in range(4):
        tt = (k + 0.5) / 4
        px, py = p0[0] + (p1[0] - p0[0]) * tt, p0[1] + (p1[1] - p0[1]) * tt - 4
        post = (np.abs(x - px) < 2.2) & (y > py - 22) & (y < py)
        img[post, :3] = wood * 0.8
        img[post, 3] = 1
    for off in (8, 16):
        tt = (x - p0[0]) / (p1[0] - p0[0])
        ey = p0[1] + tt * (p1[1] - p0[1]) - 4 - off
        rail = (tt > 0.04) & (tt < 0.96) & (np.abs(y - ey) < 1.6)
        img[rail, :3] = wood
        img[rail, 3] = 1
    return img


def hh33_objects():
    """Things that stand on a cell, 96 x 160 each, bottom-aligned: drawn in
    the sprite pass and sorted by their cell, so a villager walks behind or
    in front of them. Two cottages, two trees, a well, a fence."""
    rng = np.random.default_rng(330)
    H = 160
    tiles = [
        _hh_house(rng, (0.86, 0.80, 0.68), (0.62, 0.26, 0.18), H),
        _hh_house(rng, (0.80, 0.72, 0.60), (0.30, 0.38, 0.52), H),
        _hh_tree(rng, H, (0.30, 0.52, 0.22)),
        _hh_tree(rng, H, (0.42, 0.55, 0.20)),
        _hh_well(H),
        _hh_fence(H),
    ]
    sheet = np.concatenate(tiles, axis=1)
    save_rgba(OUT / "hamlet" / "objects.png", sheet[..., :3], sheet[..., 3])


def hh33_villager():
    """Villagers, 32 x 48, feet at the bottom, facing right: a hood, a
    tunic and a walking stride (two frames side by side) - three tunics."""
    for k, tunic in enumerate(((0.75, 0.22, 0.18), (0.20, 0.42, 0.70), (0.85, 0.65, 0.20))):
        _hh_villager(k, np.array(tunic))


def _hh_villager(k, tunic):
    W, H = 32, 48
    frames = []
    for f in range(2):
        img = np.zeros((H, W, 4))
        y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
        sh = (((x - 16) / 11) ** 2 + ((y - 45) / 3) ** 2) < 1
        img[sh, 3] = 0.35
        stride = 4 if f == 0 else -4
        for lx in (16 - stride / 2, 16 + stride / 2):
            leg = (np.abs(x - lx) < 2.2) & (y > 32) & (y < 45)
            img[leg, :3] = np.array([0.28, 0.22, 0.18])
            img[leg, 3] = 1
        body = (((x - 16) / 8) ** 2 + ((y - 26) / 10) ** 2) < 1
        img[body, :3] = tunic * (0.8 + 0.2 * (x[body] < 16))[:, None]
        img[body, 3] = 1
        head = (((x - 17) / 5.5) ** 2 + ((y - 12) / 6) ** 2) < 1
        img[head, :3] = np.array([0.92, 0.74, 0.60])
        img[head, 3] = 1
        hood = head & (x < 17.5) | ((((x - 16) / 6.5) ** 2 + ((y - 10) / 6) ** 2) < 1) & (y < 9)
        img[hood, :3] = np.array([0.35, 0.28, 0.22])
        img[hood, 3] = 1
        frames.append(img)
    sheet = np.concatenate(frames, axis=1)
    save_rgba(OUT / "hamlet" / f"villager_{k}.png", sheet[..., :3], sheet[..., 3])


def hh33_cursor():
    """The picked cell: a diamond outline."""
    s, t = _hh_diamond_st(HH_W, HH_H)
    m = np.minimum(np.minimum(s, 1 - s), np.minimum(t, 1 - t))
    a = smoothstep(-0.02, 0.0, m) * (1 - smoothstep(0.02, 0.05, m))
    save_rgba(OUT / "hamlet" / "cursor.png", 1.0, a)



# ── 34 Crater Isle ─────────────────────────────────────────────────────
# A cartoon island blasted a crater at a time. The island is one carvable
# sprite drawn at one texel a unit; `CI_SURFACE` is its top, shared with
# gen_scenes so the crates are put down on it.

CI_W, CI_H = 1280, 400          # the island texture and sprite, 1:1
CI_SEA_TEX = 64


def ci_surface(x):
    """The island's top, in texels from the top of the image (y down), at
    texel column `x` - or None where there is no island (the sea)."""
    u = x / CI_W
    if u < 0.06 or u > 0.94:
        return None
    # Shoulders down to the sea at either end, two hills and a dip between.
    edge = min(u - 0.06, 0.94 - u) / 0.16
    shoulder = 1.0 - min(edge, 1.0) ** 0.6
    hills = 0.5 + 0.22 * math.sin(u * 9.0 + 0.6) + 0.12 * math.sin(u * 23.0 + 1.3)
    top = 70 + (1.0 - hills) * 110
    return top + shoulder * (CI_H - 120 - top)


def ci34_island():
    rng = np.random.default_rng(34)
    H, W = CI_H, CI_W
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    top = np.array([ci_surface(c + 0.5) if ci_surface(c + 0.5) is not None else H * 2 for c in range(W)])
    depth = y - top[None, :]                       # texels below the surface
    inside = depth >= 0
    # Soil: warm brown, darkening with depth, in wavy strata, with pebbles.
    strata = np.sin(y * 0.11 + np.sin(x * 0.013) * 3.0 + np.sin(x * 0.041)) * 0.5 + 0.5
    soil = np.array([0.55, 0.36, 0.20])[None, None, :] * (0.95 - 0.35 * np.clip(depth / 260.0, 0, 1))[..., None]
    soil = soil * (0.88 + 0.12 * strata[..., None])
    n = np.zeros((H, W))
    for cells, amp in ((40, 0.5), (90, 0.3), (200, 0.2)):
        g = rng.random((H // cells + 2, W // cells + 2))
        n += amp * np.kron(g, np.ones((cells, cells)))[:H, :W]
    soil = soil * (0.9 + 0.2 * n[..., None])
    pebbles = rng.random((H, W)) > 0.9988
    pebble = np.zeros((H, W))
    for py, px in zip(*np.nonzero(pebbles)):
        r = rng.uniform(2.0, 5.0)
        y0, y1, x0, x1 = int(max(py - r - 1, 0)), int(min(py + r + 2, H)), int(max(px - r - 1, 0)), int(min(px + r + 2, W))
        yy, xx = np.mgrid[y0:y1, x0:x1]
        pebble[y0:y1, x0:x1] = np.maximum(pebble[y0:y1, x0:x1], (((yy - py) ** 2 + (xx - px) ** 2) < r * r) * 1.0)
    rgb = soil * (1 - pebble[..., None]) + np.array([0.62, 0.58, 0.52]) * pebble[..., None] * (0.8 + 0.2 * n[..., None])
    # Grass: a bright crust, a darker lip under it with tufts hanging down.
    tuft = 20 + 7 * np.sin(x * 0.35) * np.sin(x * 0.11 + 1.0)
    grass = (depth < tuft) & inside
    shade = np.clip(depth / tuft, 0, 1)
    grass_rgb = np.array([0.38, 0.72, 0.25])[None, None, :] * (1.05 - 0.35 * shade[..., None]) * (0.92 + 0.12 * n[..., None])
    rgb = np.where(grass[..., None], grass_rgb, rgb)
    # A dark outline along the surface.
    outline = inside & (depth < 3)
    rgb = np.where(outline[..., None], np.array([0.12, 0.22, 0.08]), rgb)
    save_rgba(OUT / "crater" / "island.png", rgb, inside.astype(np.float64))


def ci34_sky():
    t = np.linspace(0.0, 1.0, 256)[:, None] * np.ones((1, 4))
    top = np.array([0.30, 0.55, 0.92])
    bottom = np.array([0.80, 0.90, 0.98])
    rgb = top[None, None, :] * (1 - t[..., None]) + bottom[None, None, :] * t[..., None]
    save_rgba(OUT / "crater" / "sky.png", rgb, np.ones((256, 4)))


def ci34_sea():
    """A strip of sea that tiles across: a wavy crest with foam, deepening
    downwards."""
    W, H = 256, 160
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    crest = 18 + 6 * np.sin(x / W * 2 * np.pi * 4) + 3 * np.sin(x / W * 2 * np.pi * 9 + 1.0)
    d = y - crest
    a = smoothstep(-1.0, 1.0, d)
    rgb = np.array([0.18, 0.52, 0.78])[None, None, :] * (1.0 - 0.45 * np.clip(d / H, 0, 1))[..., None]
    foam = (d >= 0) & (d < 6)
    rgb = np.where(foam[..., None], np.array([0.92, 0.97, 1.0]), rgb)
    glint = (np.sin(x * 0.21 + y * 0.6) > 0.985) & (d > 14)
    rgb = np.where(glint[..., None], np.array([0.6, 0.85, 1.0]), rgb)
    save_rgba(OUT / "crater" / "sea.png", rgb, a)


def ci34_cloud():
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    a = np.zeros((H, W))
    for cx, cy, r in ((70, 80, 40), (120, 62, 52), (175, 78, 42), (215, 92, 28), (40, 96, 26)):
        a = np.maximum(a, smoothstep(r + 1.5, r - 1.5, np.sqrt((x - cx) ** 2 + (y - cy) ** 2)))
    a = a * (y < 112)
    shade = 0.86 + 0.14 * smoothstep(110, 40, y)
    save_rgba(OUT / "crater" / "cloud.png", np.ones((H, W, 3)) * shade[..., None], a)


def ci34_props():
    # A bomb: a black ball, a highlight, a fuse.
    S = 36
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 18) ** 2 + (y - 20) ** 2)
    a = smoothstep(15.5, 14.5, r)
    hl = smoothstep(5, 1, np.sqrt((x - 13) ** 2 + (y - 15) ** 2))
    rgb = np.array([0.10, 0.10, 0.13])[None, None, :] + hl[..., None] * 0.55
    fuse = (np.abs(x - 22 - (y - 6) * -0.4) < 1.6) & (y < 7) & (y > 1)
    rgb = np.where(fuse[..., None], np.array([0.75, 0.62, 0.35]), rgb)
    a = np.maximum(a, fuse * 1.0)
    save_rgba(OUT / "crater" / "bomb.png", rgb, a)
    # A crate: planks and a frame.
    S = 48
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    wood = np.array([0.72, 0.50, 0.28]) * (0.9 + 0.1 * np.sin(y * 1.3))[..., None]
    frame = (x < 5) | (x > S - 6) | (y < 5) | (y > S - 6) | (np.abs(x - y) < 3.5)
    rgb = np.where(frame[..., None], np.array([0.52, 0.34, 0.18]), wood)
    edge = (x < 1.5) | (x > S - 2.5) | (y < 1.5) | (y > S - 2.5)
    rgb = np.where(edge[..., None], np.array([0.25, 0.15, 0.08]), rgb)
    save_rgba(OUT / "crater" / "crate.png", rgb, np.ones((S, S)))
    # A barrel, seen from the side: round, with hoops.
    S = 40
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 20) ** 2 + (y - 20) ** 2)
    a = smoothstep(19.5, 18.5, r)
    rgb = np.array([0.78, 0.30, 0.18])[None, None, :] * (0.75 + 0.35 * smoothstep(20, 4, x))[..., None]
    hoop = (np.abs(r - 14) < 1.8) | (np.abs(r - 6) < 1.4)
    rgb = np.where(hoop[..., None], np.array([0.30, 0.28, 0.30]), rgb)
    save_rgba(OUT / "crater" / "barrel.png", rgb, a)



# ── 35 Neon Signs ──────────────────────────────────────────────────────

def ns35_board():
    """A chalkboard menu in a wooden frame, 360 x 300."""
    W, H = 360, 300
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    frame = 18
    inner = (x >= frame) & (x < W - frame) & (y >= frame) & (y < H - frame)
    grain = 0.85 + 0.15 * np.sin(x * 0.09 + np.sin(y * 0.05) * 2.0) * np.sin(y * 0.6)
    wood = np.array([0.42, 0.27, 0.15])[None, None, :] * grain[..., None]
    # A slate, dusty with old chalk.
    dust = fbm(256, 8, 4)
    dust = np.array(Image.fromarray((dust * 255).astype(np.uint8)).resize((W, H))) / 255.0
    slate = np.array([0.10, 0.12, 0.11])[None, None, :] * (0.85 + 0.35 * dust[..., None])
    rgb = np.where(inner[..., None], slate, wood)
    bevel = (~inner) & ((x < 3) | (x > W - 4) | (y < 3) | (y > H - 4))
    rgb = np.where(bevel[..., None], wood * 0.6, rgb)
    save_rgba(OUT / "env" / "ns_board.png", rgb, np.ones((H, W)))


def ns35_burst():
    """A cartoon starburst sticker, 280 x 280: sixteen points, a white rim."""
    S = 280
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = x - S / 2, y - S / 2
    ang = np.arctan2(dy, dx)
    r = np.sqrt(dx * dx + dy * dy)
    edge = 112 + 20 * np.cos(ang * 16)
    a = smoothstep(edge + 1.0, edge - 1.0, r)
    rim = (r > edge - 9) & (a > 0)
    rgb = np.where(rim[..., None], np.array([0.98, 0.96, 0.92]),
                   np.array([0.86, 0.13, 0.12]) * (0.92 + 0.08 * (dy < 0))[..., None])
    save_rgba(OUT / "env" / "ns_burst.png", rgb, a)


def ns35_card():
    """A paper price card with a punched hole, 220 x 120."""
    W, H = 220, 120
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    r = 10
    cx = np.clip(x, r, W - r)
    cy = np.clip(y, r, H - r)
    a = smoothstep(r + 0.8, r - 0.8, np.sqrt((x - cx) ** 2 + (y - cy) ** 2))
    hole = np.sqrt((x - 20) ** 2 + (y - H / 2) ** 2) < 6
    a = a * (~hole)
    paper = np.array([0.95, 0.90, 0.78]) * (0.95 + 0.05 * np.sin(x * 0.4) * np.sin(y * 0.3))[..., None]
    save_rgba(OUT / "env" / "ns_card.png", paper, a)



# ── 36 Rolling Hills ───────────────────────────────────────────────────

def rh36_grass():
    """The edge strip: a band of grass whose blades stand above the line and
    a dark, rooty lip under it, 256 x 64, tiling left to right. v 0 is the
    outer (upper) side, which the spline shape puts outside the fill."""
    W, H = 256, 64
    rng = np.random.default_rng(36)
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    base_line = 26.0                       # where the turf starts, top down
    a = np.zeros((H, W))
    rgb = np.zeros((H, W, 3))
    # Turf: green, darkening towards the soil.
    turf = (y >= base_line) & (y < 50)
    depth = np.clip((y - base_line) / 24.0, 0, 1)
    green = np.array([0.34, 0.62, 0.20])[None, None, :] * (1.05 - 0.45 * depth[..., None])
    rgb = np.where(turf[..., None], green, rgb)
    a = np.where(turf, 1.0, a)
    # A rooty lip that fades into the fill below.
    lip = y >= 50
    rgb = np.where(lip[..., None], np.array([0.30, 0.20, 0.11]), rgb)
    a = np.where(lip, np.clip(1.0 - (y - 50) / 14.0, 0, 1), a)
    # Blades above the turf: thin wedges, tiling (x wraps).
    for _ in range(170):
        bx = rng.uniform(0, W)
        h = rng.uniform(8, 24)
        lean = rng.uniform(-5, 5)
        w0 = rng.uniform(1.6, 3.0)
        t = np.clip((base_line - y) / h, 0, 1)
        cx = bx + lean * t
        for shift in (-W, 0, W):
            d = np.abs(x - (cx + shift))
            m = (y <= base_line + 1) & (y >= base_line - h) & (d < w0 * (1 - t) + 0.4)
            shade = 0.75 + 0.35 * t + rng.uniform(-0.08, 0.08)
            col = np.array([0.36, 0.66, 0.22])[None, None, :] * shade[..., None]
            rgb = np.where(m[..., None], col, rgb)
            a = np.where(m, 1.0, a)
    save_rgba(OUT / "env" / "rh_grass.png", rgb, a)


def rh36_soil():
    """The fill: earth, tileable 256 x 256, with buried stones and a few
    roots, darker than the grass so the edge reads."""
    S = 256
    rng = np.random.default_rng(361)
    n = fbm(S, 6, 5)
    rgb = np.array([0.42, 0.28, 0.16])[None, None, :] * (0.78 + 0.35 * n[..., None])
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    for _ in range(14):
        cx, cy, r = rng.uniform(0, S), rng.uniform(0, S), rng.uniform(3, 8)
        for ox in (-S, 0, S):
            for oy in (-S, 0, S):
                d = np.sqrt(((x - cx - ox) / 1.3) ** 2 + (y - cy - oy) ** 2)
                m = d < r
                stone = np.array([0.50, 0.42, 0.34]) * (0.85 + 0.2 * (y < cy + oy))[..., None]
                rgb = np.where(m[..., None], stone, rgb)
                rim = (d >= r) & (d < r + 1.6)
                rgb = np.where(rim[..., None], rgb * 0.8, rgb)
    save_rgba(OUT / "env" / "rh_soil.png", rgb, np.ones((S, S)))


def rh36_ball():
    """The rolling wheel: a wooden cart wheel, 64 x 64, spokes to show it turn."""
    S = 64
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = x - S / 2, y - S / 2
    r = np.sqrt(dx * dx + dy * dy)
    ang = np.arctan2(dy, dx)
    a = smoothstep(31.5, 30.5, r)
    rim = (r > 24) & (r < 31)
    hub = r < 7
    spoke = (np.abs(np.sin(ang * 3)) < 0.18) & (r < 25)
    wood = np.array([0.55, 0.36, 0.20])
    rgb = np.zeros((S, S, 3)) + wood * 0.3
    rgb = np.where(spoke[..., None], wood, rgb)
    rgb = np.where(rim[..., None], wood * 0.85, rgb)
    rgb = np.where(((r > 29) & (r < 31))[..., None], np.array([0.25, 0.25, 0.27]), rgb)   # iron tyre
    rgb = np.where(hub[..., None], np.array([0.30, 0.30, 0.32]), rgb)
    a = a * ((r > 24) | spoke | hub).astype(np.float64)
    save_rgba(OUT / "env" / "rh_wheel.png", rgb, a)



# ── 37 Harbour at Night ────────────────────────────────────────────────

def hb37_boat(name, w, h, hull, cabin, mast, funnel=False):
    """A little boat side on: a hull curving up at bow and stern, a cabin,
    a mast if asked; `w` x `h` with the waterline about a third up."""
    img = np.zeros((h, w, 4))
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    u = x / w
    deck = h * 0.55
    keel = h * 0.98
    # The hull: between the deck line (which rises at the ends) and a keel
    # rounding up to it.
    top = deck - (np.abs(u - 0.5) * 2) ** 3 * h * 0.18
    bottom = keel - (np.abs(u - 0.5) * 2) ** 2 * (keel - deck) * 0.95
    hullm = (y >= top) & (y <= bottom)
    shade = 0.75 + 0.35 * (y - top) / np.maximum(bottom - top, 1)
    img[hullm, :3] = (np.array(hull)[None, :] * (1.2 - 0.5 * (shade[hullm, None] - 0.75)))
    stripe = hullm & (np.abs(y - (top + 6)) < 2.2)
    img[stripe, :3] = np.array([0.92, 0.9, 0.85])
    img[hullm, 3] = 1
    if cabin:
        cx0, cx1 = w * 0.42, w * 0.72
        cy0 = deck - h * 0.32
        cab = (x > cx0) & (x < cx1) & (y > cy0) & (y < deck + 1)
        img[cab, :3] = np.array(cabin)
        img[cab, 3] = 1
        win = (x > cx0 + 6) & (x < cx0 + 16) & (y > cy0 + 5) & (y < cy0 + 13)
        win |= (x > cx0 + 22) & (x < cx0 + 32) & (y > cy0 + 5) & (y < cy0 + 13)
        img[win & cab, :3] = np.array([1.0, 0.82, 0.45])
    if funnel:
        # A stack behind the cabin, black-topped, for the smoke to leave.
        fx0, fx1 = w * 0.74, w * 0.84
        f = (x > fx0) & (x < fx1) & (y > h * 0.06) & (y < deck - h * 0.28)
        img[f, :3] = np.where((y[f] < h * 0.13)[:, None], np.array([0.08, 0.08, 0.09]), np.array([0.78, 0.62, 0.20]))
        img[f, 3] = 1
    if mast:
        m = (np.abs(x - w * 0.3) < 1.6) & (y > h * 0.02) & (y < deck)
        img[m, :3] = np.array([0.35, 0.25, 0.16])
        img[m, 3] = 1
        flag = (x > w * 0.3) & (x < w * 0.3 + 14) & (y > h * 0.03) & (y < h * 0.03 + 8)
        img[flag, :3] = np.array([0.85, 0.2, 0.15])
        img[flag, 3] = 1
    save_rgba(OUT / "env" / f"{name}.png", img[..., :3], img[..., 3])


def hb37_boats():
    hb37_boat("hb_trawler", 180, 110, (0.70, 0.18, 0.14), (0.88, 0.86, 0.80), True)
    hb37_boat("hb_dinghy", 100, 50, (0.18, 0.42, 0.62), None, False)
    hb37_boat("hb_tug", 150, 100, (0.20, 0.35, 0.22), (0.80, 0.72, 0.30), False, funnel=True)


def hb37_water():
    """The harbour's water seen side on, 256 wide (tiling across) and 512
    deep (drawn once down): darkening with depth, with short faint streaks
    of light near the top - and the surface line on its own, 256 x 24."""
    W, H = 256, 512
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    n = fbm(W, 6, 4)
    n = np.tile(n, (H // n.shape[0] + 1, 1))[:H]
    depth = y / H
    rgb = np.array([0.07, 0.17, 0.30])[None, None, :] * (1.1 - 0.75 * depth[..., None]) * (0.9 + 0.2 * n[..., None])
    rng = np.random.default_rng(37)
    for _ in range(46):
        cy = rng.uniform(4, 150) ** 1.0
        cx = rng.uniform(0, W)
        length = rng.uniform(10, 34) * (1.0 - cy / 200)
        dx = (x - cx + W / 2) % W - W / 2
        streak = np.clip(1 - np.abs(dx) / length, 0, 1) * np.clip(1 - np.abs(y - cy) / 1.2, 0, 1)
        k = 0.22 * (1 - cy / 170)
        rgb = rgb + streak[..., None] * np.array([0.35, 0.45, 0.6]) * k
    save_rgba(OUT / "env" / "hb_water.png", np.clip(rgb, 0, 1), np.full((H, W), 0.84))
    S = 24
    y, x = np.mgrid[0:S, 0:W].astype(np.float64)
    crest = 10 + 3 * np.sin(x / W * 2 * np.pi * 3) + 1.5 * np.sin(x / W * 2 * np.pi * 7 + 1)
    a = smoothstep(crest - 1.2, crest + 0.4, y) * (1 - smoothstep(crest + 2.5, crest + 5, y))
    save_rgba(OUT / "env" / "hb_surface.png", np.ones((S, W, 3)) * np.array([0.6, 0.75, 0.9]), a)


def hb37_pier():
    """Pier planks side on, 256 x 40, and a post, 24 x 200."""
    W, H = 256, 40
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    plank = (np.floor(x / 32) % 2)
    wood = np.array([0.36, 0.25, 0.16])[None, None, :] * (0.85 + 0.12 * plank[..., None]) * (0.9 + 0.1 * np.sin(y * 0.9))[..., None]
    wood = np.where(((x % 32) < 1.5)[..., None], wood * 0.5, wood)
    save_rgba(OUT / "env" / "hb_planks.png", wood, np.ones((H, W)))
    W, H = 24, 200
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    post = np.array([0.30, 0.21, 0.14])[None, None, :] * (0.7 + 0.5 * np.sin((x + 0.5) / W * np.pi))[..., None]
    save_rgba(OUT / "env" / "hb_post.png", post, np.ones((H, W)))
    # The quay: dressed stone in courses, 128 x 128, tiling.
    W = H = 128
    y, x = np.mgrid[0:W, 0:H].astype(np.float64)
    course = np.floor(y / 32)
    bx = (x + (course % 2) * 32) % 64
    joint = ((y % 32) < 2) | (bx < 2)
    n = fbm(W, 4, 3)
    stone = np.array([0.30, 0.30, 0.34])[None, None, :] * (0.8 + 0.3 * n[..., None]) * (0.9 + 0.1 * ((course * 7 + np.floor((x + (course % 2) * 32) / 64) * 3) % 3)[..., None] / 2)
    stone = np.where(joint[..., None], stone * 0.45, stone)
    save_rgba(OUT / "env" / "hb_quay.png", stone, np.ones((H, W)))


def hb37_fan():
    """A fan's blades, 96 x 96 (it turns), and its housing ring."""
    S = 96
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = x - S / 2, y - S / 2
    r = np.sqrt(dx * dx + dy * dy)
    ang = np.arctan2(dy, dx)
    blade = (np.cos(ang * 4 + r * 0.04) > 0.55) & (r < 42) & (r > 6)
    hub = r < 8
    a = (blade | hub).astype(np.float64)
    rgb = np.where(hub[..., None], np.array([0.3, 0.3, 0.32]), np.array([0.75, 0.78, 0.8]) * (0.8 + 0.2 * (dx < 0))[..., None])
    save_rgba(OUT / "env" / "hb_fan.png", rgb, a)
    ring = smoothstep(46.5, 45.0, r) * smoothstep(41.0, 42.5, r)
    save_rgba(OUT / "env" / "hb_fan_ring.png", np.ones((S, S, 3)) * 0.45, ring)



# ── 38 Hex Strategy Board ──────────────────────────────────────────────
# Pointy-topped hexagons, 80 wide and 92 tall (a regular hexagon of radius
# 46), as the scene's tilemap has them.

HX_W, HX_H = 80, 92


def _hx_mask(w, h, inset=0.0):
    """A pointy-topped hexagon filling a w x h box: (inside 0..1 soft, the
    distance in from its edge in pixels)."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    dx = np.abs(x - w / 2)
    dy = np.abs(y - h / 2)
    # The upright sides at |dx| = w/2; the slanted ones meet at the points.
    half_side = h / 4
    slant = (h / 2 - half_side) / (w / 2)
    d_side = w / 2 - dx
    d_slant = ((h / 2) - dy - slant * dx) / np.sqrt(1 + slant * slant)
    d = np.minimum(d_side, d_slant) - inset
    return np.clip(d + 0.5, 0, 1), d


def _hx_ground_tile(rng, base, speck, kind):
    w, h = HX_W, HX_H
    inside, d = _hx_mask(w, h, 1.0)
    n = fbm_rect(w, h, 5, 4)
    rgb = np.array(base)[None, None, :] * (0.88 + 0.24 * n[..., None])
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    if kind == "grass":
        tufts = rng.random((h, w)) < 0.018
        rgb[tufts] = np.array(speck)
    elif kind == "sand":
        rgb = rgb + (np.sin(x * 0.35 + np.sin(y * 0.2) * 2) > 0.9)[..., None] * 0.05
    elif kind == "water":
        waves = (np.sin(x * 0.28 + y * 0.1 + np.sin(y * 0.25) * 1.5) > 0.86) & (np.sin(y * 0.5) > 0.2)
        rgb = np.where(waves[..., None], np.array(speck), rgb)
    elif kind == "hills":
        for _ in range(3):
            cx, cy, r = rng.uniform(18, 62), rng.uniform(30, 70), rng.uniform(10, 16)
            mound = np.clip(1 - ((x - cx) / r) ** 2 - ((y - cy) / (r * 0.6)) ** 2, 0, 1)
            light = np.clip(1 - ((x - cx + r * 0.3) / r) ** 2 - ((y - cy + r * 0.2) / (r * 0.5)) ** 2, 0, 1)
            rgb = rgb * (1 - 0.25 * mound[..., None]) + light[..., None] * 0.08
    # A bevel: lighter towards the top-left edge, darker at the bottom right,
    # and a dark rim - a board piece rather than a flat tile.
    rim = smoothstep(0.0, 4.0, d)
    tilt = ((x - w / 2) * -0.6 + (y - h / 2) * -0.8) / (h / 2)
    edge = 1 - smoothstep(0.0, 7.0, d)
    rgb = rgb * (1 + 0.18 * edge[..., None] * tilt[..., None])
    rgb = rgb * (0.55 + 0.45 * rim[..., None])
    return np.concatenate([np.clip(rgb, 0, 1), inside[..., None]], axis=-1)


def hx38_ground():
    """Ground hexagons in one sheet: grass, grass with flowers, sand, water,
    forest floor, hills."""
    rng = np.random.default_rng(38)
    tiles = [
        _hx_ground_tile(rng, (0.36, 0.58, 0.24), (0.55, 0.75, 0.3), "grass"),
        _hx_ground_tile(rng, (0.40, 0.62, 0.26), (0.95, 0.85, 0.4), "grass"),
        _hx_ground_tile(rng, (0.86, 0.76, 0.52), None, "sand"),
        _hx_ground_tile(rng, (0.16, 0.38, 0.62), (0.55, 0.75, 0.9), "water"),
        _hx_ground_tile(rng, (0.24, 0.40, 0.18), (0.32, 0.5, 0.2), "grass"),
        _hx_ground_tile(rng, (0.56, 0.58, 0.32), None, "hills"),
    ]
    sheet = np.concatenate(tiles, axis=1)
    save_rgba(OUT / "hex" / "ground.png", sheet[..., :3], sheet[..., 3])


HX_TALL = 140


def _hx_pine(img, cx, base, height, colour):
    h, w = img.shape[:2]
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    trunk = (np.abs(x - cx) < 2.0) & (y > base - 8) & (y < base)
    img[trunk, :3] = np.array([0.35, 0.24, 0.14])
    img[trunk, 3] = 1
    for k, (frac, wid) in enumerate(((0.0, 1.0), (0.32, 0.8), (0.6, 0.55))):
        top = base - 6 - height * (frac + 0.42)
        bot = base - 6 - height * frac
        half = wid * height * 0.32 * (y - top) / max(bot - top, 1)
        cone = (y > top) & (y < bot) & (np.abs(x - cx) < half)
        shade = 0.85 + 0.3 * (x < cx)
        img[cone, :3] = np.array(colour)[None, :] * shade[cone][:, None] * (0.9 + 0.1 * k)
        img[cone, 3] = 1


def _hx_shadow(img, cx, cy, rx, ry, a=0.3):
    h, w = img.shape[:2]
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    m = (((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2) < 1
    m &= img[..., 3] < 0.5
    img[m, :3] = 0
    img[m, 3] = a


def hx38_objects():
    """What stands on a hexagon, 80 x 140, standing on the bottom of the
    hexagon's box (the image's bottom 92 rows are the hexagon): a stand of
    pines, a mountain, a village."""
    rng = np.random.default_rng(381)
    w, h = HX_W, HX_TALL
    foot = h - HX_H / 2          # the hexagon's centre row
    tiles = []
    # Forest
    img = np.zeros((h, w, 4))
    _hx_shadow(img, 40, foot + 6, 30, 12)
    for cx, by, ht in sorted([(26, foot - 4, 46), (52, foot - 8, 52), (38, foot + 10, 44), (62, foot + 8, 40),
                              (18, foot + 12, 38)], key=lambda t: t[1]):
        _hx_pine(img, cx, by, ht, (0.16 + rng.uniform(0, 0.05), 0.36 + rng.uniform(0, 0.08), 0.18))
    tiles.append(img)
    # Mountain
    img = np.zeros((h, w, 4))
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    _hx_shadow(img, 40, foot + 10, 36, 13, 0.35)
    peak_x, peak_y, base_y = 38, 30, foot + 16
    half = (y - peak_y) / (base_y - peak_y) * 37
    body = (y > peak_y) & (y < base_y) & (np.abs(x - peak_x) < half)
    n = fbm_rect(w, h, 6, 3)
    lit = x < peak_x + (y - peak_y) * 0.15
    rgb = np.where(lit[..., None], np.array([0.62, 0.58, 0.55]), np.array([0.40, 0.37, 0.36])) * (0.9 + 0.2 * n[..., None])
    snow = body & (y < peak_y + 22 + 4 * np.sin(x * 0.6))
    img[body, :3] = rgb[body]
    img[snow, :3] = np.where(lit[snow][:, None], np.array([0.96, 0.97, 1.0]), np.array([0.78, 0.82, 0.9]))
    img[body, 3] = 1
    tiles.append(img)
    # Village: three little houses.
    img = np.zeros((h, w, 4))
    _hx_shadow(img, 40, foot + 6, 32, 12)
    for hx, hy, wall, roof in ((24, foot - 2, (0.86, 0.80, 0.66), (0.66, 0.26, 0.18)),
                               (54, foot - 6, (0.82, 0.76, 0.62), (0.32, 0.40, 0.58)),
                               (40, foot + 14, (0.88, 0.82, 0.70), (0.66, 0.30, 0.20))):
        body = (np.abs(x - hx) < 11) & (y > hy - 14) & (y < hy)
        img[body, :3] = np.array(wall) * (0.85 + 0.15 * (x[body] < hx))[:, None]
        img[body, 3] = 1
        roofm = (y > hy - 28) & (y <= hy - 13) & (np.abs(x - hx) < (y - (hy - 28)) * 0.95)
        img[roofm, :3] = np.array(roof) * (0.8 + 0.25 * (x[roofm] < hx))[:, None]
        img[roofm, 3] = 1
        door = (np.abs(x - hx) < 2.5) & (y > hy - 7) & (y < hy)
        img[door, :3] = np.array([0.3, 0.2, 0.12])
        win = (np.abs(x - hx + 6) < 2) & (np.abs(y - (hy - 8)) < 2)
        img[win, :3] = np.array([1.0, 0.85, 0.45])
    tiles.append(img)
    sheet = np.concatenate(tiles, axis=1)
    save_rgba(OUT / "hex" / "objects.png", sheet[..., :3], sheet[..., 3])


def hx38_marks():
    """Overlays a hexagon at a time, white so the tilemap tints nothing and
    the art says what each is: where a unit can go (a soft wash and a rim),
    where it can strike (red), a step of its path (a dot), the selected
    unit's hexagon (a gold ring)."""
    w, h = HX_W, HX_H
    inside, d = _hx_mask(w, h, 3.0)
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    rim = smoothstep(0.0, 1.5, d) * (1 - smoothstep(2.5, 4.5, d))
    tiles = []
    move = np.zeros((h, w, 4))
    move[..., :3] = np.array([0.85, 0.95, 1.0])
    move[..., 3] = np.maximum(inside * 0.22, rim * 0.85)
    tiles.append(move)
    attack = np.zeros((h, w, 4))
    attack[..., :3] = np.array([1.0, 0.3, 0.25])
    attack[..., 3] = np.maximum(inside * 0.3, rim * 0.95)
    tiles.append(attack)
    path = np.zeros((h, w, 4))
    r = np.sqrt((x - w / 2) ** 2 + (y - h / 2) ** 2)
    path[..., :3] = np.array([1.0, 0.95, 0.75])
    path[..., 3] = np.maximum(smoothstep(9.5, 8.0, r), move[..., 3] * 0.6)
    tiles.append(path)
    sel = np.zeros((h, w, 4))
    sel[..., :3] = np.array([1.0, 0.85, 0.35])
    sel[..., 3] = smoothstep(0.0, 1.5, d) * (1 - smoothstep(4.5, 6.5, d))
    tiles.append(sel)
    sheet = np.concatenate(tiles, axis=1)
    save_rgba(OUT / "hex" / "marks.png", sheet[..., :3], sheet[..., 3])


def _hx_unit(team, kind):
    """A board piece, 56 x 72, standing on its base at the bottom: a round
    base in the team's colour, a figure, and an icon on its shield."""
    w, h = 56, 72
    img = np.zeros((h, w, 4))
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    col = np.array(team)
    # Shadow and base.
    sh = (((x - 28) / 22) ** 2 + ((y - 66) / 6) ** 2) < 1
    img[sh, 3] = 0.35
    base = (((x - 28) / 18) ** 2 + ((y - 62) / 6.5) ** 2) < 1
    img[base, :3] = col * 0.65
    img[base, 3] = 1
    top = (((x - 28) / 18) ** 2 + ((y - 59) / 5.5) ** 2) < 1
    img[top, :3] = col * 0.95
    # The figure: a body and a head.
    bodym = (((x - 28) / 11) ** 2 + ((y - 42) / 15) ** 2) < 1
    img[bodym, :3] = col * (0.75 + 0.3 * (x[bodym] < 28))[:, None]
    img[bodym, 3] = 1
    head = (((x - 28) / 7) ** 2 + ((y - 20) / 7) ** 2) < 1
    skin = np.array([0.92, 0.78, 0.64])
    if kind == "knight":
        helm = head
        img[helm, :3] = np.array([0.78, 0.8, 0.84]) * (0.8 + 0.25 * (x[helm] < 28))[:, None]
        visor = (np.abs(y - 20) < 1.2) & (np.abs(x - 29) < 5)
        img[visor, :3] = 0.15
        plume = (((x - 28) / 2.5) ** 2 + ((y - 11) / 4) ** 2) < 1
        img[plume, :3] = col * 1.1
        img[plume, 3] = 1
    else:
        img[head, :3] = skin * (0.85 + 0.2 * (x[head] < 28))[:, None]
        hood = head & (y < 19)
        img[hood, :3] = col * 0.6
    img[head, 3] = 1
    # A shield (knight), a bow (archer), a lance and pennant (rider).
    if kind == "knight":
        sh = (np.abs(x - 22) < 7) & (y > 34) & (y < 50) & ((y < 44) | (np.abs(x - 22) < (50 - y) * 0.9))
        img[sh, :3] = np.array([0.92, 0.9, 0.84])
        cross = sh & ((np.abs(x - 22) < 1.5) | (np.abs(y - 40) < 1.5))
        img[cross, :3] = col * 0.8
        img[sh, 3] = 1
        blade = (np.abs(x - 40) < 1.3) & (y > 22) & (y < 50)
        img[blade, :3] = np.array([0.85, 0.87, 0.9])
        img[blade, 3] = 1
    elif kind == "archer":
        r = np.sqrt((x - 30) ** 2 + (y - 40) ** 2)
        bow = (np.abs(r - 15) < 1.4) & (x > 36)
        img[bow, :3] = np.array([0.5, 0.33, 0.18])
        img[bow, 3] = 1
        string = (np.abs(x - 37.5) < 0.6) & (np.abs(y - 40) < 13)
        img[string, :3] = 0.9
        img[string, 3] = 1
    else:
        lance = (np.abs((x - 14) - (y - 12) * 0.25) < 1.2) & (y > 4) & (y < 56)
        img[lance, :3] = np.array([0.6, 0.45, 0.28])
        img[lance, 3] = 1
        pen = (x > 15) & (x < 25) & (y > 6) & (y < 6 + (25 - x) * 0.6)
        img[pen, :3] = col * 1.1
        img[pen, 3] = 1
    img[..., :3] = np.clip(img[..., :3], 0, 1)
    return img


def hx38_units():
    """Six pieces in one sheet: knight, archer and rider, blue then red."""
    tiles = [_hx_unit(t, k) for t in ((0.25, 0.45, 0.85), (0.85, 0.25, 0.22))
             for k in ("knight", "archer", "rider")]
    sheet = np.concatenate(tiles, axis=1)
    save_rgba(OUT / "hex" / "units.png", sheet[..., :3], sheet[..., 3])


def hx38_table():
    """The table under the board: dark wood, 256 square, tiling."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    n = fbm(S, 4, 4)
    grain = np.sin(y * 0.18 + n * 6.0 + np.sin(x * 2 * np.pi / S) * 1.5) * 0.5 + 0.5
    plank = (np.floor(y / 64) % 2)
    rgb = np.array([0.30, 0.19, 0.11])[None, None, :] * (0.8 + 0.25 * grain[..., None] + 0.06 * plank[..., None])
    seam = (y % 64) < 1.5
    rgb = np.where(seam[..., None], rgb * 0.5, rgb)
    save_rgba(OUT / "hex" / "table.png", np.clip(rgb, 0, 1), np.ones((S, S)))


# ── 39 Lantern Orchard ─────────────────────────────────────────────────
# Top-down: what stands in the orchard is seen from above, and its shadow
# is the shape of its sprite (`light_occluder.from_alpha`).

def lo39_grass():
    """Night grass seen from above, 256 square, tiling."""
    S = 256
    rng = np.random.default_rng(39)
    n = fbm(S, 5, 4)
    rgb = np.array([0.30, 0.46, 0.22])[None, None, :] * (0.78 + 0.35 * n[..., None])
    blades = rng.random((S, S)) < 0.05
    rgb[blades] *= 1.25
    dark = rng.random((S, S)) < 0.03
    rgb[dark] *= 0.7
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    for _ in range(14):
        cx, cy = rng.uniform(0, S, 2)
        d = np.sqrt(((x - cx + S / 2) % S - S / 2) ** 2 + ((y - cy + S / 2) % S - S / 2) ** 2)
        flower = d < 1.6
        rgb[flower] = np.array([0.95, 0.9, 0.7]) if rng.random() < 0.5 else np.array([0.85, 0.55, 0.75])
    save_rgba(OUT / "orchard" / "grass.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def _lo_blobs(S, blobs, rng, rough=0.12):
    """A union of round clumps with a leafy, noisy edge: (alpha, height)."""
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    n = fbm(S, 8, 3)
    height = np.zeros((S, S))
    for cx, cy, r in blobs:
        d = np.sqrt((x - cx) ** 2 + (y - cy) ** 2) / r
        d = d * (1 + rough * (n - 0.5) * 2)
        height = np.maximum(height, np.clip(1 - d, 0, 1))
    return (height > 0.02).astype(float), height


def _lo_tree(rng, S, blobs, leaf, apples=True):
    alpha, height = _lo_blobs(S, blobs, rng)
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    n = fbm(S, 16, 3)
    # Lit from nowhere in particular: the dome reads by its height, and the
    # leaves by noise.
    shade = 0.55 + 0.55 * np.sqrt(height) + 0.25 * (n - 0.5)
    rgb = np.array(leaf)[None, None, :] * shade[..., None]
    if apples:
        for _ in range(int(S * 0.12)):
            ax, ay = rng.uniform(0, S, 2)
            ix, iy = int(ax), int(ay)
            if alpha[iy, ix] > 0 and height[iy, ix] > 0.15:
                m = (x - ax) ** 2 + (y - ay) ** 2 < 9
                rgb[m] = np.array([0.85, 0.18, 0.12]) * (0.8 + 0.4 * (y[m] < ay))[:, None]
    return np.concatenate([np.clip(rgb, 0, 1), alpha[..., None]], axis=-1)


def lo39_trees():
    """Three apple trees from above, 256 square each: crowns of clumps,
    some apart from the rest - so their shadows have gaps in them."""
    rng = np.random.default_rng(391)
    S = 256
    crowns = [
        [(128, 128, 70), (88, 100, 46), (170, 96, 44), (150, 170, 48), (90, 168, 40), (206, 190, 24)],
        [(120, 130, 62), (170, 120, 50), (128, 80, 44), (80, 150, 38), (40, 70, 22)],
        [(130, 120, 56), (95, 160, 42), (175, 160, 40), (140, 70, 40), (210, 60, 20), (50, 210, 18)],
    ]
    for i, blobs in enumerate(crowns):
        img = _lo_tree(rng, S, blobs, (0.22, 0.42, 0.18))
        save_rgba(OUT / "orchard" / f"tree_{i}.png", img[..., :3], img[..., 3])


def lo39_fence():
    """A run of fence from above, 320 x 40: posts with gaps, joined by a
    thin rail that light passes over the top of - the rail is left out of
    the alpha's solid part by being translucent."""
    W, H = 320, 40
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    img = np.zeros((H, W, 4))
    rail = np.abs(y - 20) < 2.5
    img[rail, :3] = np.array([0.45, 0.33, 0.22])
    img[rail, 3] = 0.45
    for k in range(9):
        px = 16 + k * 36
        post = (np.abs(x - px) < 8) & (np.abs(y - 20) < 12)
        tone = 0.85 + 0.15 * (x[post] < px)
        img[post, :3] = np.array([0.72, 0.62, 0.48]) * tone[:, None]
        img[post, 3] = 1
        cap = (np.abs(x - px) < 5) & (np.abs(y - 20) < 4)
        img[cap, :3] = np.array([0.82, 0.74, 0.6])
    save_rgba(OUT / "orchard" / "fence.png", img[..., :3], img[..., 3])


def lo39_scarecrow():
    """A scarecrow from above, 160 square: a cross of straw arms, a coat,
    a round hat."""
    S = 160
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    img = np.zeros((S, S, 4))
    arms = (np.abs(y - 80) < 9) & (np.abs(x - 80) < 72)
    img[arms, :3] = np.array([0.55, 0.42, 0.28])
    img[arms, 3] = 1
    for side in (-1, 1):
        straw = (np.abs(y - 80) < 13) & (np.abs(x - (80 + side * 68)) < 8) & (np.sin(y * 1.3 + x * 0.3) > -0.3)
        img[straw, :3] = np.array([0.86, 0.74, 0.38])
        img[straw, 3] = 1
    coat = (((x - 80) / 26) ** 2 + ((y - 84) / 34) ** 2) < 1
    img[coat, :3] = np.array([0.35, 0.30, 0.45]) * (0.85 + 0.2 * (x[coat] < 80))[:, None]
    img[coat, 3] = 1
    r = np.sqrt((x - 80) ** 2 + (y - 78) ** 2)
    brim = r < 26
    img[brim, :3] = np.array([0.62, 0.48, 0.25])
    img[brim, 3] = 1
    crown = r < 15
    img[crown, :3] = np.array([0.72, 0.58, 0.32])
    band = (r > 13) & (r < 16)
    img[band, :3] = np.array([0.55, 0.15, 0.12])
    save_rgba(OUT / "orchard" / "scarecrow.png", img[..., :3], img[..., 3])


def lo39_props():
    """Hay bales (two round, 96), a well (a stone ring, 128) and a cart
    (120 x 72), from above."""
    S = 96
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 48) ** 2 + (y - 48) ** 2)
    a = np.arctan2(y - 48, x - 48)
    bale = r < 44
    swirl = np.sin(r * 0.9 + a * 2) * 0.5 + 0.5
    rgb = np.array([0.80, 0.66, 0.32])[None, None, :] * (0.75 + 0.3 * swirl[..., None]) * (1.05 - 0.3 * (r / 44)[..., None])
    save_rgba(OUT / "orchard" / "bale.png", np.clip(rgb, 0, 1), bale.astype(float))
    S = 128
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 64) ** 2 + (y - 64) ** 2)
    a = np.arctan2(y - 64, x - 64)
    ring = (r > 38) & (r < 60)
    stones = (np.sin(a * 14) > 0.85) | (np.abs(r - 49) < 1.0)
    rgb = np.where(stones[..., None], np.array([0.30, 0.30, 0.32]), np.array([0.58, 0.57, 0.55]))
    water = r <= 38
    rgb = np.where(water[..., None], np.array([0.05, 0.08, 0.12]), rgb)
    save_rgba(OUT / "orchard" / "well.png", rgb, (ring | water).astype(float) * np.where(water, 0.9, 1.0))
    W, H = 120, 72
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    img = np.zeros((H, W, 4))
    bed = (np.abs(x - 52) < 40) & (np.abs(y - 36) < 26)
    planks = (np.floor((y - 10) / 9) % 2)
    img[bed, :3] = np.array([0.50, 0.36, 0.22])[None, :] * (0.85 + 0.12 * planks[bed])[:, None]
    img[bed, 3] = 1
    for wy in (6, 66):
        wheel = (np.abs(x - 52) < 14) & (np.abs(y - wy) < 5)
        img[wheel, :3] = np.array([0.25, 0.18, 0.12])
        img[wheel, 3] = 1
    for hy in (26, 46):
        handle = (x > 92) & (x < 118) & (np.abs(y - hy) < 2.5)
        img[handle, :3] = np.array([0.45, 0.32, 0.2])
        img[handle, 3] = 1
    apples = np.random.default_rng(39).uniform([20, 16], [84, 56], (14, 2))
    for ax, ay in apples:
        m = (x - ax) ** 2 + (y - ay) ** 2 < 25
        img[m, :3] = np.array([0.85, 0.2, 0.12])
    save_rgba(OUT / "orchard" / "cart.png", img[..., :3], img[..., 3])


def lo39_lantern():
    """The lantern from above, 48 square: a dark iron frame round a bright
    glass, which the scene draws unlit."""
    S = 48
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 24) ** 2 + (y - 24) ** 2)
    img = np.zeros((S, S, 4))
    frame = (np.abs(x - 24) < 15) & (np.abs(y - 24) < 15)
    img[frame, :3] = np.array([0.18, 0.16, 0.15])
    img[frame, 3] = 1
    glass = (np.abs(x - 24) < 11) & (np.abs(y - 24) < 11)
    img[glass, :3] = np.array([1.0, 0.85, 0.5]) * (1.1 - 0.3 * (r[glass] / 16))[:, None]
    ring = (np.abs(r - 4) < 2.0)
    img[ring, :3] = np.array([0.25, 0.22, 0.2])
    save_rgba(OUT / "orchard" / "lantern.png", np.clip(img[..., :3], 0, 1), img[..., 3])


# ── 40 Moonlit Study ───────────────────────────────────────────────────
# A study seen from above at night, lit through light cookies: each light's
# picture is drawn here, with the light at the picture's centre.

def ms40_floor():
    """Oak floorboards, 256 square, tiling: boards 32 wide, ends staggered."""
    S = 256
    rng = np.random.default_rng(40)
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    board = np.floor(x / 32)
    n = fbm(S, 4, 4)
    tone = 0.85 + 0.15 * ((board * 7) % 5) / 4
    grain = 0.5 + 0.5 * np.sin(y * 0.09 + n * 7 + board * 2.1)
    rgb = np.array([0.50, 0.33, 0.20])[None, None, :] * (tone * (0.82 + 0.25 * grain))[..., None]
    seam = (x % 32) < 1.2
    ends = np.zeros((S, S), bool)
    for b in range(8):
        off = rng.uniform(0, S)
        ends |= (np.floor(x / 32) == b) & (np.abs(((y - off) % 128)) < 1.2)
    rgb = np.where((seam | ends)[..., None], rgb * 0.45, rgb)
    save_rgba(OUT / "study" / "floor.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def ms40_rug():
    """An oval rug, 420 x 260: a red field, a cream border, a medallion."""
    W, H = 420, 260
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    e = np.sqrt(((x - W / 2) / (W / 2 - 4)) ** 2 + ((y - H / 2) / (H / 2 - 4)) ** 2)
    a = np.arctan2(y - H / 2, x - W / 2)
    rgb = np.zeros((H, W, 3))
    rgb[:] = np.array([0.55, 0.12, 0.10])
    border = (e > 0.82) & (e < 0.93)
    rgb[border] = np.array([0.85, 0.76, 0.55])
    rgb[(e > 0.93)] = np.array([0.30, 0.12, 0.10])
    med = e < 0.42
    petals = (np.cos(a * 8) * 0.5 + 0.5) * 0.25
    rgb[med] = np.array([0.20, 0.25, 0.45])
    rgb[(e < 0.42 * (0.55 + petals))] = np.array([0.85, 0.70, 0.35])
    rgb[(e < 0.12)] = np.array([0.55, 0.12, 0.10])
    dots = (np.sin(x * 0.25) * np.sin(y * 0.25) > 0.85) & (e > 0.45) & (e < 0.8)
    rgb[dots] = np.array([0.80, 0.55, 0.30])
    n = fbm(512, 32, 2)[:H, :W]
    rgb *= (0.88 + 0.2 * n)[..., None]
    save_rgba(OUT / "study" / "rug.png", np.clip(rgb, 0, 1), np.clip((1 - e) * 60, 0, 1))


def _ms_box(W, H, colour, rim=0.6, r=6):
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    d = np.minimum(np.minimum(x, W - x), np.minimum(y, H - y))
    img = np.zeros((H, W, 4))
    img[..., :3] = np.array(colour)[None, None, :] * (rim + (1 - rim) * np.clip(d / r, 0, 1))[..., None]
    img[..., 3] = 1
    return img, x, y


def ms40_furniture():
    """From above: a desk with papers and an open book (240 x 120), an
    armchair (110 x 110), a bookcase (300 x 70) and a potted fern (100)."""
    rng = np.random.default_rng(401)
    img, x, y = _ms_box(240, 120, (0.38, 0.22, 0.12), 0.55, 8)
    for (px, py, w, h, rot) in ((40, 30, 50, 64, 0.2), (150, 70, 46, 60, -0.15)):
        c, s = np.cos(rot), np.sin(rot)
        lx = (x - px) * c + (y - py) * s
        ly = -(x - px) * s + (y - py) * c
        paper = (np.abs(lx) < w / 2) & (np.abs(ly) < h / 2)
        img[paper, :3] = np.array([0.92, 0.9, 0.82])
        lines = paper & ((ly + 30) % 7 < 1.1) & (np.abs(lx) < w / 2 - 6)
        img[lines, :3] = np.array([0.4, 0.4, 0.45])
    book = (np.abs(x - 110) < 34) & (np.abs(y - 40) < 24)
    img[book, :3] = np.array([0.95, 0.92, 0.84])
    spine = book & (np.abs(x - 110) < 1.5)
    img[spine, :3] = np.array([0.4, 0.3, 0.25])
    inkwell = (x - 205) ** 2 + (y - 30) ** 2 < 80
    img[inkwell, :3] = np.array([0.1, 0.1, 0.15])
    save_rgba(OUT / "study" / "desk.png", img[..., :3], img[..., 3])
    # Armchair
    S = 110
    img = np.zeros((S, S, 4))
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    back = (np.abs(x - 55) < 50) & (y > 4) & (y < 34)
    arms = ((np.abs(x - 12) < 10) | (np.abs(x - 98) < 10)) & (y > 20) & (y < 104)
    seat = (np.abs(x - 55) < 34) & (y > 30) & (y < 102)
    for m, c in ((back, (0.30, 0.38, 0.30)), (arms, (0.28, 0.35, 0.28)), (seat, (0.36, 0.45, 0.36))):
        img[m, :3] = np.array(c)
        img[m, 3] = 1
    cush = seat & ((x - 55) ** 2 / 900 + (y - 66) ** 2 / 1100 < 1)
    img[cush, :3] *= 1.12
    save_rgba(OUT / "study" / "armchair.png", np.clip(img[..., :3], 0, 1), img[..., 3])
    # Bookcase: shelves of book tops.
    img, x, y = _ms_box(300, 70, (0.30, 0.18, 0.10), 0.5, 5)
    bx = 8
    while bx < 292:
        w = rng.uniform(6, 13)
        col = np.array(rng.choice([(0.55, 0.15, 0.12), (0.15, 0.30, 0.45), (0.20, 0.38, 0.22), (0.65, 0.55, 0.30),
                                   (0.35, 0.20, 0.35)]))
        m = (x >= bx) & (x < bx + w - 1) & (y > 10) & (y < 10 + rng.uniform(36, 50))
        img[m, :3] = col * (0.8 + 0.3 * rng.random())
        bx += w
    save_rgba(OUT / "study" / "bookcase.png", img[..., :3], img[..., 3])
    # Potted fern
    S = 100
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    img = np.zeros((S, S, 4))
    r = np.sqrt((x - 50) ** 2 + (y - 50) ** 2)
    a = np.arctan2(y - 50, x - 50)
    pot = r < 26
    img[pot, :3] = np.array([0.62, 0.32, 0.20])
    img[pot, 3] = 1
    fronds = (r < 48) & (np.cos(a * 9 + r * 0.05) > 0.2 + r / 80)
    img[fronds, :3] = np.array([0.20, 0.45, 0.20]) * (0.8 + 0.4 * (1 - r[fronds] / 48))[:, None]
    img[fronds, 3] = 1
    save_rgba(OUT / "study" / "fern.png", np.clip(img[..., :3], 0, 1), img[..., 3])


def ms40_walls():
    """A wall seen from above, 256 x 64, tiling across: dark plaster with a
    paler cap; and a window opening in it, 200 x 64 (glass in a frame)."""
    W, H = 256, 64
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    n = fbm(256, 8, 3)[:H]
    rgb = np.array([0.20, 0.18, 0.17])[None, None, :] * (0.85 + 0.25 * n[..., None])
    cap = np.abs(y - 32) < 3
    rgb[cap] *= 1.6
    save_rgba(OUT / "study" / "wall.png", np.clip(rgb, 0, 1), np.ones((H, W)))
    W = 200
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    img = np.zeros((H, W, 3))
    img[:] = np.array([0.35, 0.25, 0.16])
    glass = (x > 10) & (x < W - 10) & (np.abs(y - 32) < 9)
    img[glass] = np.array([0.55, 0.65, 0.85])
    mull = glass & ((np.abs(x - W / 3) < 3) | (np.abs(x - 2 * W / 3) < 3))
    img[mull] = np.array([0.35, 0.25, 0.16])
    save_rgba(OUT / "study" / "window.png", img, np.ones((H, W)))


def _cookie(S=256):
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    return x, y


def ms40_cookies():
    """The lights' pictures, 512 square, each light at the picture's centre
    and +y in the picture running down (away from the wall the window is in):
    the window's panes stretched across the floor, a rose window's colours,
    a lampshade's pool, and a ceiling fan's blades. Every edge is a ramp a
    texel or two wide - a hard-edged mask shows its texels as a staircase
    along any slanted edge, however the light filters it."""
    S = 512
    x, y = _cookie(S)
    c = S / 2
    k = S / 256          # the drawing was made at 256
    edge = lambda d, w=1.0: np.clip(d / (w * k) + 0.5, 0, 1)
    # Moonlight through a window of three panes and a transom, thrown down
    # and to the right, widening and fading with the distance.
    t = np.clip((y - c) / (S - c), 0, 1)
    sx = (x - c - t * 40 * k) / (1 + t * 0.7)
    pane = edge(40 * k - np.abs(sx)) * edge(y - c - 6 * k)
    mull = np.maximum(edge(1.6 * k - np.abs(sx + 13.3 * k)), edge(1.6 * k - np.abs(sx - 13.3 * k)))
    transom = edge(0.018 * (S - c) - np.abs(t - 0.42) * (S - c))
    moon = pane * (1 - mull) * (1 - transom) * (1 - smoothstep(0.75, 1.0, t))
    save_rgba(OUT / "study" / "cookie_window.png", np.ones((S, S, 3)) * moon[..., None], np.ones((S, S)))
    # A rose window, thrown as an ellipse onto the floor below it.
    ex, ey = (x - c) / (64 * k), (y - (c + 66 * k)) / (40 * k)
    r = np.sqrt(ex ** 2 + ey ** 2)
    a = np.arctan2(ey, ex)
    seg = np.floor((a + np.pi) / (2 * np.pi) * 12) % 12
    palette = np.array([(0.9, 0.2, 0.2), (0.95, 0.75, 0.2), (0.2, 0.45, 0.9), (0.3, 0.75, 0.35),
                        (0.75, 0.3, 0.8), (0.95, 0.5, 0.15)])
    rgb = palette[(seg % 6).astype(int)]
    ring = np.floor(r * 3)
    rgb = np.where((ring % 2 == 1)[..., None], rgb[..., ::-1] * 0.9 + 0.1, rgb)
    centre = r < 0.18
    rgb = np.where(centre[..., None], np.array([1.0, 0.95, 0.7]), rgb)
    # Lead between the panes: by angle (in texels along the arc) and by ring.
    arc = np.abs(((a + np.pi) / (2 * np.pi) * 12) % 1 - 0.5) * (2 * np.pi / 12) * r * 52 * k
    ringd = np.abs((r * 3) % 1 - 0.5) * (52 * k / 3)
    lead = np.maximum(edge(arc - (0.5 * 2 * np.pi / 12 * r * 52 * k - 1.6 * k)), edge(ringd - (0.5 * 52 * k / 3 - 1.6 * k)))
    lead = np.where(centre, 0.0, lead)
    rim = edge((1.0 - r) * 40 * k)
    alpha = rim * (1 - lead)
    save_rgba(OUT / "study" / "cookie_rose.png", rgb, alpha)
    # A lampshade: a bright pool, the shade's rim a darker ring, a seam.
    rr = np.sqrt((x - c) ** 2 + (y - c) ** 2) / c
    pool = 1 - smoothstep(0.2, 1.0, rr)
    rim = 1 - 0.55 * np.exp(-((rr - 0.32) / 0.04) ** 2)
    seam = 1 - 0.3 * edge(2 * k - np.abs(x - c)) * (rr < 0.32)
    save_rgba(OUT / "study" / "cookie_lamp.png", np.ones((S, S, 3)) * (pool * rim * seam)[..., None], np.ones((S, S)))
    # A ceiling fan's shadow: four broad blades round a hub.
    a = np.arctan2(y - c, x - c)
    blade = smoothstep(0.5, 0.6, np.cos(4 * a + rr * 1.2)) * edge((rr - 0.08) * c) * edge((0.72 - rr) * c)
    hub = edge((0.1 - rr) * c)
    dark = np.maximum(blade, hub)
    shade = (1 - 0.88 * dark) * (1 - smoothstep(0.85, 1.0, rr))
    save_rgba(OUT / "study" / "cookie_fan.png", np.ones((S, S, 3)) * shade[..., None], np.ones((S, S)))


# ── 41 Waterfall Grotto ────────────────────────────────────────────────
# One 32 px tile sheet, 16 tiles across, for a cave whose water, torches,
# crystals and moss are animated tiles (`animations` on the tileset). Each
# animated family is a base tile (its first frame, what the map names and
# what shows while editing) and four frames that loop without a seam.

GR_T = 32


def _gr_periodic(rng, w, h, terms=6, fx_max=3, fy_max=3):
    """Smooth noise periodic over the tile (integer frequencies), 0..1."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    out = np.zeros((h, w))
    for _ in range(terms):
        fx, fy = rng.integers(0, fx_max + 1), rng.integers(1, fy_max + 1)
        ph = rng.uniform(0, 2 * np.pi)
        out += np.sin(2 * np.pi * (fx * x / w + fy * y / h) + ph) / (fx + fy)
    out -= out.min()
    return out / max(out.max(), 1e-6)


def _gr_rock(rng, base, mortar=True):
    T = GR_T
    y, x = np.mgrid[0:T, 0:T].astype(np.float64) + 0.5
    n = fbm(64, 8, 3)[:T, :T]
    rgb = np.array(base)[None, None, :] * (0.78 + 0.35 * n[..., None])
    if mortar:
        # Two or three stones per tile, edged dark.
        cut_y = rng.integers(11, 21)
        cut_x1, cut_x2 = rng.integers(8, 24), rng.integers(8, 24)
        joint = (np.abs(y - cut_y) < 1.0) | ((y < cut_y) & (np.abs(x - cut_x1) < 1.0)) | \
                ((y > cut_y) & (np.abs(x - cut_x2) < 1.0)) | (x < 1) | (y < 1)
        rgb = np.where(joint[..., None], rgb * 0.45, rgb)
        lit = (np.abs(y - cut_y - 1.5) < 1.0) | (np.abs(y - 1.5) < 1.0)
        rgb = np.where(lit[..., None], rgb * 1.25, rgb)
    return np.concatenate([np.clip(rgb, 0, 1), np.ones((T, T, 1))], axis=-1)


def _gr_frames(fn):
    frames = [fn(k) for k in range(4)]
    return [frames[0]] + frames


def gr41_tiles():
    rng = np.random.default_rng(41)
    T = GR_T
    y, x = np.mgrid[0:T, 0:T].astype(np.float64) + 0.5
    rock = (0.30, 0.27, 0.36)
    tiles = [_gr_rock(rng, rock) for _ in range(3)]
    # Rock with a lip of moss on top (the floor), and with drips below (the ceiling).
    top = _gr_rock(rng, rock)
    moss_h = 5 + 2 * np.sin(x * 0.7) + rng.uniform(0, 1.5, T)[None, :]
    moss = y < moss_h
    top[moss, :3] = np.array([0.28, 0.50, 0.24]) * (0.8 + 0.3 * (y[moss] < 3))[:, None]
    tiles.append(top)
    bottom = _gr_rock(rng, rock)
    for cx in (6, 17, 26):
        drip = (np.abs(x - cx) < (T - y) / 9) & (y > 18)
        bottom[drip, :3] = np.array(rock) * 0.9
    bottom[y > T - 3, :3] *= 0.6
    tiles.append(bottom)
    # The cave's back wall, darker and bluer.
    for _ in range(2):
        t = _gr_rock(rng, (0.13, 0.14, 0.22))
        t[..., :3] *= 0.9
        tiles.append(t)

    water = np.array([0.20, 0.45, 0.70])
    deep = np.array([0.08, 0.20, 0.40])
    fall_base = _gr_periodic(rng, T, T, 8, fx_max=6, fy_max=2)

    # Falling water: bright and dark strands down the tile (a value per
    # column), each with a soft pulse travelling down it - periodic in y, so
    # rolling it a quarter tile a frame loops.
    strands = rng.uniform(0, 1, T)
    strands = np.convolve(np.concatenate([strands, strands[:3]]), np.ones(3) / 3, mode="valid")[:T]
    phase = rng.uniform(0, 1, T)

    def waterfall(k):
        pulse = 0.5 + 0.5 * np.sin(2 * np.pi * ((y - k * 8) / T + phase[None, :]))
        p = 0.55 * strands[None, :] + 0.45 * pulse * strands[None, :]
        rgb = water[None, None, :] * (0.7 + 0.7 * p[..., None]) + (p[..., None] ** 4) * 0.5
        return np.concatenate([np.clip(rgb, 0, 1), np.full((T, T, 1), 0.92)], axis=-1)
    tiles += _gr_frames(waterfall)
    foam_base = _gr_periodic(rng, T, T, 8, fx_max=4, fy_max=2)

    def foam(k):
        p = np.roll(foam_base, k * 8, axis=0)
        p = np.roll(p, k * 3, axis=1)
        rgb = water[None, None, :] * 0.9 + (p[..., None] ** 2) * 0.8
        return np.concatenate([np.clip(rgb, 0, 1), np.ones((T, T, 1)) * 0.95], axis=-1)
    tiles += _gr_frames(foam)

    def surface(k):
        crest = 9 + 2.2 * np.sin(2 * np.pi * (x / T * 2 - k / 4)) + 0.8 * np.sin(2 * np.pi * (x / T * 3 + k / 4))
        body = y > crest
        rgb = np.where(body[..., None], water[None, None, :] * (1.1 - 0.4 * (y / T))[..., None], 0.0)
        line = body & (y < crest + 2)
        rgb[line] = np.array([0.75, 0.9, 1.0])
        return np.concatenate([np.clip(rgb, 0, 1), (body * 0.9)[..., None]], axis=-1)
    tiles += _gr_frames(surface)
    caustic = _gr_periodic(rng, T, T, 6, fx_max=2, fy_max=2)

    def body(k):
        c = np.roll(np.roll(caustic, k * 8, axis=1), k * 8, axis=0)
        glint = np.exp(-((c - 0.5) / 0.06) ** 2)
        rgb = deep[None, None, :] * (0.9 + 0.3 * c[..., None] + 0.6 * glint[..., None])
        return np.concatenate([np.clip(rgb, 0, 1), np.ones((T, T, 1)) * 0.92], axis=-1)
    tiles += _gr_frames(body)

    def torch(k):
        img = np.zeros((T, T, 4))
        bracket = (np.abs(x - 16) < 2.5) & (y > 17) & (y < 30)
        img[bracket, :3] = np.array([0.25, 0.2, 0.18])
        img[bracket, 3] = 1
        cup = (np.abs(x - 16) < 5) & (y > 15) & (y < 19)
        img[cup, :3] = np.array([0.35, 0.28, 0.22])
        img[cup, 3] = 1
        h = (13, 15, 12, 14)[k]
        sway = (0.0, 1.2, -0.9, 0.6)[k]
        fy = (y - (16 - h)) / h
        flame = (fy > 0) & (fy < 1) & (np.abs(x - 16 - sway * (1 - fy)) < 6.0 * np.sin(np.pi * np.clip(fy, 0, 1)) ** 0.7)
        img[flame, :3] = np.array([1.0, 0.55, 0.15])
        img[flame & (np.abs(x - 16 - sway) < 2) & (fy > 0.45), :3] = np.array([1.0, 0.92, 0.6])
        img[flame, 3] = 1
        return img
    tiles += _gr_frames(torch)

    def crystal(k):
        img = np.zeros((T, T, 4))
        glow = 0.75 + 0.25 * np.cos(2 * np.pi * k / 4)
        for cx, h, w in ((10, 18, 4), (17, 26, 5), (24, 15, 4)):
            fy = (T - y) / h
            spike = (fy > 0) & (fy < 1) & (np.abs(x - cx) < w * (1 - fy) + 0.6)
            shade = 0.75 + 0.35 * (x < cx)
            img[spike, :3] = np.array([0.35, 0.85, 1.0]) * glow * shade[spike][:, None]
            img[spike, 3] = 1
        return img
    tiles += _gr_frames(crystal)

    def moss_tuft(k):
        img = np.zeros((T, T, 4))
        for i, cx in enumerate((6, 11, 16, 21, 26)):
            h = 10 + 5 * ((i * 7) % 3)
            lean = np.sin(2 * np.pi * k / 4 + i) * 2.5
            fy = (T - y) / h
            blade = (fy > 0) & (fy < 1) & (np.abs(x - cx - lean * fy ** 2) < 1.6 * (1 - fy) + 0.4)
            img[blade, :3] = np.array([0.30, 0.58, 0.26]) * (0.8 + 0.3 * fy[blade])[:, None]
            img[blade, 3] = 1
        return img
    tiles += _gr_frames(moss_tuft)

    while len(tiles) < 48:
        tiles.append(np.zeros((T, T, 4)))
    rows = [np.concatenate(tiles[r * 16:(r + 1) * 16], axis=1) for r in range(3)]
    sheet = np.concatenate(rows, axis=0)
    save_rgba(OUT / "grotto" / "tiles.png", sheet[..., :3], sheet[..., 3])


# ── 42 Siege Blast ─────────────────────────────────────────────────────

def _sb_block(W, H, base, grain=False, seed=0):
    rng = np.random.default_rng(seed)
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm(max(W, H) * 2, 6, 3)[:H, :W]
    rgb = np.array(base)[None, None, :] * (0.82 + 0.3 * n[..., None])
    if grain:
        g = np.sin(y * 0.9 + np.sin(x * 0.05) * 3 + n * 4)
        rgb *= (0.9 + 0.08 * g)[..., None]
        for k in range(1, 3):
            nail = (np.abs(x - W * k / 3) < 1.6) & ((np.abs(y - 4) < 1.6) | (np.abs(y - (H - 4)) < 1.6))
            rgb[nail] = np.array([0.25, 0.25, 0.28])
    else:
        for _ in range(6):
            cx, cy = rng.uniform(0, W), rng.uniform(0, H)
            pit = (x - cx) ** 2 + (y - cy) ** 2 < rng.uniform(1, 4)
            rgb[pit] *= 0.75
    d = np.minimum(np.minimum(x, W - x), np.minimum(y, H - y))
    bevel = np.clip(d / 3.5, 0, 1)
    light = np.where((y < 3.5) | (x < 3.5), 1.18, np.where((y > H - 3.5) | (x > W - 3.5), 0.7, 1.0))
    rgb = rgb * (0.55 + 0.45 * bevel)[..., None] * light[..., None]
    return np.clip(rgb, 0, 1)


def sb42_blocks():
    save_rgba(OUT / "siege" / "stone.png", _sb_block(64, 32, (0.62, 0.60, 0.56), seed=1), np.ones((32, 64)))
    save_rgba(OUT / "siege" / "stone_tall.png", _sb_block(32, 64, (0.58, 0.57, 0.55), seed=2), np.ones((64, 32)))
    save_rgba(OUT / "siege" / "timber.png", _sb_block(128, 20, (0.55, 0.36, 0.20), grain=True, seed=3), np.ones((20, 128)))


def sb42_scenery():
    """The sky (a tall gradient, 64 x 512), far hills (1024 x 256, tiling
    across) and the ground strip (256 x 128, tiling)."""
    H = 512
    t = np.linspace(0, 1, H)[:, None]
    top, bottom = np.array([0.32, 0.52, 0.82]), np.array([0.95, 0.80, 0.62])
    sky = top[None, :] * (1 - t[..., None]) + bottom[None, :] * t[..., None]
    sky = np.repeat(sky, 64, axis=1)
    save_rgba(OUT / "siege" / "sky.png", sky, np.ones((H, 64)))
    W, H = 1024, 256
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    ridge = 120 + 40 * np.sin(2 * np.pi * x / W * 2) + 22 * np.sin(2 * np.pi * x / W * 5 + 1) + 9 * np.sin(2 * np.pi * x / W * 13)
    near = 170 + 25 * np.sin(2 * np.pi * x / W * 3 + 2) + 12 * np.sin(2 * np.pi * x / W * 9)
    rgb = np.zeros((H, W, 3))
    a = np.zeros((H, W))
    far = y > ridge
    rgb[far] = np.array([0.55, 0.62, 0.75])
    a[far] = 1
    nr = y > near
    rgb[nr] = np.array([0.40, 0.52, 0.45])
    save_rgba(OUT / "siege" / "hills.png", rgb, a)
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    n = fbm(256, 8, 4)[:H]
    soil = np.array([0.42, 0.30, 0.20])[None, None, :] * (0.8 + 0.3 * n[..., None])
    grass_h = 14 + 3 * np.sin(x * 0.3) + 2 * np.sin(x * 0.11)
    grass = y < grass_h
    soil[grass] = np.array([0.36, 0.58, 0.26]) * (0.85 + 0.25 * n[grass])[:, None]
    save_rgba(OUT / "siege" / "ground.png", np.clip(soil, 0, 1), np.ones((H, W)))


def sb42_cannon():
    """A cannon barrel (96 x 40, muzzle to the right, pivot at the left) and
    its carriage (90 x 60), and a cannonball (32)."""
    W, H = 96, 40
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    half = 12 + 6 * (1 - x / W)
    body = np.abs(y - 20) < half
    shade = 0.6 + 0.6 * (1 - np.abs(y - 14) / 20)
    rgb = np.array([0.22, 0.22, 0.26])[None, None, :] * shade[..., None]
    bands = body & ((np.abs(x - 20) < 3) | (np.abs(x - 88) < 4))
    rgb[bands] *= 1.5
    save_rgba(OUT / "siege" / "barrel.png", np.clip(rgb, 0, 1), body.astype(float))
    W, H = 90, 60
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    img = np.zeros((H, W, 4))
    frame = (y > 14) & (y < 34) & (x > 6) & (x < 84)
    img[frame, :3] = np.array([0.45, 0.30, 0.18])
    img[frame, 3] = 1
    for cx in (22, 68):
        r = np.sqrt((x - cx) ** 2 + (y - 42) ** 2)
        wheel = r < 17
        img[wheel, :3] = np.array([0.35, 0.24, 0.14])
        img[wheel & ((r > 14) | (r < 4) | (np.abs(np.sin(np.arctan2(y - 42, x - cx) * 3)) < 0.15)), :3] = np.array([0.18, 0.13, 0.09])
        img[wheel, 3] = 1
    save_rgba(OUT / "siege" / "carriage.png", img[..., :3], img[..., 3])
    S = 32
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - 16) ** 2 + (y - 16) ** 2)
    hl = np.sqrt((x - 11) ** 2 + (y - 11) ** 2)
    rgb = np.array([0.15, 0.15, 0.18])[None, None, :] * (1 + 1.5 * np.clip(1 - hl / 10, 0, 1))[..., None]
    save_rgba(OUT / "siege" / "ball.png", np.clip(rgb, 0, 1), np.clip(15.5 - r, 0, 1))


# ── 43 Night Market ────────────────────────────────────────────────────
# A market street at night, seen from above.

def nm43_street():
    """Cobbles, 320 square, tiling: rounded setts in staggered rows - 320 is
    a whole number of setts (40) and rows (32), or the tile shows a seam."""
    S = 320
    rng = np.random.default_rng(431)
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    row = np.floor(y / 32)
    xs = x + (row % 2) * 20
    cx = (np.floor(xs / 40) + 0.5) * 40 - (row % 2) * 20
    cy = (row + 0.5) * 32
    dx, dy = (x - cx) / 18.5, (y - cy) / 14.5
    d = np.sqrt(dx ** 4 + dy ** 4)
    stone = np.clip((1.0 - d) * 4, 0, 1)
    tone = 0.85 + 0.2 * ((np.floor(xs / 40) * 7 + row * 3) % 5) / 4
    n = fbm(S, 16, 3)
    rgb = np.array([0.52, 0.50, 0.47])[None, None, :] * (tone * (0.85 + 0.25 * n) * (0.6 + 0.4 * stone))[..., None]
    gap = stone < 0.05
    rgb[gap] = np.array([0.16, 0.15, 0.14])
    save_rgba(OUT / "market" / "street.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def _nm_stall(name, canvas, goods, rng, W=240, H=170):
    """A stall from above: a counter at the bottom with its goods, and a
    striped awning over the back."""
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    img = np.zeros((H, W, 4))
    shadow = (np.abs(x - W / 2) < W / 2 - 4) & (y > 18) & (y < H - 2)
    img[shadow, 3] = 0.35
    counter = (np.abs(x - W / 2) < W / 2 - 10) & (y > H - 62) & (y < H - 14)
    img[counter, :3] = np.array([0.42, 0.28, 0.16]) * (0.85 + 0.15 * np.sin(x[counter] * 0.3))[:, None]
    img[counter, 3] = 1
    for i in range(14):
        gx, gy = 24 + i * (W - 48) / 13 + rng.uniform(-3, 3), H - 38 + rng.uniform(-6, 6)
        r = rng.uniform(5, 8)
        g = (x - gx) ** 2 + (y - gy) ** 2 < r * r
        img[g, :3] = np.array(goods[i % len(goods)]) * (0.85 + 0.3 * (y[g] < gy))[:, None]
        img[g, 3] = 1
    # The awning: stripes, a scalloped front edge.
    edge = 92 + 7 * np.abs(np.sin(x * np.pi / 24))
    awning = (y < edge) & (np.abs(x - W / 2) < W / 2 - 2)
    stripe = (np.floor(x / 24) % 2) == 0
    col = np.where(stripe[..., None], np.array(canvas[0]), np.array(canvas[1]))
    shade = 0.75 + 0.35 * (1 - y / 100)
    img[awning, :3] = (col * shade[..., None])[awning]
    img[awning, 3] = 1
    save_rgba(OUT / "market" / f"{name}.png", np.clip(img[..., :3], 0, 1), img[..., 3])


def nm43_stalls():
    rng = np.random.default_rng(432)
    _nm_stall("stall_red", ((0.75, 0.18, 0.15), (0.92, 0.86, 0.72)), [(0.9, 0.55, 0.15), (0.85, 0.2, 0.15), (0.95, 0.8, 0.25)], rng)
    _nm_stall("stall_blue", ((0.18, 0.35, 0.65), (0.90, 0.88, 0.80)), [(0.55, 0.3, 0.6), (0.3, 0.6, 0.3), (0.8, 0.75, 0.6)], rng)
    _nm_stall("stall_green", ((0.20, 0.50, 0.30), (0.92, 0.85, 0.55)), [(0.75, 0.75, 0.8), (0.95, 0.9, 0.5), (0.6, 0.85, 0.9)], rng)
    # The grill: a stall whose counter is a bed of coals.
    _nm_stall("stall_grill", ((0.35, 0.25, 0.20), (0.75, 0.55, 0.30)), [(0.55, 0.3, 0.2)], rng)


def nm43_fountain():
    """A round fountain from above, 260: a stone rim, water, a spout."""
    S = 260
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - S / 2) ** 2 + (y - S / 2) ** 2)
    a = np.arctan2(y - S / 2, x - S / 2)
    img = np.zeros((S, S, 4))
    rim = (r > 104) & (r < 126)
    blocks = np.abs(np.sin(a * 16)) > 0.08
    img[rim, :3] = np.array([0.62, 0.60, 0.58])[None, :] * np.where(blocks[rim], 1.0, 0.55)[:, None] * (1.15 - 0.4 * (r[rim] - 104) / 22)[:, None]
    img[rim, 3] = 1
    water = r <= 104
    ripple = 0.5 + 0.5 * np.sin(r * 0.35)
    img[water, :3] = np.array([0.18, 0.38, 0.55]) * (0.8 + 0.3 * ripple[water])[:, None]
    img[water, 3] = 1
    spout = r < 22
    img[spout, :3] = np.array([0.7, 0.68, 0.66]) * (1.1 - r[spout] / 40)[:, None]
    jet = r < 8
    img[jet, :3] = np.array([0.85, 0.92, 1.0])
    save_rgba(OUT / "market" / "fountain.png", np.clip(img[..., :3], 0, 1), np.clip(126.5 - r, 0, 1) * img[..., 3])


def _nm_figure(name, cloak, hat=None, lute=False, S=72):
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    img = np.zeros((S, S, 4))
    sh = (((x - 36) / 22) ** 2 + ((y - 40) / 20) ** 2) < 1
    img[sh, 3] = 0.35
    body = (((x - 36) / 18) ** 2 + ((y - 36) / 14) ** 2) < 1
    img[body, :3] = np.array(cloak) * (0.8 + 0.3 * (x[body] < 36))[:, None]
    img[body, 3] = 1
    if lute:
        lb = (((x - 50) / 9) ** 2 + ((y - 44) / 11) ** 2) < 1
        neck = (np.abs(y - 30) < 2) & (x > 44) & (x < 64)
        img[lb | neck, :3] = np.array([0.65, 0.42, 0.20])
        img[lb | neck, 3] = 1
    head = ((x - 36) ** 2 + (y - 33) ** 2) < 8.5 ** 2
    img[head, :3] = np.array(hat if hat else (0.9, 0.74, 0.6)) * (0.85 + 0.25 * (x[head] < 36))[:, None]
    img[head, 3] = 1
    save_rgba(OUT / "market" / f"{name}.png", np.clip(img[..., :3], 0, 1), img[..., 3])


def nm43_people():
    _nm_figure("player", (0.25, 0.45, 0.70), hat=(0.35, 0.25, 0.18))
    _nm_figure("musician", (0.55, 0.20, 0.45), hat=(0.85, 0.65, 0.20), lute=True)


def nm43_chimes():
    """Wind chimes hanging from a bar, from above, 120 x 60."""
    W, H = 120, 60
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    img = np.zeros((H, W, 4))
    bar = (np.abs(y - 10) < 3) & (x > 6) & (x < W - 6)
    img[bar, :3] = np.array([0.35, 0.24, 0.14])
    img[bar, 3] = 1
    for i in range(7):
        cx = 14 + i * 15.5
        cy = 30 + (i % 3) * 7
        c = (x - cx) ** 2 + (y - cy) ** 2 < 25
        img[c, :3] = np.array([0.85, 0.75, 0.45]) * (0.8 + 0.4 * (x[c] < cx))[:, None]
        img[c, 3] = 1
        cord = (np.abs(x - cx) < 0.8) & (y > 10) & (y < cy)
        img[cord, :3] = 0.5
        img[cord, 3] = 1
    save_rgba(OUT / "market" / "chimes.png", np.clip(img[..., :3], 0, 1), img[..., 3])


# ── 44 Garden Party ────────────────────────────────────────────────────
# A formal garden at golden hour, from above.

def gp44_lawn():
    """Mown lawn, 256 square, tiling: wide stripes of light and dark, and a
    grain of blades over them."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    stripe = 0.5 + 0.5 * np.cos(x / S * 2 * np.pi * 2)
    stripe = smoothstep(0.25, 0.75, stripe)
    n = fbm(S, 8, 4)
    blades = fbm(S, 64, 2)
    shade = 0.82 + 0.16 * stripe + 0.12 * (n - 0.5) + 0.10 * (blades - 0.5)
    rgb = np.array([0.36, 0.56, 0.22])[None, None, :] * shade[..., None]
    save_rgba(OUT / "garden" / "lawn.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def gp44_gravel():
    """Raked gravel, 128 square, tiling."""
    S = 128
    n = fbm(S, 32, 3)
    grit = RNG.random((S, S))
    shade = 0.80 + 0.18 * (n - 0.5) + 0.16 * (grit - 0.5)
    rgb = np.array([0.80, 0.74, 0.62])[None, None, :] * shade[..., None]
    save_rgba(OUT / "garden" / "gravel.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def _gp_round_rect(w, h, r):
    y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
    qx = np.maximum(np.abs(x - w / 2) - (w / 2 - r), 0)
    qy = np.maximum(np.abs(y - h / 2) - (h / 2 - r), 0)
    d = np.sqrt(qx ** 2 + qy ** 2) - r + np.minimum(np.maximum(np.abs(x - w / 2) - (w / 2 - r), np.abs(y - h / 2) - (h / 2 - r)), 0)
    return x, y, d


def _gp_leaves(w, h, seed, base=(0.18, 0.36, 0.14)):
    """Clipped leaves: cells of light and dark, and fine noise."""
    rng = np.random.default_rng(seed)
    n = max(w, h)
    big = fbm(int(2 ** np.ceil(np.log2(n))), 16, 3)[:h, :w]
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    cells = np.sin(x * 0.9 + rng.uniform(0, 6)) * np.sin(y * 0.85 + rng.uniform(0, 6))
    fine = rng.random((h, w))
    shade = 0.78 + 0.35 * (big - 0.5) + 0.10 * cells + 0.12 * (fine - 0.5)
    return np.array(base)[None, None, :] * shade[..., None]


def gp44_hedge(name, w, h, seed):
    """A clipped hedge from above: rounded, lit from the upper left, darker
    at its foot."""
    x, y, d = _gp_round_rect(w, h, 14)
    rgb = _gp_leaves(w, h, seed)
    # A domed top: lighter on the side facing the sun, darker away from it.
    nx = (x - w / 2) / (w / 2)
    ny = (y - h / 2) / (h / 2)
    edge = np.clip(-d / 10.0, 0, 1)
    light = 0.75 + 0.35 * edge - 0.12 * nx - 0.12 * ny
    rgb = rgb * light[..., None]
    alpha = np.clip(0.5 - d, 0, 1)
    save_rgba(OUT / "garden" / f"{name}.png", np.clip(rgb, 0, 1), alpha)


def gp44_hedges():
    gp44_hedge("hedge_long", 300, 44, 441)
    gp44_hedge("hedge_short", 130, 44, 442)
    gp44_hedge("hedge_side", 44, 170, 443)
    gp44_hedge("hedge_curve", 240, 44, 444)


def gp44_topiary():
    """A clipped ball of box, 76 square: a lit sphere of leaves."""
    S = 76
    nx, ny, r = radius(S)
    rgb = _gp_leaves(S, S, 445, base=(0.17, 0.38, 0.15))
    nz = np.sqrt(np.clip(1 - r * r, 0, 1))
    lam = np.clip(-0.55 * nx - 0.55 * ny + 0.62 * nz, 0, 1)
    rgb = rgb * (0.45 + 0.85 * lam)[..., None]
    save_rgba(OUT / "garden" / "topiary.png", np.clip(rgb, 0, 1), np.clip((1 - r) * S / 2, 0, 1))


def gp44_fountain():
    """A round fountain, 260: a gravel apron, a stone rim, water, and a
    tiered bowl in the middle."""
    S = 260
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.sqrt((x - S / 2) ** 2 + (y - S / 2) ** 2)
    a = np.arctan2(y - S / 2, x - S / 2)
    img = np.zeros((S, S, 3))
    grit = RNG.random((S, S))
    img[:] = np.array([0.80, 0.74, 0.62]) * (0.86 + 0.14 * grit)[..., None]
    rim = (r > 92) & (r < 108)
    blocks = np.abs(np.sin(a * 14)) > 0.06
    stone = np.array([0.84, 0.82, 0.78]) * np.where(blocks, 1.0, 0.7)[..., None] * (1.08 - 0.35 * (r - 92) / 16)[..., None]
    img[rim] = stone[rim]
    water = r <= 92
    ripple = 0.5 + 0.5 * np.sin(r * 0.45)
    wcol = np.array([0.22, 0.46, 0.58]) * (0.78 + 0.28 * ripple)[..., None]
    # The sky, caught in the water on the side away from the sun.
    sheen = np.clip(((x - S / 2) + (y - S / 2)) / 120, 0, 1) * 0.25
    img[water] = (wcol + sheen[..., None])[water]
    bowl = r < 34
    img[bowl] = (np.array([0.86, 0.84, 0.80]) * (1.12 - r / 60)[..., None])[bowl]
    inner = r < 22
    img[inner] = (np.array([0.32, 0.55, 0.66]) * (0.9 + 0.2 * np.sin(r * 0.8))[..., None])[inner]
    jet = r < 6
    img[jet] = np.array([0.92, 0.97, 1.0])
    alpha = np.clip(128.5 - r, 0, 1)
    save_rgba(OUT / "garden" / "fountain.png", np.clip(img, 0, 1), alpha)


def gp44_gazebo():
    """An octagonal gazebo roof from above, 220: eight panels alternately lit,
    ridges, and a finial."""
    S = 220
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = x - S / 2, y - S / 2
    a = np.arctan2(dy, dx)
    r = np.sqrt(dx * dx + dy * dy)
    sector = np.floor((a + np.pi / 8) / (np.pi / 4)) % 8
    mid = sector * (np.pi / 4)
    # Octagon radius along this angle.
    oct_r = 104 * np.cos(np.pi / 8) / np.cos(((a + np.pi / 8) % (np.pi / 4)) - np.pi / 8)
    inside = r < oct_r
    sun = np.array([-0.7, -0.7])
    facing = np.cos(mid) * sun[0] + np.sin(mid) * sun[1]
    tone = 0.78 + 0.22 * facing
    rgb = np.array([0.93, 0.90, 0.84])[None, None, :] * tone[..., None]
    rgb = rgb * (0.88 + 0.12 * (r / 104))[..., None]
    ridge = np.abs(((a + np.pi / 8) % (np.pi / 4)) - np.pi / 8) * r < 1.4
    rgb[ridge] *= 0.72
    eave = (r > oct_r - 6) & inside
    rgb[eave] *= 0.8
    fin = r < 9
    rgb[fin] = np.array([0.75, 0.62, 0.30])
    alpha = np.clip(oct_r - r + 0.5, 0, 1)
    save_rgba(OUT / "garden" / "gazebo.png", np.clip(rgb, 0, 1), alpha)


def gp44_rosebed():
    """An oval bed of roses, 180 x 110: dark earth, leaves, and blooms."""
    W, H = 180, 110
    rng = np.random.default_rng(446)
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    e = np.sqrt(((x - W / 2) / (W / 2 - 2)) ** 2 + ((y - H / 2) / (H / 2 - 2)) ** 2)
    rgb = np.zeros((H, W, 3))
    rgb[:] = np.array([0.28, 0.20, 0.14]) * (0.85 + 0.2 * rng.random((H, W)))[..., None]
    leaves = _gp_leaves(W, H, 447, base=(0.16, 0.32, 0.13))
    leafy = (e < 0.92) & (rng.random((H, W)) < 0.9)
    rgb[leafy] = leaves[leafy]
    colours = [(0.82, 0.12, 0.18), (0.95, 0.55, 0.65), (0.96, 0.92, 0.86), (0.90, 0.30, 0.38)]
    for _ in range(46):
        bx, by = rng.uniform(14, W - 14), rng.uniform(12, H - 12)
        if ((bx - W / 2) / (W / 2 - 14)) ** 2 + ((by - H / 2) / (H / 2 - 12)) ** 2 > 1:
            continue
        rad = rng.uniform(4.0, 6.5)
        c = np.array(colours[rng.integers(len(colours))])
        d = np.sqrt((x - bx) ** 2 + (y - by) ** 2)
        m = d < rad
        petals = 0.8 + 0.25 * np.cos(np.arctan2(y - by, x - bx) * 5 + d)
        rgb[m] = (c * (petals * (1.1 - 0.3 * d / rad))[..., None])[m]
    alpha = np.clip((1 - e) * 40, 0, 1)
    save_rgba(OUT / "garden" / "rosebed.png", np.clip(rgb, 0, 1), alpha)


def gp44_bench():
    """A slatted bench from above, 110 x 36."""
    W, H = 110, 36
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    seat = (x > 3) & (x < W - 3) & (y > 4) & (y < H - 4)
    slat = (np.floor((y - 4) / 7) % 1 == 0) & ((y - 4) % 7 < 5.5)
    wood = np.array([0.58, 0.40, 0.24]) * (0.85 + 0.15 * np.sin(x * 0.7) * np.sin(y * 2.1))[..., None]
    rgb[seat & slat] = wood[seat & slat]
    alpha[seat & slat] = 1
    for ax in (8, W - 8):
        arm = (np.abs(x - ax) < 4) & (y > 1) & (y < H - 1)
        rgb[arm] = np.array([0.22, 0.20, 0.18])
        alpha[arm] = 1
    save_rgba(OUT / "garden" / "bench.png", np.clip(rgb, 0, 1), alpha)


def gp44_table():
    """A round table under a white cloth, 92: plates and a jug on it."""
    S = 92
    nx, ny, r = radius(S)
    rgb = np.zeros((S, S, 3))
    rgb[:] = np.array([0.95, 0.94, 0.90])
    fold = 0.92 + 0.08 * np.cos(np.arctan2(ny, nx) * 12) * smoothstep(0.7, 1.0, r)
    rgb *= (fold * (1.02 - 0.12 * (nx + ny)))[..., None]
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    for k in range(5):
        ang = k / 5 * 2 * np.pi + 0.3
        px, py = S / 2 + np.cos(ang) * 28, S / 2 + np.sin(ang) * 28
        d = np.sqrt((x - px) ** 2 + (y - py) ** 2)
        rgb[d < 8] = np.array([0.99, 0.99, 0.97])
        rgb[(d > 6) & (d < 8)] = np.array([0.55, 0.65, 0.80])
    d = np.sqrt((x - S / 2) ** 2 + (y - S / 2) ** 2)
    rgb[d < 7] = np.array([0.85, 0.55, 0.30])
    save_rgba(OUT / "garden" / "table.png", np.clip(rgb, 0, 1), np.clip((1 - r) * S / 2, 0, 1))


def gp44_gate():
    """A wooden garden gate, 64 x 18: boards and two iron straps."""
    W, H = 64, 18
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    board = (np.floor(x / 8) % 2)
    rgb[:] = np.array([0.55, 0.38, 0.22]) * (0.88 + 0.12 * board)[..., None]
    strap = (np.abs(x - 12) < 2) | (np.abs(x - W + 12) < 2)
    rgb[strap] = np.array([0.20, 0.19, 0.18])
    save_rgba(OUT / "garden" / "gate.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def gp44_guests():
    """Six guests from above, 32 square, facing +x: shoulders, a head, and a
    hat brim or hair."""
    looks = [
        ("guest_1", (0.82, 0.30, 0.36), (0.93, 0.86, 0.62), True),
        ("guest_2", (0.25, 0.42, 0.70), (0.30, 0.20, 0.14), False),
        ("guest_3", (0.95, 0.88, 0.70), (0.90, 0.80, 0.55), True),
        ("guest_4", (0.40, 0.62, 0.48), (0.18, 0.14, 0.12), False),
        ("guest_5", (0.70, 0.52, 0.80), (0.98, 0.95, 0.90), True),
        ("guest_6", (0.92, 0.62, 0.28), (0.55, 0.32, 0.18), False),
    ]
    S = 32
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    for name, coat, top, hat in looks:
        img = np.zeros((S, S, 3))
        alpha = np.zeros((S, S))
        sh = ((x - 15) / 11) ** 2 + ((y - 17.5) / 14) ** 2 < 1
        alpha[sh] = 0.3
        body = ((x - 15) / 8.5) ** 2 + ((y - 16) / 13) ** 2 < 1
        img[body] = (np.array(coat) * (0.82 + 0.25 * (y[body] < 16))[:, None])
        alpha[body] = 1
        if hat:
            brim = (x - 17) ** 2 + (y - 16) ** 2 < 8.5 ** 2
            img[brim] = np.array(top) * 0.92
            alpha[brim] = 1
            crown = (x - 17) ** 2 + (y - 16) ** 2 < 5 ** 2
            img[crown] = np.array(top) * 1.08
        else:
            head = (x - 16.5) ** 2 + (y - 16) ** 2 < 6 ** 2
            img[head] = np.array(top) * (0.9 + 0.2 * (y[head] < 15))[:, None]
            alpha[head] = 1
            face = (x - 20.5) ** 2 + (y - 16) ** 2 < 3 ** 2
            img[face & head] = np.array([0.90, 0.72, 0.58])
        save_rgba(OUT / "garden" / f"{name}.png", np.clip(img, 0, 1), alpha)


# ── 45 The Polyglot Inn ────────────────────────────────────────────────
# An inn's wall at night, front on.

def pi45_wall():
    """Vertical planks, 256 square, tiling: grain, knots and dark seams."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    plank = np.floor(x / 64)
    tone = 0.86 + 0.1 * ((plank * 37) % 5) / 4
    grain = fbm(S, 8, 3)
    streak = 0.5 + 0.5 * np.sin((x % 64) * 0.9 + grain * 9 + plank * 2)
    shade = tone * (0.8 + 0.12 * streak + 0.16 * (grain - 0.5))
    seam = np.minimum(x % 64, 64 - x % 64) < 1.5
    rgb = np.array([0.42, 0.27, 0.16])[None, None, :] * shade[..., None]
    rgb[seam] *= 0.35
    # A board end every so often, offset per plank.
    cut = (np.abs((y + plank * 97) % 256 - 128) < 1.0)
    rgb[cut] *= 0.45
    save_rgba(OUT / "inn" / "wall.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def pi45_window():
    """A night window, 240 x 300: a deep frame, four panes of sky, a moon."""
    W, H = 240, 300
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    rgb[:] = np.array([0.30, 0.19, 0.11])
    inner = (x > 18) & (x < W - 18) & (y > 18) & (y < H - 18)
    t = y / H
    sky = np.array([0.05, 0.08, 0.20])[None, None, :] * (1 - t)[..., None] + np.array([0.10, 0.14, 0.30])[None, None, :] * t[..., None]
    rgb[inner] = sky[inner]
    rng = np.random.default_rng(451)
    for _ in range(40):
        sx, sy = rng.uniform(22, W - 22), rng.uniform(22, H * 0.7)
        d = (x - sx) ** 2 + (y - sy) ** 2
        rgb[inner & (d < 1.8)] = np.array([0.95, 0.95, 1.0]) * rng.uniform(0.6, 1.0)
    moon = (x - 160) ** 2 + (y - 80) ** 2
    rgb[inner & (moon < 24 ** 2)] = np.array([0.98, 0.95, 0.82])
    rgb[inner & ((x - 152) ** 2 + (y - 74) ** 2 < 22 ** 2) & (moon < 24 ** 2)] = np.array([0.90, 0.88, 0.76])
    halo = np.exp(-np.maximum(np.sqrt(moon) - 24, 0) / 30) * 0.25
    rgb[inner] += (halo[..., None] * np.array([0.8, 0.8, 0.7]))[inner]
    # Distant roofs.
    roof = y > H - 18 - (40 + 18 * np.abs(np.sin(x * 0.03)) + 25 * (np.floor(x / 60) % 2))
    rgb[inner & roof] = np.array([0.03, 0.04, 0.08])
    # A few lit panes in the houses: small, scattered, below the roof line.
    pane = ((x % 23) > 15) & ((x % 23) < 21) & ((y % 13) > 6) & ((y % 13) < 11)
    lit = inner & roof & pane & (((np.floor(x / 23) * 5 + np.floor(y / 13) * 3) % 4) == 0) & (y > H - 45)
    rgb[lit] = np.array([0.95, 0.70, 0.30])
    mull = (np.abs(x - W / 2) < 5) | (np.abs(y - H / 2) < 5)
    rgb[mull & inner] = np.array([0.26, 0.16, 0.09])
    sill = y > H - 14
    rgb[sill] = np.array([0.36, 0.23, 0.13])
    save_rgba(OUT / "inn" / "window.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def pi45_sign():
    """A hanging signboard, 480 x 190: two chains and a carved board."""
    W, H = 480, 190
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    for cx in (70, W - 70):
        link = (np.abs(x - cx) < 3.5) & (y < 52) & (((y // 9) % 2 == 0) | (np.abs(x - cx) < 1.6))
        rgb[link] = np.array([0.22, 0.21, 0.20])
        alpha[link] = 1
    bx0, by0 = 8, 48
    qx = np.maximum(np.abs(x - W / 2) - (W / 2 - bx0 - 18), 0)
    qy = np.maximum(np.abs(y - (H + by0) / 2) - ((H - by0) / 2 - 18), 0)
    board = np.sqrt(qx ** 2 + qy ** 2) < 18
    grain = fbm(512, 10, 3)[:H, :W]
    wood = np.array([0.40, 0.24, 0.12])[None, None, :] * (0.8 + 0.25 * grain + 0.06 * np.sin(y * 0.6 + grain * 8))[..., None]
    rgb[board] = wood[board]
    alpha[board] = 1
    edge = board & (np.sqrt(qx ** 2 + qy ** 2) > 10)
    rgb[edge] *= 0.6
    groove = board & (np.abs(np.sqrt(qx ** 2 + qy ** 2) - 5) < 1.2) & ((qx > 0) | (qy > 0))
    save_rgba(OUT / "inn" / "sign.png", np.clip(rgb, 0, 1), alpha)


def pi45_chalkboard():
    """A chalkboard, 400 x 400: a wooden frame round dark slate, smudged."""
    S = 400
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    rgb = np.zeros((S, S, 3))
    n = fbm(512, 6, 4)[:S, :S]
    rgb[:] = np.array([0.08, 0.11, 0.10])[None, None, :] * (0.85 + 0.3 * n)[..., None]
    smudge = fbm(512, 3, 3)[:S, :S]
    rgb += (np.clip(smudge - 0.55, 0, 1) * 0.12)[..., None]
    frame = (x < 20) | (x > S - 20) | (y < 20) | (y > S - 20)
    fw = np.array([0.45, 0.29, 0.16])[None, None, :] * (0.8 + 0.25 * n)[..., None]
    rgb[frame] = fw[frame]
    bevel = frame & ((np.abs(x - 20) < 2) | (np.abs(x - S + 20) < 2) | (np.abs(y - 20) < 2) | (np.abs(y - S + 20) < 2))
    rgb[bevel] *= 0.6
    tray = (y > S - 34) & (y < S - 20) & (x > 40) & (x < S - 40)
    rgb[tray] = np.array([0.40, 0.25, 0.14])
    chalk = tray & (np.abs(x - 300) < 16) & (y < S - 26)
    rgb[chalk] = np.array([0.92, 0.92, 0.88])
    save_rgba(OUT / "inn" / "chalkboard.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def pi45_parchment():
    """A notice, 330 x 260: parchment with a rough edge and a red seal."""
    W, H = 330, 260
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm(512, 8, 4)[:H, :W]
    rough = 6 + 5 * fbm(512, 24, 2)[:H, :W]
    d = np.minimum(np.minimum(x, W - x), np.minimum(y, H - y))
    alpha = np.clip((d - rough) / 1.5, 0, 1)
    rgb = np.array([0.88, 0.80, 0.62])[None, None, :] * (0.88 + 0.18 * n)[..., None]
    age = np.clip(1 - d / 40, 0, 1) ** 2 * 0.35
    rgb *= (1 - age)[..., None]
    seal = (x - W / 2) ** 2 + (y - 18) ** 2
    rgb[seal < 13 ** 2] = np.array([0.62, 0.10, 0.10]) * (1.1 - np.sqrt(seal[seal < 13 ** 2]) / 30)[:, None]
    save_rgba(OUT / "inn" / "parchment.png", np.clip(rgb, 0, 1), alpha)


def pi45_lantern():
    """An iron lantern, 56 x 96: a ring, a cap, glass glowing amber."""
    W, H = 56, 96
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    ring = (np.abs(np.sqrt((x - W / 2) ** 2 + (y - 9) ** 2) - 6) < 1.8)
    cap = (y > 15) & (y < 30) & (np.abs(x - W / 2) < 4 + (y - 15) * 1.2)
    glass = (y >= 30) & (y < 78) & (np.abs(x - W / 2) < 19)
    base = (y >= 78) & (y < 86) & (np.abs(x - W / 2) < 22)
    iron = np.array([0.16, 0.15, 0.14])
    for m in (ring, cap, base):
        rgb[m] = iron
        alpha[m] = 1
    glow = np.exp(-((x - W / 2) ** 2 / 180 + (y - 54) ** 2 / 420))
    rgb[glass] = (np.array([1.0, 0.72, 0.32]) * (0.65 + 0.45 * glow)[..., None])[glass]
    alpha[glass] = 1
    bars = glass & ((np.abs(x - W / 2) < 1.5) | (np.abs(np.abs(x - W / 2) - 18) < 1.5))
    rgb[bars] = iron
    save_rgba(OUT / "inn" / "lantern.png", np.clip(rgb, 0, 1), alpha)


def pi45_counter():
    """The bar counter's front, 256 x 160, tiling sideways: a polished top
    lip and panelled front."""
    W, H = 256, 160
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm(256, 8, 3)[:H, :W]
    rgb = np.array([0.33, 0.20, 0.11])[None, None, :] * (0.82 + 0.2 * n)[..., None]
    top = y < 22
    rgb[top] = (np.array([0.50, 0.32, 0.18]) * (1.1 - y / 60)[..., None] * (0.9 + 0.15 * n[..., None]))[top]
    rgb[(y > 20) & (y < 25)] *= 0.4
    px = x % 128
    panel = (px > 14) & (px < 114) & (y > 40) & (y < H - 18)
    rgb[panel] *= 1.12
    rim = panel & ((np.abs(px - 14) < 2) | (np.abs(px - 114) < 2) | (np.abs(y - 40) < 2) | (np.abs(y - (H - 18)) < 2))
    rgb[rim] *= 0.55
    save_rgba(OUT / "inn" / "counter.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def pi45_mug():
    """A pewter tankard, 48 x 56, with a handle and foam."""
    W, H = 48, 56
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    body = (x > 6) & (x < 34) & (y > 10) & (y < H - 2)
    shade = 0.6 + 0.5 * np.clip(1 - np.abs(x - 15) / 14, 0, 1)
    rgb[body] = (np.array([0.62, 0.63, 0.66]) * shade[..., None])[body]
    alpha[body] = 1
    handle = (np.abs(np.sqrt((x - 34) ** 2 + (y - 30) ** 2) - 9) < 2.5) & (x > 34)
    rgb[handle] = np.array([0.45, 0.46, 0.48])
    alpha[handle] = 1
    foam = (y <= 14) & (y > 2) & (x > 4) & (x < 36) & ((x - 20) ** 2 / 300 + (y - 10) ** 2 / 40 < 1)
    rgb[foam] = np.array([0.97, 0.95, 0.88])
    alpha[foam] = 1
    save_rgba(OUT / "inn" / "mug.png", np.clip(rgb, 0, 1), alpha)


# ── 46 Sea Cove ────────────────────────────────────────────────────────
# A beach, a sea cave and the water under them, side on, late afternoon.

def _sc_rect_fbm(w, h, cells, octaves):
    n = int(2 ** np.ceil(np.log2(max(w, h))))
    return fbm(n, cells, octaves)[:h, :w]


def sc46_sky():
    """The sky, 32 x 256 (stretched): deep blue overhead to a warm haze."""
    y = (np.arange(256) + 0.5)[:, None] / 256 * np.ones((256, 32))
    top = np.array([0.18, 0.32, 0.62])
    mid = np.array([0.55, 0.62, 0.78])
    low = np.array([0.98, 0.78, 0.55])
    t = y[..., None]
    rgb = np.where(t < 0.6, top + (mid - top) * (t / 0.6), mid + (low - mid) * ((t - 0.6) / 0.4))
    save_rgba(OUT / "cove" / "sky.png", np.clip(rgb, 0, 1), np.ones((256, 32)))


def sc46_far_sea():
    """The sea out to the horizon, 64 x 64 (stretched): hazy at the top."""
    y = (np.arange(64) + 0.5)[:, None] / 64 * np.ones((64, 64))
    far = np.array([0.62, 0.66, 0.72])
    near = np.array([0.12, 0.34, 0.50])
    rgb = far + (near - far) * (y ** 0.6)[..., None]
    sparkle = (np.random.default_rng(461).random((64, 64)) > 0.985) & (y < 0.6)
    rgb[sparkle] = np.array([1.0, 0.92, 0.75])
    save_rgba(OUT / "cove" / "far_sea.png", np.clip(rgb, 0, 1), np.ones((64, 64)))


SC_BEACH = (680, 500)       # world x -640..40, y -360..140


def sc46_beach():
    """The beach and the sand under the water, 680 x 500: a slope from the
    dunes down to the seabed, dune grass at the top."""
    W, H = SC_BEACH
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    wx = -640 + x
    wy = 140 - y
    surface = 100 - (wx + 640) * 0.62 + 6 * np.sin(wx * 0.02)
    inside = wy < surface
    n = _sc_rect_fbm(W, H, 32, 3)
    grain = np.random.default_rng(462).random((H, W))
    depth = np.clip((surface - wy) / 60, 0, 1)
    sand = np.array([0.90, 0.80, 0.58])[None, None, :] * (0.88 + 0.12 * n + 0.08 * (grain - 0.5))[..., None]
    sand *= (1.0 - 0.18 * depth)[..., None]
    wet = wy < -40
    sand[wet] *= np.array([0.80, 0.85, 0.88])
    rgb = sand
    alpha = np.clip(surface - wy, 0, 1)
    # Dune grass on the top of the slope.
    rng = np.random.default_rng(463)
    for _ in range(40):
        bx = rng.uniform(0, 210)
        by = 140 - (100 - (bx) * 0.62)
        h = rng.uniform(18, 40)
        lean = rng.uniform(-0.4, 0.4)
        for k in range(int(h)):
            px = int(bx + lean * k)
            py = int(by - k)
            if 0 <= px < W and 0 <= py < H:
                rgb[py, px] = np.array([0.55, 0.62, 0.30]) * (0.8 + 0.2 * k / h)
                alpha[py, px] = 1
    save_rgba(OUT / "cove" / "beach.png", np.clip(rgb, 0, 1), alpha)


SC_CLIFF = (540, 720)       # world x 100..640, y -360..360


def sc46_cliff():
    """A sea cliff with a cave in it at the waterline, 540 x 720. The cave
    is a dark recess in the rock, with drip stones hanging from its roof."""
    W, H = SC_CLIFF
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    wx = 100 + x
    wy = 360 - y
    n = _sc_rect_fbm(W, H, 16, 4)
    face = 160 + 40 * np.sin(wy * 0.012) + 30 * (n - 0.5) - 0.12 * np.maximum(wy - 150, 0)
    rock = wx > face
    strata = 0.5 + 0.5 * np.sin(wy * 0.09 + n * 4)
    rgb = np.array([0.46, 0.40, 0.36])[None, None, :] * (0.72 + 0.25 * n + 0.1 * strata)[..., None]
    # Lit on the side facing the low sun, darker in the rock's body.
    rgb *= (1.08 - 0.25 * np.clip((wx - face) / 300, 0, 1))[..., None]
    # The cave: an arch at the waterline.
    # An uneven arch, not an ellipse: its outline wanders with the rock.
    ex, ey, rx, ry = 390, 20, 230, 140
    rough = _sc_rect_fbm(W, H, 6, 3)
    e = ((wx - ex) / rx) ** 2 + ((wy - ey) / ry) ** 2 + 0.55 * (rough - 0.5) + 0.0015 * np.maximum(wx - ex, 0)
    hollow = rock & (e < 1) & (wy > -90)
    depth = np.clip(1 - e, 0, 1)
    back = np.array([0.20, 0.17, 0.16])[None, None, :] * (0.55 + 0.6 * n)[..., None] * (1.0 - 0.55 * depth)[..., None]
    rgb[hollow] = back[hollow]
    rim = rock & (e >= 1) & (e < 1.12) & (wy > -90)
    rgb[rim] *= 0.65
    # A few thin drip stones, uneven, deeper in rather than at the mouth.
    rng = np.random.default_rng(464)
    for _ in range(9):
        sx = rng.uniform(ex - rx * 0.2, ex + rx * 0.8)
        col = hollow[:, int(np.clip(sx - 100, 0, W - 1))]
        rows = np.nonzero(col)[0]
        if len(rows) == 0:
            continue
        roof = 360 - rows.min()
        ln = rng.uniform(6, 24)
        wid = rng.uniform(1.5, 3.5)
        cone = (np.abs(wx - sx) < (1 - (roof - wy) / ln) * wid) & (wy <= roof) & (wy > roof - ln)
        rgb[cone & hollow] = np.array([0.36, 0.32, 0.29])
    save_rgba(OUT / "cove" / "cliff.png", np.clip(rgb, 0, 1), rock.astype(np.float64))


def sc46_seabed():
    """The seabed, 256 x 80, tiling sideways: sand and pebbles."""
    W, H = 256, 80
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm(256, 16, 3)[:H, :W]
    top = 14 + 6 * np.sin(x / W * 2 * np.pi * 2) + 3 * np.sin(x / W * 2 * np.pi * 7)
    rgb = np.array([0.72, 0.64, 0.48])[None, None, :] * (0.8 + 0.25 * n)[..., None]
    rng = np.random.default_rng(465)
    for _ in range(26):
        px, py, r = rng.uniform(0, W), rng.uniform(20, H), rng.uniform(4, 11)
        for dx in (-W, 0, W):
            m = ((x - px - dx) / r) ** 2 + ((y - py) / (r * 0.7)) ** 2 < 1
            rgb[m] = np.array([0.45, 0.42, 0.40]) * rng.uniform(0.7, 1.1)
    alpha = np.clip(y - top, 0, 1)
    save_rgba(OUT / "cove" / "seabed.png", np.clip(rgb, 0, 1), alpha)


def sc46_kelp():
    """A kelp stalk, 44 x 240: a wavy stem with blades, rooted at the bottom."""
    W, H = 44, 240
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    h = (H - y) / H
    cx = W / 2 + 6 * np.sin(h * 9)
    stem = np.abs(x - cx) < 2.2
    blade = np.abs(x - cx - 7 * np.sin(h * 40)) < 5 * np.abs(np.sin(h * 20)) + 0.5
    m = (stem | blade) & (y > 4)
    rgb = np.zeros((H, W, 3))
    rgb[:] = np.array([0.30, 0.42, 0.14])
    rgb *= (0.7 + 0.4 * h)[..., None]
    save_rgba(OUT / "cove" / "kelp.png", np.clip(rgb, 0, 1), m.astype(np.float64))


def sc46_fish():
    """A small fish, 40 x 18, facing +x."""
    W, H = 40, 18
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    body = ((x - 22) / 15) ** 2 + ((y - 9) / 6.5) ** 2 < 1
    tail = (x < 10) & (np.abs(y - 9) < (10 - x) * 0.8)
    m = body | tail
    rgb = np.zeros((H, W, 3))
    rgb[:] = np.array([0.95, 0.65, 0.25])
    rgb[y > 10] *= 1.15
    stripe = (np.abs(x - 22) < 2) & body
    rgb[stripe] = np.array([0.98, 0.95, 0.9])
    eye = (x - 31) ** 2 + (y - 7.5) ** 2 < 2.2
    rgb[eye] = 0.05
    save_rgba(OUT / "cove" / "fish.png", np.clip(rgb, 0, 1), m.astype(np.float64))


def sc46_gull():
    """A gull gliding, 48 x 20, from the side."""
    W, H = 48, 20
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    wing = np.abs(y - (8 + 5 * np.abs(x - 24) / 24 - 3 * np.exp(-((x - 24) / 6) ** 2))) < 1.6
    body = ((x - 26) / 8) ** 2 + ((y - 10) / 2.5) ** 2 < 1
    m = wing | body
    rgb = np.ones((H, W, 3)) * 0.95
    tips = wing & ((x < 6) | (x > 42))
    rgb[tips] = 0.2
    save_rgba(OUT / "cove" / "gull.png", rgb, m.astype(np.float64))


def sc46_swimmer():
    """A swimmer in a red cap and goggles, 76 x 30, lying along +x."""
    W, H = 76, 30
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    torso = ((x - 40) / 18) ** 2 + ((y - 15) / 6) ** 2 < 1
    legs = (x > 6) & (x < 26) & (np.abs(y - 15 - 2 * np.sin(x * 0.4)) < 3.2)
    arm = (x > 52) & (x < 72) & (np.abs(y - 11) < 2.2)
    head = (x - 61) ** 2 + (y - 15) ** 2 < 6.5 ** 2
    skin = np.array([0.92, 0.72, 0.58])
    suit = np.array([0.15, 0.30, 0.65])
    rgb[legs | arm] = skin
    rgb[torso] = suit
    rgb[head] = np.array([0.85, 0.15, 0.12])
    goggle = (x - 66) ** 2 + (y - 16) ** 2 < 2.2 ** 2
    rgb[goggle] = np.array([0.9, 0.95, 1.0])
    alpha[legs | arm | torso | head] = 1
    save_rgba(OUT / "cove" / "swimmer.png", np.clip(rgb, 0, 1), alpha)


def sc46_foam():
    """The surface seen side on, 256 x 20, tiling: a bright ragged foam line."""
    W, H = 256, 20
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    edge = 8 + 3 * np.sin(x / W * 2 * np.pi * 4) + 1.5 * np.sin(x / W * 2 * np.pi * 11)
    a = np.clip(1 - np.abs(y - edge) / 4, 0, 1) * 0.9
    save_rgba(OUT / "cove" / "foam.png", np.ones((H, W, 3)) * np.array([0.92, 0.97, 1.0]), a)


def sc46_water():
    """The water body, 32 x 256 (stretched): clear blue at the top, deep at
    the bottom - drawn over everything under the surface."""
    t = (np.arange(256) + 0.5)[:, None] / 256 * np.ones((256, 32))
    top = np.array([0.20, 0.62, 0.70])
    bottom = np.array([0.03, 0.14, 0.30])
    rgb = top + (bottom - top) * t[..., None]
    alpha = 0.42 + 0.42 * t
    save_rgba(OUT / "cove" / "water.png", np.clip(rgb, 0, 1), alpha)


def sc46_ray():
    """A shaft of light through the water, 64 x 320, soft on both sides."""
    W, H = 64, 320
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    a = np.exp(-((x - W / 2) / (W * 0.22)) ** 2) * (1 - y / H) ** 1.5 * 0.5
    save_rgba(OUT / "cove" / "ray.png", np.ones((H, W, 3)), a)


# ── 47 Autumn Run ──────────────────────────────────────────────────────
# Autumn hills, side on, for a fox to run across.

def ar47_sky():
    """Autumn sky, 32 x 256 (stretched): cool blue to a warm low haze."""
    t = (np.arange(256) + 0.5)[:, None] / 256 * np.ones((256, 32))
    top = np.array([0.36, 0.52, 0.78])
    low = np.array([0.98, 0.84, 0.66])
    rgb = top + (low - top) * (t ** 1.4)[..., None]
    save_rgba(OUT / "run" / "sky.png", np.clip(rgb, 0, 1), np.ones((256, 32)))


def _ar_ridge(W, H, base, amp, waves, seed):
    """A tiling ridge line: heights (in pixels from the top) along x."""
    rng = np.random.default_rng(seed)
    x = np.arange(W) + 0.5
    h = np.full(W, float(base))
    for k, a in waves:
        h += amp * a * np.sin(2 * np.pi * k * x / W + rng.uniform(0, 6.28))
    return h


def ar47_mountains():
    """Far mountains, 1024 x 512, tiling sideways: hazy violet peaks, drawn
    at the size they are shown so their edges stay smooth."""
    W, H = 1024, 512
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    ridge = _ar_ridge(W, H, 240, 100, [(2, 1.0), (5, 0.45), (11, 0.18)], 471)
    depth = y - ridge[None, :]
    t = np.clip(depth / 280, 0, 1)
    rgb = np.array([0.62, 0.58, 0.72])[None, None, :] * (1 - 0.25 * t)[..., None]
    snow = np.clip((22 - depth) / 2, 0, 1) * (ridge[None, :] < 190)
    rgb = rgb * (1 - snow[..., None]) + np.array([0.92, 0.90, 0.95]) * snow[..., None]
    save_rgba(OUT / "run" / "mountains.png", np.clip(rgb, 0, 1), np.clip(depth + 0.5, 0, 1))


def ar47_hills():
    """Mid hills, 1024 x 512, tiling: rust and ochre, with a row of tree
    tops - at the size they are shown."""
    W, H = 1024, 512
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    ridge = _ar_ridge(W, H, 220, 60, [(3, 1.0), (7, 0.4)], 472)
    rng = np.random.default_rng(473)
    trees = np.zeros((H, W), dtype=bool)
    tree_col = np.zeros((H, W, 3))
    palette = [(0.78, 0.36, 0.14), (0.86, 0.56, 0.18), (0.62, 0.24, 0.12), (0.70, 0.62, 0.22)]
    soft = np.zeros((H, W))
    for _ in range(46):
        cx = rng.uniform(0, W)
        r = rng.uniform(20, 40)
        cy = np.interp(cx, np.arange(W) + 0.5, ridge) - r * 0.4
        for dx in (-W, 0, W):
            d = np.sqrt(((x - cx - dx) / r) ** 2 + ((y - cy) / (r * 1.1)) ** 2)
            m = d < 1
            trees |= m
            tree_col[m] = np.array(palette[rng.integers(len(palette))]) * rng.uniform(0.85, 1.05)
            soft = np.maximum(soft, np.clip((1 - d) * r, 0, 1))
    ground = y > ridge[None, :]
    rgb = np.array([0.66, 0.42, 0.22])[None, None, :] * (0.9 - 0.25 * np.clip((y - ridge[None, :]) / 280, 0, 1))[..., None]
    rgb[trees] = tree_col[trees]
    alpha = np.maximum(np.clip(y - ridge[None, :] + 0.5, 0, 1), soft)
    save_rgba(OUT / "run" / "hills.png", np.clip(rgb, 0, 1), alpha)


def ar47_edge():
    """The ground's grassy edge, 256 x 64, tiling: dry grass and fallen leaves."""
    W, H = 256, 64
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rng = np.random.default_rng(474)
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    turf = (y > 22) & (y < 44)
    shade = 0.85 + 0.15 * np.sin(x * 0.3) * np.sin(y * 0.7)
    rgb[turf] = (np.array([0.55, 0.52, 0.22]) * shade[..., None])[turf]
    alpha[turf] = 1
    for _ in range(140):
        bx = rng.uniform(0, W)
        hgt = rng.uniform(6, 18)
        lean = rng.uniform(-0.4, 0.4)
        for k in range(int(hgt)):
            px, py = int(bx + lean * k) % W, int(24 - k)
            if 0 <= py < H:
                rgb[py, px] = np.array([0.66, 0.60, 0.28]) * (0.8 + 0.2 * k / hgt)
                alpha[py, px] = 1
    for _ in range(70):
        lx, ly = rng.uniform(0, W), rng.uniform(18, 30)
        c = np.array([(0.85, 0.35, 0.12), (0.92, 0.62, 0.15), (0.70, 0.22, 0.10)][rng.integers(3)])
        for dx in (-W, 0, W):
            m = ((x - lx - dx) / 4.5) ** 2 + ((y - ly) / 2.5) ** 2 < 1
            rgb[m] = c
            alpha[m] = 1
    # A soft dark lip where the soil starts.
    lip = (y >= 44) & (y < 50)
    rgb[lip] = np.array([0.30, 0.22, 0.12])
    alpha[lip] = np.clip(1 - (y[lip] - 44) / 6, 0, 1)
    save_rgba(OUT / "run" / "edge.png", np.clip(rgb, 0, 1), alpha)


def ar47_tree():
    """An autumn tree, 220 x 320: a trunk, and a crown of orange and red."""
    W, H = 220, 320
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rng = np.random.default_rng(475)
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    trunk = (np.abs(x - W / 2 - 4 * np.sin(y * 0.03)) < 9 + (y - 150) * 0.03) & (y > 140)
    rgb[trunk] = np.array([0.30, 0.20, 0.14])
    alpha[trunk] = 1
    for _ in range(60):
        r = rng.uniform(16, 30)
        cx = np.clip(rng.normal(W / 2, 42), r + 2, W - r - 2)
        cy = np.clip(rng.normal(118, 34), r + 2, 190)
        m = (x - cx) ** 2 + (y - cy) ** 2 < r * r
        c = np.array([(0.88, 0.42, 0.12), (0.95, 0.62, 0.16), (0.74, 0.24, 0.10), (0.86, 0.72, 0.24)][rng.integers(4)])
        lit = 1.1 - 0.35 * ((y - cy) / r).clip(-1, 1) * 0.5 - 0.15 * ((x - cx) / r)
        rgb[m] = (c * rng.uniform(0.85, 1.0) * lit[..., None])[m]
        alpha[m] = 1
    save_rgba(OUT / "run" / "tree.png", np.clip(rgb, 0, 1), alpha)


def ar47_fox():
    """A running fox, 104 x 60, side on, facing +x."""
    W, H = 104, 60
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    orange = np.array([0.88, 0.44, 0.14])
    body = ((x - 52) / 26) ** 2 + ((y - 32) / 11) ** 2 < 1
    tail = ((x - 18) / 18) ** 2 + ((y - 28 + 0.25 * (x - 18)) / 7) ** 2 < 1
    tip = tail & (x < 10)
    head = ((x - 82) / 12) ** 2 + ((y - 24) / 10) ** 2 < 1
    snout = (x > 86) & (x < 100) & (np.abs(y - 28) < 4 - (x - 86) * 0.22)
    ear = (np.abs(x - 80) < 4 - (14 - y) * 0.0) & (y > 9) & (y < 16) & (np.abs(x - 80) < (y - 8) * 0.6)
    legs = np.zeros_like(body)
    for lx, ang in ((40, -0.5), (46, 0.4), (62, -0.3), (68, 0.5)):
        legs |= (np.abs(x - lx - (y - 38) * ang) < 2.6) & (y > 36) & (y < 54)
    for m, c in ((tail, orange), (body, orange), (legs, np.array([0.25, 0.15, 0.10])), (head, orange),
                 (snout, orange), (ear, np.array([0.30, 0.16, 0.08]))):
        rgb[m] = c
        alpha[m] = 1
    rgb[tip] = np.array([0.97, 0.95, 0.92])
    chest = body & (y > 34) & (x > 64)
    rgb[chest] = np.array([0.96, 0.92, 0.86])
    rgb[(body | head | tail) & (y < 26) & ~tip] *= 1.12
    eye = (x - 87) ** 2 + (y - 22) ** 2 < 2.4
    rgb[eye] = 0.05
    nose = (x - 99) ** 2 + (y - 27) ** 2 < 3
    rgb[nose] = 0.05
    alpha[nose] = 1
    save_rgba(OUT / "run" / "fox.png", np.clip(rgb, 0, 1), alpha)


def ar47_leaf():
    """A falling leaf, 16 x 16."""
    S = 16
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    m = ((x - 8) / 6.5) ** 2 + ((y - 8) / 3.5) ** 2 < 1
    rgb = np.ones((S, S, 3)) * np.array([0.95, 0.55, 0.18])
    rgb[np.abs(y - 8) < 0.6] *= 0.7
    save_rgba(OUT / "run" / "leaf.png", rgb, m.astype(np.float64))


# ── 48 The Forge ───────────────────────────────────────────────────────
# A blacksmith's forge at night, side on.

def fg48_wall():
    """Rough stone blocks, 256 square, tiling."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    row = np.floor(y / 42)
    off = (row % 2) * 40
    col = np.floor((x + off) / 80)
    n = fbm(S, 16, 4)
    tone = 0.75 + 0.25 * (((row * 13 + col * 7) % 5) / 4)
    rgb = np.array([0.40, 0.34, 0.30])[None, None, :] * (tone * (0.8 + 0.3 * n))[..., None]
    mortar = ((y % 42) < 3) | (((x + off) % 80) < 3)
    rgb[mortar] = np.array([0.16, 0.13, 0.11])
    save_rgba(OUT / "forge" / "wall.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def fg48_hearth():
    """The forge hearth, 360 x 300: a stone hood over a bed of coals, an
    opening dark inside."""
    W, H = 360, 300
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    n = _sc_rect_fbm(W, H, 16, 3)
    hood = (y < 150) & (np.abs(x - W / 2) < 60 + y * 0.75)
    base = (y >= 150) & (np.abs(x - W / 2) < 170)
    stone = np.array([0.44, 0.38, 0.34])[None, None, :] * (0.75 + 0.35 * n)[..., None]
    for m in (hood, base):
        rgb[m] = stone[m]
        alpha[m] = 1
    blocks = ((y % 30) < 2) | (((x + (np.floor(y / 30) % 2) * 25) % 50) < 2)
    rgb[(hood | base) & blocks] *= 0.55
    mouth = (y > 160) & (y < 245) & (np.abs(x - W / 2) < 120)
    rgb[mouth] = np.array([0.06, 0.04, 0.03])
    coals = mouth & (y > 222)
    rng = np.random.default_rng(481)
    glow = np.clip(1 - np.abs(y - 236) / 14, 0, 1)
    rgb[coals] = (np.array([0.85, 0.30, 0.06]) * (0.6 + 0.6 * glow)[..., None] * (0.8 + 0.4 * rng.random((H, W)))[..., None])[coals]
    save_rgba(OUT / "forge" / "hearth.png", np.clip(rgb, 0, 1), alpha)


def fg48_anvil():
    """An anvil on a stump, 300 x 200."""
    W, H = 300, 200
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    face = (y > 20) & (y < 46) & (x > 60) & (x < 250)
    horn = (y > 20) & (y < 46 - (60 - x) * 0.3) & (x > 4) & (x <= 60) & (y > 20 + (60 - x) * 0.12)
    waist = (y >= 46) & (y < 100) & (np.abs(x - 160) < 40 + (y - 46) * 0.5)
    stump = (y >= 100) & (np.abs(x - 160) < 80)
    iron = np.array([0.24, 0.24, 0.27])
    for m in (face, horn, waist):
        rgb[m] = iron * (1.2 - 0.4 * (y[m] - 20) / 80)[..., None]
        alpha[m] = 1
    rgb[face & (y < 26)] = np.array([0.55, 0.55, 0.58])
    wood = np.array([0.36, 0.24, 0.14])
    rgb[stump] = (wood * (0.85 + 0.15 * np.sin(x * 0.4))[..., None])[stump]
    alpha[stump] = 1
    save_rgba(OUT / "forge" / "anvil.png", np.clip(rgb, 0, 1), alpha)


def fg48_hammer():
    """A smith's hammer, 60 x 200, the handle's end at the bottom (its pivot)."""
    W, H = 60, 200
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    handle = (np.abs(x - W / 2) < 5) & (y > 30)
    head = (y < 40) & (y > 4) & (np.abs(x - W / 2) < 26)
    rgb[handle] = np.array([0.50, 0.34, 0.20]) * (0.85 + 0.15 * (x[handle] < W / 2))[:, None]
    rgb[head] = np.array([0.32, 0.32, 0.35]) * (1.15 - 0.4 * y[head] / 40)[:, None]
    alpha[handle | head] = 1
    save_rgba(OUT / "forge" / "hammer.png", np.clip(rgb, 0, 1), alpha)


def fg48_bucket():
    """A quenching bucket of water, 120 x 110."""
    W, H = 120, 110
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    body = (y > 14) & (np.abs(x - W / 2) < 50 - (y - 14) * 0.08)
    rgb = np.zeros((H, W, 3))
    rgb[body] = np.array([0.38, 0.26, 0.15]) * (0.85 + 0.15 * (np.floor(x[body] / 12) % 2))[:, None]
    hoop = body & ((np.abs(y - 30) < 3) | (np.abs(y - 90) < 3))
    rgb[hoop] = np.array([0.22, 0.22, 0.24])
    water = (y > 12) & (y < 20) & (np.abs(x - W / 2) < 48)
    rgb[water] = np.array([0.18, 0.28, 0.35])
    save_rgba(OUT / "forge" / "bucket.png", np.clip(rgb, 0, 1), (body | water).astype(np.float64))


def fg48_bellows():
    """A pair of leather bellows, 220 x 90, the nozzle to the right."""
    W, H = 220, 90
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    body = ((x - 90) / 85) ** 2 + ((y - 45) / 32) ** 2 < 1
    nozzle = (x > 165) & (np.abs(y - 45) < 8 - (x - 165) * 0.08)
    rgb[body] = np.array([0.42, 0.26, 0.16]) * (0.8 + 0.25 * np.sin(x[body] * 0.25))[:, None]
    rgb[nozzle] = np.array([0.30, 0.30, 0.32])
    boards = body & ((np.abs(y - 15) < 4) | (np.abs(y - 75) < 4))
    rgb[boards] = np.array([0.50, 0.36, 0.22])
    save_rgba(OUT / "forge" / "bellows.png", np.clip(rgb, 0, 1), (body | nozzle).astype(np.float64))


def fg48_tools():
    """Tools hanging on the wall, 420 x 160: tongs, horseshoes, a rasp."""
    W, H = 420, 160
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    m = np.zeros((H, W), dtype=bool)
    m |= (np.abs(y - 8) < 3) & (x > 10) & (x < 410)
    for cx in (40, 50):
        m |= (np.abs(x - cx - (y - 20) * (0.08 if cx == 40 else -0.08)) < 2.5) & (y > 10) & (y < 150)
    for cx in (130, 200, 270):
        r = np.sqrt((x - cx) ** 2 + (y - 60) ** 2)
        m |= (np.abs(r - 26) < 6) & (y > 48)
        m |= (np.abs(x - cx) < 1.5) & (y > 8) & (y < 36)
    m |= (np.abs(x - 360) < 7) & (y > 14) & (y < 140)
    rgb = np.ones((H, W, 3)) * np.array([0.20, 0.19, 0.19])
    save_rgba(OUT / "forge" / "tools.png", rgb, m.astype(np.float64))


def fg48_bar():
    """The iron bar, 220 x 22, white - its glow is its sprite colour."""
    W, H = 220, 22
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    a = np.clip(np.minimum(np.minimum(x, W - x), np.minimum(y, H - y)) - 0.5, 0, 1)
    shade = 0.85 + 0.15 * (y < H / 2)
    save_rgba(OUT / "forge" / "bar.png", np.ones((H, W, 3)) * shade[..., None], a)


# ── 49 Stage Night ─────────────────────────────────────────────────────
# A band on a dark stage, front on.

def st49_curtain():
    """Velvet curtain, 256 x 256, tiling: deep folds, lit from the front."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    fold = 0.5 + 0.5 * np.sin(x / S * 2 * np.pi * 6 + 0.4 * np.sin(y / S * 2 * np.pi))
    fold = fold ** 1.6
    n = fbm(S, 32, 3)
    shade = 0.35 + 0.65 * fold + 0.08 * (n - 0.5)
    rgb = np.array([0.55, 0.08, 0.10])[None, None, :] * shade[..., None]
    save_rgba(OUT / "stage" / "curtain.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def st49_floor():
    """Stage boards, 256 x 64, tiling sideways: long planks, worn."""
    W, H = 256, 64
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    plank = np.floor(y / 16)
    n = fbm(256, 16, 3)[:H, :W]
    tone = 0.8 + 0.2 * ((plank * 5) % 3) / 2
    rgb = np.array([0.40, 0.28, 0.18])[None, None, :] * (tone * (0.8 + 0.3 * n))[..., None]
    seam = (y % 16) < 1.2
    rgb[seam] *= 0.4
    joint = (np.abs((x + plank * 97) % 256 - 128) < 1.0)
    rgb[joint] *= 0.5
    save_rgba(OUT / "stage" / "floor.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def st49_truss():
    """A lighting truss, 256 x 40, tiling: two rails and zig-zag braces."""
    W, H = 256, 40
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rails = (np.abs(y - 5) < 3) | (np.abs(y - 35) < 3)
    phase = (x % 32) / 32
    zig = np.abs(y - (5 + 30 * np.where(phase < 0.5, phase * 2, 2 - phase * 2))) < 2
    m = rails | zig
    rgb = np.ones((H, W, 3)) * np.array([0.30, 0.31, 0.34]) * (1.1 - 0.3 * (y / H))[..., None]
    save_rgba(OUT / "stage" / "truss.png", np.clip(rgb, 0, 1), m.astype(np.float64))


def st49_fixture():
    """A stage light hanging from a clamp, 44 x 56, lens at the bottom."""
    W, H = 44, 56
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    clamp = (np.abs(x - W / 2) < 4) & (y < 12)
    body = (np.abs(x - W / 2) < 16) & (y >= 10) & (y < 46)
    lens = (np.abs(x - W / 2) < 13) & (y >= 46) & (y < 52)
    rgb[clamp | body] = np.array([0.12, 0.12, 0.13])
    rgb[body & (np.abs(x - W / 2) < 13)] *= 1.6
    rgb[lens] = np.array([0.95, 0.95, 0.9])
    alpha[clamp | body | lens] = 1
    save_rgba(OUT / "stage" / "fixture.png", np.clip(rgb, 0, 1), alpha)


def _st_figure(name, W, H, parts):
    """A silhouette from ellipses and capsules: `parts` are
    (cx, cy, rx, ry) ellipses or ("line", x0, y0, x1, y1, width)."""
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    m = np.zeros((H, W), dtype=bool)
    for p in parts:
        if p[0] == "line":
            _, x0, y0, x1, y1, w = p
            d = np.array([x1 - x0, y1 - y0], dtype=np.float64)
            l2 = d @ d
            t = np.clip(((x - x0) * d[0] + (y - y0) * d[1]) / l2, 0, 1)
            m |= (x - (x0 + t * d[0])) ** 2 + (y - (y0 + t * d[1])) ** 2 < (w / 2) ** 2
        else:
            cx, cy, rx, ry = p
            m |= ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 < 1
    rgb = np.ones((H, W, 3)) * np.array([0.10, 0.09, 0.10])
    save_rgba(OUT / "stage" / f"{name}.png", rgb, m.astype(np.float64))


def st49_band():
    # A singer at a mic stand.
    _st_figure("singer", 120, 300, [
        (60, 30, 17, 20), ("line", 60, 50, 60, 165, 40), ("line", 52, 165, 40, 295, 16), ("line", 68, 165, 80, 295, 16),
        ("line", 50, 70, 95, 60, 12), ("line", 95, 60, 100, 52, 9),
        ("line", 104, 50, 104, 295, 4), ("line", 90, 295, 118, 295, 5), ("line", 98, 46, 108, 46, 6)])
    # A guitarist, the neck across the body.
    _st_figure("guitarist", 200, 300, [
        (95, 30, 17, 20), ("line", 95, 50, 92, 165, 40), ("line", 86, 165, 70, 295, 16), ("line", 100, 165, 116, 295, 16),
        (100, 140, 34, 26), ("line", 110, 135, 190, 90, 8), ("line", 75, 70, 110, 132, 11), ("line", 112, 72, 160, 104, 11)])
    # A drum kit with its drummer behind it.
    _st_figure("drums", 300, 220, [
        (150, 40, 18, 20), ("line", 150, 60, 150, 140, 40), ("line", 130, 75, 90, 110, 10), ("line", 170, 75, 220, 105, 10),
        (150, 170, 52, 48), (75, 125, 28, 14), (225, 125, 28, 14), ("line", 75, 125, 75, 215, 5),
        ("line", 225, 125, 225, 215, 5), (40, 70, 34, 5), ("line", 40, 70, 40, 215, 4), (262, 82, 32, 5),
        ("line", 262, 82, 262, 215, 4)])


# ── 50 Jellyfish Bloom ─────────────────────────────────────────────────
# Deep water: luminous jellyfish and kelp, every tentacle a chain of bones.

def jf50_bell():
    """A jellyfish bell, 128 x 96, seen from the side: a translucent dome,
    brightest at its rim and its frilled lower edge, with four gonads inside.
    Drawn white-ish so the sprite colour tints it; meant to be additive."""
    W, H = 128, 96
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cx, top, base = W / 2, 8.0, 78.0
    u = (x - cx) / (W * 0.46)
    v = (y - top) / (base - top)
    # The dome: inside where v reaches the curve 1 - u^2 (a parabola cap).
    edge = v - (u * u) ** 1.2
    frill = 3.0 * np.sin(x / W * np.pi * 14) * (v > 0.75)
    inside = (edge > 0) & (y < base + frill)
    d_edge = np.clip(edge * 6, 0, 1)
    rim = np.exp(-((y - (base + frill)) / 4.0) ** 2) * (np.abs(u) < 1.02)
    shell = np.exp(-(edge * 9) ** 2)
    body = 0.22 + 0.18 * (1 - v)
    gon = np.zeros_like(x)
    for gx in (-0.42, -0.14, 0.14, 0.42):
        gon += np.exp(-(((u - gx) / 0.11) ** 2 + ((v - 0.55) / 0.13) ** 2))
    n = fbm(128, 16, 3)[:H, :W]
    a = inside * (body + 0.55 * shell + 0.6 * gon) * (0.85 + 0.3 * n) + 0.9 * rim
    a *= np.clip(d_edge * 3 + rim, 0, 1)
    alpha = np.clip(a, 0, 1)
    rgb = np.ones((H, W, 3))
    rgb[..., 0] -= 0.25 * gon.clip(0, 1)
    save_rgba(OUT / "jelly" / "bell.png", np.clip(rgb, 0, 1), alpha)


def jf50_tentacle():
    """One link of a tentacle, 64 x 12, lying along x: a soft thread with a
    bead of stinging cells near its middle, faded at both ends so links
    overlapping at a joint do not double up into a seam."""
    W, H = 64, 12
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    across = np.exp(-((y - H / 2) / 1.8) ** 2)
    along = np.clip(np.minimum(x, W - x) / 8.0, 0, 1)
    bead = np.exp(-(((x - W * 0.55) / 4.0) ** 2 + ((y - H / 2) / 2.6) ** 2))
    alpha = np.clip(across * along * 0.8 + bead * 0.9, 0, 1)
    save_rgba(OUT / "jelly" / "tentacle.png", np.ones((H, W, 3)), alpha)


def jf50_arm():
    """One link of an oral arm, 64 x 28, along x: a ruffled ribbon, frilly
    at both edges, its width wavering so a chain of them reads as one."""
    W, H = 64, 28
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    half = 9.0 + 2.5 * np.sin(x / W * 2 * np.pi * 2)
    off = np.abs(y - H / 2)
    ruffle = 1.5 * np.sin(x / W * 2 * np.pi * 7 + y * 0.3)
    inside = off < half + ruffle
    core = np.exp(-(off / 3.0) ** 2)
    rim = np.exp(-((off - half - ruffle) / 1.6) ** 2)
    along = np.clip(np.minimum(x, W - x) / 6.0, 0, 1)
    alpha = np.clip((inside * 0.3 + core * 0.4 + rim * 0.7) * along, 0, 1)
    save_rgba(OUT / "jelly" / "arm.png", np.ones((H, W, 3)), alpha)


def jf50_blade():
    """A link of kelp, 64 x 30, along x: a wavy-edged blade with a midrib,
    olive, darker at its edges. Lit, so it shows where the jellies glow."""
    W, H = 64, 30
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    half = 10.0 + 2.0 * np.sin(x / W * 2 * np.pi * 3)
    off = np.abs(y - H / 2)
    inside = off < half
    alpha = np.clip((half - off) / 1.5, 0, 1) * inside
    tone = 0.75 + 0.25 * (1 - off / half.clip(1, None)) + 0.1 * np.sin(x * 0.7 + y * 0.4)
    rgb = np.array([0.30, 0.42, 0.16])[None, None, :] * tone[..., None]
    rib = off < 1.4
    rgb[rib] = np.array([0.45, 0.55, 0.24])
    save_rgba(OUT / "jelly" / "blade.png", np.clip(rgb, 0, 1), alpha)


# ── 51 Night Bloom ─────────────────────────────────────────────────────
# A meadow at night whose flowers open as they come into view.

def nb51_flower():
    """An open flower seen from the front, 128 x 128: six soft petals round
    a bright heart, white so the sprite colour tints it."""
    S = 128
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = (x - S / 2) / (S / 2), (y - S / 2) / (S / 2)
    r = np.hypot(dx, dy)
    a = np.arctan2(dy, dx)
    petal = 0.55 + 0.42 * np.abs(np.cos(a * 3))
    inside = r < petal
    edge = np.clip((petal - r) / 0.08, 0, 1)
    vein = 0.85 + 0.15 * np.cos(a * 3) ** 8
    shade = (0.55 + 0.45 * r / petal.clip(0.01, None)) * vein
    heart = np.exp(-(r / 0.16) ** 2)
    alpha = np.clip(edge * inside * 0.95 + heart, 0, 1)
    rgb = np.ones((S, S, 3)) * shade[..., None]
    rgb = rgb * (1 - heart[..., None]) + np.array([1.0, 0.95, 0.7]) * heart[..., None]
    save_rgba(OUT / "bloom" / "flower.png", np.clip(rgb, 0, 1), alpha)


def nb51_stem():
    """A stem with two leaves, 48 x 192, its foot at the bottom middle."""
    W, H = 48, 192
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    t = 1 - y / H
    cx = W / 2 + 5 * np.sin(t * 3.0)
    stem = np.abs(x - cx) < 2.2
    m = stem.astype(np.float64)
    for (ty, side) in ((0.32, 1), (0.55, -1)):
        # A leaf: an almond leaning up and out from the stem.
        ly = H * (1 - ty)
        # Its base on the stem as the stem bends at that height.
        u = (x - (W / 2 + 5 * np.sin(ty * 3.0) + side * 10)) / 11
        v = (y - (ly - 6)) / 5
        rot = 0.6 * side
        uu = u * np.cos(rot) - v * np.sin(rot)
        vv = u * np.sin(rot) + v * np.cos(rot)
        m = np.maximum(m, ((uu ** 2 + (vv * 1.0) ** 2) < 1).astype(np.float64))
    tone = 0.75 + 0.25 * t
    rgb = np.array([0.20, 0.42, 0.18])[None, None, :] * tone[..., None]
    save_rgba(OUT / "bloom" / "stem.png", np.clip(rgb, 0, 1), m)


def nb51_grass():
    """Meadow grass, 256 x 128, tiling sideways: blades along the top edge
    over a dark body."""
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rng = np.random.default_rng(51)
    top = np.full(W, 60.0)
    for _ in range(140):
        bx = rng.uniform(0, W)
        h = rng.uniform(18, 58)
        w = rng.uniform(2.0, 4.5)
        lean = rng.uniform(-6, 6)
        for k in range(int(h)):
            px = int((bx + lean * k / h) % W)
            for dx in range(-int(w), int(w) + 1):
                xx = (px + dx) % W
                if abs(dx) <= w * (1 - k / h):
                    top[xx] = min(top[xx], 60.0 - k)
    m = (y >= top[None, :]).astype(np.float64)
    n = fbm(256, 16, 3)[:H, :W]
    depth = np.clip((y - 40) / 88, 0, 1)
    rgb = np.array([0.22, 0.40, 0.20])[None, None, :] * (1.0 - 0.6 * depth + 0.15 * (n - 0.5))[..., None]
    save_rgba(OUT / "bloom" / "grass.png", np.clip(rgb, 0, 1), m)


def nb51_hills():
    """Far hills, 512 x 128, tiling sideways: a soft rolling silhouette."""
    W, H = 512, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    ph = x / W * 2 * np.pi
    ridge = 40 + 18 * np.sin(ph) + 10 * np.sin(ph * 3 + 1.0) + 5 * np.sin(ph * 7 + 2.0)
    m = np.clip(y - ridge, 0, 1)
    rgb = np.ones((H, W, 3)) * np.array([0.10, 0.12, 0.22])
    save_rgba(OUT / "bloom" / "hills.png", rgb, m)


def nb51_sky():
    """The night sky, 64 x 256: deep blue overhead to violet at the horizon."""
    W, H = 64, 256
    y = (np.mgrid[0:H, 0:W][0].astype(np.float64) + 0.5) / H
    top = np.array([0.03, 0.05, 0.14])
    low = np.array([0.22, 0.16, 0.36])
    rgb = top[None, None, :] * (1 - y[..., None] ** 1.6) + low[None, None, :] * (y[..., None] ** 1.6)
    save_rgba(OUT / "bloom" / "sky.png", rgb, np.ones((H, W)))


# ── 52 Model Railway ───────────────────────────────────────────────────
# A model railway board from above.

def mr52_board():
    """Flock grass on a model board, 256 x 256, tiling: soft green with
    tufts and the odd lighter patch."""
    S = 256
    n = fbm(S, 8, 4)
    m = fbm(S, 32, 3)
    tone = 0.75 + 0.35 * (n - 0.5) + 0.18 * (m - 0.5)
    rng = np.random.default_rng(52)
    speck = rng.random((S, S)) < 0.05
    rgb = np.array([0.30, 0.48, 0.20])[None, None, :] * tone[..., None]
    rgb[speck] *= 1.25
    save_rgba(OUT / "railway" / "board.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def mr52_pond():
    """A pond, 512 x 320: an oval of water, darker in the middle, with a
    sandy rim and a soft edge into the grass."""
    W, H = 512, 320
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm(512, 16, 3)[:H, :W]
    r = np.hypot((x - W / 2) / (W / 2 - 8), (y - H / 2) / (H / 2 - 8)) + 0.06 * (n - 0.5)
    water = r < 0.88
    rim = (r >= 0.88) & (r < 1.0)
    alpha = np.clip((1.0 - r) / 0.03, 0, 1)
    deep = np.clip(1 - r / 0.88, 0, 1)
    rgb = np.zeros((H, W, 3))
    rgb[...] = np.array([0.20, 0.42, 0.52]) * (0.75 + 0.25 * (1 - deep))[..., None] + 0.04 * (n[..., None] - 0.5)
    rgb[rim] = np.array([0.62, 0.55, 0.40])
    # A sheen across the water.
    sheen = np.exp(-(((x - W * 0.35) / 70) ** 2 + ((y - H * 0.35) / 25) ** 2)) * water
    rgb += 0.25 * sheen[..., None]
    save_rgba(OUT / "railway" / "pond.png", np.clip(rgb, 0, 1), alpha)


def _mr_box(W, H, body, roof, stripe=None, boiler=False):
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    alpha = np.zeros((H, W))
    inset = 2
    m = (x > inset) & (x < W - inset) & (y > inset) & (y < H - inset)
    rgb[m] = body
    alpha[m] = 1
    # The roof, a shade lighter towards the middle (it is curved).
    r = m & (x > 6) & (x < W - 6) & (y > 5) & (y < H - 5)
    curve = 1.0 + 0.25 * (1 - np.abs(y - H / 2) / (H / 2))
    rgb[r] = (np.array(roof)[None, None, :] * curve[..., None])[r]
    if stripe is not None:
        s = r & (np.abs(y - H / 2) < 1.5)
        rgb[s] = stripe
    if boiler:
        # The boiler runs forward (+x), with a chimney and a dome on it.
        b = (x > W * 0.38) & (x < W - 3) & (np.abs(y - H / 2) < H * 0.3)
        rgb[b] = np.array([0.10, 0.12, 0.10]) * (1.0 + 0.6 * (1 - np.abs(y - H / 2) / (H * 0.3)))[b][..., None]
        for cx, rad, col in ((W * 0.82, 4.5, (0.05, 0.05, 0.05)), (W * 0.6, 4.0, (0.75, 0.6, 0.25))):
            c = (x - cx) ** 2 + (y - H / 2) ** 2 < rad ** 2
            rgb[c] = col
        # Buffers.
        buf = (x > W - 4) & ((np.abs(y - H * 0.25) < 2) | (np.abs(y - H * 0.75) < 2))
        rgb[buf] = (0.7, 0.1, 0.08)
        alpha[buf] = 1
    return rgb, alpha


def mr52_engine():
    """A little steam engine from above, 96 x 36, chimney forward (+x)."""
    rgb, a = _mr_box(96, 36, (0.08, 0.22, 0.12), (0.10, 0.28, 0.16), boiler=True)
    save_rgba(OUT / "railway" / "engine.png", np.clip(rgb, 0, 1), a)


def mr52_carriage():
    """A carriage from above, 84 x 34: a maroon roof with a cream stripe."""
    rgb, a = _mr_box(84, 34, (0.25, 0.06, 0.06), (0.45, 0.10, 0.10), stripe=(0.92, 0.85, 0.65))
    save_rgba(OUT / "railway" / "carriage.png", np.clip(rgb, 0, 1), a)


# ── 53 Village Square ──────────────────────────────────────────────────
# A village square at dusk, seen from the front and a little above; the
# villagers are paper dolls of white parts the scene tints.

def _sq_mask_rgb(W, H):
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    return y, x, np.zeros((H, W, 3)), np.zeros((H, W))


def sq53_cobbles():
    """Cobbles, 240 x 128, tiling: rounded stones in rows, seen a little
    from above (wider than tall), mortar dark between them - ten stones a
    row, so the tile repeats without a seam."""
    W, H = 240, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    row = np.floor(y / 16)
    off = (row % 2) * 12
    cx = ((x + off) % 24) - 12
    cy = (y % 16) - 8
    d = (cx / 11.0) ** 2 + (cy / 7.0) ** 2
    stone = d < 1
    n = fbm(256, 16, 3)[:H, :W]
    n = 0.5 + 0.0 * n      # flat: noise that does not tile would show the seam
    rng = np.random.default_rng(53)
    tint = rng.uniform(0.85, 1.15, size=(H // 16, W // 24))
    t = tint[row.astype(int) % tint.shape[0], (((x + off) // 24) % tint.shape[1]).astype(int)]
    shade = (0.8 + 0.2 * (1 - np.clip(d, 0, 1))) * t * (0.9 + 0.2 * (n - 0.5))
    rgb = np.where(stone[..., None], np.array([0.55, 0.50, 0.45])[None, None, :] * shade[..., None],
                   np.array([0.20, 0.18, 0.16])[None, None, :])
    save_rgba(OUT / "square" / "cobbles.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def sq53_facades():
    """A row of house fronts, 512 x 220, tiling sideways, and the same
    windows alone as `windows.png` (white, for an additive glow)."""
    W, H = 512, 220
    y, x, rgb, a = _sq_mask_rgb(W, H)
    win = np.zeros((H, W))
    rng = np.random.default_rng(530)
    left = 0
    walls = [(0.62, 0.48, 0.36), (0.55, 0.52, 0.48), (0.66, 0.58, 0.42), (0.50, 0.40, 0.34)]
    k = 0
    while left < W:
        w = int(rng.integers(100, 150))
        if left + w > W:
            w = W - left
        top = int(rng.integers(20, 60))
        house = (x >= left) & (x < left + w) & (y >= top + 30)
        roof = (x >= left) & (x < left + w) & (y >= top) & (y < top + 30 + np.abs(x - (left + w / 2)) * 0)
        roof &= (y - top) > np.abs(x - (left + w / 2)) * 0.5 - 8
        rgb[house] = walls[k % len(walls)]
        a[house] = 1
        rgb[roof & ~house] = (0.30, 0.16, 0.12)
        a[roof & ~house] = 1
        # Timber lines.
        beam = house & ((np.abs(y - (top + 90)) < 2) | (np.abs(x - left) < 2) | (np.abs(x - (left + w - 1)) < 2))
        rgb[beam] = (0.25, 0.16, 0.10)
        for wy in (top + 55, top + 115):
            for wx in np.arange(left + 18, left + w - 24, 38):
                m = (x >= wx) & (x < wx + 18) & (y >= wy) & (y < wy + 24) & (y < H - 50)
                lit = rng.random() < 0.7
                rgb[m] = (0.95, 0.75, 0.40) if lit else (0.12, 0.12, 0.16)
                if lit:
                    win[m] = 1
        door = (x >= left + w / 2 - 12) & (x < left + w / 2 + 12) & (y >= H - 46)
        rgb[door] = (0.28, 0.17, 0.10)
        left += w
        k += 1
    shade = 0.85 + 0.15 * (y / H)
    save_rgba(OUT / "square" / "facades.png", np.clip(rgb * shade[..., None], 0, 1), a)
    save_rgba(OUT / "square" / "windows.png", np.ones((H, W, 3)), win)


def sq53_well():
    """A well with a little roof, 160 x 180, its foot at the bottom."""
    W, H = 160, 180
    y, x, rgb, a = _sq_mask_rgb(W, H)
    body = (np.abs(x - W / 2) < 56) & (y > 110) & (y < H - 2)
    brick = ((np.floor(y / 12) + np.floor((x + (np.floor(y / 12) % 2) * 10) / 20)) % 2)
    rgb[body] = (np.array([0.55, 0.53, 0.50]) * (0.85 + 0.15 * brick)[..., None])[body]
    rim = (np.abs(x - W / 2) < 60) & (np.abs(y - 112) < 6)
    rgb[rim] = (0.45, 0.43, 0.40)
    posts = ((np.abs(x - 32) < 5) | (np.abs(x - 128) < 5)) & (y > 30) & (y < 112)
    rgb[posts] = (0.35, 0.22, 0.12)
    roof = (y > 10) & (y < 40) & (np.abs(x - W / 2) < 76 - (y - 10) * 0.0) & ((y - 10) > np.abs(x - W / 2) * 0.35 - 6)
    rgb[roof] = (0.40, 0.18, 0.12)
    m = body | rim | posts | roof
    a[m] = 1
    save_rgba(OUT / "square" / "well.png", np.clip(rgb, 0, 1), a)


def sq53_lamp():
    """A lamp post, 40 x 210, its foot at the bottom, the lantern on top."""
    W, H = 40, 210
    y, x, rgb, a = _sq_mask_rgb(W, H)
    pole = (np.abs(x - W / 2) < 3) & (y > 40)
    base = (np.abs(x - W / 2) < 8) & (y > H - 14)
    cage = (np.abs(x - W / 2) < 13) & (y > 8) & (y < 42)
    glass = (np.abs(x - W / 2) < 9) & (y > 13) & (y < 38)
    rgb[pole | base | cage] = (0.12, 0.12, 0.13)
    rgb[glass] = (1.0, 0.85, 0.55)
    a[pole | base | cage] = 1
    save_rgba(OUT / "square" / "lamp.png", rgb, a)


def sq53_tree():
    """A round tree, 200 x 260, its foot at the bottom."""
    W, H = 200, 260
    y, x, rgb, a = _sq_mask_rgb(W, H)
    n = fbm_rect(W, H, 12, 4)
    crown = ((x - W / 2) / 92) ** 2 + ((y - 95) / 90) ** 2 + 0.3 * (n - 0.5) < 1
    trunk = (np.abs(x - W / 2) < 10 + (y - 150) * 0.05) & (y > 150)
    rgb[trunk] = (0.30, 0.20, 0.12)
    light = np.clip(1.1 - ((x - W * 0.4) ** 2 + (y - 60) ** 2) / 14000, 0.5, 1.1)
    rgb[crown] = (np.array([0.22, 0.40, 0.18]) * (light * (0.8 + 0.4 * n))[..., None])[crown]
    a[crown | trunk] = 1
    save_rgba(OUT / "square" / "tree.png", np.clip(rgb, 0, 1), a)


def sq53_crates():
    """Two stacked crates and a sack, 100 x 90, foot at the bottom."""
    W, H = 100, 90
    y, x, rgb, a = _sq_mask_rgb(W, H)
    for (x0, y0, s) in ((6, 44, 44), (50, 50, 38), (24, 4, 40)):
        m = (x >= x0) & (x < x0 + s) & (y >= y0) & (y < y0 + s)
        edge = m & ((x < x0 + 4) | (x >= x0 + s - 4) | (y < y0 + 4) | (y >= y0 + s - 4) | (np.abs((x - x0) - (y - y0)) < 3))
        rgb[m] = (0.55, 0.38, 0.20)
        rgb[edge] = (0.38, 0.25, 0.12)
        a[m] = 1
    save_rgba(OUT / "square" / "crates.png", rgb, a)


def sq53_parts():
    """A villager's parts, white so the scene tints each: a tunic, a head,
    two hats, an arm and a leg (both hang from their top)."""
    def save(name, W, H, mask, shade=None):
        y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
        m = mask(x, y, W, H)
        s = shade(x, y, W, H) if shade else np.ones((H, W))
        save_rgba(OUT / "square" / f"{name}.png", np.clip(s[..., None] * np.ones((H, W, 3)), 0, 1), m.astype(np.float64))

    side_light = lambda x, y, W, H: 0.8 + 0.25 * (1 - x / W)
    save("body", 44, 54, lambda x, y, W, H: np.abs(x - W / 2) < 13 + 9 * (y / H), side_light)
    save("head", 28, 30, lambda x, y, W, H: ((x - W / 2) / 13) ** 2 + ((y - H / 2) / 14) ** 2 < 1, side_light)
    save("hood", 36, 34, lambda x, y, W, H: (((x - W / 2) / 17) ** 2 + ((y - H * 0.62) / 15) ** 2 < 1)
         | ((y < H * 0.6) & (np.abs(x - W / 2) < (y / (H * 0.6)) * 14)), side_light)
    save("hat", 46, 22, lambda x, y, W, H: ((y > 14) & (np.abs(x - W / 2) < 22)) | ((y <= 14) & (np.abs(x - W / 2) < 11)),
         side_light)
    save("arm", 10, 32, lambda x, y, W, H: (np.abs(x - W / 2) < 4.5) & (y < H - 3) | (((x - W / 2) ** 2 + (y - (H - 5)) ** 2) < 20),
         side_light)
    save("leg", 12, 30, lambda x, y, W, H: (np.abs(x - W / 2) < 5) | ((y > H - 6) & (x > W / 2 - 5)),
         side_light)


# ── 54 Tesla Lab ───────────────────────────────────────────────────────
# A dark laboratory: coils, mirrors, a laser.

def tl54_wall():
    """Riveted wall panels, 256 x 256, tiling: dark steel plates."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    n = fbm(S, 16, 3)
    seam = (np.minimum(x % 128, 128 - x % 128) < 2) | (np.minimum(y % 128, 128 - y % 128) < 2)
    rivet = np.zeros((S, S), dtype=bool)
    for cx in (10, 118, 138, 246):
        for cy in (10, 118, 138, 246):
            rivet |= (x - cx) ** 2 + (y - cy) ** 2 < 9
    tone = 0.22 + 0.06 * (n - 0.5) + 0.03 * (y % 128) / 128
    rgb = np.array([0.55, 0.6, 0.7])[None, None, :] * tone[..., None]
    rgb[seam] *= 0.5
    rgb[rivet] = (0.35, 0.37, 0.42)
    save_rgba(OUT / "lab" / "wall.png", np.clip(rgb, 0, 1), np.ones((S, S)))


def tl54_floor():
    """Floor plates, 128 x 64, tiling sideways: a diamond tread."""
    W, H = 128, 64
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    tread = (np.abs(((x + y) % 16) - 8) < 1.2) | (np.abs(((x - y) % 16) - 8) < 1.2)
    rgb = np.ones((H, W, 3)) * np.array([0.20, 0.21, 0.23])
    rgb[tread] = (0.30, 0.31, 0.34)
    save_rgba(OUT / "lab" / "floor.png", rgb, np.ones((H, W)))


def tl54_coil():
    """A Tesla coil, 120 x 300, its foot at the bottom: a base, a copper
    winding up a tall column, and a torus on top."""
    W, H = 120, 300
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    a = np.zeros((H, W))
    base = (np.abs(x - W / 2) < 46) & (y > H - 40)
    col = (np.abs(x - W / 2) < 16) & (y > 70) & (y <= H - 40)
    wind = col & ((y % 6) < 3.5)
    torus = ((x - W / 2) / 56) ** 2 + ((y - 50) / 22) ** 2 < 1
    shine = np.clip(1 - np.abs(x - W * 0.42) / 30, 0, 1)
    rgb[base] = (0.18, 0.18, 0.2)
    rgb[col] = (0.55, 0.30, 0.15)
    rgb[wind] = (0.80, 0.48, 0.25)
    rgb[torus] = (np.array([0.6, 0.62, 0.66]) * (0.6 + 0.6 * shine)[..., None])[torus]
    a[base | col | torus] = 1
    save_rgba(OUT / "lab" / "coil.png", np.clip(rgb, 0, 1), a)


def tl54_box():
    """A laser emitter, 90 x 50: a grey box with a dark lens on the right."""
    W, H = 90, 50
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.ones((H, W, 3)) * np.array([0.35, 0.37, 0.4]) * (0.8 + 0.3 * (1 - y / H))[..., None]
    lens = (x > W - 14) & (np.abs(y - H / 2) < 9)
    rgb[lens] = (0.1, 0.05, 0.05)
    stripe = (y > 6) & (y < 12)
    rgb[stripe] = (0.85, 0.65, 0.1)
    save_rgba(OUT / "lab" / "emitter.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def tl54_mirror():
    """A mirror on a stand, 16 x 90: a silver face, its back dark."""
    W, H = 16, 90
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    rgb[...] = (0.15, 0.15, 0.17)
    face = x > W - 6
    rgb[face] = (np.array([0.75, 0.85, 0.95]) * (0.7 + 0.3 * np.sin(y / H * np.pi))[..., None])[face]
    save_rgba(OUT / "lab" / "mirror.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def tl54_hazard():
    """Hazard tape, 32 x 16, tiling along a line: yellow and black stripes."""
    W, H = 32, 16
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    s = ((x + y) % 32) < 16
    rgb = np.where(s[..., None], np.array([0.95, 0.75, 0.1])[None, None, :], np.array([0.08, 0.08, 0.08])[None, None, :])
    save_rgba(OUT / "lab" / "hazard.png", rgb, np.ones((H, W)))


# ── 55 Dragon's Hoard ──────────────────────────────────────────────────
# Treasure in a vault: every piece a diffuse and a normal map made from one
# height field, the normal map's alpha marking what is metal (specular).

def _hd_normals(height, strength):
    """Encode the normals of a height field (0..1, y down in the image)."""
    gy, gx = np.gradient(height)
    nx, ny, nz = -gx * strength, gy * strength, np.ones_like(height)
    l = np.sqrt(nx * nx + ny * ny + nz * nz)
    return np.stack([nx / l, ny / l, nz / l], axis=-1) * 0.5 + 0.5


def _hd_save(name, diffuse, alpha, height, strength, shine):
    save_rgba(OUT / "hoard" / f"{name}.png", np.clip(diffuse, 0, 1), alpha)
    n = _hd_normals(height, strength)
    # The normal map's alpha is the specular mask: where the piece shines.
    save_rgba(OUT / "hoard" / f"{name}_n.png", n, np.clip(shine, 0, 1) * (alpha > 0.01))


def hd55_coin():
    """A gold coin, 64 x 64: a raised rim, a face stamped with a star."""
    S = 64
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.hypot(x - S / 2, y - S / 2) / (S / 2 - 1)
    a = np.arctan2(y - S / 2, x - S / 2)
    disc = r < 1
    rim = np.clip(1 - np.abs(r - 0.86) / 0.1, 0, 1)
    star = (r < 0.55 * (0.6 + 0.4 * np.abs(np.cos(a * 2.5)))).astype(np.float64)
    height = (0.6 * rim + 0.35 * star + 0.15 * (1 - r)) * disc
    gold = np.array([0.95, 0.72, 0.25])
    diffuse = gold[None, None, :] * (0.7 + 0.35 * height)[..., None]
    _hd_save("coin", diffuse, disc.astype(np.float64), height, 6.0, np.ones((S, S)))


def hd55_shield():
    """A round shield, 160 x 160: a painted dome in an iron rim, riveted."""
    S = 160
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.hypot(x - S / 2, y - S / 2) / (S / 2 - 2)
    a = np.arctan2(y - S / 2, x - S / 2)
    disc = r < 1
    dome = np.sqrt(np.clip(1 - r * r, 0, 1)) * 0.8
    rim = np.clip(1 - np.abs(r - 0.92) / 0.07, 0, 1)
    rivet = np.zeros((S, S))
    for k in range(12):
        ang = k * np.pi / 6
        cx, cy = S / 2 + np.cos(ang) * S * 0.38, S / 2 + np.sin(ang) * S * 0.38
        rivet += np.clip(1 - np.hypot(x - cx, y - cy) / 4.5, 0, 1)
    boss = np.clip(1 - r / 0.18, 0, 1) ** 0.7
    height = (dome * 0.5 + rim * 0.6 + rivet * 0.5 + boss * 0.6) * disc
    paint = np.where((np.cos(a * 2) > 0) ^ (r < 0.5), 1.0, 0.0)
    red, cream = np.array([0.55, 0.08, 0.06]), np.array([0.85, 0.78, 0.6])
    diffuse = red[None, None, :] * paint[..., None] + cream[None, None, :] * (1 - paint)[..., None]
    metal = (rim > 0.05) | (rivet > 0.05) | (boss > 0.05)
    iron = np.array([0.55, 0.56, 0.6])
    diffuse[metal] = (iron[None, :] * (0.7 + 0.4 * height[metal][:, None]))
    shine = np.where(metal, 1.0, 0.15)
    _hd_save("shield", diffuse, disc.astype(np.float64), height, 9.0, shine)


def hd55_sword():
    """A sword lying along x, 256 x 48: a ridged blade, a crossguard and a
    leather grip with a pommel."""
    W, H = 256, 48
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cy = H / 2
    blade_half = np.where(x > 230, (256 - x) / 26 * 9, 9.0)
    blade = (x > 70) & (np.abs(y - cy) < blade_half)
    ridge = np.clip(1 - np.abs(y - cy) / np.maximum(blade_half, 1), 0, 1)
    guard = (x > 58) & (x <= 70) & (np.abs(y - cy) < 20)
    grip = (x > 18) & (x <= 58) & (np.abs(y - cy) < 5)
    pommel = np.hypot(x - 14, y - cy) < 8
    alpha = (blade | guard | grip | pommel).astype(np.float64)
    wrap = 0.5 + 0.5 * np.sin((x - y) * 0.9)
    height = np.where(blade, ridge * 0.9, 0) + np.where(guard, 0.6, 0) + np.where(grip, 0.4 * wrap, 0) \
        + np.where(pommel, np.clip(1 - np.hypot(x - 14, y - cy) / 8, 0, 1), 0)
    diffuse = np.zeros((H, W, 3))
    diffuse[blade] = (0.75, 0.78, 0.82)
    diffuse[guard] = (0.8, 0.62, 0.25)
    diffuse[grip] = (0.3, 0.18, 0.1)
    diffuse[pommel] = (0.8, 0.62, 0.25)
    shine = np.where(blade | guard | pommel, 1.0, 0.05)
    _hd_save("sword", diffuse, alpha, height, 5.0, shine)


def hd55_goblet():
    """A golden goblet side on, 80 x 120: a round cup, a stem, a foot - the
    normals turn round it as round as a cylinder's."""
    W, H = 80, 120
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cup_w = np.where(y < 55, 34 * np.sqrt(np.clip(1 - ((y - 8) / 50) ** 2, 0.15, 1)), 0)
    stem_w = np.where((y >= 50) & (y < 100), 6 + 4 * np.cos((y - 50) / 50 * np.pi * 2) ** 2, 0)
    foot_w = np.where(y >= 98, 26 - (y - 98) * 0.3, 0)
    half = np.maximum(np.maximum(cup_w, stem_w), foot_w)
    alpha = (np.abs(x - W / 2) < half) & (y > 6) & (y < H - 2)
    u = np.clip((x - W / 2) / np.maximum(half, 1), -1, 1)
    height = np.sqrt(np.clip(1 - u * u, 0, 1)) * alpha
    band = (np.abs(y - 26) < 4) & alpha
    gold = np.array([0.95, 0.7, 0.25])
    diffuse = gold[None, None, :] * (0.65 + 0.35 * height)[..., None]
    diffuse[band] = (0.55, 0.1, 0.12)
    shine = np.where(band, 0.4, 1.0)
    _hd_save("goblet", diffuse, alpha.astype(np.float64), height * 0.8 + band * 0.1, 7.0, shine)


def hd55_gem():
    """A cut gem, 48 x 48, white so each one is tinted: eight facets round a
    flat table, each with its own normal."""
    S = 48
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    dx, dy = (x - S / 2) / (S / 2 - 1), (y - S / 2) / (S / 2 - 1)
    a = np.arctan2(dy, dx)
    k = np.round(a / (np.pi / 4)) * (np.pi / 4)
    radial = dx * np.cos(k) + dy * np.sin(k)
    alpha = (np.abs(dx) + np.abs(dy) < 1.2) & (np.hypot(dx, dy) < 1)
    height = np.where(radial < 0.45, 1.0, 1.0 - (radial - 0.45) * 1.8) * alpha
    shade = 0.6 + 0.4 * np.cos(k - 0.8) * (radial >= 0.45) + 0.3 * (radial < 0.45)
    _hd_save("gem", np.ones((S, S, 3)) * shade[..., None], alpha.astype(np.float64), height, 14.0, np.ones((S, S)))


def hd55_stone():
    """Vault wall blocks, 256 x 256, tiling: bevelled stones, rough."""
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    row = np.floor(y / 64)
    off = (row % 2) * 64
    bx = (x + off) % 128
    by = y % 64
    edge = np.minimum(np.minimum(bx, 128 - bx), np.minimum(by, 64 - by))
    n = fbm(S, 16, 4)
    height = np.clip(edge / 8, 0, 1) * (0.8 + 0.2 * n)
    diffuse = np.array([0.42, 0.40, 0.38])[None, None, :] * (0.75 + 0.35 * n)[..., None]
    _hd_save("stone", diffuse, np.ones((S, S)), height, 4.0, 0.08 * np.ones((S, S)))


# ── 56 Haunted Hall ────────────────────────────────────────────────────
# A candle-lit hall and the ghosts in it, each a figure of parts.

def hh56_robe():
    """A ghost's robe, 120 x 180, white: shoulders at the top, widening and
    fraying into wisps at the bottom, fading as it goes."""
    W, H = 120, 180
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    t = y / H
    half = 30 + 28 * t
    wisp = 10 * np.sin(x / W * np.pi * 5) * (t > 0.7) * (t - 0.7) / 0.3
    inside = (np.abs(x - W / 2) < half) & (y < H - 4 - np.maximum(wisp, -12) - 14 * (t > 0.85) * np.abs(np.sin(x * 0.2)))
    shoulder = ((x - W / 2) / 34) ** 2 + ((y - 18) / 18) ** 2 < 1
    m = (inside & (y > 10)) | shoulder
    alpha = m * np.clip(1.15 - t ** 2.2, 0, 1)
    shade = 0.8 + 0.2 * np.cos((x - W / 2) / half * 1.4)
    save_rgba(OUT / "hall" / "robe.png", np.ones((H, W, 3)) * shade[..., None], alpha)


def hh56_head():
    """A ghost's head, 70 x 80, white, with dark hollow eyes and a mouth."""
    W, H = 70, 80
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    head = ((x - W / 2) / 32) ** 2 + ((y - H * 0.5) / 38) ** 2 < 1
    rgb = np.ones((H, W, 3)) * (0.85 + 0.15 * (1 - y / H))[..., None]
    for cx in (W * 0.33, W * 0.67):
        eye = ((x - cx) / 7) ** 2 + ((y - H * 0.42) / 10) ** 2 < 1
        rgb[eye] = (0.05, 0.08, 0.15)
    mouth = ((x - W / 2) / 6) ** 2 + ((y - H * 0.7) / 8) ** 2 < 1
    rgb[mouth] = (0.05, 0.08, 0.15)
    save_rgba(OUT / "hall" / "head.png", rgb, head.astype(np.float64))


def hh56_arm():
    """A sleeve, 34 x 100, white, hanging from its top, a hand at the end."""
    W, H = 34, 100
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    half = 9 + 6 * (y / H)
    sleeve = (np.abs(x - W / 2) < half) & (y < H - 18)
    hand = ((x - W / 2) / 9) ** 2 + ((y - (H - 12)) / 11) ** 2 < 1
    alpha = (sleeve | hand).astype(np.float64)
    shade = np.where(hand, 0.95, 0.82)
    save_rgba(OUT / "hall" / "arm.png", np.ones((H, W, 3)) * shade[..., None], alpha)


def hh56_portrait():
    """A portrait in a gilt frame, 140 x 180: a dim figure on a dark ground."""
    W, H = 140, 180
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    frame = (x < 14) | (x > W - 14) | (y < 14) | (y > H - 14)
    n = fbm(256, 16, 3)[:H, :W]
    rgb = np.ones((H, W, 3)) * np.array([0.12, 0.10, 0.08]) * (0.8 + 0.4 * n)[..., None]
    figure = (((x - W / 2) / 30) ** 2 + ((y - 70) / 34) ** 2 < 1) | ((np.abs(x - W / 2) < 44 - (y - 100) * -0.1) & (y > 100))
    rgb[figure & ~frame] = np.array([0.35, 0.25, 0.18]) * (0.8 + 0.4 * n[figure & ~frame])[..., None]
    gild = 0.75 + 0.25 * np.sin((x + y) * 0.3)
    rgb[frame] = (np.array([0.75, 0.55, 0.2])[None, :] * gild[frame][:, None])
    save_rgba(OUT / "hall" / "portrait.png", np.clip(rgb, 0, 1), np.ones((H, W)))


def hh56_candelabra():
    """A candelabra, 120 x 200, its foot at the bottom: three candles on
    branching brass arms."""
    W, H = 120, 200
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3))
    a = np.zeros((H, W))
    stem = (np.abs(x - W / 2) < 4) & (y > 60)
    base = (np.abs(x - W / 2) < 26 - (H - y) * 0.6) & (y > H - 20)
    arm = (np.abs(y - (70 + 0.004 * (x - W / 2) ** 2)) < 3) & (np.abs(x - W / 2) < 44)
    brass = stem | base | arm
    rgb[brass] = (0.6, 0.45, 0.2)
    a[brass] = 1
    for cx in (W / 2 - 42, W / 2, W / 2 + 42):
        candle = (np.abs(x - cx) < 6) & (y > 30) & (y < 70)
        rgb[candle] = (0.92, 0.88, 0.78)
        a[candle] = 1
    save_rgba(OUT / "hall" / "candelabra.png", rgb, a)


# ── 57 Picture House ───────────────────────────────────────────────────
# A small cinema: rows of seats and the heads of the audience.

def ph57_seats():
    """A row of seat backs, 256 x 70, tiling sideways: rounded tops, red
    velvet, a darker gap between each."""
    W, H = 256, 70
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cx = (x % 64) - 32
    top = 14 + 8 * (cx / 30) ** 2
    seat = (np.abs(cx) < 28) & (y > top)
    rgb = np.zeros((H, W, 3))
    shade = 0.6 + 0.4 * (1 - np.abs(cx) / 30)
    rgb[seat] = (np.array([0.42, 0.06, 0.08])[None, :] * shade[seat][:, None])
    # The screen's light catches the top of each back.
    rim = seat & ~np.roll(seat, 4, axis=0)
    rgb[rim] = (0.75, 0.32, 0.22)
    a = seat.astype(np.float64)
    save_rgba(OUT / "cinema" / "seats.png", np.clip(rgb, 0, 1), a)


def ph57_viewer():
    """Someone watching, 80 x 110, seen from behind: head and shoulders,
    dark - a silhouette against the screen's light."""
    W, H = 80, 110
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    head = ((x - W / 2) / 18) ** 2 + ((y - 32) / 22) ** 2 < 1
    shoulders = (((x - W / 2) / 38) ** 2 + ((y - 110) / 50) ** 2 < 1) & (y > 58)
    m = head | shoulders
    rgb = np.ones((H, W, 3)) * np.array([0.10, 0.09, 0.11])
    # A warm rim where the screen's light catches the hair and shoulders.
    rim = m & ~np.roll(m, 3, axis=0)
    rgb[rim] = (0.62, 0.40, 0.28)
    save_rgba(OUT / "cinema" / "viewer.png", rgb, m.astype(np.float64))


def ph57_projector():
    """The projector's window in the back wall, 90 x 60: a frame round a
    bright lens."""
    W, H = 90, 60
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    frame = (x < 8) | (x > W - 8) | (y < 8) | (y > H - 8)
    lens = ((x - W / 2) / 14) ** 2 + ((y - H / 2) / 14) ** 2 < 1
    rgb = np.ones((H, W, 3)) * np.array([0.08, 0.07, 0.08])
    rgb[frame] = (0.25, 0.2, 0.18)
    rgb[lens] = (1.0, 0.95, 0.85)
    save_rgba(OUT / "cinema" / "projector.png", rgb, np.ones((H, W)))


def ph57_beam():
    """The projector's beam, 512 x 256, from a point at the left widening to
    the screen's height at the right, soft-edged, faintly streaked."""
    W, H = 512, 256
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    u = x / W
    half = 4 + (H / 2 - 6) * u
    d = np.abs(y - H / 2) / half
    edge = np.clip((1 - d) / 0.25, 0, 1) ** 1.5
    streak = 0.85 + 0.15 * np.sin(y / H * 40 + np.sin(y / H * 13) * 3)
    a = edge * (0.55 + 0.45 * (1 - u)) * np.clip(u / 0.04, 0, 1) * streak
    save_rgba(OUT / "cinema" / "beam.png", 1.0, np.clip(a, 0, 1))


# ── 58 Lamplit Mine ────────────────────────────────────────────────────
# A side-view mine: rock tiles with slopes and ceiling corners, planks and
# a crystal vein, a back wall, posts; boulders and lanterns.

LM_T = 32


def lm58_noise(seed, cells):
    """Smooth value noise over one tile, wrapping at its edges so a tile
    meets itself without a seam."""
    rng = np.random.default_rng(seed)
    g = rng.random((cells, cells))
    y, x = np.mgrid[0:LM_T, 0:LM_T].astype(np.float64) + 0.5
    fx, fy = x / LM_T * cells, y / LM_T * cells
    x0, y0 = np.floor(fx).astype(int), np.floor(fy).astype(int)
    tx, ty = fx - x0, fy - y0
    tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
    x1, y1 = (x0 + 1) % cells, (y0 + 1) % cells
    x0, y0 = x0 % cells, y0 % cells
    return (g[y0, x0] * (1 - tx) + g[y0, x1] * tx) * (1 - ty) + (g[y1, x0] * (1 - tx) + g[y1, x1] * tx) * ty


def lm58_stone(seed, base, contrast=0.3, pebbles=5, strata=0.0, grain=580):
    """Stone: fine grain, a few pebbles, and - for the back wall - level
    strata whose period divides the tile, so they run on across the grid.
    Variants share the grain (`grain`), which wraps at the tile's edges, so
    any two meet without a seam; only their pebbles (`seed`) differ."""
    T = LM_T
    y, x = np.mgrid[0:T, 0:T].astype(np.float64) + 0.5
    n = 0.6 * lm58_noise(grain, 8) + 0.4 * lm58_noise(grain + 1, 16)
    shade = 1 - contrast / 2 + contrast * n
    shade *= 1 + strata * np.sin(y / T * 2 * np.pi * 2 + 0.6 * np.sin(x / T * 2 * np.pi))
    rgb = np.array(base)[None, None, :] * shade[..., None]
    rng = np.random.default_rng(seed + 7)
    for _ in range(pebbles):
        px, py = rng.uniform(4, T - 4, 2)
        rx, ry = rng.uniform(1.5, 3.5), rng.uniform(1.0, 2.5)
        d = ((x - px) / rx) ** 2 + ((y - py) / ry) ** 2
        rgb[d < 1] *= rng.uniform(0.75, 1.2)
        rgb[(d >= 1) & (d < 1.6) & (y > py)] *= 0.85
    return np.clip(rgb, 0, 1)


def lm58_tiles():
    """The mine's tileset, 8 x 2 tiles of 32 px. Row one: four rocks, floor
    slopes rising right and left, ceiling corners solid at the upper left
    and upper right. Row two: a plank, a crystal vein, a post, four back
    walls. Four of each stone so the grid does not show. The shapes each
    tile collides and casts with are the tileset's `shapes` in
    tools/gen_scenes.py."""
    T = LM_T
    y, x = np.mgrid[0:T, 0:T].astype(np.float64) + 0.5
    rocks = [lm58_stone(580 + 10 * k, (0.47, 0.40, 0.35)) for k in range(4)]
    rock_a, rock_b = rocks[0], rocks[1]
    out = np.zeros((T * 2, T * 8, 4))

    def put(i, rgb, a):
        r, c = divmod(i, 8)
        out[r * T:(r + 1) * T, c * T:(c + 1) * T, :3] = rgb
        out[r * T:(r + 1) * T, c * T:(c + 1) * T, 3] = a

    full = np.ones((T, T))
    for k, rock in enumerate(rocks):
        put(k, rock, full)
    # Slopes and corners: the rock masked to a triangle, its cut face
    # catching a little light.
    for i, inside, face in (
        (4, x + y >= T, np.abs(x + y - T)),       # floor rising right: solid lower right
        (5, x <= y, np.abs(x - y)),               # floor rising left: solid lower left
        (6, x + y <= T, np.abs(x + y - T)),       # ceiling, solid upper left
        (7, x >= y, np.abs(x - y)),               # ceiling, solid upper right
    ):
        rgb = rock_a.copy()
        rim = inside & (face < 2.5)
        rgb[rim] = np.clip(rgb[rim] * 1.35 + 0.04, 0, 1)
        put(i, rgb, inside.astype(np.float64))
    # A plank: the top ten pixels, grain and two nails.
    plank = np.zeros((T, T, 3)) + np.array([0.42, 0.27, 0.15])
    plank *= (0.85 + 0.15 * np.sin(x * 0.9 + np.sin(y * 1.7) * 2.0))[..., None]
    plank[(y < 1.5) | (np.abs(y - 9.5) < 1.0)] *= 0.6
    for nx in (4.5, T - 4.5):
        plank[(np.abs(x - nx) < 1.2) & (np.abs(y - 5) < 1.2)] = (0.25, 0.25, 0.27)
    put(8, plank, (y < 10).astype(np.float64))
    # Crystal: shards over the rock, pale and bright.
    crystal = rock_b.copy() * 0.8
    for cx, cy, ang, ln, wd in ((10, 22, -0.5, 18, 4.5), (18, 24, 0.15, 20, 5.0), (24, 20, 0.7, 14, 3.5),
                                (14, 12, -0.9, 10, 3.0), (26, 10, 0.4, 9, 2.6)):
        ux, uy = np.sin(ang), -np.cos(ang)
        along = (x - cx) * ux + (y - cy) * uy
        across = np.abs((x - cx) * uy - (y - cy) * ux)
        shard = (along > 0) & (along < ln) & (across < wd * (1 - along / ln) + 0.6)
        tone = 0.75 + 0.25 * np.clip(across / wd, 0, 1)
        crystal[shard] = np.array([0.55, 0.95, 1.0]) * tone[shard][:, None]
    put(9, crystal, full)
    # Back walls: the mine's far face - darker, cooler, smoother stone.
    for k in range(4):
        put(11 + k, lm58_stone(640 + 10 * k, (0.31, 0.29, 0.29), contrast=0.2, pebbles=2, strata=0.08, grain=640), full)
    # A post: a squared timber up the middle.
    post = np.zeros((T, T, 3)) + np.array([0.36, 0.23, 0.13])
    post *= (0.85 + 0.15 * np.sin(y * 0.7 + np.sin(x * 2.1)))[..., None]
    post[np.abs(x - T / 2) > 4.5] *= 0.7
    put(10, post, (np.abs(x - T / 2) < 6).astype(np.float64))
    save_rgba(OUT / "mine" / "tiles.png", out[..., :3], out[..., 3])


def lm58_boulder():
    """A boulder, 48 px: a round grey stone, lit from the upper left."""
    S = 48
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    d = np.hypot(x - S / 2, y - S / 2) / (S / 2 - 1)
    rng = np.random.default_rng(58)
    g = rng.random((6, 6))
    ix, iy = (x / S * 5).astype(int), (y / S * 5).astype(int)
    n = g[iy, ix]
    light = np.clip(0.75 - 0.35 * ((x - S * 0.35) / S + (y - S * 0.3) / S), 0.35, 1.0)
    rgb = np.array([0.45, 0.42, 0.40])[None, None, :] * (light * (0.85 + 0.25 * n))[..., None]
    a = np.clip((1 - d) * S / 3, 0, 1)
    save_rgba(OUT / "mine" / "boulder.png", np.clip(rgb, 0, 1), a)


def lm58_lantern():
    """A miner's lantern, 24 x 34: a dark frame round a glowing glass, a
    ring at the top."""
    W, H = 24, 34
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3)) + np.array([0.12, 0.10, 0.09])
    a = np.zeros((H, W))
    body = (np.abs(x - W / 2) < 9) & (y > 8) & (y < 31)
    ring = (np.abs(np.hypot(x - W / 2, y - 5) - 3.5) < 1.2)
    glass = (np.abs(x - W / 2) < 6) & (y > 11) & (y < 28)
    a[body | ring] = 1
    rgb[glass] = np.array([1.0, 0.85, 0.55]) * (1 - 0.3 * np.abs(x[glass] - W / 2) / 6)[:, None]
    save_rgba(OUT / "mine" / "lantern.png", rgb, a)


# ── 59 Windswept Meadow ────────────────────────────────────────────────
# Grass blades and flowers (swayed by the meadow_sway graph), a banner,
# the sky, hills and clouds behind them.

def wm59_blades():
    """Three blades of grass, 16 x 80 each, side by side (48 x 80): a thin
    one, a broad one, a forked one - tapering to their tips, darker at the
    root, a pale midrib. Drawn upright; the material leans them."""
    W, H = 16, 80
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    out_rgb = np.zeros((H, W * 3, 3))
    out_a = np.zeros((H, W * 3))
    up = 1 - y / H                                  # 1 at the tip
    for i, (width, bend) in enumerate(((4.5, 1.5), (7.0, -1.0), (5.5, 0.5))):
        cx = W / 2 + bend * up ** 2 * 3
        half = width * (1 - up ** 1.6) + 0.4
        d = np.abs(x - cx)
        a = np.clip(half - d + 0.5, 0, 1)
        if i == 2:  # a second, shorter tip
            cx2 = W / 2 - 3 - 2 * up ** 2
            half2 = 3.5 * (1 - (up / 0.75) ** 1.6) * (up < 0.75) + 0.3
            a = np.maximum(a, np.clip(half2 - np.abs(x - cx2) + 0.5, 0, 1) * (up < 0.75))
        rgb = np.zeros((H, W, 3))
        rgb[..., 0] = 0.36 + 0.30 * up
        rgb[..., 1] = 0.48 + 0.30 * up
        rgb[..., 2] = 0.18 + 0.10 * up
        rib = np.clip(1 - d / 0.9, 0, 1)[..., None] * 0.12
        out_rgb[:, i * W:(i + 1) * W] = np.clip(rgb + rib, 0, 1)
        out_a[:, i * W:(i + 1) * W] = a
    save_rgba(OUT / "meadow" / "blades.png", out_rgb, out_a)


def wm59_flowers():
    """Three flowers on stems, 32 x 96 each (96 x 96): a daisy, a poppy, a
    cornflower - the head at the top, the stem to the foot."""
    W, H = 32, 96
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W * 3, 3))
    a = np.zeros((H, W * 3))
    hx, hy = W / 2, 14.0
    stem = (np.abs(x - hx - (y - hy) * 0.03) < 1.3) & (y > hy)
    leaf = (((x - hx - 6) / 6) ** 2 + ((y - 62) / 2.5) ** 2 < 1) | (((x - hx + 6) / 6) ** 2 + ((y - 74) / 2.5) ** 2 < 1)
    ang = np.arctan2(y - hy, x - hx)
    r = np.hypot(x - hx, y - hy)
    for i, (petal_col, centre_col, petals, size) in enumerate((
            ((0.97, 0.96, 0.92), (0.95, 0.75, 0.15), 12, 11.0),    # daisy
            ((0.90, 0.16, 0.10), (0.12, 0.08, 0.08), 5, 12.0),     # poppy
            ((0.30, 0.45, 0.95), (0.20, 0.25, 0.60), 8, 10.0))):   # cornflower
        shape = size * (0.72 + 0.28 * np.abs(np.cos(ang * petals / 2)))
        head = r < shape
        centre = r < size * 0.32
        c = np.zeros((H, W, 3))
        c[stem | leaf] = (0.30, 0.55, 0.20)
        c[head] = petal_col
        c[head] *= (0.8 + 0.2 * (1 - r[head] / size))[:, None]
        c[centre] = centre_col
        al = (stem | leaf | head).astype(np.float64)
        rgb[:, i * W:(i + 1) * W] = np.clip(c, 0, 1)
        a[:, i * W:(i + 1) * W] = al
    save_rgba(OUT / "meadow" / "flowers.png", rgb, a)


def wm59_sky():
    """The sky, 8 x 512: a deep blue overhead to a warm haze at the horizon."""
    H = 512
    t = (np.arange(H) / (H - 1))[:, None, None]
    top, mid, low = np.array([0.28, 0.48, 0.82]), np.array([0.55, 0.72, 0.92]), np.array([0.98, 0.86, 0.66])
    rgb = np.where(t < 0.6, top + (mid - top) * (t / 0.6), mid + (low - mid) * ((t - 0.6) / 0.4))
    rgb = np.repeat(rgb, 8, axis=1)
    save_rgba(OUT / "meadow" / "sky.png", rgb, np.ones((H, 8)))


def wm59_hills():
    """Two bands of hills, 1024 x 256: far (blue-green, soft) and near
    (green), their ridges sums of slow waves."""
    W, H = 1024, 256
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    for name, base, phase, amp, col, shade in (
            ("hills_far", 120, 0.0, 40, (0.42, 0.58, 0.55), 0.10),
            ("hills_near", 90, 2.0, 55, (0.36, 0.60, 0.26), 0.18)):
        u = x / W * 2 * np.pi
        ridge = base - amp * (0.55 * np.sin(u * 1 + phase) + 0.3 * np.sin(u * 2.3 + phase * 1.7) + 0.15 * np.sin(u * 5.1 + 0.4))
        inside = y > ridge
        depth = np.clip((y - ridge) / 140, 0, 1)
        rgb = np.array(col)[None, None, :] * (1 - shade * depth)[..., None]
        save_rgba(OUT / "meadow" / f"{name}.png", np.clip(rgb, 0, 1), inside.astype(np.float64))


def wm59_cloud():
    """A soft cumulus, 256 x 128: overlapping puffs, a shaded underside."""
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rng = np.random.default_rng(59)
    a = np.zeros((H, W))
    for _ in range(14):
        cx, cy = rng.uniform(50, W - 50), rng.uniform(55, 85)
        r = rng.uniform(22, 42)
        a = np.maximum(a, np.clip(1 - np.hypot(x - cx, (y - cy) * 1.2) / r, 0, 1) ** 0.6)
    a *= np.clip((100 - y) / 20, 0, 1)
    shade = np.clip(0.75 + 0.25 * (1 - y / H) * 1.6, 0, 1)
    rgb = np.ones((H, W, 3)) * shade[..., None] * np.array([1.0, 0.98, 0.96])
    save_rgba(OUT / "meadow" / "cloud.png", np.clip(rgb, 0, 1), np.clip(a, 0, 1))


def wm59_banner():
    """A banner, 256 x 128: crimson, a gold border and a sun in the middle,
    a swallowtail cut at its free end."""
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    rgb = np.zeros((H, W, 3)) + np.array([0.70, 0.08, 0.10])
    border = (y < 8) | (y > H - 8) | (x < 6)
    rgb[border] = (0.95, 0.75, 0.25)
    r = np.hypot(x - 110, y - H / 2)
    ang = np.arctan2(y - H / 2, x - 110)
    sun = r < 22 + 6 * (np.cos(ang * 12) > 0.3)
    rgb[sun] = (0.97, 0.80, 0.30)
    rgb[r < 16] = (0.99, 0.88, 0.45)
    notch = x > W - 40 + np.abs(y - H / 2) * 0.62
    a = (~notch).astype(np.float64)
    save_rgba(OUT / "meadow" / "banner.png", rgb, a)


# ── 61 Firefly Glade ───────────────────────────────────────────────────
# A glade at dusk: two lines of pines in silhouette, a still pond.

def fg61_pines():
    """Two lines of pine silhouettes, 1600 x 360 each: far (smaller, paler)
    and near (taller, darker), each a row of tiered triangles on a ragged
    ground line."""
    from PIL import Image as _I, ImageDraw as _D
    W, H = 1600, 360
    rng = np.random.default_rng(61)
    for name, n, hmin, hmax in (("pines_far", 46, 120, 220), ("pines_near", 26, 200, 330)):
        img = _I.new("L", (W, H), 0)
        d = _D.Draw(img)
        d.rectangle([0, H - 30, W, H], fill=255)
        xs = np.sort(rng.uniform(-40, W + 40, n))
        for x in xs:
            h = rng.uniform(hmin, hmax)
            w = h * rng.uniform(0.32, 0.42)
            base = H - 20 - rng.uniform(0, 12)
            for k in range(4):
                top = base - h + k * h * 0.18
                bottom = base - h * (0.62 - 0.17 * k)
                half = w * (0.35 + 0.22 * k)
                d.polygon([(x, top), (x - half, bottom), (x + half, bottom)], fill=255)
            d.rectangle([x - 4, base - h * 0.15, x + 4, base], fill=255)
        a = np.asarray(img).astype(np.float64) / 255.0
        rgb = np.ones((H, W, 3))
        save_rgba(OUT / "glade" / f"{name}.png", rgb, a)


def fg61_pond():
    """A pond, 900 x 120: a dark oval of still water, a pale rim of light."""
    W, H = 900, 120
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    d = ((x - W / 2) / (W / 2 - 4)) ** 2 + ((y - H / 2) / (H / 2 - 4)) ** 2
    a = np.clip((1 - d) * 25, 0, 1)
    rim = np.clip(1 - np.abs(np.sqrt(np.maximum(d, 0)) - 0.94) / 0.05, 0, 1)
    rgb = np.ones((H, W, 3)) * np.array([0.10, 0.14, 0.24])
    rgb += rim[..., None] * np.array([0.06, 0.08, 0.12])
    # The sky's light on the water, brightest toward the far bank.
    rgb += (np.clip(1 - y / H, 0, 1) ** 2 * 0.08)[..., None] * np.array([0.6, 0.7, 1.0])
    save_rgba(OUT / "glade" / "pond.png", np.clip(rgb, 0, 1), a)


# ── 62 Phoenix ─────────────────────────────────────────────────────────
# A firebird in silhouette, wings spread: its outline is what burns.

def px62_phoenix():
    """The phoenix, 512 x 384: a body and head facing up, wings spread
    wide in long swept feathers, a tail of three plumes - a warm silhouette
    with a hotter core, for the fire to pour off."""
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    W, H = 512, 384
    mask = _I.new("L", (W, H), 0)
    d = _D.Draw(mask)
    cx = W / 2
    # Body and neck, head and beak.
    d.ellipse([cx - 34, 150, cx + 34, 270], fill=255)
    d.polygon([(cx - 16, 165), (cx + 16, 165), (cx + 10, 105), (cx - 10, 105)], fill=255)
    d.ellipse([cx - 22, 70, cx + 22, 112], fill=255)
    d.polygon([(cx - 6, 75), (cx + 6, 75), (cx, 48)], fill=255)
    # A crest of three plumes.
    for k, a in enumerate((-0.5, 0.0, 0.5)):
        x2, y2 = cx + 60 * math.sin(a), 74 - 60 * math.cos(a) * (0.8 + 0.2 * k % 2)
        d.line([(cx, 80), (x2, y2)], fill=255, width=6)
    # Wings: swept feathers fanning out to each side.
    for side in (-1, 1):
        for f in range(7):
            t = f / 6
            root = (cx + side * 28, 175 + t * 30)
            tip = (cx + side * (245 - 30 * t), 40 + 120 * t + 20 * t * t)
            mid = ((root[0] + tip[0]) / 2 + side * 10, (root[1] + tip[1]) / 2 - 30 + 18 * t)
            d.polygon([root, mid, tip, (mid[0] - side * 4, mid[1] + 26), (root[0], root[1] + 26)], fill=255)
    # Tail: three long plumes curving down.
    for k, off in enumerate((-40, 0, 40)):
        pts = [(cx + off * (t / 10) ** 1.5 + 18 * math.sin(t * 0.5 + k), 255 + t * 10) for t in range(11)]
        d.line(pts, fill=255, width=12 - k % 2 * 3)
        d.ellipse([pts[-1][0] - 12, pts[-1][1] - 8, pts[-1][0] + 12, pts[-1][1] + 14], fill=255)
    mask = mask.filter(_F.GaussianBlur(0.8))
    a = np.asarray(mask).astype(np.float64) / 255.0
    # Colour: a hot core fading to deep red at the edges.
    core = np.asarray(mask.filter(_F.GaussianBlur(14))).astype(np.float64) / 255.0
    heat = np.clip(core * 1.4, 0, 1)
    rgb = np.zeros((H, W, 3))
    rgb[..., 0] = 0.75 + 0.25 * heat
    rgb[..., 1] = 0.18 + 0.62 * heat ** 1.5
    rgb[..., 2] = 0.05 + 0.35 * heat ** 3
    save_rgba(OUT / "phoenix" / "phoenix.png", np.clip(rgb, 0, 1), a)


# ── 63 Turning Seasons ─────────────────────────────────────────────────
# Four colour grades as LUT strips (32 slices), made from the neutral strip
# the way an artist would grade a screenshot. They live in assets/luts/,
# outside sprites/, which the asset pipeline packs into atlases - a strip
# packed into an atlas is no strip.

def ts63_luts():
    N = 32
    x = np.arange(N * N)
    r = np.broadcast_to((x % N) / (N - 1), (N, N * N))
    b = np.broadcast_to((x // N) / (N - 1), (N, N * N))
    g = np.broadcast_to(np.arange(N)[:, None] / (N - 1), (N, N * N))
    n = np.stack([r, g, b], -1)
    lum = n @ np.array([0.299, 0.587, 0.114])
    out = OUT.parent / "luts"
    out.mkdir(parents=True, exist_ok=True)

    def save(name, rgb):
        rgb = np.clip(rgb, 0, 1)
        img = np.dstack([rgb, np.ones(rgb.shape[:2])])
        Image.fromarray((img * 255).round().astype(np.uint8), "RGBA").save(out / f"{name}.png")
        print(f"wrote luts/{name}.png")

    save("summer", n)
    # Autumn: greens turn to gold and rust, the light warm and low.
    autumn = np.stack([n[..., 0] * 1.02 + n[..., 1] * 0.22, n[..., 1] * 0.78 + n[..., 0] * 0.06,
                       n[..., 2] * 0.7], -1)
    save("autumn", 0.5 + (autumn - 0.5) * 1.08)
    # Winter: most of the colour gone, the blacks lifted, a cold light.
    winter = (lum[..., None] * 0.65 + n * 0.35) * np.array([0.92, 0.98, 1.08]) * 0.9 + 0.1
    save("winter", winter)
    # Dusk: the light fallen and pink, the shadows violet.
    dusk = n * np.array([1.0, 0.72, 0.86]) * 0.78 + np.array([0.05, 0.0, 0.08]) * (1 - lum[..., None])
    save("dusk", 0.5 + (dusk - 0.5) * 1.1)


# ── 64 Storming the Gate ───────────────────────────────────────────────
# The knights' parts, each drawn lying along +x as its bone does (a part's
# sprite hangs from the bone's start), a plank of the rope bridge with its
# two ropes, and a banner. The knights face left, so for an upright bone the
# image's top rows (local +y) are the side they look to: the light falls on
# that edge, and the helmet's visor is there.

def sg64_part(w, h, base, rim=0.18, round_ends=True):
    """A rounded bar w x h: steel or cloth `base`, lit along its top edge."""
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    v = (y + 0.5) / h                          # 0 top, 1 bottom
    r = h * 0.5
    if round_ends:
        cx = np.clip(x + 0.5, r, w - r)
        inside = (x + 0.5 - cx) ** 2 + (y + 0.5 - r) ** 2 <= r * r
    else:
        inside = np.ones((h, w), bool)
    shade = 0.62 + 0.55 * np.cos((v - 0.32) * np.pi) ** 2 - 0.25 * v
    rgb = np.asarray(base)[None, None, :] * shade[..., None]
    rgb = rgb + rim * np.exp(-((v - 0.22) / 0.08) ** 2)[..., None]
    # A darker line where plates meet, every third of the part.
    for k in (1, 2):
        rgb[:, int(w * k / 3)] *= 0.72
    return np.clip(rgb, 0, 1), inside.astype(np.float64)


def sg64_knights():
    out = OUT / "gate"
    steel = (0.62, 0.66, 0.72)
    # Helmet: +x is up the spine, -y (the top rows) is the face.
    s = 34
    nx, ny, d = radius(s)
    a = smoothstep(1.0, 0.92, d)
    shade = np.clip(0.78 + 0.25 * nx - 0.3 * ny, 0.3, 1.2)
    rgb = np.asarray(steel)[None, None, :] * shade[..., None]
    slit = (nx > 0.0) & (nx < 0.16) & (ny < -0.15) & (ny > -0.95)
    rgb[slit] = (0.08, 0.08, 0.1)
    crest = (nx > 0.62) & (ny > -0.35) & (ny < 0.75)
    rgb[crest] = (0.75, 0.15, 0.12)
    save_rgba(out / "helmet.png", np.clip(rgb, 0, 1), a)
    # Torsos: a breastplate under a tabard, red and blue.
    for name, cloth in (("red", (0.62, 0.12, 0.1)), ("blue", (0.15, 0.25, 0.6))):
        rgb, a = sg64_part(64, 26, steel)
        y, x = np.mgrid[0:26, 0:64].astype(np.float64)
        tab = (x > 6) & (x < 52)
        rgb[tab] = rgb[tab] * 0.25 + np.asarray(cloth) * (0.75 + 0.3 * (1 - y[tab] / 26))[..., None] * 0.9
        cross = tab & ((np.abs(x - 30) < 3) | (np.abs(y - 13) < 2.5))
        rgb[cross] = (0.92, 0.85, 0.6)
        rgb[:, 8:12] = (0.35, 0.22, 0.1)       # the belt, at the hip
        save_rgba(out / f"torso_{name}.png", rgb, a)
    rgb, a = sg64_part(46, 13, steel)
    save_rgba(out / "arm.png", rgb, a)
    rgb, a = sg64_part(54, 15, (0.5, 0.53, 0.58))
    save_rgba(out / "leg.png", rgb, a)
    # A spear: a shaft and a leaf blade at +x.
    w, h = 170, 14
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    cy = h / 2
    shaft = (np.abs(y + 0.5 - cy) < 2.2) & (x < 140)
    t = np.clip((x - 140) / 30, 0, 1)
    blade = (x >= 140) & (np.abs(y + 0.5 - cy) < 6.5 * np.sin(np.pi * np.clip(t * 0.95 + 0.05, 0, 1)))
    rgb = np.zeros((h, w, 3))
    rgb[shaft] = (0.45, 0.3, 0.16)
    rgb[blade] = (0.85, 0.88, 0.92)
    save_rgba(out / "spear.png", rgb, (shaft | blade).astype(np.float64))
    # A plank of the bridge, a rope along the top of it.
    w, h = 64, 22
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    grain = 0.85 + 0.15 * np.sin(x * 0.35 + 3 * np.sin(y * 0.6)) + 0.05 * np.random.default_rng(64).random((h, w))
    wood = np.asarray((0.5, 0.33, 0.18))[None, None, :] * grain[..., None]
    board = (y >= 8) & (y < 20) & (x >= 1) & (x < w - 1)
    rope = (y >= 1) & (y < 4)
    rgb = np.zeros((h, w, 3))
    rgb[board] = wood[board] * (1.0 - 0.3 * ((y[board] - 8) / 12))[..., None]
    rgb[rope] = (0.62, 0.52, 0.34)
    tie = (x >= w / 2 - 2) & (x < w / 2 + 2) & (y >= 3) & (y < 9)
    rgb[tie] = (0.55, 0.45, 0.3)
    save_rgba(out / "plank.png", np.clip(rgb, 0, 1), (board | rope | tie).astype(np.float64))
    # A banner on the wall, red with a gold cross, its foot cut in a swallowtail.
    w, h = 60, 120
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    notch = (y > h - 22) & (np.abs(x - w / 2) < (y - (h - 22)) * 0.9)
    a = (~notch).astype(np.float64)
    fold = 0.82 + 0.18 * np.sin(x / w * np.pi * 3)
    rgb = np.asarray((0.6, 0.1, 0.08))[None, None, :] * fold[..., None]
    cross = (np.abs(x - w / 2) < 4) | (np.abs(y - 42) < 4)
    rgb[cross & (y < h - 26)] = (0.9, 0.75, 0.3)
    save_rgba(out / "banner.png", np.clip(rgb, 0, 1), a)


# ── 66 Puppet Theatre ──────────────────────────────────────────────────
# A toy theatre: the proscenium (a wall with a gilt arch and a hole for the
# stage), velvet curtains and a valance, two painted backdrops, stage boards,
# and the cast as card cut-outs - each with a white card rim round it.

def pt66_rim(alpha, r=4):
    """The cut-out's card: its shape grown by `r` pixels."""
    grown = alpha > 0.5
    out = grown.copy()
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy <= r * r:
                out |= np.roll(np.roll(grown, dy, 0), dx, 1)
    return out


def pt66_cutout(path, rgb, alpha, r=4):
    """Paint `rgb` over a white card the shape of `alpha` grown by `r`."""
    card = pt66_rim(alpha, r)
    out = np.ones(rgb.shape) * np.array([0.93, 0.9, 0.84])
    a = alpha[..., None]
    out = out * (1 - a) + rgb * a
    save_rgba(path, np.clip(out, 0, 1), card.astype(np.float64))


def pt66_theatre():
    out = OUT / "theatre"
    rng = np.random.default_rng(66)
    # The proscenium: a dark red wall, a gilt frame and an arched opening.
    W, H = 1280, 720
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    ox0, ox1, oy0, oy1 = 180, 1100, 110, 590     # the opening, image pixels
    cx = (ox0 + ox1) / 2
    arch_r = (ox1 - ox0) / 2
    arch_y = oy0 + 70                            # where the arch's curve starts
    def inside(pad):
        box = (x > ox0 - pad) & (x < ox1 + pad) & (y < oy1 + pad) & (y > arch_y)
        dome = ((x - cx) / (arch_r + pad)) ** 2 + ((y - arch_y) / (70 + pad)) ** 2 < 1
        return box | (dome & (y <= arch_y))
    hole = inside(0)
    frame = inside(26) & ~hole
    wall = np.asarray((0.30, 0.08, 0.07)) * (0.85 + 0.15 * rng.random((H, W)))[..., None]
    wall *= (0.75 + 0.25 * np.cos((x - cx) / W * np.pi))[..., None]
    rgb = wall.copy()
    gold = np.asarray((0.86, 0.66, 0.28))
    # A bevel: bands round the opening, lit on the inner edge.
    shine = np.where(inside(7), 1.25, np.where(inside(14), 1.0, np.where(inside(21), 0.8, 0.62)))
    rgb[frame] = (gold * shine[..., None])[frame]
    alpha = (~hole).astype(np.float64)
    save_rgba(out / "frame.png", np.clip(rgb, 0, 1), alpha)
    # A curtain: folds, darker at its foot, a gilt hem.
    cw, ch = 520, 560
    y, x = np.mgrid[0:ch, 0:cw].astype(np.float64)
    folds = 0.55 + 0.45 * np.cos(x * 2 * np.pi / 46) ** 2
    rgb = np.asarray((0.62, 0.06, 0.08)) * (folds * (1.0 - 0.25 * y / ch))[..., None]
    rgb[y > ch - 26] = (gold * (0.8 + 0.2 * np.cos(x[y > ch - 26] * 0.6))[..., None])
    save_rgba(out / "curtain.png", np.clip(rgb, 0, 1), np.ones((ch, cw)))
    # The valance: a swag across the top with a fringe.
    vw, vh = 1000, 100
    y, x = np.mgrid[0:vh, 0:vw].astype(np.float64)
    swag = 60 + 22 * np.abs(np.sin(x / vw * np.pi * 5))
    body = y < swag
    rgb = np.asarray((0.55, 0.05, 0.07)) * (0.7 + 0.3 * np.cos(x * 2 * np.pi / 46) ** 2)[..., None]
    fringe = (y >= swag) & (y < swag + 14) & ((x.astype(int) % 6) < 3)
    rgb[fringe] = gold
    save_rgba(out / "valance.png", np.clip(rgb, 0, 1), (body | fringe).astype(np.float64))
    # Two painted backdrops.
    bw, bh = 920, 480
    y, x = np.mgrid[0:bh, 0:bw].astype(np.float64)
    brush = 0.94 + 0.06 * rng.random((bh, bw))
    def hills(base, amp, freq, phase):
        return base + amp * np.sin(x / bw * np.pi * freq + phase) + amp * 0.4 * np.sin(x / bw * np.pi * freq * 2.7 + phase)
    day = np.stack([0.45 + 0.4 * y / bh, 0.68 + 0.25 * y / bh, 0.95 - 0.05 * y / bh], -1)
    for base, amp, f, ph, col in ((300, 30, 3, 0.4, (0.42, 0.62, 0.36)), (350, 24, 5, 1.7, (0.30, 0.52, 0.28)),
                                  (410, 16, 7, 0.2, (0.22, 0.42, 0.22))):
        day[y > hills(base, amp, f, ph)] = col
    for cx2, cy2, s in ((180, 110, 1.0), (560, 80, 0.8), (760, 150, 0.9)):
        for dx, dy, r in ((0, 0, 34), (34, 8, 26), (-30, 10, 24), (12, -16, 22)):
            day[((x - cx2 - dx * s) ** 2 + (y - cy2 - dy * s) ** 2) < (r * s) ** 2] = (0.97, 0.97, 0.95)
    save_rgba(out / "backdrop_day.png", np.clip(day * brush[..., None], 0, 1), np.ones((bh, bw)))
    night = np.stack([0.05 + 0.08 * y / bh, 0.07 + 0.1 * y / bh, 0.22 + 0.12 * y / bh], -1)
    stars = rng.random((bh, bw)) > 0.9985
    night[stars & (y < 300)] = (0.95, 0.92, 0.75)
    # A castle on the far hill.
    castle = ((x > 600) & (x < 760) & (y > 210) & (y < 330)) | ((x > 590) & (x < 625) & (y > 170) & (y < 330)) | \
             ((x > 735) & (x < 770) & (y > 160) & (y < 330))
    night[castle] = (0.12, 0.12, 0.2)
    night[castle & ((x.astype(int) % 40) < 8) & (y > 240) & (y < 256)] = (0.95, 0.75, 0.35)
    for base, amp, f, ph, col in ((320, 30, 3, 0.4, (0.10, 0.16, 0.22)), (370, 24, 5, 1.7, (0.07, 0.12, 0.17)),
                                  (420, 16, 7, 0.2, (0.05, 0.08, 0.12))):
        night[y > hills(base, amp, f, ph)] = col
    save_rgba(out / "backdrop_night.png", np.clip(night * brush[..., None], 0, 1), np.ones((bh, bw)))
    # The stage boards.
    fw, fh = 1000, 70
    y, x = np.mgrid[0:fh, 0:fw].astype(np.float64)
    grain = 0.8 + 0.12 * np.sin(x * 0.08 + 4 * np.sin(y * 0.3)) + 0.05 * rng.random((fh, fw))
    rgb = np.asarray((0.45, 0.29, 0.16)) * grain[..., None]
    rgb[(x.astype(int) % 125) < 2] *= 0.55
    rgb[y < 4] = (0.62, 0.45, 0.26)
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((fh, fw)))

    # The cast, facing right (the knight) and left (the dragon).
    def canvas(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64)
        return x, y, np.zeros((h, w, 3)), np.zeros((h, w))
    def paint(rgb, a, mask, col):
        rgb[mask] = col
        a[mask] = 1.0
    # The knight, 150 x 190.
    x, y, rgb, a = canvas(150, 190)
    paint(rgb, a, (x - 60) ** 2 / 26 ** 2 + (y - 168) ** 2 / 8 ** 2 < 1, (0.35, 0.22, 0.12))        # boots
    paint(rgb, a, (np.abs(x - 52) < 8) & (y > 120) & (y < 168), (0.55, 0.58, 0.64))                # legs
    paint(rgb, a, (np.abs(x - 70) < 8) & (y > 120) & (y < 168), (0.5, 0.53, 0.6))
    paint(rgb, a, (np.abs(x - 61) < 22) & (y > 64) & (y < 126), (0.62, 0.12, 0.12))                # tabard
    paint(rgb, a, (np.abs(x - 61) < 4) & (y > 70) & (y < 120), (0.92, 0.8, 0.4))
    paint(rgb, a, (x - 61) ** 2 + (y - 46) ** 2 < 20 ** 2, (0.7, 0.73, 0.78))                       # helmet
    paint(rgb, a, (x > 64) & (x < 82) & (np.abs(y - 46) < 3), (0.08, 0.08, 0.1))                    # visor
    paint(rgb, a, (np.abs(x - 58) < 4) & (y > 14) & (y < 30), (0.85, 0.15, 0.12))                   # plume
    paint(rgb, a, (x - 34) ** 2 + (y - 98) ** 2 < 22 ** 2, (0.2, 0.3, 0.65))                        # shield
    paint(rgb, a, (x - 34) ** 2 + (y - 98) ** 2 < 8 ** 2, (0.92, 0.8, 0.4))
    paint(rgb, a, (x > 84) & (x < 140) & (np.abs(y - 84 + (x - 84) * 0.9) < 3), (0.85, 0.88, 0.92))  # sword
    paint(rgb, a, (np.abs(x - 84) < 6) & (np.abs(y - 86) < 6), (0.92, 0.8, 0.4))
    pt66_cutout(out / "knight.png", rgb, a)
    # The dragon, 300 x 210, facing left.
    x, y, rgb, a = canvas(300, 210)
    green, dark = np.asarray((0.25, 0.55, 0.28)), np.asarray((0.15, 0.38, 0.18))
    paint(rgb, a, (x - 160) ** 2 / 90 ** 2 + (y - 130) ** 2 / 42 ** 2 < 1, green)                    # body
    paint(rgb, a, (x - 70) ** 2 / 34 ** 2 + (y - 80) ** 2 / 24 ** 2 < 1, green)                      # head
    paint(rgb, a, (x > 80) & (x < 120) & (np.abs(y - 100 - (x - 80) * 0.5) < 14), green)            # neck
    paint(rgb, a, (x > 236) & (x < 296) & (np.abs(y - 140 + (x - 236) * 0.8) < 7), green)            # tail
    wing = (x > 120) & (x < 230) & (y > 20) & (y < 120) & (y > 120 - (x - 120) * 0.9) & (y > 20 + np.abs(x - 175) * 0.4)
    paint(rgb, a, wing, dark)
    paint(rgb, a, (np.abs(x - 140) < 7) & (y > 160) & (y < 196), dark)                               # legs
    paint(rgb, a, (np.abs(x - 190) < 7) & (y > 160) & (y < 196), dark)
    paint(rgb, a, (x - 60) ** 2 + (y - 72) ** 2 < 5 ** 2, (0.95, 0.85, 0.2))                         # eye
    paint(rgb, a, (x > 36) & (x < 56) & (np.abs(y - 92) < 2), (0.1, 0.1, 0.1))                       # mouth
    for k in range(5):
        sx = 140 + k * 22
        paint(rgb, a, (np.abs(x - sx) < 7 - np.abs(y - 82) * 0.6) & (y > 76) & (y < 90), (0.85, 0.3, 0.15))  # spines
    pt66_cutout(out / "dragon.png", rgb, a)
    # Sun and moon.
    x, y, rgb, a = canvas(130, 130)
    d = np.sqrt((x - 65) ** 2 + (y - 65) ** 2)
    ang = np.arctan2(y - 65, x - 65)
    paint(rgb, a, d < 34 + 14 * (np.cos(ang * 12) > 0.3), (0.98, 0.72, 0.2))
    paint(rgb, a, d < 34, (1.0, 0.86, 0.35))
    pt66_cutout(out / "sun.png", rgb, a)
    x, y, rgb, a = canvas(110, 110)
    moon = ((x - 55) ** 2 + (y - 55) ** 2 < 40 ** 2) & ((x - 75) ** 2 + (y - 45) ** 2 > 34 ** 2)
    paint(rgb, a, moon, (0.95, 0.93, 0.8))
    pt66_cutout(out / "moon.png", rgb, a)


# ── 67 Signs of the Silk Road ──────────────────────────────────────────
# A caravanserai at dusk: the sky, domes and minarets against it, a sandstone
# wall with arched doorways, wooden shop signs and paper lanterns.

def sr67_silk():
    out = OUT / "silk"
    rng = np.random.default_rng(67)
    # The sky: deep blue overhead to a warm band at the horizon.
    h = 512
    t = np.linspace(0, 1, h)[:, None]
    top, mid, low = np.array([0.10, 0.12, 0.30]), np.array([0.42, 0.30, 0.48]), np.array([0.98, 0.62, 0.36])
    sky = np.where(t < 0.6, top + (mid - top) * (t / 0.6), mid + (low - mid) * ((t - 0.6) / 0.4))
    save_rgba(out / "sky.png", np.repeat(sky[:, None, :], 64, 1).reshape(h, 64, 3), np.ones((h, 64)))
    # Domes and minarets, one dark silhouette.
    w, h = 1400, 260
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    sil = y > 200
    for cx, r, base in ((180, 90, 200), (520, 120, 200), (900, 80, 200), (1220, 105, 200)):
        sil |= ((x - cx) ** 2 + (y - base) ** 2 < r * r) & (y < base)
        sil |= (np.abs(x - cx) < 4) & (y > base - r - 30) & (y < base - r + 4)          # finial
        sil |= (np.abs(x - cx) < r) & (y >= base)
    for mx in (360, 720, 1060):
        sil |= (np.abs(x - mx) < 14) & (y > 40)                                            # minaret
        sil |= ((x - mx) ** 2 + (y - 40) ** 2 < 18 ** 2)
        sil |= (np.abs(x - mx) < 20) & (np.abs(y - 90) < 5)                                # balcony
    rgb = np.zeros((h, w, 3)) + np.array([0.16, 0.10, 0.22])
    save_rgba(out / "domes.png", rgb, sil.astype(np.float64))
    # A tileable sandstone wall.
    s = 256
    y, x = np.mgrid[0:s, 0:s].astype(np.float64)
    row = (y // 32).astype(int)
    xs = (x + (row % 2) * 32) % 64
    mortar = ((y % 32) < 3) | (xs < 3)
    stone = np.array([0.72, 0.56, 0.38]) * (0.85 + 0.15 * rng.random((s, s)))[..., None]
    stone *= (0.9 + 0.1 * np.sin(row[..., None] * 1.7 + (x[..., None] // 64) * 2.3))
    stone[mortar] = np.array([0.45, 0.34, 0.24])
    save_rgba(out / "wall.png", np.clip(stone, 0, 1), np.ones((s, s)))
    # An arched doorway, warm inside.
    w, h = 150, 210
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    cx, r = w / 2, w / 2 - 8
    hole = ((x - cx) ** 2 + (y - (r + 8)) ** 2 < r * r) | ((np.abs(x - cx) < r) & (y > r + 8))
    frame = (((x - cx) ** 2 + (y - (r + 8)) ** 2 < (r + 8) ** 2) | ((np.abs(x - cx) < r + 8) & (y > r + 8))) & ~hole
    rgb = np.zeros((h, w, 3))
    rgb[hole] = (np.array([0.55, 0.28, 0.10]) * (0.5 + 0.5 * (y / h))[..., None])[hole]
    rgb[frame] = np.array([0.52, 0.38, 0.24])
    save_rgba(out / "door.png", rgb, (hole | frame).astype(np.float64))
    # A wooden sign board with a darker frame and two rings.
    w, h = 200, 100
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    grain = 0.84 + 0.08 * np.sin(y * 0.55 + 2.0 * np.sin(x * 0.03)) + 0.05 * rng.random((h, w))
    rgb = np.array([0.42, 0.26, 0.13]) * grain[..., None]
    edge = (x < 7) | (x > w - 8) | (y < 7 + 6) | (y > h - 8)
    rgb[edge] *= 0.62
    a = (y > 6).astype(np.float64)
    for rx in (24, w - 24):
        ring = np.abs(np.sqrt((x - rx) ** 2 + (y - 6) ** 2) - 5) < 1.6
        rgb[ring] = (0.3, 0.3, 0.32)
        a[ring] = 1.0
    save_rgba(out / "sign.png", np.clip(rgb, 0, 1), a)
    # A paper lantern.
    w, h = 44, 64
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    body = ((x - w / 2) / 20) ** 2 + ((y - 34) / 24) ** 2 < 1
    caps = (np.abs(x - w / 2) < 10) & ((np.abs(y - 9) < 3) | (np.abs(y - 59) < 3))
    rgb = np.zeros((h, w, 3))
    ribs = 0.75 + 0.25 * np.cos((y - 34) * 0.9) ** 2
    rgb[body] = (np.array([1.0, 0.45, 0.18]) * ribs[..., None])[body]
    rgb[caps] = (0.15, 0.1, 0.08)
    save_rgba(out / "lantern.png", rgb, (body | caps).astype(np.float64))


# ── 68 Moon Lander ─────────────────────────────────────────────────────
# A lunar module, the grey ground with its craters, the Earth in the sky.

def ml68_lander():
    out = OUT / "lander"
    rng = np.random.default_rng(68)
    # The module, 120 x 110: a gold-foil descent stage, a grey cabin, legs.
    w, h = 120, 110
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3))
    a = np.zeros((h, w))

    def paint(mask, col):
        rgb[mask] = col
        a[mask] = 1.0

    foil = (np.abs(x - 60) < 34) & (y > 50) & (y < 80)
    crinkle = 0.78 + 0.22 * np.sin(x * 0.9 + 3 * np.sin(y * 0.7)) * np.cos(y * 1.1)
    paint(foil, np.array([0.95, 0.72, 0.25]) * crinkle[foil][:, None])
    cabin = ((x - 60) ** 2 / 30 ** 2 + (y - 36) ** 2 / 24 ** 2 < 1) & (y < 52)
    paint(cabin, np.array([0.72, 0.73, 0.76]) * (0.85 + 0.15 * (x[cabin] < 60))[:, None])
    window = (x - 50) ** 2 + (y - 32) ** 2 < 6 ** 2
    paint(window, (0.12, 0.14, 0.2))
    paint((np.abs(x - 60) < 2) & (y > 4) & (y < 14), (0.6, 0.6, 0.62))              # antenna
    paint((x - 60) ** 2 + (y - 4) ** 2 < 4 ** 2, (0.85, 0.85, 0.88))
    for sx in (-1, 1):
        # A leg out and down from the stage, and its pad.
        t = (x - 60 - sx * 30) * sx
        leg = (t > 0) & (t < 24) & (np.abs(y - (78 + t * 1.15)) < 2.5)
        paint(leg, (0.55, 0.55, 0.58))
        foot = (np.abs(x - 60 - sx * 54) < 9) & (np.abs(y - 106) < 2.5)
        paint(foot, (0.6, 0.6, 0.62))
    nozzle = (np.abs(x - 60) < 10 - (y - 80) * 0.3) & (y >= 80) & (y < 92)
    paint(nozzle, (0.35, 0.35, 0.38))
    save_rgba(out / "module.png", np.clip(rgb, 0, 1), a)
    # The ground: a grey crust with craters, lit from the left.
    w, h = 1400, 240
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    top = 26 + 6 * np.sin(x / w * np.pi * 7) + 3 * np.sin(x / w * np.pi * 23)
    ground = y > top
    shade = 0.62 + 0.08 * rng.random((h, w)) - 0.18 * np.clip((y - top) / 200, 0, 1)
    rgb = np.repeat(shade[..., None], 3, -1) * np.array([0.92, 0.92, 0.95])
    for cx, cy, r in ((160, 90, 46), (420, 150, 30), (700, 80, 22), (980, 140, 52), (1250, 100, 34), (560, 200, 26)):
        d = np.sqrt((x - cx) ** 2 + ((y - cy) * 2.4) ** 2)
        bowl = d < r
        rgb[bowl] *= (0.72 + 0.28 * np.clip((x[bowl] - cx) / r + 0.5, 0, 1))[:, None]
        rim = (d >= r) & (d < r + 4)
        rgb[rim] *= 1.18
    save_rgba(out / "ground.png", np.clip(rgb, 0, 1), ground.astype(np.float64))
    # The Earth.
    s = 170
    nx, ny, d = radius(s)
    disc = d < 0.96
    land = (np.sin(nx * 7 + 1) + np.cos(ny * 6 - nx * 3) + 0.6 * np.sin(nx * 15 + ny * 9)) > 0.9
    rgb = np.zeros((s, s, 3)) + np.array([0.15, 0.35, 0.75])
    rgb[land] = (0.25, 0.55, 0.3)
    cloud = (np.sin(nx * 11 + ny * 4) + np.sin(ny * 13 - nx * 2)) > 1.3
    rgb[cloud] = (0.95, 0.95, 0.97)
    light = np.clip(0.25 + 0.9 * (-nx * 0.6 - ny * 0.3 + np.sqrt(np.clip(1 - d * d, 0, 1)) * 0.6), 0.08, 1.2)
    rgb *= light[..., None]
    save_rgba(out / "earth.png", np.clip(rgb, 0, 1), smoothstep(0.98, 0.94, d))


# ── 69 The Kraken ──────────────────────────────────────────────────────
# A storm at sea: the sky, a ship with its sails, two layers of waves, and
# the tentacle's segments - a tapering limb with suckers along one side,
# drawn along +x as its bones lie.

def kr69_kraken():
    out = OUT / "kraken"
    rng = np.random.default_rng(69)
    # The storm sky, dark with a lighter band low down.
    h = 512
    t = np.linspace(0, 1, h)[:, None]
    sky = np.array([0.05, 0.07, 0.10]) + np.array([0.12, 0.15, 0.18]) * np.clip((t - 0.3) / 0.7, 0, 1) ** 1.5
    save_rgba(out / "sky.png", np.repeat(sky[:, None, :], 64, 1).reshape(h, 64, 3), np.ones((h, 64)))
    # Clouds: soft dark masses.
    w, h = 1024, 256
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    c = np.zeros((h, w))
    for _ in range(40):
        cx, cy, r = rng.random() * w, 60 + rng.random() * 120, 40 + rng.random() * 90
        c = np.maximum(c, np.clip(1 - np.sqrt((x - cx) ** 2 + ((y - cy) * 1.8) ** 2) / r, 0, 1))
    rgb = np.zeros((h, w, 3)) + np.array([0.10, 0.12, 0.16]) + 0.06 * (1 - y / h)[..., None]
    save_rgba(out / "clouds.png", rgb, np.clip(c * 1.4, 0, 0.95))
    # The ship, side on, bow to the right: hull, two masts, sails, a flag.
    w, h = 360, 330
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3))
    a = np.zeros((h, w))

    def paint(mask, col):
        rgb[mask] = col
        a[mask] = 1.0

    hull = (y > 250) & (y < 300) & (x > 20 + (y - 250) * 0.6) & (x < 340 - (y - 250) * 1.2)
    paint(hull, np.array([0.35, 0.22, 0.12]) * (0.8 + 0.2 * np.sin(y * 0.6))[hull][:, None])
    paint((y > 244) & (y < 252) & (x > 18) & (x < 344), (0.55, 0.38, 0.2))      # rail
    for i in range(6):
        paint(((x - (80 + i * 40)) ** 2 + (y - 270) ** 2) < 16, (0.08, 0.06, 0.05))  # ports
    for mx, top in ((130, 40), (240, 70)):
        paint((np.abs(x - mx) < 4) & (y > top) & (y < 250), (0.3, 0.2, 0.12))    # mast
        sail = (y > top + 20) & (y < 228) & (np.abs(x - mx - 6 - (y - top) * 0.05) < 48 - np.abs(y - (top + 130)) * 0.08)
        shade = 0.82 + 0.18 * np.cos((x - mx) * 0.06)
        paint(sail & ~((np.abs(x - mx) < 4)), np.array([0.86, 0.83, 0.74]) * shade[sail & ~((np.abs(x - mx) < 4))][:, None])
    paint((x > 130) & (x < 165) & (y > 40) & (y < 58) & (y < 40 + (165 - x) * 0.6 + 10), (0.7, 0.12, 0.1))  # flag
    save_rgba(out / "ship.png", np.clip(rgb, 0, 1), a)
    # A wave band, tileable across: a dark sea with a lighter crest line.
    w, h = 512, 160
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    crest = 30 + 10 * np.sin(x / w * np.pi * 4) + 5 * np.sin(x / w * np.pi * 10 + 1)
    sea = y > crest
    rgb = np.array([0.06, 0.16, 0.20]) * (1.1 - 0.5 * np.clip((y - crest) / 120, 0, 1))[..., None]
    foam = np.abs(y - crest) < 4
    rgb[foam] = (0.55, 0.65, 0.68)
    save_rgba(out / "waves.png", np.clip(rgb, 0, 1), (sea | foam).astype(np.float64))
    # A tentacle segment, 64 x 24: round, purple-red, suckers below.
    w, h = 64, 24
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    r = h / 2
    cx = np.clip(x + 0.5, r, w - r)
    body = (x + 0.5 - cx) ** 2 + (y + 0.5 - r) ** 2 <= r * r
    v = (y + 0.5) / h
    rgb = np.array([0.55, 0.15, 0.22]) * (0.6 + 0.6 * np.cos((v - 0.3) * np.pi) ** 2)[..., None]
    belly = v > 0.62
    rgb[belly & body] = (np.array([0.85, 0.55, 0.5]) * (0.9 + 0.1 * np.cos(x * 0.5))[..., None])[belly & body]
    for sx in range(8, w, 14):
        sucker = (x - sx) ** 2 + (y - 19) ** 2 < 9
        rgb[sucker & body] = (0.95, 0.75, 0.7)
    save_rgba(out / "segment.png", np.clip(rgb, 0, 1), body.astype(np.float64))


# ── 70 Eight Ways ──────────────────────────────────────────────────────
# A forest clearing seen from above - grass, a trodden patch, tree canopies
# and their shadows, rocks, a stump, hay-bale targets - and the ranger's
# parts, drawn facing +x (the way its bones point): boots, a cloak over the
# shoulders, a hood, a bow held across, a quiver and an arrow.

def rg70_ranger():
    out = OUT / "ranger"
    rng = np.random.default_rng(70)

    def noise(size, cells):
        grid = rng.random((cells, cells))
        y, x = np.mgrid[0:size, 0:size].astype(np.float64)
        fx, fy = x / size * cells, y / size * cells
        x0, y0 = np.floor(fx).astype(int) % cells, np.floor(fy).astype(int) % cells
        x1, y1 = (x0 + 1) % cells, (y0 + 1) % cells
        tx, ty = smoothstep(0.0, 1.0, fx - np.floor(fx)), smoothstep(0.0, 1.0, fy - np.floor(fy))
        a = grid[y0, x0] * (1 - tx) + grid[y0, x1] * tx
        b = grid[y1, x0] * (1 - tx) + grid[y1, x1] * tx
        return a * (1 - ty) + b * ty

    def fbm2(size, cells, octaves):
        total, amp, norm = np.zeros((size, size)), 1.0, 0.0
        for k in range(octaves):
            total += noise(size, cells * 2 ** k) * amp
            norm += amp
            amp *= 0.5
        return total / norm

    # Grass, tileable: two greens by noise, flecked with blades.
    s = 256
    n = fbm2(s, 4, 4)
    fleck = noise(s, 64)
    rgb = np.array([0.20, 0.36, 0.12]) + (n - 0.5)[..., None] * np.array([0.16, 0.2, 0.08])
    rgb = rgb + (fleck > 0.78)[..., None] * np.array([0.08, 0.1, 0.02]) - (fleck < 0.18)[..., None] * 0.04
    save_rgba(out / "grass.png", np.clip(rgb, 0, 1), np.ones((s, s)))
    # The trodden clearing: soft-edged earth showing through thin grass.
    w, h = 500, 300
    nx, ny, d = radius(w, h)
    big = np.array(Image.fromarray((fbm2(512, 6, 4) * 255).astype(np.uint8)).resize((w, h))) / 255.0
    edge = smoothstep(0.95, 0.55, d + (big - 0.5) * 0.5)
    rgb = np.array([0.42, 0.33, 0.2]) * (0.8 + 0.4 * big)[..., None]
    save_rgba(out / "clearing.png", np.clip(rgb, 0, 1), edge * 0.85)
    # A soft shadow.
    w = 128
    _, _, d = radius(w)
    save_rgba(out / "shadow.png", np.zeros((w, w, 3)) + 0.02, smoothstep(1.0, 0.45, d) * 0.9)
    # Canopies: lumpy discs of leaf clusters, lit from the top left.
    w = 256
    nx, ny, d = radius(w)
    for k in range(3):
        canopy = np.zeros((w, w))
        light = np.zeros((w, w))
        base = [np.array([0.10, 0.26, 0.09]), np.array([0.14, 0.30, 0.08]), np.array([0.09, 0.22, 0.12])][k]
        for _ in range(26):
            a, r = rng.random() * math.tau, rng.random() ** 0.6 * 0.62
            cx, cy, cr = math.cos(a) * r, math.sin(a) * r, 0.22 + rng.random() * 0.16
            dd = np.sqrt((nx - cx) ** 2 + (ny - cy) ** 2) / cr
            blob = np.clip(1 - dd, 0, 1)
            # Each cluster is a small dome: lit on its top-left side.
            shade = np.clip(1 - np.sqrt((nx - cx + cr * 0.35) ** 2 + (ny - cy + cr * 0.35) ** 2) / cr, 0, 1)
            canopy = np.maximum(canopy, blob)
            light = np.where(blob > 0.02, np.maximum(light, shade), light)
        leaf = noise(w, 48)
        rgb = base * (0.55 + 0.75 * light + 0.25 * (leaf - 0.5))[..., None]
        save_rgba(out / f"canopy{k}.png", np.clip(rgb, 0, 1), smoothstep(0.0, 0.08, canopy))
    # A rock: grey, lit from the top left.
    w = 96
    nx, ny, d = radius(w)
    lump = d + (noise(w, 6) - 0.5) * 0.3
    body = lump < 0.85
    lit = np.clip(0.55 - nx * 0.3 - ny * 0.35 + (1 - lump) * 0.4, 0.2, 1.1)
    rgb = np.array([0.45, 0.45, 0.42]) * lit[..., None] * (0.9 + 0.2 * noise(w, 24))[..., None]
    save_rgba(out / "rock.png", np.clip(rgb, 0, 1), smoothstep(0.9, 0.82, lump))
    # A stump: bark ring, pale wood, growth rings.
    w = 96
    nx, ny, d = radius(w)
    rings = 0.5 + 0.5 * np.sin(d * 34)
    rgb = np.where((d > 0.8)[..., None], np.array([0.25, 0.16, 0.09]),
                   np.array([0.66, 0.5, 0.3]) * (0.85 + 0.15 * rings)[..., None])
    save_rgba(out / "stump.png", np.clip(rgb, 0, 1), smoothstep(0.98, 0.92, d))
    # A round hay bale with a painted target on top.
    w = 128
    nx, ny, d = radius(w)
    straw = 0.75 + 0.25 * np.sin(np.arctan2(ny, nx) * 40 + d * 30) * noise(w, 32)
    rgb = np.array([0.78, 0.64, 0.3]) * straw[..., None] * (1.05 - 0.25 * d)[..., None]
    for r0, r1, col in ((0.0, 0.16, (0.8, 0.12, 0.1)), (0.3, 0.42, (0.85, 0.82, 0.75)), (0.55, 0.64, (0.8, 0.12, 0.1))):
        m = (d >= r0) & (d < r1)
        rgb[m] = np.array(col) * (0.9 + 0.1 * straw[m])[..., None]
    save_rgba(out / "bale.png", np.clip(rgb, 0, 1), smoothstep(0.98, 0.9, d))
    # Boots: a dark leather oval, toe to +x.
    w, h = 36, 22
    nx, ny, d = radius(w, h)
    lit = np.clip(0.8 - ny * 0.3 + nx * 0.1, 0.4, 1.0)
    save_rgba(out / "boot.png", np.array([0.28, 0.18, 0.1]) * lit[..., None], smoothstep(1.0, 0.85, d))
    # The cloak over the shoulders: a green oval, wider across than deep, its
    # hem darker; the shoulders lit.
    w, h = 64, 96
    nx, ny, d = radius(w, h)
    lit = np.clip(1.0 - d * 0.5 - ny * 0.15, 0.3, 1.0)
    fold = 0.9 + 0.1 * np.sin(ny * 18)
    rgb = np.array([0.18, 0.34, 0.16]) * (lit * fold)[..., None]
    save_rgba(out / "cloak.png", np.clip(rgb, 0, 1), smoothstep(1.0, 0.9, d))
    # The hood: a darker green disc with a rim and the face's shadow ahead.
    w = 48
    nx, ny, d = radius(w)
    rgb = np.array([0.13, 0.25, 0.12]) * np.clip(1.15 - d * 0.6 - ny * 0.2, 0.3, 1.0)[..., None]
    face = smoothstep(0.35, 0.75, nx) * smoothstep(0.55, 0.2, np.abs(ny))
    rgb = rgb * (1 - 0.6 * face)[..., None]
    save_rgba(out / "hood.png", np.clip(rgb, 0, 1), smoothstep(1.0, 0.88, d))
    # The bow, held across: a bent limb and its string.
    w, h = 24, 100
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    t = (y - h / 2) / (h / 2)
    limb_x = 14 - 10 * t * t
    limb = np.abs(x - limb_x) < 2.6 * (1.2 - 0.5 * np.abs(t))
    string = (np.abs(x - 4.5) < 0.8) & (np.abs(t) < 0.97)
    rgb = np.zeros((h, w, 3))
    rgb[limb] = (0.45, 0.28, 0.12)
    rgb[string] = (0.85, 0.82, 0.7)
    save_rgba(out / "bow.png", rgb, (limb | string).astype(np.float64))
    # The quiver: a leather tube with fletchings at its mouth.
    w, h = 48, 18
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    tube = (np.abs(y - h / 2) < 6) & (x > 8)
    flet = (x <= 8) & (np.abs(y - h / 2) < 5) & ((y.astype(int) % 3) != 0)
    rgb = np.zeros((h, w, 3))
    rgb[tube] = (0.32, 0.19, 0.1)
    rgb[flet] = (0.7, 0.68, 0.6)
    save_rgba(out / "quiver.png", rgb, (tube | flet).astype(np.float64))
    # An arrow: shaft, steel head to +x, white fletching.
    w, h = 80, 12
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    shaft = (np.abs(y - h / 2) < 1.2) & (x > 8) & (x < 68)
    head = (x >= 66) & (np.abs(y - h / 2) < (80 - x) * 0.4)
    flet = (x < 18) & (np.abs(y - h / 2) < 2 + (18 - x) * 0.25) & (np.abs(y - h / 2) > 1.2)
    rgb = np.zeros((h, w, 3))
    rgb[shaft] = (0.6, 0.45, 0.25)
    rgb[head] = (0.75, 0.78, 0.82)
    rgb[flet] = (0.92, 0.92, 0.9)
    save_rgba(out / "arrow.png", rgb, (shaft | head | flet).astype(np.float64))


# ── 71 The Quest Journal ───────────────────────────────────────────────
# A desk, an open leather journal, a candle and a quill - and the small
# icons the journal's text carries inline: a sword, a key, a coin, a potion,
# a scroll, a skull and a gem, each 64 px with a dark outline so it reads on
# parchment.

def qj71_journal():
    out = OUT / "journal"
    rng = np.random.default_rng(71)

    def noise(size_w, size_h, cells_x, cells_y):
        grid = rng.random((cells_y + 1, cells_x + 1))
        y, x = np.mgrid[0:size_h, 0:size_w].astype(np.float64)
        fx, fy = x / size_w * cells_x, y / size_h * cells_y
        x0, y0 = np.floor(fx).astype(int), np.floor(fy).astype(int)
        tx, ty = smoothstep(0.0, 1.0, fx - x0), smoothstep(0.0, 1.0, fy - y0)
        x1, y1 = np.minimum(x0 + 1, cells_x), np.minimum(y0 + 1, cells_y)
        a = grid[y0, x0] * (1 - tx) + grid[y0, x1] * tx
        b = grid[y1, x0] * (1 - tx) + grid[y1, x1] * tx
        return a * (1 - ty) + b * ty

    # The desk: dark planks with grain.
    w, h = 640, 360
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    plank = (y // 90).astype(int)
    grain = noise(w, h, 4, 40) * 0.6 + noise(w, h, 2, 120) * 0.4
    rgb = np.array([0.26, 0.15, 0.08]) * (0.75 + 0.45 * grain)[..., None]
    rgb = rgb * (0.92 + 0.08 * ((plank % 2) * 2 - 1))[..., None]
    rgb[(y % 90) < 2] *= 0.5
    save_rgba(out / "desk.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # The open journal: a leather cover round two parchment pages that darken
    # into the gutter and at their edges.
    w, h = 1040, 640
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    leather = np.array([0.32, 0.14, 0.08]) * (0.8 + 0.3 * noise(w, h, 30, 18))[..., None]
    rgb = np.zeros((h, w, 3)) + leather
    inner = (x > 26) & (x < w - 26) & (y > 22) & (y < h - 22)
    u = np.abs(x - w / 2) / (w / 2 - 26)
    v = np.abs(y - h / 2) / (h / 2 - 22)
    paper = np.array([0.93, 0.86, 0.68]) * (0.92 + 0.08 * noise(w, h, 60, 36))[..., None]
    shade = 1 - 0.35 * np.clip(1 - u / 0.08, 0, 1) ** 1.5 - 0.18 * np.clip((u - 0.9) / 0.1, 0, 1) - 0.15 * np.clip((v - 0.88) / 0.12, 0, 1)
    rgb[inner] = (paper * shade[..., None])[inner]
    stitch = (np.abs(x - w / 2) < 2) & (y % 24 < 12) & inner
    rgb[stitch] = (0.5, 0.36, 0.22)
    save_rgba(out / "book.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # A candle with its flame, and a soft glow.
    w, h = 64, 160
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    wax = (np.abs(x - w / 2) < 16) & (y > 60)
    rgb = np.zeros((h, w, 3))
    rgb[wax] = (np.array([0.92, 0.88, 0.76]) * (0.8 + 0.2 * np.cos((x - w / 2) / 16 * 1.4))[..., None])[wax]
    flame = ((x - w / 2) / 7) ** 2 + ((y - 40) / 18) ** 2 < 1
    rgb[flame] = (1.0, 0.82, 0.4)
    core = ((x - w / 2) / 3.5) ** 2 + ((y - 46) / 9) ** 2 < 1
    rgb[core] = (1.0, 0.97, 0.85)
    wick = (np.abs(x - w / 2) < 1.2) & (y > 52) & (y <= 60)
    rgb[wick] = (0.1, 0.08, 0.06)
    save_rgba(out / "candle.png", rgb, (wax | flame | wick).astype(np.float64))
    w = 128
    _, _, d = radius(w)
    save_rgba(out / "glow.png", np.zeros((w, w, 3)) + np.array([1.0, 0.75, 0.4]), smoothstep(1.0, 0.0, d) ** 2)
    # A quill: a long feather, its vane tapering to a nib.
    w, h = 300, 60
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    t = x / w
    spine = h / 2 + 8 * np.sin(t * 2.5)
    half = np.where(t > 0.25, 22 * np.sin(np.clip((t - 0.25) / 0.75, 0, 1) * np.pi) ** 0.6, 0)
    vane = np.abs(y - spine) < half
    shaft = (np.abs(y - spine) < 1.6) & (t > 0.03)
    rgb = np.zeros((h, w, 3))
    barbs = 0.85 + 0.15 * np.sin((x + (y - spine) * 1.5) * 0.9)
    rgb[vane] = (np.array([0.88, 0.86, 0.8]) * barbs[..., None])[vane]
    rgb[shaft] = (0.75, 0.7, 0.6)
    nib = (t <= 0.06) & (np.abs(y - spine) < (0.06 - t) * 40)
    rgb[nib] = (0.15, 0.12, 0.1)
    save_rgba(out / "quill.png", rgb, (vane | shaft | nib).astype(np.float64))

    # Icons, 64 px, drawn at 3x and outlined in dark ink.
    def icon(name, draw):
        s = 192
        y, x = np.mgrid[0:s, 0:s].astype(np.float64) / s * 64
        rgb, mask = draw(x, y)
        m = mask.astype(np.float64)
        # A 2 px outline: the mask grown.
        grown = m.copy()
        for dx, dy in ((6, 0), (-6, 0), (0, 6), (0, -6), (4, 4), (-4, 4), (4, -4), (-4, -4)):
            grown = np.maximum(grown, np.roll(np.roll(m, dx, 1), dy, 0))
        full = np.where(m[..., None] > 0, rgb, np.array([0.12, 0.08, 0.05]))
        # Averaged premultiplied, then divided back out, so edges do not darken.
        img = np.concatenate([full * grown[..., None], grown[..., None]], axis=2).reshape(64, 3, 64, 3, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def sword(x, y):
        rgb = np.zeros(x.shape + (3,))
        u, v = (x + y) / np.sqrt(2), (x - y) / np.sqrt(2)  # along / across the diagonal
        cu = 45.25
        blade = (u > cu - 36) & (u < cu + 14) & (np.abs(v) < 4 - np.clip(u - cu - 6, 0, 8) * 0.5)
        guard = (np.abs(u - (cu + 16)) < 3) & (np.abs(v) < 13)
        grip = (u > cu + 18) & (u < cu + 30) & (np.abs(v) < 3)
        pommel = (u - (cu + 33)) ** 2 + v ** 2 < 16
        rgb[blade] = (0.82, 0.85, 0.9)
        rgb[guard | pommel] = (0.85, 0.65, 0.2)
        rgb[grip] = (0.4, 0.22, 0.12)
        return rgb, blade | guard | grip | pommel

    def key(x, y):
        rgb = np.zeros(x.shape + (3,))
        ring = (np.abs(np.sqrt((x - 18) ** 2 + (y - 32) ** 2) - 9) < 4)
        shaft = (x > 26) & (x < 56) & (np.abs(y - 32) < 3.5)
        bits = ((np.abs(x - 50) < 3) | (np.abs(x - 42) < 3)) & (y > 32) & (y < 44)
        m = ring | shaft | bits
        rgb[m] = (0.95, 0.75, 0.25)
        return rgb, m

    def coin(x, y):
        d = np.sqrt((x - 32) ** 2 + (y - 32) ** 2)
        m = d < 24
        rgb = np.zeros(x.shape + (3,)) + np.array([0.95, 0.75, 0.25])
        rgb[(d > 18) & m] = (0.8, 0.58, 0.15)
        rgb[(np.abs(x - 32) < 3) & (np.abs(y - 32) < 11)] = (0.8, 0.58, 0.15)
        return rgb, m

    def potion(x, y):
        body = (x - 32) ** 2 + (y - 40) ** 2 < 19 ** 2
        neck = (np.abs(x - 32) < 6) & (y > 8) & (y < 26)
        cork = (np.abs(x - 32) < 7) & (y > 4) & (y < 12)
        rgb = np.zeros(x.shape + (3,)) + np.array([0.75, 0.85, 0.9])
        rgb[body & (y > 34)] = (0.85, 0.15, 0.2)
        rgb[cork] = (0.6, 0.4, 0.2)
        return rgb, body | neck | cork

    def scroll(x, y):
        sheet = (x > 14) & (x < 50) & (y > 12) & (y < 52)
        rolls = ((np.abs(y - 12) < 5) | (np.abs(y - 52) < 5)) & (x > 10) & (x < 54)
        rgb = np.zeros(x.shape + (3,)) + np.array([0.93, 0.86, 0.66])
        rgb[rolls] = (0.8, 0.7, 0.5)
        lines = sheet & (np.abs(((y - 20) % 8) - 4) < 1) & (x > 20) & (x < 44) & (y > 18) & (y < 46)
        rgb[lines] = (0.35, 0.25, 0.15)
        return rgb, sheet | rolls

    def skull(x, y):
        head = (x - 32) ** 2 / 22 ** 2 + (y - 28) ** 2 / 20 ** 2 < 1
        jaw = (np.abs(x - 32) < 12) & (y > 40) & (y < 54)
        eyes = ((x - 24) ** 2 + (y - 30) ** 2 < 30) | ((x - 40) ** 2 + (y - 30) ** 2 < 30)
        rgb = np.zeros(x.shape + (3,)) + np.array([0.92, 0.9, 0.82])
        rgb[eyes] = (0.15, 0.1, 0.08)
        rgb[jaw & (np.abs(((x - 32) % 6) - 3) < 0.8)] = (0.3, 0.25, 0.2)
        return rgb, head | jaw

    def gem(x, y):
        m = (np.abs(x - 32) * 1.1 + np.abs(y - 34) < 26) & (y > 14)
        rgb = np.zeros(x.shape + (3,)) + np.array([0.2, 0.75, 0.55])
        rgb[(y < 26) & m] = (0.55, 0.95, 0.8)
        rgb[(np.abs(x - 32) < 2) & m] = (0.4, 0.9, 0.7)
        return rgb, m

    for name, draw in (("sword", sword), ("key", key), ("coin", coin), ("potion", potion), ("scroll", scroll),
                       ("skull", skull), ("gem", gem)):
        icon(name, draw)


# ── 72 Scrap Yard ──────────────────────────────────────────────────────
# A scrap yard at dusk: the sky, a fence, distant heaps, the ground; a
# gantry crane's beam, trolley and electromagnet; and the junk it drops -
# a bicycle, a kettle, a gear, a tyre, a chair, an anvil, a desk lamp and a
# bent pipe - each drawn with a clear alpha so its outline is its collider.

def sy72_scrapyard():
    out = OUT / "scrap"
    rng = np.random.default_rng(72)
    SS = 3  # supersampling

    def canvas(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y, np.zeros((h * SS, w * SS, 3)), np.zeros((h * SS, w * SS))

    def put(rgb, a, mask, col):
        rgb[mask] = col
        a[mask] = 1.0

    def seg(x, y, p, q, r):
        """Pixels within r of the segment p-q."""
        px, py = x - p[0], y - p[1]
        dx, dy = q[0] - p[0], q[1] - p[1]
        t = np.clip((px * dx + py * dy) / max(dx * dx + dy * dy, 1e-9), 0, 1)
        return (px - t * dx) ** 2 + (py - t * dy) ** 2 <= r * r

    def poly(x, y, pts):
        inside = np.zeros(x.shape, bool)
        n = len(pts)
        for i in range(n):
            (x1, y1), (x2, y2) = pts[i], pts[(i + 1) % n]
            crosses = ((y1 > y) != (y2 > y)) & (x < (x2 - x1) * (y - y1) / ((y2 - y1) + 1e-12) + x1)
            inside ^= crosses
        return inside

    def rust(rgb, a, base, amount=0.35, cells=10):
        h, w = a.shape
        g = rng.random((cells + 1, cells + 1))
        yy, xx = np.mgrid[0:h, 0:w] / np.array([h, w])[:, None, None] * cells
        n = g[yy.astype(int), xx.astype(int)]
        speck = rng.random((h, w))
        tint = np.array(base) * (0.8 + 0.4 * n)[..., None]
        tint = np.where((speck < amount * n)[..., None], tint * np.array([1.1, 0.7, 0.45]), tint)
        m = a > 0
        rgb[m] = rgb[m] * tint[m] / np.array(base)

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    # The yard: a dusk sky, a corrugated fence, distant heaps, dirt.
    w, h = 64, 512
    t = np.linspace(0, 1, h)[:, None]
    sky = np.array([0.18, 0.2, 0.32]) * (1 - t) + np.array([0.85, 0.5, 0.3]) * t ** 1.6
    save_rgba(out / "sky.png", np.repeat(sky[:, None, :], w, 1).reshape(h, w, 3), np.ones((h, w)))
    w, h = 256, 160
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    ridges = 0.75 + 0.25 * np.cos(x / w * np.pi * 16)
    rgb = np.array([0.24, 0.24, 0.26]) * ridges[..., None] * (0.85 + 0.15 * (y / h))[..., None]
    rgb[(y % 80) < 3] *= 0.6
    save_rgba(out / "fence.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    w, h = 1024, 220
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    top = 110 + 50 * np.sin(x / w * 7.1) + 25 * np.sin(x / w * 23 + 2) + 12 * np.sin(x / w * 61)
    heap = y > top
    speck = rng.random((h, w))
    rgb = np.array([0.16, 0.13, 0.13]) * (0.8 + 0.4 * speck)[..., None]
    save_rgba(out / "heaps.png", rgb, heap.astype(np.float64))
    w, h = 256, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    speck = rng.random((h, w))
    rgb = np.array([0.30, 0.24, 0.18]) * (0.75 + 0.35 * speck)[..., None]
    rgb[y < 6] = (0.38, 0.31, 0.22)
    save_rgba(out / "ground.png", rgb, np.ones((h, w)))
    # The crane: a girder beam, the trolley, the magnet.
    w, h = 512, 48
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    beam = (y < 8) | (y > 40) | (np.abs(((x + y) % 48) - 24) < 3) | (np.abs(((x - y) % 48) - 24) < 3)
    rgb = np.zeros((h, w, 3)) + np.array([0.85, 0.62, 0.12]) * (0.8 + 0.2 * (y < 24))[..., None]
    save_rgba(out / "beam.png", rgb, beam.astype(np.float64))
    x, y, rgb, a = canvas(96, 48)
    put(rgb, a, (x > 6) & (x < 90) & (y > 10) & (y < 40), (0.7, 0.5, 0.1))
    for cx in (22, 74):
        put(rgb, a, (x - cx) ** 2 + (y - 10) ** 2 < 64, (0.2, 0.2, 0.22))
    save("trolley", rgb, a)
    x, y, rgb, a = canvas(96, 56)
    put(rgb, a, (np.abs(x - 48) < 44) & (y > 14) & (y < 50) & ((x - 48) ** 2 / 44 ** 2 + (y - 14) ** 2 / 36 ** 2 < 1.3), (0.32, 0.33, 0.36))
    put(rgb, a, (np.abs(x - 48) < 40) & (y > 44) & (y < 52), (0.55, 0.56, 0.6))
    put(rgb, a, (np.abs(x - 48) < 8) & (y < 16), (0.25, 0.25, 0.27))
    save("magnet", rgb, a)

    # The junk, each lit from above a little.
    def bicycle():
        x, y, rgb, a = canvas(160, 100)
        for cx in (34, 126):
            r = np.sqrt((x - cx) ** 2 + (y - 66) ** 2)
            put(rgb, a, (r > 24) & (r < 31), (0.1, 0.1, 0.1))
            for k in range(8):
                ang = k * np.pi / 8
                put(rgb, a, seg(x, y, (cx - 24 * np.cos(ang), 66 - 24 * np.sin(ang)), (cx + 24 * np.cos(ang), 66 + 24 * np.sin(ang)), 0.8), (0.6, 0.6, 0.62))
            put(rgb, a, r < 4, (0.5, 0.5, 0.52))
        red = (0.72, 0.16, 0.12)
        for p, q in (((34, 66), (72, 66)), ((72, 66), (60, 30)), ((60, 30), (34, 66)), ((60, 30), (112, 30)), ((112, 30), (72, 66)),
                     ((112, 30), (126, 66)), ((112, 30), (108, 16))):
            put(rgb, a, seg(x, y, p, q, 3), red)
        put(rgb, a, seg(x, y, (52, 22), (70, 22), 3.5), (0.12, 0.1, 0.1))
        put(rgb, a, seg(x, y, (60, 30), (60, 22), 2), (0.5, 0.5, 0.5))
        put(rgb, a, seg(x, y, (100, 14), (116, 14), 2.5), (0.15, 0.15, 0.15))
        return rgb, a

    def kettle():
        x, y, rgb, a = canvas(100, 84)
        body = ((x - 50) / 32) ** 2 + ((y - 52) / 28) ** 2 < 1
        body &= y < 78
        put(rgb, a, body, (0.6, 0.62, 0.66))
        put(rgb, a, (np.abs(x - 50) < 30) & (y >= 74) & (y < 80), (0.4, 0.4, 0.44))
        put(rgb, a, poly(x, y, [(78, 50), (98, 26), (92, 24), (74, 40)]), (0.55, 0.57, 0.6))
        r = np.sqrt((x - 50) ** 2 + ((y - 30) * 1.2) ** 2)
        put(rgb, a, (r > 22) & (r < 28) & (y < 30), (0.18, 0.15, 0.13))
        put(rgb, a, ((x - 50) ** 2 + (y - 25) ** 2 < 25), (0.18, 0.15, 0.13))
        rust(rgb, a, (0.6, 0.62, 0.66), 0.2)
        return rgb, a

    def gear():
        x, y, rgb, a = canvas(96, 96)
        r = np.sqrt((x - 48) ** 2 + (y - 48) ** 2)
        ang = np.arctan2(y - 48, x - 48)
        teeth = (np.cos(ang * 10) > 0.2) & (r < 44)
        put(rgb, a, ((r < 36) | teeth) & (r > 12), (0.55, 0.5, 0.42))
        rust(rgb, a, (0.55, 0.5, 0.42), 0.5)
        return rgb, a

    def tyre():
        x, y, rgb, a = canvas(84, 84)
        r = np.sqrt((x - 42) ** 2 + (y - 42) ** 2)
        put(rgb, a, (r < 40) & (r > 20), (0.12, 0.12, 0.13))
        tread = (r > 34) & (np.cos(np.arctan2(y - 42, x - 42) * 24) > 0.3) & (r < 40)
        rgb[tread] = (0.2, 0.2, 0.21)
        return rgb, a

    def chair():
        x, y, rgb, a = canvas(72, 110)
        wood = (0.52, 0.33, 0.18)
        put(rgb, a, (x > 8) & (x < 64) & (y > 58) & (y < 66), wood)
        put(rgb, a, (x > 8) & (x < 15) & (y > 4) & (y < 106), wood)
        put(rgb, a, (x > 57) & (x < 64) & (y > 58) & (y < 106), wood)
        for yy in (14, 30, 46):
            put(rgb, a, (x > 8) & (x < 15 + 0) & (np.abs(y - yy) < 3), wood)
        put(rgb, a, (x > 14) & (x < 18) & (y > 6) & (y < 58), (0.42, 0.26, 0.14))
        return rgb, a

    def anvil():
        x, y, rgb, a = canvas(120, 64)
        top = poly(x, y, [(4, 6), (116, 6), (116, 20), (86, 22), (70, 36), (50, 36), (34, 22), (20, 20), (4, 12)])
        foot = poly(x, y, [(38, 34), (82, 34), (96, 58), (24, 58)])
        put(rgb, a, top | foot, (0.25, 0.25, 0.27))
        rgb[(y < 9) & top] = (0.45, 0.45, 0.48)
        return rgb, a

    def lamp():
        x, y, rgb, a = canvas(80, 110)
        green = (0.2, 0.42, 0.3)
        put(rgb, a, (np.abs(x - 24) < 20) & (y > 98) & (y < 106), green)
        put(rgb, a, seg(x, y, (24, 100), (40, 52), 3), (0.6, 0.6, 0.62))
        put(rgb, a, seg(x, y, (40, 52), (24, 20), 3), (0.6, 0.6, 0.62))
        put(rgb, a, poly(x, y, [(20, 12), (40, 4), (74, 28), (54, 44)]), green)
        put(rgb, a, (x - 64) ** 2 + (y - 40) ** 2 < 30, (1.0, 0.9, 0.6))
        return rgb, a

    def pipe():
        x, y, rgb, a = canvas(110, 90)
        p = seg(x, y, (14, 18), (60, 18), 11) | seg(x, y, (78, 36), (78, 78), 11)
        r = np.sqrt((x - 60) ** 2 + (y - 36) ** 2)
        p |= (r < 29) & (r > 7) & (x > 60) & (y < 36)
        put(rgb, a, p, (0.48, 0.38, 0.3))
        put(rgb, a, (np.abs(x - 6) < 4) & (np.abs(y - 18) < 15), (0.4, 0.32, 0.25))
        rust(rgb, a, (0.48, 0.38, 0.3), 0.6)
        return rgb, a

    for name, draw in (("bicycle", bicycle), ("kettle", kettle), ("gear", gear), ("tyre", tyre), ("chair", chair),
                       ("anvil", anvil), ("lamp", lamp), ("pipe", pipe)):
        rgb, a = draw()
        # A little light from above.
        h = a.shape[0]
        shade = 1.1 - 0.25 * (np.arange(h)[:, None] / h)
        save(name, rgb * shade[..., None], a)


# ── 73 Spirograph ──────────────────────────────────────────────────────
# Dark drawing paper, tileable: a deep blue with a faint fibre.

def sp73_spirograph():
    out = OUT / "spiro"
    rng = np.random.default_rng(73)
    s = 256
    grid = rng.random((17, 17))
    grid[16, :] = grid[0, :]
    grid[:, 16] = grid[:, 0]
    y, x = np.mgrid[0:s, 0:s].astype(np.float64) / s * 16
    x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
    tx, ty = smoothstep(0.0, 1.0, x - x0), smoothstep(0.0, 1.0, y - y0)
    n = (grid[y0, x0] * (1 - tx) + grid[y0, x0 + 1] * tx) * (1 - ty) + (grid[y0 + 1, x0] * (1 - tx) + grid[y0 + 1, x0 + 1] * tx) * ty
    fibre = rng.random((s, s))
    rgb = np.array([0.035, 0.045, 0.085]) * (0.85 + 0.25 * n + 0.12 * fibre)[..., None]
    save_rgba(out / "paper.png", np.clip(rgb, 0, 1), np.ones((s, s)))


# ── 74 Baggage Claim ───────────────────────────────────────────────────
# An airport baggage hall at night: the wall and its window onto the apron,
# the floor, a belt's rubber slats and steel frame, the rubber flaps over
# the hatch the bags come through, a sign - and the luggage, drawn side on,
# each with a clear alpha so its collider is its outline.

def bc74_baggage():
    out = OUT / "baggage"
    rng = np.random.default_rng(74)
    SS = 3

    def canvas(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y, np.zeros((h * SS, w * SS, 3)), np.zeros((h * SS, w * SS))

    def put(rgb, a, mask, col):
        rgb[mask] = col
        a[mask] = 1.0

    def rrect(x, y, x0, y0, x1, y1, r):
        cx = np.clip(x, x0 + r, x1 - r)
        cy = np.clip(y, y0 + r, y1 - r)
        return (x - cx) ** 2 + (y - cy) ** 2 <= r * r

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    # The hall: a wall of panels, lit from above.
    w, h = 256, 256
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.array([0.30, 0.31, 0.34]) * (1.05 - 0.35 * (y / h))[..., None]
    rgb[(x % 128) < 2] *= 0.7
    rgb[(y % 128) < 2] *= 0.75
    save_rgba(out / "wall.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # The window onto the apron at night: sky, lights, a parked plane.
    w, h = 1024, 220
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    t = y / h
    rgb = np.array([0.03, 0.04, 0.10]) * (1 - t)[..., None] + np.array([0.12, 0.10, 0.16]) * t[..., None]
    apron = y > 170
    rgb[apron] = (0.08, 0.08, 0.09)
    for _ in range(40):
        lx, ly = rng.random() * w, 160 + rng.random() * 30
        m = (x - lx) ** 2 + ((y - ly) * 2) ** 2 < 4 + rng.random() * 6
        rgb[m] = np.array([1.0, 0.8, 0.4]) * (0.6 + 0.4 * rng.random())
    # A plane: fuselage, tail, wing, windows.
    body = rrect(x, y, 300, 110, 760, 150, 20)
    tail = (x > 690) & (x < 760) & (y > 40) & (y < 130) & (x - 690 > (130 - y) * 0.75)
    wing = (x > 470) & (x < 620) & (y > 140) & (y < 160 + (x - 470) * 0.12)
    rgb[body | tail | wing] = (0.55, 0.57, 0.62)
    rgb[body & (np.abs(y - 125) < 3) & ((x % 14) < 6) & (x > 330) & (x < 700)] = (1.0, 0.85, 0.5)
    rgb[tail & (y < 90)] = (0.6, 0.1, 0.12)
    for xx in range(0, w, 128):
        rgb[(x >= xx) & (x < xx + 6)] = (0.22, 0.22, 0.25)
    save_rgba(out / "window.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # Floor tiles.
    w, h = 256, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.array([0.42, 0.40, 0.37]) * (0.9 + 0.1 * rng.random((h, w)))[..., None]
    rgb[((x % 64) < 2) | ((y % 64) < 2)] = (0.3, 0.29, 0.27)
    rgb[y < 4] = (0.55, 0.53, 0.5)
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # A rubber slat, the belt's frame, the flaps, the hatch.
    x, y, rgb, a = canvas(40, 14)
    put(rgb, a, rrect(x, y, 1, 1, 39, 13, 3), (0.13, 0.13, 0.14))
    put(rgb, a, rrect(x, y, 3, 2, 37, 5, 1.5), (0.24, 0.24, 0.26))
    save("slat", rgb, a)
    w, h = 128, 32
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.array([0.55, 0.57, 0.6]) * (0.75 + 0.35 * np.cos((y / h - 0.3) * np.pi))[..., None]
    rgb[(x % 64) < 3] *= 0.7
    save_rgba(out / "frame.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    w, h = 140, 120
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.06, 0.06, 0.07])
    strips = (x % 14) < 12
    rgb[strips] = (0.12, 0.12, 0.13)
    rgb[strips & ((x % 14) < 3)] = (0.18, 0.18, 0.2)
    save_rgba(out / "flaps.png", rgb, np.ones((h, w)))
    # The sign: a yellow panel (its words are world text over it).
    w, h = 360, 90
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.95, 0.75, 0.15])
    rgb[(y < 4) | (y > h - 5)] = (0.6, 0.45, 0.08)
    save_rgba(out / "sign.png", rgb, np.ones((h, w)))

    # The luggage.
    def hardcase(name, w, h, col):
        x, y, rgb, a = canvas(w, h + 14)
        body = rrect(x, y, 2, 12, w - 2, h + 6, 8)
        put(rgb, a, body, col)
        for k in range(1, 4):
            rgb[body & (np.abs(x - w * k / 4) < 1.6)] = np.array(col) * 0.7
        handle = rrect(x, y, w * 0.35, 2, w * 0.65, 13, 3) & ~rrect(x, y, w * 0.35 + 4, 6, w * 0.65 - 4, 14, 1)
        put(rgb, a, handle, (0.15, 0.15, 0.16))
        for cx in (12, w - 12):
            put(rgb, a, (x - cx) ** 2 + (y - (h + 8)) ** 2 < 25, (0.1, 0.1, 0.1))
        save(name, rgb, a)

    hardcase("case_red", 120, 76, (0.72, 0.16, 0.14))
    hardcase("case_blue", 100, 64, (0.18, 0.32, 0.62))
    hardcase("case_silver", 90, 90, (0.66, 0.68, 0.72))

    x, y, rgb, a = canvas(140, 64)
    body = rrect(x, y, 2, 14, 138, 62, 22)
    put(rgb, a, body, (0.16, 0.3, 0.22))
    for cx in (42, 98):
        put(rgb, a, body & (np.abs(x - cx) < 4), (0.1, 0.1, 0.1))
        put(rgb, a, (np.abs(np.sqrt((x - cx) ** 2 + ((y - 14) * 1.6) ** 2) - 12) < 2.5) & (y < 16), (0.1, 0.1, 0.1))
    save("duffel", rgb, a)

    x, y, rgb, a = canvas(76, 96)
    body = rrect(x, y, 4, 8, 72, 94, 18)
    put(rgb, a, body, (0.82, 0.48, 0.12))
    put(rgb, a, rrect(x, y, 14, 52, 62, 86, 8), (0.7, 0.4, 0.1))
    put(rgb, a, (np.abs(np.sqrt((x - 38) ** 2 + ((y - 9) * 1.4) ** 2) - 10) < 2.5) & (y < 10), (0.25, 0.15, 0.06))
    save("backpack", rgb, a)

    x, y, rgb, a = canvas(190, 66)
    # A guitar case lying down: the body's two bouts and the neck.
    big = ((x - 46) / 42) ** 2 + ((y - 34) / 30) ** 2 < 1
    small = ((x - 102) / 30) ** 2 + ((y - 34) / 23) ** 2 < 1
    neck = rrect(x, y, 110, 24, 186, 44, 8)
    put(rgb, a, big | small | neck, (0.08, 0.08, 0.09))
    rgb[(big | small | neck) & (np.abs(y - 34) < 1.2)] = (0.25, 0.25, 0.27)
    save("guitar", rgb, a)

    x, y, rgb, a = canvas(96, 72)
    body = (x > 2) & (x < 94) & (y > 2) & (y < 70)
    put(rgb, a, body, (0.62, 0.46, 0.28))
    put(rgb, a, body & (np.abs(y - 36) < 6), (0.78, 0.68, 0.45))
    rgb[body & ((x < 5) | (x > 91) | (y < 5) | (y > 67))] = (0.48, 0.34, 0.2)
    save("box", rgb, a)


# ── 75 The Controls Room ───────────────────────────────────────────────
# A CRT's glass, dark green-black with a faint grid; a starfield for the
# playfield; the ship (pointing up) and its shield.

def cr75_controls():
    out = OUT / "controls"
    rng = np.random.default_rng(75)
    w, h = 640, 360
    nx, ny, d = radius(w, h)
    rgb = np.array([0.010, 0.030, 0.018]) * (1.25 - 0.55 * d)[..., None]
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    grid = ((x % 32) < 1) | ((y % 32) < 1)
    rgb[grid] += np.array([0.0, 0.025, 0.012])
    save_rgba(out / "screen.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    w, h = 512, 448
    rgb = np.zeros((h, w, 3))
    a = np.zeros((h, w))
    for _ in range(140):
        sx, sy = int(rng.random() * w), int(rng.random() * h)
        b = 0.3 + 0.7 * rng.random() ** 3
        rgb[sy, sx] = (b * 0.8, b, b * 0.85)
        a[sy, sx] = 1.0
    save_rgba(out / "stars.png", rgb, a)
    # The ship, drawn at 3x: an arrowhead, nose up, with a cockpit.
    s = 144
    y, x = np.mgrid[0:s, 0:s].astype(np.float64) / 3.0
    def tri(px, py, a_, b_, c_):
        def side(p1, p2):
            return (px - p2[0]) * (p1[1] - p2[1]) - (p1[0] - p2[0]) * (py - p2[1])
        d1, d2, d3 = side(a_, b_), side(b_, c_), side(c_, a_)
        neg = (d1 < 0) | (d2 < 0) | (d3 < 0)
        pos = (d1 > 0) | (d2 > 0) | (d3 > 0)
        return ~(neg & pos)
    hull = tri(x, y, (24, 3), (44, 44), (24, 34)) | tri(x, y, (24, 3), (4, 44), (24, 34))
    edge = hull & ~(tri(x, y, (24, 8), (40, 41), (24, 31)) | tri(x, y, (24, 8), (8, 41), (24, 31)))
    cockpit = ((x - 24) / 3.5) ** 2 + ((y - 20) / 6) ** 2 < 1
    rgb = np.zeros((s, s, 3)) + np.array([0.15, 0.5, 0.25])
    rgb[edge] = (0.6, 1.0, 0.7)
    rgb[cockpit] = (0.8, 1.0, 0.9)
    a = (hull | cockpit).astype(np.float64)
    img = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(48, 3, 48, 3, 4).mean(axis=(1, 3))
    save_rgba(out / "ship.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])
    w = 128
    _, _, d = radius(w)
    ring = np.exp(-((d - 0.82) / 0.07) ** 2) + 0.15 * smoothstep(0.85, 0.0, d)
    save_rgba(out / "shield.png", np.zeros((w, w, 3)) + 1.0, np.clip(ring, 0, 1))


# ── 76 Monster Zoo ─────────────────────────────────────────────────────
# A zoo of one creature and its variants: the creature drawn in pale greys
# so a variant's colour tints it - plain, horned (the ember beasts) and
# winged (the sky beasts), each a texture a variant swaps in - and the
# grass, the gravel path, the pens' fences and their plaques.

def mz76_zoo():
    out = OUT / "zoo"
    rng = np.random.default_rng(76)
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def beast(w, h, ox, oy, horn=False, wings=False):
        """The beast on a w x h canvas, its 96 x 96 body at (ox, oy)."""
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        x, y = x - ox, y - oy
        rgb, a = np.zeros(x.shape + (3,)), np.zeros(x.shape)

        def put(mask, col):
            rgb[mask] = col
            a[mask] = 1.0

        if wings:
            for side in (-1, 1):
                cx = 48 + side * 52
                wing = ((x - cx) / 36) ** 2 + ((y - 34) / 22) ** 2 < 1
                put(wing, (0.97, 0.98, 1.0))
                for k in range(3):
                    rgb[wing & (np.abs(y - (38 + k * 6) + (x - cx) * side * 0.15) < 1.0)] = (0.8, 0.84, 0.9)
        body = ((x - 48) / 38) ** 2 + ((y - 54) / 34) ** 2 < 1
        ears = (((x - 26) / 9) ** 2 + ((y - 20) / 14) ** 2 < 1) | (((x - 70) / 9) ** 2 + ((y - 20) / 14) ** 2 < 1)
        feet = (((x - 32) / 10) ** 2 + ((y - 88) / 5) ** 2 < 1) | (((x - 64) / 10) ** 2 + ((y - 88) / 5) ** 2 < 1)
        fur = body | ears | feet
        put(fur, (0.86, 0.86, 0.86))
        shade = np.clip(1.05 - 0.35 * ((y - 30) / 60), 0.6, 1.05)
        rgb[fur] *= shade[fur][:, None]
        rgb[((x - 48) / 22) ** 2 + ((y - 64) / 18) ** 2 < 1] = (0.98, 0.98, 0.98)
        if horn:
            h_ = (np.abs(x - 48) < (y + 14) * 0.22) & (y > -14) & (y < 22)
            put(h_, (0.95, 0.88, 0.7))
            rgb[h_ & ((np.floor(y) % 8) < 2)] = (0.75, 0.66, 0.5)
        for ex in (36, 60):
            rgb[((x - ex) / 8) ** 2 + ((y - 44) / 9) ** 2 < 1] = (1.0, 1.0, 1.0)
            rgb[((x - ex - 1.5) / 4) ** 2 + ((y - 45) / 5) ** 2 < 1] = (0.05, 0.05, 0.06)
        rgb[(np.abs(np.sqrt((x - 48) ** 2 + (y - 54) ** 2) - 9) < 1.2) & (y > 57)] = (0.25, 0.2, 0.2)
        return rgb, a

    save("beast", *beast(96, 96, 0, 0))
    save("beast_horned", *beast(96, 120, 0, 24, horn=True))
    save("beast_winged", *beast(200, 96, 52, 0, wings=True))
    # The grounds: grass, a gravel path, a wooden fence (tileable), a plaque.
    w = 256
    fl = rng.random((w, w))
    rgb = np.array([0.26, 0.45, 0.18]) * (0.85 + 0.2 * fl)[..., None]
    rgb[fl > 0.97] = (0.4, 0.62, 0.25)
    save_rgba(out / "grass.png", np.clip(rgb, 0, 1), np.ones((w, w)))
    rgb = np.array([0.62, 0.56, 0.46]) * (0.75 + 0.35 * fl)[..., None]
    save_rgba(out / "gravel.png", np.clip(rgb, 0, 1), np.ones((w, w)))
    w, h = 128, 64
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    posts = ((x % 64) < 10) & (y > 4)
    rails = (np.abs(y - 20) < 4) | (np.abs(y - 44) < 4)
    m = posts | rails
    rgb = np.zeros((h, w, 3)) + np.array([0.5, 0.33, 0.18])
    rgb[posts] *= 0.85
    rgb[m & ((x + y * 0.3) % 9 < 1)] *= 0.8
    save_rgba(out / "fence.png", np.clip(rgb, 0, 1), m.astype(np.float64))
    w, h = 240, 70
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.86, 0.78, 0.6])
    rgb[(x < 4) | (x > w - 5) | (y < 4) | (y > h - 5)] = (0.45, 0.3, 0.15)
    save_rgba(out / "plaque.png", rgb, np.ones((h, w)))

# ── 77 The Heist ───────────────────────────────────────────────────────
# A museum at night seen through its security cameras, from above: marble
# floors, a steel vault, an octagonal rotunda inlaid with a star, the dark
# garden outside the walls, framed paintings, plinths, a statue, the jewel,
# and the thief.

def hs77_heist():
    out = OUT / "heist"
    rng = np.random.default_rng(77)
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def tile_noise(w, scale, seed):
        """Tileable value noise: a random grid, wrapped, smoothly interpolated."""
        r = np.random.default_rng(seed)
        n = w // scale
        g = r.random((n, n))
        y, x = np.mgrid[0:w, 0:w].astype(np.float64) / scale
        x0, y0 = np.floor(x).astype(int), np.floor(y).astype(int)
        fx, fy = x - x0, y - y0
        fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
        a, b = g[y0 % n, x0 % n], g[y0 % n, (x0 + 1) % n]
        c, d = g[(y0 + 1) % n, x0 % n], g[(y0 + 1) % n, (x0 + 1) % n]
        return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy

    # Marble: two tones of square slab, veined, with dark grout.
    w = 256
    y, x = np.mgrid[0:w, 0:w].astype(np.float64)
    n = 0.6 * tile_noise(w, 32, 1) + 0.4 * tile_noise(w, 8, 2)
    vein = np.abs(np.sin((x * 0.6 + y * 0.35) / 256 * 2 * np.pi * 3 + n * 9)) < 0.07
    checker = ((x // 128) + (y // 128)) % 2
    base = np.where(checker[..., None] > 0, np.array([0.86, 0.84, 0.8]), np.array([0.58, 0.56, 0.54]))
    rgb = base * (0.9 + 0.12 * n)[..., None]
    rgb[vein] *= 0.82
    grout = ((x % 128) < 2) | ((y % 128) < 2)
    rgb[grout] = (0.25, 0.24, 0.23)
    save_rgba(out / "marble.png", np.clip(rgb, 0, 1), np.ones((w, w)))

    # A carpet runner, tileable along x: crimson with a gold border and a
    # repeating diamond.
    cw, ch = 128, 128
    cy_, cx_ = np.mgrid[0:ch, 0:cw].astype(np.float64)
    rgb = np.zeros((ch, cw, 3)) + np.array([0.5, 0.06, 0.08])
    rgb *= (0.9 + 0.1 * tile_noise(128, 8, 9))[..., None]
    d = np.abs(cx_ - 64) / 40 + np.abs(cy_ - 64) / 36
    rgb[np.abs(d - 0.8) < 0.06] = (0.78, 0.6, 0.25)
    rgb[d < 0.25] = (0.7, 0.5, 0.2)
    rgb[(cy_ < 10) | (cy_ > ch - 11)] = (0.72, 0.55, 0.22)
    rgb[(cy_ < 4) | (cy_ > ch - 5)] = (0.3, 0.04, 0.05)
    save_rgba(out / "carpet.png", np.clip(rgb, 0, 1), np.ones((ch, cw)))

    # The vault's floor: riveted steel plates.
    n = tile_noise(w, 16, 3)
    rgb = np.zeros((w, w, 3)) + np.array([0.36, 0.39, 0.42]) * (0.88 + 0.16 * n)[..., None]
    seam = ((x % 128) < 3) | ((y % 128) < 3)
    rgb[seam] = (0.16, 0.17, 0.18)
    rx, ry = x % 128, y % 128
    rivet = np.zeros((w, w), bool)
    for cx in (12, 116):
        for cy in (12, 116):
            rivet |= (rx - cx) ** 2 + (ry - cy) ** 2 < 16
    rgb[rivet] = (0.62, 0.64, 0.66)
    save_rgba(out / "steel.png", np.clip(rgb, 0, 1), np.ones((w, w)))

    # The garden outside: dark hedges and gravel - what a camera sees when it
    # looks past a wall.
    n = 0.7 * tile_noise(w, 32, 4) + 0.3 * tile_noise(w, 8, 5)
    rgb = np.zeros((w, w, 3)) + np.array([0.08, 0.16, 0.07]) * (0.6 + 0.8 * n)[..., None]
    rgb[n > 0.62] = np.array([0.14, 0.26, 0.11]) * 1.0
    save_rgba(out / "garden.png", np.clip(rgb, 0, 1), np.ones((w, w)))

    # The rotunda's floor: an octagon of dark marble, a ring and an
    # eight-pointed star inlaid in gold and white. Flat edges top and bottom,
    # to match the room's outline in tools/gen_scenes.py.
    w = 512
    y, x = (np.mgrid[0:w * SS, 0:w * SS].astype(np.float64) / SS - w / 2) / (w / 2)
    ang = np.arctan2(y, x)
    r = np.sqrt(x * x + y * y)
    # Apothem 1: an octagon whose flat sides face the axes.
    oct_d = np.max(np.stack([np.abs(x), np.abs(y), (np.abs(x) + np.abs(y)) / np.sqrt(2)]), axis=0)
    inside = oct_d < 1.0
    n = np.random.default_rng(6).random(x.shape)
    rgb = np.zeros(x.shape + (3,)) + np.array([0.3, 0.26, 0.28])
    rgb *= (0.92 + 0.08 * n)[..., None]
    rgb[(oct_d > 0.93) & inside] = (0.6, 0.5, 0.3)
    rgb[np.abs(r - 0.62) < 0.012] = (0.85, 0.7, 0.35)
    rgb[np.abs(r - 0.66) < 0.006] = (0.85, 0.7, 0.35)
    star = r < 0.55 * (0.55 + 0.45 * np.abs(np.cos(4 * ang)) ** 6)
    rgb[star] = (0.9, 0.88, 0.84)
    star2 = r < 0.42 * (0.5 + 0.5 * np.abs(np.cos(4 * ang + np.pi / 4)) ** 6)
    rgb[star2] = (0.85, 0.68, 0.32)
    rgb[r < 0.08] = (0.3, 0.26, 0.28)
    save("rotunda", np.clip(rgb, 0, 1), inside.astype(np.float64))

    # Paintings: a gilt frame round four small landscapes and portraits.
    for k in range(4):
        w, h = 120, 90
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        rgb = np.zeros(x.shape + (3,))
        u, v = (x - 12) / (w - 24), (y - 12) / (h - 24)
        if k == 0:  # a sunset over the sea
            rgb[:] = np.stack([0.95 - 0.3 * v, 0.55 - 0.2 * v, 0.35 + 0.2 * v], axis=-1)
            rgb[v > 0.6] = (0.15, 0.25, 0.45)
            rgb[((u - 0.5) / 0.12) ** 2 + ((v - 0.58) / 0.12) ** 2 < 1] = (1.0, 0.8, 0.4)
        elif k == 1:  # a portrait
            rgb[:] = (0.2, 0.15, 0.12)
            rgb[((u - 0.5) / 0.2) ** 2 + ((v - 0.35) / 0.25) ** 2 < 1] = (0.85, 0.7, 0.58)
            rgb[((u - 0.5) / 0.38) ** 2 + ((v - 1.0) / 0.4) ** 2 < 1] = (0.15, 0.2, 0.4)
        elif k == 2:  # green hills
            rgb[:] = (0.6, 0.75, 0.9)
            rgb[v > 0.45 + 0.12 * np.sin(u * 7)] = (0.3, 0.55, 0.25)
            rgb[v > 0.7 + 0.08 * np.sin(u * 11 + 1)] = (0.2, 0.42, 0.2)
        else:  # a still life
            rgb[:] = (0.35, 0.3, 0.25)
            rgb[((u - 0.4) / 0.15) ** 2 + ((v - 0.6) / 0.15) ** 2 < 1] = (0.85, 0.2, 0.15)
            rgb[((u - 0.62) / 0.12) ** 2 + ((v - 0.62) / 0.12) ** 2 < 1] = (0.9, 0.75, 0.2)
            rgb[v > 0.78] = (0.5, 0.35, 0.2)
        frame = (x < 12) | (x > w - 12) | (y < 12) | (y > h - 12)
        rgb[frame] = np.array([0.8, 0.62, 0.25]) * (0.85 + 0.25 * np.sin((x + y)[frame] * 0.5))[..., None]
        save(f"painting{k}", np.clip(rgb, 0, 1), np.ones(x.shape))

    def round_thing(name, size, layers):
        """Top-down discs: [(radius 0..1, colour), ...] outermost first."""
        y, x = (np.mgrid[0:size * SS, 0:size * SS].astype(np.float64) / SS - size / 2) / (size / 2)
        r = np.sqrt(x * x + y * y)
        rgb = np.zeros(x.shape + (3,))
        a = np.zeros(x.shape)
        for rad, col in layers:
            m = r < rad
            # Lit from the top left.
            shade = np.clip(1.05 - 0.25 * (x - y) / max(rad, 1e-3), 0.7, 1.15)
            rgb[m] = np.array(col) * shade[m][:, None]
            a[m] = 1.0
        return rgb, a

    save("plinth", *round_thing("plinth", 96, [(0.98, (0.55, 0.52, 0.5)), (0.8, (0.8, 0.78, 0.74))]))
    save("vase", *round_thing("vase", 64, [(0.95, (0.2, 0.32, 0.6)), (0.7, (0.85, 0.82, 0.7)),
                                           (0.45, (0.2, 0.32, 0.6)), (0.25, (0.05, 0.05, 0.08))]))
    # The statue from above: a figure's shoulders, head and raised arm.
    w = 160
    y, x = np.mgrid[0:w * SS, 0:w * SS].astype(np.float64) / SS
    rgb = np.zeros(x.shape + (3,))
    a = np.zeros(x.shape)
    base = ((x - 80) / 74) ** 2 + ((y - 80) / 74) ** 2 < 1
    rgb[base] = (0.45, 0.43, 0.42)
    a[base] = 1
    body = (((x - 80) / 44) ** 2 + ((y - 84) / 26) ** 2 < 1) | (((x - 116) / 30) ** 2 + ((y - 70) / 9) ** 2 < 1)
    head = ((x - 80) / 17) ** 2 + ((y - 82) / 17) ** 2 < 1
    rgb[body] = (0.88, 0.87, 0.84)
    rgb[head] = (0.95, 0.94, 0.92)
    rgb[base & ~body & ~head] *= (0.85 + 0.15 * np.cos(np.arctan2(y - 80, x - 80) * 12))[base & ~body & ~head][:, None]
    save("statue", np.clip(rgb, 0, 1), a)
    # The jewel: a cut stone with facets and a sparkle.
    w = 64
    y, x = (np.mgrid[0:w * SS, 0:w * SS].astype(np.float64) / SS - w / 2) / (w / 2)
    oct_d = np.max(np.stack([np.abs(x), np.abs(y), (np.abs(x) + np.abs(y)) / np.sqrt(2)]), axis=0)
    gem = oct_d < 0.8
    ang = np.arctan2(y, x)
    rgb = np.zeros(x.shape + (3,))
    facet = (np.floor((ang + np.pi) / (np.pi / 4)) % 2)
    rgb[gem] = np.array([0.3, 0.9, 0.75]) * (0.75 + 0.3 * facet[gem])[:, None]
    rgb[oct_d < 0.38] = (0.75, 1.0, 0.92)
    save("jewel", np.clip(rgb, 0, 1), gem.astype(np.float64))
    # The thief from above, facing +x: hood, shoulders, a sack on the back.
    w = 72
    y, x = np.mgrid[0:w * SS, 0:w * SS].astype(np.float64) / SS
    rgb = np.zeros(x.shape + (3,))
    a = np.zeros(x.shape)

    def put(m, col):
        rgb[m] = col
        a[m] = 1

    put(((x - 24) / 16) ** 2 + ((y - 36) / 18) ** 2 < 1, (0.55, 0.42, 0.25))
    put(((x - 24) / 16) ** 2 + ((y - 36) / 18) ** 2 < 0.25, (0.45, 0.33, 0.18))
    put(((x - 38) / 13) ** 2 + ((y - 36) / 26) ** 2 < 1, (0.12, 0.12, 0.15))
    put(((x - 44) / 12) ** 2 + ((y - 36) / 12) ** 2 < 1, (0.18, 0.18, 0.22))
    put(((x - 52) / 5) ** 2 + ((y - 36) / 7) ** 2 < 1, (0.85, 0.72, 0.6))
    for side in (-1, 1):
        put(((x - 50) / 9) ** 2 + ((y - 36 - side * 20) / 5) ** 2 < 1, (0.12, 0.12, 0.15))
    save("thief", rgb, a)

# ── 78 Echo Canyon ─────────────────────────────────────────────────────
# Dusk in a desert canyon, side on: a sky going from indigo to amber, far
# mesas, two banded canyon walls, the ground, a camp (tent, crate, radio,
# cactus) and the hiker walking and calling.

def ec78_canyon():
    out = OUT / "canyon"
    rng = np.random.default_rng(78)
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def ridge(w, octaves, seed, rough=0.5):
        """A 1D height profile in 0..1, `w` samples, from summed sines."""
        r = np.random.default_rng(seed)
        x = np.linspace(0, 1, w)
        h = np.zeros(w)
        amp = 1.0
        for o in range(octaves):
            f = 2 ** o
            h += amp * np.sin(2 * np.pi * (f * x + r.random())) * (0.6 + 0.4 * r.random())
            amp *= rough
        return (h - h.min()) / (h.max() - h.min())

    # The sky: indigo above, rose, then amber at the horizon.
    h = 256
    v = np.linspace(0, 1, h)[:, None]
    top, mid, low = np.array([0.10, 0.10, 0.26]), np.array([0.62, 0.34, 0.45]), np.array([1.0, 0.68, 0.36])
    rgb = np.where(v < 0.55, top + (mid - top) * (v / 0.55), mid + (low - mid) * ((v - 0.55) / 0.45))
    rgb = np.repeat(rgb[:, None, :] if rgb.ndim == 2 else rgb, 4, axis=1).reshape(h, 4, 3)
    save_rgba(out / "sky.png", np.clip(rgb, 0, 1), np.ones((h, 4)))

    # Far mesas: flat tops and steep sides, a purple silhouette.
    w, h = 1024, 256
    prof = ridge(w, 3, 1, 0.45)
    tops = np.round(prof * 3) / 3  # terraced: the flat tops of mesas
    prof = 0.25 + 0.55 * (0.3 * prof + 0.7 * tops)
    y = np.arange(h)[:, None] / h
    a = (y > 1 - prof[None, :]).astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.36, 0.24, 0.38])
    rgb *= (0.85 + 0.15 * (1 - y))[..., None]
    save_rgba(out / "mesas.png", rgb, a)

    def wall(w, h, face_right, seed):
        """A canyon wall: banded red rock, its inner face ragged."""
        yy, xx = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        r = np.random.default_rng(seed)
        edge = ridge(h * SS, 5, seed, 0.55)
        # The face's x at each height: far out at the top, near at the foot.
        u = yy / h
        face = w * (0.35 + 0.45 * edge[(yy * SS).astype(int).clip(0, h * SS - 1)] * (0.4 + 0.6 * u))
        inside = xx < face if face_right else xx > (w - face)
        band = np.sin(yy * 0.11 + 2 * np.sin(xx * 0.01 + seed)) * 0.5 + 0.5
        rgb = np.zeros(yy.shape + (3,))
        base = np.array([0.66, 0.30, 0.18])
        stripe = np.array([0.84, 0.50, 0.28])
        rgb[:] = base + (stripe - base) * (band ** 3)[..., None]
        # Lit from the sunset: the face edge glows.
        dist = np.abs(xx - (face if face_right else w - face))
        rgb += np.array([0.25, 0.12, 0.02]) * np.exp(-dist / 10)[..., None]
        rgb *= (0.7 + 0.3 * (1 - u))[..., None]
        cracks = np.abs(np.sin(xx * 0.07 + yy * 0.013 + r.random() * 6)) < 0.03
        rgb[cracks] *= 0.7
        return np.clip(rgb, 0, 1), inside.astype(np.float64)

    save("wall_left", *wall(420, 720, True, 5))
    save("wall_right", *wall(420, 720, False, 9))

    # The ground: sand and pebbles, tileable along x.
    w, h = 256, 128
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.70, 0.46, 0.28])
    rgb *= (0.9 + 0.1 * rng.random((h, w)))[..., None]
    rgb *= (1.0 - 0.35 * yy / h)[..., None]
    for _ in range(60):
        cx, cy, rr = rng.integers(0, w), rng.integers(10, h), rng.integers(2, 5)
        d = ((xx - cx + w / 2) % w - w / 2) ** 2 + (yy - cy) ** 2
        rgb[d < rr * rr] = np.array([0.5, 0.36, 0.26]) * (0.8 + 0.4 * rng.random())
    rgb[yy < 4] = (0.86, 0.62, 0.38)
    save_rgba(out / "ground.png", np.clip(rgb, 0, 1), np.ones((h, w)))

    def canvas(w, h):
        yy, xx = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return xx, h - yy, np.zeros(xx.shape + (3,)), np.zeros(xx.shape)

    # The tent: a canvas triangle with its door open.
    xx, yy, rgb, a = canvas(200, 130)
    tent = (yy < 120 - np.abs(xx - 100) * 1.15) & (yy > 0)
    rgb[tent] = (0.85, 0.74, 0.52)
    rgb[tent & (xx > 100)] = (0.70, 0.58, 0.40)
    door = (yy < 70 - np.abs(xx - 100) * 1.5) & (yy > 0)
    rgb[door] = (0.18, 0.12, 0.10)
    a[tent] = 1
    save("tent", rgb, a)
    # A crate with the radio on it.
    xx, yy, rgb, a = canvas(90, 64)
    box = (xx > 4) & (xx < 86) & (yy < 60)
    rgb[box] = (0.55, 0.38, 0.22)
    rgb[box & ((np.abs(xx - 45) < 3) | (np.abs(yy - 30) < 3))] = (0.42, 0.28, 0.16)
    a[box] = 1
    save("crate", rgb, a)
    xx, yy, rgb, a = canvas(100, 110)
    body = (xx > 8) & (xx < 92) & (yy > 4) & (yy < 60)
    rgb[body] = (0.48, 0.18, 0.14)
    grille = body & (xx > 16) & (xx < 52) & (yy > 12) & (yy < 52)
    rgb[grille & ((yy.astype(int) % 6) < 3)] = (0.25, 0.10, 0.08)
    dial = ((xx - 72) ** 2 + (yy - 36) ** 2) < 12 ** 2
    rgb[dial] = (0.95, 0.85, 0.55)
    rgb[dial & (np.abs((xx - 72) - (yy - 36) * 0.4) < 1.5) & (yy > 36)] = (0.3, 0.1, 0.05)
    antenna = (np.abs(xx - 20 - (yy - 60) * 0.5) < 1.6) & (yy >= 60) & (yy < 108)
    tip = ((xx - 44) ** 2 + (yy - 106) ** 2) < 16
    a[body | antenna | tip] = 1
    rgb[antenna | tip] = (0.75, 0.75, 0.78)
    save("radio", rgb, a)
    # A saguaro.
    xx, yy, rgb, a = canvas(110, 190)
    trunk = (np.abs(xx - 55) < 13) & (yy < 180) | (((xx - 55) ** 2 + (yy - 180) ** 2) < 13 ** 2)
    arm1 = ((np.abs(xx - 26) < 9) & (yy > 80) & (yy < 140)) | ((np.abs(yy - 80) < 9) & (xx > 26) & (xx < 55)) | (((xx - 26) ** 2 + (yy - 140) ** 2) < 81)
    arm2 = ((np.abs(xx - 86) < 9) & (yy > 100) & (yy < 150)) | ((np.abs(yy - 100) < 9) & (xx > 55) & (xx < 86)) | (((xx - 86) ** 2 + (yy - 150) ** 2) < 81)
    c = trunk | arm1 | arm2
    rgb[c] = (0.22, 0.42, 0.24)
    rgb[c & (np.sin(xx * 1.1) > 0.6)] = (0.30, 0.52, 0.30)
    a[c] = 1
    save("cactus", rgb, a)

    def hiker(arms_up):
        xx, yy, rgb, a = canvas(70, 120)

        def put(m, col):
            rgb[m] = col
            a[m] = 1

        legs = ((np.abs(xx - 30) < 5) | (np.abs(xx - 41) < 5)) & (yy < 46)
        put(legs, (0.24, 0.26, 0.34))
        put(((np.abs(xx - 30) < 7) | (np.abs(xx - 42) < 7)) & (yy < 8), (0.30, 0.20, 0.14))
        put((np.abs(xx - 36) < 13) & (yy > 40) & (yy < 86), (0.82, 0.38, 0.20))
        put((np.abs(xx - 22) < 9) & (yy > 44) & (yy < 82), (0.40, 0.30, 0.20))  # backpack
        put(((xx - 37) ** 2 + (yy - 97) ** 2) < 10 ** 2, (0.92, 0.72, 0.56))
        put((np.abs(xx - 37) < 15) & (np.abs(yy - 105) < 3), (0.55, 0.40, 0.22))  # hat brim
        put((np.abs(xx - 37) < 8) & (yy > 105) & (yy < 114), (0.55, 0.40, 0.22))
        if arms_up:
            put((np.abs(xx - 52 - (yy - 80) * 0.25) < 4) & (yy > 80) & (yy < 112), (0.82, 0.38, 0.20))
            put((np.abs(xx - 47 - (yy - 80) * 0.1) < 4) & (yy > 84) & (yy < 104), (0.82, 0.38, 0.20))
        else:
            put((np.abs(xx - 46) < 4) & (yy > 50) & (yy < 82), (0.76, 0.34, 0.18))
        return rgb, a

    save("hiker", *hiker(False))
    save("hiker_call", *hiker(True))

# ── 79 Paper Dolls ─────────────────────────────────────────────────────
# A paper doll and her wardrobe, cut from card: every piece has a white
# paper border and a soft shadow, so it reads as cut out and laid on. The
# body (head, arms, legs) and four outfits - summer, rain, winter, party -
# of hat, coat, sleeve and boot; and the little theatre she stands in.

def pd79_dolls():
    out = OUT / "dolls"
    SS = 3
    PAD = 12

    def piece(name, w, h, draw, border=3.0, shadow=(4.0, -4.0)):
        """Draw a piece: `draw(x, y)` -> (rgb, mask) in the piece's own pixels
        (y up, origin bottom-left); add a paper border and a drop shadow."""
        # Drawn with a margin of PAD pixels all round, for the border and
        # the shadow; the scene allows for it (`PD_PAD`).
        w, h = w + 2 * PAD, h + 2 * PAD
        yy, xx = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        x, y = xx - PAD, h - yy - PAD
        rgb, m = draw(x, y)
        m = m.astype(bool)
        # The border: the mask grown by `border` pixels (a disc dilation).
        r = int(border * SS)
        grown = m.copy()
        for dy in range(-r, r + 1):
            for dx in range(-r, r + 1):
                if dx * dx + dy * dy <= r * r and (dx or dy):
                    grown |= np.roll(np.roll(m, dy, axis=0), dx, axis=1)
        out_rgb = np.zeros(m.shape + (3,))
        # The shadow: the grown shape moved down-right and blurred (three box
        # blurs make a near-Gaussian).
        sh = np.roll(np.roll(grown.astype(np.float64), int(-shadow[1] * SS), axis=0), int(shadow[0] * SS), axis=1)
        k = 3 * SS
        for _ in range(3):
            for axis in (0, 1):
                c = np.cumsum(np.pad(sh, [(k, k) if a_ == axis else (0, 0) for a_ in (0, 1)], mode="edge"), axis=axis)
                lo = np.take(c, range(0, sh.shape[axis]), axis=axis)
                hi = np.take(c, range(2 * k, 2 * k + sh.shape[axis]), axis=axis)
                sh = (hi - lo) / (2 * k)
        out_a = sh * 0.35
        out_rgb[grown] = (0.97, 0.95, 0.9)
        out_a[grown] = 1.0
        out_rgb[m] = rgb[m]
        # Card grain.
        grain = np.random.default_rng(len(name)).random(m.shape) * 0.05
        out_rgb = out_rgb * (0.97 + grain)[..., None]
        img_rgb = out_rgb.reshape(h, SS, w, SS, 3).mean(axis=(1, 3))
        img_a = out_a.reshape(h, SS, w, SS).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img_rgb, 0, 1), np.clip(img_a, 0, 1))

    def ell(x, y, cx, cy, rx, ry):
        return ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2 < 1

    def fill(shape, col):
        rgb = np.zeros(shape.shape + (3,))
        rgb[shape] = col
        return rgb

    SKIN = np.array([0.95, 0.78, 0.64])

    # The body.
    def head(x, y):
        face = ell(x, y, 55, 52, 34, 40)
        hair = ell(x, y, 55, 66, 37, 30) & ~ell(x, y, 55, 46, 30, 32) | ell(x, y, 22, 40, 8, 22) | ell(x, y, 88, 40, 8, 22)
        neck = (np.abs(x - 55) < 10) & (y > 0) & (y < 16)
        m = face | hair | neck
        rgb = fill(m, SKIN)
        rgb[hair] = (0.40, 0.24, 0.14)
        for ex in (43, 67):
            rgb[ell(x, y, ex, 54, 4, 5)] = (0.15, 0.12, 0.12)
        rgb[ell(x, y, 37, 42, 7, 5) | ell(x, y, 73, 42, 7, 5)] = (0.98, 0.62, 0.6)
        smile = (np.abs(np.hypot(x - 55, y - 44) - 11) < 1.6) & (y < 40)
        rgb[smile] = (0.6, 0.2, 0.2)
        return rgb, m

    piece("head", 110, 112, head)

    def arm(x, y):
        m = (np.abs(x - 18) < 9) & (y > 10) & (y < 120) | ell(x, y, 18, 12, 11, 11)
        return fill(m, SKIN), m

    piece("arm", 36, 130, arm)

    def leg(x, y):
        m = (np.abs(x - 20) < 13) & (y > 0) & (y < 160)
        rgb = fill(m, (0.32, 0.36, 0.5))
        rgb[m & (np.abs(x - 20) < 1.2)] = (0.26, 0.29, 0.42)
        return rgb, m

    piece("leg", 40, 160, leg)

    # The wardrobe. Each outfit: hat, coat, sleeve, boot.
    def coat_shape(x, y):
        return (np.abs(x - 80) < 62 - (y / 170) * 6) & (y > 4) & (y < 158) | ell(x, y, 80, 150, 60, 18)

    def sleeve_shape(x, y):
        return (np.abs(x - 22) < 14) & (y > 40) & (y < 132) | ell(x, y, 22, 130, 15, 8)

    def boot_shape(x, y, tall=40, toe=24):
        return ((np.abs(x - 30) < 17) & (y > 6) & (y < tall)) | ell(x, y, 30 + toe * 0.5, 12, 17 + toe * 0.5, 10)

    outfits = {
        "summer": dict(coat=(0.98, 0.84, 0.3), trim=(0.95, 0.5, 0.3), boot=(0.66, 0.42, 0.24), hat=(0.94, 0.82, 0.5)),
        "rain": dict(coat=(1.0, 0.82, 0.12), trim=(0.8, 0.6, 0.1), boot=(1.0, 0.82, 0.12), hat=(1.0, 0.82, 0.12)),
        "winter": dict(coat=(0.78, 0.14, 0.16), trim=(0.96, 0.94, 0.9), boot=(0.48, 0.32, 0.2), hat=(0.2, 0.42, 0.7)),
        "party": dict(coat=(0.12, 0.12, 0.16), trim=(0.96, 0.96, 0.96), boot=(0.08, 0.08, 0.1), hat=(0.1, 0.1, 0.12)),
    }
    for label, c in outfits.items():
        def coat(x, y, c=c, label=label):
            m = coat_shape(x, y)
            rgb = fill(m, c["coat"])
            if label == "summer":  # a T-shirt with a stripe
                rgb[m & (np.abs(y - 90) < 9)] = c["trim"]
            elif label == "rain":  # buttons down the front
                for by in (40, 75, 110):
                    rgb[ell(x, y, 80, by, 5, 5)] = (0.3, 0.25, 0.1)
                rgb[m & (np.abs(x - 80) < 1.5)] = c["trim"]
            elif label == "winter":  # a fur hem and collar
                rgb[m & ((y < 22) | (y > 140))] = c["trim"]
            else:  # a tuxedo: white shirt, lapels, bow tie
                rgb[m & (np.abs(x - 80) < 18 - (158 - y) * 0.05) & (y > 50)] = (0.96, 0.96, 0.96)
                rgb[ell(x, y, 72, 148, 9, 6) | ell(x, y, 88, 148, 9, 6)] = (0.75, 0.1, 0.15)
            return rgb, m

        def sleeve(x, y, c=c, label=label):
            m = sleeve_shape(x, y) & (y > (95 if label == "summer" else 40))
            rgb = fill(m, c["coat"])
            if label == "winter":
                rgb[m & (y < 52)] = c["trim"]
            return rgb, m

        def boot(x, y, c=c, label=label):
            tall = {"summer": 14, "rain": 54, "winter": 46, "party": 22}[label]
            m = boot_shape(x, y, tall)
            rgb = fill(m, c["boot"])
            rgb[m & (y < 7)] = np.array(c["boot"]) * 0.6
            if label == "summer":  # sandals: straps over a bare foot
                foot = m.copy()
                rgb[foot] = SKIN
                for sy in (10, 18):
                    rgb[m & (np.abs(y - sy) < 2.2)] = c["boot"]
            if label == "winter":
                rgb[m & (np.abs(y - tall) < 6)] = (0.96, 0.94, 0.9)
            return rgb, m

        def hat(x, y, c=c, label=label):
            if label == "summer":  # a straw sun hat
                m = ell(x, y, 70, 12, 66, 9) | ell(x, y, 70, 26, 34, 22) & (y > 10)
                rgb = fill(m, c["hat"])
                rgb[m & (np.abs(y - 18) < 4) & (np.abs(x - 70) < 34)] = (0.85, 0.3, 0.3)
            elif label == "rain":  # a sou'wester
                m = ell(x, y, 70, 14, 60, 12) | ell(x, y, 70, 26, 38, 26) & (y > 12)
                rgb = fill(m, c["hat"])
            elif label == "winter":  # a beanie and its pompom
                m = ell(x, y, 70, 18, 40, 30) & (y > 4) | ell(x, y, 70, 52, 13, 13)
                rgb = fill(m, c["hat"])
                rgb[m & (y < 14)] = (0.96, 0.94, 0.9)
                rgb[ell(x, y, 70, 52, 13, 13)] = (0.96, 0.94, 0.9)
            else:  # a top hat
                m = ell(x, y, 70, 8, 52, 7) | (np.abs(x - 70) < 32) & (y > 6) & (y < 78)
                rgb = fill(m, c["hat"])
                rgb[m & (np.abs(y - 18) < 5) & (np.abs(x - 70) < 32)] = (0.75, 0.1, 0.15)
            return rgb, m

        piece(f"coat_{label}", 160, 172, coat)
        piece(f"sleeve_{label}", 44, 140, sleeve)
        piece(f"boot_{label}", 64, 64, boot)
        piece(f"hat_{label}", 140, 84, hat)

    # The theatre: a dark backcloth, red curtains, boards.
    w, h = 256, 256
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.16, 0.12, 0.2])
    rgb *= (0.85 + 0.15 * np.sin(xx / w * 2 * np.pi * 4) ** 2)[..., None]
    stars = np.random.default_rng(79).random((h, w)) > 0.996
    rgb[stars] = (0.9, 0.85, 0.6)
    save_rgba(out / "backcloth.png", rgb, np.ones((h, w)))
    w, h = 128, 512
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    folds = 0.6 + 0.4 * np.cos(xx / w * 2 * np.pi * 3) ** 2
    rgb = np.array([0.62, 0.08, 0.1]) * folds[..., None] * (0.8 + 0.2 * (1 - yy / h))[..., None]
    save_rgba(out / "curtain.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    w, h = 256, 64
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.58, 0.38, 0.22])
    rgb[(xx % 64) < 2] = (0.36, 0.22, 0.12)
    rgb *= (0.9 + 0.1 * np.sin(yy * 0.9 + xx * 0.05))[..., None]
    rgb[yy < 5] = (0.75, 0.55, 0.32)
    save_rgba(out / "boards.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # A swing tag for each outfit's label on the rack.
    w, h = 150, 54
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float64)
    tag = (xx > 4) & (xx < w - 4) & (yy > 4) & (yy < h - 4)
    rgb = np.zeros((h, w, 3)) + np.array([0.96, 0.92, 0.82])
    rgb[((xx - 18) ** 2 + (yy - h / 2) ** 2) < 25] = (0.4, 0.3, 0.2)
    save_rgba(out / "tag.png", rgb, tag.astype(np.float64))

# ── 80 Treasure Isle ───────────────────────────────────────────────────
# A treasure island from above, as one 8 x 2 sheet of 64 px tiles - water in
# two frames, sand and shells, five kinds of grass, three of lava, rock, a
# dirt path, and the two marker tiles (an X for a chest, a sprout for a
# palm) that become things when the scene loads - plus the chest, the palm
# and the explorer.

def ti80_isle():
    out = OUT / "isle"
    rng = np.random.default_rng(80)
    T = 64
    sheet = np.zeros((2 * T, 8 * T, 3))
    yy, xx = np.mgrid[0:T, 0:T].astype(np.float64)

    def noise(seed, scale=8):
        r = np.random.default_rng(seed)
        n = r.random((T // scale + 2, T // scale + 2))
        u, v = xx / scale, yy / scale
        x0, y0 = u.astype(int), v.astype(int)
        fx, fy = u - x0, v - y0
        fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
        a, b = n[y0 % n.shape[0], x0 % n.shape[1]], n[y0 % n.shape[0], (x0 + 1) % n.shape[1]]
        c, d = n[(y0 + 1) % n.shape[0], x0 % n.shape[1]], n[(y0 + 1) % n.shape[0], (x0 + 1) % n.shape[1]]
        return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy

    def put(i, rgb):
        r, c = divmod(i, 8)
        sheet[r * T:(r + 1) * T, c * T:(c + 1) * T] = np.clip(rgb, 0, 1)

    def base(col, seed, amt=0.12):
        return np.zeros((T, T, 3)) + np.array(col) * (1 - amt + 2 * amt * noise(seed))[..., None]

    # 0, 1: water, two frames of the same waves a little apart.
    for i, ph in ((0, 0.0), (1, 1.6)):
        rgb = base((0.10, 0.42, 0.62), 1, 0.06)
        wave = np.sin((xx * 0.18 + yy * 0.07) + ph) * np.sin(yy * 0.21 - ph * 0.7)
        rgb[wave > 0.72] = (0.55, 0.80, 0.90)
        put(i, rgb)
    # 2: sand; 14: sand with shells.
    sand = base((0.92, 0.82, 0.58), 2)
    put(2, sand)
    shells = sand.copy()
    for cx, cy in ((18, 20), (44, 40), (30, 50)):
        m = ((xx - cx) ** 2 + (yy - cy) ** 2) < 16
        shells[m] = (0.98, 0.88, 0.86)
        shells[m & (np.abs(xx - cx) < 1)] = (0.8, 0.6, 0.6)
    put(14, shells)
    # 3: grass; 4 flowers; 5 a tall tuft; 6 pebbles; 12 dark grass.
    grass = base((0.36, 0.64, 0.28), 3, 0.1)
    put(3, grass)
    g = grass.copy()
    for cx, cy, col in ((16, 18, (1.0, 0.95, 0.4)), (44, 30, (1.0, 0.6, 0.75)), (26, 46, (1.0, 1.0, 1.0))):
        for k in range(5):
            a = k * 2 * np.pi / 5
            g[((xx - cx - 3 * np.cos(a)) ** 2 + (yy - cy - 3 * np.sin(a)) ** 2) < 5] = col
        g[((xx - cx) ** 2 + (yy - cy) ** 2) < 3] = (0.95, 0.75, 0.2)
    put(4, g)
    g = grass.copy()
    for k in range(9):
        x0 = 20 + k * 3
        g[(np.abs(xx - x0 - (40 - yy) * (k - 4) * 0.06) < 1.2) & (yy > 20) & (yy < 44)] = (0.22, 0.46, 0.18)
    put(5, g)
    g = grass.copy()
    for cx, cy in ((20, 40), (40, 20), (46, 46)):
        g[((xx - cx) ** 2 / 36 + (yy - cy) ** 2 / 20) < 1] = (0.62, 0.6, 0.56)
    put(6, g)
    put(12, base((0.28, 0.54, 0.22), 12, 0.1))
    # 7: lava; 8 with a crust; 15 with a bubble.
    lava = base((0.95, 0.32, 0.06), 7, 0.15)
    lava[noise(17, 6) > 0.62] = (1.0, 0.78, 0.25)
    put(7, lava)
    crust = lava.copy()
    crust[noise(18, 10) > 0.55] = (0.25, 0.12, 0.1)
    put(8, crust)
    bub = lava.copy()
    bub[((xx - 32) ** 2 + (yy - 32) ** 2) < 100] = (1.0, 0.9, 0.5)
    bub[np.abs(np.hypot(xx - 32, yy - 32) - 10) < 1.5] = (0.8, 0.2, 0.05)
    put(15, bub)
    # 9: rock.
    rock = base((0.44, 0.42, 0.40), 9, 0.15)
    rock[noise(19, 12) > 0.6] *= 0.8
    put(9, rock)
    # 10: the chest marker - sand with a red X; 11: the palm marker.
    x = sand.copy()
    x[((np.abs(xx - yy) < 3) | (np.abs(xx + yy - 63) < 3)) & (np.abs(xx - 32) < 18) & (np.abs(yy - 32) < 18)] = (0.75, 0.12, 0.1)
    put(10, x)
    p = sand.copy()
    p[((xx - 32) ** 2 + (yy - 32) ** 2) < 30] = (0.3, 0.6, 0.25)
    put(11, p)
    # 13: a dirt path.
    put(13, base((0.62, 0.46, 0.30), 13, 0.12))
    save_rgba(out / "isle_tiles.png", sheet, np.ones(sheet.shape[:2]))

    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def canvas(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y, np.zeros(x.shape + (3,)), np.zeros(x.shape)

    # The chest, from above and a little to the front.
    x, y, rgb, a = canvas(56, 48)
    body = (x > 4) & (x < 52) & (y > 14) & (y < 44)
    lid = (x > 3) & (x < 53) & (y > 6) & (y < 20)
    rgb[body] = (0.55, 0.32, 0.14)
    rgb[lid] = (0.68, 0.40, 0.18)
    rgb[(body | lid) & ((np.abs(x - 14) < 2) | (np.abs(x - 42) < 2))] = (0.85, 0.7, 0.25)
    rgb[((x - 28) ** 2 + (y - 22) ** 2) < 12] = (0.95, 0.85, 0.35)
    a[body | lid] = 1
    save("chest", rgb, a)
    # A palm from above: fronds round a crown.
    x, y, rgb, a = canvas(150, 150)
    ang = np.arctan2(y - 75, x - 75)
    r = np.hypot(x - 75, y - 75)
    fronds = (r < 70) & (np.abs(np.sin(ang * 3.5)) > 0.35 + r / 140)
    rgb[fronds] = (0.20, 0.50, 0.20)
    rgb[fronds & (np.abs(np.sin(ang * 3.5)) > 0.9)] = (0.30, 0.62, 0.28)
    crown = r < 12
    rgb[crown] = (0.45, 0.32, 0.18)
    for k in range(3):
        rgb[((x - 75 - 9 * np.cos(k * 2.1)) ** 2 + (y - 75 - 9 * np.sin(k * 2.1)) ** 2) < 25] = (0.35, 0.25, 0.12)
    a[fronds | crown] = 1
    save("palm", rgb, a)
    # The explorer from above, facing +x: pith helmet, shoulders, pack.
    x, y, rgb, a = canvas(60, 60)

    def dot(m, col):
        rgb[m] = col
        a[m] = 1

    dot(((x - 22) / 10) ** 2 + ((y - 30) / 13) ** 2 < 1, (0.45, 0.32, 0.18))
    dot(((x - 32) / 10) ** 2 + ((y - 30) / 19) ** 2 < 1, (0.85, 0.72, 0.45))
    dot(((x - 36) / 12) ** 2 + ((y - 30) / 12) ** 2 < 1, (0.95, 0.92, 0.80))
    dot(((x - 36) / 6) ** 2 + ((y - 30) / 6) ** 2 < 1, (0.85, 0.82, 0.70))
    dot(((x - 50) / 4) ** 2 + ((y - 30) / 5) ** 2 < 1, (0.9, 0.72, 0.58))
    save("explorer", rgb, a)

# ── 81 Sandstorm ───────────────────────────────────────────────────────
# A desert in a storm, side on: a dusty sky, two rows of dunes, the rocks
# the sand streams round, and a grain and a funnel of dust for the cloud.

def ss81_sand():
    out = OUT / "sand"
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    # The sky: brown haze above, a pale burning band at the horizon.
    h = 256
    v = np.linspace(0, 1, h)[:, None, None]
    top, low = np.array([0.42, 0.28, 0.18]), np.array([0.98, 0.80, 0.52])
    rgb = np.repeat(top + (low - top) * v ** 1.6, 4, axis=1)
    save_rgba(out / "sky.png", np.clip(rgb, 0, 1), np.ones((h, 4)))

    def dunes(name, w, h, seed, col, crest):
        r = np.random.default_rng(seed)
        x = np.linspace(0, 1, w * SS)
        prof = np.zeros_like(x)
        amp = 1.0
        for o in range(4):
            prof += amp * np.sin(2 * np.pi * (2 ** o * x * 1.5 + r.random()))
            amp *= 0.45
        prof = 0.45 + 0.25 * (prof - prof.min()) / (prof.max() - prof.min())
        yy = np.arange(h * SS)[:, None] / (h * SS)
        a = (yy > 1 - prof[None, :]).astype(np.float64)
        rgb = np.zeros(a.shape + (3,)) + np.array(col)
        # The lit crest, a darker lee below it.
        top = 1 - prof[None, :]
        rgb += np.array(crest) * np.exp(-(yy - top) * h * SS / 10)[..., None] * (yy > top)[..., None]
        rgb *= (1.0 - 0.25 * (yy - top).clip(0, 1))[..., None]
        save(name, np.clip(rgb, 0, 1), a)

    dunes("dunes_far", 1024, 200, 1, (0.78, 0.56, 0.34), (0.12, 0.10, 0.06))
    dunes("dunes_near", 1024, 240, 2, (0.86, 0.62, 0.36), (0.14, 0.12, 0.08))

    def rock(name, w, h, seed):
        r = np.random.default_rng(seed)
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        cx, cy = w / 2, h * 0.55
        ang = np.arctan2(y - cy, x - cx)
        d = np.hypot((x - cx) / (w * 0.48), (y - cy) / (h * 0.46))
        wob = 1 + 0.08 * np.sin(ang * 5 + r.random() * 6) + 0.05 * np.sin(ang * 11 + r.random() * 6)
        m = (d < wob) & (y < h - 2)
        rgb = np.zeros(m.shape + (3,)) + np.array([0.48, 0.30, 0.20])
        # Lit from the top right, strata across.
        lit = np.clip(0.6 + 0.6 * ((x - cx) / w - (y - cy) / h), 0.45, 1.25)
        rgb *= lit[..., None]
        rgb[m & (np.sin(y * 0.35 + x * 0.05) > 0.7)] *= 0.82
        save(name, np.clip(rgb, 0, 1), m.astype(np.float64))

    rock("rock_a", 220, 170, 3)
    rock("rock_b", 160, 140, 4)
    rock("rock_c", 120, 110, 5)
    # A grain: a soft dot, warm.
    w = 16
    y, x = np.mgrid[0:w * SS, 0:w * SS].astype(np.float64) / SS
    d = np.hypot(x - w / 2, y - w / 2) / (w / 2)
    save("grain", np.zeros(d.shape + (3,)) + 1.0, np.clip(1 - d, 0, 1) ** 1.5)
    # The funnel of a whirlwind: a dusty cone, wide at the top.
    w, h = 200, 360
    y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
    u = y / h
    half = 12 + 88 * (1 - u) ** 1.4
    edge = np.clip(1 - np.abs(x - w / 2 - 6 * np.sin(u * 9)) / half, 0, 1)
    bands = 0.75 + 0.25 * np.sin(u * 40 + x * 0.08)
    a = edge ** 1.2 * bands * np.clip(u * 4, 0, 1) * 0.8
    save("funnel", np.zeros(a.shape + (3,)) + np.array([0.9, 0.72, 0.5]), np.clip(a, 0, 1))

# ── 82 Stained Light ───────────────────────────────────────────────────
# A stone pillar for the chapel: a fluted shaft, a base and a capital, pale
# so the light that reaches it - and the light that bounces onto it - shows.

def cp82_chapel():
    out = OUT / "chapel"
    w, h = 76, 580
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    u = (x - w / 2) / (w / 2)
    rgb = np.zeros((h, w, 3)) + np.array([0.80, 0.77, 0.72])
    # Rounded: lighter down the middle, the flutes darker lines.
    rgb *= (0.78 + 0.22 * np.sqrt(np.clip(1 - u * u, 0, 1)))[..., None]
    flutes = (np.abs(np.sin(u * np.pi * 3.5)) < 0.12) & (y > 40) & (y < h - 40)
    rgb[flutes] *= 0.86
    a = np.ones((h, w))
    # The capital and the base: wider bands; the shaft narrower between.
    shaft = (np.abs(u) < 0.78) | (y < 34) | (y > h - 34)
    a[~shaft] = 0.0
    rgb[(y < 34) | (y > h - 34)] = np.array([0.74, 0.71, 0.66])
    rgb[(np.abs(y - 34) < 3) | (np.abs(y - (h - 34)) < 3)] *= 0.8
    rng = np.random.default_rng(82)
    rgb *= (0.95 + 0.08 * rng.random((h, w)))[..., None]
    save_rgba(out / "pillar.png", np.clip(rgb, 0, 1), a)


# ── 83 Siege Workshop ──────────────────────────────────────────────────
# The parts siege engines are built of, each its own sprite so a prefab can
# hang them together: a spoked wheel, beams, a throwing arm and its bucket,
# a stone, a ballista's bow and bolt, a wagon bed, barrels, a pennant and a
# royal banner. And the yard: an afternoon sky, the castle wall, dirt.

def sw83_siege_workshop():
    out = OUT / "siegeworks"
    rng = np.random.default_rng(83)
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def grid(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y

    WOOD = np.array([0.55, 0.36, 0.2])
    IRON = np.array([0.24, 0.24, 0.27])

    def grain(x, y, k=0.0):
        """Wood: long streaks and a little noise."""
        g = 0.88 + 0.1 * np.sin(y * 0.9 + np.sin(x * 0.05 + k) * 3.0) + 0.04 * np.sin(y * 3.1 + x * 0.02)
        return WOOD * g[..., None]

    # A spoked wheel with an iron tyre and a hub.
    x, y = grid(96, 96)
    dx, dy = x - 48, y - 48
    r = np.sqrt(dx * dx + dy * dy)
    ang = np.arctan2(dy, dx)
    rgb, a = np.zeros(x.shape + (3,)), np.zeros(x.shape)
    rim = (r < 47) & (r > 37)
    step = np.pi / 4
    off = (ang / step - np.round(ang / step)) * step
    spokes = (r < 38) & (np.abs(off) * r < 3.2)
    hub = r < 11
    wood = rim | spokes | hub
    rgb[wood] = grain(r * 3, ang * 20)[wood]
    a[wood] = 1.0
    tyre = (r < 47) & (r > 43)
    rgb[tyre] = IRON
    rgb[(r < 6)] = IRON * 1.3
    for k in range(8):
        bx, by = 48 + 40 * np.cos(k * np.pi / 4 + 0.39), 48 + 40 * np.sin(k * np.pi / 4 + 0.39)
        rgb[(x - bx) ** 2 + (y - by) ** 2 < 2.2] = IRON * 0.8
    rgb *= (0.85 + 0.15 * np.clip(-dy / 48, -1, 1))[..., None] * 0.5 + 0.55
    save("wheel", rgb, a)

    def beam(name, w, h, bolts=(), k=0.0, dark=1.0):
        x, y = grid(w, h)
        rgb = grain(x, y, k) * dark
        e = np.minimum(np.minimum(x, w - x), np.minimum(y, h - y))
        rgb *= (0.7 + 0.3 * np.clip(e / 3.0, 0, 1))[..., None]
        for bx, by in bolts:
            rgb[(x - bx) ** 2 + (y - by) ** 2 < 5] = IRON
        a = np.ones(x.shape)
        save(name, rgb, a)

    beam("frame", 220, 40, bolts=[(20, 20), (110, 20), (200, 20)])
    beam("upright", 22, 110, bolts=[(11, 10), (11, 100)], k=1.0)
    # The arm: a beam tapered toward its tip, its pivot at the bottom.
    x, y = grid(22, 170)
    half = 11 - 4 * (1 - y / 170)
    a = (np.abs(x - 11) < half).astype(np.float64)
    rgb = grain(x, y, 2.0)
    rgb[(np.abs(y - 12) < 3)] = IRON
    rgb[(x - 11) ** 2 + (y - 158) ** 2 < 14] = IRON
    save("arm", rgb, a)
    # The bucket: a cup of planks with an iron band.
    x, y = grid(56, 40)
    cup = ((x - 28) / 28) ** 2 + ((y - 4) / 36) ** 2 < 1
    cup &= y > 4
    rgb = grain(x * 2, y, 3.0) * 0.9
    rgb[cup & (np.abs(y - 16) < 2.5)] = IRON
    save("bucket", rgb, cup.astype(np.float64))
    # A stone: grey, lumpy, lit from above.
    x, y = grid(40, 40)
    dx, dy = x - 20, y - 20
    ang = np.arctan2(dy, dx)
    rr = 17 + 1.6 * np.sin(ang * 3 + 1) + 1.0 * np.sin(ang * 5)
    st = np.sqrt(dx * dx + dy * dy) < rr
    rgb = np.zeros(x.shape + (3,)) + np.array([0.6, 0.58, 0.55])
    rgb *= (0.75 + 0.35 * np.clip(-dy / 20, -1, 1))[..., None]
    rgb *= (0.9 + 0.1 * rng.random(x.shape))[..., None]
    save("stone", rgb, st.astype(np.float64))
    # The ballista's bow: an arc of wood and its string, drawn upright.
    x, y = grid(40, 160)
    t = (y - 80) / 80
    bow_x = 30 - 22 * (1 - t * t)
    bow = (np.abs(x - bow_x) < 3.6 - 1.4 * np.abs(t)) & (np.abs(t) < 0.98)
    string = np.abs(x - 32) < 0.8
    rgb = np.zeros(x.shape + (3,))
    rgb[bow] = grain(x, y, 4.0)[bow] * 1.1
    rgb[string & ~bow] = (0.85, 0.82, 0.7)
    a = (bow | (string & (np.abs(t) < 0.98))).astype(np.float64)
    save("bow", rgb, a)
    # The bolt: a shaft, an iron head, three fletches.
    x, y = grid(150, 14)
    shaft = (np.abs(y - 7) < 1.8) & (x < 132)
    head = (x >= 128) & (np.abs(y - 7) < (150 - x) * 0.32)
    fl = (x < 26) & (np.abs(y - 7) < 2 + (26 - x) * 0.18)
    rgb = np.zeros(x.shape + (3,))
    rgb[shaft] = WOOD * 1.2
    rgb[fl] = (0.85, 0.22, 0.18)
    rgb[head] = IRON * 1.4
    save("bolt", rgb, (shaft | head | fl).astype(np.float64))
    beam("stock", 170, 16, bolts=[(14, 8), (156, 8)], k=5.0)
    beam("post", 20, 70, bolts=[(10, 8)], k=6.0, dark=0.9)
    beam("base", 180, 34, bolts=[(18, 17), (90, 17), (162, 17)], k=7.0)
    # The wagon bed: planks across, iron corners.
    x, y = grid(200, 56)
    rgb = grain(y * 4, x * 0.3, 8.0)
    rgb[(y % 14) < 1.2] *= 0.6
    rgb[((x < 10) | (x > 190)) & ((y < 10) | (y > 46))] = IRON
    save("bed", rgb, np.ones(x.shape))
    # A barrel: staves and two hoops.
    x, y = grid(48, 56)
    u = (x - 24) / 24
    bulge = 1 - 0.12 * ((y - 28) / 28) ** 2
    bar = np.abs(u) < bulge
    rgb = np.zeros(x.shape + (3,)) + WOOD * 1.1
    rgb *= (0.7 + 0.35 * np.sqrt(np.clip(1 - (u / bulge) ** 2, 0, 1)))[..., None]
    rgb[bar & ((x % 8) < 0.8)] *= 0.75
    rgb[bar & ((np.abs(y - 12) < 2.2) | (np.abs(y - 44) < 2.2))] = IRON
    save("barrel", rgb, bar.astype(np.float64))
    # A pennant (white, tinted in the scene) and the royal banner: crimson,
    # gold trim and a crown.
    x, y = grid(60, 30)
    pen = (np.abs(y - 15) < 15 * (1 - x / 62))
    rgb = np.ones(x.shape + (3,)) * (0.92 + 0.08 * np.sin(x * 0.3))[..., None]
    save("pennant", rgb, pen.astype(np.float64))
    x, y = grid(50, 80)
    tail = y > 64
    ban = ~tail | (np.abs(x - 25) > (y - 64) * 1.6)
    rgb = np.zeros(x.shape + (3,)) + np.array([0.62, 0.08, 0.1])
    trim = (x < 4) | (x > 46) | (y < 4)
    rgb[trim] = (0.9, 0.72, 0.25)
    crown = (np.abs(x - 25) < 12) & (y > 26) & (y < 40) & ~((y < 33) & (np.abs(((x - 13) % 8) - 4) > 2))
    rgb[crown] = (0.95, 0.78, 0.3)
    save("banner", rgb, ban.astype(np.float64))

    # The yard. A sky from pale gold at the horizon to blue above.
    h = 256
    t = np.linspace(0, 1, h)[:, None, None]
    sky = (1 - t) * np.array([0.32, 0.52, 0.78]) + t * np.array([0.98, 0.86, 0.62])
    save_rgba(out / "sky.png", np.repeat(sky, 4, axis=1), np.ones((h, 4)))
    # The castle wall: tileable stone courses, crenellations on top, a
    # tower every tile.
    w, h = 512, 240
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    top = 40 + np.where((x % 512 > 200) & (x % 512 < 312), -36, 0)
    merlon = ((x % 32) < 18) & (y > top - 18)
    body = y > top
    m = body | merlon
    rgb = np.zeros((h, w, 3)) + np.array([0.62, 0.58, 0.52])
    course = (y % 22) < 1.5
    joint = (((x + (np.floor(y / 22) % 2) * 20) % 40) < 1.5)
    rgb[course | joint] *= 0.78
    rgb *= (0.92 + 0.08 * rng.random((h, w)))[..., None]
    rgb *= (0.8 + 0.2 * (y / h))[..., None]
    slit = (np.abs((x % 512) - 256) < 4) & (y > 70) & (y < 110)
    rgb[slit] = (0.12, 0.1, 0.1)
    save_rgba(out / "castle.png", np.clip(rgb, 0, 1), m.astype(np.float64))
    # Dirt: packed earth with ruts and pebbles.
    w = 256
    fl = rng.random((w, w))
    y, x = np.mgrid[0:w, 0:w].astype(np.float64)
    rgb = np.array([0.5, 0.38, 0.25]) * (0.85 + 0.15 * fl)[..., None]
    rgb *= (0.97 + 0.03 * np.sin(y * 2 * np.pi / 32 + np.sin(x * 2 * np.pi / 128) * 2))[..., None]
    rgb[fl > 0.985] = (0.66, 0.62, 0.56)
    save_rgba(out / "dirt.png", np.clip(rgb, 0, 1), np.ones((w, w)))
    # A plaque, as the zoo's.
    w, h = 300, 46
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.86, 0.78, 0.6])
    rgb[(x < 4) | (x > w - 5) | (y < 4) | (y > h - 5)] = (0.42, 0.28, 0.14)
    save_rgba(out / "plaque.png", rgb, np.ones((h, w)))


# ── 84 Options Menu ────────────────────────────────────────────────────
# A lighthouse on a headland at dusk behind a game's options panel: the sky,
# the sea (tileable), the headland and tower as one silhouette, the panel
# with its gold border, the row highlight, an arrow, and a quill to be the
# cursor.

def om84_options():
    out = OUT / "options"
    rng = np.random.default_rng(84)
    SS = 3

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def grid(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y

    # Dusk: deep violet above, rose, then a band of orange at the horizon.
    h = 256
    t = np.linspace(0, 1, h)[:, None]
    top, mid, low = np.array([0.10, 0.08, 0.24]), np.array([0.55, 0.28, 0.42]), np.array([1.0, 0.62, 0.34])
    sky = np.where(t < 0.6, top + (mid - top) * (t / 0.6), mid + (low - mid) * ((t - 0.6) / 0.4))
    save_rgba(out / "sky.png", np.repeat(sky[:, None, :], 4, axis=1).reshape(h, 4, 3), np.ones((h, 4)))
    # The sea: dark, with long glints that tile across.
    w, h = 256, 128
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    rgb = np.zeros((h, w, 3)) + np.array([0.08, 0.10, 0.22])
    # Glints: short dashes scattered at random, longer and brighter
    # toward the horizon (the top of the tile), wrapping across its edges.
    seed = rng.random((h, w)) < 0.006 * (1.4 - y / h)
    glint = np.zeros((h, w), bool)
    for k in range(-6, 7):
        glint |= np.roll(seed, k, axis=1)
    rgb[glint] = (0.5, 0.36, 0.46)
    rgb[np.roll(seed, 0, axis=1)] = (0.75, 0.52, 0.55)
    save_rgba(out / "sea.png", np.clip(rgb, 0, 1), np.ones((h, w)))
    # The headland and the lighthouse, one silhouette with a lit lamp room.
    x, y = grid(600, 420)
    u = x / 600
    ground = 300 - 120 * np.exp(-((u - 0.32) / 0.28) ** 2) - 30 * np.sin(u * 9) * (1 - u)
    rock = y > ground + 30 * (u > 0.75) * ((u - 0.75) / 0.25) ** 2 * 3
    tower_x, tower_base, tower_top = 190.0, 190.0, 40.0
    half = 22 - 8 * (tower_base - y) / (tower_base - tower_top)
    tower = (np.abs(x - tower_x) < half) & (y > tower_top) & (y < tower_base + 10)
    lamp = (np.abs(x - tower_x) < 17) & (y > tower_top - 24) & (y <= tower_top)
    cap = (np.abs(x - tower_x) < (tower_top - 24 - y + 22) * 0.9) & (y > tower_top - 46) & (y <= tower_top - 24)
    rgb = np.zeros(x.shape + (3,)) + np.array([0.06, 0.05, 0.10])
    a = (rock | tower | lamp | cap).astype(np.float64)
    stripes = tower & ((((y - tower_top) // 26) % 2) == 0)
    rgb[tower] = (0.22, 0.20, 0.28)
    rgb[stripes] = (0.32, 0.12, 0.16)
    rgb[lamp] = (1.0, 0.86, 0.5)
    rgb[lamp & ((np.abs(x - tower_x) % 9) < 1.2)] = (0.25, 0.2, 0.2)
    rgb[cap] = (0.12, 0.10, 0.14)
    # A rim of dusk light on the sea-facing side.
    rim = rock & ~np.roll(rock, -SS * 2, axis=1) & (u > 0.3)
    rgb[rim] = (0.30, 0.18, 0.26)
    save("headland", rgb, a)
    # The panel: dark glass, a gold double border, corner studs.
    W_, H_ = 560, 560
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    e = np.minimum(np.minimum(x, W_ - 1 - x), np.minimum(y, H_ - 1 - y))
    rgb = np.zeros((H_, W_, 3)) + np.array([0.07, 0.07, 0.13])
    rgb *= (1.0 + 0.25 * (1 - y / H_))[..., None]
    gold = np.array([0.86, 0.68, 0.32])
    rgb[(e >= 6) & (e < 9)] = gold
    rgb[(e >= 13) & (e < 14.5)] = gold * 0.7
    a = np.where(e < 6, 0.0, 0.9)
    for cx, cy in ((22, 22), (W_ - 23, 22), (22, H_ - 23), (W_ - 23, H_ - 23)):
        d = np.sqrt((x - cx) ** 2 + (y - cy) ** 2)
        rgb[d < 6] = gold * (1.0 + 0.3 * (d[d < 6] < 3)[:, None])
    save_rgba(out / "panel.png", np.clip(rgb, 0, 1), a)
    # The selected row: gold fading out to the right.
    w, h = 520, 52
    y, x = np.mgrid[0:h, 0:w].astype(np.float64)
    a = np.clip(1.2 - x / w, 0, 1) * 0.35 * np.clip(1 - np.abs(y - h / 2) / (h / 2), 0, 1) ** 0.5
    a[(x < 4)] = 0.9
    save_rgba(out / "row.png", np.zeros((h, w, 3)) + gold, a)
    # A chevron, pointing right.
    x, y = grid(24, 32)
    chev = (np.abs(np.abs(y - 16) - (x - 4) * 0.95) < 2.6) & (x > 3) & (x < 21)
    save("arrow", np.ones(x.shape + (3,)), chev.astype(np.float64))
    # The quill: a feather from top right to a nib at the bottom left (its hot spot).
    x, y = grid(32, 32)
    # Along the shaft from the nib (2, 30) to the tip (30, 2).
    sx, sy = x - 2, 30 - y
    along = (sx + sy) / np.sqrt(2)
    across = (sx - sy) / np.sqrt(2)
    vane = (along > 8) & (along < 39) & (np.abs(across + 1.5) < 5.5 * np.sin(np.clip((along - 8) / 31, 0, 1) * np.pi))
    shaft = (along > 0) & (along < 40) & (np.abs(across) < 0.9)
    nib = (along >= 0) & (along < 7) & (np.abs(across) < (along * 0.35 + 0.4))
    rgb = np.zeros(x.shape + (3,))
    rgb[vane] = (0.96, 0.94, 0.88)
    rgb[vane & ((along % 3.2) < 0.7)] = (0.78, 0.74, 0.70)
    rgb[shaft] = (0.55, 0.45, 0.35)
    rgb[nib] = (0.1, 0.1, 0.12)
    m = vane | shaft | nib
    rim = np.zeros_like(m)
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1)):
        rim |= np.roll(np.roll(m, dx * SS, axis=1), dy * SS, axis=0)
    rgb[rim & ~m] = (0.05, 0.04, 0.06)
    save("quill", rgb, (m | rim).astype(np.float64))


# ── 85 Guest Book ──────────────────────────────────────────────────────
# An inn's guest book open on a desk by candlelight: the desk (tileable
# wood), the open book in one image (leather, two cream pages curving to the
# spine), a candle and an inkwell.

def gb85_guest_book():
    out = OUT / "guestbook"
    rng = np.random.default_rng(85)
    SS = 2

    def save(name, rgb, a):
        h, w = a.shape
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2)
        img = pm.reshape(h // SS, SS, w // SS, SS, 4).mean(axis=(1, 3))
        save_rgba(out / f"{name}.png", np.clip(img[..., :3] / np.maximum(img[..., 3:], 1e-6), 0, 1), img[..., 3])

    def grid(w, h):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        return x, y

    # The desk: dark walnut, streaked along x, tiling both ways.
    w = 256
    y, x = np.mgrid[0:w, 0:w].astype(np.float64)
    streak = np.sin(y * 2 * np.pi / 64 * 3 + np.sin(x * 2 * np.pi / 256) * 1.5) * 0.5 + 0.5
    rgb = np.array([0.30, 0.18, 0.10]) * (0.75 + 0.3 * streak)[..., None]
    rgb *= (0.92 + 0.08 * rng.random((w, w)))[..., None]
    rgb[(y % 64) < 1.0] *= 0.7
    save_rgba(out / "desk.png", np.clip(rgb, 0, 1), np.ones((w, w)))
    # The open book: a leather cover a little larger than its pages, the
    # pages bowing down into the spine.
    W_, H_ = 1100, 640
    x, y = grid(W_, H_)
    cover = (x > 0) & (x < W_) & (y > 0) & (y < H_)
    rgb = np.zeros(x.shape + (3,)) + np.array([0.36, 0.12, 0.08])
    rgb *= (0.85 + 0.15 * np.sin(x * 0.05) * np.sin(y * 0.07))[..., None]
    a = cover.astype(np.float64)
    m = 16.0
    u = (x - W_ / 2) / (W_ / 2 - m)              # -1 .. 1 across the pages
    sag = 6.0 * (1 - np.abs(u)) ** 3             # the pages dip at the spine
    pages = (np.abs(u) < 1) & (y > m + sag) & (y < H_ - m - sag)
    paper = np.array([0.95, 0.90, 0.78])
    shade = 1.0 - 0.38 * np.exp(-np.abs(x - W_ / 2) / 26.0) - 0.06 * np.abs(u) ** 4
    tone = (0.97 + 0.03 * rng.random(x.shape)) * shade
    rgb[pages] = (paper * tone[..., None])[pages]
    # Page edges at the outer sides: a few stacked lines.
    for k in range(3):
        edge = pages & (np.abs(np.abs(u) - (0.997 - k * 0.004)) < 0.0012)
        rgb[edge] *= 0.86
    rgb[pages & (np.abs(x - W_ / 2) < 1.0)] *= 0.55
    save("book", rgb, a)
    # A candle: a cream stick, a wick, a pool of wax at the top.
    x, y = grid(60, 170)
    stick = (np.abs(x - 30) < 16) & (y > 40) & (y < 168)
    drip = (np.abs(x - 19) < 3) & (y > 40) & (y < 70 + 10 * np.sin(x))
    wick = (np.abs(x - 30) < 1.2) & (y > 30) & (y <= 40)
    flame = ((x - 30) / 7) ** 2 + ((y - 18) / 15) ** 2 < 1
    rgb = np.zeros(x.shape + (3,))
    rgb[stick | drip] = np.array([0.93, 0.88, 0.74]) * (0.8 + 0.25 * (1 - np.abs(x[stick | drip] - 30) / 16))[..., None]
    rgb[wick] = (0.15, 0.1, 0.08)
    core = ((x - 30) / 4) ** 2 + ((y - 21) / 9) ** 2 < 1
    rgb[flame] = (1.0, 0.75, 0.3)
    rgb[core] = (1.0, 0.95, 0.75)
    save("candle", rgb, (stick | drip | wick | flame).astype(np.float64))
    # An inkwell: a squat dark glass pot with a brass collar.
    x, y = grid(90, 90)
    pot = (((x - 45) / 40) ** 2 + ((y - 62) / 26) ** 2 < 1) & (y > 40)
    neck = (np.abs(x - 45) < 15) & (y > 26) & (y <= 42)
    rgb = np.zeros(x.shape + (3,)) + np.array([0.08, 0.09, 0.14])
    rgb[(x - 30) ** 2 + (y - 55) ** 2 < 40] = (0.35, 0.38, 0.5)
    rgb[neck] = (0.72, 0.56, 0.25)
    save("inkwell", rgb, (pot | neck).astype(np.float64))


# ── 86 The Long Corridor ───────────────────────────────────────────────
# Eight halls of one endless corridor, each its own scene and its own wall
# painting (so each streams its own texture in): stone courses, two pillars,
# an arched window onto a sky of its own, banners in the hall's colours.
# A stone floor that tiles, a torch bracket, and the walker.

LC_HALLS = [
    # wall, banner, sky top, sky bottom, motif
    ((0.42, 0.38, 0.34), (0.62, 0.12, 0.12), (0.10, 0.12, 0.30), (0.45, 0.30, 0.42), "stars"),
    ((0.36, 0.40, 0.36), (0.14, 0.40, 0.20), (0.35, 0.55, 0.80), (0.80, 0.85, 0.90), "clouds"),
    ((0.44, 0.36, 0.30), (0.70, 0.52, 0.12), (0.90, 0.50, 0.25), (0.98, 0.80, 0.45), "sun"),
    ((0.33, 0.34, 0.42), (0.20, 0.25, 0.60), (0.04, 0.06, 0.16), (0.12, 0.18, 0.36), "moon"),
    ((0.40, 0.40, 0.40), (0.45, 0.15, 0.50), (0.40, 0.30, 0.60), (0.85, 0.55, 0.60), "birds"),
    ((0.38, 0.33, 0.28), (0.15, 0.45, 0.45), (0.20, 0.45, 0.55), (0.30, 0.60, 0.55), "sea"),
    ((0.45, 0.42, 0.36), (0.55, 0.30, 0.10), (0.55, 0.65, 0.75), (0.85, 0.80, 0.70), "hills"),
    ((0.30, 0.30, 0.34), (0.10, 0.10, 0.10), (0.02, 0.02, 0.05), (0.10, 0.06, 0.14), "aurora"),
]


def lc86_corridor():
    out = OUT / "corridor"
    W_, H_ = 1280, 720
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    for k, (wall, banner, sky_top, sky_bot, motif) in enumerate(LC_HALLS):
        rng = np.random.default_rng(860 + k)
        rgb = np.zeros((H_, W_, 3)) + np.array(wall)
        # Stone courses, staggered, each block its own shade.
        row = np.floor(y / 48)
        col = np.floor((x + (row % 2) * 60) / 120)
        shade = rng.random((int(row.max()) + 2, int(col.max()) + 3))
        rgb *= (0.85 + 0.25 * shade[row.astype(int), col.astype(int) + 1])[..., None]
        joint = ((y % 48) < 2.5) | (((x + (row % 2) * 60) % 120) < 2.5)
        rgb[joint] *= 0.62
        rgb *= (0.92 + 0.08 * rng.random((H_, W_)))[..., None]
        # The window: an arch onto this hall's sky.
        cx, top, bot, half = W_ / 2, 150.0, 470.0, 150.0
        in_arch = (np.abs(x - cx) < half) & (y < bot) & ((y > top + half) | ((x - cx) ** 2 + (y - top - half) ** 2 < half ** 2))
        t = np.clip((y - top) / (bot - top), 0, 1)[..., None]
        sky = (1 - t) * np.array(sky_top) + t * np.array(sky_bot)
        sx, sy = x - cx, y - top
        if motif == "stars":
            stars = rng.random((H_, W_)) > 0.996
            sky[stars] = (1.0, 0.95, 0.85)
        elif motif == "clouds":
            c = np.sin(sx * 0.03 + np.sin(sy * 0.05) * 2) * np.sin(sy * 0.08) > 0.55
            sky[c] = sky[c] * 0.3 + 0.7
        elif motif == "sun":
            sky[(sx ** 2 + (sy - 220) ** 2) < 60 ** 2] = (1.0, 0.95, 0.7)
        elif motif == "moon":
            m = (sx + 40) ** 2 + (sy - 110) ** 2 < 42 ** 2
            m &= ~((sx + 22) ** 2 + (sy - 100) ** 2 < 38 ** 2)
            sky[m] = (0.95, 0.95, 0.85)
            sky[rng.random((H_, W_)) > 0.998] = (0.9, 0.9, 1.0)
        elif motif == "birds":
            for bx, by in ((-60, 120), (10, 90), (60, 140), (-20, 170)):
                v = np.abs(np.abs(sx - bx) * 0.5 - (by - sy)) < 1.6
                sky[v & (np.abs(sx - bx) < 14)] = (0.15, 0.1, 0.15)
        elif motif == "sea":
            sea = sy > 200 + 6 * np.sin(sx * 0.08)
            sky[sea] = np.array((0.08, 0.25, 0.35)) * (1 + 0.3 * np.sin(sy[sea] * 0.6))[..., None]
        elif motif == "hills":
            h1 = sy > 230 - 40 * np.sin(sx * 0.012 + 1)
            sky[h1] = (0.30, 0.45, 0.25)
            h2 = sy > 270 - 25 * np.sin(sx * 0.02)
            sky[h2] = (0.22, 0.36, 0.18)
        elif motif == "aurora":
            band = np.exp(-((sy - 120 - 30 * np.sin(sx * 0.03)) / 25) ** 2)
            sky = sky + band[..., None] * np.array((0.1, 0.6, 0.35))
        rgb[in_arch] = sky[in_arch]
        # The arch's stone frame.
        d = np.sqrt((x - cx) ** 2 + (y - top - half) ** 2)
        frame = ((d > half) & (d < half + 18) & (y < top + half)) | ((np.abs(np.abs(x - cx) - half - 9) < 9) & (y >= top + half) & (y < bot + 10))
        frame |= (np.abs(x - cx) < half + 18) & (y >= bot) & (y < bot + 16)
        rgb[frame] = np.array(wall) * 1.25
        # Pillars at the sides.
        for px in (190, W_ - 190):
            p = np.abs(x - px) < 46
            rgb[p] = np.array(wall) * (1.05 + 0.25 * np.cos((x[p] - px) / 46 * 1.4))[..., None]
            rgb[p & ((y % 160) < 6)] *= 0.7
        # Banners, in the hall's colour, between window and pillars.
        for bx in (380, W_ - 380):
            b = (np.abs(x - bx) < 42) & (y > 120) & (y < 420 - 30 * (1 - np.abs(x - bx) / 42))
            rgb[b] = np.array(banner) * (0.85 + 0.2 * np.cos((x[b] - bx) / 42 * 1.5))[..., None]
            rgb[b & (np.abs(y - 140) < 4)] = (0.85, 0.7, 0.3)
        save_rgba(out / f"hall_{k}.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    # Floor: flagstones that tile.
    w = 256
    y, x = np.mgrid[0:w, 0:w].astype(np.float64)
    rng = np.random.default_rng(86)
    rgb = np.zeros((w, w, 3)) + np.array((0.32, 0.29, 0.26))
    rgb *= (0.85 + 0.15 * rng.random((w, w)))[..., None]
    rgb[((x % 128) < 3) | ((y % 64) < 3)] *= 0.55
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((w, w)))
    # A torch bracket: an iron arm and a cup.
    SS = 3
    y, x = np.mgrid[0:96 * SS, 0:48 * SS].astype(np.float64) / SS
    arm = (np.abs(x - 24) < 3) & (y > 30) & (y < 92)
    cup = (np.abs(x - 24) < 12 - (y - 18) * 0.3) & (y > 18) & (y < 34)
    plate = (np.abs(x - 24) < 9) & (y > 78) & (y < 94)
    m = arm | cup | plate
    rgb = np.zeros(x.shape + (3,)) + np.array((0.16, 0.15, 0.16))
    rgb[cup] = (0.30, 0.24, 0.16)
    a = m.astype(np.float64)
    pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(96, SS, 48, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "bracket.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])
    # The walker: a hooded traveller with a staff, facing right.
    y, x = np.mgrid[0:160 * SS, 0:96 * SS].astype(np.float64) / SS
    robe = (np.abs(x - 46) < 10 + (y - 40) * 0.24) & (y > 40) & (y < 156)
    hood = ((x - 48) / 17) ** 2 + ((y - 34) / 20) ** 2 < 1
    face = ((x - 55) / 6) ** 2 + ((y - 37) / 8) ** 2 < 1
    staff = (np.abs(x - 78 + (y - 10) * 0.05) < 2.2) & (y > 10) & (y < 158)
    pack = ((x - 30) / 11) ** 2 + ((y - 70) / 16) ** 2 < 1
    rgb = np.zeros(x.shape + (3,))
    rgb[robe | hood] = (0.30, 0.22, 0.34)
    rgb[robe & (y > 140)] = (0.22, 0.16, 0.26)
    rgb[pack] = (0.42, 0.30, 0.18)
    rgb[face] = (0.12, 0.09, 0.10)
    rgb[staff] = (0.48, 0.34, 0.20)
    m = robe | hood | staff | pack
    a = m.astype(np.float64)
    pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(160, SS, 96, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "walker.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])


# ── 87 Button Prompts ──────────────────────────────────────────────────
# A dojo at dusk: a shoji wall lit from behind, plank floor, a monk and a
# straw dummy, a parchment card for the tutorial - and a glyph per button
# per family, each with its label baked in, which the scene's sprite_library
# holds under the glyph ids `Input.get_prompt` returns ("xbox/a",
# "playstation/cross", "switch/b", "keyboard/space").

BP_GLYPHS = {
    # glyph id: (shape, fill, mark colour, mark) - mark is a letter, or a
    # PlayStation symbol by name.
    "xbox/a": ("circle", (0.12, 0.12, 0.13), (0.42, 0.78, 0.27), "A"),
    "xbox/x": ("circle", (0.12, 0.12, 0.13), (0.25, 0.55, 0.98), "X"),
    "xbox/rb": ("pill", (0.12, 0.12, 0.13), (0.92, 0.92, 0.92), "RB"),
    "playstation/cross": ("circle", (0.10, 0.10, 0.14), (0.50, 0.66, 0.98), "@cross"),
    "playstation/square": ("circle", (0.10, 0.10, 0.14), (0.93, 0.55, 0.78), "@square"),
    "playstation/r1": ("pill", (0.10, 0.10, 0.14), (0.92, 0.92, 0.95), "R1"),
    "switch/b": ("circle", (0.22, 0.22, 0.23), (0.97, 0.97, 0.97), "B"),
    "switch/y": ("circle", (0.22, 0.22, 0.23), (0.97, 0.97, 0.97), "Y"),
    "switch/r": ("pill", (0.22, 0.22, 0.23), (0.97, 0.97, 0.97), "R"),
    "generic/a": ("circle", (0.34, 0.35, 0.38), (0.97, 0.97, 0.97), "A"),
    "generic/x": ("circle", (0.34, 0.35, 0.38), (0.97, 0.97, 0.97), "X"),
    "generic/rb": ("pill", (0.34, 0.35, 0.38), (0.97, 0.97, 0.97), "RB"),
    "keyboard/space": ("wide", (0.90, 0.89, 0.86), (0.15, 0.14, 0.13), "Space"),
    "keyboard/f": ("key", (0.90, 0.89, 0.86), (0.15, 0.14, 0.13), "F"),
    "keyboard/d": ("key", (0.90, 0.89, 0.86), (0.15, 0.14, 0.13), "D"),
}


def bp_glyph_file(glyph):
    return glyph.replace("/", "_") + ".png"


def bp87_prompts():
    from PIL import ImageDraw, ImageFont
    out = OUT / "prompts"
    out.mkdir(parents=True, exist_ok=True)
    font_path = Path(__file__).resolve().parents[3] / "crates" / "jengine-core" / "assets" / "fonts" / "NotoSans-Regular.ttf"
    SS = 4
    c255 = lambda c: tuple(int(round(v * 255)) for v in c)

    for glyph, (shape, fill, mark, label) in BP_GLYPHS.items():
        w, h = (128, 64) if shape in ("wide", "pill") else (64, 64)
        img = Image.new("RGBA", (w * SS, h * SS), (0, 0, 0, 0))
        d = ImageDraw.Draw(img)
        W_, H_ = w * SS, h * SS
        m = 3 * SS
        if shape == "circle":
            d.ellipse((m, m + 2 * SS, W_ - m, H_ - m), fill=c255(tuple(v * 0.55 for v in fill)) + (255,))
            d.ellipse((m, m, W_ - m, H_ - m - 2 * SS), fill=c255(fill) + (255,))
            d.ellipse((m + 3 * SS, m + 3 * SS, W_ - m - 3 * SS, H_ - m - 5 * SS), outline=c255(tuple(min(1, v + 0.12) for v in fill)) + (255,), width=SS)
        elif shape == "pill":
            r = (H_ - 2 * m) // 2
            d.rounded_rectangle((m, m + 2 * SS, W_ - m, H_ - m), r, fill=c255(tuple(v * 0.55 for v in fill)) + (255,))
            d.rounded_rectangle((m, m, W_ - m, H_ - m - 2 * SS), r, fill=c255(fill) + (255,))
        else:  # key / wide: a keycap
            r = 10 * SS
            d.rounded_rectangle((m, m, W_ - m, H_ - m), r, fill=c255(tuple(v * 0.62 for v in fill)) + (255,))
            d.rounded_rectangle((m + 4 * SS, m + 2 * SS, W_ - m - 4 * SS, H_ - m - 8 * SS), r - 2 * SS, fill=c255(fill) + (255,))
        cx, cy = W_ / 2, (H_ - 2 * SS) / 2 - (3 * SS if shape in ("key", "wide") else 0)
        col = c255(mark) + (255,)
        if label == "@cross":
            k, t = 13 * SS, 5 * SS
            d.line((cx - k, cy - k, cx + k, cy + k), fill=col, width=t)
            d.line((cx - k, cy + k, cx + k, cy - k), fill=col, width=t)
        elif label == "@square":
            k = 12 * SS
            d.rectangle((cx - k, cy - k, cx + k, cy + k), outline=col, width=4 * SS)
        else:
            size = {"circle": 34, "pill": 30, "key": 32, "wide": 26}[shape] * SS
            font = ImageFont.truetype(str(font_path), size)
            d.text((cx, cy), label, font=font, fill=col, anchor="mm", stroke_width=SS, stroke_fill=col)
        img = img.resize((w, h), Image.LANCZOS)
        img.save(out / bp_glyph_file(glyph))

    # The wall: shoji panes lit warm from behind, dark frames, a beam.
    W_, H_ = 1280, 720
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rng = np.random.default_rng(87)
    paper = np.array((1.0, 0.80, 0.52)) * (0.78 + 0.22 * np.exp(-((x - 640) / 520) ** 2 - ((y - 330) / 300) ** 2))[..., None]
    paper *= (0.96 + 0.04 * rng.random((H_, W_)))[..., None]
    rgb = paper.copy()
    lattice = ((x % 160) < 9) | ((y % 120) < 9)
    rgb[lattice] = (0.16, 0.10, 0.07)
    posts = (np.abs(x % 640 - 0) < 26) | (np.abs(x % 640 - 640) < 26)
    rgb[posts] = np.array((0.22, 0.14, 0.09)) * (0.9 + 0.2 * np.cos((x[posts] % 640) / 26))[..., None]
    beam = y < 70
    rgb[beam] = np.array((0.20, 0.13, 0.08)) * (0.85 + 0.15 * rng.random((int(beam.sum()),)))[..., None]
    rgb[(y > 58) & (y < 70)] *= 0.6
    # Two hanging scrolls between the posts.
    for sx in (320, 960):
        sc = (np.abs(x - sx) < 60) & (y > 110) & (y < 420)
        rgb[sc] = (0.93, 0.89, 0.78)
        strokes = sc & (np.abs(x - sx - 6 * np.sin(y / 23)) < 7) & (y > 150) & (y < 380) & ((y // 38) % 2 == 0)
        rgb[strokes] = (0.12, 0.10, 0.10)
        rgb[(np.abs(x - sx) < 66) & ((np.abs(y - 108) < 6) | (np.abs(y - 422) < 6))] = (0.30, 0.18, 0.10)
    save_rgba(out / "wall.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))

    # Floor planks that tile.
    w = 256
    y, x = np.mgrid[0:128, 0:w].astype(np.float64)
    rgb = np.zeros((128, w, 3)) + np.array((0.42, 0.27, 0.15))
    rgb *= (0.88 + 0.12 * np.sin(x * 0.11 + np.sin(y * 0.3) * 2))[..., None]
    rgb[(y % 32) < 2] *= 0.5
    rgb[((x + (y // 32) * 90) % 256) < 2] *= 0.6
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((128, w)))

    # The monk, facing right: a saffron robe, a shaved head, a sash.
    y, x = np.mgrid[0:128 * SS, 0:96 * SS].astype(np.float64) / SS
    robe = (np.abs(x - 46) < 12 + (y - 50) * 0.28) & (y > 46) & (y < 124)
    head = ((x - 50) / 15) ** 2 + ((y - 32) / 16) ** 2 < 1
    arm = (np.abs(x - 60 - (y - 60) * 0.3) < 6) & (y > 56) & (y < 92)
    sash = robe & (np.abs(y - 78) < 4)
    feet = ((x - 40) / 10) ** 2 + ((y - 124) / 4) ** 2 < 1
    rgb = np.zeros(x.shape + (3,))
    rgb[robe | arm] = (0.95, 0.55, 0.12)
    rgb[robe & (x < 40)] *= 0.82
    rgb[sash] = (0.55, 0.12, 0.10)
    rgb[head] = (0.88, 0.68, 0.52)
    rgb[head & (x > 58) & (np.abs(y - 30) < 2)] = (0.15, 0.1, 0.08)
    rgb[feet] = (0.30, 0.20, 0.12)
    m = robe | head | arm | feet
    a = m.astype(np.float64)
    pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(128, SS, 96, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "monk.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])

    # A straw dummy on a post.
    y, x = np.mgrid[0:192 * SS, 0:96 * SS].astype(np.float64) / SS
    post = (np.abs(x - 48) < 5) & (y > 60) & (y < 190)
    body = ((x - 48) / 26) ** 2 + ((y - 92) / 40) ** 2 < 1
    head = ((x - 48) / 17) ** 2 + ((y - 36) / 18) ** 2 < 1
    arms = (np.abs(y - 78) < 6) & (np.abs(x - 48) < 46)
    base = (np.abs(x - 48) < 30) & (y > 182) & (y < 192)
    rgb = np.zeros(x.shape + (3,))
    straw = np.array((0.80, 0.66, 0.34)) * (0.8 + 0.2 * np.sin(x * 1.7 + y * 0.4))[..., None]
    rgb[:] = straw
    rgb[post | base] = (0.35, 0.22, 0.12)
    rgb[(body | head) & ((np.abs(y - 70) < 2) | (np.abs(y - 112) < 2) | (np.abs(y - 48) < 2))] = (0.45, 0.25, 0.12)
    m = post | body | head | arms | base
    a = m.astype(np.float64)
    pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(192, SS, 96, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "dummy.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])

    # The tutorial card: parchment with a dark rim.
    cw, ch = 640, 360
    y, x = np.mgrid[0:ch, 0:cw].astype(np.float64)
    r = 22.0
    dx = np.maximum(np.maximum(r - x, x - (cw - 1 - r)), 0)
    dy = np.maximum(np.maximum(r - y, y - (ch - 1 - r)), 0)
    inside = np.sqrt(dx ** 2 + dy ** 2)
    alpha = np.clip(r - inside + 0.5, 0, 1)
    rgb = np.zeros((ch, cw, 3)) + np.array((0.93, 0.86, 0.70))
    rgb *= (0.93 + 0.07 * rng.random((ch, cw)))[..., None]
    edge = np.minimum(np.minimum(x, cw - 1 - x), np.minimum(y, ch - 1 - y))
    rgb *= (0.75 + 0.25 * np.clip(edge / 26, 0, 1))[..., None]
    rim = (inside > r - 5)
    rgb[rim] = (0.30, 0.18, 0.10)
    save_rgba(out / "card.png", np.clip(rgb, 0, 1), alpha)
    # A white block for the charge meter.
    save_rgba(out / "bar.png", np.ones((8, 8, 3)), np.ones((8, 8)))


# ── 88 Night Drive ─────────────────────────────────────────────────────
# A synthwave coast road at night: a striped sun on a magenta horizon,
# mountains and a lit skyline that tile sideways (so one sprite each can
# scroll forever), palms, a road with neon edges, and a wedge of a car.

def nd88_drive():
    out = OUT / "drive"
    out.mkdir(parents=True, exist_ok=True)
    SS = 3
    rng = np.random.default_rng(88)

    # The sky, with the sun and the dark land below the horizon.
    W_, H_ = 1280, 720
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    HZ = 470.0
    t = np.clip(y / HZ, 0, 1)[..., None]
    sky = (1 - t) * np.array((0.03, 0.02, 0.10)) + t * np.array((0.55, 0.12, 0.42))
    sky += np.exp(-((y - HZ) / 60) ** 2)[..., None] * np.array((0.45, 0.18, 0.10))
    stars = (rng.random((H_, W_)) > 0.9985) & (y < 300)
    sky[stars] = (0.9, 0.85, 1.0)
    # The sun: banded, warm at the top, pink at the foot.
    d = np.sqrt((x - 640) ** 2 + (y - 360) ** 2)
    sun = d < 170
    st = np.clip((y - 190) / 340, 0, 1)[..., None]
    suncol = (1 - st) * np.array((1.0, 0.86, 0.35)) + st * np.array((1.0, 0.25, 0.55))
    gaps = (y > 330) & (((y - 330) % 26) < 4 + (y - 330) / 22)
    paint = sun & ~gaps & (y < HZ)
    sky[paint] = suncol[paint]
    sky += np.exp(-np.maximum(d - 170, 0) / 90)[..., None] * np.array((0.25, 0.08, 0.12)) * (y < HZ)[..., None]
    ground = y >= HZ
    sky[ground] = np.array((0.06, 0.02, 0.10))
    lines = ground & ((((y - HZ) ** 1.3) % 22) < 1.5)
    sky[lines] = (0.55, 0.10, 0.60)
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # Mountains that tile: a ridge from whole sine periods of the width.
    W_, H_ = 1024, 220
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    ph = 2 * np.pi * x / W_
    ridge = 70 + 40 * np.sin(ph * 2 + 1) + 25 * np.sin(ph * 5 + 2) + 12 * np.sin(ph * 11)
    body = y > ridge
    rgb = np.zeros((H_, W_, 3)) + np.array((0.09, 0.03, 0.16))
    rim = body & (y < ridge + 3)
    rgb[rim] = (0.95, 0.30, 0.75)
    rgb[body & ~rim] *= (0.7 + 0.5 * (y[body & ~rim] - 70) / 150)[..., None]
    save_rgba(out / "mountains.png", np.clip(rgb, 0, 1), body.astype(np.float64))

    # A skyline that tiles: towers of random heights, lit windows.
    W_, H_ = 1024, 260
    a = np.zeros((H_, W_))
    rgb = np.zeros((H_, W_, 3))
    xpos = 0
    while xpos < W_:
        w = int(rng.integers(40, 90))
        h = int(rng.integers(70, 240))
        x0, x1 = xpos, min(xpos + w, W_)
        a[H_ - h:, x0:x1] = 1
        rgb[H_ - h:, x0:x1] = (0.06, 0.04, 0.12)
        for wy in range(H_ - h + 10, H_ - 8, 14):
            for wx in range(x0 + 6, x1 - 6, 10):
                if rng.random() < 0.38:
                    col = (1.0, 0.85, 0.45) if rng.random() < 0.7 else (0.35, 0.95, 1.0)
                    rgb[wy:wy + 6, wx:wx + 4] = col
        if rng.random() < 0.3:
            rgb[H_ - h:H_ - h + 3, x0:x1] = (0.95, 0.3, 0.8)
        xpos += w + int(rng.integers(0, 12))
    save_rgba(out / "city.png", rgb, a)

    # A palm, as a silhouette with a magenta rim.
    PW, PH = 160, 300
    y, x = np.mgrid[0:PH * SS, 0:PW * SS].astype(np.float64) / SS
    trunk = np.abs(x - (80 + 18 * np.sin((y - 60) / 150 * 1.6))) < (3 + (y - 60) / 60)
    trunk &= y > 60
    fronds = np.zeros_like(trunk)
    for ang in np.linspace(-2.6, -0.5, 7):
        ca, sa = np.cos(ang), np.sin(ang)
        u = (x - 82) * ca + (y - 62) * -sa
        v = (x - 82) * -sa * -1 + (y - 62) * ca
        bend = 0.004 * u ** 2
        fronds |= (u > 0) & (u < 75) & (np.abs(v - bend) < 7 * (1 - u / 80))
    m = trunk | fronds
    rgb = np.zeros(x.shape + (3,)) + np.array((0.05, 0.02, 0.08))
    a_ = m.astype(np.float64)
    pm = np.concatenate([rgb * a_[..., None], a_[..., None]], axis=2).reshape(PH, SS, PW, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "palm.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])

    # The road, tiling sideways: asphalt, neon edges, dashed centre.
    W_, H_ = 256, 160
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.zeros((H_, W_, 3)) + np.array((0.07, 0.05, 0.10))
    rgb *= (0.85 + 0.15 * rng.random((H_, W_)))[..., None]
    rgb[y < 5] = (0.25, 0.95, 1.0)
    rgb[(y >= 5) & (y < 10)] = (0.10, 0.35, 0.45)
    rgb[y > H_ - 6] = (0.95, 0.25, 0.80)
    rgb[(np.abs(y - 80) < 3) & ((x % 256) < 128)] = (1.0, 0.85, 0.40)
    save_rgba(out / "road.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))

    # The car: a low wedge, magenta with a cyan stripe, facing right.
    CW, CH = 320, 110
    y, x = np.mgrid[0:CH * SS, 0:CW * SS].astype(np.float64) / SS
    body = (y > 48) & (y < 88) & (x > 8) & (x < 312) & (y > 48 + (x - 250) * 0.35 * (x > 250))
    cabin = (y > 22) & (y <= 50) & (x > 95 + (50 - y) * 1.6) & (x < 225 - (50 - y) * 0.9)
    glass = cabin & (y > 27) & (x > 100 + (50 - y) * 1.6) & (x < 218 - (50 - y) * 0.9) & (np.abs(x - 165) > 3)
    stripe = body & (np.abs(y - 66) < 2.5)
    tail = body & (x < 22) & (y > 52) & (y < 64)
    head = body & (x > 296) & (y > 60) & (y < 70)
    arches = (((x - 75) ** 2 + (y - 88) ** 2) < 30 ** 2) | (((x - 250) ** 2 + (y - 88) ** 2) < 30 ** 2)
    m = (body | cabin) & ~arches
    rgb = np.zeros(x.shape + (3,))
    rgb[body] = (0.75, 0.10, 0.55)
    rgb[body & (y > 76)] = (0.40, 0.05, 0.32)
    rgb[cabin] = (0.55, 0.08, 0.42)
    rgb[glass] = (0.15, 0.55, 0.85)
    rgb[stripe] = (0.30, 0.95, 1.0)
    rgb[tail] = (1.0, 0.15, 0.20)
    rgb[head] = (1.0, 0.95, 0.80)
    a_ = m.astype(np.float64)
    pm = np.concatenate([rgb * a_[..., None], a_[..., None]], axis=2).reshape(CH, SS, CW, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "car.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])

    # A wheel with five spokes, so its turn shows.
    R = 64
    y, x = np.mgrid[0:R * SS, 0:R * SS].astype(np.float64) / SS
    dx, dy = x - R / 2, y - R / 2
    rr = np.sqrt(dx ** 2 + dy ** 2)
    ang = np.arctan2(dy, dx)
    tyre = rr < 30
    rim = rr < 17
    spokes = rim & (np.abs(((ang * 5 / (2 * np.pi)) % 1) - 0.5) > 0.32)
    rgb = np.zeros(x.shape + (3,)) + np.array((0.05, 0.05, 0.06))
    rgb[rim] = (0.70, 0.72, 0.78)
    rgb[spokes] = (0.10, 0.10, 0.12)
    rgb[rr < 4] = (0.9, 0.9, 0.95)
    a_ = tyre.astype(np.float64)
    pm = np.concatenate([rgb * a_[..., None], a_[..., None]], axis=2).reshape(R, SS, R, SS, 4).mean(axis=(1, 3))
    save_rgba(out / "wheel.png", np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3])

    # A white block for the meters.
    save_rgba(out / "bar.png", np.ones((8, 8, 3)), np.ones((8, 8)))


# ── 89 Archer's Range ──────────────────────────────────────────────────
# A long practice range at golden hour, seen from the side: a sky and far
# hills, a meadow strip and a turf path that tile sideways (they scroll past
# the walking archer), straw targets on posts, and the archer in parts - legs,
# a tunic, a hooded head, a bow arm, a drawing arm in two pieces, the bow and
# its arrow - for a rig whose upper body is a layer.

def aq89_range():
    out = OUT / "range"
    out.mkdir(parents=True, exist_ok=True)
    SS = 3
    rng = np.random.default_rng(89)

    def parts(w, h, paint):
        y, x = np.mgrid[0:h * SS, 0:w * SS].astype(np.float64) / SS
        rgb, m = paint(x, y)
        a = m.astype(np.float64)
        pm = np.concatenate([rgb * a[..., None], a[..., None]], axis=2).reshape(h, SS, w, SS, 4).mean(axis=(1, 3))
        return np.clip(pm[..., :3] / np.maximum(pm[..., 3:], 1e-6), 0, 1), pm[..., 3]

    # Sky: warm at the horizon, hills in two blue-green layers.
    W_, H_ = 1280, 720
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = np.clip(y / 470, 0, 1)[..., None]
    sky = (1 - t) * np.array((0.42, 0.60, 0.85)) + t * np.array((1.0, 0.80, 0.55))
    sun = np.exp(-(((x - 980) / 70) ** 2 + ((y - 300) / 70) ** 2))
    sky += sun[..., None] * np.array((0.6, 0.45, 0.2))
    for k, (base, amp, col) in enumerate(((380, 40, (0.45, 0.55, 0.62)), (430, 30, (0.36, 0.50, 0.40)))):
        ridge = base - amp * np.sin(x / 210 + k * 2) - amp * 0.5 * np.sin(x / 77 + k)
        sky[y > ridge] = col
    sky[y > 470] = (0.40, 0.55, 0.28)
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # Meadow strip that tiles: long grass with flowers.
    W_, H_ = 512, 120
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.zeros((H_, W_, 3)) + np.array((0.34, 0.52, 0.22))
    rgb *= (0.8 + 0.25 * np.sin(x * 2 * np.pi / W_ * 37 + np.sin(y * 0.3)) * 0.5 + 0.2 * rng.random((H_, W_)))[..., None]
    blades = (np.abs(((x * 7) % 11) - 5) < 1) & (y < 30 + 20 * np.sin(x * 2 * np.pi / W_ * 13))
    a = np.where((y > 20 + 12 * np.sin(x * 2 * np.pi / W_ * 9)) | blades, 1.0, 0.0)
    for _ in range(60):
        fx, fy = rng.integers(0, W_), rng.integers(30, H_ - 10)
        rgb[max(0, fy - 2):fy + 2, max(0, fx - 2):fx + 2] = (1.0, 0.9, 0.4) if rng.random() < 0.5 else (0.95, 0.5, 0.6)
    save_rgba(out / "meadow.png", np.clip(rgb, 0, 1), a)

    # The path: packed earth that tiles, a grass lip on top.
    W_, H_ = 256, 160
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.zeros((H_, W_, 3)) + np.array((0.55, 0.42, 0.28))
    rgb *= (0.85 + 0.15 * rng.random((H_, W_)))[..., None]
    rgb[(y < 14 + 4 * np.sin(x * 2 * np.pi / W_ * 6))] = (0.38, 0.58, 0.24)
    pebbles = rng.random((H_, W_)) > 0.995
    rgb[pebbles] = (0.75, 0.70, 0.62)
    save_rgba(out / "path.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))

    # A straw target on its post: rings painted on a bale face.
    def target(x, y):
        d = np.sqrt((x - 50) ** 2 + (y - 50) ** 2)
        face = d < 46
        post = (np.abs(x - 50) < 6) & (y > 80) & (y < 200)
        rgb = np.zeros(x.shape + (3,)) + np.array((0.45, 0.30, 0.16))
        ring = np.floor(d / 9)
        cols = [(1.0, 0.85, 0.2), (1.0, 0.85, 0.2), (0.85, 0.15, 0.12), (0.2, 0.45, 0.85), (0.15, 0.15, 0.18), (0.95, 0.93, 0.85)]
        for i, c in enumerate(cols):
            rgb[face & (ring == i)] = c
        rgb[face & (d > 44)] = (0.80, 0.68, 0.40)
        return rgb, face | post
    rgb, a = parts(100, 200, target)
    save_rgba(out / "target.png", rgb, a)

    # The archer: a green hooded tunic, brown leggings, boots.
    def leg(x, y):
        m = (np.abs(x - 13) < 9 - y * 0.02) & (y < 100)
        rgb = np.zeros(x.shape + (3,)) + np.array((0.35, 0.26, 0.18))
        rgb[y > 80] = (0.22, 0.15, 0.10)
        rgb[x < 10] *= 0.85
        return rgb, m
    save_rgba(out / "leg.png", *parts(26, 100, leg))

    def torso(x, y):
        m = (np.abs(x - 26) < 18 + (96 - y) * 0.06) & (y > 2)
        rgb = np.zeros(x.shape + (3,)) + np.array((0.22, 0.45, 0.25))
        rgb[np.abs(y - 62) < 4] = (0.40, 0.28, 0.16)
        rgb[(np.abs(x - 34) < 4) & (np.abs(y - 62) < 4)] = (0.85, 0.72, 0.3)
        rgb[x < 18] *= 0.82
        return rgb, m
    save_rgba(out / "torso.png", *parts(52, 98, torso))

    def head(x, y):
        hood = ((x - 22) / 20) ** 2 + ((y - 24) / 21) ** 2 < 1
        face = ((x - 29) / 9) ** 2 + ((y - 26) / 11) ** 2 < 1
        rgb = np.zeros(x.shape + (3,)) + np.array((0.18, 0.38, 0.21))
        rgb[face] = (0.90, 0.72, 0.58)
        rgb[face & (np.abs(x - 33) < 1.5) & (np.abs(y - 23) < 1.5)] = (0.15, 0.1, 0.1)
        return rgb, hood
    save_rgba(out / "head.png", *parts(46, 48, head))

    def arm(w, h, sleeve):
        def paint(x, y):
            m = (np.abs(x - w / 2) < w / 2 - 1) & (y < h - 1)
            rgb = np.zeros(x.shape + (3,)) + np.array(sleeve)
            rgb[y > h - 10] = (0.90, 0.72, 0.58)
            return rgb, m
        return paint
    save_rgba(out / "arm_bow.png", *parts(18, 74, arm(18, 74, (0.20, 0.42, 0.23))))
    save_rgba(out / "arm_upper.png", *parts(16, 42, arm(16, 42, (0.20, 0.42, 0.23))))
    save_rgba(out / "arm_fore.png", *parts(14, 40, arm(14, 40, (0.24, 0.48, 0.26))))

    # The bow, drawn vertical: a recurve limb and its string.
    def bow(x, y):
        c = 75
        limb = np.abs(x - (10 + 14 * (1 - ((y - c) / 74) ** 2))) < 3.2
        limb &= (y > 1) & (y < 149)
        string = (np.abs(x - 7) < 0.8) & (y > 3) & (y < 147)
        rgb = np.zeros(x.shape + (3,)) + np.array((0.45, 0.28, 0.14))
        rgb[string] = (0.9, 0.88, 0.8)
        return rgb, limb | string
    save_rgba(out / "bow.png", *parts(30, 150, bow))

    def arrow(x, y):
        shaft = (np.abs(y - 4) < 1.2) & (x > 6) & (x < 90)
        head_ = (x >= 90) & (np.abs(y - 4) < (100 - x) * 0.45)
        fletch = (x < 18) & (np.abs(y - 4) < 4) & (np.abs(y - 4) > 0.8)
        rgb = np.zeros(x.shape + (3,)) + np.array((0.62, 0.48, 0.30))
        rgb[head_] = (0.55, 0.57, 0.62)
        rgb[fletch] = (0.85, 0.2, 0.18)
        return rgb, shaft | head_ | fletch
    save_rgba(out / "arrow.png", *parts(100, 8, arrow))


# ── 90 Fireworks Night ─────────────────────────────────────────────────
# A harbour town under fireworks. Three flipbook sheets the particles play:
# a starburst that blooms and thins over 32 frames (white, tinted by each
# burst's colour), a smoke puff that billows over 16, and a four-frame
# twinkle the glitter flickers through. And the night: sky, town, harbour.

def fwn90_fireworks():
    out = OUT / "fireworks"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(90)

    # The starburst: 8 x 4 frames of 128 px - a ring of rays that flies out,
    # its core flaring first, the rays thinning and breaking up as they go.
    F = 128
    y, x = np.mgrid[0:F, 0:F].astype(np.float64)
    dx, dy = x - F / 2 + 0.5, y - F / 2 + 0.5
    r = np.sqrt(dx ** 2 + dy ** 2) / (F / 2)
    ang = np.arctan2(dy, dx)
    rays = 22
    phase = rng.random(rays) * 0.6
    sheet = np.zeros((F * 4, F * 8, 4))
    for i in range(32):
        t = i / 31
        front = 0.15 + 0.8 * (1 - (1 - t) ** 2)
        width = 0.10 + 0.25 * t
        k = np.floor((ang + np.pi) / (2 * np.pi) * rays).astype(int) % rays
        within = np.abs(((ang + np.pi) / (2 * np.pi) * rays) % 1 - 0.5) < (0.16 - 0.08 * t)
        streak = within & (r < front) & (r > front - width - phase[k] * 0.2)
        a = streak * (1 - t) ** 0.8 * np.clip((r - (front - width)) / width, 0, 1)
        a += np.exp(-(r / (0.12 + 0.25 * t)) ** 2) * max(0.0, 1 - t * 2.5)
        # It breaks up into sparks as it fades.
        a *= np.where(rng.random((F, F)) < 0.15 + 0.7 * t, 0.3, 1.0)
        a = np.clip(a, 0, 1)
        cx, cy = (i % 8) * F, (i // 8) * F
        sheet[cy:cy + F, cx:cx + F, :3] = 1.0
        sheet[cy:cy + F, cx:cx + F, 3] = a
    save_rgba(out / "burst.png", sheet[..., :3], sheet[..., 3])

    # Smoke: 4 x 4 frames of 128 px - lumps of noise that swell and thin.
    blobs = [(rng.random() * 0.5 + 0.25, rng.random() * 0.5 + 0.25, 0.12 + rng.random() * 0.12) for _ in range(9)]
    sheet = np.zeros((F * 4, F * 4, 4))
    u, v = x / F, y / F
    for i in range(16):
        t = i / 15
        a = np.zeros((F, F))
        for bx, by, br in blobs:
            cxp = 0.5 + (bx - 0.5) * (0.5 + 0.9 * t)
            cyp = 0.5 + (by - 0.5) * (0.5 + 0.9 * t) - 0.08 * t
            rr = br * (0.8 + 1.0 * t)
            a = np.maximum(a, np.clip(1 - ((u - cxp) ** 2 + (v - cyp) ** 2) / rr ** 2, 0, 1))
        a *= 0.85 * (1 - t) ** 0.6 * (0.8 + 0.2 * np.sin(u * 23 + v * 17 + i))
        # Round off the frame's edge, and fade in so a burst is seen first.
        a *= np.clip((0.5 - np.sqrt((u - 0.5) ** 2 + (v - 0.5) ** 2)) / 0.15, 0, 1)
        a *= min(1.0, 0.2 + t * 3)
        cx, cy = (i % 4) * F, (i // 4) * F
        sheet[cy:cy + F, cx:cx + F, :3] = 0.85 - 0.25 * v[..., None]
        sheet[cy:cy + F, cx:cx + F, 3] = np.clip(a, 0, 1)
    save_rgba(out / "smoke.png", sheet[..., :3], sheet[..., 3])

    # Twinkle: 4 frames of 32 px - a four-pointed star whose arms grow and
    # shrink.
    S = 32
    y2, x2 = np.mgrid[0:S, 0:S].astype(np.float64)
    ddx, ddy = np.abs(x2 - S / 2 + 0.5), np.abs(y2 - S / 2 + 0.5)
    sheet = np.zeros((S, S * 4, 4))
    for i, arm in enumerate((0.35, 0.8, 1.0, 0.55)):
        cross = np.exp(-(ddx / 1.4) ** 2) * np.clip(1 - ddy / (S / 2 * arm), 0, 1) + \
            np.exp(-(ddy / 1.4) ** 2) * np.clip(1 - ddx / (S / 2 * arm), 0, 1)
        core = np.exp(-((ddx ** 2 + ddy ** 2) / 9))
        sheet[:, i * S:(i + 1) * S, :3] = 1.0
        sheet[:, i * S:(i + 1) * S, 3] = np.clip(cross + core, 0, 1)
    save_rgba(out / "twinkle.png", sheet[..., :3], sheet[..., 3])

    # The night sky: deep blue to a faint glow over the town, and stars.
    W_, H_ = 1280, 720
    yy, xx = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = np.clip(yy / 520, 0, 1)[..., None]
    sky = (1 - t) * np.array((0.01, 0.015, 0.05)) + t * np.array((0.07, 0.06, 0.14))
    stars = rng.random((H_, W_)) > 0.9975
    sky[stars & (yy < 420)] = (0.75, 0.75, 0.9)
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # The town: rooftops, a spire and a lighthouse, lit windows.
    W_, H_ = 1280, 240
    a = np.zeros((H_, W_))
    rgb = np.zeros((H_, W_, 3)) + np.array((0.02, 0.02, 0.05))
    xpos = 0
    while xpos < W_:
        w = int(rng.integers(50, 110))
        h = int(rng.integers(60, 150))
        roof = int(rng.integers(0, 30))
        x0, x1 = xpos, min(W_, xpos + w)
        for xi in range(x0, x1):
            peak = h + roof * (1 - abs((xi - x0) / max(1, w) - 0.5) * 2)
            a[int(H_ - peak):, xi] = 1
        for wy in range(H_ - h + 14, H_ - 10, 18):
            for wx in range(x0 + 8, x1 - 8, 16):
                if rng.random() < 0.35:
                    rgb[wy:wy + 7, wx:wx + 5] = (1.0, 0.78, 0.38)
        xpos += w
    # A spire.
    sx = 820
    spire = (np.abs(np.arange(W_)[None, :] - sx) < (np.arange(H_)[:, None] - 0) * 0.06) & (np.arange(H_)[:, None] > 0)
    a[spire] = 1
    save_rgba(out / "town.png", rgb, a)

    # The harbour: dark water with long ripples.
    W_, H_ = 1280, 160
    yy, xx = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.zeros((H_, W_, 3)) + np.array((0.015, 0.02, 0.05))
    rip = np.sin(xx * 0.05 + np.sin(yy * 0.4) * 3 + yy * 0.7) > 0.93
    rgb[rip] = (0.10, 0.10, 0.18)
    save_rgba(out / "water.png", rgb, np.ones((H_, W_)))


# ── 91 Procedural Isles ────────────────────────────────────────────────
# The islands themselves are the Mapmaker's terrain tileset, filled by the
# scene's script from Noise.*; this draws what drifts over them: a cloud
# puff (white in alpha, so its sprite colour tints it - the same picture
# dark is its shadow) and a soft streak for the wind's trails.

def pri91_isles():
    out = OUT / "isles"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(91)

    # A cumulus puff: a flat-bottomed heap of round lumps, its edge broken by
    # fine noise, shaded a little darker underneath.
    W_, H_ = 320, 192
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    a = np.zeros((H_, W_))
    lumps = [(0.5, 0.52, 0.30), (0.30, 0.60, 0.20), (0.70, 0.60, 0.21), (0.42, 0.40, 0.21),
             (0.60, 0.38, 0.19), (0.18, 0.68, 0.13), (0.83, 0.68, 0.13), (0.52, 0.30, 0.15)]
    u, v = x / W_, y / H_
    for cx, cy, r in lumps:
        d = np.sqrt(((u - cx) * W_ / H_) ** 2 + (v - cy) ** 2) / r
        a = np.maximum(a, np.clip(1.25 - d, 0, 1))
    # A flat base, softened.
    a *= np.clip((0.86 - v) / 0.12, 0, 1)
    grain = fbm(W_, 16, 4)[:H_, :W_]
    a = np.clip((a + (grain - 0.5) * 0.35 - 0.12) * 2.2, 0, 1)
    a = a ** 0.8
    shade = 1.0 - 0.22 * np.clip((v - 0.35) / 0.5, 0, 1)
    rgb = np.ones((H_, W_, 3)) * shade[..., None]
    save_rgba(out / "cloud.png", rgb, a)

    # A streak: soft across its width (a trail stretches it head to tail).
    S = 32
    yy = np.mgrid[0:S, 0:S][0].astype(np.float64)
    across = np.exp(-((yy - S / 2 + 0.5) / (S * 0.22)) ** 2)
    save_rgba(out / "streak.png", np.ones((S, S, 3)), across)


# ── 92 Crane Yard ──────────────────────────────────────────────────────
# A dock at dusk: a sky with a far harbour, the quay, a crane truck (body,
# boom, wheels), its trolley and hook, crates and a container to lift, and
# bollards to bump over. Side view; every moving part is its own sprite.

def cy92_crane():
    out = OUT / "crane"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(92)

    def canvas(w, h):
        return np.zeros((h, w, 3)), np.zeros((h, w))

    def rect(rgb, a, x0, y0, x1, y1, col, alpha=1.0):
        rgb[y0:y1, x0:x1] = col
        a[y0:y1, x0:x1] = np.maximum(a[y0:y1, x0:x1], alpha)

    # The sky: dusk, orange at the horizon to deep blue, a far harbour of
    # cranes and warehouses in silhouette.
    W_, H_ = 1280, 720
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = np.clip(y / 470, 0, 1)[..., None]
    sky = (1 - t) * np.array((0.10, 0.12, 0.28)) + t * np.array((0.95, 0.55, 0.32))
    sky = np.where(t > 0.85, sky * 1.0, sky)
    sil = np.zeros((H_, W_), bool)
    base = 470
    xs = 0
    while xs < W_:
        w = int(rng.integers(40, 120))
        h = int(rng.integers(20, 70))
        sil[base - h:base + 40, xs:xs + w] = True
        xs += w + int(rng.integers(0, 30))
    for cx in (180, 520, 930, 1150):
        sil[base - 190:base, cx:cx + 10] = True               # mast
        sil[base - 190:base - 178, cx - 60:cx + 140] = True   # jib
        sil[base - 178:base - 120, cx + 120:cx + 122] = True  # cable
    sky[sil] = sky[sil] * 0.35 + np.array((0.05, 0.05, 0.10)) * 0.65
    water = y > base + 40
    sky[water] = np.array((0.10, 0.12, 0.22)) + 0.05 * np.sin(x[water] * 0.07 + y[water] * 0.9)[..., None]
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # The quay: concrete with a yellow and black edge, 256 x 128, tiled.
    W_, H_ = 256, 128
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.ones((H_, W_, 3)) * np.array((0.42, 0.42, 0.44))
    rgb *= (0.9 + 0.1 * rng.random((H_, W_)))[..., None]
    stripe = ((x + y) // 16) % 2 == 0
    rgb[:12][stripe[:12]] = (0.95, 0.75, 0.10)
    rgb[:12][~stripe[:12]] = (0.08, 0.08, 0.08)
    rgb[12:16] = (0.25, 0.25, 0.27)
    rgb[:, ::64] *= 0.7
    save_rgba(out / "quay.png", rgb, np.ones((H_, W_)))

    # The truck: a low yellow body, a cab at the right, a black chassis.
    W_, H_ = 260, 70
    rgb, a = canvas(W_, H_)
    rect(rgb, a, 0, 34, 260, 62, (0.10, 0.10, 0.11))
    rect(rgb, a, 4, 14, 190, 46, (0.96, 0.72, 0.12))
    rect(rgb, a, 4, 14, 190, 18, (1.0, 0.85, 0.35))
    rect(rgb, a, 190, 0, 256, 46, (0.96, 0.72, 0.12))
    rect(rgb, a, 200, 6, 248, 26, (0.55, 0.75, 0.90))
    rect(rgb, a, 0, 46, 260, 50, (0.98, 0.25, 0.15))
    for sx in range(14, 180, 28):
        rect(rgb, a, sx, 22, sx + 14, 40, (0.85, 0.62, 0.10))
    save_rgba(out / "truck.png", rgb, a)

    # The boom: a lattice girder, 360 x 30, with a mast stub at its left.
    W_, H_ = 360, 30
    rgb, a = canvas(W_, H_)
    col = (0.98, 0.70, 0.10)
    rect(rgb, a, 0, 0, 360, 5, col)
    rect(rgb, a, 0, 25, 360, 30, col)
    for i in range(0, 360, 24):
        for k in range(24):
            yy = int(5 + k * 20 / 24) if (i // 24) % 2 == 0 else int(25 - k * 20 / 24)
            rect(rgb, a, i + k, yy - 2, i + k + 1, yy + 2, col)
    save_rgba(out / "boom.png", rgb, a)
    # The mast: a lattice tower, 30 x 140.
    W_, H_ = 30, 140
    rgb, a = canvas(W_, H_)
    rect(rgb, a, 0, 0, 5, 140, col)
    rect(rgb, a, 25, 0, 30, 140, col)
    for j in range(0, 140, 20):
        for k in range(20):
            xx = int(5 + k * 20 / 20) if (j // 20) % 2 == 0 else int(25 - k * 20 / 20)
            rect(rgb, a, xx - 2, j + k, xx + 2, j + k + 1, col)
    save_rgba(out / "mast.png", rgb, a)

    # A wheel: tyre, rim, hub and five bolts, so the turning shows.
    S = 96
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    r = np.sqrt((x - S / 2 + 0.5) ** 2 + (y - S / 2 + 0.5) ** 2)
    ang = np.arctan2(y - S / 2, x - S / 2)
    a = np.clip(S / 2 - r, 0, 1)
    rgb = np.zeros((S, S, 3)) + 0.07
    tread = (np.sin(ang * 18) > 0.3) & (r > S / 2 - 6)
    rgb[tread] = 0.12
    rim = r < S * 0.30
    rgb[rim] = (0.70, 0.70, 0.72)
    rgb[r < S * 0.12] = (0.35, 0.35, 0.38)
    for k in range(5):
        bx = S / 2 + np.cos(k * 1.2566) * S * 0.2
        by = S / 2 + np.sin(k * 1.2566) * S * 0.2
        rgb[(x - bx) ** 2 + (y - by) ** 2 < 9] = (0.25, 0.25, 0.27)
    rgb[(np.abs(ang) < 0.12) & (r < S * 0.3) & (r > S * 0.12)] = (0.98, 0.25, 0.15)
    save_rgba(out / "wheel.png", rgb, a)

    # The trolley: a small carriage on four rollers.
    W_, H_ = 56, 28
    rgb, a = canvas(W_, H_)
    rect(rgb, a, 4, 8, 52, 24, (0.22, 0.24, 0.28))
    rect(rgb, a, 4, 8, 52, 11, (0.40, 0.42, 0.48))
    for rx in (8, 20, 32, 44):
        rect(rgb, a, rx, 2, rx + 6, 8, (0.12, 0.12, 0.13))
    save_rgba(out / "trolley.png", rgb, a)

    # The hook: a block with a red and white band and a magnet plate.
    W_, H_ = 64, 56
    rgb, a = canvas(W_, H_)
    rect(rgb, a, 12, 0, 52, 30, (0.20, 0.21, 0.24))
    for i, sx in enumerate(range(12, 52, 8)):
        rect(rgb, a, sx, 12, sx + 8, 20, (0.95, 0.20, 0.15) if i % 2 == 0 else (0.95, 0.95, 0.95))
    rect(rgb, a, 2, 34, 62, 52, (0.30, 0.31, 0.35))
    rect(rgb, a, 2, 48, 62, 56, (0.55, 0.56, 0.60))
    rect(rgb, a, 30, 30, 34, 34, (0.10, 0.10, 0.10))
    save_rgba(out / "hook.png", rgb, a)

    # Crates: a wooden one and a blue container.
    W_, H_ = 64, 64
    rgb = np.ones((H_, W_, 3)) * np.array((0.62, 0.42, 0.22))
    rgb *= (0.85 + 0.15 * rng.random((H_, W_)))[..., None]
    for k in range(0, 64, 16):
        rgb[k:k + 1] *= 0.6
    rgb[:5] = rgb[-5:] = (0.40, 0.26, 0.13)
    rgb[:, :5] = rgb[:, -5:] = (0.40, 0.26, 0.13)
    for k in range(64):
        for d in (-2, -1, 0, 1, 2):
            if 0 <= k + d < 64:
                rgb[k, min(63, max(0, k + d))] = (0.45, 0.30, 0.15)
    save_rgba(out / "crate.png", rgb, np.ones((H_, W_)))
    W_, H_ = 128, 56
    rgb = np.ones((H_, W_, 3)) * np.array((0.15, 0.35, 0.65))
    for k in range(4, 128, 8):
        rgb[:, k:k + 3] *= 0.75
    rgb[:4] = rgb[-4:] = (0.10, 0.22, 0.42)
    rgb[:, :4] = rgb[:, -4:] = (0.10, 0.22, 0.42)
    save_rgba(out / "container.png", rgb, np.ones((H_, W_)))

    # A bollard: a squat iron post with a cap, 40 x 40.
    S = 40
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    r = np.sqrt((x - S / 2 + 0.5) ** 2 + (y - S / 2 + 0.5) ** 2)
    a = np.clip(S / 2 - r, 0, 1)
    rgb = np.zeros((S, S, 3)) + np.array((0.15, 0.16, 0.18))
    rgb[(y < S / 2) & (r < S / 2 - 3)] = (0.22, 0.23, 0.26)
    rgb[r < 5] = (0.35, 0.36, 0.40)
    save_rgba(out / "bollard.png", rgb, a)

    # The cursor: a ring with a cross.
    S = 48
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    r = np.sqrt((x - S / 2 + 0.5) ** 2 + (y - S / 2 + 0.5) ** 2)
    a = np.clip(1.5 - np.abs(r - 18), 0, 1)
    a = np.maximum(a, ((np.abs(x - S / 2 + 0.5) < 1.2) | (np.abs(y - S / 2 + 0.5) < 1.2)) * (r < 9))
    save_rgba(out / "cursor.png", np.ones((S, S, 3)), a)

    # The cable, a soft 8 x 8 strip a line stretches.
    save_rgba(out / "cable.png", np.ones((8, 8, 3)) * 0.12, np.ones((8, 8)))


# ── 93 Ghost Lantern ───────────────────────────────────────────────────
# A haunted picture gallery: damask wallpaper over panelling, a boarded
# floor, framed paintings, candle sconces, ghosts in parts (a body with a
# wavy hem, two arms, eyes - white in alpha so their sprites tint them), and
# the cursed lantern.

def gl93_gallery():
    out = OUT / "gallery"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(93)

    # The wall: deep green damask above, dark panelling below, 512 x 512.
    S = 512
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    rgb = np.ones((S, S, 3)) * np.array((0.20, 0.30, 0.26))
    u, v = (x % 128) / 128 - 0.5, (y % 160) / 160 - 0.5
    motif = (np.abs(u) * 1.6 + np.abs(v) * 1.1 < 0.42) & (np.abs(u) * 1.6 + np.abs(v) * 1.1 > 0.30)
    motif |= (u ** 2 + v ** 2) < 0.006
    rgb[motif] = (0.27, 0.38, 0.32)
    rgb *= (0.93 + 0.07 * rng.random((S, S)))[..., None]
    panel = y > 340
    rgb[panel] = (0.24, 0.15, 0.09)
    rgb[(y > 340) & (y < 350)] = (0.36, 0.24, 0.14)
    for px in range(0, S, 128):
        rgb[(y > 365) & (y < 495) & (np.abs(x - px - 64) < 52) & ((np.abs(x - px - 64) > 47) | (np.abs(y - 430) > 60))] = (0.18, 0.11, 0.06)
    save_rgba(out / "wall.png", rgb, np.ones((S, S)))

    # The floor: boards, 256 x 128.
    W_, H_ = 256, 128
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.ones((H_, W_, 3)) * np.array((0.30, 0.20, 0.12))
    rgb *= (0.88 + 0.12 * np.sin(x * 0.11 + np.sin(y * 0.3) * 2))[..., None]
    rgb[(y.astype(int) % 32) < 2] = (0.14, 0.09, 0.05)
    save_rgba(out / "floor.png", rgb, np.ones((H_, W_)))

    # Three paintings in gilt frames, 160 x 200: a sea, a mountain, a portrait.
    for k in range(3):
        W_, H_ = 160, 200
        y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
        rgb = np.zeros((H_, W_, 3))
        inner = (x > 16) & (x < W_ - 16) & (y > 16) & (y < H_ - 16)
        t = (y / H_)[..., None]
        if k == 0:
            pic = (1 - t) * np.array((0.55, 0.62, 0.70)) + t * np.array((0.15, 0.25, 0.40))
            pic[y > 120] = (0.12, 0.22, 0.35)
        elif k == 1:
            pic = (1 - t) * np.array((0.75, 0.60, 0.45)) + t * np.array((0.40, 0.45, 0.35))
            m = y > 150 - 70 * (1 - np.abs(x - 80) / 70)
            pic[m] = (0.30, 0.28, 0.30)
        else:
            pic = np.ones((H_, W_, 3)) * np.array((0.18, 0.12, 0.10))
            face = ((x - 80) / 26) ** 2 + ((y - 80) / 34) ** 2 < 1
            body = ((x - 80) / 52) ** 2 + ((y - 190) / 70) ** 2 < 1
            pic[body] = (0.10, 0.10, 0.14)
            pic[face] = (0.75, 0.62, 0.50)
        rgb[inner] = pic[inner]
        gilt = ~inner
        rgb[gilt] = np.array((0.70, 0.52, 0.18)) * (0.8 + 0.2 * np.sin((x + y)[gilt] * 0.5))[..., None]
        save_rgba(out / f"painting{k}.png", rgb, np.ones((H_, W_)))

    # A sconce: a brass plate and a candle, 40 x 90.
    W_, H_ = 40, 90
    rgb, a = np.zeros((H_, W_, 3)), np.zeros((H_, W_))
    a[40:90, 16:24] = 1
    rgb[40:90, 16:24] = (0.62, 0.48, 0.20)
    a[20:44, 14:26] = 1
    rgb[20:44, 14:26] = (0.92, 0.88, 0.78)
    a[86:90, 6:34] = 1
    rgb[86:90, 6:34] = (0.62, 0.48, 0.20)
    save_rgba(out / "sconce.png", rgb, a)

    # A ghost's body: a head and a sheet with a wavy hem, 128 x 160.
    W_, H_ = 128, 160
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    head = ((x - 64) / 44) ** 2 + ((y - 48) / 44) ** 2 < 1
    hem = 140 + 10 * np.sin(x / W_ * np.pi * 5)
    sheet = (np.abs(x - 64) < 44 + (y - 48) * 0.12) & (y > 48) & (y < hem)
    a = (head | sheet).astype(float)
    save_rgba(out / "ghost_body.png", np.ones((H_, W_, 3)), a)
    # An arm: a drooping sleeve, 64 x 40.
    W_, H_ = 64, 40
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    a = (((x - 32) / 30) ** 2 + ((y - 16) / 12) ** 2 < 1) | (((x - 52) / 10) ** 2 + ((y - 26) / 12) ** 2 < 1)
    save_rgba(out / "ghost_arm.png", np.ones((H_, W_, 3)), a.astype(float))
    # An eye: a dark oval.
    W_, H_ = 20, 28
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    a = (((x - 10) / 9) ** 2 + ((y - 14) / 13) ** 2 < 1).astype(float)
    save_rgba(out / "ghost_eye.png", np.ones((H_, W_, 3)) * 0.05, a)

    # The cursed lantern: an iron frame round violet glass, 48 x 72.
    W_, H_ = 48, 72
    rgb, a = np.zeros((H_, W_, 3)), np.zeros((H_, W_))
    a[10:66, 6:42] = 1
    rgb[10:66, 6:42] = (0.10, 0.09, 0.11)
    a[16:60, 11:37] = 1
    rgb[16:60, 11:37] = (0.45, 0.20, 0.65)
    rgb[16:60, 23:25] = (0.10, 0.09, 0.11)
    a[0:12, 18:30] = 1
    rgb[0:12, 18:30] = (0.10, 0.09, 0.11)
    a[2:6, 20:28] = 0
    save_rgba(out / "lantern.png", rgb, a)


# ── 94 Warehouse Robots ────────────────────────────────────────────────
# A warehouse from above: a concrete floor, rack tiles (the tilemap the
# robots path round), a delivery robot, a parcel, a wooden crate the mouse
# drags into their way, and the loading dock.

def wr94_warehouse():
    out = OUT / "warehouse"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(94)

    # Concrete, 256 x 256, with saw-cut joints every 128.
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    rgb = np.ones((S, S, 3)) * np.array((0.46, 0.47, 0.48))
    rgb *= (0.93 + 0.07 * fbm(S, 8, 4))[..., None]
    rgb *= (0.96 + 0.04 * rng.random((S, S)))[..., None]
    rgb[(x % 128 < 2) | (y % 128 < 2)] *= 0.75
    save_rgba(out / "floor.png", rgb, np.ones((S, S)))

    # A rack, seen from above, 40 x 40: a steel frame, boxes on the shelf.
    T = 40
    y, x = np.mgrid[0:T, 0:T].astype(np.float64)
    rgb = np.ones((T, T, 3)) * np.array((0.20, 0.22, 0.26))
    for bx, by, bw, bh, col in ((4, 5, 14, 13, (0.70, 0.52, 0.32)), (21, 4, 15, 14, (0.62, 0.45, 0.28)),
                                (5, 22, 13, 13, (0.74, 0.58, 0.36)), (21, 22, 14, 13, (0.30, 0.42, 0.62))):
        rgb[by:by + bh, bx:bx + bw] = col
        rgb[by:by + 1, bx:bx + bw] = np.array(col) * 1.2
    rgb[:, :2] = rgb[:, -2:] = (0.95, 0.55, 0.10)      # orange uprights
    rgb[:2, :] = rgb[-2:, :] = (0.30, 0.32, 0.36)
    save_rgba(out / "rack.png", np.clip(rgb, 0, 1), np.ones((T, T)))

    # The robot: a rounded white body, a dark deck, a green light at the front (+x).
    S = 64
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    dx, dy = x - S / 2 + 0.5, y - S / 2 + 0.5
    body = (np.abs(dx) / 28) ** 4 + (np.abs(dy) / 24) ** 4 < 1
    deck = (np.abs(dx) / 20) ** 4 + (np.abs(dy) / 16) ** 4 < 1
    rgb = np.zeros((S, S, 3))
    rgb[body] = (0.92, 0.93, 0.95)
    rgb[deck] = (0.25, 0.27, 0.30)
    light = (dx - 24) ** 2 + dy ** 2 < 9
    rgb[light] = (0.3, 1.0, 0.5)
    wheels = (np.abs(dy) > 20) & (np.abs(dy) < 25) & (np.abs(dx) < 14)
    rgb[wheels] = (0.08, 0.08, 0.09)
    save_rgba(out / "robot.png", rgb, (body | wheels).astype(float))

    # A parcel: a cardboard box with tape.
    S = 28
    rgb = np.ones((S, S, 3)) * np.array((0.78, 0.60, 0.38))
    rgb[:, 12:16] = (0.88, 0.80, 0.62)
    rgb[:2, :] = rgb[-2:, :] = rgb[:, :2] = rgb[:, -2:] = (0.55, 0.40, 0.24)
    save_rgba(out / "parcel.png", rgb, np.ones((S, S)))

    # A crate from above: planks and a cross brace.
    S = 64
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    rgb = np.ones((S, S, 3)) * np.array((0.60, 0.42, 0.22))
    rgb *= (0.88 + 0.12 * np.sin(y * 0.5 + np.sin(x * 0.15) * 2))[..., None]
    rgb[(y.astype(int) % 16) < 2] *= 0.65
    for d in (-3, -2, -1, 0, 1, 2, 3):
        rgb[np.abs(x - y + d) < 1] = (0.45, 0.30, 0.15)
    rgb[:5] = rgb[-5:] = (0.40, 0.27, 0.13)
    rgb[:, :5] = rgb[:, -5:] = (0.40, 0.27, 0.13)
    save_rgba(out / "crate.png", rgb, np.ones((S, S)))

    # The dock: a yellow-and-black hatched pad, 128 x 128.
    S = 128
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    a = np.ones((S, S))
    rgb = np.ones((S, S, 3)) * np.array((0.30, 0.31, 0.33))
    edge = (x < 10) | (x > S - 11) | (y < 10) | (y > S - 11)
    hatch = ((x + y) // 10) % 2 == 0
    rgb[edge & hatch] = (0.95, 0.75, 0.10)
    rgb[edge & ~hatch] = (0.08, 0.08, 0.08)
    save_rgba(out / "dock.png", rgb, a)


# ── 95 Equaliser ───────────────────────────────────────────────────────
# A club: a dark back wall, an LED cell (white, so the spectrum tints it),
# a dance-floor tile, a crowd silhouette in a few poses, the DJ booth and a
# keyboard key for the synth strip.

def eq95_club():
    out = OUT / "club"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(95)

    # The back wall: dark acoustic panels, 256 x 256.
    S = 256
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    rgb = np.ones((S, S, 3)) * np.array((0.05, 0.05, 0.08))
    panel = ((x % 64) < 60) & ((y % 64) < 60)
    rgb[panel] = (0.08, 0.075, 0.11)
    bump = (np.sin(x * 0.4) * np.sin(y * 0.4)) > 0.6
    rgb[panel & bump] = (0.10, 0.095, 0.14)
    save_rgba(out / "wall.png", rgb, np.ones((S, S)))

    # An LED cell: a rounded square with a hot centre, 32 x 32, white.
    S = 32
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    d = np.maximum(np.abs(x - S / 2 + 0.5), np.abs(y - S / 2 + 0.5))
    a = np.clip((14 - d) / 2, 0, 1)
    core = np.exp(-((x - S / 2 + 0.5) ** 2 + (y - S / 2 + 0.5) ** 2) / 60)
    save_rgba(out / "led.png", np.ones((S, S, 3)) * (0.75 + 0.25 * core[..., None]), a)

    # A dance-floor tile: frosted glass over a light, white, 64 x 64.
    S = 64
    y, x = np.mgrid[0:S, 0:S].astype(np.float64)
    d = np.maximum(np.abs(x - S / 2 + 0.5), np.abs(y - S / 2 + 0.5))
    a = (d < 30).astype(float)
    glow = 0.7 + 0.3 * np.exp(-((x - 32) ** 2 + (y - 32) ** 2) / 500)
    save_rgba(out / "tile.png", np.ones((S, S, 3)) * glow[..., None], a)

    # Dancers: four silhouettes, 64 x 128 - arms down, up, one up, wide.
    W_, H_ = 64, 128
    for k in range(4):
        y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
        a = np.zeros((H_, W_))
        a[((x - 32) / 10) ** 2 + ((y - 18) / 11) ** 2 < 1] = 1           # head
        a[(np.abs(x - 32) < 11 - (y - 30) * 0.04) & (y > 28) & (y < 78)] = 1  # body
        for side in (-1, 1):
            a[(np.abs(x - 32 - side * 7) < 5) & (y >= 76) & (y < 126)] = 1   # legs
        def arm(x0, y0, x1, y1):
            for t in np.linspace(0, 1, 40):
                cx, cy = x0 + (x1 - x0) * t, y0 + (y1 - y0) * t
                a[((x - cx) ** 2 + (y - cy) ** 2) < 16] = 1
        poses = [((21, 34, 14, 70), (43, 34, 50, 70)), ((21, 34, 12, 2), (43, 34, 52, 2)),
                 ((21, 34, 14, 70), (43, 34, 54, 4)), ((21, 34, 2, 40), (43, 34, 62, 40))]
        for (x0, y0, x1, y1) in poses[k]:
            arm(x0, y0, x1, y1)
        save_rgba(out / f"dancer{k}.png", np.ones((H_, W_, 3)) * 0.02, a)

    # The DJ booth: a desk with two decks, 360 x 110.
    W_, H_ = 360, 110
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    rgb = np.ones((H_, W_, 3)) * np.array((0.10, 0.10, 0.13))
    rgb[:10] = (0.22, 0.22, 0.28)
    for cx in (90, 270):
        deck = (x - cx) ** 2 + ((y - 34) * 2.2) ** 2 < 42 ** 2
        rgb[deck] = (0.04, 0.04, 0.05)
        rgb[(x - cx) ** 2 + ((y - 34) * 2.2) ** 2 < 6 ** 2] = (0.7, 0.7, 0.75)
    rgb[60:64, 20:340] = (0.9, 0.3, 0.8)
    save_rgba(out / "booth.png", rgb, np.ones((H_, W_)))

    # A keyboard key, white with a shadow line, 48 x 120.
    W_, H_ = 48, 120
    rgb = np.ones((H_, W_, 3)) * 0.92
    rgb[:, -3:] = 0.6
    rgb[-6:] = 0.7
    save_rgba(out / "key.png", rgb, np.ones((H_, W_)))


# ── 96 Photo Mode ──────────────────────────────────────────────────────
# A scenic overlook at sunset: sky, sun, three ranges of mountains, a lake,
# pines, the rock ledge you stand on, a bird; and the photo mode's frame -
# a viewfinder's corners - and a polaroid border for the gallery.

def pm96_overlook():
    out = OUT / "overlook"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(96)

    W_, H_ = 1600, 900
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = (y / H_)[..., None]
    sky = (1 - t) * np.array((0.18, 0.22, 0.48)) + t * np.array((1.0, 0.62, 0.38))
    sun = np.exp(-(((x - 1020) ** 2 + (y - 470) ** 2) / 9000.0))[..., None]
    halo = np.exp(-(((x - 1020) ** 2 + (y - 470) ** 2) / 120000.0))[..., None]
    sky = sky + sun * np.array((1.0, 0.9, 0.6)) * 0.9 + halo * np.array((1.0, 0.6, 0.3)) * 0.35
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # Three ranges, far to near: a ridge line each, hazier the further.
    def ridge(seed, base, amp, rough):
        r = np.random.default_rng(seed)
        xs = np.arange(W_)
        h = np.zeros(W_)
        for k in range(1, 7):
            h += np.sin(xs / W_ * np.pi * (k * rough) + r.random() * 6) * amp / k
        return base + h
    for name, seed, base, amp, rough, col in (("range_far", 1, 300, 90, 2.0, (0.55, 0.45, 0.62)),
                                              ("range_mid", 2, 380, 120, 2.6, (0.36, 0.28, 0.45)),
                                              ("range_near", 3, 470, 90, 3.4, (0.20, 0.16, 0.26))):
        top = ridge(seed, base, amp, rough)
        a = (y >= top[None, :]).astype(float)
        shade = 1.0 - 0.25 * np.clip((y - top[None, :]) / 300.0, 0, 1)
        rgb = np.ones((H_, W_, 3)) * np.array(col) * shade[..., None]
        save_rgba(out / f"{name}.png", rgb, a)

    # The lake: the sky's colours mirrored, rippled, 1600 x 220.
    W2, H2 = 1600, 220
    y2, x2 = np.mgrid[0:H2, 0:W2].astype(np.float64)
    t2 = (1 - y2 / H2)[..., None]
    rgb = (1 - t2) * np.array((0.12, 0.14, 0.30)) + t2 * np.array((0.85, 0.52, 0.36))
    glint = np.exp(-((x2 - 1020) ** 2) / 4000.0) * (np.sin(y2 * 0.9 + np.sin(x2 * 0.05) * 2) > 0.3)
    rgb = rgb + glint[..., None] * np.array((1.0, 0.85, 0.5)) * 0.6
    rgb *= (0.92 + 0.08 * np.sin(y2 * 0.6 + x2 * 0.01))[..., None]
    save_rgba(out / "lake.png", np.clip(rgb, 0, 1), np.ones((H2, W2)))

    # A pine, 80 x 200, near-black against the sunset.
    W3, H3 = 80, 200
    y3, x3 = np.mgrid[0:H3, 0:W3].astype(np.float64)
    a = np.zeros((H3, W3))
    for tier in range(6):
        top = 10 + tier * 28
        half = 10 + tier * 6
        a[(y3 >= top) & (y3 < top + 40) & (np.abs(x3 - 40) < half * (y3 - top) / 40 + 2)] = 1
    a[(y3 >= 180) & (np.abs(x3 - 40) < 5)] = 1
    save_rgba(out / "pine.png", np.ones((H3, W3, 3)) * np.array((0.05, 0.06, 0.07)), a)

    # The ledge in the foreground, 1600 x 200.
    W4, H4 = 1600, 200
    y4, x4 = np.mgrid[0:H4, 0:W4].astype(np.float64)
    edge = 60 + 25 * np.sin(x4 / 140) + 12 * np.sin(x4 / 37 + 2)
    a = (y4 >= edge).astype(float)
    rgb = np.ones((H4, W4, 3)) * np.array((0.10, 0.08, 0.08))
    rgb *= (0.8 + 0.2 * rng.random((H4, W4)))[..., None]
    save_rgba(out / "ledge.png", rgb, a)

    # A bird: a shallow V, 40 x 16.
    W5, H5 = 40, 16
    y5, x5 = np.mgrid[0:H5, 0:W5].astype(np.float64)
    v = np.abs(y5 - (2 + np.abs(x5 - 20) * 0.45)) < 1.6
    save_rgba(out / "bird.png", np.ones((H5, W5, 3)) * 0.05, v.astype(float))

    # The viewfinder: four corner brackets and a centre cross, 512 x 288.
    W6, H6 = 512, 288
    a = np.zeros((H6, W6))
    L, T = 48, 4
    for cx, cy in ((0, 0), (W6 - L, 0), (0, H6 - T), (W6 - L, H6 - T)):
        a[cy:cy + T, cx:cx + L] = 1
    for cx, cy in ((0, 0), (W6 - T, 0), (0, H6 - L), (W6 - T, H6 - L)):
        a[cy:cy + L, cx:cx + T] = 1
    a[H6 // 2 - 1:H6 // 2 + 1, W6 // 2 - 14:W6 // 2 + 14] = 1
    a[H6 // 2 - 14:H6 // 2 + 14, W6 // 2 - 1:W6 // 2 + 1] = 1
    save_rgba(out / "viewfinder.png", np.ones((H6, W6, 3)), a)


# ── 97 Bazaar Signs ────────────────────────────────────────────────────
# A market street at dusk: sky, a row of shop fronts, striped awnings, and
# the signs - a painted board, a brass-rimmed plaque and a neon frame, all
# white so each sign's colour tints them.

def bz97_bazaar():
    out = OUT / "bazaar"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(97)

    W_, H_ = 1400, 800
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = (y / H_)[..., None]
    sky = (1 - t) * np.array((0.10, 0.10, 0.28)) + t * np.array((0.75, 0.38, 0.30))
    stars = (rng.random((H_, W_)) > 0.9985) & (y < 300)
    sky[stars] = (0.9, 0.9, 1.0)
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H_, W_)))

    # Shop fronts: a row of plastered buildings, arched doorways, lit windows.
    W_, H_ = 1400, 420
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    a = np.zeros((H_, W_))
    rgb = np.zeros((H_, W_, 3))
    xs = 0
    palette = [(0.55, 0.38, 0.28), (0.48, 0.42, 0.36), (0.60, 0.45, 0.30), (0.42, 0.34, 0.30)]
    k = 0
    while xs < W_:
        w = int(rng.integers(180, 260))
        h = int(rng.integers(260, 400))
        col = np.array(palette[k % len(palette)])
        x0, x1 = xs, min(W_, xs + w)
        region = (x >= x0) & (x < x1) & (y >= H_ - h)
        a[region] = 1
        rgb[region] = col * (0.85 + 0.15 * rng.random())
        # A doorway arch and two windows.
        cx = (x0 + x1) / 2
        # An arched doorway: a box to the ground under a half-round top.
        door = ((np.abs(x - cx) < 40) & (y > H_ - 120)) | (((x - cx) ** 2 + (y - (H_ - 120)) ** 2) < 40 ** 2)
        rgb[door & region] = (0.08, 0.05, 0.04)
        for wx in (x0 + w * 0.25, x0 + w * 0.75):
            win = (np.abs(x - wx) < 18) & (y > H_ - h + 50) & (y < H_ - h + 100)
            rgb[win & region] = (1.0, 0.78, 0.40) if rng.random() < 0.7 else (0.10, 0.08, 0.10)
        xs += w
        k += 1
    save_rgba(out / "shops.png", rgb, a)

    # An awning: red and cream stripes with a scalloped edge, 220 x 70.
    W_, H_ = 220, 70
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    stripe = (x // 22) % 2 == 0
    rgb = np.where(stripe[..., None], np.array((0.78, 0.16, 0.14)), np.array((0.92, 0.86, 0.72)))
    rgb *= (1.0 - 0.25 * (y / H_))[..., None]
    edge = 52 + 10 * np.abs(np.sin(x / 22 * np.pi))
    a = (y < edge).astype(float)
    save_rgba(out / "awning.png", rgb, a)

    # A painted board: rounded, with a darker rim and plank lines, 256 x 96.
    W_, H_ = 256, 96
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    d = np.maximum(np.abs(x - W_ / 2 + 0.5) - (W_ / 2 - 14), 0) ** 2 + np.maximum(np.abs(y - H_ / 2 + 0.5) - (H_ / 2 - 14), 0) ** 2
    a = (d < 14 ** 2).astype(float)
    rim = (d > 9 ** 2) & (d < 14 ** 2)
    # Wood grain along the board: long, faint streaks.
    g = 0.9 + 0.06 * np.sin(y * 0.55 + np.sin(x * 0.015) * 3) + 0.04 * np.sin(y * 1.7 + x * 0.004)
    rgb = np.ones((H_, W_, 3)) * g[..., None]
    rgb[(y.astype(int) % 24) == 0] *= 0.82
    rgb[rim] = 0.45
    save_rgba(out / "board.png", rgb, a)

    # A neon frame: a glowing rounded outline, 256 x 96.
    a = np.clip(1.0 - np.abs(np.sqrt(d) - 10) / 4.5, 0, 1)
    glow = np.exp(-((np.sqrt(d) - 10) ** 2) / 60)
    save_rgba(out / "neon.png", np.ones((H_, W_, 3)), np.clip(a + glow * 0.5, 0, 1))

    # A chain link for hanging a sign, 6 x 40.
    rgb = np.ones((40, 6, 3)) * 0.25
    save_rgba(out / "chain.png", rgb, np.ones((40, 6)))
# ── 98 Readable Menus ──────────────────────────────────────────────────
# A title screen's backdrop - dawn over folded hills, a castle on the far
# ridge and a lantern-lit road winding up to it - and a white speaker icon
# the caption strip tints.

def rm98_readable():
    out = OUT / "readable"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(98)

    W_, H_ = 1400, 800
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    t = (y / H_)
    # Sky: night blue overhead to a warm band at the horizon (y ~ 470).
    h = np.clip(t / 0.6, 0, 1)[..., None]
    sky = (1 - h) * np.array((0.05, 0.07, 0.18)) + h * np.array((0.95, 0.58, 0.36))
    sky += np.exp(-((x - 980) ** 2 / 9e4 + (y - 470) ** 2 / 6e3))[..., None] * np.array((0.35, 0.25, 0.10))
    stars = (rng.random((H_, W_)) > 0.9982) & (y < 260)
    sky[stars] = (0.9, 0.92, 1.0)
    rgb = sky.copy()
    noise = fbm_rect(W_, H_, 6, 4)

    def ridge(base, amp, freq, phase):
        r = base + amp * (np.sin(x[0] * freq + phase) * 0.6 + np.sin(x[0] * freq * 2.7 + phase * 1.9) * 0.3
                          + np.sin(x[0] * freq * 6.1 + phase * 0.7) * 0.1)
        return r[None, :]

    # Hills, far (hazy, warm) to near (dark, cool).
    layers = [(470, 40, 0.004, 0.5, (0.55, 0.36, 0.34)), (520, 50, 0.006, 2.1, (0.33, 0.22, 0.27)),
              (590, 45, 0.005, 4.0, (0.18, 0.13, 0.19)), (680, 35, 0.007, 1.3, (0.08, 0.07, 0.11))]
    castle_ridge = None
    for i, (base, amp, freq, ph, col) in enumerate(layers):
        top = ridge(base, amp, freq, ph)
        land = y >= top
        shade = 0.9 + 0.2 * noise
        rgb[land] = (np.array(col)[None, :] * shade[land][:, None])
        if i == 1:
            castle_ridge = top[0]
    # A castle on the second ridge: keep, two towers with pointed roofs, a lit window or two.
    cx = 1040
    base = castle_ridge[cx]
    sil = np.zeros((H_, W_), bool)
    sil |= (np.abs(x - cx) < 46) & (y > base - 70) & (y < base + 6)
    for tx, th in ((cx - 58, 110), (cx + 58, 96), (cx, 140)):
        sil |= (np.abs(x - tx) < 15) & (y > base - th) & (y < base + 6)
        sil |= (np.abs(x - tx) < 19 - (base - th - y) * 0.45) & (y <= base - th) & (y > base - th - 40)
    # Crenellations on the keep.
    sil &= ~((np.abs(x - cx) < 46) & (y < base - 64) & (y > base - 72) & ((x // 8) % 2 == 0))
    rgb[sil] = (0.20, 0.14, 0.20)
    for wx, wy in ((cx, base - 110), (cx - 58, base - 70), (cx + 20, base - 40)):
        win = (np.abs(x - wx) < 3) & (np.abs(y - wy) < 6)
        rgb[win] = (1.0, 0.82, 0.45)
    # A road winding up from the bottom left to the castle, with lanterns.
    pts = []
    for k in range(240):
        s = k / 239
        px = 260 + s * (cx - 260) + 140 * np.sin(s * 9.0) * (1 - s)
        py = H_ + 10 - s * (H_ + 10 - base)
        pts.append((px, py))
    road = np.zeros((H_, W_))
    for k, (px, py) in enumerate(pts):
        w = 3 + 30 * (1 - k / 239) ** 1.6
        x0, x1 = int(max(0, px - w - 2)), int(min(W_, px + w + 2))
        y0, y1 = int(max(0, py - 4)), int(min(H_, py + 4))
        if x0 < x1 and y0 < y1:
            road[y0:y1, x0:x1] = np.maximum(road[y0:y1, x0:x1], (np.abs(x[y0:y1, x0:x1] - px) < w).astype(float))
    rgb = rgb * (1 - road[..., None] * 0.55) + road[..., None] * np.array((0.42, 0.32, 0.30)) * 0.55
    glow = np.zeros((H_, W_))
    for k in range(10, 236, 22):
        px, py = pts[k]
        size = 2 + 3.5 * (1 - k / 239)
        glow += np.exp(-((x - px - size * 2) ** 2 + (y - py + size * 2) ** 2) / (2 * (size * 2.4) ** 2))
    rgb += glow[..., None] * np.array((1.0, 0.7, 0.35))
    save_rgba(out / "backdrop.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))

    # A speaker icon, white on clear, 64 x 64: a box, a cone, two sound arcs.
    W_, H_ = 64, 64
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64) + 0.5
    a = np.zeros((H_, W_))
    a = np.maximum(a, ((x > 8) & (x < 20) & (np.abs(y - 32) < 9)).astype(float))
    cone = (x >= 19) & (x < 34) & (np.abs(y - 32) < 9 + (x - 19) * 0.95)
    a = np.maximum(a, cone.astype(float))
    r = np.sqrt((x - 30) ** 2 + (y - 32) ** 2)
    ang = np.abs(np.arctan2(y - 32, x - 30))
    for rr in (12, 21):
        arc = np.clip(1.6 - np.abs(r - rr), 0, 1) * (ang < 0.85) * (x > 34)
        a = np.maximum(a, arc)
    save_rgba(out / "speaker.png", np.ones((H_, W_, 3)), a)
# ── 99 Night Watch ──────────────────────────────────────────────────────
# Top-down actors for a moonlit courtyard, all facing +x: a guard (cloak,
# steel helmet, pauldrons, a spear), a hooded thief, the thief's eyes (drawn
# unlit, so the thief can be found in the dark), a chest shut and open, and
# a little lantern.

def nw99_watch():
    out = OUT / "watch"
    out.mkdir(parents=True, exist_ok=True)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # Guard, 72 x 72, centre (36, 36).
    W_ = H_ = 72
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    spear = ((x > 36) & (x < 70) & (np.abs(y - 50) < 1.6)).astype(float)
    a = put(rgb, a, spear, (0.45, 0.32, 0.20))
    tip = ((x > 62) & (x < 71) & (np.abs(y - 50) < (71 - x) * 0.45)).astype(float)
    a = put(rgb, a, tip, (0.80, 0.82, 0.86))
    cloak = disc(x, y, 34, 36, 21)
    shade = 0.75 + 0.25 * np.clip((y - 18) / 36, 0, 1)
    a = put(rgb, a, cloak, (0.16, 0.22, 0.42))
    rgb *= np.where(cloak[..., None] > 0.5, shade[..., None], 1.0)
    for py in (20, 52):
        a = put(rgb, a, disc(x, y, 38, py, 7.5), (0.55, 0.57, 0.62))
    helm = disc(x, y, 38, 36, 12)
    a = put(rgb, a, helm, (0.70, 0.72, 0.78))
    a = put(rgb, a, disc(x, y, 41, 33, 4.5), (0.92, 0.93, 0.96))
    crest = ((np.abs(y - 36) < 2.2) & (x > 26) & (x < 46)).astype(float)
    a = put(rgb, a, crest, (0.62, 0.12, 0.10))
    hand = disc(x, y, 50, 50, 4)
    a = put(rgb, a, hand, (0.80, 0.62, 0.48))
    save_rgba(out / "guard.png", rgb, a)

    # Thief, 60 x 60: a dark hooded shape with a pale satchel strap.
    W_ = H_ = 60
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, disc(x, y, 28, 30, 18), (0.10, 0.09, 0.12))
    a = put(rgb, a, disc(x, y, 34, 30, 11), (0.17, 0.15, 0.20))
    strap = ((np.abs((x - 30) - (y - 30) * 0.9) < 2.0) & (disc(x, y, 28, 30, 17) > 0.5)).astype(float)
    a = put(rgb, a, strap, (0.45, 0.38, 0.30))
    a = put(rgb, a, disc(x, y, 18, 44, 6.5), (0.30, 0.24, 0.16))
    save_rgba(out / "thief.png", rgb, a)

    # Eyes, 20 x 12: two glints.
    W_, H_ = 20, 12
    x, y = grid(W_, H_)
    a = np.maximum(disc(x, y, 6, 6, 2.6, 1.6), disc(x, y, 14, 6, 2.6, 1.6))
    save_rgba(out / "eyes.png", np.ones((H_, W_, 3)), a)

    # Chests, 64 x 48: shut, and open with gold.
    W_, H_ = 64, 48
    x, y = grid(W_, H_)
    for name, open_ in (("chest_shut", False), ("chest_open", True)):
        rgb = np.zeros((H_, W_, 3))
        a = np.zeros((H_, W_))
        body = ((x > 4) & (x < 60) & (y > 10) & (y < 44)).astype(float)
        a = put(rgb, a, body, (0.42, 0.24, 0.12))
        planks = body * ((np.abs(((y - 10) % 11) - 0)) < 1.0)
        rgb[planks > 0] *= 0.6
        if open_:
            inside = ((x > 9) & (x < 55) & (y > 14) & (y < 30)).astype(float)
            a = put(rgb, a, inside, (0.12, 0.07, 0.04))
            rng = np.random.default_rng(99)
            for _ in range(26):
                cx, cy = rng.uniform(12, 52), rng.uniform(16, 29)
                a = put(rgb, a, disc(x, y, cx, cy, 3.0) * (y < 31), (0.95, 0.76, 0.25))
            lid = ((x > 4) & (x < 60) & (y > 2) & (y < 12)).astype(float)
            a = put(rgb, a, lid, (0.34, 0.19, 0.10))
        else:
            lid = ((x > 3) & (x < 61) & (y > 6) & (y < 22)).astype(float)
            a = put(rgb, a, lid, (0.50, 0.29, 0.14))
            band = (((np.abs(x - 16) < 2.5) | (np.abs(x - 48) < 2.5)) & (y > 6) & (y < 44)).astype(float)
            a = put(rgb, a, band, (0.62, 0.58, 0.50))
            lock = ((np.abs(x - 32) < 4) & (y > 18) & (y < 27)).astype(float)
            a = put(rgb, a, lock, (0.85, 0.70, 0.30))
        save_rgba(out / f"{name}.png", rgb, a)

    # A lantern, 20 x 24: a brass frame round a warm glass.
    W_, H_ = 20, 24
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    frame = ((x > 3) & (x < 17) & (y > 5) & (y < 22)).astype(float)
    a = put(rgb, a, frame, (0.55, 0.42, 0.20))
    glass = ((x > 5.5) & (x < 14.5) & (y > 8) & (y < 19.5)).astype(float)
    a = put(rgb, a, glass, (1.0, 0.85, 0.45))
    ring = disc(x, y, 10, 3.5, 3) - disc(x, y, 10, 3.5, 1.6)
    a = put(rgb, a, np.clip(ring, 0, 1), (0.55, 0.42, 0.20))
    save_rgba(out / "lantern.png", rgb, a)
# ── 100 Ember Cavern ────────────────────────────────────────────────────
# A cave in cross-section: a rock back wall and its normal map (cracked,
# layered stone, so the light of passing embers rakes across it), a ceiling
# of stalactites and a floor ledge (each with a normal map), and the lava
# river - drawn hot and unlit.

def ec100_cavern():
    out = OUT / "cavern"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(100)

    # The back wall, 1400 x 800: layered strata, lumps and cracks.
    W_, H_ = 1400, 800
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64)
    n1 = fbm_rect(W_, H_, 7, 5)
    n2 = fbm_rect(W_, H_, 22, 4)
    strata = 0.5 + 0.5 * np.sin((y + 60 * n1) / 23.0)
    height = 0.55 * n1 + 0.25 * n2 + 0.2 * strata
    # Cracks: thin valleys along a second noise's zero lines.
    n3 = fbm_rect(W_, H_, 6, 3)
    crack = np.exp(-(((n3 - 0.5) * 70.0) ** 2)) * np.clip((n1 - 0.42) * 6.0, 0, 1)
    height -= 0.35 * crack
    shade = 0.55 + 0.45 * height
    rgb = np.stack([0.62 * shade, 0.52 * shade, 0.47 * shade], axis=-1)
    rgb *= (1.0 - 0.6 * crack)[..., None]
    save_rgba(out / "rock.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "rock_n.png", normals_from_height(height, 7.0, wrap=False), np.ones((H_, W_)))

    def strip(name, h, top, seed, spikes):
        """A band of rock with a ragged edge and spikes off it: `top` hangs
        from the top (stalactites), else stands on the bottom."""
        r = np.random.default_rng(seed)
        y, x = np.mgrid[0:h, 0:W_].astype(np.float64)
        edge = h * 0.42 + 18 * np.sin(x / 61.0) + 11 * np.sin(x / 23.0 + 1.3) + 6 * np.sin(x / 9.0)
        depth = y if top else (h - 1 - y)
        solid = depth < edge
        height = np.clip((edge - depth) / 30.0, 0, 1)
        for _ in range(spikes):
            cx = r.uniform(0, W_)
            length = r.uniform(h * 0.5, h * 1.0)
            width = r.uniform(20, 52)
            # A cone from the band down (or up) to its tip.
            t = np.clip(depth / length, 0, 1)
            half = width * (1 - t) ** 1.3
            inside = (np.abs(x - cx) < half) & (depth < length)
            solid |= inside
            height = np.maximum(height, np.where(inside, np.sqrt(np.clip(1 - ((x - cx) / np.maximum(half, 1e-3)) ** 2, 0, 1)) * (1 - t * 0.5), 0))
        n = fbm_rect(W_, h, 18, 4)
        height = height * 0.8 + n * 0.2
        shade = 0.45 + 0.55 * height
        rgb = np.stack([0.66 * shade, 0.55 * shade, 0.48 * shade], axis=-1)
        a = solid.astype(float)
        save_rgba(out / f"{name}.png", rgb, a)
        save_rgba(out / f"{name}_n.png", normals_from_height(height, 6.0, wrap=False), a)

    strip("ceiling", 260, True, 101, 15)
    strip("ledge", 200, False, 102, 7)

    # Lava, 1400 x 120: molten swirls with a dark crust at the top edge.
    W2, H2 = 1400, 120
    y, x = np.mgrid[0:H2, 0:W2].astype(np.float64)
    swirl = fbm_rect(W2, H2, 14, 4)
    heat = np.clip(0.55 + 0.6 * (swirl - 0.5) + 0.25 * (y / H2), 0, 1)
    rgb = np.stack([0.9 + 0.1 * heat, 0.25 + 0.6 * heat ** 2, 0.05 + 0.3 * heat ** 4], axis=-1)
    crust = (fbm_rect(W2, H2, 30, 3) > 0.68) & (y > 14)
    rgb[crust] *= 0.35
    edge = 10 + 4 * np.sin(x / 37.0) + 3 * np.sin(x / 11.0)
    a = np.clip((y - edge) / 3.0, 0, 1)
    save_rgba(out / "lava.png", np.clip(rgb, 0, 1), a)
# ── 101 The Lit House ───────────────────────────────────────────────────
# A cottage from above at night: grass (and its normal map), a plank floor
# (and its normal map), a tiled gable roof, the furniture - a rug, a table
# with the lamp, a bed, a chair, a shelf - a tree and a bush, window glass,
# and the walker with a torch in hand, facing +x.

def lh101_house():
    out = OUT / "lithouse"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(101)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def box(x, y, x0, y0, x1, y1):
        return ((x >= x0) & (x < x1) & (y >= y0) & (y < y1)).astype(float)

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # Grass, 256 tiling: blades as streaks of noise, clumps darker.
    S = 256
    n1 = fbm(S, 8, 4)
    n2 = fbm(S, 32, 3)
    height = 0.6 * n1 + 0.4 * n2
    g = 0.55 + 0.45 * height
    rgb = np.stack([0.26 * g, 0.44 * g, 0.20 * g], axis=-1)
    save_rgba(out / "grass.png", rgb, np.ones((S, S)))
    save_rgba(out / "grass_n.png", normals_from_height(height, 4.0), np.ones((S, S)))

    # Floor planks, 360 x 260: boards running across, grain, gaps.
    W_, H_ = 360, 260
    x, y = grid(W_, H_)
    board = 26.0
    row = np.floor(y / board)
    off = (row * 97.0) % 160.0
    grain = fbm_rect(W_, H_, 30, 3)
    shade = 0.75 + 0.25 * np.sin((x + off) / 7.0 + grain * 6.0) * 0.3 + 0.15 * (((row * 37) % 5) / 5.0)
    gap = (np.abs((y % board) - 0.5) < 1.0) | (np.abs(((x + off) % 160.0) - 0.5) < 1.0)
    rgb = np.stack([0.52 * shade, 0.36 * shade, 0.22 * shade], axis=-1)
    rgb[gap] *= 0.35
    height = 1.0 - gap.astype(float) + 0.15 * grain
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "floor_n.png", normals_from_height(height, 3.0, wrap=False), np.ones((H_, W_)))

    # The roof, 420 x 320, ridge across the middle: rows of slate tiles, the
    # two slopes shaded apart, a chimney.
    W_, H_ = 420, 320
    x, y = grid(W_, H_)
    ridge = H_ / 2
    top = y < ridge
    rowh = 18.0
    d = np.abs(y - ridge)
    r = np.floor(d / rowh)
    tilew = 28.0
    tx = (x + (r % 2) * tilew / 2) % tilew
    edge = (np.abs((d % rowh) - rowh + 1.5) < 1.5) | (tx < 1.6)
    n = fbm_rect(W_, H_, 20, 3)
    base = np.where(top, 0.95, 0.72) * (0.85 + 0.25 * n)
    rgb = np.stack([0.78 * base, 0.42 * base, 0.30 * base], axis=-1)
    rgb[edge] *= 0.55
    ridge_band = np.abs(y - ridge) < 6
    rgb[ridge_band] = np.array([0.20, 0.13, 0.11]) * (0.9 + 0.2 * n[ridge_band])[:, None]
    a = np.ones((H_, W_))
    chim = box(x, y, 300, 70, 340, 112)
    a = put(rgb, a, chim, (0.30, 0.25, 0.24))
    a = put(rgb, a, box(x, y, 306, 76, 334, 106), (0.07, 0.06, 0.06))
    # Eaves: a darker rim.
    rim = (x < 6) | (x > W_ - 6) | (y < 6) | (y > H_ - 6)
    rgb[rim] *= 0.6
    save_rgba(out / "roof.png", np.clip(rgb, 0, 1), a)

    # Rug 140 x 90, woven bands.
    W_, H_ = 140, 90
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = box(x, y, 2, 2, W_ - 2, H_ - 2)
    band = np.minimum(np.minimum(x, W_ - x), np.minimum(y, H_ - y))
    rgb[:] = (0.55, 0.14, 0.12)
    rgb[(band > 8) & (band < 14)] = (0.85, 0.65, 0.30)
    rgb[(band > 22) & (band < 26)] = (0.20, 0.25, 0.45)
    rgb *= (0.85 + 0.15 * fbm_rect(W_, H_, 24, 2))[..., None]
    save_rgba(out / "rug.png", rgb, a)

    # Table 96 x 64 with a cloth, a book and a mug.
    W_, H_ = 96, 64
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 2, 2, W_ - 2, H_ - 2), (0.40, 0.26, 0.15))
    a = put(rgb, a, box(x, y, 6, 6, W_ - 6, H_ - 6), (0.48, 0.32, 0.19))
    a = put(rgb, a, box(x, y, 14, 30, 40, 50), (0.25, 0.30, 0.45))
    a = put(rgb, a, disc(x, y, 76, 46, 6), (0.85, 0.85, 0.80))
    a = put(rgb, a, disc(x, y, 76, 46, 3.5), (0.30, 0.18, 0.10))
    save_rgba(out / "table.png", rgb, a)

    # The lamp seen from above, 40 x 40: a shade round a bright bulb.
    W_, H_ = 40, 40
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = disc(x, y, 20, 20, 17)
    rgb[:] = (0.95, 0.85, 0.60)
    a2 = disc(x, y, 20, 20, 8)
    rgb = rgb * (1 - a2[..., None]) + np.array((1.0, 1.0, 0.92)) * a2[..., None]
    rim = np.clip(a - disc(x, y, 20, 20, 15), 0, 1)
    rgb = rgb * (1 - rim[..., None]) + np.array((0.55, 0.40, 0.20)) * rim[..., None]
    save_rgba(out / "lamp.png", rgb, a)

    # Bed 90 x 130: frame, sheet, pillow, a folded blanket.
    W_, H_ = 90, 130
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 0, 0, W_, H_), (0.32, 0.20, 0.12))
    a = put(rgb, a, box(x, y, 6, 6, W_ - 6, H_ - 6), (0.85, 0.83, 0.78))
    a = put(rgb, a, box(x, y, 14, 10, W_ - 14, 34), (0.95, 0.95, 0.92))
    a = put(rgb, a, box(x, y, 6, 64, W_ - 6, H_ - 6), (0.30, 0.40, 0.55))
    rgb[(np.abs(y - 64) < 2) & (a > 0)] *= 0.7
    save_rgba(out / "bed.png", rgb, a)

    # Chair 34 x 34, shelf 120 x 30.
    W_, H_ = 34, 34
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 3, 3, 31, 31), (0.40, 0.26, 0.15))
    a = put(rgb, a, box(x, y, 3, 3, 31, 9), (0.30, 0.19, 0.11))
    save_rgba(out / "chair.png", rgb, a)
    W_, H_ = 120, 30
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = put(rgb, np.zeros((H_, W_)), box(x, y, 0, 0, W_, H_), (0.36, 0.23, 0.13))
    for i in range(14):
        bx = 6 + i * 8
        col = [(0.55, 0.15, 0.12), (0.20, 0.35, 0.25), (0.25, 0.25, 0.45), (0.65, 0.55, 0.30)][i % 4]
        a = put(rgb, a, box(x, y, bx, 4, bx + 6, 26 - (i % 3) * 3), col)
    save_rgba(out / "shelf.png", rgb, a)

    # Window glass, 16 x 16 tiling pale blue.
    save_rgba(out / "glass.png", np.ones((16, 16, 3)) * np.array((0.70, 0.82, 0.95)), np.ones((16, 16)) * 0.55)

    # A tree's crown from above, 180 x 180, and a bush, 70 x 70.
    for name, size, col, seed in (("tree", 180, (0.30, 0.55, 0.26), 7), ("bush", 70, (0.34, 0.60, 0.28), 9)):
        W_ = H_ = size
        x, y = grid(W_, H_)
        r2 = np.random.default_rng(seed)
        a = np.zeros((H_, W_))
        shade = np.zeros((H_, W_))
        for _ in range(14 if size > 100 else 7):
            cx, cy = r2.uniform(size * 0.3, size * 0.7), r2.uniform(size * 0.3, size * 0.7)
            rr = r2.uniform(size * 0.18, size * 0.28)
            dd = disc(x, y, cx, cy, rr, 2.0)
            a = np.maximum(a, dd)
            shade = np.maximum(shade, dd * np.clip(1 - ((x - cx + rr * 0.3) ** 2 + (y - cy + rr * 0.3) ** 2) / (rr * rr), 0, 1))
        leaf = fbm_rect(W_, H_, 16 if size > 100 else 8, 3)
        k = 0.55 + 0.35 * shade + 0.25 * leaf
        rgb = np.stack([col[0] * k, col[1] * k, col[2] * k], axis=-1)
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), a)

    # The walker, 56 x 56, facing +x: shoulders, a cap, an arm with a torch.
    W_ = H_ = 56
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 30, 34, 50, 39), (0.30, 0.30, 0.34))
    a = put(rgb, a, box(x, y, 46, 33, 53, 40), (0.85, 0.85, 0.80))
    a = put(rgb, a, disc(x, y, 24, 28, 15), (0.45, 0.28, 0.18))
    a = put(rgb, a, disc(x, y, 26, 28, 9), (0.20, 0.32, 0.25))
    a = put(rgb, a, disc(x, y, 28, 28, 5), (0.26, 0.40, 0.32))
    a = put(rgb, a, disc(x, y, 33, 37, 3.5), (0.85, 0.66, 0.52))
    save_rgba(out / "walker.png", rgb, a)

    # A path of flat stones, 64 x 64 tiling (alpha: the stones).
    S = 64
    x, y = grid(S, S)
    a = np.zeros((S, S))
    rgb = np.zeros((S, S, 3))
    for cx, cy, rr in ((16, 18, 13), (46, 14, 11), (34, 44, 15), (8, 50, 9), (58, 50, 8)):
        dd = disc(x, y, cx, cy, rr, 1.5)
        a = put(rgb, a, dd, tuple(v * (0.9 + 0.2 * rng.random()) for v in (0.48, 0.46, 0.44)))
    save_rgba(out / "path.png", rgb, a)
# ── 102 Firefight ───────────────────────────────────────────────────────
# A night-time lot from above: the asphalt (and its normal map - stains
# are painted into it at runtime), crates, concrete barriers, sandbags, a
# burnt car, barrels, the two sides' soldiers (facing +x), a tracer streak
# and a shell casing.

def ff102_firefight():
    out = OUT / "firefight"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(102)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The lot, 1024 x 576 for 1920 x 1080 world units: asphalt grain,
    # cracks, worn yellow bay lines, old oil.
    W_, H_ = 1024, 576
    x, y = grid(W_, H_)
    g1 = fbm_rect(W_, H_, 64, 3)
    g2 = fbm_rect(W_, H_, 8, 4)
    height = 0.5 * g1 + 0.5 * g2
    shade = 0.26 + 0.14 * g2 + 0.10 * g1
    rgb = np.stack([shade * 0.92, shade * 0.93, shade * 1.0], axis=-1)
    n3 = fbm_rect(W_, H_, 10, 3)
    crack = np.exp(-(((n3 - 0.5) * 140.0) ** 2)) * (g2 > 0.6)
    rgb *= (1 - 0.55 * crack)[..., None]
    height -= 0.4 * crack
    # Bay lines: two rows of parking stripes and a centre line, worn.
    wear = np.clip((fbm_rect(W_, H_, 40, 2) - 0.35) * 3.0, 0, 1)
    lines = np.zeros((H_, W_))
    for row_y, up in ((120.0, True), (456.0, False)):
        for bx in np.arange(70, W_, 92):
            lines = np.maximum(lines, box(x, y, bx - 3, row_y - (0 if up else 70), bx + 3, row_y + (70 if up else 0)))
        lines = np.maximum(lines, box(x, y, 40, row_y - 3, W_ - 40, row_y + 3))
    lines = np.maximum(lines, box(x, y, 40, 285, W_ - 40, 291) * ((x // 30) % 2 == 0))
    lm = lines * wear
    rgb = rgb * (1 - lm[..., None] * 0.85) + np.array((0.78, 0.62, 0.18)) * lm[..., None] * 0.85
    height += 0.15 * lm
    # Old oil: faint, soft and broken up, or it reads as a hole in the lot.
    blot = fbm_rect(W_, H_, 24, 3)
    for _ in range(9):
        cx, cy, r = rng.uniform(80, W_ - 80), rng.uniform(60, H_ - 60), rng.uniform(14, 40)
        o = disc(x, y, cx, cy, r, 18.0) * np.clip(blot * 1.4 - 0.2, 0, 1) * 0.22
        rgb *= (1 - o)[..., None]
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "floor_n.png", normals_from_height(height, 5.0, wrap=False), np.ones((H_, W_)))

    def textured(name, w, h, base, mask, height, strength=4.0):
        save_rgba(out / f"{name}.png", np.clip(base, 0, 1), mask)
        save_rgba(out / f"{name}_n.png", normals_from_height(height, strength, wrap=False), mask)

    # A crate, 64 x 64: planks, a frame, a cross brace.
    S = 64
    x, y = grid(S, S)
    wood = fbm_rect(S, S, 12, 3)
    frame = (box(x, y, 2, 2, 62, 62) - box(x, y, 8, 8, 56, 56)).clip(0, 1)
    brace = np.clip(1 - np.abs((x - y)) / 4.0, 0, 1) * box(x, y, 8, 8, 56, 56)
    planks = (np.abs(((y - 8) % 12) - 0.5) < 1.0) * box(x, y, 8, 8, 56, 56)
    shade = 0.62 + 0.25 * wood - 0.25 * planks + 0.12 * (frame + brace)
    rgb = np.stack([0.62 * shade, 0.44 * shade, 0.26 * shade], axis=-1)
    a = box(x, y, 1, 1, 63, 63)
    textured("crate", S, S, rgb, a, 0.5 * wood + 0.6 * (frame + brace) - 0.4 * planks)

    # A concrete barrier from above, 140 x 40: a ridge along its middle.
    W_, H_ = 140, 40
    x, y = grid(W_, H_)
    ridge = 1 - np.abs(y - H_ / 2) / (H_ / 2)
    n = fbm_rect(W_, H_, 14, 3)
    shade = 0.55 + 0.3 * ridge + 0.12 * n
    rgb = np.stack([shade * 0.78, shade * 0.77, shade * 0.74], axis=-1)
    stripe = ((x // 18) % 2 == 0) * (np.abs(y - H_ / 2) < 3)
    rgb[stripe > 0] = (0.75, 0.25, 0.15)
    a = box(x, y, 2, 3, W_ - 2, H_ - 3, 1.5)
    textured("barrier", W_, H_, rgb, a, ridge + 0.2 * n)

    # Sandbags, 180 x 60: two staggered rows of bags.
    W_, H_ = 180, 60
    x, y = grid(W_, H_)
    hgt = np.zeros((H_, W_))
    for row, cy in enumerate((17, 43)):
        for i in range(7):
            cx = 14 + i * 26 + (13 if row else 0)
            if cx > W_ - 10:
                continue
            dx, dy = (x - cx) / 15.0, (y - cy) / 12.0
            hgt = np.maximum(hgt, np.clip(1 - (dx * dx + dy * dy), 0, 1))
    a = np.clip(hgt * 6, 0, 1)
    n = fbm_rect(W_, H_, 20, 2)
    shade = 0.45 + 0.4 * np.sqrt(hgt) + 0.1 * n
    rgb = np.stack([shade * 0.72, shade * 0.64, shade * 0.46], axis=-1)
    textured("sandbags", W_, H_, rgb, a, hgt + 0.15 * n)

    # A burnt car, 200 x 96, nose to +x: a shell, an open roof, scorched.
    W_, H_ = 200, 96
    x, y = grid(W_, H_)
    body = box(x, y, 4, 8, W_ - 4, H_ - 8, 3.0)
    roof = box(x, y, 60, 18, 140, H_ - 18, 2.0)
    n = fbm_rect(W_, H_, 18, 3)
    burnt = np.clip(fbm_rect(W_, H_, 6, 3) * 1.6 - 0.3, 0, 1)
    paint = np.array((0.30, 0.34, 0.40))
    char = np.array((0.08, 0.07, 0.07))
    rgb = paint * (1 - burnt[..., None]) + char * burnt[..., None]
    rgb = rgb * (0.7 + 0.3 * n[..., None])
    rgb[roof > 0.5] *= 0.35
    glass = (box(x, y, 140, 16, 160, H_ - 16) + box(x, y, 44, 18, 58, H_ - 18)).clip(0, 1)
    rgb = rgb * (1 - glass[..., None] * 0.7)
    for wx, wy in ((40, 6), (160, 6), (40, H_ - 6), (160, H_ - 6)):
        tyre = box(x, y, wx - 14, wy - 5, wx + 14, wy + 5)
        rgb = rgb * (1 - tyre[..., None]) + np.array((0.05, 0.05, 0.05)) * tyre[..., None]
        body = np.maximum(body, tyre)
    textured("car", W_, H_, rgb, body, 0.6 * body - 0.4 * roof + 0.2 * n)

    # Barrels, 40 x 40: a red one with hazard bands, a rusty open one.
    S = 40
    x, y = grid(S, S)
    a = disc(x, y, 20, 20, 18)
    r = np.sqrt((x - 20) ** 2 + (y - 20) ** 2)
    rim = np.clip(1 - np.abs(r - 15) / 2.0, 0, 1)
    lid = np.clip(1 - r / 18.0, 0, 1)
    rgb = np.zeros((S, S, 3))
    rgb[:] = (0.70, 0.12, 0.08)
    sym = (np.abs(r - 7) < 2.2) | ((r < 4))
    rgb[sym] = (0.95, 0.80, 0.15)
    rgb *= (0.75 + 0.35 * lid - 0.3 * rim)[..., None]
    textured("barrel_red", S, S, rgb, a, lid - 0.5 * rim)
    rust = fbm_rect(S, S, 8, 3)
    rgb = np.stack([0.38 + 0.25 * rust, 0.22 + 0.12 * rust, 0.12 + 0.05 * rust], axis=-1)
    inside = disc(x, y, 20, 20, 13)
    rgb = rgb * (1 - inside[..., None] * 0.85)
    textured("barrel_rust", S, S, rgb, a, 1 - inside * 0.8 - 0.3 * rim)

    # The soldiers, 64 x 64, facing +x: shoulders, a head, arms and a gun.
    def soldier(name, jacket, head, helmet_band, gun, tip, visor=None):
        S = 64
        x, y = grid(S, S)
        rgb = np.zeros((S, S, 3))
        a = np.zeros((S, S))
        a = put(rgb, a, box(x, y, 30, 34, 60, 39), gun)                     # the gun
        a = put(rgb, a, box(x, y, 56, 34.5, 61, 38.5), tip)                 # its muzzle
        a = put(rgb, a, disc(x, y, 34, 38, 5), (0.30, 0.25, 0.22))          # the fore hand
        a = put(rgb, a, disc(x, y, 26, 32, 17), jacket)                     # shoulders
        sh = disc(x, y, 30, 28, 12) * 0.25
        rgb[:] = rgb * (1 + sh[..., None])
        a = put(rgb, a, disc(x, y, 22, 36, 6), tuple(c * 0.8 for c in jacket))
        a = put(rgb, a, disc(x, y, 29, 31, 9.5), head)                      # the head
        a = put(rgb, a, np.clip(disc(x, y, 29, 31, 9.5) - disc(x, y, 31, 31, 8.0), 0, 1), helmet_band)
        if visor is not None:
            a = put(rgb, a, box(x, y, 35, 27, 39, 35), visor)
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), a)

    soldier("soldier_red", (0.50, 0.12, 0.10), (0.25, 0.20, 0.16), (0.65, 0.15, 0.12),
            (0.18, 0.17, 0.16), (0.30, 0.28, 0.25))
    soldier("soldier_blue", (0.16, 0.24, 0.40), (0.30, 0.35, 0.42), (0.20, 0.26, 0.34),
            (0.22, 0.26, 0.32), (0.40, 0.95, 1.0), visor=(0.45, 0.95, 1.0))

    # A tracer, 96 x 10: a white-hot core in an orange glow, brightest at
    # its head (+x).
    W_, H_ = 96, 10
    x, y = grid(W_, H_)
    along = np.clip(x / W_, 0, 1) ** 1.5
    across = np.exp(-((y - H_ / 2) ** 2) / 6.0)
    core = np.exp(-((y - H_ / 2) ** 2) / 1.2) * along
    rgb = np.stack([np.ones_like(x), 0.75 + 0.25 * core, 0.45 + 0.55 * core], axis=-1)
    save_rgba(out / "tracer.png", np.clip(rgb, 0, 1), np.clip(across * along * 1.3, 0, 1))

    # A shell casing, 10 x 4: brass.
    W_, H_ = 10, 4
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    rgb[:] = (0.85, 0.65, 0.25)
    rgb[:, 7:] = (0.65, 0.48, 0.18)
    save_rgba(out / "casing.png", rgb, box(x, y, 0.5, 0.5, 9.5, 3.5, 0.6))
# ── 103 Snowfield ───────────────────────────────────────────────────────
# A snowfield at night from above: snow with wind-cut ridges and drifts (its
# normal map's alpha is a sparse sparkle mask, read as specular), pines with
# snow on their boughs, rocks, a cabin roof under snow, a frozen pond, a
# wolf, a walker in a hooded coat and a sled - each facing +x.

def sf103_snowfield():
    out = OUT / "snowfield"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(103)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The field, 1024 x 576 for 1920 x 1080 world units. Snow is near white
    # with a cold cast; its relief is drifts (broad) and sastrugi (ridges cut
    # by the wind, running roughly along x).
    W_, H_ = 1024, 576
    x, y = grid(W_, H_)
    drift = fbm_rect(W_, H_, 4, 3)
    warp = fbm_rect(W_, H_, 6, 2)
    ridges = 0.5 + 0.5 * np.sin((y + 40 * warp) / 7.0 + 3.0 * drift)
    ridges = ridges ** 3 * np.clip(fbm_rect(W_, H_, 5, 2) * 1.8 - 0.5, 0, 1)
    grain = fbm_rect(W_, H_, 200, 2)
    height = 1.6 * drift + 0.35 * ridges + 0.008 * grain
    shade = 0.80 + 0.10 * drift + 0.05 * grain
    rgb = np.stack([shade * 0.92, shade * 0.95, shade * 1.0], axis=-1)
    save_rgba(out / "snow.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    n = normals_from_height(height, 2.2, wrap=False)
    # Sparkle: one texel in ~70 is a crystal, shiny where the rest is matt.
    # Its normal is the snow's - a crystal tilted its own way also turned
    # its diffuse away from the light and read as a speck of grit.
    sparkle = (rng.random((H_, W_)) < 0.014).astype(np.float64)
    save_rgba(out / "snow_n.png", n, 0.06 + 0.94 * sparkle)

    def textured(name, base, mask, height, strength=4.0):
        save_rgba(out / f"{name}.png", np.clip(base, 0, 1), mask)
        save_rgba(out / f"{name}_n.png", normals_from_height(height, strength, wrap=False), mask)

    # A pine from above, 160 x 160: bough lobes in rings, smaller toward the
    # top, each a dome; snow lies on the side of each lobe facing up-left.
    S = 160
    x, y = grid(S, S)
    hgt = np.zeros((S, S))
    for ring, (rad, count, lobe, lift) in enumerate(((58, 14, 17, 0.35), (40, 10, 15, 0.6), (22, 7, 12, 0.85), (0, 1, 13, 1.0))):
        for i in range(count):
            ang = 2 * np.pi * (i + rng.random() * 0.5) / count + ring
            cx = S / 2 + np.cos(ang) * rad * (0.9 + 0.2 * rng.random())
            cy = S / 2 + np.sin(ang) * rad * (0.9 + 0.2 * rng.random())
            r = lobe * (0.85 + 0.3 * rng.random())
            d = np.clip(1 - ((x - cx) ** 2 + (y - cy) ** 2) / r ** 2, 0, 1)
            hgt = np.maximum(hgt, np.sqrt(d) * 0.4 + lift * (d > 0))
    a = np.clip(hgt * 10, 0, 1)
    needles = fbm_rect(S, S, 40, 2)
    green = np.stack([0.10 + 0.06 * needles, 0.26 + 0.10 * needles, 0.17 + 0.06 * needles], axis=-1)
    green *= (0.55 + 0.5 * hgt)[..., None]
    gy, gx = np.gradient(hgt)
    snow = np.clip((-gx - gy) * 9 + (fbm_rect(S, S, 18, 2) - 0.5) * 1.2 - 0.05, 0, 1) * (a > 0.5)
    snow = np.clip(snow * 1.6, 0, 1)
    rgb = green * (1 - snow[..., None]) + np.array((0.86, 0.9, 0.97)) * (0.8 + 0.2 * hgt)[..., None] * snow[..., None]
    textured("pine", rgb, a, hgt * 3 + 0.2 * needles + snow * 0.3)

    # A rock, 90 x 70, its top under snow.
    W_, H_ = 90, 70
    x, y = grid(W_, H_)
    rr = np.sqrt(((x - 45) / 40) ** 2 + ((y - 35) / 30) ** 2)
    lump = fbm_rect(W_, H_, 5, 3)
    a = np.clip((1.0 - rr + 0.15 * (lump - 0.5)) * 12, 0, 1)
    hgt = np.clip(1 - rr, 0, 1) ** 0.6 + 0.2 * lump
    capm = np.clip((hgt - 0.55) * 6 + (lump - 0.5), 0, 1)
    stone = np.stack([0.30 + 0.1 * lump, 0.30 + 0.1 * lump, 0.33 + 0.1 * lump], axis=-1)
    rgb = stone * (1 - capm[..., None]) + np.array((0.86, 0.9, 0.97)) * capm[..., None]
    textured("rock", rgb, a, hgt * 2)

    # A cabin from above, 220 x 170: a gabled roof under snow - the ridge
    # along x with a cap, the gable ends' dark fascia, snow lumped over the
    # eaves - and a stone chimney.
    W_, H_ = 220, 170
    x, y = grid(W_, H_)
    lump = fbm_rect(W_, H_, 12, 2)
    edge = np.minimum(np.minimum(x, W_ - x), np.minimum(y, H_ - y))
    roof = np.clip((edge - 4 - 3 * lump) / 1.5, 0, 1)
    ridge = 1 - np.abs(y - H_ / 2) / (H_ / 2)
    fascia = np.clip(1 - np.minimum(x - 4, W_ - 4 - x) / 7.0, 0, 1) * roof
    snow = np.array((0.86, 0.9, 0.97))
    # The north slope (top) faces the moon; the south is a shade bluer.
    slope = np.where(y < H_ / 2, 1.0, 0.86)[..., None]
    rgb = snow * slope * (0.92 + 0.08 * lump[..., None]) * np.ones((H_, W_, 3))
    cap = np.clip(1 - np.abs(y - H_ / 2) / 2.5, 0, 1)
    rgb = rgb * (1 - 0.18 * cap[..., None])
    rgb = rgb * (1 - fascia[..., None]) + np.array((0.24, 0.15, 0.1)) * fascia[..., None]
    chim = box(x, y, 150, 34, 176, 60, 1.0)
    stones = (fbm_rect(W_, H_, 40, 2) > 0.5) * 0.08
    rgb = rgb * (1 - chim[..., None]) + (np.array((0.36, 0.33, 0.32)) + stones[..., None]) * chim[..., None]
    hole = box(x, y, 155, 39, 171, 55, 1.0)
    rgb = rgb * (1 - hole[..., None]) + np.array((0.03, 0.02, 0.02)) * hole[..., None]
    textured("cabin", rgb, roof, ridge * 1.5 + chim * 0.8 - hole * 0.6 + 0.1 * lump - fascia * 0.3)

    # Ice, 360 x 220: dark clear ice with pale cracks and a frosted rim.
    W_, H_ = 360, 220
    x, y = grid(W_, H_)
    rr = np.sqrt(((x - 180) / 172) ** 2 + ((y - 110) / 104) ** 2)
    edge_n = fbm_rect(W_, H_, 6, 2)
    a = np.clip((1.0 - rr + 0.06 * (edge_n - 0.5)) * 30, 0, 1)
    frost = np.clip((rr - 0.78) * 5 + edge_n * 0.4, 0, 1)
    c3 = fbm_rect(W_, H_, 8, 3)
    crack = np.exp(-(((c3 - 0.5) * 90.0) ** 2)) * np.clip(fbm_rect(W_, H_, 5, 2) * 2 - 0.6, 0, 1)
    base = np.stack([0.30 + 0.08 * c3, 0.42 + 0.1 * c3, 0.55 + 0.1 * c3], axis=-1)
    base = base + crack[..., None] * 0.18
    rgb = base * (1 - frost[..., None]) + np.array((0.78, 0.84, 0.93)) * frost[..., None]
    save_rgba(out / "ice.png", np.clip(rgb, 0, 1), a)
    nn = normals_from_height(0.05 * c3 - 0.12 * crack + frost * 0.6, 3.0, wrap=False)
    save_rgba(out / "ice_n.png", nn, a * (1 - 0.7 * frost))

    # A wolf from above, 64 x 32, nose to +x.
    W_, H_ = 64, 32
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    fur = fbm_rect(W_, H_, 30, 2)
    body = np.clip(1 - (((x - 28) / 20) ** 2 + ((y - 16) / 8.5) ** 2), 0, 1)
    a = put(rgb, a, np.clip(body * 8, 0, 1), (0.42, 0.42, 0.44))
    tail = np.clip(1 - (((x - 7) / 8) ** 2 + ((y - 16) / 3.5) ** 2), 0, 1)
    a = put(rgb, a, np.clip(tail * 8, 0, 1), (0.36, 0.35, 0.36))
    head = disc(x, y, 50, 16, 7.0)
    a = put(rgb, a, head, (0.46, 0.45, 0.47))
    snout = np.clip(1 - (((x - 58) / 5) ** 2 + ((y - 16) / 3) ** 2), 0, 1)
    a = put(rgb, a, np.clip(snout * 8, 0, 1), (0.52, 0.5, 0.5))
    for ex in (-4, 4):
        a = put(rgb, a, disc(x, y, 47, 16 + ex, 2.2), (0.3, 0.29, 0.3))
    back = np.clip(1 - np.abs(y - 16) / 3, 0, 1) * (body > 0.2)
    rgb = rgb * (0.85 + 0.3 * fur[..., None]) * (1 - 0.25 * back[..., None])
    save_rgba(out / "wolf.png", np.clip(rgb, 0, 1), a)

    # A walker from above, 40 x 40, facing +x: a hooded coat and a pack.
    S = 40
    x, y = grid(S, S)
    rgb = np.zeros((S, S, 3))
    a = np.zeros((S, S))
    a = put(rgb, a, box(x, y, 7, 11, 17, 29, 2.0), (0.22, 0.24, 0.2))       # the pack
    a = put(rgb, a, disc(x, y, 20, 20, 12), (0.55, 0.16, 0.12))             # shoulders
    a = put(rgb, a, disc(x, y, 22, 20, 7.5), (0.62, 0.2, 0.15))             # the hood
    a = put(rgb, a, disc(x, y, 25, 20, 5), (0.82, 0.8, 0.76))               # its fur rim
    a = put(rgb, a, disc(x, y, 31, 28, 3.5), (0.35, 0.25, 0.2))             # the lantern hand
    save_rgba(out / "walker.png", np.clip(rgb, 0, 1), a)

    # A sled from above, 80 x 36, front to +x: two runners, a slatted bed,
    # a load under canvas.
    W_, H_ = 80, 36
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    for ry in (5, 31):
        a = put(rgb, a, box(x, y, 2, ry - 2, 78, ry + 2, 0.8), (0.18, 0.13, 0.1))
    slats = (np.abs(((x - 6) % 8) - 0.5) < 1.2) * 0.25
    bed = box(x, y, 6, 7, 70, 29, 0.8)
    a = put(rgb, a, bed, (0.5, 0.34, 0.2))
    rgb *= (1 - slats * bed)[..., None]
    load = np.clip(1 - (((x - 36) / 24) ** 2 + ((y - 18) / 9) ** 2), 0, 1)
    a = put(rgb, a, np.clip(load * 6, 0, 1), (0.55, 0.5, 0.38))
    save_rgba(out / "sled.png", np.clip(rgb, 0, 1), a)
# ── 104 Wildfire ────────────────────────────────────────────────────────
# A dry meadow at dusk from above, 1920 x 1080 world units: golden grass, a
# river winding down it, two ploughed firebreaks - the same geometry the
# scene's fire reads (WF_RIVER, WF_BREAKS in gen_scenes.py). Oaks, a water
# bomber facing +x, and a transparent canvas for the fire to be painted into.

WF_RIVER_W = 70.0


def wf_river_x(y):
    """Where the river runs at height y - the scene's script has the same."""
    return 360 + 140 * np.sin(y * 0.0042 + 0.6) + 35 * np.sin(y * 0.013)
WF_BREAKS = [(-262, -560, -232, 300), (-960, -152, -500, -124)]


def wf104_wildfire():
    out = OUT / "wildfire"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(104)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The meadow, 1024 x 576 texels for 1920 x 1080 units.
    W_, H_ = 1024, 576
    x, y = grid(W_, H_)
    wx = x / W_ * 1920 - 960          # world units, y up
    wy = 540 - y / H_ * 1080
    patch = fbm_rect(W_, H_, 6, 3)
    blades = fbm_rect(W_, H_, 260, 2)
    tufts = fbm_rect(W_, H_, 60, 2)
    dry = np.clip(patch * 1.4 - 0.2, 0, 1)
    green = np.array((0.36, 0.42, 0.16))
    gold = np.array((0.62, 0.52, 0.24))
    rgb = green * (1 - dry[..., None]) + gold * dry[..., None]
    rgb = rgb * (0.8 + 0.25 * blades[..., None] + 0.12 * tufts[..., None])
    height = 0.6 * blades + 0.5 * tufts + 0.3 * patch

    # The firebreaks: bare ploughed earth, furrows along their length.
    for x0, y0, x1, y1 in WF_BREAKS:
        long_x = (x1 - x0) > (y1 - y0)
        m = (np.clip((wx - x0) / 4 + 0.5, 0, 1) * np.clip((x1 - wx) / 4 + 0.5, 0, 1)
             * np.clip((wy - y0) / 4 + 0.5, 0, 1) * np.clip((y1 - wy) / 4 + 0.5, 0, 1))
        furrow = 0.5 + 0.5 * np.sin((wy if long_x else wx) * 1.1)
        dirt = np.array((0.36, 0.26, 0.17)) * (0.8 + 0.3 * furrow[..., None])
        rgb = rgb * (1 - m[..., None]) + dirt * m[..., None]
        height = height * (1 - m) + (0.5 * furrow) * m

    # The river: how far across from its middle, in world units.
    d = np.abs(wx - wf_river_x(wy))
    half = WF_RIVER_W / 2
    water = np.clip((half - d) / 3 + 0.5, 0, 1)
    bank = np.clip((half + 14 - d) / 6, 0, 1) * (1 - water)
    ripple = fbm_rect(W_, H_, 40, 2)
    deep = np.clip(1 - d / half, 0, 1)
    wcol = np.array((0.10, 0.20, 0.24)) * (1 - 0.4 * deep[..., None]) + 0.05 * ripple[..., None]
    rgb = rgb * (1 - bank[..., None] * 0.45) + np.array((0.24, 0.2, 0.12)) * bank[..., None] * 0.45
    rgb = rgb * (1 - water[..., None]) + wcol * water[..., None]
    height = height * (1 - water) + 0.05 * ripple * water - 0.4 * bank
    save_rgba(out / "meadow.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    n = normals_from_height(height, 2.0, wrap=False)
    save_rgba(out / "meadow_n.png", n, water * 0.9)

    # An oak from above, 150 x 150: rounded clumps of leaves.
    S = 150
    x, y = grid(S, S)
    hgt = np.zeros((S, S))
    for ring, (rad, count, lobe, lift) in enumerate(((50, 11, 20, 0.3), (28, 7, 18, 0.6), (0, 1, 20, 0.85))):
        for i in range(count):
            ang = 2 * np.pi * (i + rng.random() * 0.6) / count + ring * 0.7
            cx = S / 2 + np.cos(ang) * rad * (0.85 + 0.3 * rng.random())
            cy = S / 2 + np.sin(ang) * rad * (0.85 + 0.3 * rng.random())
            r = lobe * (0.85 + 0.3 * rng.random())
            dd = np.clip(1 - ((x - cx) ** 2 + (y - cy) ** 2) / r ** 2, 0, 1)
            hgt = np.maximum(hgt, np.sqrt(dd) * 0.45 + lift * (dd > 0))
    a = np.clip(hgt * 10, 0, 1)
    leaves = fbm_rect(S, S, 36, 2)
    rgb = np.stack([0.20 + 0.10 * leaves, 0.32 + 0.12 * leaves, 0.10 + 0.05 * leaves], axis=-1)
    rgb *= (0.55 + 0.55 * hgt)[..., None]
    save_rgba(out / "oak.png", np.clip(rgb, 0, 1), a)
    save_rgba(out / "oak_n.png", normals_from_height(hgt * 3 + 0.3 * leaves, 4.0, wrap=False), a)

    # A water bomber from above, 160 x 130, nose to +x: a yellow fuselage,
    # high wings, twin engines, a red tail.
    W_, H_ = 160, 130
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    yellow = (0.95, 0.72, 0.12)
    wing = np.clip(1 - np.abs(x - 84) / 14, 0, 1) * (np.abs(y - 65) < 62)
    a = put(rgb, a, np.clip(wing * 4, 0, 1), (0.88, 0.66, 0.1))
    tail = np.clip(1 - np.abs(x - 18) / 7, 0, 1) * (np.abs(y - 65) < 26)
    a = put(rgb, a, np.clip(tail * 4, 0, 1), (0.82, 0.18, 0.12))
    body = np.clip(1 - (((x - 82) / 74) ** 2 + ((y - 65) / 11) ** 2), 0, 1)
    a = put(rgb, a, np.clip(body * 6, 0, 1), yellow)
    for ey in (38, 92):
        eng = np.clip(1 - (((x - 98) / 16) ** 2 + ((y - ey) / 6) ** 2), 0, 1)
        a = put(rgb, a, np.clip(eng * 6, 0, 1), (0.3, 0.3, 0.32))
    a = put(rgb, a, disc(x, y, 146, 65, 5), (0.2, 0.35, 0.5))       # the cockpit glass
    stripe = (np.abs(y - 65) < 2) * (body > 0.2)
    rgb[stripe] = (0.8, 0.15, 0.1)
    shade = 0.8 + 0.25 * np.clip(1 - np.abs(y - 65) / 11, 0, 1)
    rgb *= shade[..., None]
    save_rgba(out / "bomber.png", np.clip(rgb, 0, 1), a)

    # The canvas the fire is painted into: 384 x 216 (5 units a texel),
    # white, transparent.
    save_rgba(out / "canvas.png", np.ones((216, 384, 3)), np.zeros((216, 384)))
# ── 105 Graffiti Alley ──────────────────────────────────────────────────
# An alley at night, seen straight on: a brick wall (its normal map's alpha
# a faint wet sheen, so fresh paint can be glossier than the brick), the wet
# asphalt in front of it, a tagger seen from behind (the arm is a part of
# its own), a spray can, and a street lamp's head.

def ga105_alley():
    out = OUT / "alley"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(105)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The wall, 1024 x 512 texels for 1920 x 960 units: running-bond brick,
    # each brick its own shade, mortar sunk, soot toward the ground.
    W_, H_ = 1024, 512
    x, y = grid(W_, H_)
    bw, bh = 48.0, 20.0
    row = np.floor(y / bh)
    off = (row % 2) * bw / 2
    bx = np.floor((x + off) / bw)
    seed = (bx * 73 + row * 151) % 997
    tone = (np.sin(seed * 12.9898) * 43758.5453) % 1.0
    fx = ((x + off) % bw) / bw
    fy = (y % bh) / bh
    mortar = np.clip(1 - np.minimum(np.minimum(fx, 1 - fx) * bw, np.minimum(fy, 1 - fy) * bh) / 2.2, 0, 1)
    grit = fbm_rect(W_, H_, 120, 2)
    blot = fbm_rect(W_, H_, 6, 3)
    brick = np.stack([0.42 + 0.14 * tone, 0.20 + 0.07 * tone, 0.15 + 0.05 * tone], axis=-1)
    brick = brick * (0.8 + 0.3 * grit[..., None]) * (0.75 + 0.35 * blot[..., None])
    mcol = np.array((0.36, 0.34, 0.31)) * (0.8 + 0.3 * grit[..., None])
    rgb = brick * (1 - mortar[..., None]) + mcol * mortar[..., None]
    soot = np.clip((y / H_ - 0.55) * 2.2, 0, 1) * (0.6 + 0.4 * blot)
    rgb = rgb * (1 - 0.45 * soot[..., None])
    height = (1 - mortar) * (0.7 + 0.25 * grit) + 0.15 * tone
    save_rgba(out / "wall.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    # A faint sheen on the brick: wet brick glistens a little, paint more.
    save_rgba(out / "wall_n.png", normals_from_height(height, 4.0, wrap=False), 0.08 + 0.06 * (1 - mortar))

    # The alley floor in front of it, 1024 x 128 for 1920 x 240: wet asphalt
    # with puddles (the shine is in the alpha).
    W_, H_ = 1024, 128
    x, y = grid(W_, H_)
    g = fbm_rect(W_, H_, 200, 2)
    pud = np.clip((fbm_rect(W_, H_, 8, 3) - 0.55) * 8, 0, 1)
    shade = 0.12 + 0.06 * g
    rgb = np.stack([shade, shade * 1.02, shade * 1.08], axis=-1) * (1 - 0.4 * pud[..., None])
    save_rgba(out / "ground.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "ground_n.png", normals_from_height(0.4 * g * (1 - pud), 2.0, wrap=False), 0.25 + 0.75 * pud)

    # A tagger from behind, 80 x 170: hood up, a jacket, jeans, trainers.
    W_, H_ = 80, 170
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    for lx in (30, 50):
        a = put(rgb, a, box(x, y, lx - 8, 104, lx + 8, 162, 1.5), (0.18, 0.22, 0.34))      # jeans
        a = put(rgb, a, box(x, y, lx - 9, 158, lx + 10, 168, 1.5), (0.85, 0.85, 0.82))     # trainers
    torso = box(x, y, 16, 44, 64, 110, 4.0)
    a = put(rgb, a, torso, (0.20, 0.42, 0.30))
    a = put(rgb, a, disc(x, y, 40, 30, 17), (0.18, 0.38, 0.27))                           # the hood
    a = put(rgb, a, box(x, y, 16, 46, 26, 100, 3.0), (0.17, 0.36, 0.26))                  # the near arm
    fold = fbm_rect(W_, H_, 14, 2)
    rgb *= (0.8 + 0.35 * fold[..., None])
    save_rgba(out / "tagger.png", np.clip(rgb, 0, 1), a)
    # The spraying arm, 70 x 16, from the shoulder (left) to the hand.
    W_, H_ = 70, 16
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 2, 3, 58, 13, 3.0), (0.19, 0.4, 0.28))
    a = put(rgb, a, disc(x, y, 61, 8, 6), (0.78, 0.6, 0.48))
    save_rgba(out / "arm.png", np.clip(rgb, 0, 1), a)
    # A spray can, 16 x 34, nozzle up.
    W_, H_ = 16, 34
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 2, 6, 14, 33, 1.5), (0.85, 0.85, 0.88))
    a = put(rgb, a, box(x, y, 5, 1, 11, 7, 1.0), (0.2, 0.2, 0.22))
    rgb *= (0.7 + 0.4 * np.clip(1 - np.abs(x - 6) / 6, 0, 1))[..., None]
    save_rgba(out / "can.png", np.clip(rgb, 0, 1), a)
    # A street lamp's head, 90 x 40: a dark housing over a glowing lens.
    W_, H_ = 90, 40
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 4, 4, 86, 26, 3.0), (0.16, 0.16, 0.18))
    a = put(rgb, a, box(x, y, 14, 24, 76, 34, 3.0), (1.0, 0.85, 0.55))
    save_rgba(out / "lamp.png", np.clip(rgb, 0, 1), a)
# ── 106 The Foundry ─────────────────────────────────────────────────────
# A foundry seen side on: a sooty brick and iron back wall, a sand casting
# bed with three mould cavities, a crucible on a crane, the crane's rail and
# trolley, and a brick furnace with an arched mouth.

FD_MOULDS = [(-150.0, 160.0), (150.0, 160.0), (450.0, 160.0)]   # centre x, width
FD_BED_TOP, FD_CAVITY_FLOOR = -300.0, -372.0


def fd106_foundry():
    out = OUT / "foundry"
    out.mkdir(parents=True, exist_ok=True)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def textured(name, base, mask, height, strength=4.0):
        save_rgba(out / f"{name}.png", np.clip(base, 0, 1), mask)
        save_rgba(out / f"{name}_n.png", normals_from_height(height, strength, wrap=False), mask)

    # The back wall, 1024 x 576 for 1920 x 1080: brick below, riveted iron
    # panels above, soot climbing from the floor and blooming round the top.
    W_, H_ = 1024, 576
    x, y = grid(W_, H_)
    grit = fbm_rect(W_, H_, 140, 2)
    blot = fbm_rect(W_, H_, 6, 3)
    bw, bh = 40.0, 16.0
    row = np.floor(y / bh)
    off = (row % 2) * bw / 2
    fx, fy = ((x + off) % bw) / bw, (y % bh) / bh
    mortar = np.clip(1 - np.minimum(np.minimum(fx, 1 - fx) * bw, np.minimum(fy, 1 - fy) * bh) / 2.0, 0, 1)
    seed = (np.floor((x + off) / bw) * 73 + row * 151) % 997
    tone = (np.sin(seed * 12.9898) * 43758.5453) % 1.0
    brick = np.stack([0.30 + 0.1 * tone, 0.16 + 0.05 * tone, 0.12 + 0.04 * tone], axis=-1)
    rgb = brick * (1 - mortar[..., None]) + np.array((0.2, 0.19, 0.18)) * mortar[..., None]
    height = (1 - mortar) * 0.8
    panel_band = y < 250
    px, py = (x % 128) / 128, (y % 125) / 125
    seam = np.clip(1 - np.minimum(np.minimum(px, 1 - px) * 128, np.minimum(py, 1 - py) * 125) / 2.5, 0, 1)
    rivet = np.zeros((H_, W_))
    for cx in (8, 120):
        for cy in (8, 117):
            rivet = np.maximum(rivet, disc(x % 128, y % 125, cx, cy, 3.2))
    iron = np.array((0.22, 0.22, 0.24)) * (0.8 + 0.4 * grit[..., None]) * (1 - 0.4 * seam[..., None])
    iron = iron + rivet[..., None] * 0.08
    rgb = np.where(panel_band[..., None], iron, rgb)
    height = np.where(panel_band, 0.5 - 0.3 * seam + 0.5 * rivet, height)
    soot = np.clip((y / H_ - 0.35) * 1.6, 0, 1) * 0.3 + np.clip(1 - y / 200, 0, 1) * 0.35
    rgb = rgb * (1 - soot[..., None] * (0.7 + 0.3 * blot[..., None])) * (0.8 + 0.25 * grit[..., None])
    textured("wall", rgb, np.ones((H_, W_)), height + 0.1 * grit, 3.0)

    # The casting bed, 1024 x 128 for 1920 x 240 (its top at y = -300): damp
    # dark sand, the three cavities dug into it, a little scorched round them.
    W_, H_ = 1024, 128
    x, y = grid(W_, H_)
    wx = x / W_ * 1920 - 960
    wy = -300 - y / H_ * 240
    sand = fbm_rect(W_, H_, 160, 2)
    clump = fbm_rect(W_, H_, 14, 3)
    rgb = np.stack([0.30 + 0.08 * sand, 0.24 + 0.06 * sand, 0.17 + 0.05 * sand], axis=-1) * (0.75 + 0.35 * clump[..., None])
    cav = np.zeros((H_, W_))
    scorch = np.zeros((H_, W_))
    for cx, w in FD_MOULDS:
        m = (np.clip((wx - (cx - w / 2)) / 2 + 0.5, 0, 1) * np.clip(((cx + w / 2) - wx) / 2 + 0.5, 0, 1)
             * np.clip((wy - FD_CAVITY_FLOOR) / 2 + 0.5, 0, 1))
        cav = np.maximum(cav, m)
        scorch = np.maximum(scorch, np.clip(1 - np.abs(wx - cx) / (w * 0.8), 0, 1) * np.clip(1 - (-300 - wy) / 120, 0, 1))
    rgb = rgb * (1 - 0.45 * scorch[..., None])
    a = 1 - cav
    textured("bed", rgb, a, 0.4 * sand + 0.4 * clump, 3.0)

    # The crucible, 220 x 200, its trunnion at the middle of the image: a
    # tapered iron pot, a pouring lip to the right, molten metal glowing in
    # its mouth.
    W_, H_ = 220, 200
    x, y = grid(W_, H_)
    top, bottom = 40.0, 190.0
    half = 92 - (y - top) / (bottom - top) * 26
    pot = np.clip((half - np.abs(x - 110)) / 1.5 + 0.5, 0, 1) * np.clip((y - top) / 1.5 + 0.5, 0, 1) * np.clip((bottom - y) / 1.5 + 0.5, 0, 1)
    rim = box(x, y, 12, 30, 208, 46, 1.5)
    lip = np.clip(1 - (((x - 205) / 16) ** 2 + ((y - 38) / 9) ** 2), 0, 1) * 4
    a = np.clip(np.maximum(np.maximum(pot, rim), np.clip(lip, 0, 1)), 0, 1)
    g = fbm_rect(W_, H_, 50, 2)
    shade = np.clip(1 - np.abs(x - 110) / half, 0, 1) ** 0.6
    iron = np.stack([0.20 + 0.1 * shade, 0.19 + 0.09 * shade, 0.20 + 0.08 * shade], axis=-1) * (0.8 + 0.3 * g[..., None])
    rgb = iron * (1 - rim[..., None] * 0.3) + rim[..., None] * np.array((0.3, 0.28, 0.26))
    # Heat on the iron near its mouth.
    heat = np.clip(1 - (y - 46) / 50, 0, 1) * pot
    rgb = rgb + heat[..., None] * np.array((0.25, 0.07, 0.0))
    trunnion = disc(x, y, 110, 100, 10) + disc(x, y, 110, 100, 6) * 0.3
    rgb = rgb * (1 - np.clip(trunnion, 0, 1)[..., None] * 0.4)
    textured("crucible", rgb, a, shade + rim * 0.5 + np.clip(trunnion, 0, 1) * 0.6, 3.0)
    # The glow in its mouth (drawn unlit over the pot).
    mouth = box(x, y, 20, 26, 200, 40, 3.0)
    save_rgba(out / "crucible_glow.png", np.ones((H_, W_, 3)) * np.array((1.0, 0.75, 0.35)), mouth)

    # The rail, 512 x 24 (a long girder), and the trolley, 120 x 70.
    W_, H_ = 512, 24
    x, y = grid(W_, H_)
    web = np.clip(1 - np.abs(y - 12) / 10, 0, 1)
    rgb = np.ones((H_, W_, 3)) * np.array((0.24, 0.23, 0.22)) * (0.7 + 0.4 * web[..., None])
    rgb[np.abs(y - 12) > 9] *= 1.3
    textured("rail", rgb, np.ones((H_, W_)), web, 3.0)
    W_, H_ = 120, 70
    x, y = grid(W_, H_)
    body = box(x, y, 6, 14, 114, 60, 2.0)
    wheels = disc(x, y, 30, 12, 11) + disc(x, y, 90, 12, 11)
    a = np.clip(body + wheels, 0, 1)
    rgb = np.ones((H_, W_, 3)) * np.array((0.45, 0.32, 0.08)) * (0.75 + 0.3 * fbm_rect(W_, H_, 20, 2)[..., None])
    rgb = rgb * (1 - np.clip(wheels, 0, 1)[..., None]) + np.clip(wheels, 0, 1)[..., None] * np.array((0.15, 0.15, 0.16))
    stripe = ((x + y) // 10 % 2 == 0) & (y > 48) & (y < 58)
    rgb[stripe & (body > 0.5)] = (0.08, 0.08, 0.08)
    textured("trolley", rgb, a, body * 0.6 + np.clip(wheels, 0, 1), 3.0)

    # The furnace, 360 x 420: firebrick with an arched mouth (transparent -
    # its fire is a sprite behind it).
    W_, H_ = 360, 420
    x, y = grid(W_, H_)
    shape = box(x, y, 10, 30, 350, 420, 2.0) * np.clip((150 - np.abs(x - 180)) / 2 + 0.5, 0, 1)
    chimney = box(x, y, 130, 0, 230, 40, 2.0)
    arch = (np.clip((80 - np.abs(x - 180)) / 1.5 + 0.5, 0, 1) * np.clip((y - 230) / 1.5 + 0.5, 0, 1)) \
        + disc(x, y, 180, 230, 80) * (y < 232)
    arch = np.clip(arch, 0, 1) * (y < 400)
    a = np.clip(shape + chimney, 0, 1) * (1 - arch)
    bw, bh = 36.0, 15.0
    row = np.floor(y / bh)
    off = (row % 2) * bw / 2
    fx, fy = ((x + off) % bw) / bw, (y % bh) / bh
    mortar = np.clip(1 - np.minimum(np.minimum(fx, 1 - fx) * bw, np.minimum(fy, 1 - fy) * bh) / 1.8, 0, 1)
    tone = (np.sin(((np.floor((x + off) / bw) * 73 + row * 151) % 997) * 12.9898) * 43758.5453) % 1.0
    rgb = np.stack([0.45 + 0.12 * tone, 0.28 + 0.06 * tone, 0.2 + 0.04 * tone], axis=-1)
    rgb = rgb * (1 - mortar[..., None]) + np.array((0.22, 0.2, 0.18)) * mortar[..., None]
    soot = np.clip(1 - np.abs(x - 180) / 120, 0, 1) * np.clip(1 - (y - 120) / 160, 0, 1)
    rgb = rgb * (1 - 0.6 * soot[..., None])
    glow_edge = np.clip(1 - (np.sqrt((x - 180) ** 2 + (np.maximum(y - 230, 0) * 0.4) ** 2) - 80) / 30, 0, 1) * (1 - arch)
    rgb = rgb + glow_edge[..., None] * np.array((0.35, 0.12, 0.02))
    textured("furnace", rgb, a, (1 - mortar) * 0.8, 3.0)
# ── 107 Night Pursuit ───────────────────────────────────────────────────
# A city street at night from above, tiling along x: four lanes of wet
# asphalt between kerbs and pavements; rooftops; cars (a getaway coupe, a
# police cruiser, a sedan, a van, a taxi) facing +x; a street lamp's head; a
# rotor blade's shadow.

def np107_pursuit():
    out = OUT / "pursuit"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(107)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The street, 512 x 384 for 1200 x 900 world units (y from 450 down to
    # -450): pavement, kerb, four lanes (centre double yellow, dashed lane
    # lines, edge lines), kerb, pavement. Tiles along x.
    W_, H_ = 512, 384
    x, y = grid(W_, H_)
    wy = 450 - y / H_ * 900          # world y
    wx = x / W_ * 1200
    grain = fbm_rect(W_, H_, 160, 2)
    puddle = np.clip((fbm_rect(W_, H_, 9, 3) - 0.56) * 7, 0, 1)
    road = np.abs(wy) < 260
    shade = 0.10 + 0.05 * grain
    rgb = np.stack([shade, shade * 1.02, shade * 1.1], axis=-1)
    # Pavement slabs.
    pave = (np.abs(wy) >= 280) & (np.abs(wy) < 400)
    slab = np.clip(1 - np.minimum(np.abs(((wx) % 80) - 40) - 38, np.abs(((wy) % 60) - 30) - 28) * -1, 0, 1)
    pcol = 0.26 + 0.05 * grain
    rgb[pave] = np.stack([pcol, pcol, pcol * 1.04], axis=-1)[pave]
    joints = pave & ((np.abs((wx % 80) - 0) < 2) | (np.abs((wy % 60) - 0) < 2))
    rgb[joints] *= 0.6
    kerb = (np.abs(wy) >= 260) & (np.abs(wy) < 280)
    rgb[kerb] = (0.42, 0.42, 0.44)
    gutter = (np.abs(wy) >= 400)
    rgb[gutter] = (0.06, 0.06, 0.07)
    paint = np.zeros((H_, W_))
    paint = np.maximum(paint, ((np.abs(np.abs(wy) - 5) < 3) & road).astype(float))            # double yellow
    paint_y = paint.copy()
    dash = ((wx % 200) < 110) & (np.abs(np.abs(wy) - 130) < 3) & road
    edge = (np.abs(np.abs(wy) - 248) < 3)
    paint_w = (dash | edge).astype(float)
    wear = np.clip(fbm_rect(W_, H_, 40, 2) * 1.6 - 0.2, 0, 1)
    rgb = rgb * (1 - paint_y[..., None] * wear[..., None]) + np.array((0.75, 0.6, 0.15)) * paint_y[..., None] * wear[..., None]
    rgb = rgb * (1 - paint_w[..., None] * wear[..., None]) + np.array((0.8, 0.8, 0.78)) * paint_w[..., None] * wear[..., None]
    rgb = rgb * (1 - 0.35 * puddle[..., None] * road[..., None])
    save_rgba(out / "street.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    height = 0.3 * grain - 0.2 * puddle + 0.6 * pave + 0.8 * kerb - 0.2 * joints
    save_rgba(out / "street_n.png", normals_from_height(height, 2.5, wrap=False), np.clip(0.25 * road + 0.75 * puddle * road, 0, 1))

    # A rooftop, 256 x 192, tinted per building: tar, a parapet, vents, an
    # AC unit or two, a skylight.
    W_, H_ = 256, 192
    x, y = grid(W_, H_)
    g = fbm_rect(W_, H_, 40, 2)
    base = 0.3 + 0.12 * g
    rgb = np.stack([base, base, base * 1.03], axis=-1)
    parapet = np.clip(1 - np.minimum(np.minimum(x, W_ - x), np.minimum(y, H_ - y)) / 8, 0, 1)
    rgb = rgb * (1 + 0.35 * parapet[..., None])
    hgt = parapet * 0.8
    for cx, cy, w, h in ((60, 60, 44, 32), (180, 120, 36, 36), (120, 150, 60, 20)):
        m = box(x, y, cx - w / 2, cy - h / 2, cx + w / 2, cy + h / 2, 1.5)
        fins = (np.abs(((x - cx) % 6) - 3) < 1) * m
        rgb = rgb * (1 - m[..., None]) + (np.array((0.5, 0.52, 0.55)) - fins[..., None] * 0.15) * m[..., None]
        hgt = np.maximum(hgt, m * 1.2)
    sky = box(x, y, 150, 30, 210, 70, 1.5)
    rgb = rgb * (1 - sky[..., None]) + np.array((0.15, 0.2, 0.28)) * sky[..., None]
    save_rgba(out / "roof.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "roof_n.png", normals_from_height(hgt + 0.1 * g, 3.0, wrap=False), 0.1 * np.ones((H_, W_)))

    # Cars, 128 x 60, nose to +x.
    def car(name, body, roof, extra=None):
        W_, H_ = 128, 60
        x, y = grid(W_, H_)
        rgb = np.zeros((H_, W_, 3))
        a = np.zeros((H_, W_))
        shape = box(x, y, 4, 6, 124, 54, 6.0)
        a = put(rgb, a, shape, body)
        hood = np.clip(1 - np.abs(y - 30) / 24, 0, 1)
        rgb *= (0.75 + 0.4 * hood[..., None])
        glass = box(x, y, 40, 10, 92, 50, 3.0)
        rgb[:] = rgb * (1 - glass[..., None]) + np.array((0.08, 0.1, 0.14)) * glass[..., None]
        top = box(x, y, 50, 13, 84, 47, 3.0)
        rgb[:] = rgb * (1 - top[..., None]) + np.array(roof) * top[..., None]
        for hy in (12, 48):
            a = put(rgb, a, box(x, y, 118, hy - 4, 125, hy + 4, 1.0), (0.95, 0.95, 0.85))   # headlights
            a = put(rgb, a, box(x, y, 3, hy - 4, 8, hy + 4, 1.0), (0.6, 0.05, 0.05))        # tail lights
        if extra:
            extra(x, y, rgb, a)
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), a)

    car("coupe", (0.9, 0.62, 0.05), (0.12, 0.12, 0.13))
    def lightbar(x, y, rgb, a):
        bar = box(x, y, 62, 10, 70, 50, 1.0)
        rgb[:] = rgb * (1 - bar[..., None]) + np.array((0.2, 0.2, 0.22)) * bar[..., None]
        door = box(x, y, 36, 6, 96, 54, 2.0) * (1 - box(x, y, 40, 10, 92, 50, 3.0))
        rgb[:] = rgb * (1 - door[..., None] * 0.9) + np.array((0.92, 0.92, 0.95)) * door[..., None] * 0.9
    car("police", (0.08, 0.08, 0.1), (0.9, 0.9, 0.92), lightbar)
    car("sedan", (0.55, 0.57, 0.62), (0.4, 0.42, 0.46))
    def checker(x, y, rgb, a):
        sign = box(x, y, 62, 22, 72, 38, 1.0)
        rgb[:] = rgb * (1 - sign[..., None]) + np.array((0.95, 0.95, 0.8)) * sign[..., None]
    car("taxi", (0.95, 0.75, 0.1), (0.95, 0.75, 0.1), checker)
    W_, H_ = 150, 66
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 4, 4, 146, 62, 4.0), (0.85, 0.85, 0.88))
    a = put(rgb, a, box(x, y, 118, 10, 140, 56, 2.0), (0.08, 0.1, 0.14))
    ribs = (np.abs((x % 14) - 7) < 1) * box(x, y, 10, 8, 110, 58)
    rgb *= (1 - 0.12 * ribs)[..., None]
    save_rgba(out / "van.png", np.clip(rgb, 0, 1), a)

    # A street lamp's head from above, 48 x 48: a ring round a hot lens.
    S = 48
    x, y = grid(S, S)
    rgb = np.ones((S, S, 3)) * np.array((1.0, 0.85, 0.6))
    a = disc(x, y, 24, 24, 20)
    rgb = rgb * (0.3 + 0.7 * disc(x, y, 24, 24, 14)[..., None])
    save_rgba(out / "lamp.png", np.clip(rgb, 0, 1), a)

    # The rotor's shadow, 512 x 512: two long blades, soft.
    S = 512
    x, y = grid(S, S)
    blades = (np.clip(1 - np.abs(y - 256) / 9, 0, 1) + np.clip(1 - np.abs(x - 256) / 9, 0, 1)).clip(0, 1)
    blades *= disc(x, y, 256, 256, 250, 20)
    save_rgba(out / "rotor.png", np.zeros((S, S, 3)), blades * 0.8)
# ── 108 Orbital Station ─────────────────────────────────────────────────
# A station seen side on (a spine of modules between two solar wings), two
# kinds of fighter facing +x, a missile, rock chunks, and the shield
# bubble's rim. The planet is the showcase's own (`env/planet.png`).

def os108_orbital():
    out = OUT / "orbital"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(108)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def box(x, y, x0, y0, x1, y1, soft=0.8):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def disc(x, y, cx, cy, r, soft=1.2):
        return np.clip((r - np.sqrt((x - cx) ** 2 + (y - cy) ** 2)) / soft + 0.5, 0, 1)

    def put(rgb, a, mask, col):
        rgb[:] = rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]
        return np.maximum(a, mask)

    # The station, 640 x 300: a long spine, modules (cylinders seen side on),
    # a docking ring, two solar wings of cells, windows.
    W_, H_ = 640, 300
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    hgt = np.zeros((H_, W_))
    g = fbm_rect(W_, H_, 80, 2)
    for wx0, wx1 in ((20, 220), (420, 620)):
        for row, (y0, y1) in enumerate(((20, 120), (180, 280))):
            panel = box(x, y, wx0, y0, wx1, y1, 1.0)
            cells = ((np.abs(((x - wx0) % 20) - 10) < 9) & (np.abs(((y - y0) % 20) - 10) < 9)).astype(float) * panel
            a = put(rgb, a, panel, (0.08, 0.1, 0.14))
            rgb[:] = rgb * (1 - cells[..., None]) + np.array((0.12, 0.22, 0.42)) * cells[..., None] * (0.8 + 0.4 * g[..., None])
            hgt = np.maximum(hgt, panel * 0.3)
        strut = box(x, y, (wx0 + wx1) / 2 - 4, 120, (wx0 + wx1) / 2 + 4, 180, 1.0)
        a = put(rgb, a, strut, (0.45, 0.45, 0.48))
    spine = box(x, y, 200, 140, 440, 160, 1.5)
    a = put(rgb, a, spine, (0.55, 0.55, 0.58))
    for cx, w, h in ((250, 70, 60), (330, 80, 76), (400, 60, 52)):
        m = box(x, y, cx - w / 2, 150 - h / 2, cx + w / 2, 150 + h / 2, 3.0)
        cyl = np.clip(1 - np.abs(y - 150) / (h / 2), 0, 1)
        a = put(rgb, a, m, (0.72, 0.72, 0.74))
        rgb[:] = rgb * (1 - m[..., None] * (1 - (0.55 + 0.45 * np.sqrt(cyl))[..., None]))
        hgt = np.maximum(hgt, m * (0.4 + 0.6 * np.sqrt(cyl)))
        for wy in range(int(150 - h / 2 + 10), int(150 + h / 2 - 6), 10):
            for wxp in range(int(cx - w / 2 + 8), int(cx + w / 2 - 4), 12):
                win = box(x, y, wxp, wy, wxp + 5, wy + 3, 0.6) * (rng.random() < 0.6)
                rgb[:] = rgb * (1 - win[..., None]) + np.array((1.0, 0.85, 0.5)) * win[..., None]
    ring = np.clip(1 - np.abs(np.sqrt(((x - 330) / 26) ** 2 + ((y - 150) / 90) ** 2) - 1) * 10, 0, 1)
    a = put(rgb, a, ring, (0.6, 0.6, 0.62))
    hgt = np.maximum(hgt, ring * 0.7)
    rgb *= (0.85 + 0.25 * g[..., None])
    save_rgba(out / "station.png", np.clip(rgb, 0, 1), a)
    save_rgba(out / "station_n.png", normals_from_height(hgt + 0.05 * g, 4.0, wrap=False), a * 0.6)

    # Fighters, 72 x 48, nose to +x: a blue dart and a red crescent.
    W_, H_ = 72, 48
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    dart = np.clip(1 - (np.abs(y - 24) / (20 * (1 - (x - 4) / 66).clip(0, 1) + 0.5)), 0, 1) * (x > 4) * (x < 70)
    a = put(rgb, a, np.clip(dart * 6, 0, 1), (0.55, 0.62, 0.75))
    a = put(rgb, a, box(x, y, 30, 21, 56, 27, 1.0), (0.2, 0.75, 1.0))           # canopy
    a = put(rgb, a, box(x, y, 2, 16, 10, 32, 1.0), (0.3, 0.3, 0.35))            # engine
    save_rgba(out / "dart.png", np.clip(rgb, 0, 1), a)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    cres = np.clip((disc(x, y, 30, 24, 22) - disc(x, y, 16, 24, 20)) * 3, 0, 1)
    a = put(rgb, a, cres, (0.7, 0.25, 0.2))
    a = put(rgb, a, disc(x, y, 40, 24, 7), (1.0, 0.5, 0.2))                    # the eye
    a = put(rgb, a, box(x, y, 4, 18, 18, 30, 1.0), (0.3, 0.25, 0.25))
    save_rgba(out / "crescent.png", np.clip(rgb, 0, 1), a)
    # A missile, 36 x 10.
    W_, H_ = 36, 10
    x, y = grid(W_, H_)
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    a = put(rgb, a, box(x, y, 2, 3, 30, 7, 1.0), (0.8, 0.8, 0.82))
    a = put(rgb, a, disc(x, y, 30, 5, 3), (0.9, 0.3, 0.2))
    a = put(rgb, a, box(x, y, 2, 0, 8, 10, 1.0), (0.4, 0.4, 0.42))
    save_rgba(out / "missile.png", np.clip(rgb, 0, 1), a)
    # Rock, 96 x 96, three shapes in a row (one sheet, picked by uv).
    for k in range(3):
        S = 96
        x, y = grid(S, S)
        ang = np.arctan2(y - 48, x - 48)
        r = np.sqrt((x - 48) ** 2 + (y - 48) ** 2)
        edge = 37 + 3.5 * np.sin(ang * (2 + k) + k) + 2.5 * np.sin(ang * 5 + 2 * k) + 1.5 * np.sin(ang * 11 + k)
        a = np.clip((edge - r) / 1.5 + 0.5, 0, 1)
        lump = fbm_rect(S, S, 8 + 2 * k, 3)
        hgt = np.clip(1 - r / edge, 0, 1) ** 0.5 + 0.3 * lump
        shade = 0.28 + 0.15 * lump
        rgb = np.stack([shade, shade * 0.95, shade * 0.9], axis=-1)
        save_rgba(out / f"rock{k}.png", np.clip(rgb, 0, 1), a)
        save_rgba(out / f"rock{k}_n.png", normals_from_height(hgt, 4.0, wrap=False), a * 0.3)
    # The shield bubble, 512 x 320: a soft rim, faint inside.
    W_, H_ = 512, 320
    x, y = grid(W_, H_)
    e = np.sqrt(((x - 256) / 250) ** 2 + ((y - 160) / 154) ** 2)
    rim = np.clip(1 - np.abs(e - 0.97) / 0.06, 0, 1) ** 2
    inner = np.clip(1 - e, 0, 1) * 0.12 * (e < 1)
    hexes = (np.abs(np.sin(x * 0.12) * np.sin(y * 0.14 + x * 0.06)) > 0.92) * (e < 1) * 0.15
    save_rgba(out / "shield.png", np.ones((H_, W_, 3)) * np.array((0.55, 0.85, 1.0)), np.clip(rim + inner + hexes, 0, 1))

    # The planet, 1024 px: the showcase's `planet()` looks its noise up per
    # pixel without interpolation, which is blocky this big - so read it
    # bilinearly and soften every edge.
    size = 1024
    nx, ny, r = radius(size)
    inside = r < 1.0
    nz = np.sqrt(np.clip(1.0 - r * r, 0.0, 1.0))
    lon = np.arctan2(nx, np.maximum(nz, 1e-4))
    latr = np.arcsin(np.clip(-ny, -1.0, 1.0))

    def bilin(g, fx, fy):
        n = g.shape[0]
        x0 = np.floor(fx).astype(int)
        y0 = np.floor(fy).astype(int)
        tx, ty = fx - x0, fy - y0
        x0, y0 = x0 % n, y0 % n
        x1, y1 = (x0 + 1) % n, (y0 + 1) % n
        return ((g[y0, x0] * (1 - tx) + g[y0, x1] * tx) * (1 - ty)
                + (g[y1, x0] * (1 - tx) + g[y1, x1] * tx) * ty)

    fx = (lon / np.pi * 0.5 + 0.5) * 511.0
    fy = (latr / np.pi + 0.5) * 511.0
    land = bilin(fbm(512, 5, 6), fx, fy)
    lat = np.abs(latr) / (np.pi / 2)
    ocean = np.array([0.06, 0.17, 0.38])
    deep = np.array([0.03, 0.09, 0.24])
    shore = np.array([0.62, 0.56, 0.38])
    green = np.array([0.20, 0.34, 0.15])
    dry = np.array([0.45, 0.38, 0.22])
    ice = np.array([0.88, 0.92, 0.96])
    t_land = smoothstep(0.515, 0.53, land)
    sea = deep + (ocean - deep) * smoothstep(0.35, 0.52, land)[..., None]
    ground = green + (dry - green) * smoothstep(0.6, 0.72, land)[..., None]
    rgb = sea * (1 - t_land[..., None]) + ground * t_land[..., None]
    band = np.clip(1 - np.abs(land - 0.522) / 0.01, 0, 1) * 0.6
    rgb = rgb * (1 - band[..., None]) + shore * band[..., None]
    t_ice = smoothstep(0.7, 0.76, lat + (land - 0.5) * 0.5)
    rgb = rgb * (1 - t_ice[..., None]) + ice * t_ice[..., None]
    clouds = smoothstep(0.52, 0.72, bilin(fbm(512, 8, 5), fx + 73.0, fy))
    rgb = rgb * (1.0 - clouds[..., None] * 0.75) + clouds[..., None] * 0.92
    alpha = smoothstep(1.0, 0.992, r)
    save_rgba(out / "planet.png", np.clip(rgb, 0, 1), alpha)
    n = np.stack([nx, -ny, nz], axis=-1)
    bump = normals_from_height(land * t_land + 0.52 * (1 - t_land), 1.5, wrap=False) * 2.0 - 1.0
    n = n + np.stack([bump[..., 0], bump[..., 1], np.zeros_like(nz)], axis=-1) * 0.12
    n = n / np.maximum(np.linalg.norm(n, axis=-1, keepdims=True), 1e-6)
    save_rgba(out / "planet_n.png", np.where(inside[..., None], n * 0.5 + 0.5, 0.5), alpha)
# ── 109 Mirror Maze ─────────────────────────────────────────────────────
# A carnival hall of mirrors seen from above: a chequered marble floor, the
# gilt frame each mirror wall sits in, and the brass laser that bounces round
# the room. The visitors are the Garden Party's guests, and the room seen
# through the portal is that garden.

def mm109_mirrors():
    out = OUT / "mirrors"
    out.mkdir(parents=True, exist_ok=True)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    # The floor, 512 x 512: four marble squares, black and cream, veined,
    # with a groove between them.
    S = 512
    x, y = grid(S, S)
    cell = ((x // 256) + (y // 256)) % 2
    veins = fbm_rect(S, S, 6, 5)
    vein = np.clip(1 - np.abs(np.sin((veins * 9 + x / 90 + y / 140))) * 6, 0, 1) ** 2
    cream = np.array([0.86, 0.82, 0.74]) * (0.92 + 0.12 * veins[..., None]) - vein[..., None] * 0.12
    black = np.array([0.07, 0.07, 0.09]) * (0.9 + 0.3 * veins[..., None]) + vein[..., None] * np.array([0.18, 0.16, 0.14])
    rgb = np.where(cell[..., None] > 0.5, cream, black)
    gx, gy = np.minimum(x % 256, 256 - x % 256), np.minimum(y % 256, 256 - y % 256)
    groove = np.clip(1.5 - np.minimum(gx, gy), 0, 1)
    rgb = rgb * (1 - groove[..., None] * 0.6)
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((S, S)))
    hgt = 1 - groove * 0.8 + veins * 0.04
    save_rgba(out / "floor_n.png", normals_from_height(hgt, 3.0, wrap=True), np.ones((S, S)) * 0.85)

    # The frame, 512 x 32: a gilt moulding, tiled along every mirror wall.
    W_, H_ = 512, 32
    x, y = grid(W_, H_)
    t = y / H_
    profile = 0.5 + 0.5 * np.cos((t - 0.5) * np.pi * 2.2)
    beads = 0.5 + 0.5 * np.cos(x / 6.0) * (np.abs(t - 0.5) < 0.12)
    gold = np.array([0.78, 0.58, 0.24])
    rgb = gold[None, None, :] * (0.55 + 0.5 * profile[..., None]) + beads[..., None] * 0.08
    save_rgba(out / "frame.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))
    save_rgba(out / "frame_n.png", normals_from_height(profile * 0.8 + beads * 0.2, 4.0, wrap=False), np.ones((H_, W_)))

    # The laser, 64 x 64 from above: a brass drum with a lens to +x.
    S = 64
    x, y = grid(S, S)
    r = np.sqrt((x - 32) ** 2 + (y - 32) ** 2)
    drum = np.clip(22 - r, 0, 1)
    barrel = np.clip(1 - np.maximum(np.abs(y - 32) - 6, 0), 0, 1) * np.clip(1 - np.maximum(x - 60, 0), 0, 1) * (x > 32)
    a = np.maximum(drum, barrel)
    shade = 0.65 + 0.35 * np.clip(1 - r / 22, 0, 1) ** 0.5
    rgb = np.array([0.72, 0.55, 0.28])[None, None, :] * shade[..., None]
    lens = np.clip(4.5 - np.sqrt((x - 58) ** 2 + (y - 32) ** 2), 0, 1)
    rgb = rgb * (1 - lens[..., None]) + np.array([1.0, 0.3, 0.3]) * lens[..., None]
    save_rgba(out / "laser.png", np.clip(rgb, 0, 1), a)
    hgt = np.clip(1 - r / 22, 0, 1) ** 0.5 * drum + barrel * 0.6
    save_rgba(out / "laser_n.png", normals_from_height(hgt, 4.0, wrap=False), a)
# ── 110 Harbour Storm ───────────────────────────────────────────────────
# A lighthouse on a rocky headland, side on: a white tower banded red, a
# lantern room of glass at the top (left clear - the light is drawn), and
# the dark wet rock it stands on. The boats, pier and crates are the
# showcase's harbour's, the sky and clouds the storm's.

def hs110_storm():
    out = OUT / "storm"
    out.mkdir(parents=True, exist_ok=True)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    # The lighthouse, 160 x 440: a tapering tower, two red bands, a gallery,
    # the lantern room and a dome.
    W_, H_ = 160, 440
    x, y = grid(W_, H_)
    cx = 80.0
    t = (y - 110) / (H_ - 110)               # 0 at the gallery, 1 at the foot
    half = 30 + 22 * np.clip(t, 0, 1)
    tower = np.clip(half - np.abs(x - cx), 0, 1) * (y > 110)
    round_ = np.sqrt(np.clip(1 - ((x - cx) / np.maximum(half, 1)) ** 2, 0, 1))
    rgb = np.zeros((H_, W_, 3))
    a = np.zeros((H_, W_))
    white = np.array([0.88, 0.86, 0.82])
    red = np.array([0.62, 0.12, 0.1])
    band = (((y - 110) // 70) % 2 == 1)
    col = np.where(band[..., None], red, white) * (0.45 + 0.55 * round_[..., None])
    rgb = rgb * (1 - tower[..., None]) + col * tower[..., None]
    a = np.maximum(a, tower)
    # The gallery: a dark rail across.
    gallery = ((y > 100) & (y < 112) & (np.abs(x - cx) < 46)).astype(float)
    rgb = rgb * (1 - gallery[..., None]) + np.array([0.12, 0.12, 0.13]) * gallery[..., None]
    a = np.maximum(a, gallery)
    # The lantern room: thin dark mullions over clear glass (alpha 0.25).
    room = ((y > 46) & (y <= 100) & (np.abs(x - cx) < 30)).astype(float)
    mull = room * ((np.abs(((x - cx + 30) % 15) - 7.5) > 6.0) | (y > 96) | (y < 50)).astype(float)
    a = np.maximum(a, room * 0.18)
    rgb = rgb * (1 - mull[..., None]) + np.array([0.1, 0.1, 0.11]) * mull[..., None]
    a = np.maximum(a, mull)
    dome = np.clip(1 - np.sqrt(((x - cx) / 34) ** 2 + ((y - 46) / 30) ** 2), 0, 1) > 0
    dome = dome & (y <= 46)
    rgb[dome] = np.array([0.15, 0.14, 0.14])
    a[dome] = 1.0
    save_rgba(out / "lighthouse.png", np.clip(rgb, 0, 1), a)
    hgt = round_ * tower + gallery * 0.8 + mull * 0.6 + dome * 0.7
    save_rgba(out / "lighthouse_n.png", normals_from_height(hgt, 3.0, wrap=False), a)

    # The headland, 700 x 300: a heap of wet dark rock, flat enough on top
    # for the tower, ragged where the sea meets it.
    W_, H_ = 700, 300
    x, y = grid(W_, H_)
    n1 = fbm_rect(W_, H_, 10, 5)
    ridge = 60 + 40 * np.clip(1 - np.abs(x - 470) / 300, 0, 1) - 30 * (x < 200) * (1 - x / 200) \
        + 26 * (fbm_rect(W_, 1, 18, 4)[0][None, :] - 0.5)
    top = np.clip(ridge, 10, None)
    rock = np.clip((y - (H_ - top - 140 * np.clip((x - 120) / 580, 0, 1))) / 2.0, 0, 1)
    rock = np.clip(rock + (y > H_ - 60) * 1.0, 0, 1)
    shade = 0.3 + 0.2 * n1
    rgb = np.stack([shade * 0.9, shade * 0.92, shade], axis=-1)
    wet = np.clip((y - (H_ - 90)) / 90, 0, 1)
    rgb = rgb * (1 - 0.3 * wet[..., None]) + np.array([0.02, 0.05, 0.07]) * 0.3 * wet[..., None]
    save_rgba(out / "headland.png", np.clip(rgb, 0, 1), rock)
    save_rgba(out / "headland_n.png", normals_from_height(n1 * 1.5 + rock * 0.5, 5.0, wrap=False), rock)
# ── 111 Arena Boss ──────────────────────────────────────────────────────
# A stone golem in pieces, each lying along +x (a bone's length) so a bone
# carries it: rough rock with a normal map, and a second image of the same
# piece holding only its glowing cracks. The arena's floor (carved apart in
# the fight) and the ruined colonnade behind it.

def ab111_arena():
    out = OUT / "arena"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(111)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    def rock(name, w, h, round_=0.35, seed=0):
        """A rough slab w x h along +x: its outline wobbles, its face is
        fbm-lumpy, and cracks run through it - drawn dark in the rock and
        bright in the `_glow` image."""
        x, y = grid(w, h)
        u, v = x / w, y / h
        n = fbm_rect(w, h, 6 + seed % 3, 4)
        edge_x = np.minimum(u, 1 - u) * w
        edge_y = np.minimum(v, 1 - v) * h
        wob = (fbm_rect(w, h, 3, 2) - 0.5) * 10
        r = min(w, h) * round_
        dx = np.clip(r - edge_x, 0, None)
        dy = np.clip(r - edge_y, 0, None)
        inside = r - np.sqrt(dx ** 2 + dy ** 2) + wob
        a = np.clip(inside / 2.0 + 0.5, 0, 1)
        cracks = np.zeros((h, w))
        for _ in range(2):
            y0 = rng.uniform(0.25, 0.75) * h
            slope = rng.uniform(-0.4, 0.4)
            jag = np.sin(x * rng.uniform(0.05, 0.12) + rng.uniform(0, 6)) * 6 + (fbm_rect(w, 1, 8, 2)[0][None, :] - 0.5) * 10
            line = y0 + slope * (x - w / 2) + jag
            cracks = np.maximum(cracks, np.clip(1.1 - np.abs(y - line), 0, 1) * (u > 0.12) * (u < 0.88))
        shade = 0.42 + 0.22 * n
        rgb = np.stack([shade * 0.95, shade * 0.9, shade * 0.85], axis=-1)
        rgb = rgb * (1 - cracks[..., None] * 0.75)
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), a)
        hgt = np.clip(inside / r, 0, 1) ** 0.6 + n * 0.5 - cracks * 0.6
        save_rgba(out / f"{name}_n.png", normals_from_height(hgt, 4.0, wrap=False), a)
        glow = np.clip(cracks * 1.2, 0, 1) * a
        save_rgba(out / f"{name}_glow.png", np.ones((h, w, 3)), glow)

    rock("torso", 150, 132, 0.3, 1)
    rock("pelvis", 70, 96, 0.35, 2)
    rock("head", 76, 70, 0.4, 3)
    rock("upper", 104, 50, 0.45, 4)
    rock("fore", 116, 62, 0.42, 5)
    rock("thigh", 84, 52, 0.45, 6)
    rock("shin", 90, 58, 0.42, 7)

    # The floor: flagstones over packed earth, 1920 x 260 - one image, not a
    # tile, since its copy is what the slams carve.
    W_, H_ = 1920, 260
    x, y = grid(W_, H_)
    n = fbm_rect(W_, H_, 40, 4)
    top = 30
    flags = (y < top).astype(float)
    joint = ((np.abs(((x + (y // 14 % 2) * 32) % 64) - 32) > 30) | (np.abs(y % 14 - 7) > 6)) & (y < top)
    stone = np.array([0.55, 0.52, 0.48]) * (0.8 + 0.3 * n[..., None])
    earth = np.array([0.32, 0.25, 0.19]) * (0.7 + 0.5 * n[..., None])
    rgb = np.where(flags[..., None] > 0.5, stone, earth)
    rgb = rgb * (1 - joint[..., None] * 0.5)
    pebbles = (fbm_rect(W_, H_, 150, 2) > 0.68) & (y > top)
    rgb[pebbles] *= 1.35
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((H_, W_)))

    # The colonnade: broken arches in silhouette, 1024 x 420, alpha.
    W_, H_ = 1024, 420
    x, y = grid(W_, H_)
    a = np.zeros((H_, W_))
    for i in range(8):
        cx = 64 + i * 128
        broken = rng.uniform(0, 1) < 0.35
        hgt = rng.uniform(250, 330) if not broken else rng.uniform(120, 220)
        pillar = (np.abs(x - cx) < 22) & (y > H_ - hgt)
        a = np.maximum(a, pillar.astype(float))
        if i < 7 and not broken:
            ax = cx + 64
            arch_r = np.sqrt((x - ax) ** 2 + ((y - (H_ - hgt + 40)) * 1.2) ** 2)
            arch = (arch_r > 50) & (arch_r < 76) & (y < H_ - hgt + 40)
            a = np.maximum(a, arch.astype(float))
    n = fbm_rect(W_, H_, 10, 3)
    rgb = np.stack([0.32 + 0.1 * n, 0.3 + 0.1 * n, 0.32 + 0.1 * n], axis=-1)
    save_rgba(out / "colonnade.png", np.clip(rgb, 0, 1), a)
# ── 112 Haunted Manor ───────────────────────────────────────────────────
# Four rooms' back walls, side on, 1280 x 720 each - the entrance hall
# (panelled, portraits), the library (shelves of books), the ballroom (tall
# moonlit windows) and the chapel (stone arches, a rose window) - with
# normal maps, floorboards, and a ghost: a pale wisp that fades to nothing.

def hm112_manor():
    out = OUT / "manor"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(112)

    def grid(w, h):
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        return x, y

    W_, H_ = 1280, 720
    x, y = grid(W_, H_)
    n = fbm_rect(W_, H_, 24, 4)

    def box(x0, y0, x1, y1, soft=1.0):
        return (np.clip((x - x0) / soft + 0.5, 0, 1) * np.clip((x1 - x) / soft + 0.5, 0, 1)
                * np.clip((y - y0) / soft + 0.5, 0, 1) * np.clip((y1 - y) / soft + 0.5, 0, 1))

    def paint(rgb, mask, col):
        return rgb * (1 - mask[..., None]) + np.array(col) * mask[..., None]

    def save(name, rgb, hgt):
        save_rgba(out / f"{name}.png", np.clip(rgb * (0.88 + 0.2 * n[..., None]), 0, 1), np.ones((H_, W_)))
        save_rgba(out / f"{name}_n.png", normals_from_height(hgt + 0.05 * n, 3.0, wrap=False), np.ones((H_, W_)))

    # The hall: wallpaper above, panelling below, a dado rail, three portraits.
    rgb = np.zeros((H_, W_, 3)) + np.array([0.36, 0.14, 0.12])
    stripes = (np.sin(x * 0.18) > 0.6).astype(float) * (y < 430)
    rgb = paint(rgb, stripes * 0.5, (0.45, 0.2, 0.16))
    hgt = np.zeros((H_, W_))
    panel = (y >= 430).astype(float)
    rgb = paint(rgb, panel, (0.3, 0.18, 0.1))
    for i in range(8):
        p = box(20 + i * 160, 460, 140 + i * 160, 680, 2.0)
        hgt += p * 0.5
        rgb = paint(rgb, p * 0.35, (0.38, 0.24, 0.13))
    rail = box(0, 420, W_, 438)
    rgb = paint(rgb, rail, (0.42, 0.28, 0.14))
    hgt += rail * 0.8
    for cx in (240, 640, 1040):
        frame = box(cx - 90, 120, cx + 90, 360, 1.5) - box(cx - 74, 136, cx + 74, 344, 1.5)
        canvas = box(cx - 74, 136, cx + 74, 344, 1.5)
        face = np.clip(1 - np.sqrt(((x - cx) / 40) ** 2 + ((y - 220) / 52) ** 2), 0, 1) > 0
        rgb = paint(rgb, canvas, (0.12, 0.1, 0.08))
        rgb = paint(rgb, face * canvas * 0.8, (0.55, 0.45, 0.36))
        rgb = paint(rgb, frame, (0.6, 0.45, 0.18))
        hgt += frame * 1.0
    save("hall", rgb, hgt)

    # The library: shelves floor to ceiling, books of every colour.
    rgb = np.zeros((H_, W_, 3)) + np.array([0.18, 0.11, 0.06])
    hgt = np.zeros((H_, W_))
    for row in range(5):
        y0 = 60 + row * 128
        shelf = box(0, y0 + 112, W_, y0 + 126)
        rgb = paint(rgb, shelf, (0.3, 0.19, 0.1))
        hgt += shelf * 0.6
        bx = 0
        while bx < W_:
            w = rng.integers(14, 30)
            h = rng.integers(70, 108)
            col = rng.choice([(0.42, 0.1, 0.08), (0.12, 0.22, 0.12), (0.12, 0.14, 0.3), (0.45, 0.32, 0.12),
                              (0.25, 0.08, 0.2), (0.32, 0.26, 0.2)])
            if rng.random() > 0.08:
                b = box(bx + 1, y0 + 112 - h, bx + w - 1, y0 + 112, 1.0)
                rgb = paint(rgb, b, np.array(col) * rng.uniform(0.8, 1.2))
                band = b * ((np.abs(y - (y0 + 112 - h * 0.75)) < 3) | (np.abs(y - (y0 + 112 - h * 0.25)) < 3))
                rgb = paint(rgb, band, (0.65, 0.5, 0.2))
                hgt += b * 0.5
            bx += w
    save("library", rgb, hgt)

    # The ballroom: pale panelled walls, three tall arched windows of night.
    rgb = np.zeros((H_, W_, 3)) + np.array([0.5, 0.52, 0.55])
    hgt = np.zeros((H_, W_))
    for cx in (220, 640, 1060):
        win = box(cx - 110, 120, cx + 110, 620, 1.5) + (np.sqrt(((x - cx) / 110) ** 2 + ((y - 120) / 110) ** 2) < 1) * (y < 120)
        win = np.clip(win, 0, 1)
        glass = np.array([0.08, 0.1, 0.2]) + np.array([0.1, 0.14, 0.25]) * np.clip((620 - y) / 500, 0, 1)[..., None]
        rgb = rgb * (1 - win[..., None]) + glass * win[..., None]
        mull = win * ((np.abs(x - cx) < 4) | (np.abs(((y - 120) % 125) - 62) > 59))
        rgb = paint(rgb, mull, (0.3, 0.3, 0.32))
        frame = np.clip(box(cx - 124, 110, cx + 124, 632, 2.0) - win, 0, 1)
        hgt += frame * 0.8 + mull * 0.5
    save("ballroom", rgb, hgt)

    # The chapel: stone blocks, two pointed arches and a rose window.
    blocks = ((np.abs(((x + (y // 48 % 2) * 64) % 128) - 64) > 62) | (np.abs(y % 48 - 24) > 22)).astype(float)
    rgb = np.zeros((H_, W_, 3)) + np.array([0.4, 0.38, 0.36]) * (0.85 + 0.25 * fbm_rect(W_, H_, 40, 3)[..., None])
    rgb = rgb * (1 - blocks[..., None] * 0.45)
    hgt = 1 - blocks * 0.6
    for cx in (230, 1050):
        arch = box(cx - 90, 260, cx + 90, 700, 1.5) * 1.0
        point = (np.abs(x - cx) < 90) & (y < 260) & (y > 260 - (90 - np.abs(x - cx)) * 1.4)
        a = np.clip(arch + point, 0, 1)
        rgb = paint(rgb, a, (0.06, 0.05, 0.06))
        hgt -= a * 0.5
    r = np.sqrt((x - 640) ** 2 + (y - 260) ** 2)
    ang = np.arctan2(y - 260, x - 640)
    rose = (r < 150).astype(float)
    petals = 0.5 + 0.5 * np.cos(ang * 8)
    hue = np.stack([0.5 + 0.4 * np.cos(ang * 4), 0.25 + 0.2 * np.sin(ang * 3 + r * 0.03), 0.5 + 0.4 * np.sin(ang * 4)], -1)
    rgb = rgb * (1 - rose[..., None]) + hue * (0.35 + 0.4 * petals[..., None]) * rose[..., None]
    lead = rose * ((np.abs(((ang + np.pi) % (np.pi / 4)) - np.pi / 8) < 0.02 * 150 / np.maximum(r, 1)) | (np.abs(r - 75) < 3))
    rgb = paint(rgb, lead, (0.08, 0.08, 0.08))
    hgt += np.clip(1 - np.abs(r - 152) / 6, 0, 1)
    save("chapel", rgb, hgt)
    # The rose window again, alone, for an unlit glow over the wall.
    save_rgba(out / "rose.png", np.clip(hue * (0.5 + 0.5 * petals[..., None]), 0, 1)[60:460, 440:840],
              (rose * (1 - lead))[60:460, 440:840])

    # Floorboards, 512 x 128, tiled.
    W2, H2 = 512, 128
    x2, y2 = grid(W2, H2)
    g = fbm_rect(W2, H2, 30, 3)
    board = (np.abs(y2 % 32 - 16) > 15) | (np.abs(((x2 + (y2 // 32) * 97) % 256) - 128) > 127)
    rgb = np.stack([0.32 + 0.1 * g, 0.21 + 0.07 * g, 0.12 + 0.05 * g], -1) * (1 - board[..., None] * 0.5)
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((H2, W2)))
    save_rgba(out / "floor_n.png", normals_from_height(1 - board * 0.7 + g * 0.1, 3.0, wrap=True), np.ones((H2, W2)))

    # The ghost: a hooded wisp, 120 x 220, fading to nothing at its tail.
    W3, H3 = 120, 220
    x3, y3 = grid(W3, H3)
    t = y3 / H3
    half = 30 + 18 * np.sin(t * np.pi * 0.9) + 6 * np.sin(y3 * 0.12)
    body = np.clip((half - np.abs(x3 - 60)) / 8, 0, 1)
    head = np.clip(1 - np.sqrt(((x3 - 60) / 30) ** 2 + ((y3 - 40) / 34) ** 2), 0, 1) ** 0.3
    a = np.clip(np.maximum(body * np.clip(1.25 - t, 0, 1) ** 1.5, head), 0, 1)
    eyes = (np.sqrt(((x3 - 49) / 5) ** 2 + ((y3 - 42) / 7) ** 2) < 1) | (np.sqrt(((x3 - 71) / 5) ** 2 + ((y3 - 42) / 7) ** 2) < 1)
    rgb = np.ones((H3, W3, 3)) * np.array([0.8, 0.9, 1.0])
    rgb[eyes] = (0.05, 0.05, 0.1)
    save_rgba(out / "ghost.png", rgb, a * 0.85)
# ── 113 Falling Sands ───────────────────────────────────────────────────
# The cave as a seed picture for its `pixel_sim`: every pixel the key colour
# of a material (`ecs::pixel_sim::MATERIALS`) - stone walls, a ledge of sand,
# a wooden bridge, a pool of water, a pocket of oil, a pool of lava - and the
# dark back wall the empty cells show.

def fs113_sands():
    out = OUT / "sands"
    out.mkdir(parents=True, exist_ok=True)
    W_, H_ = 480, 270
    y, x = np.mgrid[0:H_, 0:W_].astype(np.float64) + 0.5
    n = fbm_rect(W_, H_, 14, 4)
    n2 = fbm_rect(W_, H_, 30, 3)
    STONE, SAND, WATER, OIL, LAVA, WOOD = (128, 128, 128), (230, 200, 110), (40, 110, 230), (70, 40, 20), (255, 60, 0), (130, 80, 40)
    img = np.zeros((H_, W_, 4), dtype=np.uint8)

    def put(mask, col):
        img[mask] = list(col) + [255]

    # The rock: thick walls, a ragged ceiling with two cracks, a floor that
    # rises in the middle, and shelves.
    wob = (n - 0.5) * 30
    wall = (x < 26 + wob) | (x > W_ - 26 - wob) | (y > H_ - 18 + (n2 - 0.5) * 14)
    ceiling = (y < 14 + (n - 0.5) * 16) & ~((np.abs(x - 90) < 8) | (np.abs(x - 250) < 6))
    shelf_l = (y > 96 + wob * 0.3) & (y < 110 + wob * 0.3) & (x < 170)
    basin = ((y > 205) & ((np.abs(x - 150) < 6) | (np.abs(x - 300) < 6))) | ((y > 248) & (x > 140) & (x < 310))
    bowl = ((y > 225) & ((np.abs(x - 352) < 6))) | ((y > 252) & (x > 345))
    shelf_r = (y > 150) & (y < 160) & (x > 320) & (x < 460)
    lip = (y > 135) & (y < 160) & (np.abs(x - 322) < 5)
    put(wall | ceiling | shelf_l | basin | bowl | shelf_r | lip, STONE)
    # What is loose or wet, in its places.
    put((y > 70 + (n - 0.5) * 30) & (y <= 96 + wob * 0.3) & (x > 24) & (x < 150) & (img[..., 3] == 0), SAND)
    put((y > 215) & (y <= 248) & (x > 156) & (x < 294) & (img[..., 3] == 0), WATER)
    put((y > 138) & (y <= 150) & (x > 327) & (x < 455) & (img[..., 3] == 0), OIL)
    put((y > 238) & (x > 358) & (x < W_ - 20) & (img[..., 3] == 0), LAVA)
    put((y > 160) & (y < 165) & (x > 180) & (x < 300), WOOD)
    put((y > 165) & (y < 205) & ((np.abs(x - 186) < 3) | (np.abs(x - 294) < 3)), WOOD)
    from PIL import Image
    # Outside sprites/: the build packs and compresses that folder, and a
    # seed is read by its exact colours.
    maps = OUT.parent / "maps"
    maps.mkdir(parents=True, exist_ok=True)
    Image.fromarray(img, "RGBA").save(maps / "cave_seed.png")
    print(f"wrote {maps / 'cave_seed.png'}  {W_}x{H_}")

    # The back wall: dark rock, lit by what burns.
    W2, H2 = 960, 540
    b = fbm_rect(W2, H2, 16, 5)
    rgb = np.stack([0.16 + 0.1 * b, 0.14 + 0.09 * b, 0.13 + 0.08 * b], -1)
    save_rgba(out / "backwall.png", rgb, np.ones((H2, W2)))
    save_rgba(out / "backwall_n.png", normals_from_height(b, 4.0, wrap=False), np.ones((H2, W2)))


# ── 114 Smoke Chamber ───────────────────────────────────────────────────
# What stands in the smoke: a fan's blades and its ring, an iron weight on
# a chain, and a steam pipe - each with a normal map, so the lamps model
# them as they light the smoke round them.

def sc114_chamber():
    out = OUT / "smoke"
    out.mkdir(parents=True, exist_ok=True)
    # Four blades, turned by the script.
    S = 256
    nx, ny, r = radius(S)
    ang = np.arctan2(ny, nx)
    blade = np.cos((ang * 4.0) % (2 * np.pi) - 0.5)
    shape = (blade > 0.55 - 0.25 * r) & (r < 0.96) & (r > 0.12)
    hub = r < 0.2
    alpha = np.clip(shape | hub, 0, 1).astype(np.float64)
    alpha = np.maximum(alpha * smoothstep(1.0, 0.95, r), smoothstep(0.21, 0.19, r))
    height = np.where(hub, 1.0 - r * 3.0, blade * 0.6) + fbm(S, 16, 3) * 0.08
    shade = 0.35 + 0.25 * blade
    rgb = np.stack([shade * 0.9, shade * 0.88, shade * 0.85], -1)
    rgb[hub] = [0.55, 0.45, 0.3]
    save_rgba(out / "fan.png", rgb, alpha)
    save_rgba(out / "fan_n.png", normals_from_height(height, 3.0, wrap=False), alpha)
    # The fan's ring and its spokes.
    ring_a = smoothstep(0.86, 0.88, r) * smoothstep(1.0, 0.98, r)
    spoke = ((np.abs(nx) < 0.04) | (np.abs(ny) < 0.04)) & (r < 0.9)
    a = np.maximum(ring_a, spoke.astype(np.float64))
    n = fbm(S, 20, 3)
    rgb = np.stack([0.3 + 0.1 * n, 0.28 + 0.09 * n, 0.26 + 0.08 * n], -1)
    h = np.maximum(np.sin(np.clip((r - 0.86) / 0.14, 0, 1) * np.pi), spoke * 0.5)
    save_rgba(out / "fan_ring.png", rgb, a)
    save_rgba(out / "fan_ring_n.png", normals_from_height(h, 4.0, wrap=False), a)
    # The weight: an iron ball, pitted.
    nz = np.sqrt(np.clip(1.0 - r * r, 0.0, 1.0))
    a = smoothstep(1.0, 0.97, r)
    pit = fbm(S, 24, 4)
    v = 0.22 + 0.12 * pit
    rgb = np.stack([v, v * 0.96, v * 0.92], -1)
    save_rgba(out / "bob.png", rgb, a)
    base = np.stack([nx, -ny, np.maximum(nz, 0.05)], axis=-1)
    save_rgba(out / "bob_n.png", normals_from_height(pit * 0.4, 2.0, wrap=False, base=base), a)
    # A pipe coming up through the floor, with a grate at its mouth.
    W, H = 96, 256
    y, x = np.mgrid[0:H, 0:W].astype(np.float64)
    u = (x + 0.5) / W * 2 - 1
    band = ((y % 64) < 8).astype(np.float64)
    a = np.ones((H, W))
    n = fbm_rect(W, H, 12, 3)
    v = 0.3 + 0.1 * n + 0.12 * band - 0.15 * np.abs(u) ** 3
    rgb = np.stack([v * 1.05, v * 0.95, v * 0.85], -1)
    save_rgba(out / "pipe.png", rgb, a)
    save_rgba(out / "pipe_n.png", normals_from_height(band * 0.5 + n * 0.1, 3.0, wrap=False, base=cylinder_normal(u)), a)


# ── 115 Rewind Ruins ────────────────────────────────────────────────────
# A pilgrim in a cloak with a staff, four frames of a walk across one
# sheet, so a rewound walk shows its frames going backwards.

def rr115_ruins():
    out = OUT / "ruins"
    out.mkdir(parents=True, exist_ok=True)
    FW, FH, N = 64, 96, 4
    img = np.zeros((FH, FW * N, 4))
    for k in range(N):
        y, x = np.mgrid[0:FH, 0:FW].astype(np.float64) + 0.5
        phase = k / N * 2 * np.pi
        a = np.zeros((FH, FW))
        rgb = np.zeros((FH, FW, 3))

        def blob(mask, col):
            a[mask] = 1.0
            rgb[mask] = col

        stride = np.sin(phase) * 9
        bob = abs(np.cos(phase)) * 2
        # Legs: two strokes from the hip, swinging opposite ways.
        for s, shade in ((1, 0.18), (-1, 0.24)):
            fx = 32 + s * stride
            t = np.clip((y - (58 - bob)) / 34, 0, 1)
            lx = 32 + (fx - 32) * t
            blob((np.abs(x - lx) < 3.2) & (y > 58 - bob) & (y < 93), (shade, shade * 0.9, shade * 0.8))
        # The cloak: a bell from the shoulders, hem swinging with the stride.
        top, hem = 22 - bob, 70 - bob
        t = np.clip((y - top) / (hem - top), 0, 1)
        half = 8 + 13 * t
        cx = 32 + np.sin(phase) * 2 * t
        cloak = (y > top) & (y < hem + np.sin(x * 0.6 + phase) * 1.5) & (np.abs(x - cx) < half)
        n = np.sin(x * 0.9) * 0.03
        a[cloak] = 1.0
        rgb[cloak] = np.stack([0.46 + n, 0.30 + n, np.full_like(n, 0.20)], -1)[cloak]
        # The hood and the face in it.
        hood = (x - 32) ** 2 + (y - (16 - bob)) ** 2 < 10.5 ** 2
        blob(hood, (0.40, 0.26, 0.17))
        face = (x - 35) ** 2 / 4.5 ** 2 + (y - (17 - bob)) ** 2 / 5.5 ** 2 < 1
        blob(face, (0.86, 0.66, 0.50))
        # The staff, planted ahead and swung through.
        sx = 46 + np.sin(phase + 0.6) * 5
        blob((np.abs(x - (sx + (y - 50) * 0.08)) < 1.6) & (y > 8) & (y < 94), (0.55, 0.40, 0.22))
        img[:, k * FW:(k + 1) * FW, :3] = rgb
        img[:, k * FW:(k + 1) * FW, 3] = a
    save_rgba(out / "pilgrim.png", img[..., :3], img[..., 3])


# ── 117 Moor at Midnight ────────────────────────────────────────────────
# Standing stones, the moor they stand on, low hills behind and a crofter's
# cottage with a lit window - each lit by the lantern, so each with a
# normal map.

def mm117_moor():
    out = OUT / "moor"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(117)
    # Three standing stones: rough slabs leaning a little, lichen on them.
    for k in range(3):
        w, h = 96, 224
        y, x = np.mgrid[0:h, 0:w].astype(np.float64) + 0.5
        u = (x / w) * 2 - 1
        v = y / h
        lean = (rng.random() - 0.5) * 0.25
        n = fbm_rect(w, h, 6, 4)
        half = 0.78 - 0.18 * (1 - v) ** 2 + (n - 0.5) * 0.25
        top = 0.06 + 0.08 * np.abs(np.sin(u * 2 + k)) + (n - 0.5) * 0.05
        shape = (np.abs(u - lean * (1 - v)) < half) & (v > top)
        a = shape.astype(np.float64)
        height = n * 0.6 + (1 - np.abs(u)) * 0.4
        g = 0.34 + 0.22 * n
        lichen = (fbm_rect(w, h, 18, 3) > 0.62).astype(np.float64)
        rgb = np.stack([g * 0.95, g, g * 1.02], -1)
        rgb = rgb * (1 - lichen[..., None] * 0.35) + lichen[..., None] * np.array([0.42, 0.48, 0.25]) * 0.35
        save_rgba(out / f"stone{k}.png", rgb, a)
        save_rgba(out / f"stone{k}_n.png", normals_from_height(height, 3.5, wrap=False), a)
    # The moor: heather and peat, tiling.
    S = 256
    n = fbm(S, 10, 5)
    n2 = fbm(S, 40, 3)
    rgb = np.stack([0.17 + 0.1 * n + 0.06 * n2, 0.15 + 0.08 * n, 0.14 + 0.07 * n + 0.04 * n2], -1)
    save_rgba(out / "moor.png", rgb, np.ones((S, S)))
    save_rgba(out / "moor_n.png", normals_from_height(n * 0.7 + n2 * 0.3, 3.0), np.ones((S, S)))
    # Low hills against the sky, a silhouette.
    W2, H2 = 1024, 256
    y, x = np.mgrid[0:H2, 0:W2].astype(np.float64)
    ridge = 120 + 50 * np.sin(x / W2 * 2 * np.pi * 2 + 0.7) + 25 * np.sin(x / W2 * 2 * np.pi * 5 + 2.0)
    a = (y > ridge).astype(np.float64)
    save_rgba(out / "hills.png", np.stack([np.full_like(x, 0.05), np.full_like(x, 0.06), np.full_like(x, 0.09)], -1), a)
    # The cottage: stone walls, a thatched roof, a window and a door.
    W3, H3 = 240, 200
    y, x = np.mgrid[0:H3, 0:W3].astype(np.float64) + 0.5
    n = fbm_rect(W3, H3, 16, 4)
    wall = (y > 80) & (x > 20) & (x < 220)
    roof = (y > 10) & (y <= 86) & (np.abs(x - 120) < 30 + (y - 10) * 1.25)
    window = (y > 110) & (y < 150) & (x > 140) & (x < 180)
    door = (y > 120) & (x > 50) & (x < 85)
    a = (wall | roof).astype(np.float64)
    rgb = np.zeros((H3, W3, 3))
    stone = 0.3 + 0.18 * n
    rgb[wall] = np.stack([stone, stone * 0.97, stone * 0.92], -1)[wall]
    thatch = 0.28 + 0.15 * n + 0.08 * np.sin(x * 0.9 + y * 0.3)
    rgb[roof] = np.stack([thatch * 1.1, thatch * 0.9, thatch * 0.55], -1)[roof]
    rgb[door] = [0.18, 0.12, 0.08]
    rgb[window] = [0.05, 0.04, 0.03]
    height = n * 0.8 + roof * 0.3
    save_rgba(out / "cottage.png", rgb, a)
    save_rgba(out / "cottage_n.png", normals_from_height(height, 3.0, wrap=False), a)


# ── 118 The Endless Dungeon ─────────────────────────────────────────────
# Sixteen tiles, one for each set of open sides (bit 1 up, 2 right, 4
# down, 8 left - the tile's index), seen from above: flagstone floor
# through rough rock. Each tile's edge is open or rock all the way, so any
# two whose touching sides agree fit.

def ed118_dungeon():
    out = OUT / "dungeon"
    out.mkdir(parents=True, exist_ok=True)
    T = 64
    img = np.zeros((T * 4, T * 4, 3))
    y, x = np.mgrid[0:T, 0:T].astype(np.float64) + 0.5
    for b in range(16):
        tx, ty = (b % 4) * T, (b // 4) * T
        rock_n = fbm(T, 6, 4)
        half = 14.0
        # Distance outside the floor: the centre square and the arms to each
        # open side.
        dx = np.abs(x - T / 2)
        dy = np.abs(y - T / 2)
        inside = (dx < half) & (dy < half)
        if b & 1:
            inside |= (dx < half) & (y < T / 2)
        if b & 2:
            inside |= (dy < half) & (x > T / 2)
        if b & 4:
            inside |= (dx < half) & (y > T / 2)
        if b & 8:
            inside |= (dy < half) & (x < T / 2)
        if b == 0:
            inside[:] = False
        floor = inside.astype(np.float64)
        # The rock darkens toward the floor: the floor spread a few pixels
        # by a box blur, where it reaches rock.
        spread = floor.copy()
        for _ in range(3):
            p = np.pad(spread, 2, mode="edge")
            spread = sum(p[i:i + T, j:j + T] for i in range(5) for j in range(5)) / 25.0
        rim = np.clip(spread * 2.0, 0, 1) * (1 - floor)
        # Flagstones: a grid of slabs with mortar.
        sx = (x + (np.floor(y / 16) % 2) * 8) % 16
        sy = y % 16
        mortar = ((sx < 1.2) | (sy < 1.2)).astype(np.float64)
        slab = 0.36 + 0.08 * fbm(T, 4, 2) - 0.12 * mortar
        rock = 0.15 + 0.12 * rock_n - 0.06 * rim
        v = floor * slab + (1 - floor) * rock
        rgb = np.stack([v * 1.02, v * 0.96, v * 0.9], -1)
        img[ty:ty + T, tx:tx + T] = rgb
    save_rgba(out / "tiles.png", img, np.ones((T * 4, T * 4)))


# ── 120 A Day on the Island ─────────────────────────────────────────────
# An island seen from the sea - a mound of sand with grass on its crown -
# two palms and a fisher's hut, each with a normal map so the sun models
# them as it crosses the sky.

def di120_island():
    out = OUT / "island"
    out.mkdir(parents=True, exist_ok=True)
    # The island.
    W, H = 1024, 320
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    u = x / W * 2 - 1
    n = fbm_rect(W, H, 12, 4)
    # Down to the water at both ends, and faded out at the bottom, which the
    # sea covers.
    top = H * (0.22 + 0.8 * u ** 2 + 0.06 * np.sin(u * 7)) + (n - 0.5) * 18
    a = (y > top).astype(np.float64) * np.clip((H - y) / 56.0, 0, 1)
    depth = np.clip((y - top) / 60.0, 0, 1)
    grass = (y - top < 26 + 10 * np.sin(x * 0.05)) & (np.abs(u) < 0.55)
    sand = np.stack([0.78 + 0.08 * n, 0.68 + 0.07 * n, 0.46 + 0.05 * n], -1)
    green = np.stack([0.25 + 0.12 * n, 0.5 + 0.15 * n, 0.2 + 0.06 * n], -1)
    rgb = np.where(grass[..., None], green, sand) * (1.0 - 0.25 * depth[..., None])
    save_rgba(out / "island.png", rgb, a)
    save_rgba(out / "island_n.png", normals_from_height(n * 0.5 + (1 - depth) * 0.6, 3.0, wrap=False), a)
    # A palm: a leaning trunk in rings and a crown of fronds.
    W, H = 256, 384
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    v = 1 - y / H
    curve = 128 + 40 * v ** 2
    trunk = (np.abs(x - curve) < 9 - 4 * v) & (v < 0.78)
    rings = 0.5 + 0.5 * np.sin(y * 0.45)
    a = trunk.astype(np.float64)
    rgb = np.zeros((H, W, 3))
    t = 0.38 + 0.1 * rings
    rgb[trunk] = np.stack([t * 1.1, t * 0.85, t * 0.55], -1)[trunk]
    height = trunk * (0.6 + 0.3 * rings)
    cx, cy = 168.0, 384 * 0.22
    for k in range(7):
        ang = -0.2 - k * (np.pi + 0.4) / 6.0
        for s in np.linspace(0, 1, 60):
            px = cx + np.cos(ang) * 110 * s
            py = cy - np.sin(ang) * 110 * s + 40 * s * s
            w = 14 * (1 - s) + 3
            m = (x - px) ** 2 + (y - py) ** 2 < w * w
            frond = m & (a < 0.5)
            a[m] = 1.0
            g = 0.3 + 0.15 * (1 - s)
            rgb[frond] = [g * 0.6, g * 1.2, g * 0.45]
            height[m] = np.maximum(height[m], 0.5 + 0.4 * (1 - s))
    save_rgba(out / "palm.png", rgb, a)
    save_rgba(out / "palm_n.png", normals_from_height(height, 3.0, wrap=False), a)
    # The hut: stilts, plank walls, a thatched roof and a window.
    W, H = 256, 240
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 16, 3)
    walls = (y > 100) & (y < 200) & (x > 40) & (x < 216)
    roof = (y > 20) & (y <= 104) & (np.abs(x - 128) < 18 + (y - 20) * 1.25)
    stilts = (y >= 200) & ((np.abs(x - 60) < 6) | (np.abs(x - 196) < 6) | (np.abs(x - 128) < 6))
    window = (y > 130) & (y < 170) & (x > 150) & (x < 190)
    a = (walls | roof | stilts).astype(np.float64)
    rgb = np.zeros((H, W, 3))
    plank = 0.42 + 0.1 * n - 0.08 * ((x % 22) < 2)
    rgb[walls] = np.stack([plank * 1.05, plank * 0.8, plank * 0.55], -1)[walls]
    thatch = 0.5 + 0.12 * n + 0.08 * np.sin(x * 0.8 + y * 0.4)
    rgb[roof] = np.stack([thatch * 0.95, thatch * 0.8, thatch * 0.45], -1)[roof]
    rgb[stilts] = [0.3, 0.22, 0.14]
    rgb[window] = [0.2, 0.13, 0.08]
    height = n * 0.6 + roof * 0.4
    save_rgba(out / "hut.png", rgb, a)
    save_rgba(out / "hut_n.png", normals_from_height(height, 3.0, wrap=False), a)


# ── 121 The Endless Road ────────────────────────────────────────────────
# A country road seen from the side: two bands of hills that tile end to
# end, the road and its verge, poplars and round trees, telegraph poles, a
# farmhouse, a fence, and a little red car with spoked wheels that turn.

def _er_ridge(W, H, base, waves, seed):
    """Height of a ridge at each column, periodic over W so the band tiles."""
    x = np.arange(W) + 0.5
    rng = np.random.default_rng(seed)
    h = np.full(W, float(base))
    for k, amp in waves:
        h += amp * np.sin(2 * np.pi * k * x / W + rng.uniform(0, 2 * np.pi))
    return h


def er121_road():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "road"
    out.mkdir(parents=True, exist_ok=True)
    # Hills: far (hazy blue-green) and near (green), each 1024 wide, tiling.
    for name, H, base, waves, top, bottom, seed in (
        ("hills_far", 256, 120, [(2, 34), (5, 12), (11, 4)], (0.50, 0.62, 0.66), (0.42, 0.55, 0.58), 1),
        ("hills_near", 256, 150, [(3, 30), (7, 10), (13, 3)], (0.40, 0.58, 0.26), (0.30, 0.46, 0.20), 2),
    ):
        W = 1024
        y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
        ridge = _er_ridge(W, H, base, waves, seed)
        inside = y > (H - ridge)[None, :]
        t = np.clip((y - (H - ridge)[None, :]) / 120.0, 0, 1)
        n = fbm_rect(W, H, 16, 3)
        rgb = np.stack([top[i] + (bottom[i] - top[i]) * t + 0.03 * (n - 0.5) for i in range(3)], -1)
        alpha = inside.astype(np.float64)
        # Soften the top edge by a pixel.
        edge = np.clip((y - (H - ridge)[None, :]) + 0.5, 0, 1)
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), alpha * edge)
    # The road: a 512 x 128 tile - verge grass, kerb, asphalt, dashed line.
    W, H = 512, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 32, 3)
    rgb = np.zeros((H, W, 3))
    grass = y < 26
    rgb[grass] = np.stack([0.30 + 0.08 * n, 0.46 + 0.1 * n, 0.2 + 0.05 * n], -1)[grass]
    kerb = (y >= 26) & (y < 32)
    rgb[kerb] = [0.62, 0.6, 0.55]
    tar = y >= 32
    g = 0.2 + 0.05 * n
    rgb[tar] = np.stack([g, g, g * 1.05], -1)[tar]
    dash = (np.abs(y - 82) < 3) & ((x % 128) < 72)
    rgb[dash] = [0.85, 0.82, 0.6]
    save_rgba(out / "road.png", np.clip(rgb, 0, 1), np.ones((H, W)))
    # Field below the road: a 512 x 256 tile of furrows.
    W, H = 512, 256
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 16, 3)
    furrow = 0.5 + 0.5 * np.sin(y * 0.35 + np.sin(x * 2 * np.pi * 2 / W) * 2)
    rgb = np.stack([0.42 + 0.06 * furrow + 0.05 * n, 0.36 + 0.05 * furrow + 0.04 * n, 0.2 + 0.03 * n], -1)
    save_rgba(out / "field.png", np.clip(rgb, 0, 1), np.ones((H, W)))

    def png(name, W, H, draw):
        img = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(img), img)
        img.save(out / f"{name}.png")
        print(f"wrote {out / (name + '.png')}  {W}x{H}")

    # The car: a little red saloon, 240 x 110, wheels drawn separately.
    def car(d, img):
        red, dark, glass, chrome = (178, 34, 30, 255), (120, 20, 18, 255), (150, 190, 215, 255), (210, 210, 205, 255)
        d.rounded_rectangle([6, 44, 234, 88], radius=14, fill=red)
        d.polygon([(54, 46), (78, 8), (168, 8), (196, 46)], fill=red)
        d.polygon([(64, 44), (84, 14), (118, 14), (118, 44)], fill=glass)
        d.polygon([(126, 44), (126, 14), (162, 14), (186, 44)], fill=glass)
        d.rectangle([118, 14, 126, 46], fill=red)
        d.line([(10, 62), (230, 62)], fill=dark, width=2)
        d.rectangle([2, 74, 22, 84], fill=chrome)
        d.rectangle([218, 74, 238, 84], fill=chrome)
        d.ellipse([224, 54, 236, 66], fill=(255, 236, 170, 255))
        d.rectangle([4, 54, 12, 64], fill=(200, 40, 30, 255))
        # Wheel arches.
        for cx in (52, 188):
            d.pieslice([cx - 30, 62, cx + 30, 122], 180, 360, fill=(30, 22, 20, 255))
    png("car", 240, 110, car)

    # A wheel: tyre, rim and five spokes, so a turn shows.
    def wheel(d, img):
        d.ellipse([0, 0, 47, 47], fill=(28, 26, 26, 255))
        d.ellipse([9, 9, 38, 38], fill=(190, 190, 185, 255))
        d.ellipse([13, 13, 34, 34], fill=(120, 120, 118, 255))
        import math
        for k in range(5):
            a = k * 2 * math.pi / 5
            d.line([(24, 24), (24 + 11 * math.cos(a), 24 + 11 * math.sin(a))], fill=(215, 215, 210, 255), width=3)
        d.ellipse([20, 20, 28, 28], fill=(230, 230, 225, 255))
    png("wheel", 48, 48, wheel)

    # A poplar and a round tree.
    def poplar(d, img):
        d.rectangle([28, 230, 36, 300], fill=(80, 58, 40, 255))
        d.ellipse([6, 10, 58, 250], fill=(52, 92, 40, 255))
        d.ellipse([16, 30, 46, 200], fill=(66, 112, 48, 255))
    png("poplar", 64, 300, poplar)

    def roundtree(d, img):
        d.rectangle([70, 140, 86, 220], fill=(84, 60, 42, 255))
        for (x0, y0, r, c) in ((78, 90, 70, (60, 104, 44)), (46, 110, 46, (52, 92, 38)), (110, 108, 48, (54, 96, 40)),
                               (80, 62, 46, (74, 122, 54))):
            d.ellipse([x0 - r, y0 - r, x0 + r, y0 + r], fill=c + (255,))
    png("roundtree", 160, 220, roundtree)

    # A telegraph pole with its crossbar.
    def pole(d, img):
        d.rectangle([13, 20, 21, 280], fill=(92, 70, 50, 255))
        d.rectangle([0, 30, 34, 36], fill=(92, 70, 50, 255))
        for x0 in (2, 15, 28):
            d.rectangle([x0, 24, x0 + 4, 30], fill=(200, 210, 220, 255))
    png("pole", 34, 280, pole)

    # A farmhouse: whitewashed, red roof, two windows and a door.
    def house(d, img):
        d.polygon([(10, 80), (120, 10), (230, 80)], fill=(150, 52, 40, 255))
        d.rectangle([24, 78, 216, 180], fill=(232, 226, 210, 255))
        d.rectangle([150, 30, 168, 70], fill=(120, 60, 50, 255))
        for x0 in (44, 160):
            d.rectangle([x0, 100, x0 + 34, 134], fill=(70, 90, 110, 255))
            d.line([(x0 + 17, 100), (x0 + 17, 134)], fill=(232, 226, 210, 255), width=2)
        d.rectangle([104, 120, 136, 180], fill=(110, 70, 44, 255))
    png("house", 240, 180, house)

    # A post-and-rail fence, 256 x 60, tiling.
    def fence(d, img):
        wood = (150, 118, 80, 255)
        for x0 in (8, 136):
            d.rectangle([x0, 4, x0 + 10, 60], fill=wood)
        d.rectangle([0, 14, 255, 21], fill=wood)
        d.rectangle([0, 34, 255, 41], fill=wood)
    png("fence", 256, 60, fence)


# ── 122 Bullet Time ─────────────────────────────────────────────────────
# A brick warehouse at night seen from the side: a concrete floor, two
# windows on the city, a running hero in a long coat, two gunmen behind
# crates, and a bullet streak.

def bt122_bullet():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "bullet"
    out.mkdir(parents=True, exist_ok=True)
    # The floor: a 512 x 128 tile of concrete slabs.
    W, H = 512, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 24, 4)
    g = 0.34 + 0.08 * n - 0.06 * ((x % 128) < 2) - 0.1 * (y < 4)
    rgb = np.stack([g * 0.98, g * 0.97, g], -1)
    save_rgba(out / "floor.png", np.clip(rgb, 0, 1), np.ones((H, W)))

    def png(name, W, H, draw):
        img = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(img))
        img.save(out / f"{name}.png")
        print(f"wrote {out / (name + '.png')}  {W}x{H}")

    # A window on the city at night: a frame, dark sky, lit windows far off.
    def window(d):
        d.rectangle([0, 0, 219, 139], fill=(70, 60, 55, 255))
        d.rectangle([10, 10, 209, 129], fill=(22, 28, 52, 255))
        rng = np.random.default_rng(122)
        for _ in range(40):
            bx, by = int(rng.uniform(14, 200)), int(rng.uniform(50, 124))
            c = (230, 200, 120, 255) if rng.random() < 0.6 else (150, 190, 230, 255)
            d.rectangle([bx, by, bx + 3, by + 4], fill=c)
        for x0 in (12, 52, 92, 140, 176):
            h = int(rng.uniform(40, 80))
            d.rectangle([x0, 129 - h, x0 + 30, 129], fill=(14, 16, 30, 255))
        d.rectangle([106, 10, 113, 129], fill=(70, 60, 55, 255))
        d.rectangle([10, 66, 209, 72], fill=(70, 60, 55, 255))
    png("window", 220, 140, window)

    # The hero, mid-stride, leaning into the run: a long coat flying.
    def hero(d):
        coat, skin, dark = (40, 70, 78, 255), (220, 180, 150, 255), (28, 30, 36, 255)
        d.polygon([(30, 150), (40, 104), (52, 104), (46, 152)], fill=dark)        # back leg
        d.polygon([(56, 104), (68, 104), (86, 140), (78, 148)], fill=dark)        # front leg
        d.polygon([(34, 46), (66, 40), (72, 104), (10, 120), (26, 96)], fill=coat)  # coat
        d.polygon([(58, 50), (74, 58), (88, 74), (82, 80), (66, 66)], fill=coat)  # arm forward
        d.ellipse([46, 14, 72, 42], fill=skin)
        d.polygon([(44, 14), (74, 12), (72, 24), (46, 24)], fill=(30, 24, 22, 255))  # hair
    png("hero", 100, 160, hero)

    # A gunman behind cover, aiming right (flipped for the left side).
    def gunman(d):
        jacket, skin, dark = (96, 40, 36, 255), (200, 160, 130, 255), (30, 30, 34, 255)
        d.rectangle([34, 100, 50, 150], fill=dark)
        d.rectangle([54, 100, 70, 150], fill=dark)
        d.rounded_rectangle([28, 44, 76, 108], radius=10, fill=jacket)
        d.polygon([(66, 56), (110, 60), (110, 70), (66, 70)], fill=jacket)      # arm out
        d.rectangle([104, 54, 124, 62], fill=(40, 40, 44, 255))                # pistol
        d.ellipse([38, 10, 66, 42], fill=skin)
        d.pieslice([36, 6, 68, 34], 180, 360, fill=(26, 22, 20, 255))         # cap
    png("gunman", 130, 150, gunman)


# ── 123 Sundial Plaza ───────────────────────────────────────────────────
# A town square from the side: an obelisk that is a sundial, a fountain,
# lamp posts, hour stones and three townsfolk - all flat-coloured, since the
# sun's light and the shadows they lay on the cobbles are the scene.

def sp123_plaza():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "plaza"
    out.mkdir(parents=True, exist_ok=True)

    def png(name, W, H, draw):
        img = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(img))
        img.save(out / f"{name}.png")
        print(f"wrote {out / (name + '.png')}  {W}x{H}")

    # The obelisk: a tapering stone on a stepped plinth, a bronze tip.
    def obelisk(d):
        d.rectangle([4, 300, 75, 339], fill=(170, 160, 140, 255))
        d.rectangle([12, 286, 67, 300], fill=(186, 176, 156, 255))
        d.polygon([(22, 286), (57, 286), (50, 40), (29, 40)], fill=(208, 198, 176, 255))
        d.polygon([(40, 286), (57, 286), (50, 40), (40, 40)], fill=(186, 176, 156, 255))
        d.polygon([(29, 40), (50, 40), (39, 10)], fill=(176, 130, 60, 255))
    png("obelisk", 80, 340, obelisk)

    # The fountain from the side: a stone basin on a stem, a bowl above.
    def fountain(d):
        d.polygon([(10, 80), (250, 80), (236, 128), (24, 128)], fill=(170, 164, 150, 255))
        d.rectangle([10, 74, 250, 84], fill=(190, 184, 170, 255))
        d.rectangle([20, 82, 240, 92], fill=(90, 140, 170, 255))
        d.rectangle([120, 30, 140, 80], fill=(176, 170, 156, 255))
        d.polygon([(84, 26), (176, 26), (160, 42), (100, 42)], fill=(184, 178, 164, 255))
        d.rectangle([127, 4, 133, 28], fill=(160, 210, 235, 200))
    png("fountain", 260, 130, fountain)

    # A lamp post: a slim iron post with a lantern.
    def lamp(d):
        iron = (40, 42, 46, 255)
        d.rectangle([17, 40, 23, 299], fill=iron)
        d.rectangle([10, 286, 30, 299], fill=iron)
        d.polygon([(8, 14), (32, 14), (28, 44), (12, 44)], fill=iron)
        d.rectangle([12, 18, 28, 40], fill=(250, 220, 150, 255))
        d.polygon([(6, 14), (34, 14), (20, 2)], fill=iron)
    png("lamppost", 40, 300, lamp)

    # Three townsfolk mid-stride, facing right.
    def person(coat, hat, skin=(222, 184, 150, 255), skirt=False):
        def draw(d):
            leg = (54, 46, 44, 255)
            d.polygon([(22, 128), (26, 84), (34, 84), (30, 128)], fill=leg)
            d.polygon([(34, 84), (42, 84), (48, 126), (40, 128)], fill=leg)
            if skirt:
                d.polygon([(16, 50), (46, 50), (54, 100), (10, 100)], fill=coat)
            else:
                d.polygon([(18, 46), (44, 46), (46, 92), (16, 92)], fill=coat)
            d.polygon([(40, 52), (48, 58), (52, 82), (46, 84), (42, 64)], fill=coat)
            d.ellipse([20, 18, 42, 44], fill=skin)
            d.rectangle([16, 14, 46, 22], fill=hat)
            d.rectangle([22, 2, 40, 16], fill=hat)
        return draw
    png("person0", 60, 130, person((92, 60, 120, 255), (40, 34, 30, 255)))
    png("person1", 60, 130, person((176, 80, 52, 255), (120, 90, 50, 255), skirt=True))
    png("person2", 60, 130, person((60, 110, 90, 255), (30, 40, 60, 255)))

    # An hour stone: a low slab set in the cobbles.
    def stone(d):
        d.rounded_rectangle([0, 0, 31, 13], radius=4, fill=(200, 190, 168, 255))
        d.rectangle([3, 9, 28, 13], fill=(160, 150, 130, 255))
    png("hourstone", 32, 14, stone)
    # Pale paving slabs, 256 x 128, tiling: a light floor shows a shadow.
    W, H = 256, 128
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 16, 3)
    joint = ((x % 64) < 2) | ((y % 32) < 2) | (((x + (np.floor(y / 32) % 2) * 32) % 64) < 2)
    g = 0.78 + 0.06 * n - 0.18 * joint
    rgb = np.stack([g * 1.0, g * 0.95, g * 0.86], -1)
    save_rgba(out / "paving.png", np.clip(rgb, 0, 1), np.ones((H, W)))


# ── 124 The Paper Theatre ───────────────────────────────────────────────
# A toy theatre's cut-outs, flat as paper - hills, a castle, a tree, a
# dragon, clouds - each one colour with a fibre grain and no normal map:
# the engine works their edges out (`auto_normal`). The stage: a gilt
# proscenium with red curtains, and a plank floor.

def pt124_theatre():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "papertheatre"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(124)

    def paper(name, W, H, colour, draw):
        """A cut-out: `draw` paints the silhouette in white, then the paper's
        colour and grain fill it."""
        img = _I.new("L", (W, H), 0)
        draw(_D.Draw(img))
        a = np.asarray(img).astype(np.float64) / 255.0
        n = fbm_rect(W, H, max(4, W // 24), 3)
        fibre = 0.012 * np.sin(np.arange(W)[None, :] * 0.9 + n * 6.0)
        rgb = np.stack([np.clip(colour[i] * (0.94 + 0.1 * n) + fibre, 0, 1) for i in range(3)], -1)
        save_rgba(out / f"{name}.png", rgb, a)

    def hills(seed, amps):
        def draw(d):
            W, H = 1024, 300
            r = np.random.default_rng(seed)
            ph = r.uniform(0, 6.28, len(amps))
            pts = [(0, H)]
            for x in range(0, W + 1, 8):
                y = 150 - sum(a * np.sin(x / W * 6.283 * k + ph[i]) for i, (k, a) in enumerate(amps))
                pts.append((x, y))
            pts.append((W, H))
            d.polygon(pts, fill=255)
        return draw
    paper("hills_back", 1024, 300, (0.46, 0.55, 0.62), hills(1, [(1, 40), (3, 22), (7, 8)]))
    paper("hills_front", 1024, 300, (0.42, 0.62, 0.32), hills(2, [(2, 34), (5, 14), (9, 5)]))

    def castle(d):
        d.rectangle([60, 120, 300, 340], fill=255)
        for x0 in (20, 300):
            d.rectangle([x0, 60, x0 + 70, 340], fill=255)
            for k in range(3):
                d.rectangle([x0 + k * 26, 40, x0 + k * 26 + 16, 62], fill=255)
            d.polygon([(x0 - 6, 62), (x0 + 76, 62), (x0 + 35, -4)], fill=255)
        for k in range(6):
            d.rectangle([66 + k * 40, 100, 86 + k * 40, 122], fill=255)
        d.rectangle([150, 20, 210, 120], fill=255)
        d.polygon([(144, 22), (216, 22), (180, -30)], fill=255)
        # The gate and windows cut through.
        d.pieslice([140, 240, 220, 330], 180, 360, fill=0)
        d.rectangle([140, 285, 220, 340], fill=0)
        for x0, y0 in ((45, 120), (325, 120), (172, 60)):
            d.rectangle([x0, y0, x0 + 16, y0 + 30], fill=0)
    paper("castle", 380, 340, (0.78, 0.74, 0.66), castle)

    def tree(d):
        d.rectangle([70, 150, 90, 260], fill=255)
        for (x0, y0, r) in ((80, 90, 70), (40, 120, 45), (120, 118, 46), (82, 50, 48)):
            d.ellipse([x0 - r, y0 - r, x0 + r, y0 + r], fill=255)
    paper("tree", 160, 260, (0.3, 0.5, 0.26), tree)

    def dragon(d):
        # Body, neck, head, tail, a wing and four legs, side on, facing left.
        d.ellipse([120, 90, 300, 170], fill=255)
        d.polygon([(130, 120), (70, 40), (90, 30), (160, 110)], fill=255)
        d.polygon([(60, 30), (110, 22), (118, 44), (30, 52)], fill=255)
        d.polygon([(280, 120), (380, 90), (420, 60), (400, 100), (300, 160)], fill=255)
        d.polygon([(190, 100), (230, 10), (290, 0), (260, 60), (300, 50), (250, 110)], fill=255)
        for x0 in (140, 170, 240, 270):
            d.rectangle([x0, 150, x0 + 16, 200], fill=255)
        d.ellipse([70, 30, 80, 40], fill=0)
    paper("dragon", 430, 200, (0.66, 0.2, 0.16), dragon)

    def cloud(d):
        for (x0, y0, r) in ((60, 70, 40), (110, 50, 50), (170, 68, 42), (120, 82, 36)):
            d.ellipse([x0 - r, y0 - r, x0 + r, y0 + r], fill=255)
    paper("cloud", 230, 120, (0.9, 0.9, 0.86), cloud)

    def moon(d):
        d.ellipse([10, 10, 130, 130], fill=255)
        d.ellipse([46, 0, 160, 110], fill=0)
    paper("moon", 140, 140, (0.95, 0.88, 0.6), moon)

    # The proscenium: a gilt arch over the stage with red curtains at its
    # sides, open in the middle.
    W, H = 1280, 720
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    cx = W / 2
    arch_top = 120 + 50 * np.sqrt(np.clip(1 - ((x - cx) / 520) ** 2, 0, 1))
    opening = (np.abs(x - cx) < 520) & (y > 180 - (arch_top - 120)) & (y < 640)
    a = (~opening).astype(np.float64)
    n = fbm_rect(W, H, 40, 3)
    gold = np.stack([0.78 + 0.1 * n, 0.6 + 0.08 * n, 0.24 + 0.04 * n], -1)
    rgb = gold.copy()
    # Curtains: red folds hanging at either side of the opening.
    curtain = (np.abs(x - cx) > 430) & (np.abs(x - cx) < 520) & (y > 100) & (y < 640)
    fold = 0.5 + 0.5 * np.sin(x * 0.18)
    red = np.stack([0.55 + 0.2 * fold, 0.06 + 0.04 * fold, 0.08 + 0.04 * fold], -1)
    rgb[curtain] = red[curtain]
    a[curtain] = 1.0
    # A valance across the top.
    valance = (np.abs(x - cx) < 520) & (y > 100) & (y < 150 + 14 * np.sin(x * 0.05))
    rgb[valance] = red[valance]
    a[valance] = 1.0
    save_rgba(out / "proscenium.png", np.clip(rgb, 0, 1), a)

    # The stage floor: warm planks.
    W, H = 512, 64
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    n = fbm_rect(W, H, 32, 3)
    g = 0.42 + 0.06 * n + 0.04 * np.sin(x * 0.07) - 0.12 * ((x % 128) < 2)
    save_rgba(out / "floor.png", np.stack([g * 1.0, g * 0.72, g * 0.48], -1), np.ones((H, W)))


# ── 125 Torchlit Depths ──────────────────────────────────────────────────
# A dug-out cave lit by a tile light: a 32-px tileset (rock, its top and
# underside, back wall, lava in five frames, a glowing mushroom, a crystal),
# a torch on a stake and a miner with a lantern. The rock is painted fairly
# light, since texture colour is sRGB and the cave is lit only by what the
# flood brings - dark paint reads as "the light does not reach".

def td125_depths():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "depths"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(125)
    T = 32
    sheet = np.zeros((T * 2, T * 8, 4))

    def put(local, rgb, a=None):
        r, c = divmod(local, 8)
        sheet[r * T:(r + 1) * T, c * T:(c + 1) * T, :3] = np.clip(rgb, 0, 1)
        sheet[r * T:(r + 1) * T, c * T:(c + 1) * T, 3] = 1.0 if a is None else np.clip(a, 0, 1)

    yy, xx = np.mgrid[0:T, 0:T].astype(np.float64)

    def stone(seed, base, crack=0.25):
        r = np.random.default_rng(seed)
        n = fbm_rect(T, T, 4, 3)
        # Blocky facets: a few cells of a Voronoi, each its own shade.
        pts = r.uniform(0, T, (6, 2))
        d = np.stack([np.hypot(xx - px, yy - py) for px, py in pts])
        srt = np.sort(d, 0)
        edge = np.clip((srt[1] - srt[0]) / 2.5, 0, 1)
        shade = r.uniform(0.86, 1.08, 6)[np.argmin(d, 0)]
        v = (0.82 + 0.3 * n) * shade * (1 - crack * (1 - edge))
        return np.stack([base[i] * v for i in range(3)], -1)

    rock = (0.58, 0.5, 0.42)
    for k in range(3):
        put(k, stone(10 + k, rock))
    # Top: rubble and a lighter lip where the floor meets the air.
    top = stone(20, rock)
    lip = np.clip(1.0 - yy / 7.0, 0, 1)[..., None]
    top = top * (1 - 0.35 * lip) + lip * 0.35 * np.array([0.78, 0.7, 0.6])
    put(3, top)
    # Underside: darker, with drips.
    bot = stone(21, rock) * 0.9
    for x0 in rng.uniform(2, 30, 3):
        L = rng.uniform(5, 11)
        m = (np.abs(xx - x0) < 1.6 * (1 - (T - 1 - yy) / L).clip(0, 1)) & (yy > T - 1 - L)
        bot[m] = bot[m] * 0.8 + np.array([0.45, 0.42, 0.4]) * 0.2
    put(4, bot)
    # Back wall: the same stone, smaller and dimmer, without facets' edges.
    for k in range(2):
        n = fbm_rect(T, T, 6, 3)
        v = 0.62 + 0.25 * n
        put(5 + k, np.stack([np.full((T, T), c) * v for c in (0.36, 0.34, 0.36)], -1))
    # Lava: a base tile and four frames of slow crust drifting over glow.
    for f in range(5):
        ph = f * 0.9
        n = fbm_rect(T, T, 3, 3)
        flow = 0.5 + 0.5 * np.sin((xx / T * 2 + n * 2.5 + ph) * np.pi)
        crust = np.clip((flow - 0.78) * 3, 0, 1) * 0.8
        hot = np.stack([np.full((T, T), 1.0), 0.45 + 0.35 * (1 - flow), 0.08 + 0.1 * (1 - flow)], -1)
        cool = np.array([0.25, 0.09, 0.05])
        put(7 + f, hot * (1 - crust[..., None]) + cool * crust[..., None])
    # A glowing mushroom on the floor (bottom of the tile).
    img = _I.new("RGBA", (T, T), (0, 0, 0, 0))
    d = _D.Draw(img)
    for cx, h, w in ((10, 15, 10), (21, 21, 13), (27, 10, 7)):
        d.rectangle([cx - 1, T - h, cx + 1, T - 1], fill=(200, 230, 220, 255))
        d.ellipse([cx - w // 2, T - h - w // 2, cx + w // 2, T - h + w // 3], fill=(90, 230, 210, 255))
        d.ellipse([cx - w // 4, T - h - w // 4, cx, T - h], fill=(200, 255, 245, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    put(12, a[..., :3], a[..., 3])
    # A crystal cluster hanging from the ceiling (top of the tile).
    img = _I.new("RGBA", (T, T), (0, 0, 0, 0))
    d = _D.Draw(img)
    for cx, L, w in ((9, 18, 5), (16, 26, 7), (24, 14, 5)):
        d.polygon([(cx - w, 0), (cx + w, 0), (cx, L)], fill=(170, 120, 255, 255))
        d.polygon([(cx - w // 2, 0), (cx, 0), (cx, L - 2)], fill=(225, 200, 255, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    put(13, a[..., :3], a[..., 3])
    save_rgba(out / "tiles.png", sheet[..., :3], sheet[..., 3])

    # A torch: a wooden stake with a bound head; the flame is a glow sprite.
    img = _I.new("RGBA", (16, 48), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.rectangle([6, 12, 9, 47], fill=(120, 80, 45, 255))
    d.rectangle([5, 8, 10, 15], fill=(70, 50, 35, 255))
    d.rectangle([5, 10, 10, 11], fill=(150, 120, 80, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "torch.png", a[..., :3], a[..., 3])

    # The miner: helmet, coat, boots, a pick on his shoulder - side on,
    # facing right; the lantern is a sprite of its own in his hand.
    img = _I.new("RGBA", (48, 64), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.line([(14, 26), (40, 8)], fill=(110, 80, 50, 255), width=3)       # pick haft
    d.polygon([(34, 4), (46, 10), (44, 13), (36, 9)], fill=(150, 150, 160, 255))
    d.rectangle([16, 46, 22, 62], fill=(55, 45, 40, 255))                 # legs
    d.rectangle([26, 46, 32, 62], fill=(55, 45, 40, 255))
    d.rectangle([14, 58, 23, 63], fill=(35, 28, 25, 255))                 # boots
    d.rectangle([25, 58, 34, 63], fill=(35, 28, 25, 255))
    d.rounded_rectangle([13, 24, 35, 50], 5, fill=(150, 95, 55, 255))     # coat
    d.rectangle([13, 44, 35, 47], fill=(90, 60, 35, 255))                 # belt
    d.ellipse([16, 10, 32, 27], fill=(225, 175, 140, 255))               # face
    d.rectangle([26, 20, 33, 26], fill=(150, 120, 100, 255))              # beard
    d.chord([14, 4, 34, 22], 180, 360, fill=(220, 180, 50, 255))          # helmet
    d.rectangle([12, 12, 36, 14], fill=(190, 150, 40, 255))
    d.ellipse([29, 7, 34, 12], fill=(255, 250, 210, 255))                 # helmet lamp
    d.rectangle([33, 30, 40, 34], fill=(150, 95, 55, 255))                # arm
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "miner.png", a[..., :3], a[..., 3])
    # His lantern: a little cage round a flame.
    img = _I.new("RGBA", (16, 22), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.arc([4, 0, 12, 8], 180, 360, fill=(60, 55, 50, 255), width=2)
    d.rectangle([3, 4, 13, 20], outline=(60, 55, 50, 255), width=2)
    d.rectangle([5, 6, 11, 18], fill=(255, 220, 140, 255))
    d.rectangle([3, 19, 13, 21], fill=(60, 55, 50, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "lantern.png", a[..., :3], a[..., 3])


# ── 126 Billiard Hall ────────────────────────────────────────────────────
# A pool table seen from above: green baize, a mahogany rail with six
# pockets, sixteen balls (numbers drawn on, stripes banded), a cue, a dotted
# aim line and the floorboards of the hall round it.

BH_FELT = (1040, 540)
BH_RAIL = 56


def bh126_billiards():
    from PIL import Image as _I, ImageDraw as _D, ImageFont as _F
    out = OUT / "billiards"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(126)
    W, H = BH_FELT
    # Baize: fine fibre noise over an even green.
    n = fbm_rect(W // 2, H // 2, 40, 3)
    n = np.kron(n, np.ones((2, 2)))[:H, :W]
    grain = rng.normal(0, 0.02, (H, W))
    v = 0.92 + 0.12 * n + grain
    felt = np.stack([0.10 * v, 0.46 * v, 0.24 * v], -1)
    save_rgba(out / "felt.png", felt, np.ones((H, W)))
    # The rail: mahogany round the felt, pockets cut through it, diamond
    # sights inlaid on top.
    R = BH_RAIL
    FW, FH = W + 2 * R, H + 2 * R
    wood = fbm_rect(FW, FH, 18, 4)
    streak = 0.5 + 0.5 * np.sin(np.arange(FW)[None, :] * 0.08 + wood * 9.0)
    tone = 0.75 + 0.2 * streak + 0.1 * wood
    rail = np.stack([0.42 * tone, 0.17 * tone, 0.09 * tone], -1)
    a = np.ones((FH, FW))
    a[R:R + H, R:R + W] = 0.0
    img = _I.new("L", (FW, FH), 0)
    d = _D.Draw(img)
    pockets = [(R, R), (FW // 2, R - 6), (FW - R, R), (R, FH - R), (FW // 2, FH - R + 6), (FW - R, FH - R)]
    for (px, py) in pockets:
        d.ellipse([px - 30, py - 30, px + 30, py + 30], fill=255)
    holes = np.asarray(img).astype(np.float64) / 255.0
    # The pockets are black leather holes in the rail.
    rail = rail * (1 - holes[..., None]) + np.array([0.03, 0.025, 0.02]) * holes[..., None]
    a = np.maximum(a, holes)
    # A darker cushion strip along the inside of the rail.
    cush = np.zeros((FH, FW))
    cush[R - 10:R + H + 10, R - 10:R + W + 10] = 1.0
    cush[R:R + H, R:R + W] = 0.0
    cush *= (1 - holes)
    rail = rail * (1 - cush[..., None]) + np.array([0.06, 0.32, 0.17]) * cush[..., None]
    sights = _I.new("L", (FW, FH), 0)
    d = _D.Draw(sights)
    for k in (1, 2, 3, 5, 6, 7):
        x = R + W * k / 8
        for y in (R // 2 - 4, FH - R // 2 + 4):
            d.polygon([(x, y - 5), (x + 4, y), (x, y + 5), (x - 4, y)], fill=255)
    for k in (1, 2, 3):
        y = R + H * k / 4
        for x in (R // 2 - 4, FW - R // 2 + 4):
            d.polygon([(x, y - 5), (x + 4, y), (x, y + 5), (x - 4, y)], fill=255)
    s = np.asarray(sights).astype(np.float64)[..., None] / 255.0
    rail = rail * (1 - s) + np.array([0.92, 0.88, 0.75]) * s
    save_rgba(out / "rail.png", rail, a)
    # Floorboards round the table.
    FWL, FHL = 1024, 512
    w2 = fbm_rect(FWL, FHL, 24, 3)
    board = (np.arange(FHL)[:, None] // 64) % 2
    t2 = 0.6 + 0.15 * w2 + 0.06 * board + 0.05 * np.sin(np.arange(FWL)[None, :] * 0.05 + w2 * 7)
    seam = ((np.arange(FHL) % 64) < 2)[:, None] * 1.0
    t2 = t2 * (1 - 0.5 * seam)
    floor = np.stack([0.30 * t2, 0.20 * t2, 0.13 * t2], -1)
    save_rgba(out / "floor.png", floor, np.ones((FHL, FWL)))
    # The balls: the cue ball, 1-7 solid, 8 black, 9-15 striped.
    colours = {1: (240, 196, 30), 2: (30, 70, 190), 3: (200, 35, 35), 4: (110, 40, 150), 5: (240, 120, 30),
               6: (30, 130, 70), 7: (130, 40, 30), 8: (18, 18, 20)}
    try:
        font = _F.truetype("arialbd.ttf", 22)
    except OSError:
        font = _F.load_default()
    S = 64
    for k in range(16):
        img = _I.new("RGBA", (S, S), (0, 0, 0, 0))
        d = _D.Draw(img)
        if k == 0:
            base = (238, 234, 222)
            d.ellipse([1, 1, S - 2, S - 2], fill=base + (255,))
        else:
            c = colours[k if k <= 8 else k - 8]
            if k <= 8:
                d.ellipse([1, 1, S - 2, S - 2], fill=c + (255,))
            else:
                d.ellipse([1, 1, S - 2, S - 2], fill=(238, 234, 222, 255))
                band = _I.new("L", (S, S), 0)
                _D.Draw(band).rectangle([0, S * 0.26, S, S * 0.74], fill=255)
                disc = _I.new("L", (S, S), 0)
                _D.Draw(disc).ellipse([1, 1, S - 2, S - 2], fill=255)
                from PIL import ImageChops as _C
                img.paste(_I.new("RGBA", (S, S), c + (255,)), (0, 0), _C.multiply(band, disc))
                d = _D.Draw(img)
            d.ellipse([S / 2 - 12, S / 2 - 12, S / 2 + 12, S / 2 + 12], fill=(245, 242, 232, 255))
            txt = str(k)
            box = d.textbbox((0, 0), txt, font=font)
            tw, th = box[2] - box[0], box[3] - box[1]
            d.text((S / 2 - tw / 2 - box[0], S / 2 - th / 2 - box[1]), txt, font=font, fill=(20, 20, 20, 255))
        rgba = np.asarray(img).astype(np.float64) / 255.0
        # Shade it as a sphere lit from above-left, with a hot highlight.
        yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
        nx, ny = (xx - S / 2 + 0.5) / (S / 2 - 1), (yy - S / 2 + 0.5) / (S / 2 - 1)
        nz = np.sqrt(np.clip(1 - nx * nx - ny * ny, 0, 1))
        lam = np.clip(-0.35 * nx - 0.35 * ny + 0.87 * nz, 0, 1)
        shade = 0.45 + 0.6 * lam
        spec = np.clip(((-0.35 * nx - 0.35 * ny + 0.87 * nz) - 0.93) / 0.07, 0, 1) ** 2
        rgb = np.clip(rgba[..., :3] * shade[..., None] + spec[..., None] * 0.85, 0, 1)
        save_rgba(out / f"ball{k}.png", rgb, rgba[..., 3])
    # A soft round shadow for under each ball.
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
    r = np.hypot(xx - S / 2 + 0.5, yy - S / 2 + 0.5) / (S / 2)
    save_rgba(out / "shadow.png", np.zeros((S, S, 3)), np.clip(1 - r, 0, 1) ** 1.5)
    # The cue: ash shaft, a dark butt, brass ferrule and blue chalked tip;
    # drawn along +x with the tip at the right end.
    L, T = 600, 16
    img = _I.new("RGBA", (L, T), (0, 0, 0, 0))
    d = _D.Draw(img)
    for x in range(L):
        f = x / L
        half = 7.5 - 3.5 * f
        col = (60, 30, 18) if f < 0.45 else (222, 190, 130)
        if 0.44 < f < 0.47:
            col = (190, 160, 70)
        if f > 0.985:
            col = (60, 110, 200)
        elif f > 0.96:
            col = (240, 236, 222)
        d.line([(x, T / 2 - half), (x, T / 2 + half)], fill=col + (255,))
    rgba = np.asarray(img).astype(np.float64) / 255.0
    yy = np.mgrid[0:T, 0:L][0].astype(np.float64)
    round_ = 0.7 + 0.45 * np.clip(1 - np.abs(yy - T / 2 + 2) / (T / 2), 0, 1)
    save_rgba(out / "cue.png", np.clip(rgba[..., :3] * round_[..., None], 0, 1), rgba[..., 3])
    # A dot to tile along the aim line, and a ring for the ghost ball.
    img = _I.new("RGBA", (32, 32), (0, 0, 0, 0))
    _D.Draw(img).ellipse([10, 10, 22, 22], fill=(255, 255, 255, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "dot.png", a[..., :3], a[..., 3])
    img = _I.new("RGBA", (64, 64), (0, 0, 0, 0))
    _D.Draw(img).ellipse([3, 3, 60, 60], outline=(255, 255, 255, 255), width=3)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "ring.png", a[..., :3], a[..., 3])


# ── 127 The Glass House ──────────────────────────────────────────────────
# A Victorian glasshouse at dusk: a pane of glass (tint, reflection streaks,
# a bright rim), brick for the base, a dusk sky, lawn, palms, ferns, pots
# and the stones thrown at it.

def gh127_glasshouse():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "glasshouse"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(127)
    # Glass: a cool tint, two soft diagonal reflections, a bright rim.
    S = 128
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
    d = (xx + yy * 0.7) / S
    streak = np.exp(-((d - 0.55) / 0.05) ** 2) * 0.55 + np.exp(-((d - 0.72) / 0.025) ** 2) * 0.4
    rim = np.clip(1.0 - np.minimum.reduce([xx, yy, S - 1 - xx, S - 1 - yy]) / 3.0, 0, 1)
    n = fbm_rect(S, S, 6, 2)
    rgb = np.stack([0.62 + 0.3 * streak, 0.82 + 0.15 * streak, 0.86 + 0.12 * streak], -1)
    rgb = np.clip(rgb + rim[..., None] * 0.3, 0, 1)
    a = np.clip(0.1 + 0.04 * n + 0.32 * streak + 0.5 * rim, 0, 1)
    save_rgba(out / "glass.png", rgb, a)
    # Brick, a running bond.
    W, H = 256, 128
    bw, bh = 32, 16
    img = np.zeros((H, W, 3))
    for r in range(H // bh):
        off = (r % 2) * bw // 2
        for c in range(-1, W // bw + 1):
            x0 = c * bw + off
            tone = rng.uniform(0.85, 1.12)
            col = np.array([0.55, 0.26, 0.18]) * tone
            xs, xe = max(0, x0 + 1), min(W, x0 + bw - 1)
            if xe > xs:
                img[r * bh + 1:(r + 1) * bh - 1, xs:xe] = col
    mortar = (img.sum(-1) == 0)
    img[mortar] = [0.62, 0.58, 0.52]
    grain = fbm_rect(W, H, 20, 3)
    img = np.clip(img * (0.85 + 0.3 * grain[..., None]), 0, 1)
    save_rgba(out / "brick.png", img, np.ones((H, W)))
    # Dusk sky: deep blue down to a warm band at the horizon.
    Hs = 256
    t = np.linspace(0, 1, Hs)[:, None]
    top, mid, low = np.array([0.06, 0.08, 0.2]), np.array([0.3, 0.25, 0.45]), np.array([0.95, 0.55, 0.35])
    sky = np.where(t < 0.6, top + (mid - top) * (t / 0.6), mid + (low - mid) * ((t - 0.6) / 0.4))
    sky = np.repeat(sky.reshape(Hs, 1, 3), 8, axis=1)
    save_rgba(out / "sky.png", sky, np.ones((Hs, 8)))
    # Lawn strip.
    W2, H2 = 512, 64
    g = fbm_rect(W2, H2, 30, 3)
    blades = rng.uniform(0, 1, (H2, W2)) ** 8
    lawn = np.stack([0.16 + 0.08 * g, 0.34 + 0.12 * g + 0.15 * blades, 0.14 + 0.05 * g], -1)
    save_rgba(out / "lawn.png", np.clip(lawn, 0, 1), np.ones((H2, W2)))
    # The back wall inside: limewash, darker low down.
    W3, H3 = 256, 256
    w3 = fbm_rect(W3, H3, 10, 3)
    shade = 0.7 + 0.3 * np.linspace(0, 1, H3)[::-1, None]
    wall = np.stack([0.62 * shade * (0.9 + 0.15 * w3), 0.6 * shade * (0.9 + 0.15 * w3), 0.52 * shade * (0.9 + 0.15 * w3)], -1)
    save_rgba(out / "wall.png", np.clip(wall, 0, 1), np.ones((H3, W3)))

    def plant(name, W, H, draw):
        im = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(im), W, H)
        arr = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", arr[..., :3], arr[..., 3])

    def palm(d, W, H):
        cx = W / 2
        # Trunk, ringed.
        for k in range(14):
            y = H - 60 - k * 18
            d.ellipse([cx - 12 + k * 0.6, y - 10, cx + 12 + k * 0.6, y + 10], fill=(110, 85, 55, 255))
        top = (cx + 8, H - 60 - 13 * 18)
        r2 = np.random.default_rng(5)
        for k in range(9):
            a = -math.pi + k * math.pi / 8 + r2.uniform(-0.1, 0.1)
            L = r2.uniform(110, 150)
            pts = []
            for s in range(12):
                f = s / 11
                droop = f * f * 60
                pts.append((top[0] + math.cos(a) * L * f, top[1] + math.sin(a) * L * f + droop))
            for s in range(1, 12):
                w = 14 * (1 - s / 12)
                (x0, y0), (x1, y1) = pts[s - 1], pts[s]
                d.line([(x0, y0), (x1, y1)], fill=(40, 120, 55, 255), width=int(max(2, w)))
        # Pot.
        d.polygon([(cx - 48, H - 62), (cx + 48, H - 62), (cx + 36, H - 1), (cx - 36, H - 1)], fill=(170, 90, 55, 255))
        d.rectangle([cx - 52, H - 70, cx + 52, H - 58], fill=(185, 100, 62, 255))
    plant("palm", 300, 420, palm)

    def fern(d, W, H):
        r2 = np.random.default_rng(9)
        for k in range(11):
            a = math.pi * (0.1 + 0.8 * k / 10) + r2.uniform(-0.05, 0.05)
            L = r2.uniform(70, 100)
            base = (W / 2, H - 52)
            for s in range(10):
                f = s / 9
                x = base[0] - math.cos(a) * L * f
                y = base[1] - math.sin(a) * L * f + f * f * 25
                rr = 9 * (1 - f) + 2
                d.ellipse([x - rr, y - rr * 0.6, x + rr, y + rr * 0.6], fill=(60, 140, 60, 255))
        d.polygon([(W / 2 - 34, H - 52), (W / 2 + 34, H - 52), (W / 2 + 26, H - 1), (W / 2 - 26, H - 1)], fill=(160, 85, 50, 255))
    plant("fern", 220, 170, fern)

    def lamp(d, W, H):
        d.line([(W / 2, 0), (W / 2, H - 40)], fill=(40, 40, 40, 255), width=2)
        d.polygon([(W / 2 - 18, H - 40), (W / 2 + 18, H - 40), (W / 2 + 12, H - 4), (W / 2 - 12, H - 4)], fill=(50, 45, 40, 255))
        d.rectangle([W / 2 - 9, H - 36, W / 2 + 9, H - 8], fill=(255, 220, 150, 255))
    plant("lamp", 48, 140, lamp)

    def stone(d, W, H):
        d.ellipse([2, 3, W - 2, H - 2], fill=(120, 115, 108, 255))
        d.ellipse([6, 5, W / 2, H / 2], fill=(160, 155, 148, 255))
    plant("stone", 20, 18, stone)


# ── 128 Quicksave Workshop ───────────────────────────────────────────────
# A toymaker's bench: plank wall, bench top, cream dominoes with pips, glass
# marbles, a brass pendulum bob, a wooden ramp, a glass jar and a white
# polaroid frame for the quicksave's photograph.

def wk128_workshop():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "workshop"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(128)
    # Plank wall: vertical boards, grain, dark gaps.
    W, H = 512, 512
    g = fbm_rect(W, H, 14, 4)
    xs = np.arange(W)[None, :]
    board = xs // 64
    tone = 0.85 + 0.15 * np.sin(board * 2.7)
    streak = 0.5 + 0.5 * np.sin(xs * 0.15 + g * 8.0 + board)
    v = tone * (0.8 + 0.12 * streak + 0.12 * g)
    gap = ((xs % 64) < 2) * 1.0
    v = v * (1 - 0.6 * gap)
    wall = np.stack([0.5 * v, 0.34 * v, 0.22 * v], -1)
    save_rgba(out / "wall.png", np.clip(wall, 0, 1), np.ones((H, W)))
    # Bench: a thick top edge, then the front.
    W2, H2 = 512, 160
    g2 = fbm_rect(W2, H2, 20, 3)
    v2 = 0.75 + 0.15 * np.sin(np.arange(W2)[None, :] * 0.04 + g2 * 6) + 0.1 * g2
    bench = np.stack([0.62 * v2, 0.42 * v2, 0.25 * v2], -1)
    bench[:14] *= 1.18
    bench[14:18] *= 0.55
    save_rgba(out / "bench.png", np.clip(bench, 0, 1), np.ones((H2, W2)))
    # A ramp board.
    W3, H3 = 256, 24
    g3 = fbm_rect(W3, H3, 12, 2)
    v3 = 0.8 + 0.2 * g3
    ramp = np.stack([0.7 * v3, 0.5 * v3, 0.3 * v3], -1)
    ramp[:3] *= 1.2
    ramp[-3:] *= 0.6
    save_rgba(out / "ramp.png", np.clip(ramp, 0, 1), np.ones((H3, W3)))

    def drawn(name, W, H, draw):
        im = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(im), W, H)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", a[..., :3], a[..., 3])

    def domino(d, W, H):
        d.rounded_rectangle([0, 0, W - 1, H - 1], 4, fill=(238, 228, 205, 255), outline=(120, 100, 80, 255))
        d.line([(3, H // 2), (W - 4, H // 2)], fill=(90, 70, 55, 255), width=2)
        for (cx, cy) in ((W / 2, H * 0.25), (W / 2, H * 0.68), (W / 2, H * 0.84)):
            d.ellipse([cx - 3, cy - 3, cx + 3, cy + 3], fill=(40, 35, 30, 255))
    drawn("domino", 24, 96, domino)

    for k, (c1, c2) in enumerate((((60, 120, 220), (230, 240, 255)), ((220, 60, 60), (255, 220, 120)),
                                  ((60, 180, 90), (240, 255, 220)), ((240, 170, 40), (120, 40, 160)))):
        def marble(d, W, H, c1=c1, c2=c2):
            d.ellipse([1, 1, W - 2, H - 2], fill=(200, 215, 230, 200))
            d.chord([6, 4, W - 6, H - 6], 200, 340, fill=c1 + (255,))
            d.chord([5, 8, W - 8, H - 4], 20, 160, fill=c2 + (255,))
            d.ellipse([W * 0.25, H * 0.2, W * 0.45, H * 0.4], fill=(255, 255, 255, 230))
        drawn(f"marble{k}", 32, 32, marble)

    def bob(d, W, H):
        d.ellipse([1, 1, W - 2, H - 2], fill=(200, 160, 60, 255), outline=(120, 90, 30, 255), width=2)
        d.ellipse([W * 0.25, H * 0.2, W * 0.5, H * 0.45], fill=(255, 235, 170, 255))
    drawn("bob", 48, 48, bob)

    def peg(d, W, H):
        d.rectangle([0, 0, W - 1, H // 3], fill=(90, 60, 35, 255))
        d.ellipse([W / 2 - 6, H / 2 - 6, W / 2 + 6, H / 2 + 6], fill=(180, 170, 150, 255))
    drawn("peg", 60, 36, peg)

    def jar(d, W, H):
        d.rounded_rectangle([2, 10, W - 3, H - 2], 10, outline=(210, 230, 240, 230), width=5, fill=(200, 225, 235, 50))
        d.rectangle([0, 4, W - 1, 14], fill=(170, 190, 200, 200))
    drawn("jar", 120, 130, jar)

    def frame(d, W, H):
        d.rectangle([0, 0, W - 1, H - 1], fill=(246, 244, 236, 255))
        d.rectangle([10, 10, W - 11, H - 40], fill=(0, 0, 0, 0))
    drawn("polaroid", 220, 170, frame)

    # Toys on the shelf behind: blocks and a little train, painted flat.
    def shelf(d, W, H):
        d.rectangle([0, H - 18, W - 1, H - 1], fill=(110, 75, 45, 255))
        r2 = np.random.default_rng(4)
        x = 20
        cols = [(200, 70, 60), (60, 120, 200), (230, 190, 60), (80, 170, 100)]
        while x < W - 60:
            s = int(r2.integers(26, 44))
            c = cols[int(r2.integers(0, 4))]
            d.rectangle([x, H - 18 - s, x + s, H - 18], fill=c + (255,), outline=(60, 40, 30, 255))
            d.text((x + s / 2 - 4, H - 18 - s / 2 - 6), "ABCD"[int(r2.integers(0, 4))], fill=(255, 255, 255, 255))
            x += s + int(r2.integers(10, 40))
    drawn("shelf", 900, 90, shelf)


# ── 129 Carnival Banners ─────────────────────────────────────────────────
# A fairground at dusk: a sky, a striped tent, the gateway's posts and its
# arch board, a carousel's canopy, platform and horses, a pennant strip (to
# tile along a string) and a bulb.

def cb129_carnival():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "carnival"
    out.mkdir(parents=True, exist_ok=True)
    # Dusk: deep violet to rose at the horizon.
    Hs = 256
    t = np.linspace(0, 1, Hs)[:, None]
    top, mid, low = np.array([0.08, 0.06, 0.2]), np.array([0.32, 0.16, 0.38]), np.array([0.95, 0.5, 0.42])
    sky = np.where(t < 0.55, top + (mid - top) * (t / 0.55), mid + (low - mid) * ((t - 0.55) / 0.45))
    save_rgba(out / "sky.png", np.repeat(sky.reshape(Hs, 1, 3), 8, axis=1), np.ones((Hs, 8)))

    def drawn(name, W, H, draw):
        im = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        draw(_D.Draw(im), W, H)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", a[..., :3], a[..., 3])

    def tent(d, W, H):
        stripes = 9
        for k in range(stripes):
            x0 = W * k / stripes
            x1 = W * (k + 1) / stripes
            col = (200, 40, 50, 255) if k % 2 == 0 else (240, 230, 210, 255)
            d.polygon([(W / 2, 0), (x0, H * 0.45), (x1, H * 0.45)], fill=col)
            d.rectangle([x0, H * 0.45, x1, H - 1], fill=col)
        for k in range(stripes):
            x = W * (k + 0.5) / stripes
            d.pieslice([x - W / stripes / 2, H * 0.45 - 14, x + W / stripes / 2, H * 0.45 + 14], 0, 180,
                       fill=(230, 180, 60, 255))
        d.rectangle([W * 0.42, H * 0.6, W * 0.58, H - 1], fill=(40, 20, 30, 255))
    drawn("tent", 360, 300, tent)

    def post(d, W, H):
        d.rectangle([W * 0.2, 0, W * 0.8, H - 1], fill=(120, 40, 60, 255))
        for y in range(0, H, 24):
            d.rectangle([W * 0.2, y, W * 0.8, y + 10], fill=(230, 190, 80, 255))
        d.rectangle([0, 0, W - 1, 14], fill=(230, 190, 80, 255))
    drawn("post", 40, 400, post)

    def canopy(d, W, H):
        n = 12
        for k in range(n):
            x0, x1 = W * k / n, W * (k + 1) / n
            col = (60, 110, 200, 255) if k % 2 == 0 else (240, 230, 210, 255)
            d.polygon([(W / 2, 0), (x0, H * 0.7), (x1, H * 0.7)], fill=col)
        d.rectangle([0, H * 0.7, W - 1, H - 1], fill=(200, 160, 60, 255))
        for k in range(n):
            x = W * (k + 0.5) / n
            d.pieslice([x - W / n / 2, H * 0.84, x + W / n / 2, H + W / n / 3], 0, 180, fill=(60, 110, 200, 255))
        d.polygon([(W / 2 - 6, 0), (W / 2 + 6, 0), (W / 2, -1)], fill=(230, 190, 80, 255))
    drawn("canopy", 420, 170, canopy)

    def platform(d, W, H):
        d.ellipse([0, 0, W - 1, H - 1], fill=(150, 60, 70, 255))
        d.ellipse([10, 4, W - 11, H * 0.6], fill=(200, 160, 90, 255))
    drawn("platform", 440, 70, platform)

    def horse(d, W, H):
        body = (240, 236, 225, 255)
        d.ellipse([18, 40, 92, 78], fill=body)
        d.polygon([(80, 48), (104, 14), (118, 22), (98, 58)], fill=body)
        d.ellipse([100, 8, 128, 30], fill=body)
        for x in (28, 44, 70, 84):
            d.line([(x, 70), (x - 4, 104)], fill=body, width=7)
        d.polygon([(18, 52), (2, 70), (14, 74)], fill=(200, 80, 90, 255))
        d.polygon([(96, 14), (104, 4), (110, 16)], fill=(200, 80, 90, 255))
        d.rectangle([40, 44, 66, 56], fill=(60, 110, 200, 255))
    drawn("horse", 130, 110, horse)

    def pennants(d, W, H):
        # One flag a tile, hanging from the string along the top.
        cols = [(220, 60, 60, 255), (240, 200, 60, 255), (60, 140, 220, 255), (80, 180, 100, 255)]
        for k in range(4):
            x0 = W * k / 4
            d.polygon([(x0 + 4, 4), (x0 + W / 4 - 4, 4), (x0 + W / 8, H - 2)], fill=cols[k])
        d.rectangle([0, 2, W - 1, 5], fill=(60, 50, 50, 255))
    drawn("pennants", 256, 64, pennants)

    def bulb(d, W, H):
        d.ellipse([W * 0.2, H * 0.2, W * 0.8, H * 0.8], fill=(255, 240, 200, 255))
    drawn("bulb", 32, 32, bulb)

    def grass(d, W, H):
        d.rectangle([0, 0, W - 1, H - 1], fill=(40, 60, 40, 255))
    drawn("ground", 64, 64, grass)

# ── 60 Moonlit Duel ────────────────────────────────────────────────────
# Two duellists on a cliff against a great moon: their silhouettes, the
# moon, cloud bands, the cliff.

def md60_fighter(name, flip, stance):
    """A duellist, 160 x 220, facing right (or left with `flip`): a robed
    figure in a wide stance, a topknot, a long blade held forward. Flat
    and pale - the lights give it its colour."""
    from PIL import Image as _I, ImageDraw as _D
    W, H = 160, 220
    img = _I.new("RGBA", (W, H), (0, 0, 0, 0))
    d = _D.Draw(img)
    c = (215, 212, 205, 255)
    # Legs, wide apart, and the robe's skirt between them.
    d.polygon([(52, 140), (78, 140), (60, 215), (40, 215)], fill=c)
    d.polygon([(82, 140), (104, 140), (122, 215), (100, 215)], fill=c)
    d.polygon([(48, 100), (112, 100), (126, 168), (36, 168)], fill=c)
    # Torso, leaning into the stance, and the head with its topknot.
    d.polygon([(58, 58), (100, 58), (108, 104), (52, 104)], fill=c)
    d.ellipse([66, 26, 94, 58], fill=c)
    d.ellipse([74, 14, 86, 28], fill=c)
    # Arms forward to the grip, and the blade: low (stance 0) or raised (1).
    if stance == 0:
        d.polygon([(96, 66), (104, 62), (132, 92), (124, 98)], fill=c)
        d.line([(128, 95), (158, 120)], fill=c, width=4)
    else:
        d.polygon([(96, 66), (104, 62), (128, 48), (122, 40)], fill=c)
        d.line([(125, 44), (156, 6)], fill=c, width=4)
    d.polygon([(60, 66), (68, 62), (124, 94), (118, 100)], fill=c)
    if flip:
        img = img.transpose(_I.FLIP_LEFT_RIGHT)
    img.save(OUT / "duel" / f"{name}.png")
    print(f"wrote {OUT / 'duel' / (name + '.png')}  {W}x{H}")


def md60_fighters():
    (OUT / "duel").mkdir(parents=True, exist_ok=True)
    md60_fighter("left_low", False, 0)
    md60_fighter("left_high", False, 1)
    md60_fighter("right_low", True, 0)
    md60_fighter("right_high", True, 1)


def md60_moon():
    """The moon, 512 x 512: a pale disc, its maria darker, soft limb."""
    S = 512
    y, x = np.mgrid[0:S, 0:S].astype(np.float64) + 0.5
    r = np.hypot(x - S / 2, y - S / 2) / (S / 2 - 6)
    rng = np.random.default_rng(60)
    rgb = np.ones((S, S, 3)) * np.array([0.93, 0.92, 0.86])
    for _ in range(14):
        cx, cy, rr = rng.uniform(80, S - 80), rng.uniform(80, S - 80), rng.uniform(25, 90)
        m = np.clip(1 - np.hypot(x - cx, y - cy) / rr, 0, 1) ** 0.5
        rgb *= (1 - 0.12 * m)[..., None]
    rgb *= (1 - 0.18 * r ** 4)[..., None]
    a = np.clip((1 - r) * 60, 0, 1)
    save_rgba(OUT / "duel" / "moon.png", np.clip(rgb, 0, 1), a)


def md60_clouds():
    """A band of night cloud, 1024 x 160: soft lumps along a ragged line,
    lit from above."""
    W, H = 1024, 160
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    u = x / W * 2 * np.pi
    top = 70 + 18 * np.sin(u * 3 + 1) + 10 * np.sin(u * 7.3) + 6 * np.sin(u * 13)
    bottom = 120 + 10 * np.sin(u * 2 + 2) + 6 * np.sin(u * 9)
    a = np.clip((y - top) / 18, 0, 1) * np.clip((bottom - y) / 22, 0, 1)
    a *= np.clip(np.minimum(x, W - x) / 160, 0, 1)      # the band's ends fade out
    shade = np.clip(1.0 - (y - top) / 90, 0.45, 1.0)
    rgb = np.ones((H, W, 3)) * np.array([0.82, 0.84, 0.92]) * shade[..., None]
    save_rgba(OUT / "duel" / "clouds.png", rgb, a * 0.9)


def md60_cliff():
    """The cliff top they stand on, 1400 x 260: a rock ledge with a grassy
    rim, falling away at the right."""
    W, H = 1400, 260
    y, x = np.mgrid[0:H, 0:W].astype(np.float64) + 0.5
    u = x / W
    rim = 60 + 8 * np.sin(u * 40) * np.sin(u * 7) + np.where(u > 0.86, (u - 0.86) * 900, 0)
    inside = y > rim
    rgb = np.ones((H, W, 3)) * np.array([0.42, 0.40, 0.42])
    grass = inside & (y < rim + 10)
    rgb[grass] = (0.32, 0.38, 0.28)
    rng = np.random.default_rng(61)
    n = rng.random((H // 8 + 1, W // 8 + 1))
    rgb *= (0.85 + 0.15 * n[(y // 8).astype(int), (x // 8).astype(int)])[..., None]
    save_rgba(OUT / "duel" / "cliff.png", np.clip(rgb, 0, 1), inside.astype(np.float64))



# ── 130 Photo Album ──────────────────────────────────────────────────────
# A wooden table and a stack of holiday snapshots: each a small landscape
# painted here (sky, sun or moon, hills, sea or trees) inside a white
# border, a soft shadow to lift one with, and a hand for the demo.

def pa130_album():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "album"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(130)
    # Table: warm planks running across.
    W, H = 512, 512
    g = fbm_rect(W, H, 16, 4)
    ys = np.arange(H)[:, None]
    plank = ys // 96
    streak = 0.5 + 0.5 * np.sin(np.arange(W)[None, :] * 0.03 + g * 7 + plank * 3)
    v = (0.78 + 0.08 * np.sin(plank * 1.9)) * (0.85 + 0.1 * streak + 0.12 * g)
    v = v * (1 - 0.5 * ((ys % 96) < 2))
    table = np.stack([0.55 * v, 0.36 * v, 0.22 * v], -1)
    save_rgba(out / "table.png", np.clip(table, 0, 1), np.ones((H, W)))

    scenes = [
        ((0.45, 0.7, 0.95), (0.95, 0.85, 0.6), (0.25, 0.55, 0.3), "sea"),
        ((0.95, 0.6, 0.4), (1.0, 0.85, 0.5), (0.35, 0.3, 0.45), "hills"),
        ((0.5, 0.75, 0.95), (1.0, 0.95, 0.75), (0.3, 0.5, 0.25), "trees"),
        ((0.12, 0.14, 0.3), (0.95, 0.95, 0.85), (0.1, 0.12, 0.18), "moon"),
        ((0.6, 0.8, 0.95), (1.0, 0.95, 0.7), (0.85, 0.85, 0.9), "snow"),
        ((0.9, 0.5, 0.55), (1.0, 0.8, 0.5), (0.2, 0.35, 0.5), "sea"),
    ]
    PW, PH, B = 300, 220, 14
    for k, (sky, sun, land, kind) in enumerate(scenes):
        img = _I.new("RGB", (PW, PH), (245, 242, 232))
        d = _D.Draw(img)
        iw, ih = PW - 2 * B, PH - 2 * B - 26
        pic = _I.new("RGB", (iw, ih))
        pd = _D.Draw(pic)
        for y in range(ih):
            f = y / ih
            c = tuple(int(255 * (s * (1 - 0.35 * f) + 0.3 * f * sn)) for s, sn in zip(sky, sun))
            pd.line([(0, y), (iw, y)], fill=c)
        sx, sy, sr = rng.uniform(0.2, 0.8) * iw, rng.uniform(0.15, 0.4) * ih, rng.uniform(12, 22)
        pd.ellipse([sx - sr, sy - sr, sx + sr, sy + sr], fill=tuple(int(255 * c) for c in sun))
        base = ih * rng.uniform(0.55, 0.7)
        pts = [(0, ih)]
        ph = rng.uniform(0, 6.28, 3)
        for x in range(0, iw + 1, 6):
            yy = base - 18 * math.sin(x / iw * 6.28 + ph[0]) - 9 * math.sin(x / iw * 15 + ph[1])
            pts.append((x, yy))
        pts.append((iw, ih))
        pd.polygon(pts, fill=tuple(int(255 * c) for c in land))
        if kind == "sea":
            pd.rectangle([0, ih * 0.78, iw, ih], fill=(40, 90, 150))
            for y in range(int(ih * 0.8), ih, 6):
                pd.line([(rng.uniform(0, iw), y), (rng.uniform(0, iw), y)], fill=(160, 200, 230), width=1)
        if kind == "trees":
            for _ in range(9):
                tx = rng.uniform(0, iw)
                ty = base + rng.uniform(5, 30)
                pd.polygon([(tx, ty - 34), (tx - 12, ty), (tx + 12, ty)], fill=(30, 80, 40))
        if kind == "snow":
            for _ in range(80):
                x, y = rng.uniform(0, iw), rng.uniform(0, ih)
                pd.ellipse([x - 1.5, y - 1.5, x + 1.5, y + 1.5], fill=(255, 255, 255))
        pic = pic.filter(_F.GaussianBlur(0.6))
        img.paste(pic, (B, B))
        # A pencilled caption line on the white below.
        d.line([(B + 20, PH - 22), (PW - B - 60, PH - 22)], fill=(150, 150, 165), width=2)
        a = np.ones((PH, PW))
        save_rgba(out / f"photo{k}.png", np.asarray(img).astype(np.float64) / 255.0, a)
    # The back of a print: cream card, a faint printer's mark.
    img = _I.new("RGB", (PW, PH), (232, 226, 210))
    d = _D.Draw(img)
    d.rectangle([3, 3, PW - 4, PH - 4], outline=(205, 198, 180), width=2)
    for k in range(-PH, PW, 18):
        d.line([(k, PH), (k + PH, 0)], fill=(224, 217, 200), width=1)
    save_rgba(out / "back.png", np.asarray(img).astype(np.float64) / 255.0, np.ones((PH, PW)))
    # A soft rounded shadow.
    S = 128
    yy, xx = np.mgrid[0:S, 0:S].astype(np.float64)
    dx = np.clip(np.abs(xx - S / 2 + 0.5) - S * 0.24, 0, None)
    dy = np.clip(np.abs(yy - S / 2 + 0.5) - S * 0.24, 0, None)
    dd = np.hypot(dx, dy) / (S * 0.26)
    save_rgba(out / "shadow.png", np.zeros((S, S, 3)), np.clip(1 - dd, 0, 1) ** 2)
    # A touch marker for the idle demo: a soft white disc inside a ring,
    # the way a screen recording shows where a finger is.
    T = 96
    yy, xx = np.mgrid[0:T, 0:T].astype(np.float64)
    r = np.hypot(xx - T / 2 + 0.5, yy - T / 2 + 0.5) / (T / 2)
    disc = np.clip((0.55 - r) / 0.08, 0, 1) * 0.55
    ring = np.clip(1 - np.abs(r - 0.8) / 0.07, 0, 1)
    alpha = np.clip(disc + ring * 0.9, 0, 1)
    save_rgba(out / "touch.png", np.ones((T, T, 3)), alpha)



# ── 131 Rack Focus ───────────────────────────────────────────────────────
# A misty forest at dawn, drawn as layers a camera can focus between: a
# sky, far hills, two rows of pines, a deer in a clearing, ferns close to
# the lens and mist bands between them. Each layer is its own sprite so its
# z can be its depth.

def rf131_forest():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "rackfocus"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(131)

    # Sky: pale gold low, cool blue-grey high.
    W, H = 256, 256
    t = np.linspace(0, 1, H)[:, None] * np.ones((1, W))
    low, high = np.array([0.98, 0.86, 0.62]), np.array([0.55, 0.66, 0.78])
    sky = low[None, None, :] * (1 - t[..., None]) ** 1.5 + high[None, None, :] * (1 - (1 - t[..., None]) ** 1.5)
    save_rgba(out / "sky.png", sky[::-1], np.ones((H, W)))

    def ridge(w, h, base, amp, freq, seed, color, top_soft=0.0):
        r = np.random.default_rng(seed)
        xs = np.arange(w)
        ph = r.uniform(0, 6.28, 4)
        y = (base + amp * (0.55 * np.sin(xs / w * 6.28 * freq + ph[0])
                           + 0.3 * np.sin(xs / w * 6.28 * freq * 2.3 + ph[1])
                           + 0.15 * np.sin(xs / w * 6.28 * freq * 5.1 + ph[2])))
        yy = np.arange(h)[:, None]
        a = np.clip((yy - y[None, :]) + 1.0, 0, 1)
        rgb = np.ones((h, w, 3)) * np.array(color)[None, None, :]
        # Lighter toward the top edge, as haze catches the ridge.
        lift = np.clip(1 - (yy - y[None, :]) / (h * 0.5), 0, 1)[..., None] * top_soft
        return np.clip(rgb + lift, 0, 1), a

    rgb, a = ridge(1024, 256, 90, 40, 1.5, 1, (0.62, 0.68, 0.76), 0.12)
    save_rgba(out / "hills.png", rgb, a)

    def pines(w, h, n, size, color, seed, ground):
        img = _I.new("RGBA", (w, h), (0, 0, 0, 0))
        d = _D.Draw(img)
        r = np.random.default_rng(seed)
        c = tuple(int(255 * v) for v in color) + (255,)
        d.rectangle([0, h - ground, w, h], fill=c)
        xs = np.sort(r.uniform(0, w, n))
        for x in xs:
            s = size * r.uniform(0.7, 1.25)
            base = h - ground + r.uniform(-4, 6)
            tiers = 5
            for k in range(tiers):
                f = k / tiers
                wy = base - s * f * 0.95
                half = s * 0.32 * (1 - f * 0.8)
                d.polygon([(x - half, wy), (x + half, wy), (x, wy - s * 0.32)], fill=c)
            d.rectangle([x - s * 0.03, base - s * 0.1, x + s * 0.03, base + 2], fill=c)
        a = np.asarray(img).astype(np.float64) / 255.0
        # A little noise in the colour so a layer is not a flat cut-out.
        g = fbm_rect(w, h, 24, 3)
        rgb = a[..., :3] * (0.9 + 0.2 * g[..., None])
        return np.clip(rgb, 0, 1), a[..., 3]

    rgb, a = pines(1024, 384, 34, 170, (0.36, 0.45, 0.48), 2, 40)
    save_rgba(out / "pines_far.png", rgb, a)
    rgb, a = pines(1024, 512, 16, 330, (0.17, 0.25, 0.24), 3, 60)
    save_rgba(out / "pines_near.png", rgb, a)

    # The deer, side on, head up: an ellipse body, neck, head, ears, legs,
    # a few-tined antler - drawn big and downsampled for soft edges.
    S = 4
    img = _I.new("RGBA", (256 * S, 256 * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    coat, dark, belly = (150, 98, 62, 255), (92, 58, 36, 255), (196, 160, 120, 255)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    # Legs: thigh to hock to hoof, the far pair darker.
    for (hx, hy), (kx, ky), (fx, fy), col in (
        ((84, 140), (76, 186), (80, 228), dark),
        ((150, 140), (156, 186), (154, 228), dark),
        ((96, 142), (90, 188), (95, 228), coat),
        ((164, 142), (170, 188), (168, 228), coat),
    ):
        d.line(P((hx, hy), (kx, ky)), fill=col, width=11 * S)
        d.line(P((kx, ky), (fx, fy)), fill=col, width=6 * S)
        d.ellipse(P((fx - 4, fy - 3), (fx + 4, fy + 3)), fill=(50, 32, 24, 255))
    # Haunch, barrel and chest.
    d.ellipse(P((64, 104), (124, 158)), fill=coat)
    d.ellipse(P((92, 108), (170, 156)), fill=coat)
    d.ellipse(P((140, 100), (186, 154)), fill=coat)
    d.ellipse(P((106, 146), (156, 157)), fill=belly)
    # Neck, rising forward, and the head held up.
    d.polygon(P((150, 118), (176, 104), (198, 66), (180, 58), (160, 92)), fill=coat)
    d.ellipse(P((176, 48), (206, 74)), fill=coat)
    d.polygon(P((198, 56), (222, 66), (220, 76), (198, 72)), fill=coat)
    d.ellipse(P((216, 66), (224, 74)), fill=(40, 26, 20, 255))
    d.ellipse(P((188, 56), (193, 61)), fill=(30, 20, 16, 255))
    d.polygon(P((180, 54), (164, 40), (184, 50)), fill=coat)
    d.polygon(P((182, 52), (168, 36), (174, 50)), fill=dark)
    # Antlers: a beam back and up with three tines.
    for ax, ay, k in ((190, 50, 1.0), (196, 49, 0.9)):
        d.line(P((ax, ay), (ax - 6 * k, 30), (ax - 16 * k, 14), (ax - 30 * k, 6)), fill=dark, width=3 * S)
        d.line(P((ax - 6 * k, 30), (ax + 4 * k, 20)), fill=dark, width=2 * S)
        d.line(P((ax - 16 * k, 14), (ax - 10 * k, 2)), fill=dark, width=2 * S)
    # The white tail.
    d.ellipse(P((60, 108), (72, 122)), fill=(232, 222, 204, 255))
    img = img.resize((256, 256), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "deer.png", a[..., :3], a[..., 3])

    # Ferns: a clump of arching fronds, each a curved stem with leaflets,
    # dark and close - the near layer.
    S = 2
    FW, FH = 512, 384
    img = _I.new("RGBA", (FW * S, FH * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    for k in range(13):
        base = (FW * 0.5 + rng.uniform(-60, 60), FH)
        ang = math.radians(rng.uniform(-150, -30))
        length = rng.uniform(220, 360)
        bend = rng.uniform(0.5, 1.2) * (1 if math.cos(ang) > 0 else -1)
        shade = rng.uniform(0.6, 1.0)
        col = (int(40 * shade), int(92 * shade), int(44 * shade), 255)
        pts = []
        for i in range(41):
            f = i / 40
            a2 = ang + bend * f * f
            if i == 0:
                x, y = base
            else:
                x, y = pts[-1]
                x += math.cos(a2) * length / 40
                y += math.sin(a2) * length / 40
            pts.append((x, y))
        d.line([(x * S, y * S) for x, y in pts], fill=col, width=4 * S)
        for i in range(3, 40, 2):
            f = i / 40
            x, y = pts[i]
            x2, y2 = pts[i + 1]
            tx, ty = x2 - x, y2 - y
            n = math.hypot(tx, ty) or 1
            nx, ny = -ty / n, tx / n
            leaf = 34 * (1 - f) + 6
            for sgn in (1, -1):
                lx, ly = x + nx * leaf * sgn + tx * 1.5, y + ny * leaf * sgn + ty * 1.5
                d.polygon([(x * S, y * S), (lx * S, ly * S), ((x + tx * 2.4) * S, (y + ty * 2.4) * S)], fill=col)
    img = img.resize((FW, FH), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "ferns.png", a[..., :3], a[..., 3])

    # Grass for the clearing floor the deer stands on.
    rgb, a = ridge(1024, 128, 30, 6, 3, 4, (0.42, 0.52, 0.34), 0.08)
    g = fbm_rect(1024, 128, 40, 3)
    save_rgba(out / "meadow.png", np.clip(rgb * (0.85 + 0.3 * g[..., None]), 0, 1), a)

    # Mist: a soft horizontal band.
    MW, MH = 512, 128
    yy = (np.arange(MH)[:, None] - MH / 2) / (MH / 2)
    g = fbm_rect(MW, MH, 10, 4)
    a = np.clip(1 - yy ** 2, 0, 1) ** 1.5 * (0.55 + 0.45 * g)
    save_rgba(out / "mist.png", np.ones((MH, MW, 3)) * 0.97, np.clip(a, 0, 1))



# ── 132 Beat Hall ────────────────────────────────────────────────────────
# A club: a dark panelled back wall with a DJ booth, speaker stacks, a dance
# floor of glass tiles the script colours, and a crowd in silhouette. The
# tiles are white so the script can tint them; everything that pulses is
# moved by the beat clock, not drawn pulsing.

def bh132_hall():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "beathall"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(132)

    # The back wall: dark acoustic panels with a faint grain.
    W, H = 512, 256
    g = fbm_rect(W, H, 20, 3)
    xs = np.arange(W)[None, :]
    ys = np.arange(H)[:, None]
    seams = ((xs % 64) < 2) | ((ys % 64) < 2)
    v = (0.10 + 0.04 * g) * np.where(seams, 0.55, 1.0)
    wall = np.stack([v * 0.9, v * 0.85, v * 1.25], -1)
    save_rgba(out / "wall.png", np.clip(wall, 0, 1), np.ones((H, W)))

    # A dance-floor tile: white glass with a bevel and a darker rim, tinted
    # by the script.
    T = 64
    yy, xx = np.mgrid[0:T, 0:T].astype(np.float64)
    edge = np.minimum(np.minimum(xx, T - 1 - xx), np.minimum(yy, T - 1 - yy))
    rim = np.clip(edge / 3.0, 0, 1)
    bevel = 0.75 + 0.25 * np.clip(edge / 10.0, 0, 1)
    shine = 0.12 * np.clip(1 - (xx + yy) / (T * 0.9), 0, 1)
    v = np.clip(bevel * (0.35 + 0.65 * rim) + shine, 0, 1)
    save_rgba(out / "tile.png", np.stack([v, v, v], -1), np.ones((T, T)))

    # A speaker cabinet: a box with two cones; the cones are a separate
    # sprite the script pumps.
    img = _I.new("RGBA", (128, 256), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.rounded_rectangle([4, 4, 123, 251], 8, fill=(26, 26, 32, 255), outline=(60, 60, 72, 255), width=3)
    for y in (16, 236):
        for x in (16, 112):
            d.ellipse([x - 3, y - 3, x + 3, y + 3], fill=(90, 90, 100, 255))
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "cabinet.png", a[..., :3], a[..., 3])
    C = 96
    yy, xx = np.mgrid[0:C, 0:C].astype(np.float64)
    r = np.hypot(xx - C / 2 + 0.5, yy - C / 2 + 0.5) / (C / 2)
    cone = np.clip(0.18 + 0.25 * (1 - r) + 0.25 * (np.abs(r - 0.92) < 0.05) + 0.4 * (r < 0.18), 0, 1)
    save_rgba(out / "cone.png", np.stack([cone, cone, cone * 1.1], -1), (r < 1.0).astype(np.float64))

    # The DJ booth: a desk front with a strip the script lights.
    img = _I.new("RGBA", (384, 128), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.polygon([(10, 20), (374, 20), (360, 124), (24, 124)], fill=(34, 32, 42, 255))
    d.rectangle([10, 14, 374, 26], fill=(70, 66, 84, 255))
    for k in range(6):
        x = 40 + k * 55
        d.rectangle([x, 50, x + 30, 100], outline=(55, 52, 66, 255), width=2)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "booth.png", a[..., :3], a[..., 3])

    # The DJ, in silhouette: head, shoulders, an arm raised to the headphones.
    S = 3
    img = _I.new("RGBA", (128 * S, 160 * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    k = (14, 12, 18, 255)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    d.ellipse(P((48, 20), (80, 54)), fill=k)
    d.arc(P((42, 14), (86, 56)), 190, 350, fill=k, width=5 * S)
    d.polygon(P((30, 160), (34, 80), (50, 62), (78, 62), (94, 80), (98, 160)), fill=k)
    d.line(P((88, 80), (104, 52), (84, 34)), fill=k, width=12 * S)
    img = img.resize((128, 160), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "dj.png", a[..., :3], a[..., 3])

    # Dancers, in silhouette, a few poses: arms up, arms out, one arm up.
    for i, arms in enumerate(("up", "out", "one")):
        img = _I.new("RGBA", (96 * S, 200 * S), (0, 0, 0, 0))
        d = _D.Draw(img)
        d.ellipse(P((34, 18), (62, 48)), fill=k)
        d.polygon(P((26, 120), (30, 60), (40, 52), (56, 52), (66, 60), (70, 120)), fill=k)
        d.line(P((38, 118), (30, 196)), fill=k, width=13 * S)
        d.line(P((58, 118), (66, 196)), fill=k, width=13 * S)
        if arms == "up":
            d.line(P((34, 62), (18, 8)), fill=k, width=9 * S)
            d.line(P((62, 62), (78, 8)), fill=k, width=9 * S)
        elif arms == "out":
            d.line(P((34, 62), (4, 92)), fill=k, width=9 * S)
            d.line(P((62, 62), (92, 70)), fill=k, width=9 * S)
        else:
            d.line(P((34, 62), (22, 108)), fill=k, width=9 * S)
            d.line(P((62, 62), (84, 6)), fill=k, width=9 * S)
        img = img.resize((96, 200), _I.LANCZOS)
        a = np.asarray(img).astype(np.float64) / 255.0
        save_rgba(out / f"dancer{i}.png", a[..., :3], a[..., 3])

    # A light beam: a soft cone from the top, white, additive.
    BW, BH = 128, 512
    yy, xx = np.mgrid[0:BH, 0:BW].astype(np.float64)
    f = yy / BH
    half = 4 + f * (BW / 2 - 6)
    across = np.clip(1 - np.abs(xx - BW / 2 + 0.5) / half, 0, 1) ** 1.5
    a = across * (1 - f) ** 0.6 * 0.55
    save_rgba(out / "beam.png", np.ones((BH, BW, 3)), np.clip(a, 0, 1))



# ── 133 Ember Storm ──────────────────────────────────────────────────────
# A courtyard at night for sparks that keep state: a block wall, stone
# steps and a slanted roof for them to bounce on, a cobbled floor for them to
# settle on, a brazier on a cart that throws them, and an iron ball that
# swings through the drifts.

def es133_court():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "emberstorm"
    out.mkdir(parents=True, exist_ok=True)

    def blocks(w, h, bw, bh, base, seed, jitter=0.12):
        r = np.random.default_rng(seed)
        g = fbm_rect(w, h, 18, 4)
        ys = np.arange(h)[:, None]
        xs = np.arange(w)[None, :]
        row = ys // bh
        off = (row % 2) * (bw // 2)
        col = (xs + off) // bw
        seam = (((xs + off) % bw) < 2) | ((ys % bh) < 2)
        shade = np.zeros((h, w))
        for rr in range(h // bh + 1):
            for cc in range(w // bw + 2):
                shade[(row == rr) & (col == cc)] = r.uniform(-jitter, jitter)
        v = (1 + shade + 0.18 * (g - 0.5)) * np.where(seam, 0.45, 1.0)
        return np.clip(np.stack([v * base[0], v * base[1], v * base[2]], -1), 0, 1)

    save_rgba(out / "wall.png", blocks(512, 256, 64, 32, (0.20, 0.21, 0.26), 1), np.ones((256, 512)))
    save_rgba(out / "stone.png", blocks(256, 64, 64, 32, (0.36, 0.34, 0.33), 2, 0.08), np.ones((64, 256)))
    save_rgba(out / "floor.png", blocks(512, 96, 40, 24, (0.27, 0.25, 0.24), 3, 0.15), np.ones((96, 512)))
    # Roof: overlapping slate rows.
    W, H = 384, 64
    g = fbm_rect(W, H, 20, 3)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.ones((H, 1)) * np.arange(W)[None, :]
    row = ys // 16
    edge = (ys % 16) < 3
    tile = ((xs + (row % 2) * 12) % 24) < 2
    v = (0.9 + 0.2 * (g - 0.5)) * np.where(edge | tile, 0.5, 1.0)
    save_rgba(out / "roof.png", np.clip(np.stack([v * 0.24, v * 0.22, v * 0.26], -1), 0, 1), np.ones((H, W)))
    # The cart: wheels, a bed and an iron fire basket.
    S = 3
    img = _I.new("RGBA", (192 * S, 160 * S), (0, 0, 0, 0))
    d = _D.Draw(img)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    wood, iron, dark = (96, 62, 38, 255), (52, 52, 58, 255), (28, 26, 30, 255)
    d.rectangle(P((14, 96), (178, 116)), fill=wood, outline=dark, width=2 * S)
    for x in (44, 148):
        d.ellipse(P((x - 26, 104), (x + 26, 156)), fill=dark)
        d.ellipse(P((x - 20, 110), (x + 20, 150)), fill=wood)
        d.ellipse(P((x - 6, 124), (x + 6, 136)), fill=iron)
    d.polygon(P((56, 96), (136, 96), (152, 40), (40, 40)), fill=iron, outline=dark)
    for x in range(52, 144, 14):
        d.line(P((x, 44), (x + 4, 94)), fill=dark, width=2 * S)
    d.line(P((178, 104), (190, 84)), fill=wood, width=4 * S)
    img = img.resize((192, 160), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "cart.png", a[..., :3], a[..., 3])
    # The iron ball: a sphere, lit from up left.
    B = 128
    yy, xx = np.mgrid[0:B, 0:B].astype(np.float64)
    dx, dy = (xx - B / 2 + 0.5) / (B / 2), (yy - B / 2 + 0.5) / (B / 2)
    r2 = dx * dx + dy * dy
    nz = np.sqrt(np.clip(1 - r2, 0, 1))
    lit = np.clip(-dx * 0.5 - dy * 0.5 + nz * 0.7, 0, 1)
    v = 0.12 + 0.55 * lit + 0.5 * np.clip(lit - 0.85, 0, 1) * 4
    save_rgba(out / "ball.png", np.clip(np.stack([v, v * 0.98, v * 1.05], -1), 0, 1), (r2 < 1).astype(np.float64))



# ── 134 Meadow Trails ────────────────────────────────────────────────────
# A meadow in late sun for a field things move through: grass blades, reeds
# and flowers drawn white-ish so the scene can tint each, a pond, the ground
# under them, and a fox that runs through it all.

def mt134_meadow():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "meadowtrails"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(134)
    # Ground: warm green with patches, seen at a slant.
    W, H = 512, 256
    g = fbm_rect(W, H, 14, 4)
    g2 = fbm_rect(W, H, 40, 3)
    v = 0.75 + 0.3 * (g - 0.5) + 0.12 * (g2 - 0.5)
    ground = np.stack([0.30 * v, 0.42 * v, 0.16 * v], -1)
    save_rgba(out / "ground.png", np.clip(ground, 0, 1), np.ones((H, W)))
    # Grass blades: a tapering, curving stroke, lighter at the tip.
    S = 4
    for k in range(3):
        BW, BH = 32, 128
        img = _I.new("RGBA", (BW * S, BH * S), (0, 0, 0, 0))
        d = _D.Draw(img)
        curve = (k - 1) * 7
        pts_l, pts_r = [], []
        for i in range(21):
            f = i / 20
            x = BW / 2 + curve * f * f
            half = 4.5 * (1 - f) + 0.4
            y = BH - f * (BH - 4)
            pts_l.append(((x - half) * S, y * S))
            pts_r.append(((x + half) * S, y * S))
        d.polygon(pts_l + pts_r[::-1], fill=(255, 255, 255, 255))
        img = img.resize((BW, BH), _I.LANCZOS)
        a = np.asarray(img).astype(np.float64)[..., 3] / 255.0
        ys = np.linspace(0, 1, BH)[:, None] * np.ones((1, BW))
        shade = 0.55 + 0.45 * (1 - ys)
        save_rgba(out / f"blade{k}.png", np.stack([shade, shade, shade * 0.9], -1), a)
    # Reed: a stem with a brown cattail head.
    img = _I.new("RGBA", (24 * S, 160 * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.line([(12 * S, 160 * S), (12 * S, 20 * S)], fill=(150, 170, 90, 255), width=3 * S)
    d.rounded_rectangle([7 * S, 24 * S, 17 * S, 64 * S], 5 * S, fill=(110, 70, 40, 255))
    d.line([(12 * S, 24 * S), (12 * S, 8 * S)], fill=(150, 170, 90, 255), width=2 * S)
    d.polygon([(12 * S, 150 * S), (2 * S, 90 * S), (13 * S, 140 * S)], fill=(120, 150, 70, 255))
    img = img.resize((24, 160), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "reed.png", a[..., :3], a[..., 3])
    # Flower: a thin stem and a five-petal head.
    for k, col in enumerate(((250, 240, 245), (255, 210, 80), (200, 120, 230))):
        img = _I.new("RGBA", (40 * S, 110 * S), (0, 0, 0, 0))
        d = _D.Draw(img)
        d.line([(20 * S, 110 * S), (20 * S, 22 * S)], fill=(90, 140, 60, 255), width=2 * S)
        for j in range(5):
            a0 = j / 5 * math.tau
            cx, cy = 20 + math.cos(a0) * 7, 18 + math.sin(a0) * 7
            d.ellipse([(cx - 6) * S, (cy - 6) * S, (cx + 6) * S, (cy + 6) * S], fill=col + (255,))
        d.ellipse([16 * S, 14 * S, 24 * S, 22 * S], fill=(240, 180, 40, 255))
        img = img.resize((40, 110), _I.LANCZOS)
        a = np.asarray(img).astype(np.float64) / 255.0
        save_rgba(out / f"flower{k}.png", a[..., :3], a[..., 3])
    # Pond: still water, a soft bank.
    PW, PH = 512, 192
    yy, xx = np.mgrid[0:PH, 0:PW].astype(np.float64)
    r = np.hypot((xx - PW / 2) / (PW / 2), (yy - PH / 2) / (PH / 2))
    gg = fbm_rect(PW, PH, 10, 3)
    edge = np.clip((1 - r) / 0.12, 0, 1)
    depth = np.clip(1 - r, 0, 1)
    water = np.stack([0.18 + 0.15 * depth + 0.05 * gg, 0.32 + 0.18 * depth, 0.42 + 0.22 * depth], -1)
    save_rgba(out / "pond.png", np.clip(water, 0, 1), edge)
    # The fox, running: body, tail, legs mid-stride, head forward.
    S = 3
    img = _I.new("RGBA", (192 * S, 112 * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    fur, dark, white = (214, 110, 40, 255), (70, 40, 26, 255), (245, 235, 220, 255)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    d.polygon(P((8, 40), (40, 30), (64, 44), (44, 58), (10, 54)), fill=fur)
    d.polygon(P((6, 42), (0, 46), (8, 52)), fill=white)
    for (x0, y0), (x1, y1) in (((70, 66), (52, 100)), ((84, 66), (98, 96)), ((128, 64), (112, 100)), ((140, 62), (160, 92))):
        d.line(P((x0, y0), (x1, y1)), fill=dark, width=6 * S)
    d.ellipse(P((56, 34), (150, 76)), fill=fur)
    d.ellipse(P((76, 54), (140, 74)), fill=white)
    d.polygon(P((136, 46), (150, 30), (172, 36), (188, 52), (168, 58), (148, 62)), fill=fur)
    d.polygon(P((150, 32), (152, 14), (162, 30)), fill=fur)
    d.polygon(P((160, 33), (166, 16), (170, 34)), fill=dark)
    d.polygon(P((172, 50), (188, 52), (174, 58)), fill=white)
    d.ellipse(P((184, 49), (190, 55)), fill=dark)
    d.ellipse(P((164, 39), (168, 43)), fill=dark)
    img = img.resize((192, 112), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "fox.png", a[..., :3], a[..., 3])



# ── 135 Smooth Caves ─────────────────────────────────────────────────────
# Rock for a marching-squares terrain: a fill tile with strata that wraps
# (each tile of cells samples it whole), a moss strip for the contour (its
# top row lies on the surface), a dark back wall, a boulder and a drill.

def sc135_caves():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "smoothcaves"
    out.mkdir(parents=True, exist_ok=True)
    # Rock: strata bands warped by noise, tileable.
    T = 256
    g = fbm(T, 8, 5)
    g2 = fbm(T, 24, 3)
    ys = np.arange(T)[:, None] * np.ones((1, T))
    bands = 0.5 + 0.5 * np.sin((ys / T) * math.tau * 3 + g * 5.0)
    v = 0.55 + 0.25 * bands + 0.2 * (g2 - 0.5)
    rock = np.stack([0.42 * v, 0.36 * v, 0.33 * v], -1)
    save_rgba(out / "rock.png", np.clip(rock, 0, 1), np.ones((T, T)))
    # Moss: bright at the surface (top row), fading into the rock below.
    MW, MH = 64, 32
    g = fbm_rect(MW, MH, 6, 3)
    f = np.linspace(0, 1, MH)[:, None] * np.ones((1, MW))
    a = np.clip(1 - f * 1.2 + 0.25 * (g - 0.5), 0, 1) ** 1.3
    moss = np.stack([0.30 + 0.1 * g, 0.48 + 0.12 * g, 0.18 + 0.05 * g], -1)
    save_rgba(out / "moss.png", np.clip(moss, 0, 1), a)
    # Back wall: dark, cool, soft.
    W, H = 512, 288
    g = fbm_rect(W, H, 10, 4)
    v = 0.5 + 0.5 * g
    wall = np.stack([0.06 * v, 0.07 * v, 0.10 * v], -1)
    save_rgba(out / "backwall.png", np.clip(wall, 0, 1), np.ones((H, W)))
    # Boulder: a lumpy round stone, lit from up left.
    B = 96
    yy, xx = np.mgrid[0:B, 0:B].astype(np.float64)
    dx, dy = (xx - B / 2 + 0.5) / (B / 2), (yy - B / 2 + 0.5) / (B / 2)
    ang = np.arctan2(dy, dx)
    rr = 0.92 + 0.05 * np.sin(ang * 5 + 1) + 0.03 * np.sin(ang * 9)
    r = np.hypot(dx, dy) / rr
    nz = np.sqrt(np.clip(1 - r * r, 0, 1))
    lit = np.clip(-dx * 0.5 - dy * 0.6 + nz * 0.6, 0, 1)
    g = fbm(B, 10, 3)
    v = 0.25 + 0.55 * lit + 0.12 * (g - 0.5)
    save_rgba(out / "boulder.png", np.clip(np.stack([v * 0.9, v * 0.82, v * 0.75], -1), 0, 1), np.clip((1 - r) * 30, 0, 1))
    # Drill head: a glowing bit for the digging demo.
    S = 4
    img = _I.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
    d = _D.Draw(img)
    d.polygon([(8 * S, 20 * S), (40 * S, 20 * S), (60 * S, 32 * S), (40 * S, 44 * S), (8 * S, 44 * S)], fill=(150, 150, 160, 255))
    for k in range(4):
        x = 14 + k * 7
        d.line([(x * S, 20 * S), ((x + 6) * S, 44 * S)], fill=(90, 90, 100, 255), width=2 * S)
    d.ellipse([52 * S, 26 * S, 62 * S, 38 * S], fill=(255, 200, 120, 255))
    img = img.resize((64, 64), _I.LANCZOS)
    a = np.asarray(img).astype(np.float64) / 255.0
    save_rgba(out / "drill.png", a[..., :3], a[..., 3])


# ── 136 Fog of War ───────────────────────────────────────────────────────
# A campaign map from above: grassland, forests, a river with a ford, hills
# and a road; the player's castle, scouts on horseback, and enemy tents for
# the fog to hide.

def fw136_map():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "fogofwar"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1024, 576
    g = fbm_rect(W, H, 10, 5)
    g2 = fbm_rect(W, H, 26, 3)
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    ys = np.arange(H)[:, None] * np.ones((1, W))
    grass = np.stack([0.36 + 0.1 * g2, 0.52 + 0.12 * g, 0.24 + 0.05 * g2], -1)
    # Hills: brown where a slow noise is high, shaded by its slope.
    hill = np.clip((g - 0.6) * 4, 0, 1)
    shade = np.clip(0.5 + (np.roll(g, -2, axis=1) - g) * 30, 0, 1)
    hills = np.stack([0.48 + 0.12 * shade, 0.42 + 0.1 * shade, 0.28 + 0.06 * shade], -1)
    img = grass * (1 - hill[..., None]) + hills * hill[..., None]
    # Forests: dark green clumps.
    f = fbm_rect(W, H, 18, 4)
    forest = np.clip((f - 0.58) * 8, 0, 1) * (1 - hill)
    trees = np.stack([0.12 + 0.05 * g2, 0.27 + 0.08 * g2, 0.12 + 0.03 * g2], -1)
    img = img * (1 - forest[..., None]) + trees * forest[..., None]
    # The river: a winding band, its ford at the middle.
    river_y = H * 0.55 + np.sin(xs / W * math.tau * 1.3) * 60 + np.sin(xs / W * math.tau * 3.1) * 18
    d = np.abs(ys - river_y)
    width = 14 + 4 * np.sin(xs / 70)
    ford = np.exp(-((xs - W * 0.52) / 30) ** 2)
    water = np.clip((width - d) / 3, 0, 1) * (1 - 0.8 * ford)
    bank = np.clip((width + 6 - d) / 3, 0, 1) * (1 - water)
    img = img * (1 - bank[..., None] * 0.35)
    img = img * (1 - water[..., None]) + np.array([0.16, 0.34, 0.48])[None, None, :] * water[..., None]
    # The road: from the castle (left) across the ford to the east.
    road = _I.new("L", (W, H), 0)
    dr = _D.Draw(road)
    pts = [(120, 300), (300, 280), (470, 305), (532, 318), (620, 300), (800, 250), (1000, 230)]
    dr.line(pts, fill=255, width=9)
    r = np.asarray(road).astype(np.float64) / 255.0
    img = img * (1 - r[..., None] * 0.6) + np.array([0.55, 0.47, 0.33])[None, None, :] * r[..., None] * 0.6
    save_rgba(out / "map.png", np.clip(img, 0, 1), np.ones((H, W)))
    # Pieces, drawn from above.
    S = 4

    def piece(name, size, draw):
        img = _I.new("RGBA", (size[0] * S, size[1] * S), (0, 0, 0, 0))
        d = _D.Draw(img)
        draw(d, lambda *p: [(x * S, y * S) for x, y in p])
        img = img.resize(size, _I.LANCZOS)
        a = np.asarray(img).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    def scout(d, P):
        d.ellipse(P((10, 20), (54, 44)), fill=(120, 80, 50, 255))           # horse
        d.ellipse(P((48, 26), (62, 38)), fill=(110, 72, 45, 255))           # head
        d.ellipse(P((24, 22), (40, 42)), fill=(40, 70, 150, 255))           # rider
        d.ellipse(P((28, 26), (36, 38)), fill=(230, 200, 160, 255))         # head
        d.polygon(P((24, 22), (12, 14), (18, 24)), fill=(200, 40, 40, 255))  # pennant
    piece("scout.png", (64, 64), scout)

    def tent(d, P):
        d.polygon(P((6, 54), (32, 8), (58, 54)), fill=(170, 40, 40, 255), outline=(90, 20, 20, 255))
        d.line(P((32, 8), (32, 54)), fill=(110, 25, 25, 255), width=2 * S)
        d.polygon(P((26, 54), (32, 34), (38, 54)), fill=(50, 20, 20, 255))
    piece("tent.png", (64, 64), tent)

    def castle(d, P):
        d.rectangle(P((14, 20), (114, 108)), fill=(150, 145, 135, 255), outline=(80, 78, 72, 255), width=3 * S)
        for x, y in ((8, 14), (100, 14), (8, 100), (100, 100)):
            d.ellipse(P((x, y), (x + 22, y + 22)), fill=(130, 125, 118, 255), outline=(80, 78, 72, 255), width=2 * S)
        d.rectangle(P((44, 46), (84, 82)), fill=(110, 60, 50, 255))
        d.polygon(P((64, 30), (64, 46), (80, 38)), fill=(40, 70, 150, 255))
    piece("castle.png", (128, 128), castle)


# ── 137 Village Chatter ──────────────────────────────────────────────────
# A village square at the gate: house fronts, cobbles, a well; three
# villagers drawn whole and as portraits for the dialogue box, the gate shut
# and open, and the box itself.

def vc137_village():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "villagechatter"
    out.mkdir(parents=True, exist_ok=True)
    S = 4

    def canvas(w, h):
        img = _I.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
        return img, _D.Draw(img)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    def save(img, name, size):
        img = img.resize(size, _I.LANCZOS)
        a = np.asarray(img).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The square: a sky, a row of house fronts, cobbles in perspective-ish rows.
    W, H = 1280, 720
    img, d = canvas(W, H)
    for y in range(0, H):
        t = y / H
        c = (int(150 + 60 * t), int(190 + 40 * t), int(230 + 10 * t), 255)
        d.line(P((0, y), (W, y)), fill=c, width=S)
    # Hills far off.
    d.polygon(P((0, 250), (220, 200), (480, 235), (760, 190), (1050, 230), (1280, 205), (1280, 310), (0, 310)), fill=(120, 160, 110, 255))
    houses = [(0, 100, (196, 150, 110)), (190, 70, (210, 190, 150)), (400, 115, (170, 120, 95)),
              (610, 85, (220, 205, 170)), (820, 105, (185, 140, 105))]
    for x0, top, col in houses:
        w = 200
        d.rectangle(P((x0 + 5, top + 60), (x0 + w - 5, 420)), fill=col + (255,), outline=(90, 70, 55, 255), width=2 * S)
        d.polygon(P((x0 - 8, top + 64), (x0 + w / 2, top), (x0 + w + 8, top + 64)), fill=(150, 70, 55, 255), outline=(90, 45, 35, 255))
        for wx in (x0 + 35, x0 + w - 75):
            d.rectangle(P((wx, top + 100), (wx + 40, top + 150)), fill=(80, 105, 140, 255), outline=(70, 55, 45, 255), width=S * 2)
            d.line(P((wx + 20, top + 100), (wx + 20, top + 150)), fill=(70, 55, 45, 255), width=S * 2)
        d.rectangle(P((x0 + w / 2 - 28, 340), (x0 + w / 2 + 28, 420)), fill=(105, 70, 45, 255), outline=(70, 45, 30, 255), width=2 * S)
    # The baker's sign.
    d.ellipse(P((265, 250), (325, 290)), fill=(230, 190, 120, 255), outline=(120, 80, 40, 255), width=2 * S)
    # The town wall and its gate arch on the right.
    d.rectangle(P((1030, 120), (1280, 420)), fill=(150, 145, 135, 255), outline=(95, 90, 85, 255), width=2 * S)
    for bx in range(1030, 1280, 30):
        d.rectangle(P((bx, 100), (bx + 18, 122)), fill=(150, 145, 135, 255), outline=(95, 90, 85, 255), width=S)
    d.rectangle(P((1095, 220), (1215, 420)), fill=(40, 35, 32, 255))
    d.ellipse(P((1095, 160), (1215, 280)), fill=(40, 35, 32, 255))
    # Cobbles.
    d.rectangle(P((0, 420), (W, H)), fill=(150, 140, 125, 255))
    rng = np.random.default_rng(137)
    for row, y in enumerate(range(420, H, 22)):
        off = (row % 2) * 22
        for x in range(-40 + off, W, 44):
            g = int(rng.integers(-14, 14))
            d.rounded_rectangle(P((x + 2, y + 2), (x + 42, y + 20)), radius=6 * S,
                                fill=(140 + g, 130 + g, 115 + g, 255), outline=(110, 100, 90, 255), width=S)
    save(img, "square.png", (W, H))

    # Villagers, drawn whole (facing the viewer, 140 x 240).
    def person(name, skin, body, hair, extra):
        img, d = canvas(140, 240)
        d.ellipse(P((40, 216), (100, 236)), fill=(0, 0, 0, 70))
        d.rectangle(P((52, 180), (66, 228)), fill=(60, 50, 45, 255))
        d.rectangle(P((74, 180), (88, 228)), fill=(60, 50, 45, 255))
        d.polygon(P((40, 90), (100, 90), (112, 190), (28, 190)), fill=body + (255,))
        d.rectangle(P((26, 95), (40, 165)), fill=body + (255,))
        d.rectangle(P((100, 95), (114, 165)), fill=body + (255,))
        d.ellipse(P((25, 160), (41, 176)), fill=skin + (255,))
        d.ellipse(P((99, 160), (115, 176)), fill=skin + (255,))
        d.ellipse(P((44, 30), (96, 88)), fill=skin + (255,))
        d.ellipse(P((56, 52), (63, 60)), fill=(40, 30, 30, 255))
        d.ellipse(P((77, 52), (84, 60)), fill=(40, 30, 30, 255))
        d.arc(P((60, 60), (80, 76)), 20, 160, fill=(120, 50, 45, 255), width=2 * S)
        extra(d, hair)
        save(img, name + ".png", (140, 240))

    def baker(d, hair):
        d.polygon(P((44, 94), (96, 94), (100, 186), (40, 186)), fill=(245, 245, 240, 255))
        d.ellipse(P((40, 4), (100, 44)), fill=(250, 250, 248, 255))
        d.rectangle(P((48, 28), (92, 42)), fill=(250, 250, 248, 255))
        d.arc(P((44, 30), (96, 88)), 160, 200, fill=hair, width=4 * S)

    def guard(d, hair):
        d.ellipse(P((40, 22), (100, 62)), fill=(170, 175, 185, 255), outline=(90, 95, 105, 255), width=S * 2)
        d.rectangle(P((40, 42), (100, 50)), fill=(150, 155, 165, 255))
        d.line(P((122, 10), (122, 225)), fill=(110, 80, 50, 255), width=4 * S)
        d.polygon(P((116, 14), (122, 0), (128, 14)), fill=(190, 195, 205, 255))
        d.rectangle(P((44, 100), (96, 150)), fill=(150, 30, 35, 255))
        d.line(P((70, 100), (70, 150)), fill=(230, 200, 80, 255), width=3 * S)

    def girl(d, hair):
        d.ellipse(P((40, 24), (100, 66)), fill=hair)
        d.ellipse(P((28, 50), (46, 100)), fill=hair)
        d.ellipse(P((94, 50), (112, 100)), fill=hair)
        d.ellipse(P((48, 40), (92, 80)), fill=(232, 196, 160, 255))
        d.ellipse(P((57, 54), (63, 60)), fill=(40, 30, 30, 255))
        d.ellipse(P((77, 54), (83, 60)), fill=(40, 30, 30, 255))
        d.arc(P((62, 62), (78, 72)), 20, 160, fill=(150, 50, 50, 255), width=2 * S)

    person("baker", (236, 196, 160), (200, 140, 90), (120, 80, 50, 255), baker)
    person("guard", (220, 180, 140), (90, 90, 110), (60, 50, 40, 255), guard)
    person("girl", (232, 196, 160), (200, 60, 70), (170, 90, 40, 255), girl)

    # Portraits for the dialogue box: the head and shoulders, larger.
    def portrait(name, skin, body, bg, extra):
        img, d = canvas(160, 160)
        d.rounded_rectangle(P((2, 2), (158, 158)), radius=14 * S, fill=bg + (255,), outline=(70, 50, 35, 255), width=4 * S)
        d.pieslice(P((22, 112), (138, 200)), 180, 360, fill=body + (255,))
        d.ellipse(P((44, 36), (116, 116)), fill=skin + (255,))
        d.ellipse(P((62, 66), (72, 78)), fill=(40, 30, 30, 255))
        d.ellipse(P((88, 66), (98, 78)), fill=(40, 30, 30, 255))
        d.arc(P((66, 80), (94, 102)), 20, 160, fill=(130, 50, 45, 255), width=3 * S)
        extra(d)
        save(img, name + "_portrait.png", (160, 160))

    portrait("baker", (236, 196, 160), (245, 245, 240), (230, 200, 150),
             lambda d: (d.ellipse(P((40, 6), (120, 58)), fill=(252, 252, 250, 255)), d.rectangle(P((50, 34), (110, 54)), fill=(252, 252, 250, 255))))
    portrait("guard", (220, 180, 140), (150, 30, 35), (150, 160, 180),
             lambda d: (d.chord(P((40, 20), (120, 90)), 180, 360, fill=(175, 180, 190, 255), outline=(90, 95, 105, 255), width=2 * S), d.rectangle(P((40, 52), (120, 62)), fill=(150, 155, 165, 255))))
    portrait("girl", (232, 196, 160), (200, 60, 70), (200, 220, 160),
             lambda d: (d.chord(P((40, 26), (120, 90)), 180, 360, fill=(170, 90, 40, 255)), d.ellipse(P((28, 60), (50, 120)), fill=(170, 90, 40, 255)), d.ellipse(P((110, 60), (132, 120)), fill=(170, 90, 40, 255))))

    # The gate: shut (bars) and open (nothing but the road beyond).
    img, d = canvas(120, 260)
    d.ellipse(P((0, 0), (120, 120)), fill=(95, 65, 40, 255))
    d.rectangle(P((0, 60), (120, 260)), fill=(95, 65, 40, 255))
    for x in range(10, 120, 22):
        d.line(P((x, 8), (x, 258)), fill=(60, 40, 25, 255), width=3 * S)
    for y in (90, 170, 240):
        d.line(P((0, y), (120, y)), fill=(55, 55, 60, 255), width=6 * S)
    save(img, "gate_shut.png", (120, 260))
    img, d = canvas(120, 260)
    d.ellipse(P((0, 0), (120, 120)), fill=(170, 200, 150, 255))
    d.rectangle(P((0, 60), (120, 260)), fill=(170, 200, 150, 255))
    d.polygon(P((40, 260), (55, 120), (65, 120), (80, 260)), fill=(190, 170, 130, 255))
    save(img, "gate_open.png", (120, 260))

    # The dialogue box: dark wood and a brass rim.
    img, d = canvas(1100, 230)
    d.rounded_rectangle(P((2, 2), (1098, 228)), radius=18 * S, fill=(40, 30, 26, 236), outline=(200, 160, 90, 255), width=4 * S)
    d.rounded_rectangle(P((10, 10), (1090, 220)), radius=14 * S, outline=(120, 90, 55, 255), width=2 * S)
    save(img, "panel.png", (1100, 230))


# ── 138 Loot Satchel ─────────────────────────────────────────────────────
# An adventurer's kit on a tavern table: two leather bags of slots, an
# equipment board with silhouettes for what goes where, and the loot.

def ls138_satchel():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "lootsatchel"
    out.mkdir(parents=True, exist_ok=True)
    S = 4

    def canvas(w, h):
        img = _I.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
        return img, _D.Draw(img)

    def P(*pts):
        return [(x * S, y * S) for x, y in pts]

    def save(img, name, size):
        img = img.resize(size, _I.LANCZOS)
        a = np.asarray(img).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The table: planks with grain, warm light from the top left.
    W, H = 1280, 720
    g = fbm_rect(W, H, 6, 4)
    grain = fbm_rect(W, H, 40, 2)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    plank = (ys // 120) % 2
    wood = np.stack([0.36 + 0.08 * grain + 0.03 * plank, 0.22 + 0.05 * grain + 0.02 * plank, 0.12 + 0.03 * grain], -1)
    seam = (np.abs((ys % 120) - 0.5) < 1.5)[..., None]
    wood = wood * (1 - 0.6 * seam) * (0.85 + 0.25 * g[..., None])
    light = np.clip(1.15 - np.hypot(xs - 200, ys - 80) / 1400, 0.45, 1.15)[..., None]
    save_rgba(out / "table.png", np.clip(wood * light, 0, 1), np.ones((H, W)))

    # A leather panel, stitched round its edge.
    def panel(name, w, h):
        img, d = canvas(w, h)
        d.rounded_rectangle(P((2, 2), (w - 2, h - 2)), radius=22 * S, fill=(112, 70, 40, 250), outline=(60, 36, 20, 255), width=4 * S)
        for x in range(26, w - 24, 18):
            d.line(P((x, 12), (x + 9, 12)), fill=(230, 200, 150, 255), width=2 * S)
            d.line(P((x, h - 12), (x + 9, h - 12)), fill=(230, 200, 150, 255), width=2 * S)
        for y in range(26, h - 24, 18):
            d.line(P((12, y), (12, y + 9)), fill=(230, 200, 150, 255), width=2 * S)
            d.line(P((w - 12, y), (w - 12, y + 9)), fill=(230, 200, 150, 255), width=2 * S)
        save(img, name, (w, h))

    panel("bag_panel.png", 430, 340)
    panel("equip_panel.png", 300, 420)

    # A slot: a sunken square; and the equipment silhouettes in one.
    def slot(name, mark=None):
        img, d = canvas(88, 88)
        d.rounded_rectangle(P((3, 3), (85, 85)), radius=10 * S, fill=(44, 28, 18, 255), outline=(150, 110, 70, 255), width=3 * S)
        d.rounded_rectangle(P((8, 8), (80, 80)), radius=8 * S, outline=(30, 18, 10, 255), width=2 * S)
        if mark:
            m, md = canvas(88, 88)
            mark(md)
            m = m.filter(_F.GaussianBlur(S))
            img.alpha_composite(m)
        save(img, name, (88, 88))

    ghost = (190, 160, 120, 70)
    slot("slot.png")
    slot("slot_weapon.png", lambda d: (d.line(P((26, 64), (62, 22)), fill=ghost, width=7 * S), d.line(P((22, 52), (36, 66)), fill=ghost, width=6 * S)))
    slot("slot_shield.png", lambda d: d.polygon(P((44, 18), (68, 26), (66, 52), (44, 72), (22, 52), (20, 26)), fill=ghost))
    slot("slot_helm.png", lambda d: (d.chord(P((20, 22), (68, 78)), 180, 360, fill=ghost), d.rectangle(P((20, 48), (68, 58)), fill=ghost)))
    slot("slot_ring.png", lambda d: d.ellipse(P((26, 26), (62, 62)), outline=ghost, width=7 * S))

    # The loot, drawn at 80 x 80.
    def icon(name, draw):
        img, d = canvas(80, 80)
        draw(d)
        shadow = img.split()[3].filter(_F.GaussianBlur(3 * S))
        sh = _I.new("RGBA", img.size, (0, 0, 0, 0))
        sh.putalpha(shadow.point(lambda v: v * 0.45))
        base = _I.new("RGBA", img.size, (0, 0, 0, 0))
        base.alpha_composite(sh, (2 * S, 3 * S))
        base.alpha_composite(img)
        save(base, name, (80, 80))

    def sword(d):
        d.polygon(P((60, 12), (66, 14), (32, 52), (26, 48)), fill=(210, 215, 225, 255), outline=(120, 125, 140, 255))
        d.line(P((20, 44), (38, 60)), fill=(180, 140, 50, 255), width=5 * S)
        d.line(P((28, 54), (16, 66)), fill=(90, 55, 30, 255), width=5 * S)
        d.ellipse(P((10, 64), (18, 72)), fill=(200, 160, 60, 255))

    def axe(d):
        d.line(P((22, 70), (52, 16)), fill=(110, 70, 40, 255), width=5 * S)
        d.pieslice(P((34, 6), (74, 46)), 300, 120, fill=(190, 195, 205, 255), outline=(110, 115, 130, 255))

    def shield(d):
        d.polygon(P((40, 8), (68, 18), (66, 46), (40, 72), (14, 46), (12, 18)), fill=(150, 40, 40, 255), outline=(200, 170, 80, 255), width=3 * S)
        d.line(P((40, 12), (40, 66)), fill=(220, 190, 90, 255), width=3 * S)
        d.line(P((16, 32), (64, 32)), fill=(220, 190, 90, 255), width=3 * S)

    def helm(d):
        d.chord(P((14, 14), (66, 74)), 180, 360, fill=(170, 175, 185, 255), outline=(90, 95, 105, 255), width=2 * S)
        d.rectangle(P((14, 42), (66, 52)), fill=(150, 155, 165, 255), outline=(90, 95, 105, 255))
        d.line(P((40, 16), (40, 50)), fill=(110, 115, 125, 255), width=3 * S)
        d.polygon(P((36, 8), (40, 2), (44, 8), (40, 18)), fill=(180, 40, 40, 255))

    def ring(d):
        d.ellipse(P((20, 28), (60, 68)), outline=(230, 190, 70, 255), width=6 * S)
        d.polygon(P((32, 30), (40, 14), (48, 30)), fill=(90, 200, 230, 255), outline=(40, 120, 150, 255))

    def potion(fill):
        def draw(d):
            d.rectangle(P((34, 10), (46, 24)), fill=(180, 200, 210, 220))
            d.rectangle(P((32, 6), (48, 12)), fill=(120, 80, 50, 255))
            d.ellipse(P((18, 22), (62, 70)), fill=(200, 220, 230, 140), outline=(140, 160, 175, 255), width=2 * S)
            d.chord(P((20, 24), (60, 68)), 10, 170, fill=fill)
            d.ellipse(P((28, 30), (36, 40)), fill=(255, 255, 255, 170))
        return draw

    def coins(d):
        for i, (x, y) in enumerate(((22, 46), (38, 40), (28, 30), (44, 24))):
            d.ellipse(P((x, y), (x + 26, y + 16)), fill=(230, 190, 60, 255), outline=(150, 110, 30, 255), width=2 * S)

    def apple(d):
        d.ellipse(P((16, 22), (64, 70)), fill=(200, 40, 40, 255))
        d.ellipse(P((26, 30), (36, 42)), fill=(240, 120, 110, 255))
        d.line(P((40, 24), (44, 10)), fill=(100, 60, 30, 255), width=3 * S)
        d.ellipse(P((44, 10), (60, 20)), fill=(70, 150, 60, 255))

    def scroll(d):
        d.rectangle(P((18, 20), (62, 60)), fill=(235, 220, 180, 255))
        d.ellipse(P((12, 14), (26, 30)), fill=(210, 190, 140, 255))
        d.ellipse(P((54, 50), (68, 66)), fill=(210, 190, 140, 255))
        for y in (30, 38, 46):
            d.line(P((24, y), (54, y)), fill=(150, 120, 90, 255), width=2 * S)
        d.ellipse(P((34, 50), (46, 62)), fill=(170, 30, 30, 255))

    def gem(d):
        d.polygon(P((40, 10), (64, 30), (40, 70), (16, 30)), fill=(60, 200, 120, 255), outline=(20, 110, 60, 255), width=2 * S)
        d.polygon(P((40, 10), (52, 30), (40, 70), (28, 30)), fill=(120, 240, 170, 255))

    def key(d):
        d.ellipse(P((10, 22), (34, 46)), outline=(200, 170, 70, 255), width=5 * S)
        d.line(P((32, 34), (70, 34)), fill=(200, 170, 70, 255), width=5 * S)
        d.line(P((60, 34), (60, 46)), fill=(200, 170, 70, 255), width=4 * S)
        d.line(P((68, 34), (68, 44)), fill=(200, 170, 70, 255), width=4 * S)

    for n, f in (("sword", sword), ("axe", axe), ("shield", shield), ("helm", helm), ("ring", ring),
                 ("potion_red", potion((200, 30, 40, 255))), ("potion_blue", potion((40, 90, 210, 255))),
                 ("coins", coins), ("apple", apple), ("scroll", scroll), ("gem", gem), ("key", key)):
        icon(n + ".png", f)


# ── 139 Toy Town ─────────────────────────────────────────────────────────
# Little voxel models cut into slices (a strip, bottom slice first) for
# `sprite_stack`, and a green board with a road for them to stand on.

def ts139_town():
    out = OUT / "toytown"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(139)

    def strip(vox, name):
        """vox: (nz, ny, nx, 4) floats, z from the bottom, y down the image."""
        nz, ny, nx, _ = vox.shape
        img = np.zeros((ny, nx * nz, 4))
        for z in range(nz):
            # Lower slices a little darker, so a stack reads as solid.
            v = vox[z].copy()
            v[..., :3] *= 0.72 + 0.28 * (z / max(nz - 1, 1))
            img[:, z * nx:(z + 1) * nx] = v
        save_rgba(out / name, np.clip(img[..., :3], 0, 1), img[..., 3])

    def grid(n, h):
        z, y, x = np.mgrid[0:h, 0:n, 0:n]
        return z, (y + 0.5) - n / 2, (x + 0.5) - n / 2

    def paint(vox, mask, rgb, jitter=0.04):
        j = 1 + rng.uniform(-jitter, jitter, mask.shape)
        for c in range(3):
            vox[..., c] = np.where(mask, rgb[c] * j, vox[..., c])
        vox[..., 3] = np.where(mask, 1.0, vox[..., 3])

    def house(name, n, wall, roof, h_wall, h_roof, w, d):
        h = h_wall + h_roof
        z, y, x = grid(n, h)
        vox = np.zeros((h, n, n, 4))
        body = (np.abs(x) <= w) & (np.abs(y) <= d) & (z < h_wall)
        paint(vox, body, wall)
        # Windows and a door on the walls, dark.
        edge = body & ((np.abs(np.abs(x) - w) < 1) | (np.abs(np.abs(y) - d) < 1))
        win = edge & (z >= h_wall * 0.45) & (z < h_wall * 0.75) & ((np.abs(x) % 6 < 2.5) | (np.abs(y) % 6 < 2.5))
        paint(vox, win, (0.25, 0.32, 0.45), 0)
        door = edge & (y > d - 1) & (np.abs(x) < 2) & (z < h_wall * 0.6)
        paint(vox, door, (0.35, 0.22, 0.12), 0)
        # A gable roof along x: each slice narrower across y.
        zr = z - h_wall
        roofm = (zr >= 0) & (np.abs(x) <= w + 1) & (np.abs(y) <= (d + 1.5) * (1 - zr / h_roof))
        paint(vox, roofm, roof)
        ridge = roofm & (np.abs(y) < 1.2)
        paint(vox, ridge, tuple(c * 0.7 for c in roof), 0)
        # A chimney.
        ch = (np.abs(x - w * 0.5) < 1.5) & (np.abs(y + d * 0.3) < 1.5) & (z < h) & (z >= h_wall)
        paint(vox, ch, (0.55, 0.32, 0.25))
        strip(vox, name)

    house("house_red.png", 32, (0.92, 0.86, 0.72), (0.70, 0.22, 0.18), 12, 9, 12, 8)
    house("house_blue.png", 32, (0.80, 0.86, 0.92), (0.22, 0.36, 0.62), 14, 8, 10, 10)
    house("house_yellow.png", 32, (0.96, 0.84, 0.48), (0.45, 0.30, 0.22), 10, 10, 13, 7)

    def tree(name, n, h, crown):
        z, y, x = grid(n, h)
        vox = np.zeros((h, n, n, 4))
        paint(vox, (np.hypot(x, y) < 1.8) & (z < h * 0.45), (0.45, 0.30, 0.18))
        cz = h * 0.62
        r = np.sqrt(x ** 2 + y ** 2 + ((z - cz) * 1.2) ** 2)
        leaf = r < n * 0.38 + rng.uniform(-1.2, 1.2, r.shape)
        paint(vox, leaf, crown, 0.12)
        strip(vox, name)

    tree("tree_round.png", 24, 26, (0.30, 0.58, 0.25))
    tree("tree_autumn.png", 24, 24, (0.85, 0.50, 0.18))

    def pine(name, n, h):
        z, y, x = grid(n, h)
        vox = np.zeros((h, n, n, 4))
        paint(vox, (np.hypot(x, y) < 1.5) & (z < h * 0.3), (0.42, 0.28, 0.16))
        cone = (z >= h * 0.2) & (np.hypot(x, y) < (n * 0.45) * (1 - (z - h * 0.2) / (h * 0.8)) + 0.5)
        paint(vox, cone, (0.16, 0.40, 0.25), 0.1)
        strip(vox, name)

    pine("pine.png", 20, 30)

    # A cart: a box on four wheels, pointing along +x.
    n, h = 24, 10
    z, y, x = grid(n, h)
    vox = np.zeros((h, n, n, 4))
    paint(vox, (np.abs(x) <= 9) & (np.abs(y) <= 5) & (z >= 3) & (z < 6), (0.62, 0.40, 0.22))
    paint(vox, (np.abs(x) <= 9) & (np.abs(y) <= 5) & (z >= 6) & ((np.abs(np.abs(x) - 9) < 1) | (np.abs(np.abs(y) - 5) < 1)), (0.55, 0.34, 0.18))
    paint(vox, (np.abs(x) <= 8) & (np.abs(y) <= 4) & (z >= 5) & (z < 7) & (np.abs(x) < 7), (0.95, 0.75, 0.25))
    for wx in (-6, 6):
        for wy in (-6, 6):
            paint(vox, (np.abs(x - wx) < 2.6) & (np.abs(y - wy) < 1.2) & (z < 4), (0.20, 0.16, 0.14), 0)
    paint(vox, (x > 9) & (x < 12) & (np.abs(y) < 1) & (z == 4), (0.45, 0.30, 0.18))
    strip(vox, "cart.png")

    # A well and a lamp post.
    n, h = 16, 16
    z, y, x = grid(n, h)
    vox = np.zeros((h, n, n, 4))
    rr = np.hypot(x, y)
    paint(vox, (rr < 6.5) & (rr > 4) & (z < 5), (0.62, 0.62, 0.60))
    paint(vox, (rr <= 4) & (z < 2), (0.20, 0.35, 0.55), 0)
    paint(vox, ((np.abs(x) - 5.5) ** 2 < 1) & (np.abs(y) < 1) & (z < 12), (0.42, 0.28, 0.16))
    paint(vox, (np.abs(y) <= 4) & (np.abs(x) <= 7) & (z >= 12) & (np.abs(y) <= 4 * (1 - (z - 12) / 4)), (0.68, 0.24, 0.20))
    strip(vox, "well.png")

    n, h = 10, 26
    z, y, x = grid(n, h)
    vox = np.zeros((h, n, n, 4))
    paint(vox, (np.hypot(x, y) < 1.3) & (z < 21), (0.20, 0.20, 0.24), 0)
    paint(vox, (np.abs(x) < 3) & (np.abs(y) < 3) & (z >= 21) & (z < 25), (1.0, 0.86, 0.45), 0)
    paint(vox, (np.abs(x) < 3.5) & (np.abs(y) < 3.5) & (z == 25), (0.20, 0.20, 0.24), 0)
    strip(vox, "lamp.png")

    # The board: grass with a sand road loop round the middle.
    W = H = 1024
    g = fbm(W, 12, 4)
    yy, xx = np.mgrid[0:H, 0:W]
    cx = cy = W / 2
    dx, dy = (xx - cx) / (W * 0.33), (yy - cy) / (H * 0.30)
    ring = np.abs(np.hypot(dx, dy) - 1.0) * W * 0.31
    road = np.clip((26 - ring) / 3, 0, 1)
    grass = np.stack([0.36 + 0.08 * g, 0.58 + 0.10 * g, 0.28 + 0.06 * g], -1)
    sand = np.stack([0.82 + 0.05 * g, 0.72 + 0.05 * g, 0.52 + 0.04 * g], -1)
    img = grass * (1 - road[..., None]) + sand * road[..., None]
    edge = np.clip((30 - ring) / 3, 0, 1) * (1 - road)
    img *= 1 - 0.25 * edge[..., None]
    save_rgba(out / "board.png", np.clip(img, 0, 1), np.ones((H, W)))
    # Grass beyond the board, for when the camera turns its corners away.
    g2 = fbm(256, 6, 3)
    save_rgba(out / "grass.png", np.clip(np.stack([0.36 + 0.08 * g2, 0.58 + 0.10 * g2, 0.28 + 0.06 * g2], -1), 0, 1), np.ones((256, 256)))


# ── 140 Kart Circuit ─────────────────────────────────────────────────────
# A track from above for a Mode 7 plane (grass to its edges, since samplers
# clamp), karts baked from a little voxel model in eight views, trackside
# billboards and a sky.

KC_TRACK = [(-700, -150), (-640, -520), (-300, -700), (250, -650), (650, -420), (720, 0), (560, 380),
            (200, 520), (-120, 330), (-380, 520), (-660, 400)]


def kc_centre_line(n=400):
    """The track's centre line, a closed Catmull-Rom through KC_TRACK."""
    pts = KC_TRACK
    out = []
    m = len(pts)
    for i in range(m):
        p0, p1, p2, p3 = pts[(i - 1) % m], pts[i], pts[(i + 1) % m], pts[(i + 2) % m]
        for k in range(n // m):
            t = k / (n // m)
            t2, t3 = t * t, t * t * t
            out.append(tuple(0.5 * ((2 * p1[j]) + (-p0[j] + p2[j]) * t + (2 * p0[j] - 5 * p1[j] + 4 * p2[j] - p3[j]) * t2
                                    + (-p0[j] + 3 * p1[j] - 3 * p2[j] + p3[j]) * t3) for j in range(2)))
    return out


def kc140_track():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "kartcircuit"
    out.mkdir(parents=True, exist_ok=True)
    # The ground: 2048 x 2048 ground units on 1024 px, +y up the image.
    N = 1024
    k = N / 2048.0

    def px(p):
        return (p[0] * k + N / 2, N / 2 - p[1] * k)

    g = fbm(N, 16, 4)
    grass = np.stack([0.30 + 0.07 * g, 0.55 + 0.10 * g, 0.24 + 0.05 * g], -1)
    # Mowing stripes.
    yy = np.arange(N)[:, None] * np.ones((1, N))
    grass *= (1 + 0.05 * np.sign(np.sin(yy / 18.0)))[..., None]
    line = [px(p) for p in kc_centre_line()]
    layers = {}
    for name, width in (("curb", 92), ("road", 76), ("paint", 3)):
        im = _I.new("L", (N, N), 0)
        d = _D.Draw(im)
        d.line(line + [line[0]], fill=255, width=int(width * k * 2), joint="curve")
        for p in line:
            r = width * k
            d.ellipse((p[0] - r, p[1] - r, p[0] + r, p[1] + r), fill=255)
        layers[name] = np.asarray(im).astype(np.float64) / 255.0
    img = grass.copy()
    # Curbs: red and white blocks along the edge.
    ang = np.arctan2(yy - N / 2, np.arange(N)[None, :] - N / 2)
    blocks = (np.floor(ang * 40) % 2)[..., None]
    curb = blocks * np.array([0.92, 0.92, 0.9]) + (1 - blocks) * np.array([0.82, 0.16, 0.14])
    img = img * (1 - layers["curb"][..., None]) + curb * layers["curb"][..., None]
    asphalt = np.stack([0.30 + 0.05 * g, 0.30 + 0.05 * g, 0.33 + 0.05 * g], -1)
    img = img * (1 - layers["road"][..., None]) + asphalt * layers["road"][..., None]
    # A dashed centre line.
    dash = _I.new("L", (N, N), 0)
    dd = _D.Draw(dash)
    for i in range(0, len(line) - 1, 2):
        if (i // 2) % 2 == 0:
            dd.line([line[i], line[i + 1]], fill=255, width=3)
    dsh = np.asarray(dash).astype(np.float64)[..., None] / 255.0
    img = img * (1 - dsh) + 0.95 * dsh
    # The start line: a chequer across the track at the first point.
    s = px(KC_TRACK[0])
    for i in range(-6, 6):
        for j in range(0, 2):
            c = 0.95 if (i + j) % 2 == 0 else 0.08
            x0, y0 = int(s[0]) - 2 + j * 4, int(s[1]) + i * 6
            img[max(0, y0):y0 + 6, max(0, x0):x0 + 4] = c
    save_rgba(out / "track.png", np.clip(img, 0, 1), np.ones((N, N)))

    # Karts: a voxel kart turned through eight views and stacked upward, as
    # a sprite stack would draw it - frame 0 seen from behind.
    def kart_voxels(body):
        n, h = 20, 8
        z, y, x = np.mgrid[0:h, 0:n, 0:n]
        x = x + 0.5 - n / 2
        y = y + 0.5 - n / 2
        vox = np.zeros((h, n, n, 4))

        def put(mask, rgb):
            for c in range(3):
                vox[..., c] = np.where(mask, rgb[c], vox[..., c])
            vox[..., 3] = np.where(mask, 1.0, vox[..., 3])
        put((np.abs(x) < 8) & (np.abs(y) < 4.5) & (z >= 2) & (z < 4), body)
        put((x > 5) & (x < 9) & (np.abs(y) < 3.5) & (z >= 2) & (z < 3), tuple(c * 0.8 for c in body))
        for wx in (-5.5, 5):
            for wy in (-5.5, 5.5):
                put((np.abs(x - wx) < 2.2) & (np.abs(y - wy) < 1.4) & (z < 3), (0.12, 0.12, 0.13))
        put((np.abs(x + 1) < 2.5) & (np.abs(y) < 2.5) & (z >= 4) & (z < 7), (0.95, 0.85, 0.7))
        put((np.abs(x + 1) < 2.4) & (np.abs(y) < 2.4) & (z == 7), (0.2, 0.2, 0.25))
        put((x < -6) & (np.abs(y) < 4) & (z >= 4) & (z < 6), (0.15, 0.15, 0.18))
        return vox

    def bake(vox, frames, scale, rise):
        h, n = vox.shape[0], vox.shape[1]
        W = int(n * scale * 1.6)
        sheet = _I.new("RGBA", (W * frames, W), (0, 0, 0, 0))
        for f in range(frames):
            # Frame f: the kart's heading is f/frames of a turn anticlockwise
            # from "away from the camera" (screen up).
            a = f / frames * 360.0 + 90.0
            frame = _I.new("RGBA", (W, W), (0, 0, 0, 0))
            for zi in range(h):
                sl = (np.clip(vox[zi], 0, 1) * 255).astype(np.uint8)
                shade = 0.7 + 0.3 * zi / max(h - 1, 1)
                sl = sl.astype(np.float64)
                sl[..., :3] *= shade
                im = _I.fromarray(np.clip(sl, 0, 255).astype(np.uint8), "RGBA").resize((int(n * scale),) * 2, _I.NEAREST)
                im = im.rotate(a, resample=_I.NEAREST, expand=True)
                for kk in range(3):
                    oy = int(W * 0.62 - (zi + kk / 3) * rise - im.height / 2)
                    frame.alpha_composite(im, ((W - im.width) // 2, max(0, oy)))
            sheet.alpha_composite(frame, (f * W, 0))
        a = np.asarray(sheet).astype(np.float64) / 255.0
        return a

    for name, body in (("kart_red", (0.85, 0.15, 0.15)), ("kart_blue", (0.15, 0.35, 0.85)),
                       ("kart_green", (0.15, 0.7, 0.3)), ("kart_yellow", (0.95, 0.8, 0.15))):
        a = bake(kart_voxels(body), 8, 2.0, 2.4)
        save_rgba(out / (name + ".png"), a[..., :3], a[..., 3])

    # Billboards seen from anywhere: a pine, a tyre stack, a sign.
    S = 4

    def board(name, size, draw):
        im = _I.new("RGBA", (size[0] * S, size[1] * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        draw(d, lambda *p: [(x * S, y * S) for x, y in p])
        im = im.resize(size, _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    def pine(d, P):
        d.rectangle(P((28, 70), (36, 80)), fill=(90, 60, 35, 255))
        for i, (w, y) in enumerate(((30, 50), (24, 34), (17, 18))):
            d.polygon(P((32 - w, y + 24), (32, y - 6), (32 + w, y + 24)), fill=(30 + i * 12, 95 + i * 18, 50 + i * 8, 255))
    board("pine.png", (64, 80), pine)

    def tyres(d, P):
        for i in range(3):
            y = 44 - i * 14
            d.ellipse(P((6, y), (58, y + 18)), fill=(25, 25, 28, 255), outline=(70, 70, 75, 255), width=2 * S)
            d.ellipse(P((22, y + 5), (42, y + 13)), fill=(45, 45, 50, 255))
    board("tyres.png", (64, 64), tyres)

    def sign(d, P):
        d.rectangle(P((10, 30), (14, 78)), fill=(120, 120, 125, 255))
        d.rectangle(P((50, 30), (54, 78)), fill=(120, 120, 125, 255))
        d.rectangle(P((2, 6), (62, 36)), fill=(250, 220, 40, 255), outline=(30, 30, 30, 255), width=2 * S)
        d.polygon(P((22, 12), (40, 21), (22, 30)), fill=(30, 30, 30, 255))
    board("sign.png", (64, 80), sign)

    # The sky: a gradient with hills, wide enough to scroll with the turn.
    W, H = 2048, 256
    gg = np.linspace(0, 1, H)[:, None] * np.ones((1, W))
    sky = np.stack([0.45 + 0.35 * gg, 0.65 + 0.25 * gg, 0.95 - 0.05 * gg], -1)
    xs = np.arange(W)
    hills = H - (60 + 30 * np.sin(xs / W * math.tau * 3) + 18 * np.sin(xs / W * math.tau * 7 + 1))
    far = H - (35 + 25 * np.sin(xs / W * math.tau * 5 + 2) + 10 * np.sin(xs / W * math.tau * 11))
    ys = np.arange(H)[:, None] * np.ones((1, W))
    m1 = (ys > far[None, :])[..., None]
    sky = sky * (1 - m1) + np.array([0.55, 0.62, 0.75]) * m1
    m2 = (ys > hills[None, :])[..., None]
    sky = sky * (1 - m2) + np.array([0.25, 0.45, 0.28]) * m2
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H, W)))


# ── 141 Pocket Palette ───────────────────────────────────────────────────
# A sunset bay painted in smooth gradients, for the `palette` effect to
# quantise: warm sky, a sun, cliffs with a castle and a waterfall, and a sea
# whose blues are the only blues - so cycling them moves the water alone.

def pp141_bay():
    from PIL import Image as _I, ImageDraw as _D
    out = OUT / "pocketpalette"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    g = fbm_rect(W, H, 8, 5)
    horizon = 420
    # Sky: deep purple overhead to coral and gold at the horizon.
    t = np.clip(ys / horizon, 0, 1)
    top, mid, low = np.array([0.16, 0.08, 0.26]), np.array([0.72, 0.26, 0.42]), np.array([1.0, 0.72, 0.36])
    sky = np.where((t < 0.6)[..., None], top + (mid - top) * (t / 0.6)[..., None], mid + (low - mid) * ((t - 0.6) / 0.4)[..., None])
    # The sun, low, with a glow.
    d = np.hypot(xs - 760, ys - 380)
    sky = sky + np.clip(1.0 - d / 260, 0, 1)[..., None] ** 2 * np.array([0.4, 0.3, 0.1])
    sky = np.where((d < 60)[..., None], np.array([1.0, 0.95, 0.75]), sky)
    # Clouds: plum streaks.
    cl = np.clip((fbm_rect(W, H, 6, 4) - 0.55) * 4, 0, 1) * (ys < 330)
    sky = sky * (1 - cl[..., None] * 0.5) + np.array([0.48, 0.18, 0.36]) * cl[..., None] * 0.5
    img = sky
    # Sea: blues only, darker far out, bright where the sun lies on it.
    sea_t = np.clip((ys - horizon) / (H - horizon), 0, 1)
    sea = np.array([0.12, 0.24, 0.48]) + (np.array([0.17, 0.38, 0.66]) - np.array([0.12, 0.24, 0.48])) * sea_t[..., None]
    ripples = 0.5 + 0.5 * np.sin(xs / 9.0 + np.sin(ys / 5.0) * 3 + g * 6)
    path = np.clip(1 - np.abs(xs - 760) / (40 + (ys - horizon) * 1.6), 0, 1) * (ripples > 0.6)
    sea = sea + (np.array([0.31, 0.58, 0.84]) - sea) * (ripples * 0.4)[..., None]
    sea = sea + (np.array([0.66, 0.86, 0.96]) - sea) * path[..., None]
    img = np.where((ys >= horizon)[..., None], sea, img)
    # Cliffs on the left with grass on top; a waterfall down their face.
    cliff_edge = 380 + 120 * np.sin(ys / 140.0) * 0 + 40 * g
    cliff_top = 300 + 30 * np.sin(xs / 70.0)
    cliff = (xs < cliff_edge - (ys - 300) * 0.15) & (ys > cliff_top)
    rock = np.array([0.29, 0.23, 0.31]) + (np.array([0.42, 0.32, 0.39]) - np.array([0.29, 0.23, 0.31])) * g[..., None]
    img = np.where(cliff[..., None], rock, img)
    grass = cliff & (ys < cliff_top + 18)
    img = np.where(grass[..., None], np.array([0.18, 0.35, 0.23]) * (0.8 + 0.4 * g[..., None]), img)
    fall = cliff & (np.abs(xs - 220) < 26 + (ys - 320) * 0.04) & (ys > 330)
    stripes = 0.5 + 0.5 * np.sin(xs / 3.0 + ys / 30.0)
    img = np.where(fall[..., None], np.array([0.17, 0.38, 0.66]) + np.array([0.5, 0.48, 0.3]) * stripes[..., None], img)
    # A castle on the cliff, in silhouette against the sky.
    im = _I.new("L", (W, H), 0)
    dr = _D.Draw(im)
    dr.rectangle((90, 200, 230, 300), fill=255)
    for x0 in (80, 210):
        dr.rectangle((x0, 150, x0 + 34, 300), fill=255)
        for k in range(3):
            dr.rectangle((x0 + k * 12, 138, x0 + k * 12 + 8, 150), fill=255)
    for k in range(6):
        dr.rectangle((118 + k * 18, 188, 128 + k * 18, 200), fill=255)
    dr.polygon([(140, 200), (160, 150), (180, 200)], fill=255)
    castle = np.asarray(im).astype(np.float64)[..., None] / 255.0
    img = img * (1 - castle) + np.array([0.12, 0.07, 0.16]) * castle
    # A few lit windows.
    for (wx, wy) in ((100, 180), (222, 200), (150, 240), (190, 250)):
        img[wy:wy + 10, wx:wx + 6] = [1.0, 0.85, 0.45]
    save_rgba(out / "bay.png", np.clip(img, 0, 1), np.ones((H, W)))

    # A gull, two frames of wing.
    S = 4
    for k, lift in enumerate((10, -4)):
        im = _I.new("RGBA", (40 * S, 20 * S), (0, 0, 0, 0))
        dr = _D.Draw(im)
        dr.line([(4 * S, (10 + lift * 0.4) * S), (20 * S, 12 * S), (36 * S, (10 + lift * 0.4) * S)], fill=(250, 245, 235, 255), width=3 * S, joint="curve")
        dr.ellipse((17 * S, 10 * S, 23 * S, 15 * S), fill=(250, 245, 235, 255))
        im = im.resize((40, 20), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"gull{k}.png", a[..., :3], a[..., 3])


# ── 142 Behind the Walls ─────────────────────────────────────────────────
# A manor from above at night: courtyards and gardens open to the sky, the
# rooms under roofs and the paths under tree crowns - one overlay whose
# alpha is what hides - and a thief and two guards to be hidden by it.

BW_ROOFS = [  # x0, y0, x1, y1 in the 1280 x 720 picture
    (60, 60, 520, 250), (60, 250, 250, 520), (760, 80, 1220, 300), (980, 300, 1220, 640), (420, 470, 760, 660)]
BW_TREES = [(640, 160, 70), (330, 380, 60), (820, 420, 66), (560, 330, 48), (1160, 60, 50)]


def bw142_manor():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "behindthewalls"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    g = fbm_rect(W, H, 10, 4)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    # The ground: flagstones in the courtyards, grass at the edges, the
    # floors of the rooms (seen only through the thief's outline).
    grass = np.stack([0.16 + 0.05 * g, 0.24 + 0.07 * g, 0.14 + 0.04 * g], -1)
    flag = (np.floor(xs / 46) + np.floor(ys / 46)) % 2
    stone = np.stack([0.30 + 0.06 * g + 0.03 * flag] * 3, -1) * np.array([1.0, 0.98, 1.05])
    seams = ((xs % 46 < 2) | (ys % 46 < 2))[..., None]
    stone = stone * (1 - 0.35 * seams)
    court = (np.abs(xs - 640) < 470) & (np.abs(ys - 360) < 300)
    img = np.where(court[..., None], stone, grass)
    for (x0, y0, x1, y1) in BW_ROOFS:
        room = (xs >= x0) & (xs < x1) & (ys >= y0) & (ys < y1)
        boards = 0.28 + 0.05 * ((ys // 14) % 2) + 0.04 * g
        img = np.where(room[..., None], np.stack([boards, boards * 0.72, boards * 0.5], -1), img)
    save_rgba(out / "ground.png", np.clip(img, 0, 1), np.ones((H, W)))

    # The overlay: roofs (tiles, a ridge, eaves) and tree crowns.
    S = 2
    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    for (x0, y0, x1, y1) in BW_ROOFS:
        d.rectangle((x0 * S, y0 * S, x1 * S, y1 * S), fill=(96, 40, 36, 255), outline=(40, 18, 16, 255), width=4 * S)
        horizontal = (x1 - x0) >= (y1 - y0)
        if horizontal:
            for y in range(y0 + 10, y1, 12):
                d.line((x0 * S, y * S, x1 * S, y * S), fill=(76, 30, 28, 255), width=2 * S)
            my = (y0 + y1) // 2
            d.line((x0 * S, my * S, x1 * S, my * S), fill=(130, 60, 50, 255), width=5 * S)
        else:
            for x in range(x0 + 10, x1, 12):
                d.line((x * S, y0 * S, x * S, y1 * S), fill=(76, 30, 28, 255), width=2 * S)
            mx = (x0 + x1) // 2
            d.line((mx * S, y0 * S, mx * S, y1 * S), fill=(130, 60, 50, 255), width=5 * S)
        cx, cy = x0 + (x1 - x0) * 0.75, y0 + (y1 - y0) * 0.3
        d.rectangle(((cx - 10) * S, (cy - 10) * S, (cx + 10) * S, (cy + 10) * S), fill=(70, 66, 70, 255), outline=(30, 28, 30, 255), width=2 * S)
    for (cx, cy, r) in BW_TREES:
        for k in range(14):
            a = k / 14 * math.tau
            rr = r * (0.55 + 0.25 * ((k * 7) % 3) / 2)
            ox, oy = cx + math.cos(a) * r * 0.45, cy + math.sin(a) * r * 0.45
            d.ellipse(((ox - rr) * S, (oy - rr) * S, (ox + rr) * S, (oy + rr) * S), fill=(24, 58, 34, 255))
        d.ellipse(((cx - r * 0.6) * S, (cy - r * 0.6) * S, (cx + r * 0.2) * S, (cy + r * 0.2) * S), fill=(38, 84, 46, 255))
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "roofs.png", a[..., :3], a[..., 3])

    # The thief and a guard, from above, facing +x.
    def figure(name, cloak, trim, lamp):
        S2 = 4
        im = _I.new("RGBA", (48 * S2, 48 * S2), (0, 0, 0, 0))
        d = _D.Draw(im)

        def P(*pts):
            return [(x * S2, y * S2) for x, y in pts]
        d.ellipse(P((10, 12), (38, 36)), fill=cloak + (255,), outline=trim + (255,), width=2 * S2)
        d.ellipse(P((20, 17), (34, 31)), fill=tuple(int(c * 0.7) for c in cloak) + (255,))
        d.polygon(P((34, 20), (44, 24), (34, 28)), fill=trim + (255,))
        if lamp:
            d.ellipse(P((30, 4), (40, 14)), fill=(255, 210, 110, 255))
        im = im.resize((48, 48), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    figure("thief.png", (52, 40, 70), (140, 120, 170), False)
    figure("guard.png", (60, 76, 110), (200, 200, 210), True)


# ── 143 Card Table ───────────────────────────────────────────────────────
# A card table seen from above: green baize in a wooden rim, five card
# faces, an ornate back, and the two faces of a gold coin.

def ct143_cards():
    from PIL import Image as _I, ImageDraw as _D, ImageFont as _F, ImageFilter as _FI
    out = OUT / "cardtable"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    # The table: baize with a soft light over the middle, a wooden rim.
    g = fbm_rect(W, H, 40, 3)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    r = np.sqrt(((xs - W / 2) / (W * 0.62)) ** 2 + ((ys - H / 2) / (H * 0.62)) ** 2)
    light = np.clip(1.15 - 0.55 * r ** 2, 0.35, 1.15)
    baize = np.stack([0.05 + 0.02 * g, 0.36 + 0.06 * g, 0.20 + 0.04 * g], -1) * light[..., None]
    rim = 34
    edge = np.minimum(np.minimum(xs, W - 1 - xs), np.minimum(ys, H - 1 - ys))
    wood_g = fbm_rect(W, H, 6, 4)
    grain = 0.5 + 0.5 * np.sin(xs * 0.02 + ys * 0.35 + wood_g * 6.0)
    wood = np.stack([0.36 + 0.10 * grain, 0.20 + 0.06 * grain, 0.10 + 0.03 * grain], -1)
    bevel = np.clip(edge / rim, 0, 1)[..., None]
    wood = wood * (0.65 + 0.5 * bevel)
    img = np.where((edge < rim)[..., None], wood, baize)
    # A stitched line round the baize.
    stitch = (np.abs(edge - rim - 14) < 1.2) & (((xs + ys) // 9) % 2 == 0)
    img = np.where(stitch[..., None], img * 0.6 + 0.25, img)
    save_rgba(out / "table.png", np.clip(img, 0, 1), np.ones((H, W)))

    # The cards: 200 x 280, drawn at twice that and shrunk.
    S = 2
    CW, CH = 200, 280
    rank_font = _F.truetype("georgiab.ttf", 46 * S)
    pip_font = _F.truetype("seguisym.ttf", 40 * S)
    big_font = _F.truetype("seguisym.ttf", 120 * S)
    face_font = _F.truetype("georgiab.ttf", 96 * S)
    RED, BLACK = (186, 26, 36), (24, 22, 30)

    def blank():
        im = _I.new("RGBA", (CW * S, CH * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        d.rounded_rectangle((2 * S, 2 * S, (CW - 2) * S, (CH - 2) * S), radius=16 * S,
                            fill=(250, 246, 236, 255), outline=(150, 140, 120, 255), width=2 * S)
        return im, d

    def corner(im, rank, suit, colour):
        # Drawn on a layer of its own, then put in the opposite corner
        # turned over - so nothing under the corner is copied with it.
        layer = _I.new("RGBA", (64 * S, 110 * S), (0, 0, 0, 0))
        d = _D.Draw(layer)
        box = d.textbbox((0, 0), rank, font=rank_font)
        d.text(((64 * S - (box[2] - box[0])) / 2 - box[0], 6 * S - box[1]), rank, font=rank_font, fill=colour)
        box = d.textbbox((0, 0), suit, font=pip_font)
        d.text(((64 * S - (box[2] - box[0])) / 2 - box[0], 60 * S - box[1]), suit, font=pip_font, fill=colour)
        im.alpha_composite(layer, (6 * S, 8 * S))
        im.alpha_composite(layer.rotate(180), ((CW - 70) * S, (CH - 118) * S))

    def finish(im, name):
        im = im.resize((CW, CH), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    def centred(d, text, font, colour, cy):
        box = d.textbbox((0, 0), text, font=font)
        w, h = box[2] - box[0], box[3] - box[1]
        d.text(((CW * S - w) / 2 - box[0], cy * S - h / 2 - box[1]), text, font=font, fill=colour)

    cards = [("ace_spades.png", "A", "♠", BLACK), ("king_hearts.png", "K", "♥", RED),
             ("queen_diamonds.png", "Q", "♦", RED), ("jack_clubs.png", "J", "♣", BLACK),
             ("ten_hearts.png", "10", "♥", RED)]
    for name, rank, suit, colour in cards:
        im, d = blank()
        corner(im, rank, suit, colour)
        if rank == "A":
            centred(d, suit, big_font, colour, 140)
        elif rank in ("K", "Q", "J"):
            # A framed court panel: the letter large over a band of its suit.
            d.rounded_rectangle((60 * S, 40 * S, 140 * S, 240 * S), radius=8 * S,
                                fill=(238, 224, 190, 255), outline=colour, width=3 * S)
            d.rectangle((66 * S, 158 * S, 134 * S, 170 * S), fill=(212, 170, 60, 255))
            centred(d, rank, face_font, colour, 102)
            centred(d, suit, pip_font, colour, 196)
        else:
            for (px, py) in [(78, 60), (122, 60), (100, 92), (78, 120), (122, 120),
                             (78, 160), (122, 160), (100, 188), (78, 220), (122, 220)]:
                box = d.textbbox((0, 0), suit, font=pip_font)
                w, h = box[2] - box[0], box[3] - box[1]
                d.text((px * S - w / 2 - box[0], py * S - h / 2 - box[1]), suit, font=pip_font, fill=colour)
        finish(im, name)

    # The back: a deep red lattice in a gold border.
    im = _I.new("RGBA", (CW * S, CH * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.rounded_rectangle((2 * S, 2 * S, (CW - 2) * S, (CH - 2) * S), radius=16 * S,
                        fill=(250, 246, 236, 255), outline=(150, 140, 120, 255), width=2 * S)
    d.rounded_rectangle((14 * S, 14 * S, (CW - 14) * S, (CH - 14) * S), radius=8 * S, fill=(120, 20, 34, 255))
    for k in range(-CH, CW + CH, 18):
        d.line(((14 + k) * S, 14 * S, (14 + k + CH) * S, (14 + CH) * S), fill=(160, 40, 52, 255), width=3 * S)
        d.line(((14 + k) * S, (CH - 14) * S, (14 + k + CH) * S, (CH - 14 - CH) * S), fill=(160, 40, 52, 255), width=3 * S)
    mask = _I.new("L", (CW * S, CH * S), 0)
    _D.Draw(mask).rounded_rectangle((14 * S, 14 * S, (CW - 14) * S, (CH - 14) * S), radius=8 * S, fill=255)
    base = _I.new("RGBA", (CW * S, CH * S), (0, 0, 0, 0))
    _D.Draw(base).rounded_rectangle((2 * S, 2 * S, (CW - 2) * S, (CH - 2) * S), radius=16 * S,
                                    fill=(250, 246, 236, 255), outline=(150, 140, 120, 255), width=2 * S)
    base.paste(im, (0, 0), mask)
    d = _D.Draw(base)
    d.rounded_rectangle((14 * S, 14 * S, (CW - 14) * S, (CH - 14) * S), radius=8 * S, outline=(214, 172, 70, 255), width=4 * S)
    d.ellipse(((CW / 2 - 34) * S, (CH / 2 - 34) * S, (CW / 2 + 34) * S, (CH / 2 + 34) * S),
              fill=(120, 20, 34, 255), outline=(214, 172, 70, 255), width=4 * S)
    centred(d, "♠", pip_font, (214, 172, 70, 255), CH / 2)
    base = base.resize((CW, CH), _I.LANCZOS)
    a = np.asarray(base).astype(np.float64) / 255.0
    save_rgba(out / "card_back.png", a[..., :3], a[..., 3])

    # A gold coin: a crown on one face, a big numeral on the other.
    for name, mark in (("coin_heads.png", "♚"), ("coin_tails.png", "V")):
        C = 128 * S
        im = _I.new("RGBA", (C, C), (0, 0, 0, 0))
        d = _D.Draw(im)
        d.ellipse((2 * S, 2 * S, C - 2 * S, C - 2 * S), fill=(214, 166, 52, 255), outline=(120, 80, 20, 255), width=3 * S)
        d.ellipse((14 * S, 14 * S, C - 14 * S, C - 14 * S), outline=(250, 214, 110, 255), width=3 * S)
        f = _F.truetype("seguisym.ttf" if mark != "V" else "georgiab.ttf", 64 * S)
        box = d.textbbox((0, 0), mark, font=f)
        w, h = box[2] - box[0], box[3] - box[1]
        d.text(((C - w) / 2 - box[0], (C - h) / 2 - box[1]), mark, font=f, fill=(140, 92, 24, 255))
        im = im.resize((128, 128), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])
    # A card's shadow: a soft dark rounded rectangle.
    sh = _I.new("L", (CW + 40, CH + 40), 0)
    _D.Draw(sh).rounded_rectangle((20, 20, CW + 20, CH + 20), radius=18, fill=255)
    sh = sh.filter(_FI.GaussianBlur(9))
    a = np.asarray(sh).astype(np.float64) / 255.0
    save_rgba(out / "card_shadow.png", np.zeros(a.shape + (3,)), a * 0.55)


# ── 144 Dash Echoes ──────────────────────────────────────────────────────
# Rooftops at night: a sky with a moon, a far skyline with lit windows, the
# near roofs (with gaps to dash across) and a six-frame ninja run.

DE_ROOFS = [(-640, -330), (-250, 90), (190, 640)]  # world x ranges, tops at y -150
DE_TOP = -150


def de144_rooftops():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "dashechoes"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    t = ys / H
    sky = np.stack([0.05 + 0.20 * t, 0.04 + 0.08 * t, 0.16 + 0.16 * t], -1)
    # The moon and its halo.
    mx, my = 960, 150
    d = np.sqrt((xs - mx) ** 2 + (ys - my) ** 2)
    halo = np.exp(-(d / 160.0) ** 2) * 0.35
    sky = sky + halo[..., None] * np.array([0.7, 0.75, 0.9])
    moon = np.clip((62 - d) / 2.0, 0, 1)[..., None]
    g = fbm_rect(W, H, 12, 3)
    sky = sky * (1 - moon) + moon * np.stack([0.92 + 0.05 * g, 0.9 + 0.05 * g, 0.82 + 0.05 * g], -1)
    # Stars.
    rng = np.random.default_rng(144)
    for _ in range(140):
        x, y = rng.integers(0, W), rng.integers(0, 380)
        sky[y, x] = np.maximum(sky[y, x], 0.75)
    save_rgba(out / "sky.png", np.clip(sky, 0, 1), np.ones((H, W)))

    # The far skyline: blocks with lit windows, one alpha layer.
    S = 2
    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    dr = _D.Draw(im)
    x = 0
    while x < W:
        w = int(rng.integers(50, 120))
        h = int(rng.integers(140, 330))
        top = 560 - h
        dr.rectangle((x * S, top * S, (x + w) * S, H * S), fill=(22, 20, 44, 255))
        for wy in range(top + 14, 560, 18):
            for wx in range(x + 8, x + w - 8, 14):
                if rng.random() < 0.28:
                    dr.rectangle((wx * S, wy * S, (wx + 6) * S, (wy + 9) * S), fill=(240, 196, 110, 255))
        x += w + int(rng.integers(4, 22))
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "skyline.png", a[..., :3], a[..., 3])

    # The near roofs: tiled tops and brick faces, gaps between them.
    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    dr = _D.Draw(im)
    top = 360 - DE_TOP
    for (x0, x1) in DE_ROOFS:
        px0, px1 = x0 + 640, x1 + 640
        dr.rectangle((px0 * S, (top + 16) * S, px1 * S, H * S), fill=(46, 32, 44, 255))
        for by in range(top + 22, H, 16):
            off = 0 if ((by - top) // 16) % 2 == 0 else 15
            for bx in range(px0 - 30 + off, px1, 30):
                if bx + 27 <= px0:
                    continue
                dr.rectangle((max(bx, px0) * S, by * S, min(bx + 27, px1) * S, (by + 13) * S), fill=(62, 42, 54, 255))
        dr.rectangle(((px0 - 6) * S, top * S, (px1 + 6) * S, (top + 16) * S), fill=(30, 26, 38, 255))
        dr.rectangle(((px0 - 6) * S, top * S, (px1 + 6) * S, (top + 4) * S), fill=(90, 84, 120, 255))
        # A chimney or a water tank on each roof.
        cx = px0 + (px1 - px0) * 0.7
        dr.rectangle(((cx - 18) * S, (top - 70) * S, (cx + 18) * S, top * S), fill=(40, 30, 42, 255))
        dr.rectangle(((cx - 22) * S, (top - 78) * S, (cx + 22) * S, (top - 68) * S), fill=(30, 26, 38, 255))
        # Lit windows on the faces.
        for wy in range(top + 50, H - 20, 70):
            for wx in range(int(px0) + 30, int(px1) - 40, 90):
                lit = rng.random() < 0.45
                dr.rectangle((wx * S, wy * S, (wx + 26) * S, (wy + 34) * S),
                             fill=(250, 200, 110, 255) if lit else (20, 18, 30, 255))
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "roofs.png", a[..., :3], a[..., 3])

    # The ninja: six frames of a run, 64 x 80 each, facing +x.
    FW, FH, S4 = 64, 80, 4
    sheet = _I.new("RGBA", (FW * 6 * S4, FH * S4), (0, 0, 0, 0))
    body, scarf = (58, 54, 88, 255), (220, 44, 56, 255)
    for k in range(6):
        p = k / 6 * math.tau
        dr = _D.Draw(sheet)
        ox = k * FW * S4

        def P(x, y):
            return (ox + x * S4, (FH - y) * S4)

        hip = (32, 34 + 2 * abs(math.sin(p)))
        neck = (36, 56 + 2 * abs(math.sin(p)))
        head = (39, 64 + 2 * abs(math.sin(p)))
        for side, ph in ((0, p), (1, p + math.pi)):
            a1 = math.sin(ph) * 0.9
            knee = (hip[0] + math.sin(a1) * 14, hip[1] - math.cos(a1) * 14)
            a2 = a1 - 0.6 - 0.6 * max(0.0, math.cos(ph))
            foot = (knee[0] + math.sin(a2) * 15, knee[1] - math.cos(a2) * 15)
            shade = body if side == 0 else (40, 38, 64, 255)
            dr.line([P(*hip), P(*knee), P(*foot)], fill=shade, width=7 * S4, joint="curve")
            b1 = -math.sin(ph) * 1.0
            elbow = (neck[0] + math.sin(b1) * 10, neck[1] - 2 - math.cos(b1) * 10)
            hand = (elbow[0] + math.sin(b1 + 1.2) * 10, elbow[1] - math.cos(b1 + 1.2) * 10)
            dr.line([P(*neck), P(*elbow), P(*hand)], fill=shade, width=5 * S4, joint="curve")
        dr.line([P(*hip), P(*neck)], fill=body, width=11 * S4)
        hx, hy = P(*head)
        dr.ellipse((hx - 8 * S4, hy - 8 * S4, hx + 8 * S4, hy + 8 * S4), fill=body)
        # The headband's tails stream behind, rippling with the stride.
        tx, ty = P(head[0] - 6, head[1] + 1)
        wob = math.sin(p * 2) * 3
        dr.line([(tx, ty), (tx - 12 * S4, ty + (2 + wob) * S4), (tx - 22 * S4, ty + (-1 - wob) * S4)], fill=scarf, width=3 * S4, joint="curve")
        dr.line([(hx - 8 * S4, hy - 2 * S4), (hx + 8 * S4, hy - 2 * S4)], fill=scarf, width=3 * S4)
        ex, ey = P(head[0] + 4, head[1] + 1)
        dr.ellipse((ex - 2 * S4, ey - 1 * S4, ex + 3 * S4, ey + 1 * S4), fill=(240, 240, 250, 255))
    sheet = sheet.resize((FW * 6, FH), _I.LANCZOS)
    a = np.asarray(sheet).astype(np.float64) / 255.0
    # A moonlit rim: the silhouette grown by a pixel, pale where the body
    # is not, brighter toward the moon (up and to the right).
    al = a[..., 3]
    grown = al.copy()
    for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1), (-1, 1), (1, -1), (-1, -1), (1, 1)):
        grown = np.maximum(grown, np.roll(np.roll(al, dy, 0), dx, 1))
    lit = np.maximum(0.0, np.roll(np.roll(al, 2, 0), -2, 1) - al)
    rim = np.clip(grown - al, 0, 1)
    rgb = a[..., :3] * al[..., None] + np.array([0.62, 0.66, 0.9]) * rim[..., None]
    rgb = rgb + np.array([0.35, 0.38, 0.5]) * (lit * al)[..., None]
    out_a = np.maximum(al, rim)
    rgb = np.where(out_a[..., None] > 0, rgb / np.maximum(out_a, 1e-6)[..., None], 0)
    save_rgba(out / "ninja_run.png", np.clip(rgb, 0, 1), out_a)


# ── 145 Oil and Ink ──────────────────────────────────────────────────────
# A harbour town on a bright afternoon, with plenty of fine detail for a
# painterly filter to turn into strokes: clouds, hills, a row of painted
# houses, a quay, the water and its reflections; and two boats and a
# lighthouse beam as sprites of their own.

def oi145_harbour():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "oilandink"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    rng = np.random.default_rng(145)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    HORIZON = 300   # where the sea meets the hills
    QUAY = 430      # the quay's top edge
    # Sky: a blue gradient with fbm clouds.
    t = ys / HORIZON
    sky = np.stack([0.38 + 0.35 * t, 0.62 + 0.25 * t, 0.92 + 0.05 * t], -1)
    cl = fbm_rect(W, H, 7, 5)
    cl2 = fbm_rect(W, H, 18, 3)
    cloud = np.clip((cl * 0.8 + cl2 * 0.35 - 0.52) * 3.2, 0, 1) * np.clip(1.2 - t, 0, 1)
    shade_c = 0.82 + 0.18 * np.clip(cl2 * 1.4 - 0.2, 0, 1)
    sky = sky * (1 - cloud[..., None]) + cloud[..., None] * np.stack([shade_c, shade_c, shade_c * 1.02], -1)
    img = sky
    # Two ranges of hills, the far one hazier.
    g = fbm_rect(W, H, 5, 4)
    for top, amp, col, haze in ((235, 40, (0.42, 0.55, 0.50), 0.45), (262, 30, (0.30, 0.46, 0.28), 0.15)):
        ridge = top - amp * (np.sin(xs[0] / 170.0 + top) * 0.5 + g[top, :] * 1.2)
        mask = ys > ridge[None, :]
        tex = fbm_rect(W, H, 40, 3)
        c = np.stack([col[0] + 0.08 * tex, col[1] + 0.08 * tex, col[2] + 0.05 * tex], -1)
        c = c * (1 - haze) + np.array([0.70, 0.80, 0.90]) * haze
        img = np.where(mask[..., None], c, img)
    # The sea from the horizon down, darker toward us, with ripples.
    wt = (ys - HORIZON) / (H - HORIZON)
    rip = fbm_rect(W, H, 60, 3)
    rip_line = 0.5 + 0.5 * np.sin(ys * 0.9 + rip * 9.0)
    sea = np.stack([0.10 + 0.06 * rip_line, 0.36 + 0.10 * rip_line - 0.1 * wt, 0.52 + 0.10 * rip_line - 0.08 * wt], -1)
    img = np.where((ys > QUAY)[..., None], sea, img)
    base = np.clip(img, 0, 1)
    # The town: drawn with PIL at twice the size, over the hills and sea.
    S = 2
    town = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = _D.Draw(town)
    walls = [(222, 120, 80), (240, 200, 120), (120, 170, 200), (230, 230, 220), (200, 90, 90), (150, 190, 140), (245, 170, 90), (180, 140, 200)]
    roofs = [(150, 60, 50), (110, 50, 40), (70, 70, 90), (160, 80, 50)]
    x = 20
    houses = []
    while x < W - 20:
        w = int(rng.integers(70, 120))
        h = int(rng.integers(90, 170))
        bottom = QUAY + 4
        top = bottom - h
        wall = walls[int(rng.integers(len(walls)))]
        roof = roofs[int(rng.integers(len(roofs)))]
        houses.append((x, top, w, h))
        d.rectangle((x * S, top * S, (x + w) * S, bottom * S), fill=wall + (255,))
        # Plaster texture: speckles a shade off the wall.
        for _ in range(int(w * h / 18)):
            px, py = x + rng.random() * w, top + rng.random() * h
            k = rng.uniform(-22, 22)
            sp = tuple(int(np.clip(c + k, 0, 255)) for c in wall)
            d.rectangle((px * S, py * S, (px + 1.5) * S, (py + 1.5) * S), fill=sp + (255,))
        peak = top - int(rng.integers(26, 46))
        d.polygon([((x - 6) * S, top * S), ((x + w / 2) * S, peak * S), ((x + w + 6) * S, top * S)], fill=roof + (255,))
        for k in range(top - 2, peak, -5):
            span = (k - peak) / max(top - peak, 1) * (w / 2 + 6)
            d.line(((x + w / 2 - span) * S, k * S, (x + w / 2 + span) * S, k * S), fill=tuple(int(c * 0.82) for c in roof) + (255,), width=S)
        # Windows with shutters, a door.
        for wy in range(top + 18, bottom - 40, 36):
            for wx in range(x + 12, x + w - 22, 30):
                d.rectangle((wx * S, wy * S, (wx + 14) * S, (wy + 20) * S), fill=(40, 50, 70, 255))
                d.line(((wx + 7) * S, wy * S, (wx + 7) * S, (wy + 20) * S), fill=(230, 230, 230, 255), width=S)
                shutter = (60, 110, 90, 255) if rng.random() < 0.5 else (60, 80, 140, 255)
                d.rectangle(((wx - 6) * S, wy * S, (wx - 1) * S, (wy + 20) * S), fill=shutter)
                d.rectangle(((wx + 15) * S, wy * S, (wx + 20) * S, (wy + 20) * S), fill=shutter)
                if rng.random() < 0.4:
                    d.rectangle(((wx - 2) * S, (wy + 20) * S, (wx + 16) * S, (wy + 26) * S), fill=(200, 60, 80, 255))
                    for f in range(4):
                        fx = wx + 1 + f * 4
                        d.ellipse((fx * S, (wy + 17) * S, (fx + 4) * S, (wy + 21) * S), fill=(250, 220, 90, 255))
        dx = x + w // 2 - 10
        d.rectangle((dx * S, (bottom - 34) * S, (dx + 20) * S, bottom * S), fill=(90, 60, 40, 255))
        x += w + int(rng.integers(2, 10))
    town = town.resize((W, H), _I.LANCZOS)
    ta = np.asarray(town).astype(np.float64) / 255.0
    img = base * (1 - ta[..., 3:]) + ta[..., :3] * ta[..., 3:]
    # The quay: cut stone with a bollard row.
    qmask = (ys >= QUAY) & (ys < QUAY + 34)
    st = fbm_rect(W, H, 30, 3)
    joints = ((np.abs(((xs + (ys // 12 % 2) * 20) % 40) - 20) > 19) | (np.abs((ys - QUAY) % 12) < 1)).astype(float)
    stone = np.stack([0.62 + 0.10 * st, 0.58 + 0.10 * st, 0.52 + 0.08 * st], -1) * (1 - 0.35 * joints[..., None])
    img = np.where(qmask[..., None], stone, img)
    # The town's reflection, broken by the ripples, under the quay.
    below = ys >= QUAY + 34
    src_y = np.clip(2 * (QUAY + 34) - ys + (rip * 10 - 5), 0, H - 1).astype(int)
    src_x = np.clip(xs + np.sin(ys * 0.35 + rip * 6.0) * 6, 0, W - 1).astype(int)
    refl = img[src_y, src_x]
    fade = np.clip(1.0 - (ys - QUAY - 34) / 260.0, 0, 1)[..., None] * 0.55
    mixed = sea * (1 - fade) + refl * fade
    img = np.where(below[..., None], mixed, img)
    # Glints on the water.
    gl = (rip_line > 0.93) & (rng.random((H, W)) < 0.25) & below
    img = np.where(gl[..., None], np.minimum(img + 0.35, 1.0), img)
    save_rgba(out / "harbour.png", np.clip(img, 0, 1), np.ones((H, W)))

    # Two boats (facing +x): a red fishing boat and a white sailing dinghy.
    for name, hull, deck, sail in (("boat_red", (190, 50, 40), (240, 230, 210), None),
                                   ("boat_sail", (245, 245, 240), (90, 120, 160), (250, 246, 230))):
        BW, BH = 200, 200
        im = _I.new("RGBA", (BW * S, BH * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        wl = 150  # waterline
        d.polygon([(20 * S, (wl - 30) * S), (180 * S, (wl - 34) * S), (160 * S, wl * S), (34 * S, wl * S)], fill=hull + (255,))
        d.line((20 * S, (wl - 30) * S, 180 * S, (wl - 34) * S), fill=deck + (255,), width=5 * S)
        d.line((30 * S, (wl - 14) * S, 172 * S, (wl - 18) * S), fill=(40, 40, 50, 255), width=3 * S)
        if sail is None:
            d.rectangle((70 * S, (wl - 70) * S, 120 * S, (wl - 32) * S), fill=(235, 225, 200, 255), outline=(60, 50, 40, 255), width=2 * S)
            d.rectangle((80 * S, (wl - 62) * S, 110 * S, (wl - 48) * S), fill=(60, 90, 120, 255))
            d.line((140 * S, (wl - 33) * S, 150 * S, (wl - 110) * S), fill=(80, 60, 40, 255), width=3 * S)
            d.line((150 * S, (wl - 110) * S, 176 * S, (wl - 40) * S), fill=(60, 60, 60, 255), width=S)
        else:
            d.line((96 * S, (wl - 32) * S, 96 * S, 10 * S), fill=(110, 90, 60, 255), width=4 * S)
            d.polygon([(100 * S, 14 * S), (100 * S, (wl - 40) * S), (170 * S, (wl - 40) * S)], fill=sail + (255,))
            d.polygon([(92 * S, 26 * S), (92 * S, (wl - 40) * S), (40 * S, (wl - 40) * S)], fill=(230, 120, 90, 255))
        im = im.resize((BW, BH), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", a[..., :3], a[..., 3])
    # A soft cloud for drifting over the sky.
    cw, ch = 360, 140
    yy, xx = np.mgrid[0:ch, 0:cw]
    lumps = np.zeros((ch, cw))
    for _ in range(9):
        cx, cy, r = rng.uniform(70, cw - 70), rng.uniform(55, ch - 45), rng.uniform(28, 55)
        lumps = np.maximum(lumps, np.clip(1.0 - ((xx - cx) ** 2 + (yy - cy) ** 2) / r ** 2, 0, 1))
    a = np.clip(lumps * 1.8, 0, 1) ** 0.8
    shade = 0.86 + 0.14 * np.clip((ch - yy) / ch, 0, 1)
    save_rgba(out / "cloud.png", np.stack([shade, shade, shade * 1.02], -1), a * 0.95)


# ── 146 Curtain Call ─────────────────────────────────────────────────────
# A toy theatre: one proscenium (red velvet, gilt, the boards) over four
# painted backdrops - a forest, a desert night, the sea floor, a winter
# village - a dancer in four costumes, and the texture a dissolve burns
# through (a swirl of curtain folds).

def cc146_theatre():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "curtaincall"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    rng = np.random.default_rng(146)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    # The proscenium: everything outside the stage opening, plus the boards.
    OX0, OX1, OY0, OY1 = 170, 1110, 90, 560   # the opening, in pixels
    S = 2
    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.rectangle((0, 0, W * S, H * S), fill=(18, 10, 14, 255))
    d.rectangle((OX0 * S, OY0 * S, OX1 * S, OY1 * S), fill=(0, 0, 0, 0))
    # Gilt frame round the opening.
    for k, c in enumerate(((120, 84, 24), (214, 168, 70), (250, 220, 130), (170, 120, 40))):
        e = 26 - k * 6
        d.rectangle(((OX0 - e) * S, (OY0 - e) * S, (OX1 + e) * S, (OY0 - e + 6) * S), fill=c + (255,))
        d.rectangle(((OX0 - e) * S, (OY0 - e) * S, (OX0 - e + 6) * S, (OY1 + 4) * S), fill=c + (255,))
        d.rectangle(((OX1 + e - 6) * S, (OY0 - e) * S, (OX1 + e) * S, (OY1 + 4) * S), fill=c + (255,))
    # Swagged velvet at the top and gathered curtains at the sides.
    def velvet(box, folds, gather):
        x0, y0, x1, y1 = box
        for i in range(folds):
            fx0 = x0 + (x1 - x0) * i / folds
            fx1 = x0 + (x1 - x0) * (i + 1) / folds
            for j in range(int((fx1 - fx0) * S)):
                t = j / max((fx1 - fx0) * S, 1)
                shade = 0.45 + 0.55 * math.sin(t * math.pi) ** 0.8
                c = (int(170 * shade), int(20 * shade), int(36 * shade), 255)
                x = fx0 * S + j
                d.line((x, y0 * S, x, y1 * S), fill=c)
    velvet((OX0 - 30, OY0 - 30, OX0 + 70, OY1 + 4), 4, 0)
    velvet((OX1 - 70, OY0 - 30, OX1 + 30, OY1 + 4), 4, 0)
    for i in range(7):
        x0 = OX0 - 30 + (OX1 - OX0 + 60) * i / 7
        x1 = OX0 - 30 + (OX1 - OX0 + 60) * (i + 1) / 7
        d.chord((x0 * S, (OY0 - 70) * S, x1 * S, (OY0 + 40) * S), 0, 180, fill=(150, 18, 32, 255))
        d.arc((x0 * S, (OY0 - 70) * S, x1 * S, (OY0 + 40) * S), 0, 180, fill=(214, 168, 70, 255), width=4 * S)
    # Tie-backs.
    for x in (OX0 + 40, OX1 - 40):
        d.ellipse(((x - 14) * S, (OY0 + 300) * S, (x + 14) * S, (OY0 + 318) * S), fill=(214, 168, 70, 255))
    # The boards: the stage floor in perspective strips, and the apron.
    for i in range(10):
        y0 = OY1 + i * 16
        c = 80 + (i % 2) * 12
        d.rectangle(((OX0 - 40) * S, y0 * S, (OX1 + 40) * S, (y0 + 16) * S), fill=(c, int(c * 0.62), int(c * 0.36), 255))
        d.line(((OX0 - 40) * S, y0 * S, (OX1 + 40) * S, y0 * S), fill=(50, 30, 18, 255), width=S)
    d.rectangle(((OX0 - 40) * S, (OY1 + 150) * S, (OX1 + 40) * S, (OY1 + 160) * S), fill=(214, 168, 70, 255))
    # Footlights.
    for x in range(OX0 + 20, OX1, 60):
        d.ellipse(((x - 9) * S, (OY1 + 140) * S, (x + 9) * S, (OY1 + 152) * S), fill=(255, 230, 160, 255))
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "proscenium.png", a[..., :3], a[..., 3])

    # The backdrops, each the stage opening's size and a little more.
    BW, BH = 980, 520
    by = np.arange(BH)[:, None] * np.ones((1, BW))
    bx = np.arange(BW)[None, :] * np.ones((BH, 1))
    g = fbm_rect(BW, BH, 8, 4)

    def save_bd(name, rgb):
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), np.ones((BH, BW)))

    # Forest: dusk sky, three ranks of trees, fireflies.
    t = by / BH
    img = np.stack([0.30 + 0.25 * t, 0.42 + 0.20 * t, 0.36 + 0.06 * t], -1)
    for rank, (col, scale, base) in enumerate((((0.20, 0.32, 0.24), 0.6, 300), ((0.12, 0.22, 0.14), 0.8, 380), ((0.05, 0.10, 0.06), 1.0, 470))):
        for _ in range(9 + rank * 3):
            cx = rng.uniform(-40, BW + 40)
            hgt = rng.uniform(180, 300) * scale
            w0 = hgt * 0.42
            tri = (by > base - hgt + np.abs(bx - cx) * hgt / w0 * 0.9) & (by < base) & (np.abs(bx - cx) < w0)
            jag = 0.5 + 0.5 * np.sin(by * 0.18 + cx)
            tri &= (np.abs(bx - cx) < w0 * (0.6 + 0.4 * jag) * (by - (base - hgt)) / hgt + 2)
            img = np.where(tri[..., None], np.array(col) * (0.9 + 0.2 * g[..., None]), img)
        img = np.where((by > base)[..., None], np.array(col) * 0.9, img)
    for _ in range(40):
        x, y = rng.uniform(20, BW - 20), rng.uniform(150, 470)
        r2 = (bx - x) ** 2 + (by - y) ** 2
        img = img + np.exp(-r2 / 30.0)[..., None] * np.array([0.9, 1.0, 0.4])
    save_bd("forest", img)

    # Desert night: deep blue, stars, a big moon, dunes, a cactus.
    img = np.stack([0.05 + 0.10 * t, 0.06 + 0.12 * t, 0.20 + 0.18 * t], -1)
    stars = rng.random((BH, BW)) < 0.0025
    img = np.where((stars & (by < 300))[..., None], 0.9, img)
    r = np.sqrt((bx - 720) ** 2 + (by - 130) ** 2)
    img = img + np.exp(-(r / 120) ** 2)[..., None] * np.array([0.25, 0.22, 0.12])
    img = np.where((r < 70)[..., None], np.array([0.98, 0.92, 0.72]) - 0.06 * g[..., None], img)
    for base, amp, col in ((330, 40, (0.42, 0.28, 0.20)), (390, 50, (0.55, 0.36, 0.22)), (450, 35, (0.66, 0.45, 0.26))):
        ridge = base - amp * np.sin(bx[0] / 140.0 + base) - 14 * np.sin(bx[0] / 47.0)
        m = by > ridge[None, :]
        shade = 0.85 + 0.25 * np.clip((bx - 300) / BW, -0.5, 0.5)
        img = np.where(m[..., None], np.array(col) * shade[..., None], img)
    cx, cy = 230, 400
    cact = ((np.abs(bx - cx) < 14) & (by > cy - 130) & (by < cy + 40)) | \
           ((np.abs(bx - (cx - 34)) < 10) & (by > cy - 90) & (by < cy - 30)) | ((by > cy - 40) & (by < cy - 26) & (bx > cx - 44) & (bx < cx)) | \
           ((np.abs(bx - (cx + 36)) < 10) & (by > cy - 110) & (by < cy - 50)) | ((by > cy - 60) & (by < cy - 46) & (bx > cx) & (bx < cx + 46))
    img = np.where(cact[..., None], np.array([0.10, 0.20, 0.12]), img)
    save_bd("desert", img)

    # The sea floor: teal fading down, light shafts, sand, seaweed, bubbles.
    img = np.stack([0.05 + 0.02 * (1 - t), 0.35 - 0.20 * t, 0.45 - 0.20 * t], -1)
    for _ in range(6):
        x0 = rng.uniform(0, BW)
        shaft = np.clip(1 - np.abs(bx - x0 - by * 0.35) / 40.0, 0, 1) * np.clip(1 - t * 1.3, 0, 1)
        img = img + shaft[..., None] * np.array([0.10, 0.18, 0.16])
    sand = by > 430 - 12 * np.sin(bx / 80.0)
    img = np.where(sand[..., None], np.array([0.62, 0.55, 0.38]) * (0.85 + 0.25 * g[..., None]), img)
    for _ in range(14):
        x0 = rng.uniform(20, BW - 20)
        hgt = rng.uniform(120, 260)
        weed = (np.abs(bx - x0 - 14 * np.sin(by / 26.0 + x0)) < 7) & (by > 440 - hgt) & (by < 450)
        img = np.where(weed[..., None], np.array([0.12, 0.42, 0.22]) * (0.8 + 0.3 * g[..., None]), img)
    for _ in range(30):
        x, y, rr = rng.uniform(0, BW), rng.uniform(40, 420), rng.uniform(4, 10)
        ring = np.abs(np.sqrt((bx - x) ** 2 + (by - y) ** 2) - rr) < 1.4
        img = np.where(ring[..., None], np.array([0.75, 0.92, 0.95]), img)
    save_bd("sea", img)

    # Winter village: a grey-pink sky, snowy hills, cottages with lit
    # windows, snow on the ground.
    img = np.stack([0.62 + 0.12 * t, 0.62 + 0.08 * t, 0.72 + 0.04 * t], -1)
    hill = by > 300 - 40 * np.sin(bx / 210.0) - 12 * g * 4
    img = np.where(hill[..., None], np.array([0.86, 0.88, 0.94]) - 0.05 * g[..., None], img)
    pim = _I.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8))
    pd = _D.Draw(pim)
    for k, x in enumerate(range(60, BW - 100, 170)):
        h = 110 + (k % 3) * 25
        top = 430 - h
        wall = [(150, 80, 60), (90, 110, 150), (170, 140, 90)][k % 3]
        pd.rectangle((x, top, x + 120, 430), fill=wall)
        pd.polygon([(x - 14, top + 4), (x + 60, top - 60), (x + 134, top + 4)], fill=(70, 50, 50))
        pd.polygon([(x - 14, top + 4), (x + 60, top - 60), (x + 134, top + 4), (x + 60, top - 44)], fill=(244, 246, 252))
        for wy in range(top + 26, 400, 44):
            pd.rectangle((x + 18, wy, x + 42, wy + 24), fill=(250, 210, 120))
            pd.rectangle((x + 78, wy, x + 102, wy + 24), fill=(250, 210, 120))
        pd.rectangle((x + 50, 392, x + 70, 430), fill=(70, 46, 30))
        pd.rectangle((x + 86, top - 50, x + 100, top - 14), fill=(110, 70, 60))
    pd.rectangle((0, 430, BW, BH), fill=(240, 242, 250))
    img = np.asarray(pim).astype(np.float64) / 255.0
    save_bd("winter", img)

    # The dancer: one figure, painted white, tinted per costume.
    DW, DH = 140, 220
    im = _I.new("RGBA", (DW * S, DH * S), (0, 0, 0, 0))
    d = _D.Draw(im)

    def P(x, y):
        return (x * S, y * S)

    d.ellipse((*P(56, 14), *P(84, 42)), fill=(250, 226, 200, 255))
    d.polygon([P(58, 44), P(82, 44), P(88, 100), P(52, 100)], fill=(255, 255, 255, 255))
    d.polygon([P(52, 96), P(88, 96), P(124, 150), P(16, 150)], fill=(255, 255, 255, 255))
    d.line([P(58, 52), P(30, 30), P(16, 8)], fill=(250, 226, 200, 255), width=7 * S, joint="curve")
    d.line([P(82, 52), P(108, 70), P(126, 56)], fill=(250, 226, 200, 255), width=7 * S, joint="curve")
    d.line([P(64, 148), P(60, 206)], fill=(250, 226, 200, 255), width=8 * S)
    d.line([P(76, 148), P(98, 196), P(118, 192)], fill=(250, 226, 200, 255), width=8 * S, joint="curve")
    im = im.resize((DW, DH), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "dancer.png", a[..., :3], a[..., 3])

    # The dissolve's mask: velvet folds in a swirl, darkest at the middle,
    # so the old scene burns away from the centre out along the folds.
    MW, MH = 256, 144
    my, mx = np.mgrid[0:MH, 0:MW]
    dx, dy = (mx - MW / 2) / MH, (my - MH / 2) / MH
    rad = np.sqrt(dx ** 2 + dy ** 2)
    ang = np.arctan2(dy, dx)
    folds = 0.5 + 0.5 * np.sin(ang * 7 + rad * 14)
    m = np.clip(rad * 1.3 + folds * 0.25 + fbm_rect(MW, MH, 6, 3) * 0.15, 0, 1)
    m = (m - m.min()) / (m.max() - m.min())
    # Outside sprites/: the build packs that folder into atlases, and the
    # transition reads the mask by its own id.
    masks = OUT.parent / "masks"
    masks.mkdir(parents=True, exist_ok=True)
    save_rgba(masks / "curtain_dissolve.png", np.stack([m, m, m], -1), np.ones((MH, MW)))


# ── 147 Fruit Slicer ─────────────────────────────────────────────────────
# Fruit, each as two pictures laid out alike: its skin, and its inside as a
# cut shows it - so a half drawn with the inside lines up with the whole.
# A wooden backdrop with a bamboo screen.

FS_FRUITS = ("watermelon", "orange", "apple", "kiwi", "lemon")


def fs147_fruit():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "fruitslicer"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(147)
    N = 160
    yy, xx = np.mgrid[0:N, 0:N]
    cx = cy = (N - 1) / 2
    dx, dy = (xx - cx) / (N / 2), (yy - cy) / (N / 2)
    r = np.sqrt(dx ** 2 + dy ** 2)
    ang = np.arctan2(dy, dx)
    disc = np.clip((0.97 - r) * N * 0.5, 0, 1)
    light = np.clip(1.0 - 0.45 * (dx * 0.6 + dy * 0.7) - 0.35 * r ** 2, 0.35, 1.25)[..., None]
    g = fbm_rect(N, N, 10, 3)

    def put(name, rgb, alpha=disc):
        save_rgba(out / f"{name}.png", np.clip(rgb, 0, 1), alpha)

    def lerp(a, b, t):
        return np.array(a) * (1 - t[..., None]) + np.array(b) * t[..., None]

    # Watermelon: dark green stripes outside; red flesh, a pale rind, seeds.
    stripes = 0.5 + 0.5 * np.sin(ang * 9 + np.sin(r * 9) * 0.6 + g * 3)
    put("watermelon", lerp((0.10, 0.35, 0.10), (0.30, 0.60, 0.22), stripes) * light)
    inside = lerp((0.92, 0.20, 0.25), (0.98, 0.42, 0.40), np.clip(1 - r, 0, 1))
    inside = np.where((r > 0.80)[..., None], np.array([0.85, 0.95, 0.70]), inside)
    inside = np.where((r > 0.88)[..., None], np.array([0.15, 0.45, 0.15]), inside)
    for k in range(18):
        a, rr = k / 18 * math.tau + rng.uniform(-0.1, 0.1), rng.uniform(0.35, 0.65)
        sx, sy = math.cos(a) * rr, math.sin(a) * rr
        seed = ((dx - sx) * 3.2) ** 2 + ((dy - sy) * 5.5) ** 2 < 0.02
        inside = np.where(seed[..., None], np.array([0.08, 0.05, 0.05]), inside)
    put("watermelon_in", inside * (0.92 + 0.12 * g[..., None]))

    # Orange: dimpled peel; segments and pith.
    dimple = fbm_rect(N, N, 40, 2)
    put("orange", np.array([0.98, 0.55, 0.08]) * (0.85 + 0.25 * dimple[..., None]) * light)
    seg = np.abs(np.sin(ang * 5)) ** 0.25
    inside = lerp((0.98, 0.70, 0.30), (1.0, 0.58, 0.10), seg * np.clip(r * 1.4, 0, 1))
    inside = np.where((np.abs(np.sin(ang * 5)) < 0.06)[..., None] & (r < 0.82)[..., None], np.array([1.0, 0.92, 0.80]), inside)
    inside = np.where((r < 0.08)[..., None], np.array([1.0, 0.92, 0.80]), inside)
    inside = np.where((r > 0.82)[..., None], np.array([1.0, 0.94, 0.84]), inside)
    inside = np.where((r > 0.91)[..., None], np.array([0.98, 0.55, 0.08]), inside)
    put("orange_in", inside)

    # Apple: red with a yellow blush and a stem; cream flesh, a star of pips.
    blush = np.clip(0.5 - dx * 0.6 + g * 0.5, 0, 1)
    apple = lerp((0.80, 0.08, 0.10), (0.95, 0.70, 0.20), blush * 0.45) * light
    stem = (np.abs(dx) < 0.05) & (dy < -0.7) & (dy > -1.0)
    apple = np.where(stem[..., None], np.array([0.35, 0.22, 0.10]), apple)
    put("apple", apple, np.maximum(disc, stem.astype(float)))
    inside = np.array([0.98, 0.93, 0.78]) * (0.92 + 0.1 * g[..., None]) * np.ones((N, N, 1))
    star = (r < 0.32) & (np.abs(np.sin(ang * 2.5)) > 0.7)
    inside = np.where(star[..., None], np.array([0.85, 0.78, 0.55]), inside)
    for k in range(5):
        a = k / 5 * math.tau
        pip = ((dx - math.cos(a) * 0.2) * 6) ** 2 + ((dy - math.sin(a) * 0.2) * 9) ** 2 < 0.4
        inside = np.where(pip[..., None], np.array([0.30, 0.16, 0.06]), inside)
    inside = np.where((r > 0.9)[..., None], np.array([0.80, 0.08, 0.10]), inside)
    put("apple_in", inside)

    # Kiwi: fuzzy brown; green flesh, a white heart, a ring of black seeds.
    fuzz = fbm_rect(N, N, 60, 2)
    put("kiwi", np.array([0.52, 0.36, 0.18]) * (0.8 + 0.35 * fuzz[..., None]) * light)
    inside = lerp((0.98, 0.98, 0.85), (0.45, 0.75, 0.15), np.clip((r - 0.12) * 3.0, 0, 1))
    inside = inside * (0.95 + 0.1 * np.abs(np.sin(ang * 40))[..., None] * (r > 0.2)[..., None])
    for k in range(40):
        a = k / 40 * math.tau + rng.uniform(-0.05, 0.05)
        rr = rng.uniform(0.26, 0.34)
        seedk = ((dx - math.cos(a) * rr) * 18) ** 2 + ((dy - math.sin(a) * rr) * 18) ** 2 < 0.5
        inside = np.where(seedk[..., None], np.array([0.05, 0.05, 0.03]), inside)
    inside = np.where((r > 0.9)[..., None], np.array([0.52, 0.36, 0.18]), inside)
    put("kiwi_in", inside)

    # Lemon: yellow peel; pale segments.
    put("lemon", np.array([0.98, 0.88, 0.22]) * (0.88 + 0.2 * dimple[..., None]) * light)
    inside = lerp((1.0, 0.97, 0.70), (0.98, 0.90, 0.40), np.abs(np.sin(ang * 4.5)) ** 0.3 * np.clip(r * 1.3, 0, 1))
    inside = np.where((np.abs(np.sin(ang * 4.5)) < 0.07)[..., None] & (r < 0.84)[..., None], np.array([1.0, 1.0, 0.92]), inside)
    inside = np.where((r > 0.84)[..., None], np.array([1.0, 1.0, 0.90]), inside)
    inside = np.where((r > 0.92)[..., None], np.array([0.98, 0.88, 0.22]), inside)
    put("lemon_in", inside)

    # The backdrop: warm planks and a bamboo screen at the top.
    W, H = 1280, 720
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    wg = fbm_rect(W, H, 6, 4)
    plank = (xs // 160).astype(int)
    grain = 0.5 + 0.5 * np.sin(ys * 0.05 + wg * 8 + plank * 2.1)
    wood = np.stack([0.42 + 0.10 * grain, 0.26 + 0.06 * grain, 0.14 + 0.04 * grain], -1)
    seams = (xs % 160 < 3)
    wood = np.where(seams[..., None], wood * 0.55, wood)
    vign = np.clip(1.15 - 0.6 * (((xs - W / 2) / W) ** 2 + ((ys - H / 2) / H) ** 2), 0.4, 1.1)
    img = wood * vign[..., None]
    bam = ys < 70
    bx = xs % 48
    stalk = np.stack([0.55 + 0.2 * (bx / 48), 0.62 + 0.15 * (bx / 48), 0.30 + 0.05 * (bx / 48)], -1)
    stalk = np.where(((ys % 35) < 3)[..., None], stalk * 0.6, stalk)
    stalk = np.where((bx < 2)[..., None], stalk * 0.5, stalk)
    img = np.where(bam[..., None], stalk, img)
    img = np.where(((ys >= 70) & (ys < 78))[..., None], np.array([0.15, 0.08, 0.04]), img)
    save_rgba(out / "board.png", np.clip(img, 0, 1), np.ones((H, W)))
    # A juice splash, white so its emitter tints it.
    S = 64
    sy, sx = np.mgrid[0:S, 0:S]
    sr = np.sqrt((sx - S / 2) ** 2 + (sy - S / 2) ** 2) / (S / 2)
    blob = np.clip(1 - sr ** 1.5, 0, 1)
    save_rgba(out / "drop.png", np.ones((S, S, 3)), blob)


# ── 148 Portal Lab ───────────────────────────────────────────────────────
# A test chamber seen side on: pale wall panels, a dark floor with a gap
# for the blue portal, a right wall with a gap for the orange one, portal
# rims, a metal crate, a dispenser tube and a launcher.

def pl148_chamber():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "portallab"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    rng = np.random.default_rng(148)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    g = fbm_rect(W, H, 20, 3)
    # Back wall: off-white panels in a 64 grid with soft grime.
    pan = 0.80 + 0.06 * g - 0.04 * ((xs // 64 + ys // 64) % 2)
    seams = ((xs % 64) < 2) | ((ys % 64) < 2)
    wall = np.stack([pan, pan, pan * 1.02], -1)
    wall = np.where(seams[..., None], wall * 0.72, wall)
    # A light strip along the top and a soft vignette.
    wall = wall + np.exp(-((ys - 40) / 30.0) ** 2)[..., None] * 0.12
    vig = np.clip(1.1 - 0.5 * (((xs - W / 2) / W) ** 2 * 2 + ((ys - H / 2) / H) ** 2 * 2), 0.6, 1.1)
    save_rgba(out / "backwall.png", np.clip(wall * vig[..., None], 0, 1), np.ones((H, W)))

    def slab(name, w, h, base, stripe=False):
        sy, sx = np.mgrid[0:h, 0:w]
        n = fbm_rect(w, h, max(2, w // 40), 3)
        c = np.stack([base[0] + 0.05 * n, base[1] + 0.05 * n, base[2] + 0.05 * n], -1)
        edge = np.minimum(np.minimum(sx, w - 1 - sx), np.minimum(sy, h - 1 - sy))
        c = c * (0.75 + 0.25 * np.clip(edge / 6.0, 0, 1))[..., None]
        if stripe:
            st = (((sx + sy) // 18) % 2 == 0) & (sy < 10)
            c = np.where(st[..., None], np.array([0.95, 0.75, 0.15]), c)
        save_rgba(out / f"{name}.png", np.clip(c, 0, 1), np.ones((h, w)))

    slab("floor", 256, 64, (0.22, 0.23, 0.26), stripe=True)
    slab("wallslab", 64, 256, (0.26, 0.27, 0.30))
    slab("ceiling", 256, 48, (0.30, 0.31, 0.34))

    # Portal rims: a glowing ellipse, white so each is tinted; the line runs
    # along x, the rim a thin ring.
    RW, RH = 220, 60
    ry, rx = np.mgrid[0:RH, 0:RW]
    ex, ey = (rx - RW / 2) / (RW / 2 - 4), (ry - RH / 2) / (RH / 2 - 4)
    rr = np.sqrt(ex ** 2 + ey ** 2)
    ring = np.exp(-((rr - 0.9) / 0.07) ** 2) + 0.35 * np.exp(-((rr - 0.9) / 0.25) ** 2)
    save_rgba(out / "rim.png", np.ones((RH, RW, 3)), np.clip(ring, 0, 1))

    # The crate: a metal box with corner plates and a ring in the middle.
    S = 2
    CS = 64
    im = _I.new("RGBA", (CS * S, CS * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.rounded_rectangle((2 * S, 2 * S, (CS - 2) * S, (CS - 2) * S), radius=6 * S, fill=(170, 176, 186, 255), outline=(60, 64, 74, 255), width=3 * S)
    for (x0, y0) in ((2, 2), (CS - 20, 2), (2, CS - 20), (CS - 20, CS - 20)):
        d.rounded_rectangle((x0 * S, y0 * S, (x0 + 18) * S, (y0 + 18) * S), radius=4 * S, fill=(110, 116, 128, 255))
    d.ellipse((20 * S, 20 * S, 44 * S, 44 * S), fill=(90, 96, 108, 255), outline=(220, 120, 40, 255), width=4 * S)
    im = im.resize((CS, CS), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "crate.png", a[..., :3], a[..., 3])

    # The dispenser: a glass tube coming down from the ceiling.
    TW, TH = 90, 160
    im = _I.new("RGBA", (TW * S, TH * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.rectangle((10 * S, 0, (TW - 10) * S, (TH - 30) * S), fill=(190, 225, 240, 90), outline=(220, 240, 250, 200), width=3 * S)
    d.rounded_rectangle((0, (TH - 34) * S, TW * S, TH * S), radius=8 * S, fill=(70, 74, 84, 255))
    d.rectangle((22 * S, (TH - 22) * S, (TW - 22) * S, (TH - 8) * S), fill=(30, 30, 36, 255))
    im = im.resize((TW, TH), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "dispenser.png", a[..., :3], a[..., 3])

    # The launcher: a squat plate with a hazard band, pointing right.
    LW, LH = 120, 70
    im = _I.new("RGBA", (LW * S, LH * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.polygon([(0, LH * S), (LW * S, LH * S), ((LW - 20) * S, 24 * S), (10 * S, 24 * S)], fill=(80, 84, 96, 255))
    for k in range(0, LW, 16):
        d.polygon([((k + 10) * S, 24 * S), ((k + 18) * S, 24 * S), ((k + 10) * S, 34 * S), ((k + 2) * S, 34 * S)], fill=(240, 190, 30, 255))
    d.rounded_rectangle((30 * S, 0, 90 * S, 26 * S), radius=6 * S, fill=(120, 126, 140, 255))
    im = im.resize((LW, LH), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "launcher.png", a[..., :3], a[..., 3])


# ── 149 Jelly Bounce ─────────────────────────────────────────────────────
# A pastel kitchen: a striped wall, a checked tablecloth, a long shelf, and
# a jelly block painted white so each is tinted its own colour - rounded,
# glossy, with bubbles caught inside.

def jb149_kitchen():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "jellybounce"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    g = fbm_rect(W, H, 12, 3)
    stripe = ((xs // 40) % 2 == 0)
    wall = np.where(stripe[..., None], np.array([0.98, 0.86, 0.88]), np.array([0.99, 0.93, 0.90])) * (0.96 + 0.05 * g[..., None])
    # The tablecloth across the bottom: red-and-white check.
    cloth = ys > 520
    chk = ((xs // 36 + ys // 36) % 2 == 0)
    tc = np.where(chk[..., None], np.array([0.86, 0.22, 0.28]), np.array([0.98, 0.96, 0.94]))
    img = np.where(cloth[..., None], tc * (0.9 + 0.1 * g[..., None]), wall)
    img = np.where(((ys > 512) & (ys <= 520))[..., None], np.array([0.75, 0.15, 0.2]), img)
    # A window with a pale sky, up left.
    win = (xs > 120) & (xs < 380) & (ys > 90) & (ys < 330)
    sky = np.stack([0.65 + 0.2 * (ys / 330), 0.85 + 0.1 * (ys / 330), 0.98 + 0 * ys], -1)
    img = np.where(win[..., None], sky, img)
    frame = win & ((np.abs(xs - 250) < 5) | (np.abs(ys - 210) < 5))
    edge = ((xs > 110) & (xs < 390) & (ys > 80) & (ys < 340)) & ~win
    img = np.where((frame | edge)[..., None], np.array([0.96, 0.96, 0.98]), img)
    save_rgba(out / "kitchen.png", np.clip(img, 0, 1), np.ones((H, W)))

    # The jelly: a rounded block, a soft top highlight, a darker rim, the
    # bubbles - white, so each sprite's colour tints it.
    N = 128
    yy, xx = np.mgrid[0:N, 0:N]
    u, v = (xx + 0.5) / N * 2 - 1, (yy + 0.5) / N * 2 - 1
    r = 0.22
    q = np.maximum(np.abs(u) - (1 - r - 0.04), 0) ** 2 + np.maximum(np.abs(v) - (1 - r - 0.04), 0) ** 2
    inside = np.clip((r ** 2 - q) * 400, 0, 1)
    rim = np.clip(np.maximum(np.abs(u), np.abs(v)) - 0.75, 0, 1) * 2.0
    body = 0.78 - 0.25 * rim + 0.12 * (1 - v) * 0.5
    hi = np.exp(-(((u + 0.35) / 0.35) ** 2 + ((v + 0.62) / 0.12) ** 2)) * 0.9
    shade = np.clip(body + hi, 0, 1.2)
    rgb = np.stack([shade, shade, shade], -1)
    rng = np.random.default_rng(149)
    for _ in range(7):
        bx, by, br = rng.uniform(-0.6, 0.6), rng.uniform(-0.4, 0.7), rng.uniform(0.04, 0.09)
        d = np.sqrt((u - bx) ** 2 + (v - by) ** 2)
        ring = np.exp(-((d - br) / 0.015) ** 2) * 0.6
        rgb = rgb + ring[..., None]
    alpha = inside * 0.92
    save_rgba(out / "jelly.png", np.clip(rgb, 0, 1), alpha)
    # The shelf: a long wooden board with a lip.
    SW, SH = 512, 40
    sy, sx = np.mgrid[0:SH, 0:SW]
    grain = 0.5 + 0.5 * np.sin(sx * 0.06 + fbm_rect(SW, SH, 8, 3) * 5)
    wood = np.stack([0.66 + 0.08 * grain, 0.46 + 0.05 * grain, 0.28 + 0.03 * grain], -1)
    wood = np.where((sy < 6)[..., None], wood * 1.15, wood)
    wood = np.where((sy > SH - 6)[..., None], wood * 0.7, wood)
    save_rgba(out / "shelf.png", np.clip(wood, 0, 1), np.ones((SH, SW)))


# ── 150 Beacon Run ───────────────────────────────────────────────────────
# A wide moor seen from above at dusk: heather, grass, rocks and a path; a
# stone cairn beacon (dark, and lit); a scout in a green cloak; the gold
# arrow an off-screen marker shows at the edge and the flame icon it shows
# over a beacon in view.

def br150_moor():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "beaconrun"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(150)
    W, H = 2048, 1536
    g1 = fbm_rect(W, H, 6, 5)
    g2 = fbm_rect(W, H, 24, 3)
    heather = np.clip((g1 - 0.45) * 4, 0, 1)
    grass = np.stack([0.30 + 0.08 * g2, 0.38 + 0.10 * g2, 0.20 + 0.05 * g2], -1)
    purple = np.stack([0.40 + 0.08 * g2, 0.26 + 0.06 * g2, 0.38 + 0.08 * g2], -1)
    img = grass * (1 - heather[..., None]) + purple * heather[..., None]
    # Bog pools.
    pools = np.clip((fbm_rect(W, H, 9, 3) - 0.68) * 12, 0, 1)
    img = img * (1 - pools[..., None]) + np.array([0.16, 0.22, 0.26]) * pools[..., None]
    # Rocks: scattered grey blobs with a lit side.
    im = _I.fromarray((np.clip(img, 0, 1) * 255).astype(np.uint8))
    d = _D.Draw(im)
    for _ in range(420):
        x, y, r = rng.uniform(0, W), rng.uniform(0, H), rng.uniform(3, 11)
        c = int(rng.uniform(110, 160))
        d.ellipse((x - r, y - r * 0.8, x + r, y + r * 0.8), fill=(c, c, c + 6))
        d.ellipse((x - r * 0.6, y - r * 0.7, x + r * 0.2, y - r * 0.1), fill=(c + 40, c + 40, c + 44))
    # A winding path of trodden earth.
    pts = []
    for k in range(40):
        t = k / 39
        pts.append((t * W, H * (0.5 + 0.25 * math.sin(t * 7.0) + 0.1 * math.sin(t * 17.0))))
    d.line(pts, fill=(120, 100, 70), width=26, joint="curve")
    d.line(pts, fill=(140, 118, 84), width=14, joint="curve")
    im = im.filter(_F.GaussianBlur(0.6))
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "moor.png", a[..., :3], np.ones((H, W)))

    S = 2

    def save(name, im, w, h):
        im = im.resize((w, h), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The cairn: a ring of stones round a fire bowl, from above.
    for name, lit in (("cairn", False), ("cairn_lit", True)):
        C = 96
        im = _I.new("RGBA", (C * S, C * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        for k in range(10):
            a_ = k / 10 * math.tau
            x, y = C / 2 + math.cos(a_) * 34, C / 2 + math.sin(a_) * 34
            c = int(rng.uniform(100, 140))
            d.ellipse(((x - 11) * S, (y - 9) * S, (x + 11) * S, (y + 9) * S), fill=(c, c, c + 8, 255), outline=(60, 60, 66, 255), width=2 * S)
        d.ellipse(((C / 2 - 20) * S, (C / 2 - 20) * S, (C / 2 + 20) * S, (C / 2 + 20) * S), fill=(70, 50, 34, 255))
        for k in range(6):
            a_ = k / 6 * math.tau
            d.line(((C / 2 + math.cos(a_) * 4) * S, (C / 2 + math.sin(a_) * 4) * S, (C / 2 + math.cos(a_) * 17) * S, (C / 2 + math.sin(a_) * 17) * S),
                   fill=(120, 80, 40, 255), width=4 * S)
        if lit:
            for r, c in ((16, (255, 120, 30, 255)), (11, (255, 190, 60, 255)), (6, (255, 245, 180, 255))):
                d.ellipse(((C / 2 - r) * S, (C / 2 - r) * S, (C / 2 + r) * S, (C / 2 + r) * S), fill=c)
        save(name + ".png", im, C, C)

    # The scout, from above: a green cloak, a hood, a walking stick.
    C = 64
    for k in range(2):
        im = _I.new("RGBA", (C * S, C * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        step = 6 if k == 0 else -6
        d.ellipse(((22 + step) * S, 4 * S, (30 + step) * S, 16 * S), fill=(60, 44, 30, 255))
        d.ellipse(((34 - step) * S, 48 * S, (42 - step) * S, 60 * S), fill=(60, 44, 30, 255))
        d.ellipse((14 * S, 14 * S, 50 * S, 50 * S), fill=(48, 96, 60, 255), outline=(30, 60, 36, 255), width=2 * S)
        d.ellipse((24 * S, 22 * S, 42 * S, 42 * S), fill=(38, 80, 48, 255))
        d.line((44 * S, 18 * S, 58 * S, 6 * S), fill=(120, 90, 50, 255), width=3 * S)
        save(f"scout{k}.png", im, C, C)

    # The marker's arrow (pointing +x) and its flame icon.
    A = 64
    im = _I.new("RGBA", (A * S, A * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.polygon([(8 * S, 10 * S), (58 * S, 32 * S), (8 * S, 54 * S), (20 * S, 32 * S)], fill=(255, 205, 70, 255), outline=(120, 70, 10, 255))
    save("arrow.png", im, A, A)
    im = _I.new("RGBA", (A * S, A * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.ellipse((6 * S, 6 * S, 58 * S, 58 * S), fill=(30, 26, 22, 200), outline=(255, 205, 70, 255), width=3 * S)
    d.polygon([(32 * S, 12 * S), (44 * S, 34 * S), (40 * S, 50 * S), (24 * S, 50 * S), (20 * S, 34 * S)], fill=(255, 140, 40, 255))
    d.polygon([(32 * S, 26 * S), (38 * S, 40 * S), (32 * S, 48 * S), (26 * S, 40 * S)], fill=(255, 230, 140, 255))
    save("flame_icon.png", im, A, A)


# ── 151 Sketchbook ───────────────────────────────────────────────────────
# A village drawn in pencil on cream paper: the paper (its own sprite, so
# it can be told from the drawing), the village in graphite lines and
# hatching, a walking figure in two poses and a bird.

def sk151_sketch():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "sketchbook"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(151)
    W, H = 1280, 720
    # Paper: warm cream, fibres and a soft vignette.
    g = fbm_rect(W, H, 60, 3)
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    vig = np.clip(1.05 - 0.35 * (((xs - W / 2) / W) ** 2 + ((ys - H / 2) / H) ** 2) * 2, 0.75, 1.05)
    paper = np.stack([0.96 + 0.03 * g, 0.93 + 0.03 * g, 0.84 + 0.03 * g], -1) * vig[..., None]
    save_rgba(out / "paper.png", np.clip(paper, 0, 1), np.ones((H, W)))

    S = 2
    GRAPHITE = (52, 50, 58)

    def wobbly(d, pts, width=2, passes=2, jitter=1.6, colour=GRAPHITE, alpha=230):
        # A line drawn twice, a little off each time, as a pencil goes over.
        for _ in range(passes):
            q = [((x + rng.normal(0, jitter)) * S, (y + rng.normal(0, jitter)) * S) for x, y in pts]
            d.line(q, fill=colour + (alpha,), width=width * S, joint="curve")

    def stroke(d, a, b, **kw):
        n = max(2, int(math.dist(a, b) / 18))
        pts = [(a[0] + (b[0] - a[0]) * k / n, a[1] + (b[1] - a[1]) * k / n) for k in range(n + 1)]
        wobbly(d, pts, **kw)

    def hatch(d, box, spacing=7, angle=1.0):
        x0, y0, x1, y1 = box
        for k in range(int((x1 - x0 + y1 - y0) / spacing)):
            c = x0 + k * spacing
            a = (c, y0)
            b = (c - (y1 - y0) / angle, y1)
            # Clip to the box roughly.
            if b[0] < x0:
                b = (x0, y0 + (c - x0) * angle)
            if a[0] > x1:
                a = (x1, y0 + (c - x1) * angle)
            stroke(d, a, b, width=1, passes=1, jitter=0.6, alpha=150)

    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    # The ground line and a path winding up to the village.
    wobbly(d, [(0, 560), (300, 548), (640, 556), (980, 545), (1280, 552)], width=2)
    wobbly(d, [(560, 720), (600, 640), (680, 600), (720, 560)], width=2)
    wobbly(d, [(760, 720), (720, 640), (740, 600), (760, 560)], width=2)
    # Houses.
    for (x, w, h, roof) in ((120, 150, 130, 70), (330, 120, 170, 60), (820, 170, 120, 80), (1040, 130, 150, 65)):
        base = 552
        top = base - h
        for a, b in (((x, base), (x, top)), ((x + w, base), (x + w, top)), ((x, top), (x + w, top))):
            stroke(d, a, b)
        stroke(d, (x - 12, top + 4), (x + w / 2, top - roof))
        stroke(d, (x + w / 2, top - roof), (x + w + 12, top + 4))
        hatch(d, (x + w * 0.3, top - roof * 0.4, x + w * 0.7, top - 4), spacing=8, angle=0.8)
        # Windows and a door.
        for wy in range(int(top + 22), int(base - 50), 46):
            for wx in range(int(x + 18), int(x + w - 30), 50):
                for a, b in (((wx, wy), (wx + 24, wy)), ((wx + 24, wy), (wx + 24, wy + 28)), ((wx + 24, wy + 28), (wx, wy + 28)), ((wx, wy + 28), (wx, wy))):
                    stroke(d, a, b, width=1, passes=1)
                stroke(d, (wx + 12, wy), (wx + 12, wy + 28), width=1, passes=1)
        dx = x + w / 2 - 14
        for a, b in (((dx, base), (dx, base - 44)), ((dx, base - 44), (dx + 28, base - 44)), ((dx + 28, base - 44), (dx + 28, base))):
            stroke(d, a, b)
        hatch(d, (dx + 2, base - 42, dx + 26, base - 2), spacing=5, angle=1.6)
    # Trees: scribbled crowns on trunks.
    for x in (560, 640, 990, 1220):
        stroke(d, (x, 552), (x, 470))
        stroke(d, (x + 8, 552), (x + 6, 476))
        pts = []
        for k in range(44):
            a = k / 44 * math.tau * 3
            r = 38 + 10 * math.sin(k * 1.7)
            pts.append((x + 4 + math.cos(a) * r, 430 + math.sin(a) * r * 0.85))
        wobbly(d, pts, width=1, passes=1, jitter=1.2)
    # A sun, and hills behind.
    pts = [(1080 + math.cos(k / 30 * math.tau) * 48, 120 + math.sin(k / 30 * math.tau) * 48) for k in range(31)]
    wobbly(d, pts, width=2)
    for k in range(12):
        a = k / 12 * math.tau
        stroke(d, (1080 + math.cos(a) * 62, 120 + math.sin(a) * 62), (1080 + math.cos(a) * 84, 120 + math.sin(a) * 84), width=1, passes=1)
    wobbly(d, [(0, 420), (200, 360), (420, 400), (600, 330), (820, 380), (1000, 320), (1280, 380)], width=1, passes=1, alpha=170)
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "village.png", a[..., :3], a[..., 3])

    # The walker, two poses: a stick figure with a hat and a satchel.
    for k in range(2):
        FW, FH = 70, 110
        im = _I.new("RGBA", (FW * S, FH * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        sw = 14 if k == 0 else -14
        pts = [(35 + math.cos(t / 16 * math.tau) * 9, 22 + math.sin(t / 16 * math.tau) * 9) for t in range(17)]
        wobbly(d, pts, width=2, passes=1, jitter=0.4)
        stroke(d, (22, 13), (48, 13), width=2, passes=1, jitter=0.3)
        stroke(d, (28, 4), (42, 4), width=2, passes=1, jitter=0.3)
        stroke(d, (35, 31), (35, 70), width=2, passes=1, jitter=0.4)
        stroke(d, (35, 70), (35 + sw, 104), width=2, passes=1, jitter=0.4)
        stroke(d, (35, 70), (35 - sw, 104), width=2, passes=1, jitter=0.4)
        stroke(d, (35, 42), (35 - sw * 0.8, 62), width=2, passes=1, jitter=0.4)
        stroke(d, (35, 42), (35 + sw * 0.8, 62), width=2, passes=1, jitter=0.4)
        d.rectangle((40 * S, 50 * S, 52 * S, 62 * S), outline=GRAPHITE + (220,), width=2 * S)
        im = im.resize((FW, FH), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"walker{k}.png", a[..., :3], a[..., 3])
    # A bird: a flying V.
    for k in range(2):
        BW, BH = 48, 24
        im = _I.new("RGBA", (BW * S, BH * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        tip = 4 if k == 0 else 18
        wobbly(d, [(2, tip), (14, 10), (24, 16)], width=2, passes=1, jitter=0.3)
        wobbly(d, [(24, 16), (34, 10), (46, tip)], width=2, passes=1, jitter=0.3)
        im = im.resize((BW, BH), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"bird{k}.png", a[..., :3], a[..., 3])


# ── 152 Spell Wheel ──────────────────────────────────────────────────────
# A wizard's tower room at night, the wizard, six spell badges (fire, ice,
# wind, earth, lightning, healing) and the wheel they sit on.

SW_SPELLS = (("fire", (230, 80, 30)), ("ice", (90, 180, 240)), ("wind", (150, 220, 170)),
             ("earth", (170, 120, 60)), ("lightning", (250, 220, 60)), ("heal", (240, 120, 190)))


def sw152_wheel():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "spellwheel"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(152)
    W, H = 1280, 720
    ys = np.arange(H)[:, None] * np.ones((1, W))
    xs = np.arange(W)[None, :] * np.ones((H, 1))
    g = fbm_rect(W, H, 10, 4)
    # Stone walls: blocks in courses, a window with the moon, a rug.
    course = (ys // 48).astype(int)
    block = ((xs + course * 37) // 96).astype(int)
    shade = 0.20 + 0.05 * ((block * 7 + course * 3) % 5) / 4 + 0.05 * g
    mortar = ((ys % 48) < 3) | (((xs + course * 37) % 96) < 3)
    img = np.stack([shade * 0.9, shade * 0.85, shade * 1.05], -1)
    img = np.where(mortar[..., None], img * 0.55, img)
    # A warm light from the hearth (bottom left) and a cool one from the window.
    warm = np.exp(-(((xs - 180) / 420) ** 2 + ((ys - 600) / 300) ** 2))
    img = img + warm[..., None] * np.array([0.35, 0.18, 0.06])
    win = (np.abs(xs - 980) < 90) & (ys > 90) & (ys < 330)
    arch = (np.sqrt((xs - 980) ** 2 + (ys - 180) ** 2) < 90) & (ys <= 180)
    window = win & (ys >= 180) | arch
    sky = np.stack([0.08 + 0.1 * (ys / 330), 0.10 + 0.12 * (ys / 330), 0.25 + 0.15 * (ys / 330)], -1)
    moon = np.sqrt((xs - 1010) ** 2 + (ys - 170) ** 2) < 32
    sky = np.where(moon[..., None], np.array([0.95, 0.93, 0.85]), sky)
    img = np.where(window[..., None], sky, img)
    frame = (win | arch) & ~((np.abs(xs - 980) < 84) & (ys > 96) & (ys < 324) | (np.sqrt((xs - 980) ** 2 + (ys - 180) ** 2) < 84) & (ys <= 180))
    img = np.where(frame[..., None], np.array([0.30, 0.22, 0.16]), img)
    floor = ys > 560
    img = np.where(floor[..., None], np.array([0.24, 0.16, 0.10]) * (0.85 + 0.25 * g[..., None]), img)
    rug = floor & (np.abs(xs - 640) < 300) & (ys > 600) & (ys < 700)
    rugc = np.where((((xs // 20) + (ys // 20)) % 2 == 0)[..., None], np.array([0.50, 0.12, 0.16]), np.array([0.62, 0.42, 0.12]))
    img = np.where(rug[..., None], rugc, img)
    save_rgba(out / "tower.png", np.clip(img, 0, 1), np.ones((H, W)))

    S = 2
    # The wizard: a robe, a hat with stars, a staff.
    WW, WH = 140, 220
    im = _I.new("RGBA", (WW * S, WH * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.polygon([(40 * S, 215 * S), (100 * S, 215 * S), (86 * S, 90 * S), (54 * S, 90 * S)], fill=(60, 50, 140, 255))
    d.polygon([(46 * S, 96 * S), (94 * S, 96 * S), (70 * S, 12 * S)], fill=(50, 40, 120, 255))
    d.ellipse((34 * S, 88 * S, 106 * S, 104 * S), fill=(50, 40, 120, 255))
    for (sx, sy) in ((66, 50), (76, 72), (60, 80)):
        d.regular_polygon(((sx * S, sy * S), 4 * S), 5, fill=(250, 220, 90, 255))
    d.ellipse((58 * S, 100 * S, 82 * S, 124 * S), fill=(240, 205, 175, 255))
    d.polygon([(58 * S, 116 * S), (82 * S, 116 * S), (70 * S, 170 * S)], fill=(235, 235, 240, 255))
    d.line((112 * S, 70 * S, 104 * S, 215 * S), fill=(120, 80, 40, 255), width=5 * S)
    d.ellipse((104 * S, 56 * S, 122 * S, 74 * S), fill=(140, 230, 255, 255))
    im = im.resize((WW, WH), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "wizard.png", a[..., :3], a[..., 3])

    # Spell badges: a round gem with a symbol.
    B = 96
    for name, col in SW_SPELLS:
        im = _I.new("RGBA", (B * S, B * S), (0, 0, 0, 0))
        d = _D.Draw(im)
        d.ellipse((4 * S, 4 * S, (B - 4) * S, (B - 4) * S), fill=(30, 26, 40, 235), outline=col + (255,), width=5 * S)
        c = B / 2
        if name == "fire":
            d.polygon([(c * S, 18 * S), (66 * S, 52 * S), (60 * S, 74 * S), (36 * S, 74 * S), (30 * S, 52 * S)], fill=col + (255,))
            d.polygon([(c * S, 44 * S), (56 * S, 62 * S), (c * S, 72 * S), (40 * S, 62 * S)], fill=(255, 220, 120, 255))
        elif name == "ice":
            for k in range(3):
                a_ = k * math.pi / 3
                d.line(((c - math.cos(a_) * 28) * S, (c - math.sin(a_) * 28) * S, (c + math.cos(a_) * 28) * S, (c + math.sin(a_) * 28) * S), fill=col + (255,), width=5 * S)
        elif name == "wind":
            for k, r in enumerate((26, 18, 10)):
                d.arc(((c - r) * S, (c - r - k * 2) * S, (c + r) * S, (c + r - k * 2) * S), 200, 520, fill=col + (255,), width=4 * S)
        elif name == "earth":
            d.polygon([(20 * S, 70 * S), (40 * S, 34 * S), (54 * S, 56 * S), (64 * S, 40 * S), (78 * S, 70 * S)], fill=col + (255,))
        elif name == "lightning":
            d.polygon([(56 * S, 14 * S), (30 * S, 52 * S), (48 * S, 52 * S), (38 * S, 82 * S), (68 * S, 42 * S), (50 * S, 42 * S)], fill=col + (255,))
        else:
            d.rectangle((40 * S, 22 * S, 56 * S, 74 * S), fill=col + (255,))
            d.rectangle((22 * S, 40 * S, 74 * S, 56 * S), fill=col + (255,))
        im = im.resize((B, B), _I.LANCZOS)
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", a[..., :3], a[..., 3])

    # The wheel: a dark disc with a ring and six spokes.
    R = 360
    im = _I.new("RGBA", (R * S, R * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    d.ellipse((6 * S, 6 * S, (R - 6) * S, (R - 6) * S), fill=(20, 16, 30, 170), outline=(200, 170, 90, 220), width=4 * S)
    d.ellipse((140 * S, 140 * S, 220 * S, 220 * S), fill=(30, 24, 44, 220), outline=(200, 170, 90, 200), width=3 * S)
    for k in range(6):
        a_ = math.radians(90 + 30 + k * 60)
        d.line(((R / 2 + math.cos(a_) * 44) * S, (R / 2 - math.sin(a_) * 44) * S, (R / 2 + math.cos(a_) * 170) * S, (R / 2 - math.sin(a_) * 170) * S),
               fill=(200, 170, 90, 120), width=2 * S)
    im = im.resize((R, R), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "wheel.png", a[..., :3], a[..., 3])


# ── 153 Rainy Window ─────────────────────────────────────────────────────
# A city at night seen through glass, a little out of focus as the street
# beyond a wet window is: a deep blue sky, towers with lit windows, a lit
# street; and the neon signs, a tram and car lights drawn as their own
# sprites so they glow and move.

def rw153_city():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "rainywindow"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(153)
    W, H = 1280, 720
    t = np.arange(H)[:, None] / H * np.ones((1, W))
    sky = np.stack([0.05 + 0.10 * t, 0.06 + 0.10 * t, 0.16 + 0.12 * t], -1)
    im = _I.fromarray((np.clip(sky, 0, 1) * 255).astype(np.uint8)).convert("RGBA")
    d = _D.Draw(im)
    # Two ranks of towers, the far one hazier.
    for rank, (base, lo, hi, col, lit) in enumerate(((470, 160, 380, (28, 30, 52), 0.25), (560, 120, 300, (16, 16, 30), 0.35))):
        x = -20
        while x < W:
            w = int(rng.integers(60, 150))
            h = int(rng.integers(lo, hi))
            top = base - h
            d.rectangle((x, top, x + w, H), fill=col + (255,))
            for wy in range(top + 12, base - 10, 16):
                for wx in range(x + 8, x + w - 8, 13):
                    if rng.random() < lit:
                        warm = rng.random() < 0.7
                        c = (255, 200, 120) if warm else (170, 210, 255)
                        d.rectangle((wx, wy, wx + 6, wy + 8), fill=c + (255,))
            x += w + int(rng.integers(2, 20))
    # The street: wet asphalt catching the light.
    d.rectangle((0, 560, W, H), fill=(18, 18, 26, 255))
    for k in range(0, W, 90):
        d.rectangle((k, 640, k + 46, 646), fill=(160, 150, 120, 255))
    # Street lamps.
    for x in range(80, W, 240):
        d.rectangle((x, 470, x + 5, 560), fill=(40, 40, 50, 255))
        d.ellipse((x - 8, 462, x + 14, 476), fill=(255, 220, 160, 255))
    # Out of focus, as the street beyond wet glass is.
    im = im.filter(_F.GaussianBlur(2.6))
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "city.png", a[..., :3], np.ones((H, W)))

    # Neon signs: words in a glowing tube, white so each is tinted.
    try:
        from PIL import ImageFont as _Fo
        font = _Fo.truetype("arialbd.ttf", 54)
    except Exception:
        font = None
    for name, word in (("neon_hotel", "HOTEL"), ("neon_noodles", "NOODLES"), ("neon_open", "OPEN")):
        tw = 60 + 40 * len(word)
        sign = _I.new("RGBA", (tw, 90), (0, 0, 0, 0))
        sd = _D.Draw(sign)
        sd.rounded_rectangle((4, 4, tw - 4, 86), radius=14, outline=(255, 255, 255, 200), width=4)
        sd.text((tw / 2, 46), word, font=font, fill=(255, 255, 255, 255), anchor="mm")
        sign = sign.filter(_F.GaussianBlur(1.6))
        glow = sign.filter(_F.GaussianBlur(6))
        out_im = _I.alpha_composite(glow, sign)
        a = np.asarray(out_im).astype(np.float64) / 255.0
        save_rgba(out / f"{name}.png", a[..., :3], a[..., 3])
    # The tram: a lit box of windows on the street.
    TW, TH = 360, 110
    tram = _I.new("RGBA", (TW, TH), (0, 0, 0, 0))
    td = _D.Draw(tram)
    td.rounded_rectangle((0, 10, TW, TH - 6), radius=16, fill=(60, 30, 30, 255))
    for k in range(7):
        x = 18 + k * 48
        td.rectangle((x, 26, x + 36, 64), fill=(255, 215, 150, 255))
    td.rectangle((0, TH - 24, TW, TH - 16), fill=(200, 160, 60, 255))
    tram = tram.filter(_F.GaussianBlur(1.6))
    a = np.asarray(tram).astype(np.float64) / 255.0
    save_rgba(out / "tram.png", a[..., :3], a[..., 3])


# ── 154 Talk of the Town ─────────────────────────────────────────────────
# A market square side on: a row of stalls with striped awnings, a cobbled
# street, townsfolk (one shape, tinted), and the speech bubble's pieces - a
# rounded white card with an ink edge (nine-sliced) and its tail.

def tt154_market():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "talkofthetown"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(154)
    W, H = 1280, 720
    S = 2
    im = _I.new("RGBA", (W * S, H * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    # Sky and the houses behind the market.
    for y in range(H):
        t = y / H
        d.line((0, y * S, W * S, y * S), fill=(int(150 + 60 * t), int(200 + 30 * t), int(240 - 10 * t), 255), width=S)
    x = 0
    while x < W:
        w = int(rng.integers(110, 170))
        h = int(rng.integers(160, 260))
        top = 430 - h
        wall = [(232, 214, 180), (210, 180, 150), (190, 205, 215), (225, 195, 160)][int(rng.integers(4))]
        d.rectangle((x * S, top * S, (x + w) * S, 430 * S), fill=wall + (255,))
        d.polygon([((x - 8) * S, top * S), ((x + w / 2) * S, (top - 50) * S), ((x + w + 8) * S, top * S)], fill=(150, 80, 60, 255))
        for wy in range(top + 25, 400, 55):
            for wx in range(x + 20, x + w - 30, 45):
                d.rectangle((wx * S, wy * S, (wx + 22) * S, (wy + 30) * S), fill=(80, 100, 130, 255), outline=(250, 250, 250, 255), width=2 * S)
        x += w + 4
    # Cobbles.
    d.rectangle((0, 430 * S, W * S, H * S), fill=(150, 140, 125, 255))
    for row, y in enumerate(range(436, H, 18)):
        for cx in range(-(row % 2) * 14, W, 28):
            c = int(rng.uniform(125, 165))
            d.rounded_rectangle((cx * S, y * S, (cx + 24) * S, (y + 14) * S), radius=5 * S, fill=(c, c - 8, c - 18, 255))
    # Stalls: counters with striped awnings.
    for i, x in enumerate(range(60, W - 150, 245)):
        col = [(200, 50, 60), (50, 120, 190), (60, 150, 80), (220, 160, 40), (150, 70, 160)][i % 5]
        d.rectangle(((x + 10) * S, 440 * S, (x + 16) * S, 560 * S), fill=(110, 75, 45, 255))
        d.rectangle(((x + 184) * S, 440 * S, (x + 190) * S, 560 * S), fill=(110, 75, 45, 255))
        for k in range(8):
            c = col if k % 2 == 0 else (245, 240, 230)
            d.polygon([((x + k * 25) * S, 430 * S), ((x + (k + 1) * 25) * S, 430 * S), ((x + (k + 1) * 25) * S, 470 * S), ((x + k * 25) * S, 470 * S)], fill=c + (255,))
            d.chord(((x + k * 25) * S, 458 * S, (x + (k + 1) * 25) * S, 482 * S), 0, 180, fill=c + (255,))
        d.rectangle(((x + 4) * S, 545 * S, (x + 196) * S, 600 * S), fill=(140, 95, 55, 255), outline=(90, 60, 35, 255), width=3 * S)
        for k in range(7):
            gx = x + 18 + k * 25
            gc = [(230, 70, 60), (240, 180, 60), (120, 180, 70), (200, 140, 90)][(k + i) % 4]
            d.ellipse((gx * S, 530 * S, (gx + 20) * S, 550 * S), fill=gc + (255,))
    im = im.resize((W, H), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "market.png", a[..., :3], np.ones((H, W)))

    # A townsperson: a round head, a body, white so each is tinted.
    PW, PH = 60, 120
    p = _I.new("RGBA", (PW * S, PH * S), (0, 0, 0, 0))
    d = _D.Draw(p)
    d.rounded_rectangle((12 * S, 40 * S, 48 * S, 112 * S), radius=14 * S, fill=(255, 255, 255, 255))
    d.ellipse((16 * S, 6 * S, 44 * S, 34 * S), fill=(255, 255, 255, 255))
    d.ellipse((22 * S, 10 * S, 38 * S, 30 * S), fill=(235, 225, 215, 255))
    d.rectangle((14 * S, 108 * S, 26 * S, 120 * S), fill=(200, 200, 200, 255))
    d.rectangle((34 * S, 108 * S, 46 * S, 120 * S), fill=(200, 200, 200, 255))
    p = p.resize((PW, PH), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "person.png", a[..., :3], a[..., 3])

    # The bubble: a rounded white card with an ink edge, 64 x 64, its
    # corners within 18 pixels (the nine-slice border).
    B = 64
    b = _I.new("RGBA", (B * S, B * S), (0, 0, 0, 0))
    _D.Draw(b).rounded_rectangle((2 * S, 2 * S, (B - 2) * S, (B - 2) * S), radius=16 * S, fill=(255, 255, 255, 255), outline=(40, 36, 50, 255), width=3 * S)
    b = b.resize((B, B), _I.LANCZOS)
    a = np.asarray(b).astype(np.float64) / 255.0
    save_rgba(out / "bubble.png", a[..., :3], a[..., 3])
    # The tail: a wedge pointing down, its top flush with the card.
    T = 32
    t = _I.new("RGBA", (T * S, T * S), (0, 0, 0, 0))
    td = _D.Draw(t)
    td.polygon([(2 * S, 0), ((T - 2) * S, 0), (T * S * 0.5, (T - 1) * S)], fill=(255, 255, 255, 255))
    td.line([(2 * S, 0), (T * S * 0.5, (T - 1) * S), ((T - 2) * S, 0)], fill=(40, 36, 50, 255), width=3 * S)
    t = t.resize((T, T), _I.LANCZOS)
    a = np.asarray(t).astype(np.float64) / 255.0
    save_rgba(out / "tail.png", a[..., :3], a[..., 3])


# ── 155 Unit Select ──────────────────────────────────────────────────────
# A meadow seen from above, a dirt track across it, and soldiers in two
# kinds (a spearman, an archer) drawn white so each is tinted - each with a
# silhouette of its own (shield, spear, bow), which is what the selection
# ring follows.

def us155_field():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "unitselect"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(155)
    W, H = 1280, 720
    # Grass: two octaves of value noise over a green, a track of dirt.
    def noise(scale, seed):
        r = np.random.default_rng(seed)
        g = r.random((H // scale + 2, W // scale + 2))
        im = _I.fromarray((g * 255).astype(np.uint8)).resize((W + 2 * scale, H + 2 * scale), _I.BICUBIC)
        return np.asarray(im).astype(np.float64)[:H, :W] / 255.0
    n = 0.6 * noise(48, 1) + 0.3 * noise(12, 2) + 0.1 * noise(3, 3)
    base = np.array([0.30, 0.48, 0.20])
    rgb = base[None, None, :] * (0.8 + 0.45 * n[..., None])
    ys, xs = np.mgrid[0:H, 0:W]
    track = 360 + 90 * np.sin(xs / 210.0) + 30 * np.sin(xs / 61.0)
    d = np.abs(ys - track)
    dirt = np.clip(1.0 - (d - 26) / 10.0, 0, 1)
    dirt_col = np.array([0.55, 0.43, 0.28])[None, None, :] * (0.85 + 0.3 * noise(6, 4)[..., None])
    rgb = rgb * (1 - dirt[..., None]) + dirt_col * dirt[..., None]
    im = _I.fromarray((np.clip(rgb, 0, 1) * 255).astype(np.uint8), "RGB").convert("RGBA")
    dr = _D.Draw(im)
    # Flowers, tufts and stones.
    for _ in range(900):
        x, y = rng.integers(0, W), rng.integers(0, H)
        if abs(y - track[0, x]) < 34:
            continue
        k = rng.random()
        if k < 0.7:
            c = int(rng.integers(30, 70))
            dr.line((x, y, x + int(rng.integers(-3, 4)), y - int(rng.integers(3, 8))), fill=(c, 100 + c, 30, 255), width=1)
        elif k < 0.9:
            col = [(240, 230, 120), (240, 240, 245), (200, 120, 200)][int(rng.integers(3))]
            dr.ellipse((x - 2, y - 2, x + 2, y + 2), fill=col + (255,))
        else:
            g = int(rng.integers(110, 150))
            dr.ellipse((x - 5, y - 4, x + 5, y + 4), fill=(g, g, g - 6, 255), outline=(g - 40, g - 40, g - 45, 255))
    # Bushes round the edge, soft shadows under them.
    for _ in range(26):
        x = int(rng.choice([rng.integers(0, 140), rng.integers(W - 140, W)]))
        y = int(rng.integers(0, H))
        r = int(rng.integers(18, 34))
        sh = _I.new("RGBA", (W, H), (0, 0, 0, 0))
        _D.Draw(sh).ellipse((x - r + 6, y - r + 8, x + r + 6, y + r + 8), fill=(0, 0, 0, 90))
        im = _I.alpha_composite(im, sh.filter(_F.GaussianBlur(6)))
        dr = _D.Draw(im)
        for k in range(5):
            ox, oy = rng.integers(-r // 2, r // 2 + 1, 2)
            g = int(rng.integers(60, 100))
            dr.ellipse((x + ox - r * 0.7, y + oy - r * 0.7, x + ox + r * 0.7, y + oy + r * 0.7), fill=(30, g, 25, 255))
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "field.png", a[..., :3], np.ones((H, W)))

    # Soldiers, from above, facing +x (right), 2x supersampled.
    S = 4
    U = 64

    def soldier(kind):
        p = _I.new("RGBA", (U * S, U * S), (0, 0, 0, 0))
        d = _D.Draw(p)
        c = U * S // 2
        def e(x0, y0, x1, y1, fill):
            d.ellipse((c + x0 * S, c + y0 * S, c + x1 * S, c + y1 * S), fill=fill)
        # Shoulders, seen from above: wide across, thin front to back.
        e(-8, -17, 8, 17, (225, 225, 225, 255))
        # Arms reaching forward from each shoulder.
        e(0, -17, 12, -9, (240, 240, 240, 255))
        e(0, 9, 12, 17, (240, 240, 240, 255))
        if kind == "spear":
            # A spear held in the right hand, its head out in front; the
            # shield on the left arm, a round shield seen edge-on.
            d.line((c - 14 * S, c + 13 * S, c + 28 * S, c + 13 * S), fill=(200, 180, 150, 255), width=3 * S)
            d.polygon([(c + 27 * S, c + 9 * S), (c + 31 * S, c + 13 * S), (c + 27 * S, c + 17 * S), (c + 23 * S, c + 13 * S)],
                      fill=(225, 230, 240, 255))
            e(8, -26, 15, -5, (170, 170, 175, 255))
            e(9, -24, 14, -7, (255, 255, 255, 255))
        else:
            # A bow held out in front, its string back to the hand.
            d.arc((c - 2 * S, c - 20 * S, c + 18 * S, c + 20 * S), -75, 75, fill=(205, 175, 130, 255), width=3 * S)
            d.line((c + 10 * S, c - 19 * S, c + 4 * S, c + 13 * S, c + 10 * S, c + 19 * S), fill=(240, 240, 240, 255), width=S)
            # A quiver across the back, arrows' fletching showing.
            d.rounded_rectangle((c - 15 * S, c - 9 * S, c - 9 * S, c + 7 * S), radius=2 * S, fill=(185, 185, 185, 255))
            for k in range(3):
                d.line((c - 17 * S, c + (-6 + 4 * k) * S, c - 13 * S, c + (-6 + 4 * k) * S), fill=(255, 255, 255, 255), width=S)
        # Helmet, its crest running front to back.
        e(-9, -9, 9, 9, (250, 250, 250, 255))
        d.line((c - 9 * S, c, c + 8 * S, c), fill=(190, 190, 190, 255), width=2 * S)
        p = p.resize((U, U), _I.LANCZOS)
        a = np.asarray(p).astype(np.float64) / 255.0
        save_rgba(out / f"{kind}.png", a[..., :3], a[..., 3])

    soldier("spear")
    soldier("archer")
    # The soldier's soft shadow on the grass.
    sh = _I.new("RGBA", (U * S, U * S), (0, 0, 0, 0))
    _D.Draw(sh).ellipse((U * S * 0.25, U * S * 0.25, U * S * 0.75, U * S * 0.75), fill=(0, 0, 0, 255))
    sh = sh.filter(_F.GaussianBlur(S * 4)).resize((U, U), _I.LANCZOS)
    a = np.asarray(sh).astype(np.float64) / 255.0
    save_rgba(out / "shadow.png", a[..., :3] * 0, a[..., 3] * 0.55)
    # A flag marking where the selection was sent.
    fl = _I.new("RGBA", (32 * S, 32 * S), (0, 0, 0, 0))
    fd = _D.Draw(fl)
    fd.ellipse((10 * S, 22 * S, 22 * S, 30 * S), outline=(255, 255, 255, 255), width=S * 2)
    fd.line((16 * S, 26 * S, 16 * S, 2 * S), fill=(255, 255, 255, 255), width=S * 2)
    fd.polygon([(16 * S, 2 * S), (29 * S, 7 * S), (16 * S, 12 * S)], fill=(255, 255, 255, 255))
    fl = fl.resize((32, 32), _I.LANCZOS)
    a = np.asarray(fl).astype(np.float64) / 255.0
    save_rgba(out / "flag.png", a[..., :3], a[..., 3])


# ── 156 Thunderhead ──────────────────────────────────────────────────────
# A storm at sea at night: a sky of heavy cloud (lit, so a flash shows its
# billows), a bank of cloud base the bolts leave from, a dark rolling sea,
# and a lighthouse on its rock with a lightning rod on the cap. Painted
# fairly bright: the scene's ambient light is low, and the lightning's flash
# is what shows how much is there.

def th156_storm():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "thunderhead"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(156)
    W, H = 1280, 720

    def noise(w, h, scale, seed):
        r = np.random.default_rng(seed)
        g = r.random((h // scale + 3, w // scale + 3))
        im = _I.fromarray((g * 255).astype(np.uint8)).resize((w + 3 * scale, h + 3 * scale), _I.BICUBIC)
        return np.asarray(im).astype(np.float64)[:h, :w] / 255.0

    def fbm(w, h, seed, scales=(160, 80, 40, 20, 10)):
        acc, amp, tot = np.zeros((h, w)), 1.0, 0.0
        for i, s in enumerate(scales):
            acc += amp * noise(w, h, s, seed + i)
            tot += amp
            amp *= 0.55
        return acc / tot

    # The sky: slate cloud, lighter towards the horizon, billows of light
    # and dark worked in by noise.
    ys = np.linspace(0, 1, H)[:, None] * np.ones((1, W))
    n = fbm(W, H, 11)
    billow = np.clip((n - 0.42) * 2.8, 0, 1)
    top = np.array([0.30, 0.33, 0.42])
    low = np.array([0.52, 0.55, 0.64])
    rgb = top[None, None] * (1 - ys[..., None]) + low[None, None] * ys[..., None]
    rgb = rgb * (0.72 + 0.5 * billow[..., None])
    save_rgba(out / "sky.png", np.clip(rgb, 0, 1), np.ones((H, W)))

    # The cloud base: a band across the top whose underside hangs in lumps.
    CH = 280
    n = fbm(W, CH, 31, scales=(120, 60, 30, 15, 8))
    yy = np.arange(CH)[:, None] * np.ones((1, W))
    xx = np.arange(W)[None, :] * np.ones((CH, 1))
    edge = 150 + 55 * np.sin(xx / 97.0) + 35 * np.sin(xx / 41.0 + 1.3) + 60 * (fbm(W, 1, 37, scales=(90, 30, 12))[0][None, :] - 0.5)
    alpha = np.clip((edge - yy) / 40.0 + (n - 0.5) * 0.8, 0, 1)
    shade = 0.36 + 0.34 * n + 0.1 * (yy / CH)
    crgb = np.stack([shade * 0.92, shade * 0.95, shade * 1.08], -1)
    save_rgba(out / "clouds.png", np.clip(crgb, 0, 1), alpha)

    # The sea: dark swells streaked with the crests of waves.
    SH = 320
    n = fbm(W, SH, 51, scales=(140, 70, 35, 16))
    yy = np.arange(SH)[:, None] / SH * np.ones((1, W))
    xx = np.arange(W)[None, :] * np.ones((SH, 1))
    swell = np.sin(xx / (30 + 60 * yy) + n * 9.0 + yy * 40.0)
    crest = np.clip((swell - 0.82) * 6.0, 0, 1) * (0.3 + 0.7 * n)
    base = np.array([0.20, 0.27, 0.33])
    srgb = base[None, None] * (0.75 + 0.5 * n[..., None]) * (1.0 - 0.35 * yy[..., None])
    srgb = srgb + crest[..., None] * np.array([0.35, 0.40, 0.42])[None, None]
    save_rgba(out / "sea.png", np.clip(srgb, 0, 1), np.ones((SH, W)))

    # The lighthouse on its rock, 2x supersampled. 200 x 440.
    S = 2
    LW, LH = 200, 440
    im = _I.new("RGBA", (LW * S, LH * S), (0, 0, 0, 0))
    d = _D.Draw(im)
    def P(x, y):
        return (x * S, y * S)
    # The rock.
    rock = [P(0, 440), P(10, 392), P(36, 372), P(70, 366), P(120, 362), P(160, 374), P(190, 396), P(200, 440)]
    d.polygon(rock, fill=(92, 88, 86, 255))
    for _ in range(40):
        x, y = rng.uniform(10, 190), rng.uniform(372, 436)
        g = int(rng.uniform(70, 120))
        d.ellipse((P(x - 8, y - 4), P(x + 8, y + 4)), fill=(g, g - 3, g - 6, 255))
    # The tower: tapering, banded red and white.
    top_y, base_y = 120, 372
    def half(y):
        return 34 - 14 * (base_y - y) / (base_y - top_y)
    bands = 6
    for k in range(bands):
        y0 = top_y + (base_y - top_y) * k / bands
        y1 = top_y + (base_y - top_y) * (k + 1) / bands
        col = (200, 60, 55, 255) if k % 2 == 0 else (236, 232, 222, 255)
        d.polygon([P(100 - half(y0), y0), P(100 + half(y0), y0), P(100 + half(y1), y1), P(100 - half(y1), y1)], fill=col)
    # Shading down the left side.
    sh = _I.new("RGBA", im.size, (0, 0, 0, 0))
    _D.Draw(sh).polygon([P(100 - half(top_y), top_y), P(100 - half(top_y) + 10, top_y), P(100 - half(base_y) + 14, base_y), P(100 - half(base_y), base_y)], fill=(0, 0, 0, 70))
    im = _I.alpha_composite(im, sh)
    d = _D.Draw(im)
    # Door and windows.
    d.rounded_rectangle((P(92, 342), P(108, 372)), radius=6 * S, fill=(60, 45, 40, 255))
    for y in (190, 250, 310):
        d.rectangle((P(96, y), P(104, y + 14)), fill=(50, 55, 70, 255))
    # Gallery, lantern room, cap and rod.
    d.rectangle((P(58, 112), P(142, 122)), fill=(40, 40, 46, 255))
    for x in range(60, 142, 8):
        d.line((P(x, 100), P(x, 112)), fill=(40, 40, 46, 255), width=S)
    d.line((P(58, 100), P(142, 100)), fill=(40, 40, 46, 255), width=2 * S)
    d.rectangle((P(76, 66), P(124, 112)), fill=(250, 236, 180, 255))
    for x in (76, 92, 108, 124):
        d.line((P(x, 66), P(x, 112)), fill=(40, 40, 46, 255), width=2 * S)
    d.polygon([P(70, 66), P(130, 66), P(100, 36)], fill=(45, 45, 52, 255))
    d.line((P(100, 36), P(100, 4)), fill=(60, 60, 66, 255), width=2 * S)
    d.ellipse((P(98, 2), P(102, 6)), fill=(80, 80, 86, 255))
    im = im.resize((LW, LH), _I.LANCZOS)
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "lighthouse.png", a[..., :3], a[..., 3])

    # The beam: a soft cone from its tip, for the additive sweep.
    BW, BH = 512, 128
    xx = np.arange(BW)[None, :] / BW * np.ones((BH, 1))
    yy = (np.arange(BH)[:, None] / (BH - 1) * 2 - 1) * np.ones((1, BW))
    spread = 0.08 + 0.92 * xx
    cone = np.clip(1.0 - np.abs(yy) / spread, 0, 1) ** 1.6 * (1.0 - xx) ** 0.8 * np.clip(xx * 20, 0, 1)
    save_rgba(out / "beam.png", np.ones((BH, BW, 3)), cone)


# ── 157 Millrace ─────────────────────────────────────────────────────────
# A mill yard from above: a meadow with a mill pond in its top left corner,
# a mill's roof, a footbridge, a horizontal ("Norse") millwheel, leaves and a
# paper boat. The race itself is not painted - it is `flow_path` ribbons, a
# bank of wet earth under running water and foam - so its textures repeat
# seamlessly down their length (v), the way the ribbon tiles them.

MR_POND = (-540.0, 250.0)


def mr157_mill():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "millrace"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(157)
    W, H = 1280, 720

    def noise(w, h, scale, seed, wrap_v=False):
        r = np.random.default_rng(seed)
        gh, gw = h // scale + 3, w // scale + 3
        g = r.random((gh, gw))
        if wrap_v:
            g[-3:] = g[:3]
        im = _I.fromarray((g * 255).astype(np.uint8)).resize((w + 3 * scale, h + 3 * scale), _I.BICUBIC)
        return np.asarray(im).astype(np.float64)[:h, :w] / 255.0

    # The meadow, and the pond the race leaves from.
    n = 0.55 * noise(W, H, 60, 1) + 0.3 * noise(W, H, 15, 2) + 0.15 * noise(W, H, 4, 3)
    rgb = np.array([0.36, 0.52, 0.24])[None, None] * (0.8 + 0.42 * n[..., None])
    im = _I.fromarray((np.clip(rgb, 0, 1) * 255).astype(np.uint8), "RGB").convert("RGBA")
    d = _D.Draw(im)
    # The pond is a sprite of its own, over the race's ribbons, so the race
    # runs out from under it rather than starting square on top of it.
    PW, PH = 300, 216
    pond = _I.new("RGBA", (PW, PH), (0, 0, 0, 0))
    pdr = _D.Draw(pond)
    pdr.ellipse((0, 0, PW - 1, PH - 1), fill=(92, 78, 52, 255))
    pdr.ellipse((15, 15, PW - 16, PH - 16), fill=(52, 92, 118, 255))
    cx, cy = PW / 2, PH / 2
    for _ in range(40):
        a = rng.uniform(0, 6.283)
        rx, ry = cx + np.cos(a) * rng.uniform(10, 115), cy + np.sin(a) * rng.uniform(10, 78)
        pdr.ellipse((rx - 14, ry - 3, rx + 14, ry + 3), fill=(80, 128, 150, 255))
    for _ in range(9):
        a = rng.uniform(0, 6.283)
        rx, ry = cx + np.cos(a) * rng.uniform(20, 105), cy + np.sin(a) * rng.uniform(15, 66)
        pdr.pieslice((rx - 11, ry - 11, rx + 11, ry + 11), 20, 340, fill=(70, 130, 60, 255))
    a = np.asarray(pond).astype(np.float64) / 255.0
    save_rgba(out / "pond.png", a[..., :3], a[..., 3])
    # Tufts and flowers on the grass.
    for _ in range(700):
        x, y = rng.integers(0, W), rng.integers(0, H)
        k = rng.random()
        if k < 0.75:
            c = int(rng.integers(40, 80))
            d.line((x, y, x + int(rng.integers(-3, 4)), y - int(rng.integers(3, 8))), fill=(c, 108 + c, 36, 255))
        else:
            col = [(245, 235, 120), (250, 250, 250), (220, 140, 200)][int(rng.integers(3))]
            d.ellipse((x - 2, y - 2, x + 2, y + 2), fill=col + (255,))
    # A gravel yard by the mill.
    d.rounded_rectangle((W / 2 + 420, H / 2 - 330, W / 2 + 640, H / 2 - 40), radius=30, fill=(150, 138, 112, 255))
    a = np.asarray(im).astype(np.float64) / 255.0
    save_rgba(out / "field.png", a[..., :3], np.ones((H, W)))

    # Running water: streaks and ripple marks down its length, seamless in v.
    TW, TH = 128, 256
    n = noise(TW, TH, 16, 11, wrap_v=True)
    v = np.arange(TH)[:, None] / TH * np.ones((1, TW))
    u = np.arange(TW)[None, :] / TW * np.ones((TH, 1))
    streak = np.sin((v * 3 + n * 1.5) * 6.283) * 0.5 + 0.5
    lanes = np.abs(np.sin(u * 3.1416 * 5 + n * 2.0))
    light = np.clip(streak * lanes - 0.55, 0, 1) * 2.2
    base = np.array([0.24, 0.42, 0.55])
    wrgb = base[None, None] * (0.85 + 0.3 * n[..., None]) + light[..., None] * np.array([0.35, 0.40, 0.38])[None, None]
    save_rgba(out / "water.png", np.clip(wrgb, 0, 1), np.ones((TH, TW)))
    # Foam: white flecks drawn out along the flow, seamless in v.
    f = noise(TW, TH, 8, 21, wrap_v=True)
    g = noise(TW, TH, 32, 22, wrap_v=True)
    foam = np.clip((f * 0.6 + g * 0.4 - 0.62) * 5.0, 0, 1)
    save_rgba(out / "foam.png", np.ones((TH, TW, 3)), foam * 0.85)

    # A leaf, white so each is tinted; its stem towards -x so it trails.
    S = 4
    L = 32
    p = _I.new("RGBA", (L * S, L * S), (0, 0, 0, 0))
    pd = _D.Draw(p)
    pd.ellipse((6 * S, 10 * S, 30 * S, 22 * S), fill=(255, 255, 255, 255))
    pd.line((2 * S, 16 * S, 28 * S, 16 * S), fill=(200, 200, 200, 255), width=S)
    for k in range(4):
        x = 10 + k * 5
        pd.line((x * S, 16 * S, (x + 3) * S, (12 + k % 2) * S), fill=(215, 215, 215, 255), width=S)
        pd.line((x * S, 16 * S, (x + 3) * S, (20 - k % 2) * S), fill=(215, 215, 215, 255), width=S)
    p = p.resize((L, L), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "leaf.png", a[..., :3], a[..., 3])
    # A paper boat from above, its bow towards +x.
    B = 64
    p = _I.new("RGBA", (B * S, B * S // 2), (0, 0, 0, 0))
    pd = _D.Draw(p)
    pd.polygon([(4 * S, 16 * S), (16 * S, 4 * S), (48 * S, 4 * S), (62 * S, 16 * S), (48 * S, 28 * S), (16 * S, 28 * S)],
               fill=(245, 242, 232, 255), outline=(170, 165, 150, 255))
    pd.polygon([(16 * S, 16 * S), (32 * S, 6 * S), (48 * S, 16 * S), (32 * S, 26 * S)], fill=(225, 220, 205, 255))
    pd.line((16 * S, 16 * S, 48 * S, 16 * S), fill=(160, 155, 140, 255), width=S)
    p = p.resize((B, B // 2), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "boat.png", a[..., :3], a[..., 3])

    # The horizontal millwheel: a hub and angled paddles round it.
    R = 160
    p = _I.new("RGBA", (R * S, R * S), (0, 0, 0, 0))
    pd = _D.Draw(p)
    c = R * S / 2
    for k in range(16):
        ang = k * 6.283 / 16
        ca, sa = np.cos(ang), np.sin(ang)
        r0, r1 = 26 * S, 74 * S
        w = 7 * S
        pts = [(c + ca * r0 - sa * w * 0.6, c + sa * r0 + ca * w * 0.6), (c + ca * r1 - sa * w, c + sa * r1 + ca * w),
               (c + ca * r1 + sa * w * 0.4, c + sa * r1 - ca * w * 0.4), (c + ca * r0 + sa * w * 0.3, c + sa * r0 - ca * w * 0.3)]
        pd.polygon(pts, fill=(150, 108, 66, 255), outline=(90, 62, 38, 255))
    pd.ellipse((c - 30 * S, c - 30 * S, c + 30 * S, c + 30 * S), fill=(110, 80, 50, 255), outline=(70, 50, 30, 255), width=2 * S)
    pd.ellipse((c - 9 * S, c - 9 * S, c + 9 * S, c + 9 * S), fill=(70, 70, 76, 255))
    p = p.resize((R, R), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "wheel.png", a[..., :3], a[..., 3])

    # The mill's roof from above: two slopes of shingles and a ridge.
    MW, MH = 260, 200
    p = _I.new("RGBA", (MW * S // 2, MH * S // 2), (0, 0, 0, 0))
    pd = _D.Draw(p)
    hs = S // 2
    for row in range(0, MH, 12):
        for col in range(-(row // 12 % 2) * 10, MW, 20):
            sh = int(rng.uniform(120, 160))
            top = row < MH / 2
            col_rgb = (sh, int(sh * 0.55), int(sh * 0.42)) if top else (int(sh * 0.8), int(sh * 0.44), int(sh * 0.34))
            pd.rectangle((col * hs, row * hs, (col + 19) * hs, (row + 11) * hs), fill=col_rgb + (255,))
    pd.rectangle((0, (MH / 2 - 4) * hs, MW * hs, (MH / 2 + 4) * hs), fill=(80, 50, 40, 255))
    pd.rectangle((0, 0, MW * hs - 1, MH * hs - 1), outline=(60, 40, 32, 255), width=2 * hs)
    p = p.resize((MW, MH), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "roof.png", a[..., :3], a[..., 3])
    # A footbridge: planks across, rails along both sides; long side across x.
    BW, BH = 180, 64
    p = _I.new("RGBA", (BW * S // 2, BH * S // 2), (0, 0, 0, 0))
    pd = _D.Draw(p)
    for k in range(0, BW, 14):
        sh = int(rng.uniform(140, 175))
        pd.rectangle((k * hs, 8 * hs, (k + 12) * hs, (BH - 8) * hs), fill=(sh, int(sh * 0.74), int(sh * 0.5), 255))
    pd.rectangle((0, 0, BW * hs, 7 * hs), fill=(100, 70, 45, 255))
    pd.rectangle((0, (BH - 7) * hs, BW * hs, BH * hs), fill=(100, 70, 45, 255))
    p = p.resize((BW, BH), _I.LANCZOS)
    a = np.asarray(p).astype(np.float64) / 255.0
    save_rgba(out / "bridge.png", a[..., :3], a[..., 3])
    # A soft shadow for things standing above the water.
    sh = _I.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
    _D.Draw(sh).ellipse((10 * S, 10 * S, 54 * S, 54 * S), fill=(0, 0, 0, 255))
    sh = sh.filter(_F.GaussianBlur(S * 6)).resize((64, 64), _I.LANCZOS)
    a = np.asarray(sh).astype(np.float64) / 255.0
    save_rgba(out / "shadow.png", a[..., :3] * 0, a[..., 3] * 0.5)


# ── 158 First Snow ───────────────────────────────────────────────────────
# A village lane at dusk, side on: a sky, the ground, two cottages, a fence,
# a lamp post, a bench, pines and a hand cart - each its own sprite, painted
# with clear headroom over its top edges, because the snow (`snow_cover`)
# lies in a picture's clear texels above the surfaces it settles on.

def fs158_village():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "firstsnow"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(158)
    S = 2

    def save(im, name):
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    def canvas(w, h):
        im = _I.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
        return im, _D.Draw(im)

    def P(*xy):
        return tuple(v * S for v in xy)

    # The sky: dusk, deep blue over a pale band at the horizon.
    W, H = 1280, 720
    t = np.linspace(0, 1, H)[:, None, None]
    top = np.array([0.16, 0.20, 0.36])
    low = np.array([0.62, 0.56, 0.62])
    rgb = top * (1 - t ** 1.4) + low * t ** 1.4
    rgb = np.broadcast_to(rgb, (H, W, 3)).copy()
    # Far hills.
    xs = np.arange(W)
    for k, (base, amp, col) in enumerate(((470, 40, (0.30, 0.32, 0.42)), (520, 30, (0.24, 0.26, 0.36)))):
        ridge = base - amp * (0.6 * np.sin(xs / (140 + 40 * k)) + 0.4 * np.sin(xs / (53 + 9 * k) + k))
        mask = np.arange(H)[:, None] > ridge[None, :]
        rgb[mask] = col
    save_rgba(out / "sky.png", rgb, np.ones((H, W)))

    # The ground: a strip of frozen earth, a flat top for snow to lie on.
    gw, gh = 1400, 200
    im, d = canvas(gw, gh)
    d.rectangle(P(0, 40, gw, gh), fill=(78, 66, 56, 255))
    for _ in range(300):
        x, y = rng.uniform(0, gw), rng.uniform(44, gh)
        g = int(rng.uniform(60, 100))
        d.ellipse(P(x - 3, y - 2, x + 3, y + 2), fill=(g, g - 10, g - 18, 255))
    save(im.resize((gw, gh), _I.LANCZOS), "ground.png")

    # A cottage: walls, a steep roof, a chimney, windows lit warm.
    def cottage(name, w, h, roof, wall):
        im, d = canvas(w, h)
        eave = 110
        d.rectangle(P(20, eave, w - 20, h), fill=wall + (255,))
        for k in range(eave + 8, h, 14):
            d.line(P(20, k, w - 20, k), fill=tuple(int(c * 0.88) for c in wall) + (255,), width=S)
        d.polygon([P(4, eave + 6), P(w / 2, 34), P(w - 4, eave + 6)], fill=roof + (255,))
        d.line([P(4, eave + 6), P(w / 2, 34), P(w - 4, eave + 6)], fill=tuple(int(c * 0.7) for c in roof) + (255,), width=3 * S)
        d.rectangle(P(w * 0.68, 44, w * 0.68 + 22, 86), fill=(120, 90, 80, 255))
        d.rectangle(P(w * 0.68 - 3, 40, w * 0.68 + 25, 46), fill=(90, 66, 60, 255))
        for wx in (w * 0.24, w * 0.62):
            d.rectangle(P(wx, eave + 34, wx + 34, eave + 74), fill=(255, 205, 120, 255), outline=(70, 50, 40, 255), width=3 * S)
            d.line(P(wx + 17, eave + 34, wx + 17, eave + 74), fill=(70, 50, 40, 255), width=2 * S)
            # A sill, a ledge for snow.
            d.rectangle(P(wx - 5, eave + 74, wx + 39, eave + 80), fill=(110, 90, 72, 255))
        d.rectangle(P(w / 2 - 20, h - 64, w / 2 + 20, h), fill=(90, 60, 42, 255))
        save(im.resize((w, h), _I.LANCZOS), name)

    cottage("cottage_a.png", 300, 300, (130, 60, 50), (200, 180, 150))
    cottage("cottage_b.png", 260, 280, (70, 80, 100), (180, 160, 140))

    # A fence: posts and two rails, their tops all ledges.
    fw, fh = 420, 110
    im, d = canvas(fw, fh)
    for x in range(10, fw, 52):
        d.rectangle(P(x, 30, x + 12, fh), fill=(120, 92, 64, 255))
        d.polygon([P(x, 30), P(x + 6, 22), P(x + 12, 30)], fill=(120, 92, 64, 255))
    for y in (46, 78):
        d.rectangle(P(0, y, fw, y + 9), fill=(135, 104, 72, 255))
    save(im.resize((fw, fh), _I.LANCZOS), "fence.png")

    # A lamp post: a post, a lantern with a little roof.
    lw, lh = 80, 300
    im, d = canvas(lw, lh)
    d.rectangle(P(36, 70, 44, lh), fill=(50, 50, 56, 255))
    d.rectangle(P(26, lh - 14, 54, lh), fill=(50, 50, 56, 255))
    d.rectangle(P(24, 40, 56, 72), fill=(255, 215, 140, 255), outline=(50, 50, 56, 255), width=3 * S)
    d.polygon([P(18, 42), P(40, 22), P(62, 42)], fill=(50, 50, 56, 255))
    save(im.resize((lw, lh), _I.LANCZOS), "lamp.png")

    # A bench.
    bw, bh = 170, 90
    im, d = canvas(bw, bh)
    d.rectangle(P(10, 44, bw - 10, 54), fill=(130, 96, 62, 255))
    d.rectangle(P(10, 20, bw - 10, 28), fill=(130, 96, 62, 255))
    for x in (20, bw - 30):
        d.rectangle(P(x, 20, x + 8, bh), fill=(70, 60, 56, 255))
    save(im.resize((bw, bh), _I.LANCZOS), "bench.png")

    # A pine: tiers of boughs, each a ledge.
    def pine(name, w, h):
        im, d = canvas(w, h)
        d.rectangle(P(w / 2 - 7, h - 40, w / 2 + 7, h), fill=(80, 56, 40, 255))
        tiers = 5
        for k in range(tiers):
            top = 20 + k * (h - 70) / tiers
            half = w * (0.18 + 0.32 * (k + 1) / tiers)
            base = top + (h - 70) / tiers + 26
            d.polygon([P(w / 2, top), P(w / 2 + half, base), P(w / 2 - half, base)], fill=(30, 70 + 6 * k, 50, 255))
        save(im.resize((w, h), _I.LANCZOS), name)

    pine("pine.png", 180, 320)

    # A hand cart: a bed and a wheel.
    cw, ch = 180, 110
    im, d = canvas(cw, ch)
    d.rectangle(P(10, 30, 150, 66), fill=(140, 100, 60, 255), outline=(90, 62, 38, 255), width=2 * S)
    d.line(P(150, 40, 178, 26), fill=(110, 80, 50, 255), width=4 * S)
    d.ellipse(P(50, 52, 100, 102), outline=(70, 50, 36, 255), width=5 * S)
    d.ellipse(P(70, 72, 80, 82), fill=(70, 50, 36, 255))
    save(im.resize((cw, ch), _I.LANCZOS), "cart.png")


# ── 159 Rooftop Runners ──────────────────────────────────────────────────
# A city block from above: streets far below, five roofs of different
# kinds, and what joins them - planks over an alley, a footbridge over the
# street, a zipline. Runners are couriers seen from above, white so each is
# tinted. The roofs' sizes are the scene's (RR_ROOFS in gen_scenes.py).

RR_ROOFS = {  # name: (x0, y0, x1, y1, kind) in world units
    "RoofA": (-620.0, 60.0, -280.0, 330.0, "tiles"),
    "RoofB": (-220.0, 60.0, 160.0, 330.0, "tar"),
    "RoofE": (220.0, 100.0, 620.0, 330.0, "slate"),
    "RoofC": (-620.0, -330.0, -120.0, -80.0, "slate"),
    "RoofD": (-60.0, -330.0, 620.0, -80.0, "tiles"),
}


def rr159_city():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "rooftops"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(159)
    W, H = 1280, 720
    S = 2

    def save(im, name):
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The streets below: asphalt, kerbs, markings, the odd car roof.
    im = _I.new("RGBA", (W, H), (58, 60, 66, 255))
    d = _D.Draw(im)
    for _ in range(4000):
        x, y = rng.integers(0, W), rng.integers(0, H)
        g = int(rng.integers(50, 70))
        d.point((x, y), fill=(g, g + 2, g + 6, 255))
    # The wide street's centre line (world y -10) and pavements.
    cy = H / 2 + 10
    for x in range(0, W, 60):
        d.rectangle((x, cy - 2, x + 30, cy + 2), fill=(220, 200, 120, 255))
    for x0, y0, x1, y1, _ in RR_ROOFS.values():
        px0, py0, px1, py1 = x0 + W / 2 - 14, H / 2 - y1 - 14, x1 + W / 2 + 14, H / 2 - y0 + 14
        d.rectangle((px0, py0, px1, py1), fill=(120, 118, 112, 255))
    for k in range(7):
        x = rng.integers(40, W - 120)
        y = cy + rng.choice([-34, 30])
        col = [(170, 40, 40), (40, 90, 160), (230, 230, 230), (60, 130, 70)][k % 4]
        d.rounded_rectangle((x, y - 12, x + 62, y + 12), radius=6, fill=col + (255,))
        d.rounded_rectangle((x + 16, y - 9, x + 44, y + 9), radius=4, fill=(40, 50, 60, 255))
    save(im, "streets.png")

    # The roofs, each its own picture with a parapet round it.
    for name, (x0, y0, x1, y1, kind) in RR_ROOFS.items():
        w, h = int(x1 - x0), int(y1 - y0)
        im = _I.new("RGBA", (w, h), (0, 0, 0, 255))
        d = _D.Draw(im)
        if kind == "tiles":
            for row, y in enumerate(range(0, h, 14)):
                for x in range(-(row % 2) * 9, w, 18):
                    t = int(rng.uniform(150, 185))
                    d.rounded_rectangle((x, y, x + 16, y + 13), radius=3, fill=(t, int(t * 0.5), int(t * 0.36), 255))
        elif kind == "tar":
            d.rectangle((0, 0, w, h), fill=(64, 64, 68, 255))
            for _ in range(900):
                x, y = rng.integers(0, w), rng.integers(0, h)
                g = int(rng.integers(55, 80))
                d.point((x, y), fill=(g, g, g + 4, 255))
            for k in range(0, h, 46):
                d.line((0, k, w, k), fill=(52, 52, 56, 255), width=2)
        else:
            for row, y in enumerate(range(0, h, 12)):
                for x in range(-(row % 2) * 10, w, 20):
                    t = int(rng.uniform(85, 110))
                    d.rectangle((x, y, x + 18, y + 11), fill=(t, t + 6, t + 16, 255))
        d.rectangle((0, 0, w - 1, h - 1), outline=(150, 145, 135, 255), width=6)
        save(im, name.lower() + ".png")

    # Things on the roofs: a chimney stack, a water tank, a vent, a skylight.
    def thing(name, w, h, draw):
        im = _I.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
        draw(_D.Draw(im), w * S, h * S)
        save(im.resize((w, h), _I.LANCZOS), name)

    thing("chimney.png", 48, 48, lambda d, w, h: (d.rectangle((2, 2, w - 3, h - 3), fill=(140, 80, 66, 255), outline=(90, 50, 40, 255), width=4),
                                                  d.ellipse((w * 0.2, h * 0.2, w * 0.45, h * 0.45), fill=(30, 30, 30, 255)),
                                                  d.ellipse((w * 0.55, h * 0.55, w * 0.8, h * 0.8), fill=(30, 30, 30, 255))))
    thing("tank.png", 90, 90, lambda d, w, h: (d.ellipse((2, 2, w - 3, h - 3), fill=(130, 100, 70, 255), outline=(80, 60, 40, 255), width=6),
                                               d.ellipse((w * 0.3, h * 0.3, w * 0.7, h * 0.7), fill=(110, 84, 58, 255))))
    thing("vent.png", 56, 40, lambda d, w, h: (d.rectangle((2, 2, w - 3, h - 3), fill=(170, 175, 180, 255), outline=(110, 115, 120, 255), width=4),
                                               [d.line((8, y, w - 8, y), fill=(110, 115, 120, 255), width=3) for y in range(10, h - 6, 10)]))
    thing("skylight.png", 80, 60, lambda d, w, h: (d.rectangle((2, 2, w - 3, h - 3), fill=(150, 190, 220, 255), outline=(90, 90, 96, 255), width=6),
                                                   d.line((w / 2, 4, w / 2, h - 4), fill=(90, 90, 96, 255), width=4)))
    # A plank (long side along x), a footbridge, and a runner's shadow.
    thing("plank.png", 100, 26, lambda d, w, h: [d.rectangle((0, k * h / 2, w, (k + 1) * h / 2 - 2), fill=(165, 120, 75, 255)) for k in range(2)])
    thing("bridge.png", 60, 200, lambda d, w, h: (d.rectangle((6, 0, w - 7, h), fill=(150, 150, 156, 255)),
                                                  [d.line((6, y, w - 7, y), fill=(110, 110, 116, 255), width=3) for y in range(0, h, 16)],
                                                  d.rectangle((0, 0, 6, h), fill=(70, 70, 76, 255)), d.rectangle((w - 7, 0, w, h), fill=(70, 70, 76, 255))))
    sh = _I.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
    _D.Draw(sh).ellipse((12 * S, 12 * S, 52 * S, 52 * S), fill=(0, 0, 0, 255))
    sh = sh.filter(_F.GaussianBlur(S * 5)).resize((64, 64), _I.LANCZOS)
    a = np.asarray(sh).astype(np.float64) / 255.0
    save_rgba(out / "shadow.png", a[..., :3] * 0, a[..., 3] * 0.6)

    # A courier from above, facing +x: shoulders, a satchel, a cap.
    def runner(d, w, h):
        c = w / 2
        d.ellipse((c - 10 * S, c - 16 * S, c + 6 * S, c + 16 * S), fill=(240, 240, 240, 255))
        d.ellipse((c - 2 * S, c - 18 * S, c + 10 * S, c - 8 * S), fill=(225, 225, 225, 255))
        d.ellipse((c - 2 * S, c + 8 * S, c + 10 * S, c + 18 * S), fill=(225, 225, 225, 255))
        d.rectangle((c - 16 * S, c - 7 * S, c - 6 * S, c + 7 * S), fill=(150, 120, 80, 255))
        d.ellipse((c - 8 * S, c - 8 * S, c + 8 * S, c + 8 * S), fill=(90, 70, 55, 255))
        d.pieslice((c - 8 * S, c - 8 * S, c + 8 * S, c + 8 * S), -60, 60, fill=(255, 255, 255, 255))
        d.rectangle((c + 6 * S, c - 3 * S, c + 12 * S, c + 3 * S), fill=(255, 255, 255, 255))

    thing("runner.png", 48, 48, runner)


# ── 160 Shape Shifter ────────────────────────────────────────────────────
# A dark stage: a deep radial gradient and a faint floor line. The shapes
# themselves are vector paths in the scene.

def ss160_stage():
    out = OUT / "shapeshifter"
    out.mkdir(parents=True, exist_ok=True)
    W, H = 1280, 720
    ys, xs = np.mgrid[0:H, 0:W]
    d = np.hypot((xs - W / 2) / W, (ys - H * 0.45) / H)
    t = np.clip(d * 1.6, 0, 1)
    rgb = np.array([0.10, 0.11, 0.20])[None, None] * (1 - t[..., None]) + np.array([0.02, 0.02, 0.05])[None, None] * t[..., None]
    floor = np.exp(-((ys - 600) / 3.0) ** 2) * 0.08 * (1 - np.abs(xs - W / 2) / (W / 2))
    rgb = rgb + floor[..., None]
    save_rgba(out / "stage.png", np.clip(rgb, 0, 1), np.ones((H, W)))


# ── 161 Trophy Room ──────────────────────────────────────────────────────
# A panelled trophy room: shelves of trophies to click, and the pieces of
# the toasts - a card (nine-sliced: a rounded panel with a rim) in two
# moods, and icons drawn white so a toast can show them as they are.

def tr161_room():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "trophyroom"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(161)
    W, H = 1280, 720
    S = 2

    def save(im, name):
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The room: dark wood panelling, a wainscot, a rug, and three shelves.
    im = _I.new("RGBA", (W, H), (60, 36, 26, 255))
    d = _D.Draw(im)
    for x in range(0, W, 160):
        g = int(rng.uniform(-6, 6))
        d.rectangle((x + 6, 20, x + 154, 470), fill=(74 + g, 45 + g, 32 + g, 255), outline=(48, 28, 20, 255), width=3)
        for k in range(8):
            yy = 40 + k * 55
            d.line((x + 14, yy, x + 146, yy + int(rng.uniform(-3, 3))), fill=(68 + g, 41 + g, 29 + g, 255), width=1)
    d.rectangle((0, 470, W, 520), fill=(96, 60, 40, 255))
    d.rectangle((0, 520, W, H), fill=(52, 30, 22, 255))
    d.ellipse((220, 560, 1060, 760), fill=(110, 30, 34, 255), outline=(170, 130, 60, 255), width=5)
    for y in (170, 300, 430):
        d.rectangle((90, y, 1190, y + 16), fill=(120, 78, 48, 255), outline=(70, 44, 28, 255), width=2)
        for x in range(110, 1190, 220):
            d.polygon([(x, y + 16), (x + 20, y + 16), (x + 10, y + 40)], fill=(90, 58, 36, 255))
    im = im.filter(_F.GaussianBlur(0.6))
    save(im, "room.png")

    # Trophies: a cup, a star, a shield, a medal on a stand - white-ish
    # metal, tinted gold, silver or bronze in the scene.
    def trophy(name, draw):
        t = _I.new("RGBA", (96 * S, 112 * S), (0, 0, 0, 0))
        draw(_D.Draw(t), S)
        save(t.resize((96, 112), _I.LANCZOS), name)

    def cup(d, s):
        d.rectangle((30 * s, 96 * s, 66 * s, 110 * s), fill=(120, 90, 70, 255))
        d.rectangle((42 * s, 70 * s, 54 * s, 96 * s), fill=(235, 235, 235, 255))
        d.pieslice((16 * s, -10 * s, 80 * s, 76 * s), 0, 180, fill=(250, 250, 250, 255))
        d.arc((4 * s, 18 * s, 28 * s, 50 * s), 90, 270, fill=(235, 235, 235, 255), width=4 * s)
        d.arc((68 * s, 18 * s, 92 * s, 50 * s), -90, 90, fill=(235, 235, 235, 255), width=4 * s)
        d.ellipse((38 * s, 40 * s, 58 * s, 56 * s), fill=(210, 210, 210, 255))

    def star(d, s):
        d.rectangle((30 * s, 96 * s, 66 * s, 110 * s), fill=(120, 90, 70, 255))
        d.rectangle((44 * s, 66 * s, 52 * s, 96 * s), fill=(235, 235, 235, 255))
        pts = []
        for k in range(10):
            r = (36 if k % 2 == 0 else 15) * s
            a = np.pi / 2 + k * np.pi / 5
            pts.append((48 * s + r * np.cos(a), 38 * s - r * np.sin(a)))
        d.polygon(pts, fill=(250, 250, 250, 255))

    def shield(d, s):
        d.rectangle((30 * s, 96 * s, 66 * s, 110 * s), fill=(120, 90, 70, 255))
        d.polygon([(18 * s, 10 * s), (78 * s, 10 * s), (78 * s, 50 * s), (48 * s, 92 * s), (18 * s, 50 * s)], fill=(245, 245, 245, 255))
        d.polygon([(30 * s, 20 * s), (66 * s, 20 * s), (66 * s, 48 * s), (48 * s, 76 * s), (30 * s, 48 * s)], fill=(215, 215, 215, 255))

    def medal(d, s):
        d.rectangle((30 * s, 96 * s, 66 * s, 110 * s), fill=(120, 90, 70, 255))
        d.polygon([(30 * s, 4 * s), (48 * s, 40 * s), (66 * s, 4 * s), (56 * s, 4 * s), (48 * s, 24 * s), (40 * s, 4 * s)], fill=(160, 60, 70, 255))
        d.ellipse((24 * s, 36 * s, 72 * s, 84 * s), fill=(250, 250, 250, 255))
        d.ellipse((32 * s, 44 * s, 64 * s, 76 * s), fill=(220, 220, 220, 255))
        d.rectangle((44 * s, 84 * s, 52 * s, 96 * s), fill=(235, 235, 235, 255))

    for n, f in (("cup", cup), ("star", star), ("shield", shield), ("medal", medal)):
        trophy(f"trophy_{n}.png", f)

    # The toast cards: 64 x 64, corners within 18 px, nine-sliced.
    def card(name, fill, rim):
        c = _I.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
        cd = _D.Draw(c)
        cd.rounded_rectangle((2 * S, 2 * S, 62 * S, 62 * S), radius=14 * S, fill=fill + (240,), outline=rim + (255,), width=3 * S)
        cd.rounded_rectangle((6 * S, 6 * S, 58 * S, 20 * S), radius=8 * S, fill=tuple(min(255, v + 18) for v in fill) + (90,))
        save(c.resize((64, 64), _I.LANCZOS), name)

    card("card_gold.png", (40, 30, 22), (225, 180, 80))
    card("card_blue.png", (22, 30, 46), (110, 160, 230))

    # Icons for the toasts, in colour on clear.
    def icon(name, draw):
        c = _I.new("RGBA", (64 * S, 64 * S), (0, 0, 0, 0))
        draw(_D.Draw(c), S)
        save(c.resize((64, 64), _I.LANCZOS), name)

    icon("icon_cup.png", lambda d, s: (d.pieslice((12 * s, -6 * s, 52 * s, 44 * s), 0, 180, fill=(240, 195, 70, 255)),
                                       d.rectangle((28 * s, 38 * s, 36 * s, 52 * s), fill=(220, 175, 60, 255)),
                                       d.rectangle((20 * s, 52 * s, 44 * s, 60 * s), fill=(150, 100, 60, 255))))
    icon("icon_star.png", lambda d, s: d.polygon([(32 * s + (28 if k % 2 == 0 else 12) * s * np.cos(np.pi / 2 + k * np.pi / 5),
                                                   32 * s - (28 if k % 2 == 0 else 12) * s * np.sin(np.pi / 2 + k * np.pi / 5)) for k in range(10)],
                                                 fill=(255, 220, 90, 255)))
    icon("icon_gem.png", lambda d, s: (d.polygon([(32 * s, 58 * s), (6 * s, 24 * s), (18 * s, 8 * s), (46 * s, 8 * s), (58 * s, 24 * s)], fill=(90, 200, 230, 255)),
                                       d.polygon([(32 * s, 58 * s), (18 * s, 24 * s), (46 * s, 24 * s)], fill=(160, 230, 250, 255))))
    icon("icon_bell.png", lambda d, s: (d.pieslice((12 * s, 8 * s, 52 * s, 60 * s), 180, 360, fill=(250, 210, 120, 255)),
                                        d.rectangle((12 * s, 33 * s, 52 * s, 46 * s), fill=(250, 210, 120, 255)),
                                        d.ellipse((26 * s, 46 * s, 38 * s, 58 * s), fill=(220, 170, 80, 255))))
    icon("icon_friend.png", lambda d, s: (d.ellipse((20 * s, 6 * s, 44 * s, 30 * s), fill=(150, 210, 255, 255)),
                                          d.pieslice((8 * s, 30 * s, 56 * s, 78 * s), 180, 360, fill=(110, 170, 240, 255))))
    icon("icon_scroll.png", lambda d, s: (d.rectangle((14 * s, 12 * s, 50 * s, 52 * s), fill=(240, 225, 190, 255)),
                                          d.ellipse((8 * s, 6 * s, 20 * s, 18 * s), fill=(200, 180, 140, 255)),
                                          d.ellipse((44 * s, 46 * s, 56 * s, 58 * s), fill=(200, 180, 140, 255)),
                                          [d.line((20 * s, y * s, 44 * s, y * s), fill=(150, 120, 90, 255), width=2 * s) for y in (22, 30, 38)]))

# The pinball table of 162 Instant Replay, shared by its art and its scene:
# world units, the playfield centred on the screen.
PB_BUMPERS = ((-100.0, 140.0), (60.0, 140.0), (-20.0, 50.0))
PB_LANES = (-130.0, -60.0, 10.0)
PB_LANE_Y = 285.0
PB_POSTS = (-165.0, -95.0, -25.0, 45.0)
PB_SLING_L = ((-205.0, -80.0), (-205.0, -165.0), (-150.0, -200.0))
PB_SLING_R = ((165.0, -80.0), (165.0, -165.0), (110.0, -200.0))
PB_PIVOTS = ((-130.0, -245.0), (90.0, -245.0))
PB_RAILS = (  # every wall the ball meets, as segments
    ((-245.0, -150.0), (-245.0, 300.0)),
    ((-245.0, 300.0), (-200.0, 345.0)),
    ((-200.0, 345.0), (190.0, 345.0)),
    ((190.0, 345.0), (250.0, 285.0)),
    ((250.0, 285.0), (250.0, -360.0)),
    ((203.0, 140.0), (203.0, -360.0)),
    ((-245.0, -150.0), (-135.0, -240.0)),
    ((198.0, -150.0), (95.0, -240.0)),
)

# ── 162 Instant Replay ───────────────────────────────────────────────────
# A pinball table: the cabinet round it, the playfield (its lanes painted
# where the scene's colliders are), a chrome ball, flippers, bumper caps,
# slingshots, rollover inserts and a bar for the replay's progress.

def pb162_table():
    from PIL import Image as _I, ImageDraw as _D, ImageFilter as _F
    out = OUT / "pinball"
    out.mkdir(parents=True, exist_ok=True)
    rng = np.random.default_rng(162)
    S = 2

    def save(im, name):
        a = np.asarray(im).astype(np.float64) / 255.0
        save_rgba(out / name, a[..., :3], a[..., 3])

    # The cabinet: a dark room with two lit side panels.
    W, H = 1280, 720
    yy, xx = np.mgrid[0:H, 0:W].astype(np.float64)
    base = np.zeros((H, W, 3))
    base[...] = (0.035, 0.03, 0.06)
    glow = np.exp(-((xx - 640) ** 2) / (2 * 420.0 ** 2)) * 0.05
    base += glow[..., None] * np.array([0.5, 0.4, 1.0])
    im = _I.fromarray((np.clip(base, 0, 1) * 255).astype(np.uint8)).convert("RGBA")
    d = _D.Draw(im)
    for x0 in (40, 950):
        d.rounded_rectangle((x0, 120, x0 + 290, 630), 26, fill=(22, 16, 40, 255), outline=(120, 70, 200, 255), width=4)
        d.rounded_rectangle((x0 + 12, 132, x0 + 278, 618), 18, outline=(60, 34, 110, 255), width=2)
        for k in range(14):
            cx, cy = x0 + 145 + rng.uniform(-110, 110), rng.uniform(150, 600)
            r = rng.uniform(1.5, 3.5)
            d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(200, 170, 255, 120))
    # The table's rails, left and right of the glass.
    d.rectangle((364, 0, 380, 720), fill=(70, 72, 84, 255))
    d.rectangle((900, 0, 916, 720), fill=(70, 72, 84, 255))
    save(im.filter(_F.GaussianBlur(0.5)), "cabinet.png")

    # The playfield, 520 x 720: world (x, y) is pixel (x + 260, 360 - y).
    PW, PH = 520, 720

    def px(x, y):
        return ((x + 260) * S, (360 - y) * S)

    f = _I.new("RGBA", (PW * S, PH * S), (0, 0, 0, 255))
    fy, fx = np.mgrid[0:PH * S, 0:PW * S].astype(np.float64)
    t = fy / (PH * S)
    col = np.stack([0.05 + 0.10 * t, 0.06 + 0.04 * t, 0.22 - 0.08 * t], -1)
    rays = 0.5 + 0.5 * np.cos(np.arctan2(fy - 0.30 * PH * S, fx - 0.46 * PW * S) * 18)
    col += (rays * 0.035)[..., None] * np.array([0.6, 0.4, 1.0])
    f = _I.fromarray((np.clip(col, 0, 1) * 255).astype(np.uint8)).convert("RGBA")
    d = _D.Draw(f)
    # Stars.
    for k in range(70):
        cx, cy = rng.uniform(0, PW * S), rng.uniform(0, PH * S)
        r = rng.uniform(1, 3) * S / 2
        d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=(220, 210, 255, int(rng.uniform(60, 160))))
    # Rings under the bumpers, and a sunburst round the middle one.
    for bx, by in PB_BUMPERS:
        cx, cy = px(bx, by)
        for k, a in ((58, 70), (46, 110)):
            d.ellipse((cx - k * S, cy - k * S, cx + k * S, cy + k * S), outline=(255, 120, 200, a), width=3 * S)
    # Arrows pointing up the field at the rollover lanes.
    for lx in PB_LANES:
        cx, cy = px(lx, 210)
        d.polygon([(cx, cy - 26 * S), (cx - 14 * S, cy), (cx + 14 * S, cy)], fill=(255, 200, 80, 110))
    # Insert outlines under the rollover lights.
    for lx in PB_LANES:
        cx, cy = px(lx, PB_LANE_Y)
        d.ellipse((cx - 18 * S, cy - 18 * S, cx + 18 * S, cy + 18 * S), outline=(255, 210, 120, 160), width=2 * S)
    # The launch lane, its floor darker.
    d.rectangle(px(208, 140) + px(250, -360), fill=(12, 10, 26, 255))
    for k in range(6):
        cx, cy = px(229, -260 + k * 60)
        d.polygon([(cx, cy - 12 * S), (cx - 8 * S, cy + 2 * S), (cx + 8 * S, cy + 2 * S)], fill=(120, 220, 255, 90 + k * 20))
    # The drain's apron: a band under the flippers.
    d.polygon([px(-250, -150), px(-130, -245), px(-130, -360), px(-250, -360)], fill=(30, 18, 50, 255))
    d.polygon([px(200, -150), px(90, -245), px(90, -360), px(200, -360)], fill=(30, 18, 50, 255))
    d.rectangle(px(-130, -320) + px(90, -360), fill=(40, 10, 24, 255))
    # Every wall the ball meets, drawn as a steel rail on the art.
    rail = (170, 175, 195, 255)
    for (x0, y0), (x1, y1) in PB_RAILS:
        d.line(px(x0, y0) + px(x1, y1), fill=rail, width=10 * S)
        for x, y in ((x0, y0), (x1, y1)):
            cx, cy = px(x, y)
            d.ellipse((cx - 5 * S, cy - 5 * S, cx + 5 * S, cy + 5 * S), fill=rail)
    for x in PB_POSTS:
        cx, cy = px(x, PB_LANE_Y)
        d.ellipse((cx - 7 * S, cy - 7 * S, cx + 7 * S, cy + 7 * S), fill=(230, 200, 120, 255))
    f = f.filter(_F.GaussianBlur(0.8))
    save(f.resize((PW, PH), _I.LANCZOS), "playfield.png")

    # The ball: chrome, a sky reflection over a dark band.
    b = _I.new("RGBA", (48 * S, 48 * S), (0, 0, 0, 0))
    by_, bx_ = np.mgrid[0:48 * S, 0:48 * S].astype(np.float64)
    r = 23.0 * S
    dx, dy = (bx_ - 24 * S) / r, (by_ - 24 * S) / r
    inside = dx * dx + dy * dy <= 1.0
    v = 0.55 - 0.45 * dy + 0.25 * np.exp(-((dx + 0.35) ** 2 + (dy + 0.4) ** 2) * 9)
    v = np.where(np.abs(dy - 0.1) < 0.12, v * 0.5, v)
    rgb = np.clip(np.stack([v * 0.95, v * 0.97, v * 1.05], -1), 0, 1)
    a = np.where(inside, 1.0, 0.0)
    b = _I.fromarray((np.dstack([rgb, a]) * 255).astype(np.uint8), "RGBA").resize((48, 48), _I.LANCZOS)
    save(b, "ball.png")

    # A flipper: thick at its pivot (left), tapering, white with a red rubber.
    fl = _I.new("RGBA", (110 * S, 26 * S), (0, 0, 0, 0))
    d = _D.Draw(fl)
    d.polygon([(13 * S, 1 * S), (104 * S, 7 * S), (104 * S, 19 * S), (13 * S, 25 * S)], fill=(200, 40, 50, 255))
    d.ellipse((1 * S, 1 * S, 25 * S, 25 * S), fill=(200, 40, 50, 255))
    d.ellipse((96 * S, 7 * S, 108 * S, 19 * S), fill=(200, 40, 50, 255))
    d.polygon([(13 * S, 5 * S), (102 * S, 10 * S), (102 * S, 16 * S), (13 * S, 21 * S)], fill=(245, 245, 250, 255))
    d.ellipse((5 * S, 5 * S, 21 * S, 21 * S), fill=(245, 245, 250, 255))
    d.ellipse((9 * S, 9 * S, 17 * S, 17 * S), fill=(150, 150, 165, 255))
    fl = fl.resize((110, 26), _I.LANCZOS)
    save(fl, "flipper.png")
    save(fl.transpose(_I.FLIP_LEFT_RIGHT), "flipper_r.png")

    # A bumper cap, drawn white so the scene can tint it and flash it.
    bc = _I.new("RGBA", (72 * S, 72 * S), (0, 0, 0, 0))
    d = _D.Draw(bc)
    d.ellipse((2 * S, 2 * S, 70 * S, 70 * S), fill=(150, 150, 160, 255))
    d.ellipse((8 * S, 8 * S, 64 * S, 64 * S), fill=(255, 255, 255, 255))
    d.ellipse((18 * S, 18 * S, 54 * S, 54 * S), fill=(215, 215, 225, 255))
    for k in range(8):
        ang = k * np.pi / 4
        cx, cy = 36 * S + 23 * S * np.cos(ang), 36 * S + 23 * S * np.sin(ang)
        d.ellipse((cx - 3 * S, cy - 3 * S, cx + 3 * S, cy + 3 * S), fill=(255, 255, 255, 255))
    save(bc.resize((72, 72), _I.LANCZOS), "bumper.png")

    # A slingshot: its triangle, pixel for pixel the collider's.
    for name, pts in (("sling_l.png", PB_SLING_L), ("sling_r.png", PB_SLING_R)):
        xs, ys = [p[0] for p in pts], [p[1] for p in pts]
        x0, y1 = min(xs) - 6, max(ys) + 6
        w, h = int(max(xs) - x0 + 6), int(y1 - min(ys) + 6)
        sl = _I.new("RGBA", (w * S, h * S), (0, 0, 0, 0))
        d = _D.Draw(sl)
        poly = [((x - x0) * S, (y1 - y) * S) for x, y in pts]
        d.polygon(poly, fill=(255, 255, 255, 255), outline=(255, 255, 255, 255))
        d.line(poly + [poly[0]], fill=(150, 150, 160, 255), width=4 * S)
        save(sl.resize((w, h), _I.LANCZOS), name)

    # A round insert light (rollover lanes, the REC lamp).
    li = _I.new("RGBA", (32 * S, 32 * S), (0, 0, 0, 0))
    d = _D.Draw(li)
    d.ellipse((1 * S, 1 * S, 31 * S, 31 * S), fill=(255, 255, 255, 255))
    d.ellipse((9 * S, 6 * S, 17 * S, 12 * S), fill=(255, 255, 255, 255))
    save(li.resize((32, 32), _I.LANCZOS).filter(_F.GaussianBlur(0.4)), "light.png")

    # The progress bar and its frame.
    bar = _I.new("RGBA", (260, 14), (255, 255, 255, 255))
    save(bar, "bar.png")
    fr = _I.new("RGBA", (268, 22), (0, 0, 0, 0))
    _D.Draw(fr).rounded_rectangle((0, 0, 267, 21), 6, outline=(255, 255, 255, 255), width=2)
    save(fr, "bar_frame.png")


if __name__ == "__main__":
    moon_shaft()
    glow()
    soft_dot()
    ember()
    spark()
    ring()
    rift()
    smoke()
    flame()
    flagstones()
    pillar_top()
    bricks()
    column()
    barrel()
    crate()
    chain()
    pew()
    carpet()
    shaft()
    nebula_clouds()
    star_fields()
    star_flare()
    planet()
    moon()
    rune_ring()
    rune_seal()
    bolt()
    shard()
    mage()
    sand()
    rock()
    kelp()
    fish()
    jelly()
    bubble()
    water_bg()
    vfade()
    night_sky()
    hills()
    lantern()
    clay_box()
    clay_sphere()
    clay_cylinder()
    ribbon_soft()
    ribbon_slash()
    pixel_sky()
    pixel_cloud("pp_cloud1.png", 56, 20, 21)
    pixel_cloud("pp_cloud2.png", 40, 16, 22)
    pixel_cloud("pp_cloud3.png", 72, 24, 23)
    pixel_mountains()
    pixel_cliff("pp_cliff_left.png", 132, 150, True, 31)
    pixel_cliff("pp_cliff_right.png", 144, 140, False, 32)
    pixel_waterfall()
    pixel_pool()
    pixel_hut()
    pixel_windmill()
    pixel_pine()
    pixel_dot()
    town_grass()
    town_roof()
    town_tree()
    town_pond()
    town_lamp()
    toon_blob()
    toon_eye()
    toon_rock()
    toon_bush()
    toon_ground()
    lake_sky()
    lake_mountains()
    lake_pines()
    lake_cabin()
    lake_pier()
    lake_reeds()
    lake_boat()
    harbour_town()
    harbour_boats()
    murm_birds()
    murm_sky()
    murm_clouds()
    murm_tree()
    murm_fields()
    retro_sky()
    retro_moon()
    retro_hills()
    retro_chapel()
    retro_ground()
    retro_graves()
    retro_tree()
    retro_fence()
    retro_small()
    gr_sky()
    gr_haze()
    gr_canopy()
    gr_trunk()
    gr_floor()
    gr_leaf()
    st_sky()
    st_clouds()
    st_houses()
    st_window()
    st_street()
    st_puddle()
    st_lamp()
    st_bolt()
    st_drop()
    cr_skin()
    cr_ear()
    cr_sky()
    cr_hills()
    mh_icons()
    td_ground()
    td_sprites()
    cl_wall()
    cl_block()
    cl_boulder()
    cl_brazier()
    cb_sky()
    cb_peaks()
    cb_cliff()
    cb_grass()
    cb_plank()
    cb_post()
    cb_flag()
    cb_banner()
    cb_rope()
    cb_walker()
    ad_chart()
    st31_desk()
    st31_book()
    st31_illustration()
    st31_icons()
    st31_candle()
    mm32_terrain()
    mm32_water()
    mm32_shade()
    mm32_cursor()
    hh33_ground()
    hh33_blocks()
    hh33_objects()
    hh33_villager()
    hh33_cursor()
    ci34_island()
    ci34_sky()
    ci34_sea()
    ci34_cloud()
    ci34_props()
    ns35_board()
    ns35_card()
    ns35_burst()
    rh36_grass()
    rh36_soil()
    rh36_ball()
    hb37_boats()
    hb37_water()
    hb37_pier()
    hb37_fan()
    hx38_ground()
    hx38_objects()
    hx38_marks()
    hx38_units()
    hx38_table()
    lo39_grass()
    lo39_trees()
    lo39_fence()
    lo39_scarecrow()
    lo39_props()
    lo39_lantern()
    ms40_floor()
    ms40_rug()
    ms40_furniture()
    ms40_walls()
    ms40_cookies()
    gr41_tiles()
    sb42_blocks()
    sb42_scenery()
    sb42_cannon()
    nm43_street()
    nm43_stalls()
    nm43_fountain()
    nm43_people()
    nm43_chimes()
    gp44_lawn()
    gp44_gravel()
    gp44_hedges()
    gp44_topiary()
    gp44_fountain()
    gp44_gazebo()
    gp44_rosebed()
    gp44_bench()
    gp44_table()
    gp44_gate()
    gp44_guests()
    pi45_wall()
    pi45_window()
    pi45_sign()
    pi45_chalkboard()
    pi45_parchment()
    pi45_lantern()
    pi45_counter()
    pi45_mug()
    sc46_sky()
    sc46_far_sea()
    sc46_beach()
    sc46_cliff()
    sc46_seabed()
    sc46_kelp()
    sc46_fish()
    sc46_gull()
    sc46_swimmer()
    sc46_foam()
    sc46_water()
    sc46_ray()
    ar47_sky()
    ar47_mountains()
    ar47_hills()
    ar47_edge()
    ar47_tree()
    ar47_fox()
    ar47_leaf()
    fg48_wall()
    fg48_hearth()
    fg48_anvil()
    fg48_hammer()
    fg48_bucket()
    fg48_bellows()
    fg48_tools()
    fg48_bar()
    st49_curtain()
    st49_floor()
    st49_truss()
    st49_fixture()
    st49_band()
    jf50_bell()
    jf50_tentacle()
    jf50_arm()
    jf50_blade()
    nb51_flower()
    nb51_stem()
    nb51_grass()
    nb51_hills()
    nb51_sky()
    mr52_board()
    mr52_pond()
    mr52_engine()
    mr52_carriage()
    sq53_cobbles()
    sq53_facades()
    sq53_well()
    sq53_lamp()
    sq53_tree()
    sq53_crates()
    sq53_parts()
    tl54_wall()
    tl54_floor()
    tl54_coil()
    tl54_box()
    tl54_mirror()
    tl54_hazard()
    hd55_coin()
    hd55_shield()
    hd55_sword()
    hd55_goblet()
    hd55_gem()
    hd55_stone()
    hh56_robe()
    hh56_head()
    hh56_arm()
    hh56_portrait()
    hh56_candelabra()
    ph57_seats()
    ph57_viewer()
    ph57_projector()
    ph57_beam()
    lm58_tiles()
    lm58_boulder()
    lm58_lantern()
    wm59_blades()
    wm59_flowers()
    wm59_sky()
    wm59_hills()
    wm59_cloud()
    wm59_banner()
    md60_fighters()
    md60_moon()
    md60_clouds()
    md60_cliff()
    fg61_pines()
    fg61_pond()
    px62_phoenix()
    ts63_luts()
    sg64_knights()
    pt66_theatre()
    sr67_silk()
    ml68_lander()
    kr69_kraken()
    rg70_ranger()
    qj71_journal()
    sy72_scrapyard()
    sp73_spirograph()
    bc74_baggage()
    cr75_controls()
    mz76_zoo()
    hs77_heist()
    ec78_canyon()
    pd79_dolls()
    ti80_isle()
    ss81_sand()
    cp82_chapel()
    sw83_siege_workshop()
    om84_options()
    gb85_guest_book()
    lc86_corridor()
    bp87_prompts()
    nd88_drive()
    aq89_range()
    fwn90_fireworks()
    pri91_isles()
    cy92_crane()
    gl93_gallery()
    wr94_warehouse()
    eq95_club()
    pm96_overlook()
    bz97_bazaar()
    rm98_readable()
    nw99_watch()
    ec100_cavern()
    lh101_house()
    ff102_firefight()
    sf103_snowfield()
    wf104_wildfire()
    ga105_alley()
    fd106_foundry()
    np107_pursuit()
    os108_orbital()
    mm109_mirrors()
    hs110_storm()
    ab111_arena()
    hm112_manor()
    fs113_sands()
    sc114_chamber()
    rr115_ruins()
    mm117_moor()
    ed118_dungeon()
    di120_island()
    er121_road()
    bt122_bullet()
    sp123_plaza()
    pt124_theatre()
    td125_depths()
    bh126_billiards()
    gh127_glasshouse()
    wk128_workshop()
    cb129_carnival()
    pa130_album()
    rf131_forest()
    bh132_hall()
    es133_court()
    mt134_meadow()
    sc135_caves()
    fw136_map()
    vc137_village()
    ls138_satchel()
    ts139_town()
    kc140_track()
    pp141_bay()
    bw142_manor()
    ct143_cards()
    de144_rooftops()
    oi145_harbour()
    cc146_theatre()
    fs147_fruit()
    pl148_chamber()
    jb149_kitchen()
    br150_moor()
    sk151_sketch()
    sw152_wheel()
    rw153_city()
    tt154_market()
    us155_field()
    th156_storm()
    mr157_mill()
    fs158_village()
    rr159_city()
    ss160_stage()
    tr161_room()
    pb162_table()
