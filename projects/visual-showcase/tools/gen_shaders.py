r"""Write the showcase's shader graphs (assets/shaders/*.shader) and the
materials that use them (assets/materials/*.material).

Every effect is a real node graph - the same JSON the editor's shader graph
editor opens - built here by a tiny builder so the wiring is readable and can
be retuned by changing a number. They use only nodes the editor offers,
including the Noise, Voronoi and Scene Color nodes added for this gallery.

Re-run after editing:
    python tools/gen_shaders.py      (from the project directory)
"""
import json
from pathlib import Path

ASSETS = Path(__file__).resolve().parent.parent / "assets"


class Graph:
    """Nodes and edges, as `jengine_core::renderer::shader_graph::ShaderGraph`
    serialises them. `n(type, **fields)` adds a node and returns its id;
    `c(src, src_pin, dst, dst_pin)` wires them."""

    def __init__(self, name, description):
        self.meta = {"name": name, "description": description, "author": "jEngine showcase"}
        self.nodes = [{"id": 0, "node_type": {"type": "FragmentOutput"}, "position": [1600, 300]}]
        self.edges = []
        self.col = 0

    def n(self, type_, **fields):
        nid = len(self.nodes)
        self.col += 1
        self.nodes.append({"id": nid, "node_type": {"type": type_, **fields},
                           "position": [(self.col % 8) * 190, (self.col // 8) * 150]})
        return nid

    def c(self, src, src_pin, dst, dst_pin):
        self.edges.append({"from_node": src, "from_pin": src_pin, "to_node": dst, "to_pin": dst_pin})

    # Small conveniences over the node set.
    def f(self, v):
        return self.n("FloatConstant", value=float(v))

    def v2(self, x, y):
        return self.n("Vec2Constant", value=[float(x), float(y)])

    def color(self, r, g, b, a=1.0):
        return self.n("ColorInput", color=[float(r), float(g), float(b), float(a)])

    def op(self, type_, a, b, a_pin=0, b_pin=0):
        o = self.n(type_)
        self.c(a, a_pin, o, 0)
        self.c(b, b_pin, o, 1)
        return o

    def op3(self, type_, a, b, t, pins=(0, 0, 0)):
        o = self.n(type_)
        for i, (src, pin) in enumerate(zip((a, b, t), pins)):
            self.c(src, pin, o, i)
        return o

    def un(self, type_, a, pin=0):
        o = self.n(type_)
        self.c(a, pin, o, 0)
        return o

    def noise(self, uv, scale, octaves=4, uv_pin=0):
        o = self.n("Noise", octaves=octaves)
        self.c(uv, uv_pin, o, 0)
        self.c(self.f(scale), 0, o, 1)
        return o

    def voronoi(self, uv, scale, uv_pin=0):
        o = self.n("Voronoi")
        self.c(uv, uv_pin, o, 0)
        self.c(self.f(scale), 0, o, 1)
        return o

    def rgba(self, r, g, b, a):
        """Combine four (node, pin) floats into a vec4."""
        o = self.n("Combine", components=4)
        for i, (src, pin) in enumerate((r, g, b, a)):
            self.c(src, pin, o, i)
        return o

    def out(self, src, pin=0):
        self.c(src, pin, 0, 0)

    def json(self):
        return {"meta": self.meta, "nodes": self.nodes, "edges": self.edges, "next_id": len(self.nodes)}


def drift(g, uv, dx, dy):
    """uv + time * (dx, dy)."""
    t = g.n("TimeInput")
    return g.op("Add", uv, g.op("Multiply", t, g.v2(dx, dy)))


def dissolve():
    g = Graph("Dissolve", "The sprite burns away along a noise field as `progress` rises, "
              "with a hot edge brighter than white where it is burning.")
    uv = g.n("UvInput")
    tex = g.un("TextureSample", uv)
    n = g.noise(uv, 6.0, 5)
    split_p = g.un("Split", g.n("MaterialParam", name="progress"))
    p = split_p                                   # pin 0 = x
    keep = g.op3("SmoothStep", p, g.op("Add", p, g.f(0.015)), n)          # 0 burnt, 1 kept
    near = g.op3("SmoothStep", p, g.op("Add", p, g.f(0.09)), n)           # 0 at the edge
    glow = g.op("Multiply", g.op("Subtract", g.f(1.0), near), keep)       # a band above the edge
    hot = g.color(6.0, 1.8, 0.35, 0.0)
    lit = g.op3("Emission", tex, hot, glow)
    s = g.un("Split", lit)
    a = g.op("Multiply", (tex), keep, 4, 0)       # tex.A * keep
    g.out(g.rgba((s, 0), (s, 1), (s, 2), (a, 0)))
    return g


def hologram():
    g = Graph("Hologram", "The sprite's silhouette as projected light: cyan, scan lines "
              "rolling up it, a flicker from noise over time. Drawn additive.")
    uv = g.n("UvInput")
    tex = g.un("TextureSample", uv)
    t = g.n("TimeInput")
    uvs = g.un("Split", uv)
    wave = g.un("Sin", g.op("Add", g.op("Multiply", uvs, g.f(260.0), 1, 0), g.op("Multiply", t, g.f(-7.0))))
    scan = g.op("Add", g.f(0.65), g.op("Multiply", wave, g.f(0.35)))
    flicker_uv = g.n("Combine", components=2)
    g.c(g.op("Multiply", t, g.f(9.0)), 0, flicker_uv, 0)
    g.c(g.f(0.5), 0, flicker_uv, 1)
    flicker = g.op("Add", g.f(0.55), g.op("Multiply", g.noise(flicker_uv, 1.0, 2), g.f(0.6)))
    # A brighter band sweeping up: where fract(uv.y*0.5 - t*0.25) is near 0.5.
    band_pos = g.op("Subtract", g.op("Multiply", uvs, g.f(0.5), 1, 0), g.op("Multiply", t, g.f(0.25)))
    band = g.un("Floor", band_pos)
    band_f = g.op("Subtract", band_pos, band)                    # fract
    band_d = g.un("Abs", g.op("Subtract", band_f, g.f(0.5)))
    band_v = g.op3("SmoothStep", g.f(0.06), g.f(0.0), band_d)
    k = g.op("Multiply", g.op("Multiply", scan, flicker), g.op("Add", g.f(1.0), g.op("Multiply", band_v, g.f(1.5))))
    k = g.op("Multiply", k, tex, 0, 4)                            # * tex.A
    cyan = g.color(0.35, 1.6, 2.2, 1.0)
    g.out(g.op("Multiply", cyan, k))
    return g


def water():
    g = Graph("Water", "Two Voronoi layers drifting across each other make the bright web of "
              "caustics; noise shimmers the depth colour under them.")
    uv = g.n("UvInput")
    # Two drifting Voronoi layers; each one's Edge (F2 - F1) is a web of thin
    # lines, and where the two webs cross the light doubles up.
    v1 = g.voronoi(drift(g, uv, 0.035, 0.022), 5.0)
    uv2 = g.op("Multiply", uv, g.f(1.37))
    v2 = g.voronoi(drift(g, uv2, -0.028, 0.04), 5.0)
    line1 = g.op3("SmoothStep", g.f(0.14), g.f(0.0), v1, pins=(0, 0, 2))
    line2 = g.op3("SmoothStep", g.f(0.14), g.f(0.0), v2, pins=(0, 0, 2))
    caustic = g.op("Power", g.op("Add", line1, line2), g.f(1.6))
    uvs = g.un("Split", uv)
    deep = g.color(0.01, 0.07, 0.16)
    shallow = g.color(0.03, 0.32, 0.42)
    shimmer = g.noise(drift(g, uv, 0.02, -0.03), 3.0, 3)
    depth = g.op("Add", g.op("Multiply", uvs, g.f(0.7), 1, 0), g.op("Multiply", shimmer, g.f(0.3)))
    base = g.op3("Mix", deep, shallow, depth)
    light = g.color(0.7, 1.1, 1.2, 0.0)
    g.out(g.op("Add", base, g.op("Multiply", light, caustic)))
    return g


def shield():
    g = Graph("Energy Shield", "A dome of force: a hot rim, hexagonal-ish Voronoi cells whose "
              "edges light as a pulse runs out from the centre. Drawn additive.")
    uv = g.n("UvInput")
    t = g.n("TimeInput")
    r = g.op("Multiply", g.op("Distance", uv, g.v2(0.5, 0.5)), g.f(2.0))
    inside = g.op("Subtract", g.f(1.0), g.op3("SmoothStep", g.f(0.97), g.f(1.0), r))
    rim = g.op3("SmoothStep", g.f(0.65), g.f(1.0), r)
    cells = g.voronoi(uv, 9.0)
    edges = g.op3("SmoothStep", g.f(0.07), g.f(0.0), cells, pins=(0, 0, 2))
    pulse = g.op("Add", g.f(0.5), g.op("Multiply", g.un("Sin", g.op("Subtract", g.op("Multiply", t, g.f(3.0)),
                                                                        g.op("Multiply", r, g.f(9.0)))), g.f(0.5)))
    k = g.op("Add", g.op("Multiply", rim, g.f(1.6)), g.op("Multiply", edges, g.op("Multiply", pulse, g.f(0.9))))
    k = g.op("Multiply", k, inside)
    col = g.color(0.3, 1.0, 1.6, 1.0)
    g.out(g.op("Multiply", col, k))
    return g


def lava():
    g = Graph("Lava", "Noise warped by noise and pushed along slowly, through a colour ramp "
              "from cooling crust to a white-hot vein brighter than white.")
    uv = g.n("UvInput")
    warp = g.noise(drift(g, uv, 0.01, 0.03), 3.0, 4)
    uv_w = g.op("Add", uv, g.op("Multiply", warp, g.f(0.45)))
    heat = g.noise(drift(g, uv_w, -0.02, 0.05), 4.0, 5)
    crust = g.color(0.05, 0.01, 0.01)
    red = g.color(0.9, 0.12, 0.02)
    orange = g.color(2.4, 0.9, 0.12)
    white = g.color(5.0, 3.4, 1.4)
    c1 = g.op3("Mix", crust, red, g.op3("SmoothStep", g.f(0.38), g.f(0.55), heat))
    c2 = g.op3("Mix", c1, orange, g.op3("SmoothStep", g.f(0.55), g.f(0.68), heat))
    c3 = g.op3("Mix", c2, white, g.op3("SmoothStep", g.f(0.72), g.f(0.82), heat))
    g.out(c3)
    return g


def heat_haze():
    g = Graph("Heat Haze", "What is behind the sprite, read back through a Scene Color node and "
              "pushed sideways by noise rising over time - the air above a fire. Strongest "
              "over the flames, nothing at the sprite's edges, so it has no outline.")
    uv = g.n("UvInput")
    uvs = g.un("Split", uv)
    # Tall wisps: noise squeezed horizontally, rising (uv.y runs down the
    # sprite, so a positive drift in y carries the pattern up).
    stretch = g.op("Multiply", uv, g.v2(1.0, 0.45))
    wob_x = g.noise(drift(g, stretch, 0.0, 0.32), 7.0, 3)
    wob_y = g.noise(drift(g, g.op("Add", stretch, g.v2(3.7, 1.3)), 0.0, 0.27), 5.0, 2)
    # Centred on zero: noise is 0..1.
    dx = g.op("Subtract", wob_x, g.f(0.5))
    dy = g.op("Subtract", wob_y, g.f(0.5))
    # Strength: a bell across, rising from nothing at the top to full near the
    # bottom, and back to nothing on the last few pixels so no edge shows.
    across = g.op("Subtract", g.f(1.0), g.op3("SmoothStep", g.f(0.12), g.f(0.5),
                                                   g.un("Abs", g.op("Subtract", uvs, g.f(0.5)))))
    up = g.op3("SmoothStep", g.f(0.0), g.f(0.75), uvs, pins=(0, 0, 1))
    floor = g.op("Subtract", g.f(1.0), g.op3("SmoothStep", g.f(0.88), g.f(1.0), uvs, pins=(0, 0, 1)))
    k = g.op("Multiply", g.op("Multiply", across, up), g.op("Multiply", floor, g.f(0.016)))
    offset = g.n("Combine", components=2)
    g.c(g.op("Multiply", dx, k), 0, offset, 0)
    g.c(g.op("Multiply", dy, g.op("Multiply", k, g.f(0.5))), 0, offset, 1)
    scene = g.un("SceneColor", offset)
    g.out(scene)
    return g


def shockwave():
    g = Graph("Shockwave", "A ring running out from the middle as `progress` goes 0 to 1, pushing "
              "the scene behind it outward (a Scene Color node) and glowing in `tint` as it goes. "
              "Spellcraft's blasts; the script drives both parameters per blast.")
    uv = g.n("UvInput")
    d = g.op("Subtract", uv, g.v2(0.5, 0.5))
    ln = g.un("Length", d)
    r = g.op("Multiply", ln, g.f(2.0))                       # 0 at the middle, 1 at the rim
    p = g.un("Split", g.n("MaterialParam", name="progress"))
    fade = g.op("Subtract", g.f(1.0), p)
    band = g.op3("SmoothStep", g.f(0.16), g.f(0.0), g.un("Abs", g.op("Subtract", r, p)))
    inside = g.op3("SmoothStep", g.f(1.0), g.f(0.85), r)
    k = g.op("Multiply", g.op("Multiply", band, fade), inside)
    # Outward, in screen UV: a screen is wider than tall, so y is stretched
    # to push the same number of pixels both ways.
    dir_ = g.op("Divide", d, g.op("Add", ln, g.f(0.0001)))
    push = g.op("Multiply", g.op("Multiply", dir_, g.v2(1.0, 1.78)), g.op("Multiply", k, g.f(0.03)))
    scene = g.un("SceneColor", push)
    glow = g.op("Multiply", g.n("MaterialParam", name="tint"), g.op("Multiply", k, g.f(0.8)))
    g.out(g.op("Add", scene, glow))
    return g


def caustics_light():
    g = Graph("Caustics (light)", "Caustics as light, for a lightmap: the Water exhibit's two drifting "
              "Voronoi webs, drawn into the light buffer so they light whatever lies under them. "
              "Scaled by the lightmap's intensity (the vertex colour) and faded at the quad's edges. "
              "Additive, with alpha 1: an additive blend weights by alpha.")
    uv = g.n("UvInput")
    # The lightmap is wide and the floor squashed, so cells stretch across it.
    uvw = g.op("Multiply", uv, g.v2(3.2, 1.0))
    # Warped by slow noise, so the web wanders instead of sliding rigidly.
    warp = g.op("Multiply", g.noise(drift(g, uvw, 0.01, 0.02), 1.5, 2), g.f(0.35))
    uvw = g.op("Add", uvw, warp)
    v1 = g.voronoi(drift(g, uvw, 0.03, 0.02), 6.5)
    uv2 = g.op("Multiply", uvw, g.f(1.31))
    v2 = g.voronoi(drift(g, uv2, -0.025, 0.035), 6.5)
    line1 = g.op3("SmoothStep", g.f(0.07), g.f(0.0), v1, pins=(0, 0, 2))
    line2 = g.op3("SmoothStep", g.f(0.07), g.f(0.0), v2, pins=(0, 0, 2))
    # Bright only where the two webs cross or crowd: light gathers, it does
    # not draw every cell outline evenly.
    caustic = g.op("Power", g.op("Add", line1, line2), g.f(2.2))
    uvs = g.un("Split", uv)
    fade_x = g.op("Multiply", g.op3("SmoothStep", g.f(0.0), g.f(0.15), uvs, pins=(0, 0, 0)),
                  g.op3("SmoothStep", g.f(1.0), g.f(0.85), uvs, pins=(0, 0, 0)))
    fade_y = g.op("Multiply", g.op3("SmoothStep", g.f(0.0), g.f(0.2), uvs, pins=(0, 0, 1)),
                  g.op3("SmoothStep", g.f(1.0), g.f(0.8), uvs, pins=(0, 0, 1)))
    k = g.op("Multiply", caustic, g.op("Multiply", fade_x, fade_y))
    vc = g.un("Split", g.n("VertexColorInput"))
    k = g.op("Multiply", k, vc)                                  # * intensity (vertex r)
    col = g.color(0.55, 0.95, 1.0)
    rgb = g.op("Multiply", col, k)
    s_ = g.un("Split", rgb)
    g.out(g.rgba((s_, 0), (s_, 1), (s_, 2), (g.f(1.0), 0)))
    return g


def water_wobble():
    g = Graph("Water wobble", "The whole view seen through moving water: the scene behind, read back "
              "through a Scene Color node and nudged a few pixels by slow noise.")
    uv = g.n("UvInput")
    n1 = g.noise(drift(g, g.op("Multiply", uv, g.v2(1.8, 1.0)), 0.05, 0.07), 3.0, 2)
    n2 = g.noise(drift(g, g.op("Add", uv, g.v2(5.1, 2.3)), -0.04, 0.06), 3.0, 2)
    off = g.n("Combine", components=2)
    g.c(g.op("Multiply", g.op("Subtract", n1, g.f(0.5)), g.f(0.006)), 0, off, 0)
    g.c(g.op("Multiply", g.op("Subtract", n2, g.f(0.5)), g.f(0.008)), 0, off, 1)
    g.out(g.un("SceneColor", off))
    return g


# The lake's mirror covers LAKE_DEPTH of the view's height, starting at the
# horizon; the scene generator uses the same numbers.
LAKE_DEPTH = 0.22


def lake_mirror():
    g = Graph("Lake mirror", "A still lake: what is drawn above the horizon, read back flipped "
              "through a Scene Color node - the sprite's top edge is the horizon, so a pixel "
              "`d` below it shows the scene `d` above - broken by slow ripples and darkened "
              "toward the viewer.")
    uv = g.n("UvInput")
    uvs = g.un("Split", uv)
    # Screen-UV y runs down; the mirror image of a point uv.y * depth below the
    # horizon is the same distance above it: an offset of -2 * uv.y * depth.
    up = g.op("Multiply", uvs, g.f(-2.0 * LAKE_DEPTH), 1, 0)
    rip = g.noise(drift(g, g.op("Multiply", uv, g.v2(2.0, 14.0)), 0.02, -0.25), 3.0, 2)
    ripple = g.op("Multiply", g.op("Subtract", rip, g.f(0.5)), g.op("Multiply", g.op("Add", uvs, g.f(0.15), 1, 0), g.f(0.02)))
    off = g.n("Combine", components=2)
    g.c(ripple, 0, off, 0)
    g.c(g.op("Add", up, g.op("Multiply", ripple, g.f(0.3))), 0, off, 1)
    scene = g.un("SceneColor", off)
    # Darker and bluer toward the viewer.
    far = g.color(0.75, 0.75, 0.9)
    near = g.color(0.25, 0.3, 0.45)
    tint = g.op3("Mix", far, near, uvs, pins=(0, 0, 1))
    g.out(g.op("Multiply", scene, tint))
    return g


def custom(g, code, inputs, outputs):
    """A Custom node: WGSL reading its inputs and assigning its outputs by
    pin name. `inputs` is [(name, type, (node, pin))], `outputs` [(name, type)]."""
    o = g.n("CustomGlsl", code=code,
            inputs=[{"name": n, "pin_type": t} for n, t, _ in inputs],
            outputs=[{"name": n, "pin_type": t} for n, t in outputs])
    for i, (_, _, (src, pin)) in enumerate(inputs):
        g.c(src, pin, o, i)
    return o


def _ripple(r):
    """One ripple ring, as WGSL: `r` is (x, y, age in seconds, strength). The ring
    is squashed three to one, as water seen at a low angle is."""
    return f"""{{
    let age = {r}.z;
    if (age > 0.0 && age < 5.0 && {r}.w > 0.0) {{
        let q = (world - {r}.xy) * vec2<f32>(1.0, 3.2);
        let dist = length(q);
        let front = age * 150.0;
        let env = exp(-age * 0.8) * smoothstep(front + 12.0, front - 50.0, dist) * {r}.w;
        let wave = sin(dist * 0.12 - age * 10.0) * env;
        let dir = q / max(dist, 1.0);
        dx = dx + dir.x * wave * 10.0;
        dy = dy + dir.y * wave * 3.0;
        ring = ring + wave;
    }}
}}"""


LAKE_OFFSET = """let d = max(lake.x - world.y, 0.0);
let wind = lake.z;
// Small waves: a few sines, crossing, finer and stronger toward the viewer.
var w = sin(world.x * 0.021 + time * 0.9 + sin(world.y * 0.13 + time * 0.5) * 1.5);
w = w + 0.6 * sin(world.x * 0.047 - time * 1.3 + world.y * 0.31);
w = w + 0.35 * sin(world.y * 0.9 + time * 2.1 + sin(world.x * 0.011) * 3.0);
var dx = w * (0.6 + 2.4 * wind) * (0.2 + d * 0.012);
var dy = w * (0.3 + 1.2 * wind) * (0.1 + d * 0.004);
ring = 0.0;
""" + "\n".join(_ripple(r) for r in ("ra", "rb", "rc")) + """
// The point as far above the surface as this one is below it.
mirror = vec2<f32>(world.x + dx, 2.0 * lake.x - world.y + dy);
depth = d;
"""

LAKE_COLOUR = """let k = clamp(depth / 280.0, 0.0, 1.0);
// A lake mirrors best far off, and shows its own dark blue up close.
let tint = mix(vec3<f32>(0.80, 0.82, 0.94), vec3<f32>(0.16, 0.22, 0.34), k);
var c = scene.rgb * tint;
// The moon's glint: a broken column of light under it, wider toward us.
let gx = world.x - lake.y;
let spread = 8.0 + depth * 0.16;
let column = exp(-gx * gx / (2.0 * spread * spread));
// Glints: short flecks scattered at random over a grid of cells, each
// flaring and fading on its own clock - a hash per cell, not a lattice.
let cell = floor(world / vec2<f32>(6.0, 2.0));
let h = fract(sin(dot(cell, vec2<f32>(12.9898, 78.233))) * 43758.547);
let flare = max(sin(time * (1.5 + 2.5 * h) + h * 60.0), 0.0);
let sparkle = step(0.9, h) * flare * flare;
c = c + vec3<f32>(0.95, 0.95, 1.1) * sparkle * column * (1.5 + 2.0 * lake.z);
// Ripple crests catch the sky.
c = c + vec3<f32>(0.30, 0.30, 0.45) * max(ring, 0.0) * 0.7;
// A bright thread where the water meets the far shore.
c = c + vec3<f32>(0.7, 0.55, 0.55) * exp(-depth * 0.5) * 0.6;
color = vec4<f32>(c, 1.0);
"""


def lake_water():
    g = Graph("Lake water", "Water that reflects whatever stands above it, wherever the camera "
              "is: each pixel reads the scene at its mirror image across the surface - a "
              "World To Screen node on (x, 2 * surface - y), minus its own Screen Position - "
              "bent by small waves, by up to three ripple rings the script drops, and by the "
              "wind. Darker and bluer toward the viewer, with the moon's glint broken across it. "
              "`lake` is (surface y, moon x, wind 0..1, unused); `ra`..`rc` are ripples "
              "(x, y, age in seconds, strength).")
    pos = g.n("PositionInput")
    scr = g.n("ScreenPosition")
    t = g.n("TimeInput")
    lake = g.n("MaterialParam", name="lake")
    ra = g.n("MaterialParam", name="ripple_a")
    rb = g.n("MaterialParam", name="ripple_b")
    rc = g.n("MaterialParam", name="ripple_c")
    off = custom(g, LAKE_OFFSET,
                 [("world", "Vec2", (pos, 0)), ("time", "Float", (t, 0)), ("lake", "Vec4", (lake, 0)),
                  ("ra", "Vec4", (ra, 0)), ("rb", "Vec4", (rb, 0)), ("rc", "Vec4", (rc, 0))],
                 [("mirror", "Vec2"), ("depth", "Float"), ("ring", "Float")])
    there = g.un("WorldToScreen", off, 0)
    offset = g.op("Subtract", there, scr)
    scene = g.un("SceneColor", offset)
    col = custom(g, LAKE_COLOUR,
                 [("scene", "Vec4", (scene, 0)), ("depth", "Float", (off, 1)), ("ring", "Float", (off, 2)),
                  ("world", "Vec2", (pos, 0)), ("time", "Float", (t, 0)), ("lake", "Vec4", (lake, 0))],
                 [("color", "Vec4")])
    g.out(col)
    return g


def palette_cycle():
    g = Graph("Palette cycle", "Pixel art that animates by turning its palette, as 16-bit water "
              "did: the texture's red channel is an index 0..7 into eight colours kept in its "
              "own last row, and the index is shifted by whole steps over time. The `cycle` "
              "parameter is (steps per second, the palette row's v, one texel's u, unused).")
    uv = g.n("UvInput")
    tex = g.un("TextureSample", uv)
    idx = g.un("Floor", g.op("Multiply", tex, g.f(8.0), 1, 0))            # R * 8 -> 0..7
    cyc = g.un("Split", g.n("MaterialParam", name="cycle"))
    step = g.un("Floor", g.op("Multiply", g.n("TimeInput"), cyc, 0, 0))   # whole steps
    k = g.op("Subtract", idx, step)
    k = g.op("Subtract", k, g.op("Multiply", g.un("Floor", g.op("Multiply", k, g.f(0.125))), g.f(8.0)))   # mod 8
    pal_uv = g.n("Combine", components=2)
    g.c(g.op("Multiply", g.op("Add", k, g.f(0.5)), cyc, 0, 2), 0, pal_uv, 0)
    g.c(cyc, 1, pal_uv, 1)
    col = g.un("Split", g.un("TextureSample", pal_uv))
    g.out(g.rgba((col, 0), (col, 1), (col, 2), (tex, 4)))
    return g


def night_fog():
    g = Graph("Night fog", "Low fog drifting over a town: two layers of slow noise, "
              "thicker where they meet. The `fog` parameter's x is its density - the day "
              "and night script raises it after dusk and burns it off by morning.")
    uv = g.n("UvInput")
    n1 = g.noise(drift(g, g.op("Multiply", uv, g.v2(2.2, 1.4)), 0.012, 0.004), 3.0, 4)
    n2 = g.noise(drift(g, g.op("Add", g.op("Multiply", uv, g.v2(3.1, 2.0)), g.v2(4.3, 1.7)), -0.008, 0.006), 3.0, 3)
    n = g.op("Multiply", n1, n2)
    body = g.op3("SmoothStep", g.f(0.12), g.f(0.55), n)
    density = g.un("Split", g.n("MaterialParam", name="fog"))
    a = g.op("Multiply", body, density)
    col = g.color(0.62, 0.68, 0.82)
    c = g.un("Split", col)
    g.out(g.rgba((c, 0), (c, 1), (c, 2), (a, 0)))
    return g


def ink_outline():
    g = Graph("Ink outline", "A cartoon's ink line round the sprite's silhouette: the texture "
              "sampled at eight neighbours `width` texels away (a Texel Size node, so the line is "
              "the same thickness on any texture), and the outline colour drawn where a neighbour "
              "is opaque and this pixel is not. The art needs that much transparent margin. "
              "`outline` is (width in texels, -, -, -); `outline_color` is its colour and alpha.")
    uv = g.n("UvInput")
    tex = g.un("TextureSample", uv)
    params = g.un("Split", g.n("MaterialParam", name="outline"))
    step = g.op("Multiply", g.n("TexelSize"), params, 0, 0)          # texel * width
    steps = g.un("Split", step)
    neighbours = None
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7)):
        off = g.n("Combine", components=2)
        g.c(g.op("Multiply", steps, g.f(dx), 0, 0), 0, off, 0)
        g.c(g.op("Multiply", steps, g.f(dy), 1, 0), 0, off, 1)
        a_ = g.un("TextureSample", g.op("Add", uv, off))
        neighbours = (a_, 4) if neighbours is None else (g.op("Max", neighbours[0], a_, neighbours[1], 4), 0)
    rim = g.op("Max", g.op("Subtract", neighbours[0], tex, neighbours[1], 4), g.f(0.0))   # outside the shape
    ink = g.n("MaterialParam", name="outline_color")
    inks = g.un("Split", ink)
    ink_a = g.op("Multiply", rim, inks, 0, 3)
    # The sprite over the ink: mix by the sprite's own alpha.
    colour = g.op3("Mix", ink, g.op("Multiply", tex, g.n("VertexColorInput")), tex, pins=(0, 0, 4))
    c = g.un("Split", colour)
    alpha = g.op("Max", tex, ink_a, 4, 0)
    g.out(g.rgba((c, 0), (c, 1), (c, 2), (alpha, 0)))
    return g


def tinted(g):
    """The texture times the vertex colour; returns the UV node."""
    uv = g.n("UvInput")
    g.out(g.op("Multiply", g.un("TextureSample", uv), g.n("VertexColorInput")))
    return uv


def meadow_sway():
    g = Graph("Meadow sway", "Grass and flowers in the wind, by moving their vertices (a VertexOffset "
              "node): each top leans with a breeze whose phase runs along the field, and gusts roll "
              "across it left to right - a band of stronger wind travelling through the world, so "
              "the grass bends in a wave. The foot stays (times 1 - uv.y) and a bent tip dips. "
              "`wind` is (how far a tip leans, in world units, at full strength, -, -, -).")
    uv = tinted(g)
    t = g.n("TimeInput")
    x = g.un("Split", g.n("PositionInput"))
    breeze = g.op("Add",
                  g.op("Multiply", g.un("Sin", g.op("Add", g.op("Multiply", t, g.f(1.7)), g.op("Multiply", x, g.f(0.021)))), g.f(0.35)),
                  g.op("Multiply", g.un("Sin", g.op("Add", g.op("Multiply", t, g.f(2.9)), g.op("Multiply", x, g.f(0.057)))), g.f(0.15)))
    # A gust: the positive lobe of a long wave moving right, sharpened.
    roll = g.op("Max", g.un("Sin", g.op("Subtract", g.op("Multiply", x, g.f(0.0035)), g.op("Multiply", t, g.f(0.9)))), g.f(0.0))
    gust = g.op("Multiply", g.op("Multiply", roll, roll), g.op("Multiply", roll, g.f(1.4)))
    wind = g.un("Split", g.n("MaterialParam", name="wind"))
    up = g.op("Subtract", g.f(1.0), g.un("Split", uv), 0, 1)
    lean = g.op("Multiply", g.op("Multiply", g.op("Add", breeze, gust), wind), up)
    dip = g.op("Multiply", g.un("Abs", lean), g.f(-0.3))
    off = g.n("Combine", components=2)
    g.c(lean, 0, off, 0)
    g.c(dip, 0, off, 1)
    g.c(off, 0, g.n("VertexOffset"), 0)
    return g


def storm_grass():
    g = Graph("Storm grass", "Storm's verge bending in the wind, on the GPU (a VertexOffset node). A "
              "vertex `height` above the root, `f` of the way up its blade, bends to "
              "(sin(b f) height, (cos(b f) - 1) height) - the curve the script used to compute for "
              "every vertex and send across the bridge every frame. b is the wind's lean, a lean of "
              "the blade's own (from its x), and a sway with a phase of the blade's own. "
              "`wind` is (lean in radians, sway in radians, sway phase, the root's world y) - the "
              "phase added up by the script, since time x a speed that changes jumps.")
    uv = tinted(g)
    pos = g.un("Split", g.n("PositionInput"))
    w = g.un("Split", g.n("MaterialParam", name="wind"))
    f = g.op("Subtract", g.f(1.0), g.un("Split", uv), 0, 1)
    height = g.op("Subtract", pos, w, 1, 3)
    # A blade's own lean and phase, from where it stands: neighbours 8 units
    # apart differ by most of a radian, so the verge does not move as one.
    own = g.op("Multiply", g.un("Sin", g.op("Multiply", pos, g.f(0.37), 0, 0)), g.f(0.15))
    phase = g.op("Add", w, g.op("Multiply", g.un("Sin", g.op("Multiply", pos, g.f(0.91), 0, 0)), g.f(3.0)), 2, 0)
    sway = g.op("Multiply", g.un("Sin", phase), w, 0, 1)
    bend = g.op("Multiply", g.op("Add", g.op("Add", w, own, 0, 0), sway), f)
    off = g.n("Combine", components=2)
    g.c(g.op("Multiply", g.un("Sin", bend), height), 0, off, 0)
    g.c(g.op("Multiply", g.op("Subtract", g.un("Cos", bend), g.f(1.0)), height), 0, off, 1)
    g.c(off, 0, g.n("VertexOffset"), 0)
    return g


def banner_wave():
    g = Graph("Banner wave", "A banner rippling from its pole: a travelling wave across a mesh "
              "grid's vertices (a VertexOffset node), growing toward the free edge (times uv.x); the "
              "colour side shades the same wave's slopes so the folds catch the light.")
    uv = g.n("UvInput")
    t = g.n("TimeInput")
    u = g.un("Split", uv)
    phase = g.op("Subtract", g.op("Multiply", t, g.f(3.2)), g.op("Multiply", u, g.f(9.0)))
    wave = g.op("Add", g.un("Sin", phase), g.op("Multiply", g.un("Sin", g.op("Multiply", phase, g.f(1.9))), g.f(0.3)))
    lift = g.op("Multiply", g.op("Multiply", wave, g.f(16.0)), u)
    off = g.n("Combine", components=2)
    g.c(g.op("Multiply", g.op("Multiply", g.un("Cos", phase), g.f(-4.0)), u), 0, off, 0)
    g.c(lift, 0, off, 1)
    g.c(off, 0, g.n("VertexOffset"), 0)
    shade = g.op("Add", g.f(0.82), g.op("Multiply", g.un("Cos", phase), g.f(0.18)))
    tex = g.op("Multiply", g.un("TextureSample", uv), g.n("VertexColorInput"))
    s = g.un("Split", tex)
    rgb = g.n("Combine", components=4)
    for i in range(3):
        g.c(g.op("Multiply", s, shade, i, 0), 0, rgb, i)
    g.c(s, 3, rgb, 3)
    g.out(rgb)
    return g


def ghost_glow():
    g = Graph("Ghost glow", "For a canvas group: the group's whole picture rippling a little, "
              "with a glow round its silhouette - round the group, not round each of its parts, "
              "which overlap inside it. The picture is premultiplied, so this is too (the "
              "material blends PremultipliedAlpha). `glow` is (width in texels, strength, ripple, -); "
              "`glow_color` its colour.")
    uv = g.n("UvInput")
    t = g.n("TimeInput")
    params = g.un("Split", g.n("MaterialParam", name="glow"))
    sp = g.un("Split", uv)
    # A slow sideways ripple, a little different on every row.
    wave = g.un("Sin", g.op("Add", g.op("Multiply", sp, g.f(46.0), 1, 0), g.op("Multiply", t, g.f(2.6))))
    shift = g.n("Combine", components=2)
    g.c(g.op("Multiply", wave, params, 0, 2), 0, shift, 0)
    g.c(g.f(0.0), 0, shift, 1)
    uvw = g.op("Add", uv, shift)
    tex = g.un("TextureSample", uvw)
    vc = g.n("VertexColorInput")
    vcs = g.un("Split", vc)
    steps = g.un("Split", g.op("Multiply", g.n("TexelSize"), params, 0, 0))
    neighbours = None
    for dx, dy in ((1, 0), (-1, 0), (0, 1), (0, -1), (0.7, 0.7), (-0.7, 0.7), (0.7, -0.7), (-0.7, -0.7),
                   (2, 0), (-2, 0), (0, 2), (0, -2)):
        off = g.n("Combine", components=2)
        g.c(g.op("Multiply", steps, g.f(dx), 0, 0), 0, off, 0)
        g.c(g.op("Multiply", steps, g.f(dy), 1, 0), 0, off, 1)
        a_ = g.un("TextureSample", g.op("Add", uvw, off))
        neighbours = (a_, 4) if neighbours is None else (g.op("Max", neighbours[0], a_, neighbours[1], 4), 0)
    rim = g.op("Max", g.op("Subtract", neighbours[0], tex, neighbours[1], 4), g.f(0.0))
    rim = g.op("Multiply", g.op("Multiply", rim, params, 0, 1), vcs, 0, 3)
    glow = g.un("Split", g.n("MaterialParam", name="glow_color"))
    outside = g.op("Subtract", g.f(1.0), tex, 0, 4)
    k = g.op("Multiply", rim, outside)
    pic = g.un("Split", g.op("Multiply", tex, vc))
    out = []
    for i in range(3):
        out.append((g.op("Add", pic, g.op("Multiply", glow, k, i, 0), i, 0), 0))
    out.append((g.op("Add", pic, k, 3, 0), 0))
    g.out(g.rgba(*out))
    return g


EFFECTS = {
    "dissolve": (dissolve, "Alpha"),
    "hologram": (hologram, "Additive"),
    "water": (water, "Alpha"),
    "shield": (shield, "Additive"),
    "lava": (lava, "Alpha"),
    "heat_haze": (heat_haze, "Alpha"),
    "shockwave": (shockwave, "Alpha"),
    "caustics_light": (caustics_light, "Additive"),
    "water_wobble": (water_wobble, "Alpha"),
    "lake_mirror": (lake_mirror, "Alpha"),
    "palette_cycle": (palette_cycle, "Alpha"),
    "night_fog": (night_fog, "Alpha"),
    "ink_outline": (ink_outline, "Alpha"),
    "lake_water": (lake_water, "Alpha"),
    "meadow_sway": (meadow_sway, "Alpha"),
    "storm_grass": (storm_grass, "Alpha"),
    "banner_wave": (banner_wave, "Alpha"),
    "ghost_glow": (ghost_glow, "PremultipliedAlpha"),
}


if __name__ == "__main__":
    (ASSETS / "shaders").mkdir(parents=True, exist_ok=True)
    (ASSETS / "materials").mkdir(parents=True, exist_ok=True)
    for name, (build, blend) in EFFECTS.items():
        (ASSETS / "shaders" / f"{name}.shader").write_text(json.dumps(build().json(), indent=1) + "\n", encoding="utf-8")
        material = {"shader": "graph", "shader_graph_path": f"assets/shaders/{name}.shader", "blend_mode": blend}
        (ASSETS / "materials" / f"{name}.material").write_text(json.dumps(material, indent=1) + "\n", encoding="utf-8")
        print(f"wrote shaders/{name}.shader, materials/{name}.material ({blend})")
