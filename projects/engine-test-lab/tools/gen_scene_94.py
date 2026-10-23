r"""Write scene 94 - a colliding tilemap big enough for its cost to matter.

Generated rather than hand-authored because the point of the scene is the
*size*: 160 x 90 is 14,400 tiles, and a JSON array of 14,400 entries is not
something to maintain by hand. Every other test-lab scene is hand-written.

## What the scene is for

`PhysicsWorld::sync_tilemap_colliders` used to hash every tile of every
colliding layer **every frame** to decide whether to rebuild, and then rebuild
the entire body on any difference. The tests measure that
(`crates/jengine-core/tests/tilemap_colliders_rebuild_per_chunk.rs`); this
scene is where a person can see it, which is a different thing:

- the frame time stays flat while digging, and
- a crate resting on rock that is dug out **falls**, which is the part a
  rebuild can get wrong rather than slow.

A scene half this size would run acceptably even on the old code, so the size
is load-bearing. Keep it.

Re-run after editing here:
    python tools/gen_scene_94.py      (from the project directory)
"""
import json
import math
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SCENE = ROOT / "assets" / "scenes" / "94-tilemap-chunks.scene"

W, H = 160, 90
TILE = 16.0
# Origin top-left, so row 0 is the top: the map spans x -1280..1280 and
# y 720..-720, which is the whole view at zoom 0.5.
ORIGIN = (-W * TILE / 2.0, H * TILE / 2.0)


def surface(x):
    """A hilly surface, in rows from the top."""
    return int(34 + 9 * math.sin(x / 19.0) + 4 * math.sin(x / 6.3) + 2 * math.sin(x / 2.7))


def solid(x, y):
    if y < surface(x):
        return False
    # Caves, so the merge has runs of every length to make and the map is
    # not one enormous rectangle.
    for cx, cy, r in ((38, 62, 11), (96, 54, 9), (128, 70, 13), (20, 78, 8), (70, 80, 7)):
        if (x - cx) ** 2 + ((y - cy) * 1.6) ** 2 < r * r:
            return False
    return True


def tf(x, y):
    return {"position": [x, y], "rotation": 0.0, "scale": [1.0, 1.0]}


def text(y, s, scale=0.9, color=(0.75, 0.78, 0.85, 1), z=100):
    return {
        "name": "Hint",
        "components": {
            "transform": tf(0.0, y),
            "text": {
                "text": s, "font_id": "", "font_scale": scale, "color": list(color),
                "z_order": z, "align": "center", "wrap_width": 0.0,
            },
        },
    }


def crate(x, y):
    """A dynamic body resting on the rock. Dig under one and it must fall -
    rapier does not wake a sleeping body when the collider it was resting on
    is removed unless the removal asks for it."""
    return {
        "name": "Crate",
        "components": {
            "transform": tf(x, y),
            "sprite": {
                "texture_id": "sprites/tiles4.png", "size": [26.0, 26.0],
                "color": [1.0, 0.82, 0.45, 1.0], "z_order": 5,
            },
            "rigid_body": {
                "body_type": "Dynamic", "mass": 4.0, "gravity_scale": 1.0,
                "linear_damping": 0.0, "angular_damping": 0.0, "fixed_rotation": False,
            },
            # The shape enum is externally tagged, so it is `{"Box": {..}}` -
            # not a `"type"` field. The scene guard catches the other spelling.
            "collider": {
                "shape": {"Box": {"width": 26.0, "height": 26.0}},
                "offset": [0.0, 0.0], "is_sensor": False,
                "friction": 0.9, "restitution": 0.0, "density": 1.0,
            },
        },
    }


def main():
    tiles = [1 if solid(i % W, i // W) else None for i in range(W * H)]
    filled = sum(1 for t in tiles if t)

    entities = [
        {
            "name": "Camera",
            "components": {
                "transform": tf(0.0, 0.0),
                "camera": {"zoom": 0.5, "is_active": True, "viewport_width": 1280.0, "viewport_height": 720.0},
            },
        },
        text(690.0, "94 - A tilemap costs what you changed", 2.0, (1, 1, 1, 1)),
        text(630.0, f"{W} x {H} = {W * H:,} tiles, {filled:,} of them solid. "
                    "The colliders are built and rebuilt per 32x32 chunk."),
        text(596.0, "Hold [LMB] to dig, [RMB] to fill. Watch the frame time: it must not move. "
                    "Digging under a crate must drop it."),
        text(562.0, "Before this, the sync hashed every tile every frame (966us for a million) "
                    "and one changed tile rebuilt the whole map.", 0.85, (0.6, 0.62, 0.7, 1)),
    ]

    entities.append({
        "name": "Rock",
        "components": {
            "transform": tf(*ORIGIN),
            "tilemap": {
                "tile_size": [TILE, TILE],
                "layers": [{
                    "name": "rock", "tiles": tiles, "width": W, "height": H,
                    "visible": True, "collision": False,
                }],
                "tilesets": [{
                    "texture_id": "sprites/tiles4.png", "tile_width": 32, "tile_height": 32,
                    "columns": 2, "tile_count": 4, "first_gid": 1,
                    "image_width": 64, "image_height": 64,
                }],
                "source": None, "z_order": -10, "sort_layer": None,
            },
            "tilemap_collider": {"solid": ["rock"], "one_way": []},
        },
    })

    # Crates sitting on the surface, spread across several chunks so that
    # whichever one you dig under, a different chunk is the one rebuilt.
    for x in (24, 56, 88, 112, 140):
        wx = ORIGIN[0] + (x + 0.5) * TILE
        wy = ORIGIN[1] - (surface(x) - 1) * TILE
        entities.append(crate(wx, wy))

    entities += [
        {
            "name": "Readout",
            "components": {
                "transform": tf(0.0, -600.0),
                "text": {
                    "text": "", "font_id": "", "font_scale": 1.1, "color": [1, 0.85, 0.4, 1],
                    "z_order": 100, "align": "center", "wrap_width": 0.0,
                },
            },
        },
        text(-660.0, "[LMB] dig   [RMB] fill   [R] refill the map   [H/Esc] hub   [PgUp/PgDn] prev/next",
             0.85, (0.6, 0.62, 0.7, 1)),
        {
            "name": "Runner",
            "components": {
                "transform": tf(0.0, 0.0),
                "script": {"language": "Lua", "source_path": "scripts/lua/scene_94_tilemap_chunks.lua"},
            },
        },
    ]

    scene = {"name": "94 - Tilemap chunks", "entities": entities}
    # Indented JSON puts every array element on its own line, which for 14,400
    # tiles is a 14,400-line file. The tiles go in as a placeholder and come
    # back as one line per tile *row*, so the scene is still readable and the
    # map is legible in a diff.
    placeholder = "__TILES__"
    for e in scene["entities"]:
        if "tilemap" in e["components"]:
            e["components"]["tilemap"]["layers"][0]["tiles"] = placeholder
    out = json.dumps(scene, indent=1)
    rows = ",\n     ".join(
        ", ".join("1" if t else "null" for t in tiles[y * W:(y + 1) * W]) for y in range(H)
    )
    out = out.replace(f'"{placeholder}"', "[\n     " + rows + "\n    ]")
    SCENE.write_text(out + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {SCENE.name}: {W}x{H} = {W * H} tiles, {filled} solid, "
          f"{(W + 31) // 32}x{(H + 31) // 32} = {((W + 31) // 32) * ((H + 31) // 32)} chunks, "
          f"{SCENE.stat().st_size // 1024} KB")


if __name__ == "__main__":
    main()
