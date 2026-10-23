"""The Creature (showcase scene 21): a big cat's rig, its clips and its skin.

Everything here is generated, so the numbers agree with each other:

- the **rig** - bones by name, parent, rest position, rest rotation and
  length, in the hips' frame (x forward, y up), a cat standing on a floor
  `FLOOR` below its hips;
- the **clips** - idle, walk and run. A leg is animated by a *foot path*
  (a stance that slides back along the floor, then a swing that lifts and
  reaches forward) solved to thigh and shin rotations with the same two-bone
  rule the engine's IK uses, so the clip's feet really are on its floor and
  `ik_two_bone.ground` only has to move them onto the terrain. All three
  clips share each leg's phase offset, so walk and run blend step for step;
- the **skin** - one `skinned_mesh`: far legs, then body, then near legs, in
  one triangle list (a mesh draws in index order, so that is the layering),
  every vertex bound to the bones it sits on with offsets taken in the bind
  pose.

`rig_entities(prefix, x, y, z)` returns the scene entities; `clips()` the
three `.skelanim` documents; `ground_height()` and friends describe the
terrain the scene lays under it.
"""

import math

FLOOR = -104.0          # the clip's floor, in the hips' frame
HIND_HIP = (8.0, -12.0)
FRONT_SHOULDER = (58.0, -14.0)   # in the chest's frame
UPPER, LOWER = 58.0, 58.0
# Each leg's phase in the cycle, the same in every gait so they blend.
# The far pair hangs a little forward of the near, so standing still the
# four legs read as four.
FAR = (12.0, 4.0)
LEGS = {
    "hn": ("hips", HIND_HIP, 0.00, True),     # hind near, stifle forward
    "hf": ("hips", (HIND_HIP[0] + FAR[0], HIND_HIP[1] + FAR[1]), 0.50, True),     # hind far
    "fn": ("chest", FRONT_SHOULDER, 0.20, False),   # front near, elbow back
    "ff": ("chest", (FRONT_SHOULDER[0] + FAR[0], FRONT_SHOULDER[1] + FAR[1]), 0.70, False),   # front far
}

# name, parent, position, rotation, length
BONES = [
    ("hips", None, (0.0, 0.0), 0.0, 78.0),
    ("chest", "hips", (78.0, 0.0), 0.0, 70.0),
    ("neck", "chest", (66.0, 6.0), 0.55, 46.0),
    ("head", "neck", (46.0, 0.0), -0.60, 50.0),
    ("tail1", "hips", (-6.0, 6.0), math.pi - 0.35, 48.0),
    ("tail2", "tail1", (48.0, 0.0), 0.30, 44.0),
    ("tail3", "tail2", (44.0, 0.0), 0.30, 40.0),
]

# gait: period (s), duty, stride, lift, bob, spine flex, neck, head, tail lift, tail sway
GAITS = {
    "idle": dict(period=2.4, duty=1.0, stride=0.0, lift=0.0, bob=1.5, flex=0.01, neck=0.62, head=-0.70,
                 tail=0.35, sway=0.12),
    "walk": dict(period=1.0, duty=0.62, stride=96.0, lift=22.0, bob=3.0, flex=0.035, neck=0.50, head=-0.55,
                 tail=0.30, sway=0.10),
    "run": dict(period=0.6, duty=0.38, stride=136.0, lift=40.0, bob=9.0, flex=0.13, neck=0.25, head=-0.30,
                tail=0.12, sway=0.16),
}
SAMPLES = 24


def ground_speed(gait):
    g = GAITS[gait]
    return 0.0 if g["stride"] == 0 else g["stride"] / (g["duty"] * g["period"])


# ── 2D helpers ────────────────────────────────────────────────────────────

def rot(v, a):
    c, s = math.cos(a), math.sin(a)
    return (v[0] * c - v[1] * s, v[0] * s + v[1] * c)


def add(a, b):
    return (a[0] + b[0], a[1] + b[1])


def sub(a, b):
    return (a[0] - b[0], a[1] - b[1])


def two_bone(root, target, l1, l2, flip):
    """World directions of the upper and lower bone that put the tip at
    `target` - the engine's rule (`animation::ik`), flip_bend included."""
    d = sub(target, root)
    dist = math.hypot(*d)
    dist = min(max(dist, abs(l1 - l2) + 1e-3), l1 + l2 - 1e-3)
    theta = math.atan2(d[1], d[0])
    a = math.acos(max(-1.0, min(1.0, (l1 * l1 + dist * dist - l2 * l2) / (2 * l1 * dist))))
    b = math.acos(max(-1.0, min(1.0, (l1 * l1 + l2 * l2 - dist * dist) / (2 * l1 * l2))))
    sign = -1.0 if flip else 1.0
    upper = theta - sign * a
    lower_local = sign * (math.pi - b)
    return upper, lower_local


# ── pose ──────────────────────────────────────────────────────────────────

def body_pose(gait, phase):
    """Hips offset, and local rotations of the spine, neck, head and tail."""
    g = GAITS[gait]
    two = math.tau * 2 * phase          # twice a cycle: once per footfall pair
    one = math.tau * phase
    hips_y = -g["bob"] * (0.5 + 0.5 * math.cos(two))
    return {
        "hips": ((0.0, hips_y), g["flex"] * 0.5 * math.sin(two)),
        "chest": ((78.0, 0.0), -g["flex"] * math.sin(two + 0.6)),
        "neck": ((66.0, 6.0), g["neck"] + 0.05 * math.sin(two + 1.2)),
        "head": ((46.0, 0.0), g["head"] - 0.04 * math.sin(two + 1.2)),
        "tail1": ((-6.0, 6.0), math.pi - g["tail"] + g["sway"] * math.sin(one)),
        "tail2": ((48.0, 0.0), 0.30 + g["sway"] * math.sin(one - 0.8)),
        "tail3": ((44.0, 0.0), 0.30 + g["sway"] * math.sin(one - 1.6)),
    }


def foot_path(gait, p, neutral_x):
    """Where a foot is at leg phase `p`, in the hips' frame."""
    g = GAITS[gait]
    if g["stride"] == 0:
        return (neutral_x, FLOOR)
    s, d = g["stride"], g["duty"]
    if p < d:
        u = p / d
        return (neutral_x + s / 2 - s * u, FLOOR)
    u = (p - d) / (1 - d)
    e = u * u * (3 - 2 * u)
    return (neutral_x - s / 2 + s * e, FLOOR + g["lift"] * math.sin(math.pi * u))


def pose(gait, phase):
    """Every animated bone's (position, rotation) at `phase`: the body from
    `body_pose`, each leg's thigh and shin solved to its foot path."""
    body = body_pose(gait, phase)
    out = dict(body)
    # World (hips-frame) transforms of the spine, to hang the legs from.
    hips_pos, hips_rot = body["hips"]
    chest_pos = add(hips_pos, rot(body["chest"][0], hips_rot))
    chest_rot = hips_rot + body["chest"][1]
    frames = {"hips": (hips_pos, hips_rot), "chest": (chest_pos, chest_rot)}
    for leg, (parent, joint, offset, flip) in LEGS.items():
        ppos, prot = frames[parent]
        root = add(ppos, rot(joint, prot))
        rest_x = joint[0] + (78.0 if parent == "chest" else 0.0)
        foot = foot_path(gait, (phase + offset) % 1.0, rest_x)
        upper_world, lower_local = two_bone(root, foot, UPPER, LOWER, flip)
        out[f"{leg}_upper"] = (joint, upper_world - prot)
        out[f"{leg}_lower"] = ((UPPER, 0.0), lower_local)
    return out


def clip(gait):
    g = GAITS[gait]
    samples = [pose(gait, i / SAMPLES) for i in range(SAMPLES + 1)]
    tracks = []
    for name in samples[0]:
        keys = [(i * g["period"] / SAMPLES, s[name]) for i, s in enumerate(samples)]
        # Unwrap rotations so interpolation never takes the long way round.
        rots, prev = [], None
        for _, (_, r) in keys:
            if prev is not None:
                while r - prev > math.pi:
                    r -= math.tau
                while r - prev < -math.pi:
                    r += math.tau
            rots.append(r)
            prev = r
        track = {"bone_name": name,
                 "rotation": [{"time": round(t, 5), "value": round(r, 5)} for (t, _), r in zip(keys, rots)]}
        if name == "hips":
            track["position"] = [{"time": round(t, 5), "value": [round(p[0], 3), round(p[1], 3)]}
                                 for t, (p, _) in keys]
        tracks.append(track)
    return {"name": f"cr_{gait}", "duration": g["period"], "tracks": tracks}


def clips():
    return {f"animations/cr_{g}.skelanim": clip(g) for g in GAITS}


# ── rig (bind pose = idle at phase 0) ─────────────────────────────────────

def bind_bones():
    """Every bone: name -> (parent, local pos, local rot, length), bind pose."""
    p = pose("idle", 0.0)
    out = {}
    for name, parent, _pos, _rot, length in BONES:
        pos, r = p[name]
        out[name] = (parent, pos, r, length)
    for leg, (parent, joint, _o, _f) in LEGS.items():
        out[f"{leg}_upper"] = (parent, p[f"{leg}_upper"][0], p[f"{leg}_upper"][1], UPPER)
        out[f"{leg}_lower"] = (f"{leg}_upper", p[f"{leg}_lower"][0], p[f"{leg}_lower"][1], LOWER)
        out[f"{leg}_paw"] = (f"{leg}_lower", (LOWER, 0.0), 0.0, 0.0)
    return out


def world_frames(bones):
    """name -> (world pos, world rot) in the hips' frame."""
    frames = {}

    def get(name):
        if name in frames:
            return frames[name]
        parent, pos, r, _ = bones[name]
        if parent is None:
            frames[name] = (pos, r)
        else:
            pp, pr = get(parent)
            frames[name] = (add(pp, rot(pos, pr)), pr + r)
        return frames[name]
    for n in bones:
        get(n)
    return frames


# ── skin ──────────────────────────────────────────────────────────────────

# Along the spine, tail tip to nose: (bone, fraction along it, thickness up, down)
SPINE = [
    ("tail3", 1.00, 3, 3), ("tail3", 0.5, 5, 4), ("tail3", 0.0, 7, 6),
    ("tail2", 0.5, 8, 7), ("tail2", 0.0, 10, 8), ("tail1", 0.5, 12, 10),
    ("tail1", 0.0, 18, 16),
    ("hips", 0.0, 30, 40), ("hips", 0.35, 30, 42), ("hips", 0.7, 29, 40),
    ("chest", 0.0, 30, 40), ("chest", 0.35, 32, 44), ("chest", 0.7, 32, 44), ("chest", 1.0, 28, 34),
    ("neck", 0.35, 25, 25), ("neck", 0.75, 22, 20), ("head", 0.0, 23, 18),
    ("head", 0.35, 22, 17), ("head", 0.7, 16, 13), ("head", 0.95, 9, 8), ("head", 1.08, 4, 4),
]
# Along a leg, hip to toe: (bone, fraction, half width)
LEG = [("upper", -0.25, 24), ("upper", 0.0, 22), ("upper", 0.45, 16), ("upper", 0.85, 11), ("lower", 0.1, 10),
       ("lower", 0.6, 8), ("lower", 0.95, 9), ("lower", 1.12, 8)]


def _skin(bones):
    frames = world_frames(bones)
    names = list(bones)
    index = {n: i for i, n in enumerate(names)}
    verts, idx = [], []

    def point(bone, f):
        pos, r = frames[bone]
        return add(pos, rot((bones[bone][3] * f, 0.0), r))

    def influences(v, pairs):
        out = []
        total = sum(w for _, w in pairs)
        for b, w in pairs:
            pos, r = frames[b]
            off = rot(sub(v, pos), -r)
            out.append({"bone_index": index[b], "weight": round(w / total, 4), "offset": [round(off[0], 3), round(off[1], 3)]})
        return out

    def neighbours(chain, i):
        """A vertex near a joint leans on both bones there."""
        bone, f = chain[i][0], chain[i][1]
        if f <= 0.12 and i > 0 and chain[i - 1][0] != bone:
            return [(bone, 0.6), (chain[i - 1][0], 0.4)]
        if f >= 0.88 and i + 1 < len(chain) and chain[i + 1][0] != bone:
            return [(bone, 0.6), (chain[i + 1][0], 0.4)]
        return [(bone, 1.0)]

    def strip(chain, v0, v1, width_of):
        """A strip along `chain` - top and bottom vertices per point - on
        texture rows v0..v1, u along its length."""
        base = len(verts)
        pts = [point(c[0], c[1]) for c in chain]
        n = len(pts)
        for i, c in enumerate(chain):
            a = pts[max(0, i - 1)]
            b = pts[min(n - 1, i + 1)]
            tx, ty = b[0] - a[0], b[1] - a[1]
            ln = math.hypot(tx, ty) or 1.0
            nx, ny = -ty / ln, tx / ln          # left of travel: up for the body
            up, down = width_of(c)
            u = i / (n - 1)
            pairs = neighbours(chain, i)
            for (dist, v) in ((up, v0), (-down, v1)):
                p = (pts[i][0] + nx * dist, pts[i][1] + ny * dist)
                verts.append({"rest_position": [round(p[0], 3), round(p[1], 3)], "uv": [round(u, 4), v],
                              "influences": influences(p, pairs)})
        for i in range(n - 1):
            a, b, c, d = base + 2 * i, base + 2 * i + 1, base + 2 * i + 2, base + 2 * i + 3
            idx.extend([a, b, d, a, d, c])

    def leg(name, v0, v1):
        chain = [(f"{name}_{part}", f, w) for part, f, w in LEG]
        strip(chain, v0, v1, lambda c: (c[2], c[2]))

    # Far legs first: they draw behind the body.
    leg("hf", 0.76, 0.99)
    leg("ff", 0.76, 0.99)
    strip(SPINE, 0.01, 0.49, lambda c: (c[2], c[3]))
    leg("hn", 0.51, 0.74)
    leg("fn", 0.51, 0.74)
    return names, verts, idx


def rig_entities(prefix, x, y, z, clip_id, tex="sprites/env/cr_skin.png", ground_mask=65535):
    """The creature: a root at its hips carrying the skeleton, player and
    skin, and one entity per bone. Paws carry foot IK against the ground."""
    bones = bind_bones()
    names, verts, idx = _skin(bones)
    root = {"name": prefix, "components": {
        "transform": {"position": [x, y], "rotation": 0.0, "scale": [1.0, 1.0]},
        "skeleton_root": {"name": prefix},
        "skeletal_player": {"clip": clip_id, "time": 0.0, "speed": 1.0, "looping": True, "playing": True},
        "skinned_mesh": {"texture_id": tex, "color": [1.0, 1.0, 1.0, 1.0], "z_order": z,
                         "bones": names, "vertices": verts, "indices": idx},
    }}
    out = [root]
    for name, (parent, pos, r, length) in bones.items():
        comps = {"transform": {"position": [round(pos[0], 4), round(pos[1], 4)], "rotation": round(r, 5),
                               "scale": [1.0, 1.0]},
                 "bone": {"name": name, "length": length}}
        if name.endswith("_paw"):
            flip = LEGS[name[:2]][3]
            comps["ik_two_bone"] = {"target": [0.0, 0.0], "flip_bend": flip, "weight": 1.0,
                                    "ground": {"floor": FLOOR, "reach": 160.0, "mask": ground_mask}}
        out.append({"name": f"{prefix}_{name}", "parent": prefix if parent is None else f"{prefix}_{parent}",
                    "components": comps})
    return out


def ear_and_eye(prefix, z):
    """Sprites riding the head bone: an ear and an eye."""
    return [
        {"name": f"{prefix}_Ear", "parent": f"{prefix}_head", "components": {
            "transform": {"position": [10.0, 18.0], "rotation": 0.35, "scale": [1.0, 1.0]},
            "sprite": {"texture_id": "sprites/env/cr_ear.png", "color": [1.0, 1.0, 1.0, 1.0], "z_order": z - 1,
                       "size": [22.0, 26.0], "anchor": [0.5, 0.0]}}},
        {"name": f"{prefix}_Eye", "parent": f"{prefix}_head", "components": {
            "transform": {"position": [24.0, 5.0], "rotation": 0.0, "scale": [1.0, 1.0]},
            "sprite": {"texture_id": "sprites/fx/soft_dot.png", "color": [0.05, 0.04, 0.02, 1.0], "z_order": z + 1,
                       "size": [7.0, 6.0], "anchor": [0.5, 0.5]}}},
    ]


# ── terrain ───────────────────────────────────────────────────────────────

PERIOD = 3600.0
STEP = 30.0
BASE = -150.0


def ground_height(x):
    """The terrain, periodic in PERIOD: rolling hills, a steeper rise and a
    few rocks - every wave a whole number of cycles per period."""
    t = x / PERIOD * math.tau
    h = 38 * math.sin(t * 2 + 0.5) + 22 * math.sin(t * 5 + 1.3) + 9 * math.sin(t * 13 + 0.2)
    for cx, w, a in ((700.0, 60.0, 26.0), (1650.0, 45.0, 20.0), (2500.0, 70.0, 30.0), (3150.0, 40.0, 16.0)):
        d = ((x - cx + PERIOD / 2) % PERIOD) - PERIOD / 2
        h += a * math.exp(-(d / w) ** 2)
    return BASE + h


def ground_points(x0, x1):
    n = int((x1 - x0) / STEP)
    return [(x0 + i * STEP, ground_height(x0 + i * STEP)) for i in range(n + 1)]
