"""Scene 292: tileset margin and spacing (Oct 2026).

Two tilemaps of the same two tiles - three rows of sand over three of blue - both filtered linearly
and drawn three times their size. The left sheet packs the tiles edge to edge,
so a linearly filtered tile samples its neighbour just past its edge: a faint
line of the other colour runs down every seam between two cells of one colour. The right sheet rings each
tile with a 2 px gutter of its own edge pixels (`margin` 2, `spacing` 4), so
the cells meet cleanly - and if `margin` / `spacing` were ignored, its tiles
would be visibly shifted onto the gutters.

Regenerate: python projects/engine-test-lab/tools/gen_scene_292.py
"""
import json
from pathlib import Path

from PIL import Image, ImageDraw

ROOT = Path(__file__).resolve().parent.parent
ASSETS = ROOT / "assets"
PX = 16
GUTTER = 2


def tile(fill, mark):
    img = Image.new("RGBA", (PX, PX), fill)
    d = ImageDraw.Draw(img)
    d.rectangle((5, 5, 10, 10), fill=mark)
    return img


def sheets():
    a = tile((236, 214, 140, 255), (120, 90, 40, 255))   # sand
    b = tile((40, 50, 120, 255), (140, 170, 255, 255))   # deep blue
    packed = Image.new("RGBA", (2 * PX, PX))
    packed.paste(a, (0, 0))
    packed.paste(b, (PX, 0))
    g = GUTTER
    spaced = Image.new("RGBA", (2 * (PX + 2 * g), PX + 2 * g), (255, 0, 255, 255))
    for i, t in enumerate((a, b)):
        x, y = i * (PX + 2 * g) + g, g
        spaced.paste(t, (x, y))
        spaced.paste(t.crop((0, 0, PX, 1)).resize((PX, g)), (x, y - g))
        spaced.paste(t.crop((0, PX - 1, PX, PX)).resize((PX, g)), (x, y + PX))
        spaced.paste(t.crop((0, 0, 1, PX)).resize((g, PX)), (x - g, y))
        spaced.paste(t.crop((PX - 1, 0, PX, PX)).resize((g, PX)), (x + PX, y))
        for cx, cy, px, py in ((0, 0, x - g, y - g), (PX - 1, 0, x + PX, y - g),
                               (0, PX - 1, x - g, y + PX), (PX - 1, PX - 1, x + PX, y + PX)):
            spaced.paste(Image.new("RGBA", (g, g), t.getpixel((cx, cy))), (px, py))
    out = ASSETS / "sprites" / "filter_linear"
    packed.save(out / "tiles_packed.png")
    spaced.save(out / "tiles_spaced.png")
    return spaced.size


def tilemap_entity(name, x, texture, spaced_size=None):
    w, h = 6, 6
    tiles = [1 if r < h // 2 else 2 for r in range(h) for c in range(w)]
    ts = {"texture_id": texture, "tile_width": PX, "tile_height": PX, "columns": 2, "tile_count": 2,
          "first_gid": 1, "image_width": 2 * PX, "image_height": PX}
    if spaced_size:
        ts.update({"image_width": spaced_size[0], "image_height": spaced_size[1],
                   "margin": GUTTER, "spacing": 2 * GUTTER})
    cell = 48.0
    return {"name": name, "components": {
        "transform": {"position": [x - w * cell / 2, h * cell / 2 - 40.0], "rotation": 0.0, "scale": [1.0, 1.0]},
        "tilemap": {"tile_size": [cell, cell], "z_order": 0, "tilesets": [ts],
                    "layers": [{"name": "g", "tiles": tiles, "width": w, "height": h,
                                "visible": True, "collision": False}]}}}


def label(name, x, text):
    return {"name": name, "components": {
        "transform": {"position": [x, -235.0], "rotation": 0.0, "scale": [1.0, 1.0]},
        "text": {"text": text, "font_size": 18.0, "color": [0.85, 0.88, 0.95, 1.0], "align": "center"}}}


def ui_text(name, offset_y, h, text, size, color):
    return {"name": name, "components": {
        "ui_element": {"anchor": "TopCenter", "offset": [0.0, offset_y], "size": [1250.0, h], "z_order": 100},
        "ui_text": {"text": text, "font_size": size, "color": color, "alignment": "Center"}}}


def main():
    spaced = sheets()
    scene = {"name": "292 - Tileset margin and spacing", "entities": [
        {"name": "Camera", "components": {
            "transform": {"position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0]},
            "camera": {"zoom": 1.0, "is_active": True, "viewport_width": 1280.0, "viewport_height": 720.0}}},
        tilemap_entity("Packed", -260.0, "sprites/filter_linear/tiles_packed.png"),
        tilemap_entity("Spaced", 260.0, "sprites/filter_linear/tiles_spaced.png", spaced),
        label("PackedLabel", -260.0, "packed edge to edge: a thin line of the other tile at every seam"),
        label("SpacedLabel", 260.0, "margin 2, spacing 4, gutters of each tile's own edge: clean seams"),
        ui_text("Title", -30.0, 44.0, "292 - Tileset margin and spacing", 30.0, [0.85, 0.88, 0.95, 1]),
        ui_text("Hint", -80.0, 24.0, "[H/Esc] hub   both maps are linear-filtered and drawn 3x; the right one must show no magenta and no shifted tiles",
                15.0, [0.6, 0.65, 0.75, 1]),
        {"name": "Nav", "components": {
            "transform": {"position": [0.0, 0.0], "rotation": 0.0, "scale": [1.0, 1.0]},
            "script": {"language": "Lua", "source_path": "scripts/lua/scene_292_tile_spacing.lua"}}},
    ]}
    path = ASSETS / "scenes" / "292-tile-spacing.scene"
    path.write_text(json.dumps(scene, indent=1) + "\n", encoding="utf-8", newline="\n")
    print("wrote", path.relative_to(ROOT))


if __name__ == "__main__":
    main()
