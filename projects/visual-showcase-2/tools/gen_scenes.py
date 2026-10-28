"""Write every Visual Showcase 2 scene into assets/scenes/.

    python tools/gen_art.py      # the textures, first
    python tools/gen_scenes.py

Each scene is a diorama on a 3D stage (`hd2d.Scene`): floors, walls and
roofs as stage quads, characters and props as billboards, lights placed in
the stage so their pools shrink with distance and lie flat on the floor,
and the post chain HD-2D is known for - depth of field from the stage's
own depth, bloom over HDR, a vignette and a grade.
"""

import math

from hd2d import PIXEL, Scene, hexc

SC = "sprites/hd/scenery/"
CH = "sprites/hd/chars/"
FX = "sprites/hd/fx/"
SKY = "sprites/hd/sky/"


def surf(name):
    """A surface and its normal map."""
    return SC + name + ".png", SC + name + "_n.png"


def chrome(s, number, title, blurb, hint, script):
    import textwrap
    s.text("Title", f"{number}   {title}", "TopLeft", (56, -30), 34, hexc("#f2e2c0", 1.0), width=900)
    lines = textwrap.wrap(blurb, 118)
    s.text("Blurb", "\n".join(lines), "TopLeft", (58, -78), 15, hexc("#c8c4d8", 1.0), width=1160)
    s.text("Hint", hint, "BottomLeft", (58, 26), 15, hexc("#b8b8c8", 1.0))
    s.text("Status", "", "BottomRight", (-58, 52), 15, hexc("#f0b060", 1.0), align="Right", width=900)
    s.script("Director", script)


def hd2d_post(s, focus_range=22.0, strength=0.05, max_blur=7.0, bloom=0.35, threshold=0.95, vignette=0.45,
              contrast=1.06, saturation=1.1, tint=(1.0, 0.92, 0.82, 0.12), brightness=1.0,
              haze=(0.45, 0.42, 0.62, 0.55), haze_near=-60.0, haze_far=-420.0, haze_sky=0.25):
    """The HD-2D lens: haze thickening with depth, depth of field focused on
    the stage's target (z 0), bloom, a vignette and a grade. Depth is the
    stage's: ten z a unit into the scene, so the haze begins six units behind
    the traveller and is thickest forty-two on."""
    s.effect("depth_haze", color=haze, near=haze_near, far=haze_far, sky=haze_sky, curve=1.3)
    s.effect("depth_of_field", focus=0.0, range=focus_range, strength=strength, max_radius=max_blur)
    s.effect("bloom", threshold=threshold, intensity=bloom, blur_size=0.0)
    s.effect("vignette", intensity=vignette, smoothness=0.65, color=(0.02, 0.01, 0.04, 1.0))
    s.effect("color_grade", brightness=brightness, contrast=contrast, saturation=saturation, tint_color=tint)


def lamp_post(s, name, x, y, color=(1.0, 0.62, 0.3), intensity=2.6, radius=9.0):
    tex, n = surf("lamp_post")
    s.billboard(name, tex, (x, y, 0), (14 * 4 * 0.5, 56 * 4 * 0.5), normal=n)
    s.billboard(name + "Glass", SC + "lamp_post_glow.png", (x, y, 0), (14 * 4 * 0.5, 56 * 4 * 0.5),
                color=(3.0, 2.0, 1.0, 1.0), unlit=True, blend="Additive", z_bias=1)
    s.glow(name + "Halo", (x, y, 2.75), 90, (1.0, 0.55, 0.2, 0.55))
    s.light(name + "Light", (x, y, 2.6), color, intensity=intensity, radius=radius, falloff=1.3, height=0.5)


def card(s, name, kind, x, y, scale=1.0, **kw):
    """A standing prop card at its art size, two world pixels a texel."""
    sizes = {"oak": (56, 72), "oak_autumn": (56, 72), "pine": (36, 80), "pine_snow": (36, 80), "bush": (28, 18),
             "bush_berries": (28, 18), "barrel": (14, 18), "crate": (16, 16), "crystal": (24, 34),
             "crystal_pink": (24, 34), "stalagmite": (14, 34), "shrine_lantern": (16, 34), "fence": (32, 14),
             "signpost": (20, 30), "table": (30, 14), "mug": (6, 7), "torch_bracket": (8, 16),
             "grass_tuft": (24, 20), "grass_tuft_dry": (24, 20), "banner": (20, 64), "banner_blue": (20, 64)}
    w, h = sizes[kind]
    tex, n = surf(kind)
    # Standing props throw shadows too; flat ones and lights' own do not.
    kw.setdefault("shadow", kind not in ("mug", "torch_bracket", "crystal", "crystal_pink", "grass_tuft",
                                         "grass_tuft_dry"))
    return s.billboard(name, tex, (x, y, kw.pop("z", 0.0)), (w * PIXEL * scale, h * PIXEL * scale), normal=n, **kw)


def chimney_smoke(s, name, x, y, z):
    s.emitter(name, (x, y, z), {
        "texture_id": FX + "glow.png", "emission_rate": 4.0, "max_particles": 40, "lifetime": [3.0, 5.0],
        "speed": [10.0, 22.0], "angle": [1.35, 1.75], "color_start": [0.35, 0.32, 0.36, 0.35],
        "color_end": [0.25, 0.22, 0.3, 0.0], "size_start": [14.0, 20.0], "size_end": [60.0, 90.0],
        "gravity_modifier": -0.02, "angular_velocity": [0.0, 0.0], "burst_count": 0, "active": True,
        "world_space": True, "z_order": 0, "emit_shape": {"kind": "circle", "radius": 4.0},
        "drag": 0.3, "turbulence": {"strength": 18.0, "scale": 90.0, "speed": 0.4}}, z_bias=2)


def motes(s, name, x, y, z, color, rate=6.0, spread=60.0, size=(3.0, 6.0), unlit=True):
    s.emitter(name, (x, y, z), {
        "texture_id": FX + "mote.png", "emission_rate": rate, "max_particles": 120, "lifetime": [3.0, 6.0],
        "speed": [4.0, 14.0], "angle": [0.0, 6.28318], "color_start": list(color), "color_end": list(color[:3]) + [0.0],
        "size_start": list(size), "size_end": [size[0] * 0.5, size[1] * 0.5], "gravity_modifier": -0.01,
        "angular_velocity": [0.0, 0.0], "burst_count": 0, "active": True, "world_space": True, "z_order": 0,
        "emit_shape": {"kind": "circle", "radius": spread}, "blend_mode": "Additive",
        "turbulence": {"strength": 10.0, "scale": 60.0, "speed": 0.5}}, z_bias=40)


# ---------------------------------------------------------------------------
# 01 Lantern Square
# ---------------------------------------------------------------------------

def lantern_square():
    s = Scene("01 - Lantern Square")
    s.backdrop("Sky", SKY + "dusk.png", (1500, 900))
    s.backdrop("Hills", SKY + "hills_dusk.png", (1600, 480), pos=(0, 130), z=-29000)
    s.ambient(hexc("#5a6ab8"), 0.34)
    s.add("Sun", directional_light={"color": hexc("#ff9a60"), "intensity": 0.3, "direction": 3.6, "height": 0.35,
                                    "casts_shadows": False})
    s.stage(target=(0, -2, 0.8), yaw=0.0, pitch=0.55, distance=52.0, zoom=38.0, follow="Hero")

    cob, cob_n = surf("cobbles_warm")
    grass, grass_n = surf("grass")
    dirt, dirt_n = surf("dirt")
    water, water_n = surf("water")
    # The ground: grass far and wide, the square's cobbles over it, the
    # river and its banks across the north.
    s.floor("Meadow", grass, -40, -24, 40, 60, tile=2.4, normal=grass_n, color=hexc("#c8c0b0", 1.0))
    s.floor("Square", cob, -13, -9, 13, 9.5, tile=1.2, normal=cob_n, z_bias=1)
    s.floor("Lane", dirt, -2.2, -24, 2.2, -9, tile=1.8, normal=dirt_n, z_bias=1)
    s.floor("BankS", cob, -40, 9.5, 40, 10.2, tile=1.6, normal=cob_n, z_bias=1, color=hexc("#a89888", 1.0))
    s.floor("River", water, -40, 10.2, 40, 13.6, tile=2.0, normal=water_n, z_bias=1, specular=0.3,
            color=(0.4, 0.48, 0.72, 1.0))
    s.floor("BankN", cob, -40, 13.6, 40, 14.3, tile=1.6, normal=cob_n, z_bias=1, color=hexc("#a89888", 1.0))
    s.floor("NorthStreet", cob, -14, 14.3, 14, 22, tile=1.6, normal=cob_n, z_bias=1)
    # The bridge: planks over the water, low walls either side.
    planks, planks_n = surf("planks")
    s.floor("Bridge", planks, -1.6, 9.8, 1.6, 14.0, tile=1.0, normal=planks_n, z_bias=3, z=0.05)
    bricks, bricks_n = surf("moss_stone")
    s.box("BridgeWallW", -1.9, 9.8, -1.6, 14.0, 0.0, 0.7, bricks, bricks, tile=0.8, side_n=bricks_n, top_n=bricks_n)
    s.box("BridgeWallE", 1.6, 9.8, 1.9, 14.0, 0.0, 0.7, bricks, bricks, tile=0.8, side_n=bricks_n, top_n=bricks_n)

    # Houses round the square, and across the river.
    timber, timber_n = surf("timber")
    timber_b, timber_bn = surf("timber_blue")
    roof_r, roof_rn = surf("roof_red")
    roof_b, roof_bn = surf("roof_blue")
    brick, brick_n = surf("bricks")
    gable_t, gable_tn = surf("gable_timber")
    gable_b, gable_bn = surf("gable_blue")
    gable_k, gable_kn = surf("gable_bricks")
    win, win_n = surf("window")
    door, door_n = surf("door")
    glow = SC + "window_glow.png"
    common = dict(window_tex=win, window_n=win_n, glow_tex=glow, door_tex=door, door_n=door_n)
    s.house("Inn", -12.5, 2.0, -5.5, 7.5, 3.4, 2.4, timber, roof_r, gable_t, walls_n=timber_n, roof_n=roof_rn,
            windows=[(-11.2, 1.3), (-6.8, 1.3), (-10.0, 2.6 + 0.6), (-7.6, 2.6 + 0.6)], door=-9.0, **common)
    s.house("Smithy", 5.5, 2.5, 12.0, 7.5, 3.0, 2.2, timber_b, roof_b, gable_b, walls_n=timber_bn, roof_n=roof_bn,
            windows=[(7.0, 1.2), (10.6, 1.2)], door=8.8, **common)
    s.house("Hall", -10.0, 15.5, -2.5, 20.5, 4.2, 2.8, brick, roof_r, gable_k, walls_n=brick_n, roof_n=roof_rn,
            windows=[(-8.6, 1.5), (-6.2, 1.5), (-3.9, 1.5), (-7.4, 3.0), (-5.0, 3.0)], door=-6.2, **common)
    s.house("Mill", 2.5, 15.5, 9.0, 20.0, 3.2, 2.4, timber, roof_b, gable_t, walls_n=timber_n, roof_n=roof_bn,
            windows=[(4.0, 1.3), (7.6, 1.3)], door=5.8, **common)
    chimney_smoke(s, "SmokeInn", -7.0, 6.0, 6.2)
    chimney_smoke(s, "SmokeHall", -4.0, 19.0, 7.2)

    # The fountain in the middle.
    s.box("Fountain", -1.3, -0.3, 1.3, 2.3, 0.0, 0.65, bricks, bricks, tile=0.8, side_n=bricks_n, top_n=bricks_n)
    s.quad("FountainWater", water, (-1.05, -0.05, 0.62), (2.1, 0, 0), (0, 2.1, 0), (1, 1), normal=water_n,
           specular=1.0, color=(0.5, 0.65, 0.95, 1.0), z_bias=1, sort_bias=-0.2)
    s.emitter("Spray", (0, 1.0, 1.2), {
        "texture_id": FX + "mote.png", "emission_rate": 40.0, "max_particles": 120, "lifetime": [0.6, 0.9],
        "speed": [50.0, 80.0], "angle": [1.4, 1.74], "color_start": [0.7, 0.85, 1.0, 0.8],
        "color_end": [0.6, 0.75, 1.0, 0.0], "size_start": [3.0, 5.0], "size_end": [2.0, 3.0],
        "gravity_modifier": 1.4, "angular_velocity": [0.0, 0.0], "burst_count": 0, "active": True,
        "world_space": True, "z_order": 0, "emit_shape": {"kind": "circle", "radius": 3.0}}, z_bias=1)

    # Lamps round the square and on the far bank.
    for i, (x, y) in enumerate(((-4.0, -4.0), (4.0, -4.0), (-3.0, 8.6), (3.0, 8.6), (-11.5, 15.0), (11.0, 15.0))):
        lamp_post(s, f"Lamp{i}", x, y)
    # The inn's door lantern, warmer and lower.
    s.light("InnDoorLight", (-9.0, 1.6, 2.2), (1.0, 0.55, 0.25), intensity=1.4, radius=4.0)

    # Props.
    for i, (x, y) in enumerate(((-12.0, 0.8), (-11.3, 1.0), (-4.8, 1.5))):
        card(s, f"Barrel{i}", "barrel", x, y)
    for i, (x, y) in enumerate(((4.6, 2.0), (5.2, 1.6), (12.0, 1.8))):
        card(s, f"Crate{i}", "crate", x, y)
    card(s, "Sign", "signpost", 3.2, -8.5)
    for i, (x, y, k) in enumerate(((-16, 2, "oak"), (-15, 12, "oak_autumn"), (16, 4, "oak"), (15.5, 11.5, "oak_autumn"),
                                   (-18, -6, "pine"), (18, -5, "pine"), (-8, 24, "oak"), (11, 24, "pine"),
                                   (-24, 18, "oak_autumn"), (24, 20, "oak"))):
        card(s, f"Tree{i}", k, x, y, scale=2.0)
    for i, (x, y) in enumerate(((-6, -9.5), (6, -9.5), (-13.5, 9.0), (13.5, 9.0), (-3.5, 14.8), (10, 14.8))):
        card(s, f"Bush{i}", "bush_berries" if i % 2 else "bush", x, y, scale=1.4)
    for i in range(6):
        card(s, f"Fence{i}", "fence", -20 + i * 2.2, -8.0, scale=1.4)

    # People: the hero, and the square's evening.
    s.character("Hero", CH + "traveler.png", (0, -3, 0))
    s.character("Merchant", CH + "merchant.png", (6.0, 0.0, 0), facing=-1)
    s.character("Villager", CH + "villager.png", (-6.0, -1.0, 0))
    s.character("Lamplighter", CH + "lamplighter.png", (3.0, 8.0, 0))
    s.character("Guard", CH + "guard.png", (0.6, 12.0, 0), facing=-1)
    s.character("Child", CH + "child.png", (-2.0, 4.0, 0))
    s.character("Cleric", CH + "cleric.png", (-5.0, 17.0, 0))
    motes(s, "Fireflies", 0, 4, 1.5, (1.4, 1.2, 0.5, 0.9), rate=5.0, spread=420.0)

    hd2d_post(s, focus_range=14.0, strength=0.07, max_blur=9.0, tint=(1.0, 0.94, 0.88, 0.06))
    chrome(s, "01", "Lantern Square",
           "Dusk in a market square: houses, roofs and cobbles drawn in perspective on a 3D stage, pixel people "
           "standing in it, lanterns pooling light flat on the floor and depth of field focused on the traveller.",
           "Arrows / WASD walk   Q / E turn the camera   1-5 depth of field, bloom, vignette, grade, haze   Tab captions   "
           "PgUp / PgDn scenes   H hub", "scripts/s01_lantern_square.lua")
    return s.save("scenes/01-lantern-square.scene")


def flame(s, name, pos, size=1.0, color=(4.0, 2.2, 0.8, 1.0), light=True, radius=6.0, intensity=2.2):
    """A torch flame: a four-frame additive card, a halo, embers and a light."""
    frames = [{"uv_rect": [i / 4, 0.0, (i + 1) / 4, 1.0], "triggers": []} for i in range(4)]
    anim = {"sprite_animator": {"clips": {"burn": {"name": "burn", "frame_duration": 0.09, "looping": True,
                                                    "property_tracks": [], "frames": frames}},
                                "current_clip": "burn", "playing": True, "speed": 1.0}}
    s.billboard(name, FX + "flame.png", pos, (24 * size, 40 * size), anchor=(0.5, 0.1), uv=[0, 0, 0.25, 1],
                color=color, unlit=True, blend="Additive", z_bias=2, extra=anim)
    s.glow(name + "Halo", (pos[0], pos[1], pos[2] + 0.35 * size), 110 * size, (1.0, 0.45, 0.12, 0.5))
    s.emitter(name + "Embers", (pos[0], pos[1], pos[2] + 0.3), {
        "texture_id": FX + "mote.png", "emission_rate": 5.0, "max_particles": 30, "lifetime": [1.0, 2.2],
        "speed": [20.0, 50.0], "angle": [1.3, 1.85], "color_start": [5.0, 2.4, 0.6, 1.0],
        "color_end": [2.0, 0.3, 0.05, 0.0], "size_start": [3.0, 5.0], "size_end": [1.0, 2.0],
        "gravity_modifier": -0.08, "angular_velocity": [0.0, 0.0], "burst_count": 0, "active": True,
        "world_space": True, "z_order": 0, "emit_shape": {"kind": "circle", "radius": 4.0}, "blend_mode": "Additive",
        "turbulence": {"strength": 20.0, "scale": 40.0, "speed": 1.0}}, z_bias=3)
    if light:
        s.light(name + "Light", (pos[0], pos[1], pos[2] + 0.4), (1.0, 0.55, 0.22), intensity=intensity,
                radius=radius, height=0.45, tags=["flicker"])


# ---------------------------------------------------------------------------
# 02 Crystal Hollow
# ---------------------------------------------------------------------------

def crystal_hollow():
    s = Scene("02 - Crystal Hollow")
    s.backdrop("Dark", SKY + "cave.png", (1500, 900))
    s.ambient(hexc("#8898d8"), 0.5)
    s.stage(target=(0, -1, 0.8), yaw=0.0, pitch=0.6, distance=44.0, zoom=46.0, follow="Hero")
    rock, rock_n = surf("rock_warm")
    rock_c, rock_cn = surf("rock")
    water, water_n = surf("water")
    s.floor("Floor", rock, -16, -12, 16, 18, tile=2.2, normal=rock_n, color=hexc("#e8dccc", 1.0))
    s.floor("Pool", water, 2.5, 2.0, 9.0, 7.0, tile=2.0, normal=water_n, z_bias=1, specular=1.0,
            color=(0.35, 0.5, 0.75, 1.0))
    # The cave's walls, facing in, ragged in height; pillars standing in it.
    heights = [6.5, 8.0, 7.0, 9.5, 7.5, 8.5, 6.8, 9.0]
    xs = [-16 + i * 4 for i in range(9)]
    for i in range(8):
        s.wall(f"Back{i}", rock_c, (xs[i], 18), (xs[i + 1], 18), 0, heights[i], tile=2.4, normal=rock_cn)
    for i in range(6):
        y0, y1 = -12 + i * 5, -12 + (i + 1) * 5
        s.wall(f"West{i}", rock_c, (-16, y0), (-16, y1), 0, heights[i] - 0.5, tile=2.4, normal=rock_cn)
        s.wall(f"East{i}", rock_c, (16, y1), (16, y0), 0, heights[(i + 3) % 8] - 0.5, tile=2.4, normal=rock_cn)
    for i, (x, y, w, h) in enumerate(((-9, 8, 2.2, 7.0), (11, 11, 2.6, 8.0), (-11, -2, 1.8, 5.0))):
        s.box(f"Pillar{i}", x - w / 2, y - w / 2, x + w / 2, y + w / 2, 0, h, rock_c, None, tile=2.0, side_n=rock_cn)
    # Torches on the back wall and the pillars.
    for i, (x, y, z) in enumerate(((-6, 17.6, 2.4), (6, 17.6, 2.4), (-9, 6.8, 2.2), (11, 9.6, 2.4),
                                   (-11, -3.1, 2.0), (-15.8, 8.0, 2.4), (15.8, -2.0, 2.4))):
        card(s, f"Bracket{i}", "torch_bracket", x, y, z=z - 0.9)
        flame(s, f"Torch{i}", (x, y - 0.05, z), radius=7.5)
    # Crystals: their own cold light.
    for i, (x, y, k, c) in enumerate(((-12, 12, "crystal", (0.35, 0.55, 1.0)), (13, 3, "crystal_pink", (1.0, 0.4, 0.8)),
                                      (-4, 13, "crystal", (0.35, 0.55, 1.0)), (8, 14, "crystal_pink", (1.0, 0.4, 0.8)),
                                      (-13, -7, "crystal", (0.35, 0.55, 1.0)))):
        card(s, f"Crystal{i}", k, x, y, scale=1.8, unlit=True, color=(1.6, 1.6, 1.8, 1.0))
        s.glow(f"Crystal{i}Glow", (x, y, 1.0), 220, list(c) + [0.35])
        s.light(f"Crystal{i}Light", (x, y, 1.2), c, intensity=1.3, radius=6.0, height=0.6)
    for i, (x, y) in enumerate(((-6, 4), (5, -4), (-2, 10), (12, -8), (-12, 4), (3, 12))):
        card(s, f"Stalagmite{i}", "stalagmite", x, y, scale=1.4 + 0.3 * (i % 3))
    # Daylight through a crack far above: a shaft, motes in it, a cool pool.
    s.billboard("Shaft", FX + "shaft.png", (-4, 1.5, 0), (240, 820), anchor=(0.5, 0.0), color=(0.75, 0.9, 1.3, 0.55),
                unlit=True, blend="Additive", z_bias=30)
    s.quad("ShaftPool", FX + "glow.png", (-5.6, 0.7, 0.02), (3.2, 0, 0), (0, 1.6, 0), ground=True, unlit=True,
           blend="Additive", color=(0.6, 0.75, 1.1, 0.55), z_bias=6)
    s.light("ShaftLight", (-4, 1.5, 3.0), (0.75, 0.85, 1.0), intensity=1.6, radius=5.0, height=1.2)
    motes(s, "ShaftDust", -4, 1.5, 2.5, (0.9, 0.95, 1.2, 0.8), rate=10.0, spread=80.0, size=(2.0, 4.0))
    # The traveller carries a lantern: a warm light that goes where they go.
    s.add("Lantern", point_light={"color": [1.0, 0.7, 0.4], "intensity": 1.6, "radius": 100.0, "falloff": 1.5,
                                  "height": 0.5, "casts_shadows": False},
          stage_billboard={"position": [0.35, -0.1, 1.2], "light_radius": 5.5, "follow": "Hero"})
    s.character("Hero", CH + "traveler.png", (0, -2, 0))
    s.character("Hunter", CH + "hunter.png", (-3.5, 0.5, 0))
    s.character("Scholar", CH + "scholar.png", (3.0, -5.0, 0), facing=-1)
    hd2d_post(s, focus_range=16.0, strength=0.06, bloom=0.5, threshold=0.85, vignette=0.6, contrast=1.1,
              saturation=1.05, tint=(0.9, 0.95, 1.05, 0.1), haze=(0.03, 0.04, 0.09, 0.6), haze_near=-40.0,
              haze_far=-220.0, haze_sky=0.0)
    chrome(s, "02", "Crystal Hollow",
           "A cave lit by torches and glowing crystal: wet rock catching each light in its normal map, embers "
           "rising, a cold shaft of daylight from a crack far above, and the pool shining.",
           "Arrows / WASD walk   Q / E turn the camera   1-5 depth of field, bloom, vignette, grade, haze   Tab captions   "
           "PgUp / PgDn scenes   H hub", "scripts/s02_crystal_hollow.lua")
    return s.save("scenes/02-crystal-hollow.scene")


def falling_leaves(s, name, x, y, z, rate=6.0, spread=500.0):
    s.emitter(name, (x, y, z), {
        "texture_id": FX + "leaf.png", "emission_rate": rate, "max_particles": 80, "lifetime": [5.0, 8.0],
        "speed": [10.0, 30.0], "angle": [4.2, 5.2], "color_start": hexc("#e0902a", 1.0),
        "color_end": hexc("#c4661e", 0.0),
        "color_choices": [hexc("#e0902a", 1.0), hexc("#c4661e", 1.0), hexc("#f0b850", 1.0), hexc("#9a4418", 1.0)],
        "size_start": [6.0, 9.0], "size_end": [6.0, 9.0], "gravity_modifier": 0.05,
        "angular_velocity": [-3.0, 3.0], "burst_count": 0, "active": True, "world_space": True, "z_order": 0,
        "emit_shape": {"kind": "circle", "radius": spread}, "drag": 0.4,
        "turbulence": {"strength": 30.0, "scale": 120.0, "speed": 0.4}}, z_bias=60)


# ---------------------------------------------------------------------------
# 03 Forest Shrine
# ---------------------------------------------------------------------------

def forest_shrine():
    s = Scene("03 - Forest Shrine")
    s.backdrop("Sky", SKY + "day.png", (1500, 900), color=(0.8, 0.85, 0.75, 1.0))
    s.backdrop("Hills", SKY + "hills_forest.png", (1600, 480), pos=(0, 140), z=-29000)
    s.ambient(hexc("#90a888"), 0.6)
    s.add("Sun", directional_light={"color": hexc("#ffe0a0"), "intensity": 0.55, "direction": 4.4, "height": 0.6,
                                    "casts_shadows": False})
    s.stage(target=(0, -1, 0.8), yaw=0.0, pitch=0.5, distance=44.0, zoom=46.0, follow="Hero")
    grass, grass_n = surf("grass")
    dirt, dirt_n = surf("dirt")
    stone, stone_n = surf("moss_stone")
    planks, planks_n = surf("planks_dark")
    roof, roof_n = surf("roof_blue")
    gable, gable_n = surf("gable_planks")
    s.floor("Ground", grass, -40, -24, 40, 60, tile=2.2, normal=grass_n, color=hexc("#d8d0b0", 1.0))
    s.floor("Path", dirt, -1.6, -24, 1.6, 9, tile=1.6, normal=dirt_n, z_bias=1)
    s.floor("Court", stone, -6, 9, 6, 16, tile=1.4, normal=stone_n, z_bias=1)
    # Steps up to the shrine: three low boxes.
    for i in range(3):
        s.box(f"Step{i}", -3 + i * 0.4, 8.4 + i * 0.5, 3 - i * 0.4, 9.0 + i * 0.5, 0, 0.25 * (i + 1), stone, stone,
              tile=1.0, side_n=stone_n, top_n=stone_n)
    # The shrine: a plank hall on a stone base, a blue roof.
    s.box("Base", -4.5, 11, 4.5, 16, 0, 0.6, stone, stone, tile=1.2, side_n=stone_n, top_n=stone_n)
    s.house("Shrine", -3.6, 11.6, 3.6, 15.4, 3.0, 2.6, planks, roof, gable, walls_n=planks_n, roof_n=roof_n,
            overhang=0.8, door=0.0, door_tex=surf("door")[0], door_n=surf("door")[1])
    # A gate on the path: two posts and two beams.
    red = (0.95, 0.35, 0.25, 1.0)
    for x in (-2.4, 2.1):
        s.box(f"GatePost{x}", x, 3.6, x + 0.3, 3.9, 0, 4.2, planks, planks, tile=0.6, side_n=planks_n, color=red)
    s.box("GateBeam", -3.0, 3.55, 3.0, 3.95, 4.2, 4.6, planks, planks, tile=0.6, side_n=planks_n, color=red)
    s.box("GateTie", -2.6, 3.6, 2.6, 3.9, 3.5, 3.75, planks, planks, tile=0.6, side_n=planks_n, color=red)
    # Stone lanterns along the path, lit.
    for i, (x, y) in enumerate(((-2.8, -2), (2.8, -2), (-2.8, 5), (2.8, 5), (-5.5, 10), (5.5, 10))):
        card(s, f"Lantern{i}", "shrine_lantern", x, y, scale=1.4)
        s.glow(f"Lantern{i}Glow", (x, y - 0.05, 1.1), 70, (1.0, 0.6, 0.25, 0.7))
        s.light(f"Lantern{i}Light", (x, y - 0.1, 1.2), (1.0, 0.65, 0.3), intensity=1.2, radius=3.5, height=0.5)
    # The wood: oaks all round, deep into the distance.
    import random
    rnd = random.Random(7)
    spots = []
    for i in range(46):
        a = rnd.random() * 2 * math.pi
        r = 9 + rnd.random() * 26
        x, y = math.cos(a) * r * 1.2, 6 + math.sin(a) * r
        if abs(x) < 4 and y < 18:
            continue
        spots.append((x, y))
    for i, (x, y) in enumerate(spots):
        card(s, f"Tree{i}", "oak_autumn" if i % 3 else "oak", x, y, scale=2.2 + rnd.random() * 0.8)
    for i in range(14):
        x, y = rnd.uniform(-16, 16), rnd.uniform(-14, 20)
        if abs(x) < 3:
            continue
        card(s, f"Bush{i}", "bush" if i % 2 else "bush_berries", x, y, scale=1.3)
    # God rays: shafts of sun slanting through the canopy, with dust in them.
    for i, (x, y) in enumerate(((-3.5, -1), (2.5, 2.5), (-1, 7.5), (5.5, -4))):
        s.billboard(f"Ray{i}", FX + "shaft.png", (x, y, 0), (200, 760), anchor=(0.5, 0.0),
                    color=(1.2, 1.0, 0.6, 0.4), unlit=True, blend="Additive", z_bias=30)
        s.quad(f"Ray{i}Pool", FX + "glow.png", (x - 1.6, y - 0.8, 0.02), (3.2, 0, 0), (0, 1.6, 0), ground=True,
               unlit=True, blend="Additive", color=(1.0, 0.85, 0.5, 0.45), z_bias=6)
        motes(s, f"RayDust{i}", x, y, 2.0, (1.2, 1.1, 0.7, 0.7), rate=3.0, spread=60.0, size=(2.0, 3.5))
    falling_leaves(s, "Leaves", 0, 4, 9.0)
    motes(s, "Fireflies", 0, 6, 1.2, (1.0, 1.3, 0.5, 0.9), rate=4.0, spread=520.0)
    s.character("Hero", CH + "traveler.png", (0, 0.5, 0))
    s.character("Cleric", CH + "cleric.png", (-0.8, 10.6, 0))
    s.character("Hunter", CH + "hunter.png", (6.0, 1.0, 0), facing=-1)
    hd2d_post(s, focus_range=14.0, strength=0.07, max_blur=9.0, bloom=0.4, threshold=0.9, vignette=0.5,
              contrast=1.08, saturation=1.15, tint=(1.0, 0.95, 0.8, 0.1), haze=(0.5, 0.56, 0.38, 0.5),
              haze_near=-50.0, haze_far=-380.0, haze_sky=0.3)
    chrome(s, "03", "Forest Shrine",
           "A shrine in a clearing of autumn oaks: shafts of sun through the canopy with dust turning in them, "
           "falling leaves, stone lanterns, and the wood beyond the focus melting into colour.",
           "Arrows / WASD walk   Q / E turn the camera   1-5 depth of field, bloom, vignette, grade, haze   Tab captions   "
           "PgUp / PgDn scenes   H hub", "scripts/s03_forest_shrine.lua")
    return s.save("scenes/03-forest-shrine.scene")


# ---------------------------------------------------------------------------
# 04 The Tavern
# ---------------------------------------------------------------------------

def tavern():
    s = Scene("04 - The Tavern")
    s.backdrop("Dark", SKY + "cave.png", (1500, 900), color=(0.6, 0.5, 0.45, 1.0))
    s.ambient(hexc("#d8a898"), 0.42)
    for i, (x, y) in enumerate(((-5.0, 3.5), (2.5, 2.5), (-1.0, 7.0), (7.0, 5.5))):
        s.glow(f"Hanging{i}Glow", (x, y, 3.8), 80, (1.0, 0.6, 0.25, 0.6))
        s.light(f"Hanging{i}", (x, y, 3.8), (1.0, 0.68, 0.38), intensity=1.4, radius=6.5, height=0.8)
    s.stage(target=(0, 2, 0.8), yaw=0.0, pitch=0.68, distance=36.0, zoom=56.0, follow="Hero",
            follow_offset=(0, 3.0, 0.8))
    planks, planks_n = surf("planks")
    wallp, wallp_n = surf("planks_wall")
    brick, brick_n = surf("bricks")
    dark, dark_n = surf("planks_dark")
    # A room seen as a dollhouse: no south wall.
    s.floor("Floor", planks, -10, -4, 10, 10, tile=1.2, normal=planks_n)
    s.wall("Back", brick, (-10, 10), (10, 10), 0, 5.0, tile=1.6, normal=brick_n)
    # The cut-away front: the floor's edge and the stub of the south wall.
    s.box("FrontSill", -10.3, -4.4, 10.3, -4.0, -0.6, 0.35, dark, dark, tile=0.8, side_n=dark_n, top_n=dark_n)
    s.wall("West", wallp, (-10, -4), (-10, 10), 0, 5.0, tile=1.6, normal=wallp_n)
    s.wall("East", wallp, (10, 10), (10, -4), 0, 5.0, tile=1.6, normal=wallp_n)
    # Beams across the top of the back wall and down its corners.
    s.box("Beam", -10, 9.6, 10, 10, 4.6, 5.0, dark, dark, tile=0.8, side_n=dark_n)
    for x in (-10, -3.3, 3.3, 9.6):
        s.box(f"Post{x}", x, 9.6, x + 0.4, 10, 0, 4.6, dark, None, tile=0.8, side_n=dark_n)
    # The hearth on the back wall, a fire in it.
    s.box("Hearth", -2.0, 8.8, 2.0, 10, 0, 2.6, brick, brick, tile=0.8, side_n=brick_n, top_n=brick_n,
          color=hexc("#c0a090", 1.0))
    s.quad("HearthMouth", SC + "window.png", (-1.2, 8.75, 0.05), (2.4, 0, 0), (0, 0, 1.4), color=(0.05, 0.03, 0.03, 1.0),
           z_bias=1)
    flame(s, "Fire", (-0.4, 8.6, 0.1), size=2.0, radius=11.0, intensity=3.0)
    flame(s, "Fire2", (0.5, 8.6, 0.1), size=1.6, light=False)
    # The bar along the east wall.
    s.box("Bar", 5.5, 1.0, 7.0, 8.0, 0, 1.2, dark, planks, tile=0.9, side_n=dark_n, top_n=planks_n)
    for i, y in enumerate((1.6, 3.2, 4.8, 6.6)):
        card(s, f"Barrel{i}", "barrel", 9.2, y, scale=1.6)
    for i, (x, y) in enumerate(((-6.5, 6.5), (-6.5, 1.5), (-1.5, 3.0), (2.5, -0.5))):
        card(s, f"Table{i}", "table", x, y, scale=1.6)
        card(s, f"Mug{i}", "mug", x - 0.4, y - 0.05, scale=1.6, z=0.85)
        flame(s, f"Candle{i}", (x + 0.5, y - 0.05, 0.9), size=0.6, radius=3.5, intensity=1.2)
    for i, (x, y) in enumerate(((-9.2, 8.8), (-8.5, 9.2), (-9.4, 7.6))):
        card(s, f"Crate{i}", "crate", x, y, scale=1.5)
    # A window in the west wall with moonlight through it.
    win, win_n = surf("window")
    s.quad("Window", win, (-9.97, 7.0, 1.8), (0, -1.6, 0), (0, 0, 2.0), normal=win_n, z_bias=1)
    s.light("Moon", (-8.5, 6.0, 2.5), (0.5, 0.65, 1.0), intensity=0.9, radius=6.0, height=0.8)
    motes(s, "Dust", 0, 4, 2.0, (1.1, 0.9, 0.6, 0.5), rate=5.0, spread=380.0, size=(1.5, 3.0))
    s.character("Hero", CH + "traveler.png", (0, -2, 0))
    s.character("Barkeep", CH + "merchant.png", (7.8, 4.0, 0), facing=-1)
    s.character("Guard", CH + "guard.png", (-5.5, 6.0, 0))
    s.character("Scholar", CH + "scholar.png", (-7.5, 1.4, 0))
    s.character("Farmer", CH + "farmer.png", (3.4, -0.6, 0), facing=-1)
    s.character("Villager", CH + "villager.png", (-2.5, 2.8, 0))
    hd2d_post(s, focus_range=12.0, strength=0.07, bloom=0.5, threshold=0.85, vignette=0.6, contrast=1.08,
              saturation=1.1, tint=(1.0, 0.85, 0.7, 0.15), haze=(0.22, 0.12, 0.06, 0.35), haze_near=-30.0,
              haze_far=-140.0, haze_sky=0.0)
    chrome(s, "04", "The Tavern",
           "Inside a tavern seen as a dollhouse: the hearth's light washing across the planks, candles on the "
           "tables, moonlight through the window, regulars making their rounds between the bar and the fire.",
           "Arrows / WASD walk   Q / E turn the camera a little   1-5 depth of field, bloom, vignette, grade, haze   "
           "Tab captions   PgUp / PgDn scenes   H hub", "scripts/s04_the_tavern.lua")
    return s.save("scenes/04-the-tavern.scene")


# ---------------------------------------------------------------------------
# 05 Snowbound
# ---------------------------------------------------------------------------

def snowbound():
    s = Scene("05 - Snowbound")
    s.backdrop("Sky", SKY + "night.png", (1500, 900))
    s.backdrop("Stars", SKY + "stars.png", (1500, 760), pos=(0, 80), z=-29500, color=(1, 1, 1, 0.9))
    s.backdrop("Hills", SKY + "hills_night.png", (1600, 480), pos=(0, 120), z=-29000)
    s.ambient(hexc("#5a70b8"), 0.4)
    s.add("Moon", directional_light={"color": hexc("#a8c0ff"), "intensity": 0.45, "direction": 4.0, "height": 0.7,
                                     "casts_shadows": False})
    s.stage(target=(0, 0, 0.8), yaw=0.0, pitch=0.5, distance=48.0, zoom=44.0, follow="Hero")
    snow, snow_n = surf("snow")
    timber, timber_n = surf("timber_blue")
    roof, roof_n = surf("roof_snow")
    gable, gable_n = surf("gable_blue")
    stone, stone_n = surf("bricks_dark")
    win, win_n = surf("window")
    door, door_n = surf("door")
    common = dict(window_tex=win, window_n=win_n, glow_tex=SC + "window_glow.png", door_tex=door, door_n=door_n)
    s.floor("Snow", snow, -50, -30, 50, 70, tile=2.6, normal=snow_n)
    houses = [(-13, 3, -6, 8, 3.2, 2.6), (6, 2, 12.5, 7, 3.0, 2.4), (-9, 14, -2, 19, 3.4, 2.8), (3, 15, 10, 20, 3.0, 2.4),
              (-20, 10, -14, 15, 2.8, 2.2), (15, 12, 21, 17, 3.0, 2.4)]
    for i, (x0, y0, x1, y1, h, r) in enumerate(houses):
        mid = (x0 + x1) / 2
        s.house(f"House{i}", x0, y0, x1, y1, h, r, timber, roof, gable, walls_n=timber_n, roof_n=roof_n,
                windows=[(x0 + 1.3, 1.2), (x1 - 1.3, 1.2)], door=mid, **common)
        chimney_smoke(s, f"Smoke{i}", x1 - 1.5, (y0 + y1) / 2, h + r + 0.8)
    s.box("Well", -1.2, 5.0, 1.2, 7.4, 0, 0.9, stone, SC + "snow.png", tile=0.8, side_n=stone_n, top_n=snow_n)
    for i, (x, y) in enumerate(((-4, -3), (4, -3), (-4, 11), (4, 11), (13, 9), (-15, 6))):
        lamp_post(s, f"Lamp{i}", x, y, color=(1.0, 0.7, 0.4), intensity=2.0, radius=6.5)
    import random
    rnd = random.Random(11)
    for i in range(40):
        a = rnd.random() * 2 * math.pi
        r = 18 + rnd.random() * 22
        card(s, f"Pine{i}", "pine_snow", math.cos(a) * r * 1.3, 6 + math.sin(a) * r, scale=2.0 + rnd.random())
    for i in range(8):
        card(s, f"Fence{i}", "fence", -10 + i * 2.2, -9.0, scale=1.4)
    # Snow falling across the whole view, and drifting near the lamps.
    s.add("Snowfall", transform={"position": [0.0, 420.0], "rotation": 0.0, "scale": [1.0, 1.0]}, particle_emitter={
        "texture_id": FX + "flake.png", "emission_rate": 120.0, "max_particles": 1600, "lifetime": [7.0, 10.0],
        "speed": [30.0, 60.0], "angle": [4.4, 5.0], "color_start": [1.0, 1.0, 1.0, 0.9],
        "color_end": [1.0, 1.0, 1.0, 0.0], "size_start": [3.0, 6.0], "size_end": [3.0, 6.0],
        "gravity_modifier": 0.0, "angular_velocity": [-1.0, 1.0], "burst_count": 0, "active": True,
        "world_space": True, "z_order": 5000, "emit_shape": {"kind": "rect", "size": [1800.0, 20.0]},
        "turbulence": {"strength": 25.0, "scale": 160.0, "speed": 0.3}})
    s.character("Hero", CH + "traveler.png", (0, -2, 0))
    s.character("Lamplighter", CH + "lamplighter.png", (-4.5, -2.5, 0))
    s.character("Child", CH + "child.png", (2.0, 9.0, 0))
    s.character("Guard", CH + "guard.png", (5.0, 10.5, 0), facing=-1)
    hd2d_post(s, focus_range=18.0, strength=0.06, bloom=0.45, threshold=0.9, vignette=0.55, contrast=1.06,
              saturation=0.95, tint=(0.8, 0.88, 1.0, 0.15), haze=(0.32, 0.4, 0.62, 0.6), haze_near=-60.0,
              haze_far=-400.0, haze_sky=0.2)
    chrome(s, "05", "Snowbound",
           "A village under snow at night: cold moonlight on snowy roofs, warm windows and lamps, chimney smoke, "
           "and snow falling through the light - sharp where you stand, soft in the distance.",
           "Arrows / WASD walk   Q / E turn the camera   1-5 depth of field, bloom, vignette, grade, haze   Tab captions   "
           "PgUp / PgDn scenes   H hub", "scripts/s05_snowbound.lua")
    return s.save("scenes/05-snowbound.scene")


# ---------------------------------------------------------------------------
# 06 The Battle
# ---------------------------------------------------------------------------

def broken_wall(s, name, x, y, yaw, heights, stone, stone_n, seg=1.6, thick=1.0, seed=0):
    """A ruined wall in a group of its own: segments along its x, each broken
    off at its own height, and rubble fallen at its foot."""
    import random
    r = random.Random(seed)
    g = s.group(name, (x, y, 0), yaw)
    with s.under(g):
        x0 = -seg * len(heights) / 2
        for i, h in enumerate(heights):
            a = x0 + i * seg
            s.box(f"{name}Seg{i}", a, -thick / 2, a + seg, thick / 2, 0, h, stone, stone, tile=1.4, side_n=stone_n,
                  top_n=stone_n)
        for i in range(len(heights)):
            rx, ry = x0 + r.uniform(0, seg * len(heights)), r.choice((-1, 1)) * r.uniform(thick * 0.8, thick * 1.8)
            w = r.uniform(0.35, 0.7)
            sub = s.group(f"{name}Rubble{i}", (rx, ry, 0), r.uniform(0, 1.5))
            with s.under(sub):
                s.box(f"{name}Stone{i}", -w, -w * 0.7, w, w * 0.7, 0, w * 0.9, stone, stone, tile=1.0, side_n=stone_n,
                      top_n=stone_n, cast=False)
    return g


def column(s, name, x, y, height, stone, stone_n, fallen_yaw=None):
    """A square column standing - or lying where it fell, turned `fallen_yaw`."""
    if fallen_yaw is None:
        s.box(name, x - 0.6, y - 0.6, x + 0.6, y + 0.6, 0, height, stone, stone, tile=1.0, side_n=stone_n,
              top_n=stone_n)
        return
    g = s.group(name, (x, y, 0), fallen_yaw)
    with s.under(g):
        s.box(name + "Shaft", -height / 2, -0.5, height / 2, 0.5, 0, 1.0, stone, stone, tile=1.0, side_n=stone_n,
              top_n=stone_n)


def battle():
    s = Scene("06 - The Battle")
    s.backdrop("Sky", SKY + "dusk.png", (1500, 900))
    s.backdrop("Hills", SKY + "hills_dusk.png", (1600, 480), pos=(0, 170), z=-29000)
    s.ambient(hexc("#8a80b8"), 0.52)
    # A warm key from the party's side, a cold fill from the foes', and the
    # low sun behind the ruins throwing everything's shadow toward the lens.
    s.light("KeyParty", (13.0, -1.5, 2.6), (1.0, 0.8, 0.55), intensity=1.5, radius=15.0, height=0.7)
    s.light("FillFoes", (-13.0, 1.0, 3.0), (0.55, 0.68, 1.0), intensity=1.0, radius=14.0, height=0.8)
    s.light("Rim", (1.5, 17.0, 3.2), (1.0, 0.78, 0.45), intensity=1.4, radius=17.0, height=0.5)
    s.add("Sun", directional_light={"color": hexc("#ffb070"), "intensity": 0.4, "direction": 1.75, "height": 0.25,
                                    "casts_shadows": False})
    # Turned a little west of north, so the ruins show their sides and the
    # line of battle runs across the picture rather than into it.
    s.stage(target=(0, 4.0, 1.0), yaw=-0.22, pitch=0.4, distance=44.0, zoom=48.0)
    grass, grass_n = surf("grass")
    dark, dark_n = surf("grass_dark")
    stone, stone_n = surf("moss_stone")
    rock, rock_n = surf("rock")
    cob, cob_n = surf("cobbles")
    s.floor("Ground", grass, -50, -24, 50, 70, tile=2.2, normal=grass_n, color=hexc("#d0c8b0", 1.0))
    s.floor("Moss", dark, -16, 10, 18, 22, tile=2.2, normal=dark_n, z_bias=1, color=hexc("#c8c0a8", 1.0))
    # The arena is a dais of two steps, not a rug on the grass: its edges
    # catch the light and throw a line of shade.
    for i, (x0, y0, x1, y1, z0, z1) in enumerate(((-12.5, -5.5, 12.5, 10.0, 0.0, 0.28),
                                                  (-11.2, -4.4, 11.2, 8.9, 0.28, 0.56))):
        args = dict(normal=rock_n, color=hexc("#b8b0a4", 1.0))
        s.wall(f"Step{i}S", rock, (x0, y0), (x1, y0), z0, z1 - z0, 1.2, **args)
        s.wall(f"Step{i}W", rock, (x0, y1), (x0, y0), z0, z1 - z0, 1.2, **args)
        s.wall(f"Step{i}E", rock, (x1, y0), (x1, y1), z0, z1 - z0, 1.2, **args)
        s.floor(f"Step{i}Top", cob if i else rock, x0, y0, x1, y1, tile=1.1 if i else 1.2, z=z1,
                normal=cob_n if i else rock_n, z_bias=2 + i, color=(1, 1, 1, 1) if i else hexc("#a8a098", 1.0))
    # Ruins: broken walls, an arch and columns, each turned its own way and
    # staggered in depth - groups, so a wall and its rubble turn as one.
    broken_wall(s, "WallWest", -8.0, 13.0, 0.22, (3.6, 4.8, 2.4, 1.1, 3.2), stone, stone_n, seed=1)
    broken_wall(s, "WallEast", 14.5, 7.0, 1.3, (2.6, 4.0, 1.6, 3.0), stone, stone_n, seed=2)
    broken_wall(s, "WallFar", -2.0, 25.0, -0.12, (6.0, 7.5, 5.2, 3.0, 6.4, 4.4), stone, stone_n, seg=2.0, seed=3)
    arch = s.group("Arch", (3.5, 15.5, 0), -0.3)
    with s.under(arch):
        for i, x in enumerate((-2.4, 2.4)):
            s.box(f"ArchPier{i}", x - 0.7, -0.7, x + 0.7, 0.7, 0, 5.4, stone, stone, tile=1.4, side_n=stone_n,
                  top_n=stone_n)
        s.box("ArchLintel", -3.3, -0.8, 3.3, 0.8, 5.4, 6.4, stone, stone, tile=1.4, side_n=stone_n, top_n=stone_n)
    for i, (x, y, h) in enumerate(((-13.5, 9.0, 6.2), (10.5, 12.0, 3.0), (-4.5, 19.0, 2.2), (9.0, 21.0, 7.4),
                                   (-16.0, 18.0, 5.0))):
        column(s, f"Column{i}", x, y, h, stone, stone_n)
    column(s, "Fallen0", -15.0, 1.5, 4.0, stone, stone_n, fallen_yaw=0.7)
    column(s, "Fallen1", 15.5, -2.0, 3.2, stone, stone_n, fallen_yaw=-0.4)
    for i, (x, y, k) in enumerate(((-20, 16, "oak_autumn"), (-15, 28, "pine"), (19, 15, "oak"), (14, 30, "pine"),
                                   (-6, 32, "oak_autumn"), (6, 36, "oak"), (-24, 6, "oak"), (24, 4, "oak_autumn"))):
        card(s, f"Tree{i}", k, x, y, scale=2.6)
    # Banners planted at the dais's back corners: the party's red, the foes' blue.
    card(s, "BannerParty", "banner", 11.6, 9.4, scale=1.9)
    card(s, "BannerFoes", "banner_blue", -11.6, 9.4, scale=1.9)
    # The party on the right, facing left; the monsters on the left facing
    # right - the classic battle line, drawn on the diagonal, on the dais.
    top = 0.56
    for i, (name, x, y) in enumerate((("traveler", 5.0, -1.5), ("cleric", 6.5, 1.0), ("hunter", 8.0, 3.5),
                                      ("scholar", 9.5, 6.0))):
        s.character(f"Party{i}", CH + name + ".png", (x, y, top), facing=-1)
    mons = (("frog", -6.0, -0.5, 56, 48), ("golem", -8.5, 3.5, 64, 72), ("wraith", -5.5, 6.0, 48, 64))
    for i, (kind, x, y, w, h) in enumerate(mons):
        n = 4
        frames = lambda idx: [{"uv_rect": [k / n, 0.0, (k + 1) / n, 1.0], "triggers": []} for k in idx]
        clips = {"idle": {"name": "idle", "frame_duration": 0.5, "looping": True, "property_tracks": [],
                          "frames": frames([0, 1])},
                 "attack": {"name": "attack", "frame_duration": 0.5, "looping": False, "property_tracks": [],
                            "frames": frames([2])},
                 "hurt": {"name": "hurt", "frame_duration": 0.12, "looping": False, "property_tracks": [],
                          "frames": frames([3, 0, 3, 0])}}
        e = s.billboard(f"Foe{i}", CH + kind + ".png", (x, y, top), (w * PIXEL * 1.7, h * PIXEL * 1.7),
                        anchor=(0.5, 0.03), uv=[0, 0, 1 / n, 1], scale=(-1, 1),
                        shadow={"height": h * PIXEL * 1.7 / 60.0 * 0.95},
                        extra={"sprite_animator": {"clips": clips, "current_clip": "idle", "playing": True,
                                                   "speed": 1.0}})
        s.blob(f"Foe{i}Shadow", size=(2.0, 0.9), follow=e)
        # A spell's burst at each foe, fired from the script.
        s.emitter(f"Foe{i}Burst", (x, y, top + 1.2), {
            "texture_id": FX + "mote.png", "emission_rate": 0.0, "max_particles": 200, "lifetime": [0.4, 1.0],
            "speed": [80.0, 260.0], "angle": [0.0, 6.28318], "color_start": [6.0, 3.0, 1.0, 1.0],
            "color_end": [2.0, 0.4, 0.1, 0.0], "size_start": [4.0, 8.0], "size_end": [1.0, 2.0],
            "gravity_modifier": 0.3, "angular_velocity": [0.0, 0.0], "burst_count": 0, "active": True,
            "world_space": True, "z_order": 0, "emit_shape": {"kind": "circle", "radius": 12.0}, "drag": 2.0,
            "blend_mode": "Additive"}, z_bias=40)
        s.light(f"Foe{i}Flash", (x, y, top + 1.5), (1.0, 0.6, 0.3), intensity=0.0, radius=9.0, height=0.6)
    for i, (x, y) in enumerate(((-10.4, -3.6), (10.4, -3.6))):
        flame(s, f"Brazier{i}", (x, y, top + 1.2), size=1.4, radius=8.0)
        card(s, f"BrazierStand{i}", "torch_bracket", x, y, z=top, scale=2.0)
    # The foreground: long grass at the lens, which depth of field blurs -
    # the frame HD-2D puts round its picture.
    import random
    r = random.Random(6)
    for i in range(22):
        x = -19 + i * 1.8 + r.uniform(-0.6, 0.6)
        y = -8.6 + r.uniform(-1.0, 1.2)
        if -6 < x < 6 and y > -8.4:
            y -= 1.0
        kind = "grass_tuft_dry" if r.random() < 0.35 else "grass_tuft"
        card(s, f"Tuft{i}", kind, x, y, scale=r.uniform(1.7, 2.5))
    for i, (x, y) in enumerate(((-17.5, -6.0), (16.5, -6.5), (-13.0, 12.0), (12.5, 16.0))):
        card(s, f"Bush{i}", "bush_berries" if i % 2 else "bush", x, y, scale=2.4)
    motes(s, "Embers", 0, 3, 2.0, (1.6, 0.9, 0.4, 0.6), rate=8.0, spread=560.0)
    hd2d_post(s, focus_range=20.0, strength=0.06, max_blur=9.0, bloom=0.45, threshold=0.9, vignette=0.55,
              contrast=1.1, saturation=1.12, tint=(1.0, 0.9, 0.8, 0.12), haze=(0.5, 0.42, 0.58, 0.55),
              haze_near=-70.0, haze_far=-320.0, haze_sky=0.2)
    chrome(s, "06", "The Battle",
           "A side-on battle view: the party on the right, three monsters on the left, on a dais in a ruin. "
           "Space casts the next spell - fire, frost, light - and its flash lights the whole stage.",
           "Space cast   1-5 depth of field, bloom, vignette, grade, haze   Tab captions   PgUp / PgDn scenes   H hub",
           "scripts/s06_the_battle.lua")
    return s.save("scenes/06-the-battle.scene")


# ---------------------------------------------------------------------------
# The hub
# ---------------------------------------------------------------------------

HUB_ROWS = 6


def hub():
    s = Scene("Visual Showcase 2 - HD-2D")
    s.backdrop("Sky", SKY + "night.png", (1500, 900))
    s.backdrop("Stars", SKY + "stars.png", (1500, 760), pos=(0, 60), z=-29500, color=(1, 1, 1, 0.8))
    s.backdrop("Hills", SKY + "hills_night.png", (1600, 480), pos=(0, 60), z=-29000)
    s.ambient(hexc("#4a5a9a"), 0.35)
    s.stage(target=(0, 2, 0.8), yaw=0.0, pitch=0.42, distance=60.0, zoom=40.0)
    cob, cob_n = surf("cobbles")
    grass, grass_n = surf("grass_dark")
    s.floor("Meadow", grass, -40, -30, 40, 60, tile=2.4, normal=grass_n)
    s.floor("Plaza", cob, -6, -4, 6, 6, tile=1.6, normal=cob_n, z_bias=1)
    for i, (x, y) in enumerate(((-5, -3), (5, -3), (-5, 5), (5, 5))):
        lamp_post(s, f"Lamp{i}", x, y)
    for i in range(10):
        a = i / 10 * 2 * math.pi
        card(s, f"Tree{i}", "pine" if i % 2 else "oak", math.cos(a) * 14, 8 + math.sin(a) * 10, scale=2.0)
    s.character("Hero", CH + "traveler.png", (0, 0, 0))
    motes(s, "Fireflies", 0, 2, 1.5, (1.4, 1.2, 0.5, 0.9), rate=6.0, spread=500.0)
    hd2d_post(s, focus_range=18.0, tint=(0.85, 0.9, 1.0, 0.1))
    s.text("HubTitle", "Visual Showcase 2   -   HD-2D", "TopCenter", (0, -40), 40, hexc("#f2e2c0", 1.0),
           align="Center", width=1100)
    s.text("HubSub", "pixel people in a world drawn in perspective, lit and focused like a lens",
           "TopCenter", (0, -96), 17, hexc("#a8b0d0", 1.0), align="Center", width=1100)
    for i in range(HUB_ROWS):
        s.text(f"Row{i + 1}", "", "TopLeft", (120, -170 - i * 34), 22, hexc("#9aa0b8", 1.0), width=600)
    s.text("Blurb", "", "TopRight", (-110, -176), 16, hexc("#d8d4e4", 1.0), width=470)
    s.text("Hint", "Up / Down choose   Enter go   PgUp / PgDn step through the scenes", "BottomCenter", (0, 28), 15,
           hexc("#b8b8c8", 1.0), align="Center", width=1100)
    s.script("Driver", "scripts/hub.lua")
    return s.save("scenes/hub.scene")


def main():
    hub()
    lantern_square()
    crystal_hollow()
    forest_shrine()
    tavern()
    snowbound()
    battle()


if __name__ == "__main__":
    main()
