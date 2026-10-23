"""Build the brand kit's generated files from the editor's tokens.

    python docs/brand/build_brand.py

Reads `editor/src/theme.css` - the one source of the palette values - and
writes, under `docs/brand/`:

  tokens.json               every token, per palette and mode, plus the pixel ramps
  palettes/*.gpl            each pixel ramp as a GIMP / Aseprite palette, in slot order
  img/palettes.svg          the token board
  img/pixel-ramp.svg        the twelve pixel slots in all six palettes
  img/motifs.svg            the sun, the city, the horizon and the grid floor
  img/pixel-icons.svg       the sample icons, in all six palettes
  pixel-icons/*.png         the sample icons as indexed 16x16 PNGs (Pillow, if installed)

The sample icons are drawn as text in `pixel-icons/*.txt`, one character a
pixel (the slot letters are in README.md, "Pixel icons"). Rerun after
changing theme.css or an icon; the output is deterministic.
"""

from __future__ import annotations

import json
import re
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
THEME = ROOT / "editor" / "src" / "theme.css"
SITE = ROOT / "website" / "src" / "styles" / "palettes.css"

# Editor token -> the website's name for the same colour. The two files must
# agree on these; `check_site` fails the build when they do not.
SHARED = {
    "base": "bg", "raised": "panel", "line": "line", "ink": "ink", "text": "text",
    "muted": "muted", "accent": "accent", "accent-low": "accent-low",
    "accent-high": "accent-high", "a2": "a2", "a3": "a3",
}

PALETTES = [("dusk", "MegaTokyo Dusk"), ("sabers", "Knight Sabers"), ("genom", "GENOM Tower")]
MODES = ["dark", "light"]

TOKENS = [
    "crust", "mantle", "base", "raised", "hover", "line", "line-strong",
    "ink", "text", "muted", "faint",
    "accent", "accent-low", "accent-high", "a2", "a3",
    "ok", "warn", "err", "on-accent", "sun1", "sun2",
    "cat-1", "cat-2", "cat-3", "cat-4", "cat-5", "cat-6", "cat-7", "cat-8",
]

# ---------------------------------------------------------------- tokens


def parse_blocks(css: str):
    """(conditions, declarations) for each top-level `:root...` rule, in order."""
    css = re.sub(r"/\*.*?\*/", "", css, flags=re.S)
    out = []
    for m in re.finditer(r"(:root[^{]*)\{([^}]*)\}", css):
        selector, body = m.group(1).strip(), m.group(2)
        conds = dict(re.findall(r"\[data-(\w+)='(\w+)'\]", selector))
        decls = dict(re.findall(r"--ed-([\w-]+)\s*:\s*([^;]+);", body))
        out.append((conds, decls))
    return out


def resolve(blocks, palette: str, mode: str) -> dict[str, str]:
    applies = []
    for order, (conds, decls) in enumerate(blocks):
        if conds.get("palette", palette) != palette or conds.get("theme", mode) != mode:
            continue
        applies.append((len(conds), order, decls))
    values: dict[str, str] = {}
    for _, _, decls in sorted(applies, key=lambda a: (a[0], a[1])):
        values.update(decls)
    for _ in range(4):
        for k, v in values.items():
            m = re.fullmatch(r"var\(--ed-([\w-]+)\)", v.strip())
            if m:
                values[k] = values[m.group(1)]
    return {t: values[t].strip().lower() for t in TOKENS}


def check_site(all_tokens) -> None:
    """The website and the editor are one brand: their shared tokens must match."""
    css = re.sub(r"/\*.*?\*/", "", SITE.read_text(encoding="utf-8"), flags=re.S)
    blocks = []
    for m in re.finditer(r"(:root[^{]*)\{([^}]*)\}", css):
        conds = dict(re.findall(r"\[data-(\w+)='(\w+)'\]", m.group(1)))
        decls = dict(re.findall(r"--jx-([\w-]+)\s*:\s*([^;]+);", m.group(2)))
        blocks.append((conds, decls))
    wrong = []
    for p, _ in PALETTES:
        for mode in MODES:
            site = {}
            for conds, decls in sorted(
                [(c, d) for c, d in blocks if c.get("palette", p) == p and c.get("theme", mode) == mode],
                key=lambda cd: len(cd[0]),
            ):
                site.update(decls)
            for ed, jx in SHARED.items():
                if ed == "raised" and mode == "light":
                    continue  # light editor fields are white; the site's light panel is tinted
                a, b = all_tokens[p][mode][ed], site.get(jx, "").strip().lower()
                if a != b:
                    wrong.append(f"{p} {mode}: editor --ed-{ed} {a} / website --jx-{jx} {b or '(missing)'}")
    if wrong:
        raise SystemExit("theme.css and website palettes.css disagree:\n  " + "\n  ".join(wrong))


# ---------------------------------------------------------------- colour


def rgb(h: str) -> tuple[int, int, int]:
    h = h.lstrip("#")
    return int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16)


def hexc(c) -> str:
    return "#%02x%02x%02x" % tuple(max(0, min(255, round(x))) for x in c)


def mix(a: str, b: str, t: float) -> str:
    """`a` moved `t` of the way to `b`, in sRGB (as CSS color-mix does)."""
    ca, cb = rgb(a), rgb(b)
    return hexc(tuple(x + (y - x) * t for x, y in zip(ca, cb)))


def luminance(h: str) -> float:
    def ch(c):
        c /= 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4
    r, g, b = rgb(h)
    return 0.2126 * ch(r) + 0.7152 * ch(g) + 0.0722 * ch(b)


def contrast(a: str, b: str) -> float:
    la, lb = sorted((luminance(a), luminance(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


# ---------------------------------------------------------------- pixel ramp

# Slot order is the index order of the .gpl palettes and the indexed PNGs, so
# art drawn against one palette swaps to another by index. Never reorder.
SLOTS = [
    (".", "clear", "transparent"),
    ("o", "outline", "the silhouette's edge"),
    ("s", "shadow", "the dark side of a neutral form"),
    ("m", "mid", "a neutral body"),
    ("l", "light", "the lit side, one-pixel highlights"),
    ("a", "accent", "the palette accent"),
    ("A", "accent shade", "the accent's dark side"),
    ("b", "second", "the second accent (a2)"),
    ("B", "second shade", "a2's dark side"),
    ("c", "warm", "the third accent (a3)"),
    ("C", "warm shade", "a3's dark side"),
    ("g", "glow", "the sun's top colour; lights, highlights on warm forms"),
]


def ramp(t: dict[str, str], mode: str) -> list[str | None]:
    """The twelve slot colours for one palette and mode."""
    if mode == "dark":
        outline, shadow, mid, light = t["crust"], t["line-strong"], t["muted"], t["ink"]
        shade = 0.45
    else:
        outline, shadow, mid, light = t["ink"], t["faint"], t["line-strong"], t["raised"]
        shade = 0.35
    return [
        None,
        outline,
        shadow,
        mid,
        light,
        t["accent"],
        mix(t["accent"], outline, shade),
        t["a2"],
        mix(t["a2"], outline, shade),
        t["a3"],
        mix(t["a3"], outline, shade),
        t["sun1"],
    ]


# ---------------------------------------------------------------- icons


def load_icons() -> list[tuple[str, list[str]]]:
    icons = []
    for p in sorted((HERE / "pixel-icons").glob("*.txt")):
        # A line starting with `#` is a comment; every other line is a row.
        rows = [r for r in p.read_text(encoding="utf-8").splitlines() if not r.startswith("#")]
        if len(rows) != 16 or any(len(r) != 16 for r in rows):
            raise SystemExit(f"{p.name}: an icon is 16 rows of 16 characters")
        letters = {s[0] for s in SLOTS}
        bad = {ch for r in rows for ch in r} - letters
        if bad:
            raise SystemExit(f"{p.name}: unknown slot letters {sorted(bad)}")
        icons.append((p.stem, rows))
    return icons


def icon_rects(rows: list[str], colours: list[str | None], x0: float, y0: float, px: float) -> str:
    index = {s[0]: i for i, s in enumerate(SLOTS)}
    out = []
    for y, row in enumerate(rows):
        x = 0
        while x < 16:
            ch = row[x]
            run = 1
            while x + run < 16 and row[x + run] == ch:
                run += 1
            c = colours[index[ch]]
            if c:
                out.append(f'<rect x="{x0 + x * px:g}" y="{y0 + y * px:g}" width="{run * px:g}" height="{px:g}" fill="{c}"/>')
            x += run
    return "".join(out)


# ---------------------------------------------------------------- svg boards

FONT = "font-family=\"'IBM Plex Mono', Consolas, monospace\""
SANS = "font-family=\"'IBM Plex Sans', 'Segoe UI', sans-serif\""


def svg(w: int, h: int, body: str, bg: str) -> str:
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" shape-rendering="crispEdges">'
        f'<rect width="{w}" height="{h}" fill="{bg}"/>{body}</svg>\n'
    )


def text(x, y, s, fill, size=12, anchor="start", mono=True, weight=400) -> str:
    fam = FONT if mono else SANS
    s = s.replace("&", "&amp;").replace("<", "&lt;")
    return f'<text x="{x}" y="{y}" {fam} font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}" shape-rendering="auto">{s}</text>'


def palettes_board(all_tokens) -> str:
    groups = [
        ("Grounds", ["crust", "mantle", "base", "raised", "hover", "line", "line-strong"]),
        ("Text", ["ink", "text", "muted", "faint"]),
        ("Accents", ["accent", "accent-low", "accent-high", "a2", "a3", "sun1", "sun2"]),
        ("States", ["ok", "warn", "err", "on-accent"]),
        ("Categories", ["cat-1", "cat-2", "cat-3", "cat-4", "cat-5", "cat-6", "cat-7", "cat-8"]),
    ]
    sw, sh, gap = 64, 40, 6
    col_w = max(len(n) for _, n in groups) * (sw + gap) + 24
    row_h = 30 + len(groups) * (sh + 30) + 10
    W = 24 + 2 * col_w
    H = 24 + 3 * row_h
    body = []
    for pi, (pid, pname) in enumerate(PALETTES):
        for mi, mode in enumerate(MODES):
            t = all_tokens[pid][mode]
            x0, y0 = 24 + mi * col_w, 24 + pi * row_h
            body.append(f'<rect x="{x0 - 12}" y="{y0 - 12}" width="{col_w - 12}" height="{row_h - 12}" rx="8" fill="{t["base"]}" stroke="{t["line"]}"/>')
            body.append(text(x0, y0 + 10, f"{pname.upper()} - {mode.upper()}", t["a3"], 11, weight=500))
            y = y0 + 30
            for gname, names in groups:
                body.append(text(x0, y + 4, gname, t["muted"], 10))
                y += 10
                for i, n in enumerate(names):
                    x = x0 + i * (sw + gap)
                    body.append(f'<rect x="{x}" y="{y}" width="{sw}" height="{sh}" rx="4" fill="{t[n]}" stroke="{t["line"]}"/>')
                    label = t["on-accent"] if contrast(t[n], t["on-accent"]) >= contrast(t[n], t["ink"]) else t["ink"]
                    body.append(text(x + 5, y + 15, n, label, 9))
                    body.append(text(x + 5, y + 31, t[n], label, 9))
                y += sh + 20
    return svg(W, H, "".join(body), "#0b0712")


def ramp_board(all_tokens, ramps) -> str:
    cell = 40
    W = 210 + len(SLOTS) * (cell + 6) + 18
    H = 70 + 6 * (cell + 16) + 10
    body = [text(18, 30, "PIXEL RAMP - TWELVE SLOTS, ONE INDEX ORDER IN EVERY PALETTE", "#ff9f43", 12, weight=500)]
    for i, (ch, name, _) in enumerate(SLOTS):
        body.append(text(210 + i * (cell + 6) + cell / 2, 56, f"{i} {ch}", "#b89bb8", 10, "middle"))
    r = 0
    for pid, pname in PALETTES:
        for mode in MODES:
            t = all_tokens[pid][mode]
            y = 70 + r * (cell + 16)
            body.append(f'<rect x="10" y="{y - 6}" width="{W - 20}" height="{cell + 12}" rx="6" fill="{t["base"]}"/>')
            body.append(text(22, y + cell / 2 + 4, f"{pname} {mode}", t["text"], 11, mono=False))
            for i, c in enumerate(ramps[pid][mode]):
                x = 210 + i * (cell + 6)
                if c is None:
                    body.append(f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" fill="none" stroke="{t["line"]}" stroke-dasharray="3 3"/>')
                else:
                    body.append(f'<rect x="{x}" y="{y}" width="{cell}" height="{cell}" fill="{c}"/>')
            r += 1
    return svg(W, H, "".join(body), "#0b0712")


def icons_board(all_tokens, ramps, icons) -> str:
    px = 4
    size = 16 * px
    gap = 28
    left = 200
    W = left + len(icons) * (size + gap) + 10
    row_h = size + 34
    H = 60 + 6 * row_h
    body = [text(18, 30, "SAMPLE PIXEL ICONS - THE SAME INDEXED ART IN EVERY PALETTE (4x)", "#ff9f43", 12, weight=500)]
    r = 0
    for pid, pname in PALETTES:
        for mode in MODES:
            t = all_tokens[pid][mode]
            y = 50 + r * row_h
            body.append(f'<rect x="10" y="{y}" width="{W - 20}" height="{row_h - 6}" rx="6" fill="{t["base"]}"/>')
            body.append(text(22, y + row_h / 2 + 2, f"{pname} {mode}", t["text"], 11, mono=False))
            for i, (name, rows) in enumerate(icons):
                x = left + i * (size + gap)
                body.append(icon_rects(rows, ramps[pid][mode], x, y + 7, px))
                if r == 0:
                    body.append(text(x + size / 2, y - 4, name, "#b89bb8", 10, "middle"))
            r += 1
    return svg(W, H, "".join(body), "#0b0712")


def motifs_board(t) -> str:
    """The title card's parts, in Dusk dark: what the README describes, drawn."""
    W, H = 960, 420
    stripes = [(0.0, 0.52), (0.56, 0.66), (0.71, 0.79), (0.85, 0.91)]  # site.css .sun mask
    cx, cy, rad = 640, 200, 150
    defs = (
        '<defs>'
        f'<linearGradient id="sun" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{t["sun1"]}"/><stop offset="1" stop-color="{t["sun2"]}"/></linearGradient>'
        f'<linearGradient id="sky" x1="0" y1="0" x2="0" y2="1"><stop offset="0" stop-color="{t["base"]}"/><stop offset="1" stop-color="{mix(t["base"], t["accent"], 0.18)}"/></linearGradient>'
        f'<clipPath id="disc"><circle cx="{cx}" cy="{cy}" r="{rad}"/></clipPath>'
        '</defs>'
    )
    body = [defs, f'<rect width="{W}" height="{H}" fill="url(#sky)"/>']
    for a, b in stripes:
        y = cy - rad + a * 2 * rad
        h = (b - a) * 2 * rad
        body.append(f'<rect x="{cx - rad}" y="{y:.1f}" width="{2 * rad}" height="{h:.1f}" fill="url(#sun)" clip-path="url(#disc)" shape-rendering="auto"/>')
    horizon = 300
    # the city: flat blocks in the ground colour, a few lit windows
    blocks = [(20, 60, 55), (90, 40, 85), (140, 70, 40), (225, 28, 65), (430, 55, 48), (500, 45, 70),
              (556, 26, 100), (596, 60, 50), (690, 80, 62), (790, 36, 92), (840, 66, 45), (915, 50, 75)]
    for x, w, hpct in blocks:
        h = hpct * 1.2
        body.append(f'<rect x="{x}" y="{horizon - h:.0f}" width="{w}" height="{h:.0f}" fill="{t["base"]}"/>')
    for x, y in [(565, 210), (565, 222), (565, 234), (803, 200), (803, 214), (812, 228), (98, 230), (98, 244)]:
        body.append(f'<rect x="{x}" y="{y}" width="3" height="3" fill="{t["a3"]}"/>')
    # the floor: grid in a2, the horizon a glowing accent line
    body.append(f'<rect x="0" y="{horizon}" width="{W}" height="{H - horizon}" fill="{t["base"]}"/>')
    vp = W / 2
    for i in range(-16, 17):
        body.append(f'<line x1="{vp + i * 18}" y1="{horizon}" x2="{vp + i * 120}" y2="{H}" stroke="{t["a2"]}" stroke-opacity="0.5" shape-rendering="auto"/>')
    y, step = horizon, 6.0
    while y < H:
        body.append(f'<line x1="0" y1="{y:.1f}" x2="{W}" y2="{y:.1f}" stroke="{t["a2"]}" stroke-opacity="0.5"/>')
        step *= 1.45
        y += step
    body.append(f'<rect x="0" y="{horizon - 1}" width="{W}" height="2" fill="{t["accent"]}"/>')
    body.append(f'<rect x="0" y="{horizon - 5}" width="{W}" height="10" fill="{t["accent"]}" opacity="0.18"/>')
    # wordmark area
    body.append(text(40, 70, "ジェイ・エンジン", t["a2"], 15, mono=False))
    body.append(f'<text x="40" y="140" font-family="\'Dela Gothic One\', \'Arial Black\', Impact, sans-serif" font-size="76" fill="{t["ink"]}" shape-rendering="auto">j<tspan fill="{t["accent"]}">Engine</tspan></text>')
    body.append(text(42, 172, "2D GAME ENGINE - RUST, WGPU, LUA", t["a3"], 13, weight=500))
    # the rule under every header
    body.append(f'<defs><linearGradient id="rule" x1="0" x2="1"><stop offset="0" stop-color="{t["accent"]}"/><stop offset="0.55" stop-color="{t["a3"]}"/><stop offset="1" stop-color="{t["a2"]}"/></linearGradient></defs>')
    body.append(f'<rect x="0" y="0" width="{W}" height="3" fill="url(#rule)"/>')
    return svg(W, H, "".join(body), t["base"])


# ---------------------------------------------------------------- outputs


def write_gpl(path: Path, name: str, colours: list[str | None]):
    lines = ["GIMP Palette", f"Name: {name}", "Columns: 12", "#"]
    for (ch, slot, _), c in zip(SLOTS, colours):
        if c is None:
            c = "#ff00ff"  # slot 0 is transparent; magenta marks it in tools that have no alpha slot
        r, g, b = rgb(c)
        lines.append(f"{r:3d} {g:3d} {b:3d}\t{slot} ({ch})")
    path.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")


def write_pngs(icons, colours):
    try:
        from PIL import Image
    except ImportError:
        print("Pillow not installed: skipping pixel-icons/*.png")
        return
    index = {s[0]: i for i, s in enumerate(SLOTS)}
    flat = []
    for c in colours:
        flat += list(rgb(c)) if c else [255, 0, 255]
    flat += [0] * (768 - len(flat))
    for name, rows in icons:
        img = Image.new("P", (16, 16))
        img.putpalette(flat)
        img.putdata([index[ch] for row in rows for ch in row])
        img.info["transparency"] = 0
        img.save(HERE / "pixel-icons" / f"{name}.png", transparency=0, optimize=False)


def main():
    blocks = parse_blocks(THEME.read_text(encoding="utf-8"))
    all_tokens = {p: {m: resolve(blocks, p, m) for m in MODES} for p, _ in PALETTES}
    ramps = {p: {m: ramp(all_tokens[p][m], m) for m in MODES} for p, _ in PALETTES}
    icons = load_icons()
    check_site(all_tokens)

    (HERE / "img").mkdir(exist_ok=True)
    (HERE / "palettes").mkdir(exist_ok=True)

    doc = {
        "source": "editor/src/theme.css (generated by docs/brand/build_brand.py - do not edit)",
        "palettes": {p: {"name": n, **{m: all_tokens[p][m] for m in MODES}} for p, n in PALETTES},
        "pixel_slots": [{"index": i, "letter": ch, "name": n, "use": u} for i, (ch, n, u) in enumerate(SLOTS)],
        "pixel_ramps": {p: {m: ramps[p][m] for m in MODES} for p, _ in PALETTES},
    }
    (HERE / "tokens.json").write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n", encoding="utf-8", newline="\n")

    for p, n in PALETTES:
        for m in MODES:
            write_gpl(HERE / "palettes" / f"jengine-{p}-{m}.gpl", f"jEngine {n} {m}", ramps[p][m])

    (HERE / "img" / "palettes.svg").write_text(palettes_board(all_tokens), encoding="utf-8", newline="\n")
    (HERE / "img" / "pixel-ramp.svg").write_text(ramp_board(all_tokens, ramps), encoding="utf-8", newline="\n")
    (HERE / "img" / "pixel-icons.svg").write_text(icons_board(all_tokens, ramps, icons), encoding="utf-8", newline="\n")
    (HERE / "img" / "motifs.svg").write_text(motifs_board(all_tokens["dusk"]["dark"]), encoding="utf-8", newline="\n")
    write_pngs(icons, ramps["dusk"]["dark"])

    # Contrast of the text tokens on their grounds, for the README's table.
    for p, _ in PALETTES:
        for m in MODES:
            t = all_tokens[p][m]
            print(f"{p:7} {m:5}  text/base {contrast(t['text'], t['base']):5.2f}  muted/base {contrast(t['muted'], t['base']):5.2f}"
                  f"  faint/base {contrast(t['faint'], t['base']):5.2f}  accent/base {contrast(t['accent'], t['base']):5.2f}"
                  f"  on-accent/accent {contrast(t['on-accent'], t['accent']):5.2f}")
    print(f"{len(icons)} icons")


if __name__ == "__main__":
    main()
