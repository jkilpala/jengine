#!/usr/bin/env python3
"""Regenerate the built-in font subset embedded in every jEngine binary.

`renderer::text::built_in_font_atlas()` rasterizes the same ranges this
script cuts, but the full Noto Sans Regular it was reading is
607 KB — the other ~3000 glyphs shipped in every native binary and in the
wasm, where they were a quarter of the compressed payload.

This produces `NotoSans-Regular-ascii.ttf`, which is what the engine embeds.
`NotoSans-Regular.ttf` stays in the tree as the source to re-cut from if the
glyph range ever widens (if it does, update BOTH the range here and the loop
in `built_in_font_atlas`).

    pip install fonttools
    python crates/jengine-core/assets/fonts/subset_builtin_font.py

Verify the result renders identically with:

    cargo run -p jengine-core --features gpu-wgpu --example font_atlas_digest
"""

import pathlib
import sys

from fontTools import subset

HERE = pathlib.Path(__file__).parent
SOURCE = HERE / "NotoSans-Regular.ttf"
OUTPUT = HERE / "NotoSans-Regular-ascii.ttf"

# Must match the ranges in `built_in_font_atlas`.
#
# ASCII, plus Latin-1 Supplement and Latin Extended-A so Western and Central
# European text renders. Greek/Cyrillic/CJK are deliberately out: each is a
# large size jump for a font meant to be a sane default, not a universal one.
RANGES = [
    (0x20, 0x7E),   # ASCII
    (0xA0, 0xFF),   # Latin-1 Supplement - accents, guillemets, currency
    (0x100, 0x17F), # Latin Extended-A - Polish, Czech, Hungarian, Turkish
]


def main() -> int:
    if not SOURCE.is_file():
        print(f"missing source font: {SOURCE}", file=sys.stderr)
        return 1

    options = subset.Options()
    # Keep the font simple and small: no layout tables, no hinting programs -
    # the rasterizer (fontdue) uses outlines and horizontal metrics only.
    options.layout_features = []
    options.hinting = False
    options.desubroutinize = True
    options.drop_tables += ["GSUB", "GPOS", "GDEF", "DSIG"]
    options.name_IDs = [1, 2]
    options.notdef_outline = True
    options.recalc_bounds = True

    font = subset.load_font(str(SOURCE), options)
    subsetter = subset.Subsetter(options=options)
    codepoints = [cp for first, last in RANGES for cp in range(first, last + 1)]
    subsetter.populate(unicodes=codepoints)
    subsetter.subset(font)
    subset.save_font(font, str(OUTPUT), options)

    before = SOURCE.stat().st_size
    after = OUTPUT.stat().st_size
    ranges = ", ".join(f"U+{a:04X}..U+{b:04X}" for a, b in RANGES)
    print(
        f"{SOURCE.name} {before / 1024:.0f} KB -> {OUTPUT.name} {after / 1024:.0f} KB "
        f"({100 * after / before:.1f}%), {len(codepoints)} codepoints: {ranges}"
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
