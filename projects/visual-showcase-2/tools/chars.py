"""Pixel-art characters for Visual Showcase 2, drawn part by part.

A character is a 32 x 48 frame, facing right in three-quarter view, about
three heads tall - the HD-2D proportion. Each part (cape, back arm, legs,
body, front arm, head, hair, hat, held thing) is drawn in a three-tone ramp
lit from the upper left, then the whole figure gets a **selective outline**:
each outline texel is a darkened copy of the part it borders, not one black,
which is what keeps a small sprite from looking stamped out.

A sheet is six frames left to right: two idle (breathing), four walking.
"""

import numpy as np

from pixel import Canvas, rgb

W, H = 32, 48
FRAMES = ["idle0", "idle1", "walk0", "walk1", "walk2", "walk3"]


def tones(hex_base):
    """Shadow, base and light of one colour."""
    b = np.array(rgb(hex_base), np.float32)
    shadow = b * np.array([0.62, 0.58, 0.72])
    light = np.minimum(1.0, b * 1.18 + 0.06)
    return shadow, b, light


def shaded_rect(c, x0, y0, x1, y1, base, light_left=True):
    """A block shaded as a lit cylinder: light down its left edge, shadow
    down its right and along its bottom."""
    s, b, l = tones(base)
    c.rect(x0, y0, x1, y1, b)
    if x1 - x0 >= 3:
        c.rect(x0, y0, x0 + 1, y1, l if light_left else s)
        c.rect(x1 - 1, y0, x1, y1, s if light_left else l)
    c.rect(x0, y1 - 1, x1, y1, s)


def shaded_ellipse(c, cx, cy, rx, ry, base):
    s, b, l = tones(base)
    m = c.ellipse(cx, cy, rx, ry, b)
    ys, xs = np.mgrid[0:c.h, 0:c.w]
    dx, dy = (xs + 0.5 - cx) / rx, (ys + 0.5 - cy) / ry
    c.rgba[m & (dx + dy > 0.55), :3] = s
    c.rgba[m & (dx + dy < -0.75), :3] = l


def selective_outline(c):
    """Outline what is opaque with darkened copies of its own edge colour."""
    solid = c.rgba[:, :, 3] > 0.5
    out = c.rgba.copy()
    for y in range(c.h):
        for x in range(c.w):
            if solid[y, x]:
                continue
            best = None
            for dy, dx in ((0, -1), (0, 1), (-1, 0), (1, 0)):
                yy, xx = y + dy, x + dx
                if 0 <= yy < c.h and 0 <= xx < c.w and solid[yy, xx]:
                    best = c.rgba[yy, xx, :3]
                    break
            if best is not None:
                out[y, x, :3] = best * np.array([0.28, 0.22, 0.34]) + np.array([0.03, 0.02, 0.05])
                out[y, x, 3] = 1.0
    c.rgba = out


def walk_pose(frame):
    """(bob, front leg dx, back leg dx, front leg lift, back lift, arm swing)."""
    if frame == "idle0":
        return 0, 1, -1, 0, 0, 0
    if frame == "idle1":
        return 1, 1, -1, 0, 0, 0
    i = int(frame[-1])
    return [
        (0, 3, -3, 0, 0, 2),
        (-1, 0, 0, 0, 2, 0),
        (0, -3, 3, 0, 0, -2),
        (-1, 0, 0, 2, 0, 0),
    ][i]


def draw(spec, frame):
    """One frame of a character. `spec` is a dict of colours and choices."""
    c = Canvas(W, H)
    bob, fdx, bdx, flift, blift, swing = walk_pose(frame)
    by = 2 + bob  # everything above the hips moves with the bob
    hip_y = 30 + by
    foot = 46
    cx = 15
    robe = spec.get("robe")

    # Cape, behind everything.
    if spec.get("cape"):
        flare = 1 if frame.startswith("walk") and frame[-1] in "02" else 0
        s, b, l = tones(spec["cape"])
        for y in range(17 + by, 39 + by):
            t = (y - 17 - by) / 22
            x0 = int(cx - 7 - t * (3 + flare))
            x1 = int(cx + 1)
            c.rect(x0, y, x1, y + 1, b)
            c.put(x0, y, s)
        c.rect(cx - 9 - flare, 37 + by, cx + 1, 39 + by, s)

    def leg(dx, lift, base, boot):
        x = cx - 2 + dx
        top = hip_y
        bottom = foot - lift
        shaded_rect(c, x, top, x + 4, bottom - 3, base)
        shaded_rect(c, x, bottom - 4, x + 5, bottom, boot)

    if not robe:
        # Back leg, a little darker for depth.
        leg(bdx - 1, blift, spec.get("pants_back", spec["pants"]), spec["boots"])

    # Back arm.
    s, b, l = tones(spec.get("sleeve", spec["tunic"]))
    c.rect(cx - 5 - swing // 2, 19 + by, cx - 2 - swing // 2, 28 + by, s)

    if not robe:
        leg(fdx, flift, spec["pants"], spec["boots"])

    # Body: a tunic, or a robe down to the feet.
    if robe:
        sway = 1 if frame.startswith("walk") and frame[-1] in "13" else 0
        s, b, l = tones(robe)
        for y in range(17 + by, foot - 1):
            t = (y - 17 - by) / max(1, foot - 18 - by)
            x0 = int(cx - 5 - t * 3) + sway
            x1 = int(cx + 6 + t * 3) + sway
            c.rect(x0, y, x1, y + 1, b)
            c.put(x0, y, l)
            c.put(x1 - 1, y, s)
            c.put(x1 - 2, y, s)
        c.rect(cx - 7 + sway, foot - 2, cx + 9 + sway, foot - 1, s)
        # Feet peeking out.
        shaded_rect(c, cx - 3 + fdx // 2, foot - 2, cx + 2 + fdx // 2, foot, spec["boots"])
        if spec.get("trim"):
            c.rect(cx - 1, 18 + by, cx + 1, foot - 2, tones(spec["trim"])[1])
    else:
        shaded_rect(c, cx - 5, 17 + by, cx + 6, hip_y + 2, spec["tunic"])
        # The tunic's skirt flares a little over the hips.
        c.rect(cx - 6, hip_y - 1, cx + 7, hip_y + 2, tones(spec["tunic"])[0])
        c.rect(cx - 5, hip_y - 1, cx + 6, hip_y + 1, tones(spec["tunic"])[1])
        if spec.get("belt"):
            c.rect(cx - 5, hip_y - 4, cx + 6, hip_y - 2, tones(spec["belt"])[0])
            c.put(cx + 2, hip_y - 4, rgb("#e8c060"))
        if spec.get("armor"):
            shaded_rect(c, cx - 5, 17 + by, cx + 6, 24 + by, spec["armor"])
            c.put(cx - 1, 19 + by, tones(spec["armor"])[2])

    # Scarf / collar.
    if spec.get("scarf"):
        s, b, l = tones(spec["scarf"])
        c.rect(cx - 5, 16 + by, cx + 6, 19 + by, b)
        c.rect(cx - 5, 18 + by, cx + 6, 19 + by, s)
        if spec.get("cape") is None:
            c.rect(cx - 7, 18 + by, cx - 4, 23 + by, s)

    # Head: skin, an eye looking right, a little blush.
    head_cy = 11 + by
    shaded_ellipse(c, cx + 1, head_cy, 6.6, 6.2, spec["skin"])

    def face():
        # Three-quarter view: the near eye big, the far one a dot.
        for x, top in ((cx + 4, 1), (cx + 1, 0)):
            c.put(x, head_cy, rgb("#1a1424"))
            if top:
                c.put(x, head_cy - 1, rgb("#1a1424"))
                c.put(x - 1, head_cy - 1, rgb("#3a2a44"))
        c.put(cx + 5, head_cy + 3, tones(spec["skin"])[0])
        c.put(cx + 3, head_cy + 2, rgb("#e89a88"))
        c.put(cx + 0, head_cy + 2, rgb("#e8a090"))

    face()

    # Hair over the top and back of the head.
    hair = spec.get("hair")
    if hair:
        s, b, l = tones(hair)
        style = spec.get("hair_style", "short")
        ys, xs = np.mgrid[0:H, 0:W]
        m = ((xs + 0.5 - cx) / 6.5) ** 2 + ((ys + 0.5 - (head_cy - 2)) / 5.0) ** 2 <= 1.0
        # Keep the face clear: nothing in front of the eye line below the fringe.
        m &= ~((xs > cx - 2) & (ys > head_cy - 3))
        c.rgba[m, :3] = b
        c.rgba[m, 3] = 1
        c.rgba[m & (xs > cx - 2), :3] = l
        c.rect(cx - 6, head_cy - 3, cx - 3, head_cy + 4, s)
        if style == "long":
            c.rect(cx - 6, head_cy, cx - 2, head_cy + 12, b)
            c.rect(cx - 6, head_cy, cx - 5, head_cy + 12, s)
        if style == "tail":
            c.rect(cx - 9, head_cy - 1, cx - 5, head_cy + 2, b)
            c.rect(cx - 10, head_cy + 1, cx - 7, head_cy + 7, s)
        if style == "spiky":
            for dx in (-4, -1, 2):
                c.put(cx + dx, head_cy - 8, b)
                c.put(cx + dx + 1, head_cy - 9, l)

    hat = spec.get("hat")
    if hat == "hood":
        s, b, l = tones(spec["hat_color"])
        m = c.ellipse(cx, head_cy - 1, 7.5, 7.0, b)
        ys, xs = np.mgrid[0:H, 0:W]
        opening = m & (xs > cx - 2) & (ys > head_cy - 4) & (ys < head_cy + 5)
        # The face inside the hood's opening.
        c.rgba[opening, :3] = tones(spec["skin"])[1]
        c.rgba[opening, 3] = 1
        face()
        c.rect(cx - 7, head_cy - 2, cx - 4, head_cy + 6, s)
    elif hat == "wide":
        s, b, l = tones(spec["hat_color"])
        c.rect(cx - 9, head_cy - 5, cx + 11, head_cy - 3, b)
        c.rect(cx - 9, head_cy - 4, cx + 11, head_cy - 3, s)
        c.rect(cx - 4, head_cy - 10, cx + 5, head_cy - 4, b)
        c.rect(cx - 4, head_cy - 10, cx - 3, head_cy - 4, l)
        c.rect(cx - 4, head_cy - 6, cx + 5, head_cy - 5, tones(spec.get("band", "#8a2a2a"))[1])
    elif hat == "helm":
        shaded_ellipse(c, cx, head_cy - 2, 6.8, 5.2, spec["hat_color"])
        c.rect(cx + 2, head_cy - 2, cx + 8, head_cy, tones(spec["hat_color"])[0])
        c.put(cx - 1, head_cy - 7, rgb("#f0e8d0"))
    elif hat == "cap":
        s, b, l = tones(spec["hat_color"])
        c.rect(cx - 6, head_cy - 7, cx + 5, head_cy - 3, b)
        c.rect(cx + 3, head_cy - 4, cx + 9, head_cy - 3, s)

    # Front arm and what it holds.
    s, b, l = tones(spec.get("sleeve", spec["tunic"]))
    ax = cx + 1 + swing
    c.rect(ax, 19 + by, ax + 3, 27 + by, b)
    c.rect(ax + 2, 19 + by, ax + 3, 27 + by, s)
    hand = tones(spec["skin"])[1]
    c.rect(ax, 27 + by, ax + 3, 29 + by, hand)
    held = spec.get("held")
    if held == "sword":
        # A sheathed sword at the hip, hilt forward.
        c.rect(cx - 8, hip_y - 2, cx - 2, hip_y - 1, rgb("#4a3a3a"))
        c.rect(cx - 2, hip_y - 4, cx - 1, hip_y + 1, rgb("#d8b048"))
        c.rect(cx - 1, hip_y - 3, cx + 2, hip_y - 2, rgb("#6a4a30"))
    elif held == "staff":
        c.rect(ax + 1, 6 + by, ax + 2, foot - 1, rgb("#7a5230"))
        c.rect(ax + 1, 6 + by, ax + 2, foot - 1 - 30, rgb("#9a6a40"))
        c.ellipse(ax + 1.5, 5 + by, 2.4, 2.4, rgb("#9ae0ff"))
        c.put(ax + 1, 4 + by, rgb("#ffffff"))
    elif held == "book":
        c.rect(ax - 1, 24 + by, ax + 4, 29 + by, rgb("#6a2a40"))
        c.rect(ax - 1, 24 + by, ax + 4, 25 + by, rgb("#e8d8b0"))
    elif held == "lantern":
        c.rect(ax + 1, 29 + by, ax + 2, 31 + by, rgb("#3a3030"))
        c.rect(ax - 1, 31 + by, ax + 4, 35 + by, rgb("#ffd070"))
        c.rect(ax - 1, 31 + by, ax + 4, 32 + by, rgb("#4a3a30"))
    elif held == "spear":
        c.rect(ax + 1, 3 + by, ax + 2, foot - 2, rgb("#6a4a30"))
        c.rect(ax, 0 + by, ax + 3, 4 + by, rgb("#c8d0d8"))
    elif held == "basket":
        c.rect(ax - 2, 28 + by, ax + 5, 32 + by, rgb("#a8803a"))
        c.rect(ax - 2, 28 + by, ax + 5, 29 + by, rgb("#d84a3a"))

    selective_outline(c)
    return c


def sheet(spec):
    """Six frames side by side, 192 x 48."""
    frames = [draw(spec, f) for f in FRAMES]
    out = Canvas(W * len(frames), H)
    for i, f in enumerate(frames):
        out.rgba[:, i * W:(i + 1) * W] = f.rgba
    return out


CAST = {
    "traveler": dict(skin="#f0c8a0", hair="#4a3020", hair_style="short", tunic="#3a5a9a", pants="#4a3a30",
                     pants_back="#3a2e28", boots="#5a3a24", cape="#9a2a2a", belt="#6a4a2a", scarf="#d8b048",
                     held="sword"),
    "cleric": dict(skin="#f4d4b8", hair="#e8d090", hair_style="long", robe="#e8e4dc", trim="#4a6ab0",
                   tunic="#e8e4dc", pants="#e8e4dc", boots="#8a6a4a", held="staff"),
    "merchant": dict(skin="#d8a880", hair="#2a2020", tunic="#4a7a3a", pants="#6a5040", boots="#4a3020",
                     belt="#8a5a2a", hat="wide", hat_color="#a87a3a", band="#2a4a7a", held="basket"),
    "scholar": dict(skin="#f0c8a0", hair="#8a3a2a", hair_style="tail", robe="#6a3a7a", trim="#d8b048",
                    tunic="#6a3a7a", pants="#6a3a7a", boots="#3a2a2a", held="book"),
    "hunter": dict(skin="#c89070", hair="#1a1a1a", hair_style="spiky", tunic="#5a6a3a", pants="#4a4030",
                   boots="#3a2a1a", hat="hood", hat_color="#3a4a2a", belt="#5a3a20", held="spear"),
    "guard": dict(skin="#e8b890", hair="#5a4030", tunic="#7a2a2a", pants="#3a3a4a", boots="#2a2a30",
                  armor="#8a9aa8", hat="helm", hat_color="#9aa8b4", held="spear"),
    "villager": dict(skin="#e8c0a0", hair="#a86a30", hair_style="long", robe="#c86a4a", tunic="#c86a4a",
                     pants="#c86a4a", boots="#5a3a2a", scarf="#e8d8b8", held="basket"),
    "farmer": dict(skin="#d09870", hair="#5a4030", tunic="#a89060", pants="#5a5a7a", boots="#4a3020",
                   hat="cap", hat_color="#7a5a3a", belt="#5a3a20"),
    "lamplighter": dict(skin="#e0b090", hair="#9a9a9a", tunic="#2a3a5a", pants="#2a2a3a", boots="#1a1a20",
                        hat="cap", hat_color="#1a2a4a", scarf="#9a2a2a", held="lantern"),
    "child": dict(skin="#f4d0b0", hair="#d89040", hair_style="short", tunic="#d8a040", pants="#4a6a9a",
                  boots="#6a4a2a"),
}
