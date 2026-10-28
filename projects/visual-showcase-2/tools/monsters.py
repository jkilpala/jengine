"""Monsters for Visual Showcase 2's battle: bigger than the cast, facing
left toward the party, four frames each - two idle, an attack lunge and a
hurt flinch."""

import numpy as np

from chars import selective_outline, shaded_ellipse, tones
from pixel import Canvas, rgb

FRAMES = ["idle0", "idle1", "attack", "hurt"]


def _sheet(draw, w, h):
    out = Canvas(w * len(FRAMES), h)
    for i, f in enumerate(FRAMES):
        c = draw(f)
        out.rgba[:, i * w:(i + 1) * w] = c.rgba
    return out


def frog():
    """A big swamp toad with a lantern-bright throat."""
    w, h = 56, 48

    def draw(f):
        c = Canvas(w, h)
        bob = 1 if f == "idle1" else 0
        lunge = -5 if f == "attack" else (3 if f == "hurt" else 0)
        cx, base = 28 + lunge, 46
        body = "#4a7a3a" if f != "hurt" else "#c8e0a0"
        # Back legs, body, head with two big eyes.
        shaded_ellipse(c, cx + 9, base - 7, 9, 7, body)
        shaded_ellipse(c, cx, base - 15 + bob, 18, 13 - bob, body)
        s, b, l = tones(body)
        ys, xs = np.mgrid[0:h, 0:w]
        belly = ((xs + 0.5 - (cx - 6)) / 10) ** 2 + ((ys + 0.5 - (base - 10 + bob)) / 7) ** 2 <= 1
        c.rgba[belly, :3] = np.array(rgb("#e8d890"))
        for ex in (cx - 11, cx + 1):
            shaded_ellipse(c, ex, base - 27 + bob, 5, 5, body)
            c.ellipse(ex - 1, base - 28 + bob, 2.6, 2.8, np.array(rgb("#f4e060")))
            c.rect(ex - 2, base - 29 + bob, ex - 1, base - 26 + bob, np.array(rgb("#1a1010")))
        c.rect(cx - 16, base - 17 + bob, cx - 2, base - 16 + bob, s)
        # Spots.
        for sx, sy in ((cx + 6, base - 20), (cx + 12, base - 14), (cx + 2, base - 24)):
            c.ellipse(sx, sy + bob, 1.8, 1.4, s)
        # Front feet.
        c.rect(cx - 14, base - 3, cx - 7, base, s)
        if f == "attack":
            # The tongue lashes out.
            c.rect(2, base - 17, cx - 15, base - 15, np.array(rgb("#e06080")))
            c.ellipse(3, base - 16, 2.5, 2.5, np.array(rgb("#f08aa0")))
        selective_outline(c)
        return c

    return _sheet(draw, w, h)


def wraith():
    """A hooded ghost with a scythe, its robe tattered into wisps."""
    w, h = 48, 64

    def draw(f):
        c = Canvas(w, h)
        bob = {"idle0": 0, "idle1": 2, "attack": -1, "hurt": 1}[f]
        lunge = -6 if f == "attack" else (3 if f == "hurt" else 0)
        cx = 26 + lunge
        robe = "#3a3458" if f != "hurt" else "#c8c0f0"
        s, b, l = tones(robe)
        for y in range(10 + bob, 58):
            t = (y - 10 - bob) / 48
            half = 6 + t * 10
            for x in range(int(cx - half), int(cx + half)):
                if y > 50 and (x + y) % 4 == 0:
                    continue
                side = (x - cx) / half
                c.put(x, y, l if side < -0.6 else (s if side > 0.4 else b), a=1.0 if y < 52 else 0.7)
        # The hood and the dark inside it, with two eyes.
        shaded_ellipse(c, cx, 14 + bob, 9, 9, robe)
        c.ellipse(cx - 2, 16 + bob, 5.5, 6, np.array([0.03, 0.02, 0.06]))
        for ex in (cx - 4, cx + 0):
            c.put(ex, 16 + bob, np.array(rgb("#80f0ff")))
        # A scythe held out front.
        swing = -10 if f == "attack" else 0
        c.rect(cx - 12 + swing, 12 + bob, cx - 10 + swing, 52, np.array(rgb("#5a4030")))
        for i in range(12):
            c.put(cx - 11 + swing + i, 12 + bob + i // 3, np.array(rgb("#c8d0e0")))
            c.put(cx - 11 + swing + i, 13 + bob + i // 3, np.array(rgb("#8890a0")))
        selective_outline(c)
        return c

    return _sheet(draw, w, h)


def golem():
    """A stone golem with moss on its shoulders and a glowing core."""
    w, h = 64, 72

    def draw(f):
        c = Canvas(w, h)
        bob = 1 if f == "idle1" else 0
        lunge = -6 if f == "attack" else (3 if f == "hurt" else 0)
        cx, base = 34 + lunge, 70
        stone = "#6a6470" if f != "hurt" else "#d8d0e0"
        moss = "#4a6a32"
        # Legs, torso, shoulders, head, arms.
        for lx in (cx - 9, cx + 7):
            shaded_ellipse(c, lx, base - 8, 6, 9, stone)
        shaded_ellipse(c, cx, base - 30 + bob, 17, 16, stone)
        for sx in (cx - 18, cx + 16):
            shaded_ellipse(c, sx, base - 40 + bob, 8, 7, stone)
            c.ellipse(sx, base - 45 + bob, 6, 2.5, np.array(rgb(moss)))
        shaded_ellipse(c, cx - 2, base - 52 + bob, 8, 7, stone)
        c.rect(cx - 7, base - 53 + bob, cx - 4, base - 51 + bob, np.array(rgb("#ffb040")))
        c.rect(cx + 0, base - 53 + bob, cx + 3, base - 51 + bob, np.array(rgb("#ffb040")))
        arm_y = base - 26 + bob + (-10 if f == "attack" else 0)
        shaded_ellipse(c, cx - 22 + (-6 if f == "attack" else 0), arm_y, 6, 12, stone)
        shaded_ellipse(c, cx + 20, base - 26 + bob, 6, 12, stone)
        # Cracks and the core.
        c.ellipse(cx, base - 30 + bob, 4, 4, np.array(rgb("#ff9a30")))
        c.ellipse(cx, base - 30 + bob, 2, 2, np.array(rgb("#fff0b0")))
        for i in range(6):
            c.put(cx + 6 + i, base - 34 + bob + i // 2, tones(stone)[0])
        selective_outline(c)
        return c

    return _sheet(draw, w, h)


ALL = {"frog": frog, "wraith": wraith, "golem": golem}
