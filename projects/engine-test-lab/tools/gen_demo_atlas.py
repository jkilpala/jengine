#!/usr/bin/env python3
"""Regenerate `assets/sprites/_atlases/demo.png` from `demo.atlas`.

Scene 15 draws every region of this atlas three ways: authored `uv_rect`s,
`set_sprite_region` from Lua, and animation frames naming a region. That only
tests the engine if the image actually has art where the metadata says it does.

The checked-in PNG did not: tiles 3 and 6 were blank and slot 7 carried a "3",
so three of the eight sprites drew nothing and one drew the wrong digit. It read
as a renderer bug, which is the opposite of what a QA asset is for.

So the image is generated from the `.atlas` rather than kept in step by hand:
each region gets a flat colour, a white border and its index, drawn at the
coordinates the metadata declares. A region added to the `.atlas` shows up here
on the next run.

    python tools/gen_demo_atlas.py

Needs Pillow. Digits are drawn from a tiny built-in 3x5 bitmap, so no font file
has to exist on the machine.
"""

import json
import pathlib

from PIL import Image

HERE = pathlib.Path(__file__).resolve().parent
ATLAS = HERE.parent / "assets" / "sprites" / "_atlases" / "demo.atlas"
OUT = ATLAS.with_suffix(".png")

# One per region index, cycled if the atlas grows.
COLOURS = [
    (200, 40, 50),
    (40, 160, 70),
    (40, 70, 200),
    (225, 180, 30),
    (190, 50, 180),
    (30, 170, 170),
    (235, 120, 40),
    (110, 90, 200),
]

# 3x5 digits, one string per row, '#' = ink.
DIGITS = {
    "0": ["###", "# #", "# #", "# #", "###"],
    "1": [" # ", "## ", " # ", " # ", "###"],
    "2": ["###", "  #", "###", "#  ", "###"],
    "3": ["###", "  #", "###", "  #", "###"],
    "4": ["# #", "# #", "###", "  #", "  #"],
    "5": ["###", "#  ", "###", "  #", "###"],
    "6": ["###", "#  ", "###", "# #", "###"],
    "7": ["###", "  #", "  #", "  #", "  #"],
    "8": ["###", "# #", "###", "# #", "###"],
    "9": ["###", "# #", "###", "  #", "###"],
}


def draw_digit(img: Image.Image, text: str, cx: int, cy: int, scale: int) -> None:
    """Draw `text` centred on (cx, cy) in black, `scale` pixels per cell."""
    glyphs = [DIGITS[ch] for ch in text if ch in DIGITS]
    if not glyphs:
        return
    gap = scale
    width = sum(len(g[0]) * scale for g in glyphs) + gap * (len(glyphs) - 1)
    height = 5 * scale
    x = cx - width // 2
    top = cy - height // 2
    for glyph in glyphs:
        for row, line in enumerate(glyph):
            for col, cell in enumerate(line):
                if cell != "#":
                    continue
                x0 = x + col * scale
                y0 = top + row * scale
                for py in range(y0, y0 + scale):
                    for px in range(x0, x0 + scale):
                        if 0 <= px < img.width and 0 <= py < img.height:
                            img.putpixel((px, py), (20, 20, 25, 255))
        x += len(glyph[0]) * scale + gap


def main() -> None:
    atlas = json.loads(ATLAS.read_text(encoding="utf-8"))
    img = Image.new("RGBA", (atlas["width"], atlas["height"]), (0, 0, 0, 0))

    for i, region in enumerate(atlas["regions"]):
        x, y = region["x"], region["y"]
        w, h = region["width"], region["height"]
        colour = COLOURS[i % len(COLOURS)] + (255,)
        for py in range(y, y + h):
            for px in range(x, x + w):
                # A 2px white border, so a region drawn one pixel off its rect
                # is visible rather than merely wrong-coloured.
                edge = px - x < 2 or py - y < 2 or x + w - px <= 2 or y + h - py <= 2
                img.putpixel((px, py), (245, 245, 250, 255) if edge else colour)
        # The index, not the region name: the name is `tile_3`, and what scene
        # 15 needs to show at a glance is which slot you are looking at.
        draw_digit(img, str(i), x + w // 2, y + h // 2, max(2, min(w, h) // 10))

    img.save(OUT)
    print(f"wrote {OUT.relative_to(HERE.parent)} ({img.width}x{img.height}, "
          f"{len(atlas['regions'])} regions)")


if __name__ == "__main__":
    main()
