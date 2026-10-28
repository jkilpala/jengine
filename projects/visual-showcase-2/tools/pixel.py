"""Pixel-art helpers for Visual Showcase 2: palettes, drawing into small
numpy canvases, outlines, height fields to normal maps, and the upscale that
lets scenery be drawn small and still be filtered smoothly in perspective.

HD-2D's scenery is pixel art too - what makes it "HD" is the light, the
depth and the lens. So every surface here is drawn at its art resolution
(a few dozen texels a tile) and then written **four times larger, nearest**:
the project filters scenery linearly with mipmaps, which on a 4x texture is
crisp up close and smooth where the floor recedes, where nearest sampling
would shimmer. Characters stay at their art size and are drawn nearest.
"""

import os

import numpy as np
from PIL import Image

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")


def rgb(h):
    """'#rrggbb' -> (r, g, b) floats 0..1."""
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) / 255.0 for i in (0, 2, 4))


def ramp(*hexes):
    """A palette ramp, dark to light, as an (n, 3) array."""
    return np.array([rgb(h) for h in hexes], dtype=np.float32)


def rng(seed):
    return np.random.default_rng(seed)


def value_noise(w, h, cell, seed, octaves=3):
    """Tileable value noise in 0..1, `cell` texels a lattice step."""
    r = rng(seed)
    out = np.zeros((h, w), np.float32)
    amp, total = 1.0, 0.0
    for o in range(octaves):
        c = max(1, cell >> o)
        gw, gh = max(1, w // c), max(1, h // c)
        grid = r.random((gh, gw)).astype(np.float32)
        ys = np.arange(h) / c
        xs = np.arange(w) / c
        y0 = np.floor(ys).astype(int) % gh
        x0 = np.floor(xs).astype(int) % gw
        y1, x1 = (y0 + 1) % gh, (x0 + 1) % gw
        fy = (ys - np.floor(ys))[:, None]
        fx = (xs - np.floor(xs))[None, :]
        fy = fy * fy * (3 - 2 * fy)
        fx = fx * fx * (3 - 2 * fx)
        a = grid[y0][:, x0]
        b = grid[y0][:, x1]
        c_ = grid[y1][:, x0]
        d = grid[y1][:, x1]
        out += amp * (a * (1 - fx) * (1 - fy) + b * fx * (1 - fy) + c_ * (1 - fx) * fy + d * fx * fy)
        total += amp
        amp *= 0.5
    return out / total


BAYER = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]], np.float32) / 16.0


def shade(values, palette, dither=0.5):
    """Map 0..1 values onto a palette ramp with ordered dithering: the
    banded, hand-placed look of pixel shading rather than a smooth blend."""
    h, w = values.shape
    n = len(palette)
    b = np.tile(BAYER, (h // 4 + 1, w // 4 + 1))[:h, :w]
    idx = np.clip(np.floor(values * (n - 1) + 0.5 + (b - 0.5) * dither), 0, n - 1).astype(int)
    return palette[idx]


class Canvas:
    """An RGBA canvas at art resolution."""

    def __init__(self, w, h):
        self.w, self.h = w, h
        self.rgba = np.zeros((h, w, 4), np.float32)
        # Height for the normal map: 0 low, 1 high.
        self.height = np.zeros((h, w), np.float32)

    def put(self, x, y, color, a=1.0, height=None):
        x, y = int(x), int(y)
        if 0 <= x < self.w and 0 <= y < self.h:
            self.rgba[y, x, :3] = color
            self.rgba[y, x, 3] = a
            if height is not None:
                self.height[y, x] = height

    def rect(self, x0, y0, x1, y1, color, a=1.0, height=None):
        x0, y0 = max(0, int(x0)), max(0, int(y0))
        x1, y1 = min(self.w, int(x1)), min(self.h, int(y1))
        if x1 <= x0 or y1 <= y0:
            return
        self.rgba[y0:y1, x0:x1, :3] = color
        self.rgba[y0:y1, x0:x1, 3] = a
        if height is not None:
            self.height[y0:y1, x0:x1] = height

    def ellipse(self, cx, cy, rx, ry, color, a=1.0, height=None):
        ys, xs = np.mgrid[0:self.h, 0:self.w]
        m = ((xs + 0.5 - cx) / max(rx, 0.01)) ** 2 + ((ys + 0.5 - cy) / max(ry, 0.01)) ** 2 <= 1.0
        self.rgba[m, :3] = color
        self.rgba[m, 3] = a
        if height is not None:
            self.height[m] = height
        return m

    def outline(self, color, inside=False):
        """A one-texel outline round everything opaque: outside it (a new
        ring of texels) or on its own edge texels (`inside`)."""
        solid = self.rgba[:, :, 3] > 0.5
        grown = np.zeros_like(solid)
        for dy, dx in ((-1, 0), (1, 0), (0, -1), (0, 1)):
            grown |= np.roll(np.roll(solid, dy, 0), dx, 1)
        if inside:
            edge = solid & ~(
                np.roll(solid, 1, 0) & np.roll(solid, -1, 0) & np.roll(solid, 1, 1) & np.roll(solid, -1, 1)
            )
        else:
            edge = grown & ~solid
        self.rgba[edge, :3] = color
        self.rgba[edge, 3] = 1.0

    def image(self):
        a = np.clip(self.rgba, 0, 1)
        return Image.fromarray((a * 255 + 0.5).astype(np.uint8), "RGBA")


def normal_map(height, strength=2.0):
    """A tangent-space normal map (+y up the image) from a height field,
    by central differences - wrapping, so a tile's edges agree."""
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * 0.5
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * 0.5
    # Image y runs down; the normal's y is up.
    n = np.stack([-dx * strength, dy * strength, np.ones_like(height)], -1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return n * 0.5 + 0.5


def save(img, rel, scale=1):
    """Write an image under assets/, `scale` times larger, nearest."""
    path = os.path.join(ROOT, rel)
    os.makedirs(os.path.dirname(path), exist_ok=True)
    if scale != 1:
        img = img.resize((img.width * scale, img.height * scale), Image.NEAREST)
    img.save(path, optimize=True)
    return rel.replace("\\", "/")


def save_surface(canvas, rel, scale=4, strength=2.0):
    """A surface and its normal map (`<name>_n.png`), both `scale` times
    larger. Returns the two asset paths."""
    tex = save(canvas.image(), rel, scale)
    n = normal_map(canvas.height, strength)
    nimg = Image.fromarray((np.dstack([n, np.ones(canvas.height.shape)]) * 255 + 0.5).astype(np.uint8), "RGBA")
    nrel = rel[:-4] + "_n.png"
    save(nimg, nrel, scale)
    return tex, nrel


def soft_disc(size, power=2.0, color=(1, 1, 1)):
    """A white disc fading to clear: glows, blob shadows."""
    ys, xs = np.mgrid[0:size, 0:size]
    r = np.hypot(xs + 0.5 - size / 2, ys + 0.5 - size / 2) / (size / 2)
    a = np.clip(1 - r, 0, 1) ** power
    img = np.zeros((size, size, 4), np.float32)
    img[:, :, :3] = color
    img[:, :, 3] = a
    return Image.fromarray((img * 255 + 0.5).astype(np.uint8), "RGBA")


def gradient(w, h, stops):
    """A vertical gradient, top to bottom, through `(t, '#hex')` stops."""
    ts = np.array([s[0] for s in stops])
    cols = np.array([rgb(s[1]) for s in stops])
    y = np.linspace(0, 1, h)
    out = np.zeros((h, w, 4), np.float32)
    for c in range(3):
        out[:, :, c] = np.interp(y, ts, cols[:, c])[:, None]
    out[:, :, 3] = 1
    return Image.fromarray((out * 255 + 0.5).astype(np.uint8), "RGBA")
