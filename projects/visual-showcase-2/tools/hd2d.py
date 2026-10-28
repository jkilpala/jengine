"""Building HD-2D scenes out of engine components: a 3D `stage` camera,
`stage_quad`s (floors, walls, boxes, gabled houses) projected into meshes,
and `stage_billboard`s standing sprites, lights and emitters in it.

Stage space is x east, y north and z up, in units of about a metre; a
character is 1.6 tall. A quad's front is `u x v`: `wall(a, b)` faces to the
right of a walk from `a` to `b` (a wall from west to east faces south, toward
the camera), and `box` and `house` author every face - the stage culls the
ones turned away, so the camera can orbit.
"""

import contextlib
import json
import math
import os

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "assets")

# A character's art is drawn at two world pixels a texel at the target.
PIXEL = 2.0


def lin(c):
    """An sRGB colour (0..1 floats) in linear light, which is what the
    engine's colours are."""
    def one(v):
        return v / 12.92 if v <= 0.04045 else ((v + 0.055) / 1.055) ** 2.4
    return [round(one(v), 5) for v in c]


def hexc(h, a=None):
    h = h.lstrip("#")
    c = [int(h[i:i + 2], 16) / 255 for i in (0, 2, 4)]
    out = lin(c)
    return out + [a] if a is not None else out


def T(x=0.0, y=0.0, rot=0.0, scale=(1.0, 1.0)):
    return {"position": [float(x), float(y)], "rotation": float(rot), "scale": list(map(float, scale))}


class Scene:
    def __init__(self, name, width=1280, height=720):
        self.name = name
        self.entities = []
        self.names = set()
        self.post = []
        self.audio = []
        self.width, self.height = width, height
        self.textures = set()
        self.parent = None
        self.add("Camera", camera={"zoom": 1.0, "is_active": True, "viewport_width": float(width),
                                   "viewport_height": float(height)})

    # -- plumbing ---------------------------------------------------------

    def add(self, name, **components):
        base, n = name, 2
        while name in self.names:
            name = f"{base}{n}"
            n += 1
        self.names.add(name)
        components.setdefault("transform", T())
        entity = {"name": name, "components": components}
        if self.parent:
            entity["parent"] = self.parent
        self.entities.append(entity)
        return name

    def effect(self, name, enabled=True, **params):
        def val(v):
            if isinstance(v, (list, tuple)):
                return {"Vec4": list(map(float, v))}
            return {"Float": float(v)}
        self.post.append({"name": name, "enabled": enabled, "params": {k: val(v) for k, v in params.items()}})

    def save(self, rel, hdr=True, exposure=1.0, tonemap="aces"):
        post = {"effects": self.post}
        if hdr:
            post["hdr"] = True
            post["exposure"] = exposure
            post["tonemap"] = tonemap
        out = {"name": self.name, "post_process": post, "entities": self.entities}
        if self.audio:
            out["audio_refs"] = self.audio
        path = os.path.join(ROOT, rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            json.dump(out, f, indent=1)
            f.write("\n")
        return rel

    # -- the stage --------------------------------------------------------

    def stage(self, target=(0, 0, 0.8), yaw=0.0, pitch=0.55, distance=48.0, zoom=52.0, follow="",
              follow_offset=(0, 0, 0.8), follow_ease=3.0, script=None):
        comps = {"stage": {
            "target": list(map(float, target)), "yaw": yaw, "pitch": pitch, "distance": distance,
            "zoom": zoom, "near": 1.0, "follow": follow, "follow_offset": list(map(float, follow_offset)),
            "follow_ease": follow_ease, "z_base": 0, "z_per_unit": 10.0, "ground_z": -20000}}
        if script:
            comps["script"] = {"language": "Lua", "source_path": script}
        self.stage_name = self.add("Stage", **comps)
        return self.stage_name

    def group(self, name, pos, yaw=0.0):
        """A stage group: a frame that what is built `under` it stands in,
        moved to `pos` and turned `yaw` (radians, anticlockwise from above)."""
        return self.add(name, stage_group={"position": list(map(float, pos)), "yaw": float(yaw)})

    @contextlib.contextmanager
    def under(self, group):
        """Build inside `group`: coordinates are its, and the pieces are its
        children."""
        outer, self.parent = self.parent, group
        try:
            yield
        finally:
            self.parent = outer

    def quad(self, name, tex, origin, u, v, repeat=(1, 1), normal=None, color=(1, 1, 1, 1), ground=False,
             two_sided=False, unlit=False, blend=None, z_bias=0, sort_bias=0.0, specular=0.0, follow="",
             tags=None):
        self.textures.add(tex)
        comps = {
            "mesh": {"texture_id": tex, "color": list(map(float, color))},
            "stage_quad": {"origin": list(map(float, origin)), "u": list(map(float, u)), "v": list(map(float, v)),
                           "repeat": list(map(float, repeat)), "ground": ground, "two_sided": two_sided,
                           "sort_bias": float(sort_bias), "z_bias": int(z_bias), "follow": follow},
        }
        mat = {"material_id": "lit_sprite" if normal else "default_sprite"}
        if normal:
            mat["normal_map_id"] = normal
        if blend:
            mat["blend_mode"] = blend
        if specular:
            mat["specular"] = specular
            mat["shininess"] = 24.0
        if normal or blend or specular:
            comps["material"] = mat
        if unlit:
            comps["unlit"] = {}
        if tags:
            comps["tags"] = tags
        return self.add(name, **comps)

    def floor(self, name, tex, x0, y0, x1, y1, tile=2.0, z=0.0, **kw):
        """A floor from (x0, y0) to (x1, y1), its texture one tile every
        `tile` units."""
        kw.setdefault("ground", True)
        return self.quad(name, tex, (x0, y0, z), (x1 - x0, 0, 0), (0, y1 - y0, 0),
                         ((x1 - x0) / tile, (y1 - y0) / tile), **kw)

    def wall(self, name, tex, a, b, z0, height, tile=2.0, tile_v=None, **kw):
        """A wall standing on the line `a` -> `b`, facing to its right."""
        dx, dy = b[0] - a[0], b[1] - a[1]
        length = math.hypot(dx, dy)
        tv = tile_v or tile
        return self.quad(name, tex, (a[0], a[1], z0), (dx, dy, 0), (0, 0, height),
                         (length / tile, height / tv), **kw)

    def caster(self, name, x0, y0, x1, y1, height, base=0.0, opacity=0.7):
        """A solid's shadow on the floor from the stage's lights: its
        footprint, a rectangle, standing `height` tall."""
        return self.add(name, stage_caster={
            "footprint": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], "base": float(base), "height": float(height),
            "opacity": opacity, "lights": 2, "max_length": 3.0})

    def box(self, name, x0, y0, x1, y1, z0, z1, side, top=None, tile=2.0, side_n=None, top_n=None, color=(1, 1, 1, 1),
            top_color=None, cast=True, **kw):
        """Four walls facing out and a lid - each face its own quad - and the
        shadow it throws."""
        h = z1 - z0
        if cast and h > 0.3:
            self.caster(name + "Shadow", x0, y0, x1, y1, h, base=z0)
        args = dict(normal=side_n, color=color, **kw)
        self.wall(name + "S", side, (x0, y0), (x1, y0), z0, h, tile, **args)
        self.wall(name + "N", side, (x1, y1), (x0, y1), z0, h, tile, **args)
        self.wall(name + "W", side, (x0, y1), (x0, y0), z0, h, tile, **args)
        self.wall(name + "E", side, (x1, y0), (x1, y1), z0, h, tile, **args)
        if top:
            self.quad(name + "Top", top, (x0, y0, z1), (x1 - x0, 0, 0), (0, y1 - y0, 0),
                      ((x1 - x0) / tile, (y1 - y0) / tile), normal=top_n, color=top_color or color, **kw)

    def house(self, name, x0, y0, x1, y1, wall_h, roof_h, walls, roof, gable, walls_n=None, roof_n=None,
              overhang=0.35, tile=2.0, roof_tile=1.5, color=(1, 1, 1, 1), roof_color=(1, 1, 1, 1), windows=(),
              door=None, lit_windows=True, window_tex=None, glow_tex=None, door_tex=None, window_n=None, door_n=None):
        """A gabled house: walls, two roof slopes running east-west with the
        ridge along the middle, triangular gables (a texture with the
        triangle in its alpha), windows and a door on the south face."""
        args = dict(normal=walls_n, color=color)
        # Its shadow: the walls and half the roof - the gable's average.
        self.caster(name + "Shadow", x0, y0, x1, y1, wall_h + roof_h * 0.5)
        self.wall(name + "S", walls, (x0, y0), (x1, y0), 0, wall_h, tile, **args)
        self.wall(name + "N", walls, (x1, y1), (x0, y1), 0, wall_h, tile, **args)
        self.wall(name + "W", walls, (x0, y1), (x0, y0), 0, wall_h, tile, **args)
        self.wall(name + "E", walls, (x1, y0), (x1, y1), 0, wall_h, tile, **args)
        depth = y1 - y0
        mid = (y0 + y1) / 2
        o = overhang
        # Gables: one picture each, its triangle in alpha.
        self.quad(name + "GW", gable, (x0, y1, wall_h), (0, -depth, 0), (0, 0, roof_h), normal=walls_n, color=color)
        self.quad(name + "GE", gable, (x1, y0, wall_h), (0, depth, 0), (0, 0, roof_h), normal=walls_n, color=color)
        # Slopes, from the eave (past the wall by the overhang) to the ridge.
        run = depth / 2 + o
        drop = roof_h * o / (depth / 2)
        length = x1 - x0 + 2 * o
        slope = math.hypot(run, roof_h + drop)
        reps = (length / roof_tile, slope / roof_tile)
        rargs = dict(normal=roof_n, color=roof_color, two_sided=True)
        # The front slope sorts at its eave, so it draws over the wall top.
        self.quad(name + "RoofS", roof, (x0 - o, y0 - o, wall_h - drop), (length, 0, 0), (0, run, roof_h + drop),
                  reps, sort_bias=-(run / 2) - 0.05, **rargs)
        self.quad(name + "RoofN", roof, (x1 + o, y1 + o, wall_h - drop), (-length, 0, 0), (0, -run, roof_h + drop),
                  reps, sort_bias=(run / 2) - 0.05, **rargs)
        _ = mid
        for i, (wx, wz) in enumerate(windows):
            if window_tex:
                self.quad(f"{name}Win{i}", window_tex, (wx - 0.45, y0 - 0.03, wz), (0.9, 0, 0), (0, 0, 1.12),
                          normal=window_n, z_bias=1)
            if lit_windows and glow_tex:
                self.quad(f"{name}Glow{i}", glow_tex, (wx - 0.45, y0 - 0.05, wz), (0.9, 0, 0), (0, 0, 1.12),
                          unlit=True, blend="Additive", color=(2.2, 1.5, 0.7, 1.0), z_bias=2)
        if door and door_tex:
            dx_ = door
            self.quad(name + "Door", door_tex, (dx_ - 0.5, y0 - 0.03, 0), (1.0, 0, 0), (0, 0, 2.0), normal=door_n, z_bias=1)

    # -- things standing in it --------------------------------------------

    def billboard(self, name, tex, pos, size, anchor=(0.5, 0.0), color=(1, 1, 1, 1), uv=None, perspective=1.0,
                  scale=(1, 1), unlit=False, blend=None, normal=None, sort_bias=0.0, z_bias=0, follow="",
                  extra=None, shadow=None):
        """A sprite standing at `pos`, `size` world units at the target's depth."""
        self.textures.add(tex)
        sprite = {"texture_id": tex, "color": list(map(float, color)), "z_order": 0,
                  "size": list(map(float, size)), "anchor": list(map(float, anchor))}
        if uv:
            sprite["uv_rect"] = list(map(float, uv))
        comps = {"sprite": sprite, "stage_billboard": {
            "position": list(map(float, pos)), "perspective": perspective, "scale": list(map(float, scale)),
            "sort_bias": float(sort_bias), "z_bias": int(z_bias), "follow": follow}}
        if shadow:
            # Its silhouette thrown on the floor by the lights near it.
            comps["stage_billboard"]["shadow"] = dict({"opacity": 0.9, "fade": 0.35, "lights": 2, "max_length": 3.0},
                                                      **(shadow if isinstance(shadow, dict) else {}))
        if unlit:
            comps["unlit"] = {}
        if blend or normal:
            mat = {"material_id": "lit_sprite" if normal else "default_sprite"}
            if blend:
                mat["blend_mode"] = blend
            if normal:
                mat["normal_map_id"] = normal
            comps["material"] = mat
        if extra:
            comps.update(extra)
        return self.add(name, **comps)

    def character(self, name, sheet, pos, facing=1, walk_fps=8.0, script=None, play="idle", tags=None):
        """A pixel character: a six-frame sheet (two idle, four walking)."""
        n = 6
        clip = lambda frames, d: {"name": "", "frame_duration": d, "looping": True, "property_tracks": [],
                                  "frames": [{"uv_rect": [i / n, 0.0, (i + 1) / n, 1.0], "triggers": []} for i in frames]}
        idle, walk = clip([0, 1], 0.6), clip([2, 3, 4, 5], 1.0 / walk_fps)
        idle["name"], walk["name"] = "idle", "walk"
        extra = {"sprite_animator": {"clips": {"idle": idle, "walk": walk}, "current_clip": play, "playing": True,
                                     "speed": 1.0}}
        if script:
            extra["script"] = {"language": "Lua", "source_path": script}
        if tags:
            extra["tags"] = tags
        e = self.billboard(name, sheet, pos, (32 * PIXEL, 48 * PIXEL), anchor=(0.5, 1.5 / 48), uv=[0, 0, 1 / n, 1],
                           scale=(facing, 1), extra=extra, shadow={"height": 1.5})
        # A soft blob shadow on the ground under it, moving with it.
        self.blob(name + "Shadow", follow=e)
        return e

    def blob(self, name, pos=(0, 0, 0), size=(1.1, 0.55), alpha=0.8, follow=""):
        """A soft dark ellipse lying on the floor."""
        w, d = size
        return self.quad(name, "sprites/hd/fx/blob.png", (pos[0] - w / 2, pos[1] - d / 2, pos[2] + 0.01),
                         (w, 0, 0), (0, d, 0), ground=True, z_bias=5, color=(0, 0, 0, alpha), follow=follow)

    def light(self, name, pos, color, intensity=1.5, radius=6.0, falloff=1.6, height=0.6, kind="point", flicker=None,
              tags=None):
        light = {"color": list(map(float, color)), "intensity": float(intensity), "radius": 100.0,
                 "falloff": float(falloff), "height": float(height), "casts_shadows": False}
        comps = {f"{kind}_light": light,
                 "stage_billboard": {"position": list(map(float, pos)), "light_radius": float(radius)}}
        if tags:
            comps["tags"] = tags
        return self.add(name, **comps)

    def glow(self, name, pos, size, color, tex="sprites/hd/fx/glow.png", z_bias=50):
        """An additive, unlit halo - the light's own body, which bloom picks up."""
        return self.billboard(name, tex, pos, (size, size), anchor=(0.5, 0.5), color=color, unlit=True,
                              blend="Additive", z_bias=z_bias)

    def emitter(self, name, pos, emitter, z_bias=0):
        return self.add(name, particle_emitter=emitter,
                        stage_billboard={"position": list(map(float, pos)), "perspective": 1.0, "z_bias": z_bias})

    def ambient(self, color, intensity):
        return self.add("Ambient", ambient_light={"color": list(map(float, color)), "intensity": float(intensity)})

    def backdrop(self, name, tex, size, pos=(0, 0), z=-30000, color=(1, 1, 1, 1), unlit=True):
        """A picture behind everything - sky, far hills - not on the stage."""
        self.textures.add(tex)
        comps = {"transform": T(*pos), "sprite": {"texture_id": tex, "color": list(map(float, color)), "z_order": z,
                                                  "size": list(map(float, size)), "anchor": [0.5, 0.5]}}
        if unlit:
            comps["unlit"] = {}
        return self.add(name, **comps)

    def text(self, name, s, anchor, offset, size=16.0, color=(1, 1, 1, 1), align="Left", width=1150.0):
        """A caption in screen space: entity UI, drawn after the post chain,
        so the depth of field and the bloom leave it sharp."""
        comps = {"ui_element": {"anchor": anchor, "offset": list(map(float, offset)), "size": [width, size * 1.5],
                                "z_order": 100},
                 "ui_text": {"text": s, "font_size": float(size), "color": list(map(float, color)), "alignment": align,
                             "shadow": {"offset": [1.5, -1.5], "color": [0.0, 0.0, 0.0, 0.75]}}}
        return self.add(name, **comps)

    def script(self, name, path):
        return self.add(name, script={"language": "Lua", "source_path": path})
