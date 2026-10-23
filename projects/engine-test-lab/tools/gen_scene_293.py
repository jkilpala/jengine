"""Scene 293: a tilemap under a parent, tall tiles on a square map, and a
mesh tinted and re-ordered by the sprite calls (Oct 2026).

Left: a raft - an entity the script sways side to side - carrying a square
tilemap of grass and a flag sprite as its children. The map has to sway with
the raft, flag and all; a faint frame marks where it would hang at its local
offset alone, which is where every tilemap reader used to put it. Its trees
are pictures twice as tall as a cell and must stand on their cells, rising
into the row above, not be squashed into one cell.

Right: a checkered `mesh` whose tint `Sprite.set_color` cycles, and a card
sprite at z 5 that `Sprite.set_z_order` puts the mesh behind (z 3) and in
front of (z 7) once a second.

Regenerate: python projects/engine-test-lab/tools/gen_scene_293.py
"""
import json
import math
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
CELL = 48.0
COLS, ROWS = 6, 4
# The map's offset from the raft, and the raft's middle.
LOCAL = (-COLS * CELL / 2, ROWS * CELL / 2)
RAFT_X, RAFT_Y = -300.0, 10.0


def textures():
    out = ASSETS / "sprites"
    grass = Image.new("RGBA", (16, 16), (70, 140, 60, 255))
    d = ImageDraw.Draw(grass)
    for x, y in ((3, 4), (10, 2), (6, 11), (13, 9)):
        d.line((x, y, x, y + 2), fill=(110, 185, 80, 255))
    grass.save(out / "lab293_grass.png")
    tree = Image.new("RGBA", (16, 32), (0, 0, 0, 0))
    d = ImageDraw.Draw(tree)
    d.rectangle((7, 22, 8, 31), fill=(110, 70, 40, 255))
    d.ellipse((1, 2, 14, 24), fill=(30, 100, 50, 255))
    d.ellipse((4, 5, 9, 11), fill=(60, 140, 70, 255))
    tree.save(out / "lab293_tree.png")
    frame = Image.new("RGBA", (32, 32), (0, 0, 0, 0))
    ImageDraw.Draw(frame).rectangle((0, 0, 31, 31), outline=(255, 120, 120, 255), width=1)
    frame.save(out / "lab293_frame.png")


def at(x, y):
    return {"position": [x, y], "rotation": 0.0, "scale": [1.0, 1.0]}


def tileset(texture, w, h, first):
    return {"texture_id": texture, "tile_width": w, "tile_height": h, "columns": 1, "tile_count": 1,
            "first_gid": first, "image_width": w, "image_height": h}


def raft():
    ground = [1] * (COLS * ROWS)
    trees = [0] * (COLS * ROWS)
    for c, r in ((1, 1), (4, 1), (2, 3), (5, 3)):
        trees[r * COLS + c] = 2
    return [
        {"name": "Raft", "components": {"transform": at(RAFT_X, RAFT_Y)}},
        {"name": "Deck", "parent": "Raft", "components": {
            "transform": at(*LOCAL),
            "tilemap": {"tile_size": [CELL, CELL], "z_order": 0,
                        "tilesets": [tileset("sprites/lab293_grass.png", 16, 16, 1),
                                     tileset("sprites/lab293_tree.png", 16, 32, 2)],
                        "layers": [{"name": "ground", "tiles": ground, "width": COLS, "height": ROWS,
                                    "visible": True, "collision": False},
                                   {"name": "trees", "tiles": trees, "width": COLS, "height": ROWS,
                                    "visible": True, "collision": False}]}}},
        {"name": "Flag", "parent": "Raft", "components": {
            "transform": at(0.0, 0.0),
            "sprite": {"texture_id": "sprites/circle16.png", "size": [28.0, 28.0],
                       "color": [1.0, 0.85, 0.2, 1.0], "z_order": 10}}},
        # Where the map would hang at its local offset alone - about the origin.
        {"name": "Ghost", "components": {
            "transform": at(LOCAL[0] + COLS * CELL / 2, LOCAL[1] - ROWS * CELL / 2),
            "sprite": {"texture_id": "sprites/lab293_frame.png", "size": [COLS * CELL, ROWS * CELL],
                       "color": [1.0, 1.0, 1.0, 0.5], "z_order": -5}}},
    ]


def quad(half):
    corners = [(-half, half, 0.0, 0.0), (half, half, 1.0, 0.0), (half, -half, 1.0, 1.0), (-half, -half, 0.0, 1.0)]
    return [{"position": [x, y], "uv": [u, v], "color": [1.0, 1.0, 1.0, 1.0], "q": 1.0} for x, y, u, v in corners]


def meshes():
    return [
        {"name": "Tinted", "components": {
            "transform": at(300.0, 20.0),
            "mesh": {"texture_id": "sprites/checker64.png", "z_order": 3, "vertices": quad(110.0),
                     "indices": [0, 1, 2, 0, 2, 3]}}},
        {"name": "Card", "components": {
            "transform": at(390.0, -40.0),
            "sprite": {"texture_id": "", "size": [150.0, 150.0], "color": [0.85, 0.3, 0.35, 1.0], "z_order": 5}}},
    ]


def label(name, x, text):
    return {"name": name, "components": {
        "transform": at(x, -235.0),
        "text": {"text": text, "font_size": 18.0, "color": [0.85, 0.88, 0.95, 1.0], "align": "center"}}}


def ui_text(name, offset_y, h, text, size, color):
    return {"name": name, "components": {
        "ui_element": {"anchor": "TopCenter", "offset": [0.0, offset_y], "size": [1250.0, h], "z_order": 100},
        "ui_text": {"text": text, "font_size": size, "color": color, "alignment": "Center"}}}


def main():
    textures()
    scene = {"name": "293 - Tilemap under a parent, mesh tint", "entities": [
        {"name": "Camera", "components": {
            "transform": at(0.0, 0.0),
            "camera": {"zoom": 1.0, "is_active": True, "viewport_width": 1280.0, "viewport_height": 720.0}}},
        *raft(),
        *meshes(),
        label("RaftLabel", -300.0, "the map sways with its raft; trees stand two cells tall; the red frame is its local offset"),
        label("MeshLabel", 300.0, "the mesh's tint cycles and it passes behind / in front of the card"),
        ui_text("Title", -30.0, 44.0, "293 - Tilemap under a parent, mesh tint", 30.0, [0.85, 0.88, 0.95, 1]),
        ui_text("Hint", -80.0, 24.0,
                "[H/Esc] hub   left: map, trees and flag move together, never in the frame; right: colour cycles, z swaps each second",
                15.0, [0.6, 0.65, 0.75, 1]),
        {"name": "Nav", "components": {
            "transform": at(0.0, 0.0),
            "script": {"language": "Lua", "source_path": "scripts/lua/scene_293_tilemap_parent.lua"}}},
    ]}
    path = ASSETS / "scenes" / "293-tilemap-parent.scene"
    path.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8", newline="\n")
    print("wrote", path.relative_to(ROOT))


if __name__ == "__main__":
    main()
