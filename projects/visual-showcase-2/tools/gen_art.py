"""Write every texture Visual Showcase 2 uses into assets/sprites/hd/.

    python tools/gen_art.py

- `chars/`   pixel characters and monsters, one sheet each, drawn nearest
- `scenery/` surfaces with normal maps and standing props, written 4x
             larger and filtered linearly with mipmaps (see `pixel.py`)
- `fx/`      glows, flames, motes, blob shadows, light shafts
- `sky/`     backdrops
"""

import numpy as np
from PIL import Image

import chars
import monsters
import props
import surfaces as S
from pixel import Canvas, gradient, ramp, rng, save, save_surface, soft_disc, value_noise

SC = "sprites/hd/scenery/"
FX = "sprites/hd/fx/"
SKY = "sprites/hd/sky/"
CH = "sprites/hd/chars/"


def gable(walls, w=64, h=32):
    """A gable end: the wall's own picture, cut to a triangle, with a
    bargeboard along its two slopes."""
    c = Canvas(w, h)
    tile = walls
    for y in range(h):
        for x in range(w):
            c.rgba[y, x] = tile.rgba[y % tile.h, x % tile.w]
            c.height[y, x] = tile.height[y % tile.h, x % tile.w]
    ys, xs = np.mgrid[0:h, 0:w]
    inside = ys >= (h - 1) * (1 - np.minimum(xs + 0.5, w - xs - 0.5) / (w / 2))
    c.rgba[~inside, 3] = 0
    edge = inside & (ys < (h - 1) * (1 - np.minimum(xs + 0.5, w - xs - 0.5) / (w / 2)) + 3)
    wood = ramp("#2a1a14", "#4a3020")
    c.rgba[edge, :3] = wood[1]
    c.height[edge] = 1.0
    # A little round window in the gable.
    c.ellipse(w / 2, h * 0.62, 3.5, 3.5, np.array([0.1, 0.1, 0.16]), height=0.1)
    return c


def flame_sheet():
    """Four frames of a pixel flame, white-hot core to orange, for an
    additive card; tinted by its sprite colour."""
    w, h, n = 12, 20, 4
    out = Canvas(w * n, h)
    r = rng(5)
    for f in range(n):
        for y in range(h):
            t = y / h  # 0 top, 1 bottom
            sway = np.sin(f * 1.7 + y * 0.6) * (1 - t) * 1.6
            half = (1 - (1 - t) ** 1.6) * 4.6 * (0.85 + 0.15 * np.sin(f * 2.1 + y))
            for x in range(w):
                d = abs(x + 0.5 - (w / 2 + sway)) / max(half, 0.3)
                if d > 1 or t < 0.08:
                    continue
                heat = (1 - d) * (0.4 + 0.6 * t)
                col = np.array([1.0, 0.55 + 0.45 * heat, 0.2 + 0.8 * heat ** 2])
                out.put(f * w + x, y, col, a=min(1.0, 0.5 + heat))
        _ = r
    return out


def main():
    # -- characters -------------------------------------------------------
    for name, spec in chars.CAST.items():
        save(chars.sheet(spec).image(), CH + name + ".png")
    for name, sheet in monsters.ALL.items():
        save(sheet().image(), CH + name + ".png")

    # -- scenery surfaces (4x, with normal maps) ---------------------------
    surf = {
        "cobbles": S.cobbles(),
        "cobbles_warm": S.cobbles(seed=31, palette=ramp("#2e2228", "#4e3e3e", "#6e5a50", "#8e7662", "#ae9478", "#c8b090")),
        "grass": S.grass(),
        "grass_dark": S.grass(seed=32, flowers=False, palette=ramp("#0e1e1c", "#163028", "#1e4430", "#2a5a36", "#3a703c", "#4e8444")),
        "dirt": S.dirt(),
        "plaster": S.plaster(),
        "timber": S.timber(),
        "timber_blue": S.timber(seed=33, plaster_pal=ramp("#5a6878", "#72849a", "#8ca0b4", "#a8bacb", "#c4d2de")),
        "bricks": S.bricks(),
        "bricks_dark": S.bricks(seed=34, palette=ramp("#1e1a22", "#2e2832", "#403844", "#544a56", "#6a5e6a")),
        "roof_red": S.roof(),
        "roof_blue": S.roof(seed=35, palette=ramp("#141c2e", "#1e2a44", "#2a3a5c", "#3a4e74", "#52688c")),
        "roof_snow": S.roof(seed=36, palette=ramp("#7a8aa8", "#a2b2cc", "#c4d0e2", "#e0e8f4", "#f6f9fe")),
        "planks": S.planks(),
        "planks_dark": S.planks(seed=37, palette=ramp("#1e140e", "#2e2016", "#40301e", "#544028", "#6a5236")),
        "planks_wall": S.planks(seed=38, vertical=True),
        "rock": S.rock(),
        "rock_warm": S.rock(seed=39, palette=ramp("#3a2c26", "#5a4636", "#7a604a", "#987a5e", "#b49474", "#ccae8c")),
        "snow": S.snow(),
        "water": S.water(),
        "moss_stone": S.bricks(seed=40, rows=4, palette=ramp("#1e2a22", "#2e3e30", "#44543e", "#5e6c50", "#7a8666")),
    }
    for name, c in surf.items():
        save_surface(c, SC + name + ".png", scale=4, strength=2.5 if "grass" not in name else 1.5)
    save_surface(gable(surf["timber"]), SC + "gable_timber.png", scale=4)
    save_surface(gable(surf["timber_blue"]), SC + "gable_blue.png", scale=4)
    save_surface(gable(surf["bricks"]), SC + "gable_bricks.png", scale=4)
    save_surface(gable(surf["planks_wall"]), SC + "gable_planks.png", scale=4)
    save_surface(S.window(), SC + "window.png", scale=4)
    save(S.window(lit=True).image(), SC + "window_glow.png", 4)
    save_surface(S.door(), SC + "door.png", scale=4)

    # -- props: standing cards (4x, linear) --------------------------------
    cards = {
        "oak": props.oak(), "oak_autumn": props.oak(seed=41, palette=ramp("#3a1a10", "#6a2a14", "#9a4418", "#c4661e", "#e0902a", "#f0b850")),
        "pine": props.pine(), "pine_snow": props.pine(seed=42, snow=True), "bush": props.bush(),
        "bush_berries": props.bush(seed=43, berries=True), "lamp_post": props.lamp_post(), "barrel": props.barrel(),
        "crate": props.crate(), "crystal": props.crystal(), "crystal_pink": props.crystal(
            seed=44, color=("#3a1030", "#6a2050", "#a83a80", "#e070b0", "#ffd0ec")),
        "stalagmite": props.stalagmite(), "torch_bracket": props.torch_bracket(),
        "shrine_lantern": props.shrine_lantern(), "fence": props.fence(), "signpost": props.signpost(),
        "table": props.table(), "mug": props.mug(), "grass_tuft": props.grass_tuft(),
        "grass_tuft_dry": props.grass_tuft(seed=45, palette=ramp("#2a2414", "#4a3e1e", "#6e5a2a", "#94783a", "#b89a4e", "#d4bc6c")),
        "banner": props.banner(), "banner_blue": props.banner(
            seed=46, cloth=("#0c1430", "#14245a", "#20408a", "#3460b4"), trim="#c8ccd8"),
    }
    for name, c in cards.items():
        save_surface(c, SC + name + ".png", scale=4, strength=3.0)
    save(props.lamp_post(lit=True).image(), SC + "lamp_post_glow.png", 4)

    # -- fx --------------------------------------------------------------
    save(soft_disc(64, 2.0), FX + "glow.png")
    save(soft_disc(64, 1.2), FX + "blob.png")
    save(soft_disc(16, 1.5), FX + "mote.png")
    save(soft_disc(128, 3.0), FX + "glow_wide.png")
    save(flame_sheet().image(), FX + "flame.png", 2)
    # A light shaft: a soft vertical band fading at both ends and its sides.
    ys, xs = np.mgrid[0:128, 0:32]
    a = np.sin(np.pi * (xs + 0.5) / 32) ** 2 * np.clip((ys + 0.5) / 128 * 1.4, 0, 1) * np.clip((128 - ys) / 40, 0, 1)
    shaft = np.zeros((128, 32, 4), np.float32)
    shaft[..., :3] = 1
    shaft[..., 3] = a
    save(Image.fromarray((shaft * 255).astype(np.uint8), "RGBA"), FX + "shaft.png")
    # A snowflake and a leaf: tiny pixel shapes for particles.
    flake = Canvas(5, 5)
    for x, y in ((2, 0), (2, 1), (0, 2), (1, 2), (2, 2), (3, 2), (4, 2), (2, 3), (2, 4)):
        flake.put(x, y, np.array([1.0, 1.0, 1.0]))
    save(flake.image(), FX + "flake.png", 2)
    leaf = Canvas(5, 3)
    for x, y in ((0, 1), (1, 0), (1, 1), (2, 0), (2, 1), (3, 1), (3, 2), (4, 2)):
        leaf.put(x, y, np.array([1.0, 1.0, 1.0]))
    save(leaf.image(), FX + "leaf.png", 2)

    # -- skies -----------------------------------------------------------
    save(gradient(8, 256, [(0, "#1a1838"), (0.45, "#5a3a6a"), (0.75, "#d8785a"), (1, "#f4c27a")]), SKY + "dusk.png")
    save(gradient(8, 256, [(0, "#04060e"), (0.6, "#0c1428"), (1, "#1c2a48")]), SKY + "night.png")
    save(gradient(8, 256, [(0, "#2a4a6a"), (0.6, "#6a9ab8"), (1, "#c8dce4")]), SKY + "day.png")
    save(gradient(8, 256, [(0, "#020204"), (1, "#0a0812")]), SKY + "cave.png")
    hills(SKY + "hills_dusk.png", ramp("#2a2040", "#3a2a50", "#4a3458"), seed=50)
    hills(SKY + "hills_night.png", ramp("#0a0e1a", "#101828", "#18223a"), seed=51, snow=True)
    hills(SKY + "hills_forest.png", ramp("#12241e", "#1a3428", "#244634"), seed=52, trees=True)
    stars(SKY + "stars.png")


def hills(rel, pal, seed, snow=False, trees=False, w=320, h=96):
    """Far hills in two ranges, a flat silhouette each: the back paler."""
    c = Canvas(w, h)
    for layer, (base, amp, col) in enumerate(((0.35, 0.25, pal[2]), (0.6, 0.18, pal[0]))):
        n = value_noise(w, 1, 64 >> layer, seed + layer, 3)[0]
        for x in range(w):
            top = int(h * (base - amp * n[x] + 0.1 * layer))
            c.rect(x, top, x + 1, h, col)
            if snow and layer == 0:
                c.rect(x, top, x + 1, top + 3, np.array([0.55, 0.62, 0.78]))
            if trees and layer == 1 and x % 5 == 0:
                c.rect(x, top - 4, x + 2, top, col)
    save(c.image(), rel, 2)


def stars(rel, w=256, h=128):
    c = Canvas(w, h)
    r = rng(60)
    for _ in range(220):
        b = r.random() ** 3
        c.put(int(r.integers(0, w)), int(r.integers(0, h)), np.array([0.8 + 0.2 * b, 0.85 + 0.15 * b, 1.0]), a=0.3 + 0.7 * b)
    save(c.image(), rel, 2)


if __name__ == "__main__":
    main()
