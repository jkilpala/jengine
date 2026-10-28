# Visual Showcase 2 - HD-2D

Scenes in the **HD-2D** style: pixel-art people standing in a world
drawn in perspective, lit per pixel, and seen through a lens - depth of field,
bloom, a vignette. The first [Visual Showcase](../visual-showcase/README.md)
asked whether a 2D game on jEngine can look as good as one made in Unity or
Godot; this one asks whether it can look like *that*.

The answer needed four engine features, built for it (Oct 2026):

| Feature | What it does | Where |
|---|---|---|
| **3D stage** | A perspective camera over flat pieces placed in three dimensions: `stage_quad`s (floors, walls, roofs) projected into meshes in true perspective, `stage_billboard`s standing sprites, lights and emitters in it, all sorted by how far into the scene they stand, floors under everything | `ecs::stage`, test-lab scene 296 |
| **Depth of field by stage depth** | The velocity pass draws meshes now, and a stage object hands it its depth - per vertex for a quad, interpolated in perspective - so the floor is sharp at the traveller and softens smoothly toward the horizon | `render_velocity_wgpu`, `velocity.wgsl` |
| **Normal-mapped meshes** | The normal pass draws meshes too, in perspective, so a wall made of a stage quad shows its bricks under the light instead of lighting flat | `render_normals_wgpu`, `lit_sprite.wgsl` |
| **Shadows from stage lights** | A billboard's sprite frame projected from each nearby lamp's 3D position onto the floor: long from a low lamp, short under a high one, turning as the traveller walks past. `cast_shadow` could not: it lays a silhouette in screen space and never knew where a light is in depth | `stage::shadow_quads`, `render_world_items` |
| **Solids' shadows** | A `stage_caster` - a house's footprint and height - throws one shadow a lamp: the hull of its footprint and its top projected from the lamp onto the floor, as dark as the lamp is strong there, so a house never darkens itself twice and the shadow fades where the lamp's light does | `stage::caster_shadow` |
| **Depth haze** | A post effect mixing each pixel toward a haze colour by its depth - the stage's, read from the velocity pass as depth of field reads it - so far roofs and hills go pale and blue | `depth_haze.wgsl` |
| **Stage groups** | A `stage_group` is a frame the pieces under it stand in, moved and turned as one: the battle's ruins are each a group turned its own way, a wall with its rubble | `stage::Frames`, test-lab scene 296 |
| **Squashed lights** | `squash` on a point or spot light makes its pool an ellipse - a lamp's pool on a floor seen in perspective. The stage sets it from the pitch | `point_light.wgsl`, `lighting::squashed_distance`, test-lab scene 297 |

## Running it

Open `projects/visual-showcase-2` in the editor, or natively:

```powershell
cargo build --release -p jengine-runtime
cd projects/visual-showcase-2
../../target/release/jengine-runtime.exe --scene assets/scenes/hub.scene
```

No Rust packages, so the stock runtime and editor viewport are enough.
`tools/capture.ps1 -Scene 01-lantern-square -Out $env:TEMP\square.png` takes
screenshots without the editor.

Every scene but the battle: **Arrows / WASD** walk, **Q / E** turn the camera, **1-5** depth
of field, bloom, vignette, grade and haze on / off, **Tab** captions, **PgUp /
PgDn** previous / next, **H / Esc** the hub.

## How it is made

Everything is generated, as in the first showcase:

- `tools/pixel.py` - palettes, drawing into small canvases, outlines, height
  fields to normal maps. **Scenery is pixel art drawn small and written four
  times larger**: the project filters it linearly with mipmaps
  (`texture_filter`), which on a 4x texture is crisp up close and smooth where
  a floor recedes, where nearest sampling would shimmer. Characters are drawn
  nearest at their own size (`sprites/hd/chars/`).
- `tools/chars.py` - the cast, part by part, three heads tall, lit from the
  upper left, with a **selective outline** (each outline texel a dark copy of
  what it borders). Six frames a sheet: two idle, four walking.
- `tools/monsters.py`, `tools/props.py`, `tools/surfaces.py` - the battle's
  monsters, standing props (trees are cards, as HD-2D draws its foliage) and
  tiled surfaces.
- `tools/gen_art.py` writes all of it; `tools/gen_scenes.py` writes every
  scene through `tools/hd2d.py`, which builds floors, walls, boxes and gabled
  houses out of stage quads.
- `assets/scripts/explore.lua` walks the hero relative to the camera, slides
  along house footprints, and walks NPCs round their loops.

## Scenes

| # | Scene | Look |
|---|---|---|
| 01 | **Lantern Square** | A market square at dusk: timbered houses, warm windows, lamps pooling light on the cobbles, a river and bridge, chimney smoke, the square's evening going on round the traveller |
| 02 | **Crystal Hollow** | A cave: torches with animated flames and embers, crystals in their own cold light, a shaft of daylight with dust turning in it, and the traveller's lantern lighting the rock round them as they walk |
| 03 | **Forest Shrine** | An autumn wood: oak cards all round and over the lens, sun shafts through the canopy that brighten and fade as clouds pass, falling leaves, stone lanterns up to a shrine |
| 04 | **The Tavern** | A room seen as a dollhouse: the hearth, candles and hanging lamps, regulars going between the bar, the tables and the fire; the camera turns only a little |
| 05 | **Snowbound** | A village under snow at night: snowy roofs, warm windows, lamps, chimney smoke and snow falling across the whole view, cold moonlight |
| 06 | **The Battle** | A side-on battle view: the party on the right, three monsters on the left, on a stepped dais in a ruin - broken walls, an arch and columns turned their own ways at different depths, banners at the corners, long grass blurred at the lens. Space casts fire, frost and light, each a burst, a flash of light over the stage, the monster's flinch, a shake and the camera leaning in |

## What it found

Kept here as it is found - the first showcase's "what the renderer cannot do
yet", for HD-2D.

- **A mesh lit flat whatever its normal map** - the normal pass drew sprites
  only. Fixed: meshes take normal maps, in perspective.
- **A mesh had no depth for depth of field** - the velocity pass drew sprites
  only, so every mesh was "unknown depth" (sharp, behind everything). Fixed,
  and a stage quad gives a depth per vertex - which first came out wrong across
  a deep floor, interpolated straight across the screen; it is carried times
  the perspective weight and divided per pixel now, as the texture is.
- **A lamp's pool was a circle on a floor seen at an angle.** Fixed: `squash`.
- **Nothing threw a shadow from a lamp.** The blob under each character was
  all there was, and `cast_shadow` (screen space) barely read on the cobbles
  and pointed the wrong way, not knowing where the lamp stood in depth.
  Fixed: billboards throw shadows projected from each stage light.
- **The battle was flat**: a rug of cobbles on the grass, a row of ruins all
  facing the lens, nothing in front. It is a stepped dais now, ruins turned
  and staggered in depth, banners and blurred grass framing it, the camera
  turned a little west so the line of battle runs across the picture.
- **A house could not be moved as one** - every piece stood at absolute stage
  coordinates - and a parent's 2D transform moved its projected pieces again,
  flat (found by jSuite's stage editor). Fixed: `stage_group`, and stage
  pieces are roots to the 2D hierarchy.
