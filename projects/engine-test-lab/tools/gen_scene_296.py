"""Scenes 296 (a 3D stage) and 297 (normal-mapped meshes and squashed
lights). Run from anywhere: writes assets/scenes/296-stage.scene and
297-mesh-normals.scene."""

import json
import os

HERE = os.path.dirname(os.path.abspath(__file__))
SCENES = os.path.join(HERE, "..", "assets", "scenes")


def T(x=0.0, y=0.0):
    return {"position": [float(x), float(y)], "rotation": 0.0, "scale": [1.0, 1.0]}


def ui_text(name, anchor, offset, size, text, font, color, align="Center"):
    return {"name": name, "components": {
        "ui_element": {"anchor": anchor, "offset": list(map(float, offset)), "size": list(map(float, size)), "z_order": 100},
        "ui_text": {"text": text, "font_size": float(font), "color": color, "alignment": align}}}


def chrome(title, hint, expect1, expect2, script):
    return [
        ui_text("Title", "TopCenter", (0, -30), (1100, 44), title, 30, [0.85, 0.88, 0.95, 1]),
        ui_text("Hint", "TopCenter", (0, -80), (1250, 24), hint, 15, [0.6, 0.65, 0.75, 1]),
        ui_text("Line0", "TopLeft", (40, -110), (1100, 26), "", 18, [0.85, 0.88, 0.95, 1], "Left"),
        ui_text("Expect1", "BottomCenter", (0, 40), (1250, 22), expect1, 15, [0.7, 0.75, 0.85, 1]),
        ui_text("Expect2", "BottomCenter", (0, 16), (1250, 22), expect2, 15, [0.7, 0.75, 0.85, 1]),
        {"name": "Controller", "components": {"transform": T(), "script": {"language": "Lua", "source_path": script}}},
    ]


def quad(name, tex, origin, u, v, repeat=(1, 1), ground=False, color=(1, 1, 1, 1)):
    return {"name": name, "components": {
        "transform": T(),
        "mesh": {"texture_id": tex, "color": list(map(float, color))},
        "stage_quad": {"origin": list(map(float, origin)), "u": list(map(float, u)), "v": list(map(float, v)),
                       "repeat": list(map(float, repeat)), "ground": ground}}}


def box(name, x0, y0, x1, y1, h, color, group=False):
    """A box of five quads and its shadow. `group`: built round its middle
    in a `stage_group` of its own, so turning the group turns the box."""
    if group:
        cx, cy = (x0 + x1) / 2, (y0 + y1) / 2
        out = box(name, x0 - cx, y0 - cy, x1 - cx, y1 - cy, h, color)
        for e in out:
            e["parent"] = name
        # A 2D transform on the group, which a stage piece must not take.
        return [{"name": name, "components": {"transform": {"position": [150.0, 40.0], "rotation": 0.5, "scale": [1.5, 1.5]},
                                              "stage_group": {"position": [cx, cy, 0.0], "yaw": 0.0}}}] + out
    walls = [((x0, y0), (x1, y0)), ((x1, y1), (x0, y1)), ((x0, y1), (x0, y0)), ((x1, y0), (x1, y1))]
    out = []
    for i, (a, b) in enumerate(walls):
        length = abs(b[0] - a[0]) + abs(b[1] - a[1])
        out.append(quad(f"{name}{'SNWE'[i]}", "sprites/checker64.png", (a[0], a[1], 0), (b[0] - a[0], b[1] - a[1], 0),
                        (0, 0, h), (length / 2, h / 2), color=color))
    # Its shadow from the lamp (`stage_caster`).
    out.append({"name": name + "Shadow", "components": {"stage_caster": {
        "footprint": [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], "height": float(h), "opacity": 0.75}}})
    out.append(quad(name + "Top", "sprites/checker64.png", (x0, y0, h), (x1 - x0, 0, 0), (0, y1 - y0, 0),
                    ((x1 - x0) / 2, (y1 - y0) / 2), color=[min(1, c * 1.25) for c in color[:3]] + [1]))
    return out


def scene_296():
    ents = [
        {"name": "Camera", "components": {"transform": T(), "camera": {"zoom": 1.0, "is_active": True,
                                                                      "viewport_width": 1280.0, "viewport_height": 720.0}}},
        {"name": "Sky", "components": {"transform": T(0, 0), "sprite": {"texture_id": "sprites/white32.png",
                                                                       "size": [1400.0, 800.0], "color": [0.07, 0.09, 0.16, 1.0],
                                                                       "z_order": -30000}}},
        {"name": "Ambient", "components": {"ambient_light": {"color": [0.55, 0.6, 0.8], "intensity": 0.35}}},
        {"name": "Stage", "components": {"transform": T(0, -40), "stage": {
            "target": [0.0, 0.0, 0.5], "yaw": 0.0, "pitch": 0.6, "distance": 40.0, "zoom": 40.0,
            "follow": "Walker", "follow_offset": [0.0, 0.0, 0.5], "follow_ease": 3.0}}},
        quad("Floor", "sprites/checker64.png", (-12, -12, 0), (24, 0, 0), (0, 24, 0), (12, 12), ground=True,
             color=(0.55, 0.6, 0.65, 1)),
    ]
    ents += box("Red", -6, 2, -3, 5, 3, [0.95, 0.45, 0.4, 1], group=True)
    ents += box("Blue", 3, -1, 6, 2, 2, [0.45, 0.6, 1.0, 1])
    ents += box("Tall", -1, 7, 1, 9, 5, [0.5, 0.9, 0.55, 1])
    for i in range(8):
        x = -7 + i * 2
        ents.append({"name": f"Post{i}", "components": {
            "transform": T(), "sprite": {"texture_id": "sprites/circle16.png", "size": [26.0, 26.0], "z_order": 0,
                                         "color": [1.0, 0.85, 0.3, 1], "anchor": [0.5, 0.0]},
            "stage_billboard": {"position": [float(x), -6.0, 0.0]}}})
    ents.append({"name": "Walker", "components": {
        "transform": T(), "sprite": {"texture_id": "sprites/npc_villager.png", "size": [48.0, 64.0], "anchor": [0.5, 0.0],
                         "color": [1.0, 1.0, 1.0, 1.0], "z_order": 0},
        "stage_billboard": {"position": [0.0, 0.0, 0.0]}}})
    ents.append({"name": "Lamp", "components": {
        "transform": T(), "point_light": {"color": [1.0, 0.75, 0.4], "intensity": 1.6, "radius": 100.0, "falloff": 1.4,
                                          "casts_shadows": False},
        "stage_billboard": {"position": [0.0, 0.0, 1.0], "light_radius": 5.0, "follow": "Walker"}}})
    ents += chrome("296 - 3D stage",
                   "[H/Esc] hub   Left / Right turn   Up / Down tilt   Q / W nearer / farther   A auto   D depth of field   Z haze   G turn the red box",
                   "Expect: a checkered floor and three boxes in perspective, the faces turned away hidden as the camera turns; posts",
                   "and the walker smaller further off; the lamp's pool an ellipse, each box throwing one shadow; G turns the red box (a group) in place.",
                   "scripts/lua/scene_296_stage.lua")
    post = {"effects": [{"name": "depth_haze", "enabled": True, "params": {
        "color": {"Vec4": [0.3, 0.38, 0.6, 0.7]}, "near": {"Float": -30.0}, "far": {"Float": -150.0},
        "sky": {"Float": 0.0}, "curve": {"Float": 1.0}}},
                        {"name": "depth_of_field", "enabled": True, "params": {
        "focus": {"Float": 0.0}, "range": {"Float": 15.0}, "strength": {"Float": 0.08}, "max_radius": {"Float": 6.0}}}]}
    return {"name": "296 - 3D stage", "post_process": post, "entities": ents}


def scene_297():
    ents = [
        {"name": "Camera", "components": {"transform": T(), "camera": {"zoom": 1.0, "is_active": True,
                                                                      "viewport_width": 1280.0, "viewport_height": 720.0}}},
        {"name": "Ambient", "components": {"ambient_light": {"color": [1.0, 1.0, 1.0], "intensity": 0.08}}},
        # Left: a mesh, a trapezoid in perspective (its far edge carries a
        # smaller weight). Right: the same picture as a sprite.
        {"name": "MeshBumps", "components": {
            "transform": T(-300, -40),
            "mesh": {"texture_id": "sprites/bumps128.png", "vertices": [
                {"position": [-200.0, -160.0], "uv": [0.0, 1.0], "color": [1, 1, 1, 1], "q": 1.0},
                {"position": [200.0, -160.0], "uv": [1.0, 1.0], "color": [1, 1, 1, 1], "q": 1.0},
                {"position": [120.0, 160.0], "uv": [1.0, 0.0], "color": [1, 1, 1, 1], "q": 0.6},
                {"position": [-120.0, 160.0], "uv": [0.0, 0.0], "color": [1, 1, 1, 1], "q": 0.6}]},
            "material": {"material_id": "lit_sprite", "normal_map_id": "sprites/bumps128_n.png"}}},
        {"name": "SpriteBumps", "components": {
            "transform": T(300, -40),
            "sprite": {"texture_id": "sprites/bumps128.png", "size": [320.0, 320.0], "color": [1.0, 1.0, 1.0, 1.0],
                       "z_order": 0},
            "material": {"material_id": "lit_sprite", "normal_map_id": "sprites/bumps128_n.png"}}},
        # Two lamps in one place, round and squashed; S swaps which is lit.
        {"name": "Lamp", "components": {
            "transform": T(0, -40), "point_light": {"color": [1.0, 0.85, 0.6], "intensity": 1.4, "radius": 520.0,
                                                    "falloff": 1.3, "height": 0.25, "casts_shadows": False}}},
        {"name": "LampSquashed", "components": {
            "transform": T(0, -40), "point_light": {"color": [1.0, 0.85, 0.6], "intensity": 0.0, "radius": 520.0,
                                                    "falloff": 1.3, "height": 0.25, "casts_shadows": False,
                                                    "squash": 0.5}}},
    ]
    ents += chrome("297 - Mesh normal maps, squashed lights",
                   "[H/Esc] hub   S squash on / off   N normal maps on / off   Space hold the lamp",
                   "Expect: the left trapezoid (a mesh) shows its bumps lit from the lamp exactly as the right square (a sprite) does;",
                   "N flattens both. S squashes the lamp's pool to an ellipse half as tall as it is wide; off, a circle.",
                   "scripts/lua/scene_297_mesh_normals.lua")
    return {"name": "297 - Mesh normal maps, squashed lights", "entities": ents}


def main():
    for name, scene in (("296-stage.scene", scene_296()), ("297-mesh-normals.scene", scene_297())):
        with open(os.path.join(SCENES, name), "w", encoding="utf-8", newline="\n") as f:
            json.dump(scene, f, indent=1)
            f.write("\n")


if __name__ == "__main__":
    main()
