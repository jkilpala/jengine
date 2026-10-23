# Visual Showcase

Scenes that push jEngine's renderer as far as it goes, to answer one question:
**can a 2D game made on this engine look as good as one made in Unity or
Godot?** Each scene targets a look those engines are known for and builds it
from the engine's own parts: lights, occluders, lightmaps, particles,
materials, and the post chain.

This is not the Engine Test Lab. A test-lab scene shows that a feature
*works*. A showcase scene shows how *good* the feature can look, and where it
runs out. When a scene runs into a limit, the limit goes in "What the
renderer cannot do yet" below, so the list becomes the renderer's to-do list.

## Running it

Open `projects/visual-showcase` in the editor, or natively:

```powershell
cargo build --release -p jengine-runtime
cd projects/visual-showcase
../../target/release/jengine-runtime.exe --scene assets/scenes/01-firelight.scene
```

The project has no Rust packages, so the stock runtime and the stock editor
viewport are enough.

Every scene: **H / Esc** back to the hub, **PgUp / PgDn** (or `[` / `]`)
previous / next, **Tab** hides the captions.

## How it is made

Everything is generated, not hand-authored:

- `tools/gen_textures.py` writes all the art into `assets/sprites/`: glows,
  flames, smoke, sparks, a moonlight lightmap, and surfaces with normal maps
  (flagstones, bricks, a fluted column, a barrel, a crate, a chain) - each a
  height field whose gradient is the normal map. It uses numpy and Pillow. The FX textures are white with
  the shape in alpha, so a sprite's colour tints them.
- `tools/gen_scenes.py` writes every `.scene`. A visual scene is mostly many
  copies of a few things (pillars in a ring, flames in a brazier), and a loop
  keeps them consistent where forty copied JSON blocks would drift.
- `assets/scripts/nav.lua` holds the scene list. The hub builds its menu from
  it, and every scene's prev / next walks it.

Colours are **linear** (see "Colours are linear" in the root `CLAUDE.md`).
`lin()` in the generator converts a colour picked in sRGB. A colour a script
sets is linear too, so copy the generator's value rather than the picked one.

### Adding a scene

1. Add a function to `tools/gen_scenes.py` that returns the scene, and a row
   in its `SCENES` map. Start from `firelight()`, and include
   `caption_entities(...)` so the scene has a title, a hint line and a status
   line.
2. Add `assets/scripts/sNN_name.lua`: call `nav.tick(SCENE)` from `update`,
   and route Tab to `nav.toggle_captions`.
3. Add a row to `M.SCENES` in `assets/scripts/nav.lua`: path, title and a
   blurb for the hub. The hub has room for sixteen.
4. Run `python tools/gen_scenes.py` (and `gen_textures.py` if you added art).

## Scenes

| # | Scene | Look | Leans on |
|---|---|---|---|
| 01 | **Firelight** | A stone hall lit by a brazier; moonlight through a window | Flickering point lights with soft shadows, a lightmap, additive glows, three particle layers, lit dust, the full post chain with per-effect toggles |
| 02 | **Neon Rain** | A wet street at night under neon signs | Neon tubes as unlit `vector_path` strokes brighter than white, with a hot core, so HDR bloom makes glass of them; the signs and a street lamp are real lights on the facades and the road; stretched additive rain lit only where light falls; reflection streaks and ripples on the road; a sign that stutters (R toggles rain, T lightning). Far windows and the shop window are `Unlit` and hidden by what stands in front of them |
| 03 | **Crypt** | A torchlit crypt, side-on, every surface normal-mapped | Deferred normal maps on bricks, flagstones, fluted columns, barrels, crates and chains; round things (columns, barrels) carry a cylinder in their normals so they shade as round; four flickering torches, floor candles that rake the flagstones, and a wisp on the mouse. N takes every normal map away and gives it back (`Material.set_normal_map`), L puts the torches out. Columns, barrels and crates carry box occluders and are still lit themselves |
| 04 | **Stained Glass** | A cathedral nave from above, the sun beyond its windows | Every pane is an occluder with a `shadow_tint`, so the coloured bands on the floor are the sun's own light through the glass, and they sweep over the pews as the sun moves (Left / Right steer it). Pews cast short shadows (`shadow_length`), the dust glitters in the colours, beams of dusty air are aimed from the sun through each pane, and the flagstones, carpet and pews are normal-mapped |
| 05 | **Nebula** | Deep space: glowing gas, dust lanes, stars, a planet | Clouds are additive and brighter than white, so their hearts bloom; dust lanes are a multiply layer cutting through the glow; two parallax star layers and spiked bright stars; a normal-mapped planet and a cratered moon under a low sun (`height`) with a real night side, the terminator sweeping as the star swings; meteors; the mouse shifts every layer by its depth |
| 06 | **Shader Gallery** | Six effects, each a shader graph of nodes | A crate dissolving along noise with a burning edge brighter than white, a hologram with scan lines and flicker, water whose caustics are two drifting Voronoi layers' edges, an energy shield of Voronoi cells lit by a pulse, lava from noise warped by noise, and heat haze over a fire - the brick wall behind it read back through a Scene Color node and pushed sideways by rising noise. The graphs are written by `tools/gen_shaders.py` - the same JSON the editor's shader graph editor opens |
| 07 | **Spellcraft** | A mage in a dark courtyard where every light is a spell | A summoning circle lying *on* the floor, its runes turning inside the floor's perspective (a turned child under a squashed parent - shear); fireballs are spawned prefabs carrying their own light and trail, so the posts' bounded shadows swing as they pass; every blast is sparks, smoke, a flash of light, a scorch (a soft multiply decal) and a shockwave graph that bends the lit scene behind it; lightning is a chain of stretched segments re-forked every few frames; frost nova throws shards across the floor plane. Click, right-click and F cast; it casts on its own when left alone |
| 08 | **Underwater** | A reef floor under a sunlit surface | The caustics rippling over the normal-mapped sand are *light*: a lightmap drawn by a graph material (two drifting Voronoi webs, warped by noise), so they light the rocks, the kelp and the fish swimming through them. God rays sway down to the floor, the far edge of the sand melts into the water, kelp is a chain of parented segments that bends as it sways, a school of fish flocks and scatters from the jellyfish on the mouse (whose light the rocks shadow), bubbles rise from vents, marine snow shows only where light reaches, and the whole view wobbles through a Scene Color graph (W toggles it) |
| 09 | **Ten Thousand Lights** | A lantern festival over a lake at night | A scale test: ten thousand sky lanterns, every one an entity the engine moves itself (`velocity` + `wrap_bounds`, no script per lantern), in three depths - a haze of far sparks behind the hills, nearer ones in front. Three hundred carry real lights that catch the normal-mapped hills, more than the 256 a frame draws, so the pass draws the ones in view. The lake is a Scene Color graph reading the sky flipped about the horizon and rippled, so every lantern has its reflection. The status line reads the frame rate (about 120 fps natively, with the mirror) |
| 10 | **Clay** | A white room of white primitives, seen from above - a clay render | Shape comes only from light: bevelled boxes, domed spheres and rounded cylinders are plain white with normal maps. Ambient occlusion (from the objects' own occluders) darkens where things meet - the foot of every wall, the room's corners, the narrow gap between two cubes, around every base - and never an object's own top; a soft sun on the mouse casts shadows that end, longer for taller things. A turns the occlusion off to see what it adds, S the sun to see it alone |
| 11 | **Trails** | A meteor shower over dark hills, and things that leave light behind them | Every streak is a `trail` component - a ribbon through the points its entity has been. Thin meteors taper to nothing and pass behind the hills (a trail sorts by z like a sprite); the rarer fireballs carry lights across the hills' normal maps. The mouse leads a wisp whose long ribbon paints, curving however fast the mouse moves. Click (or Space) and a blade slashes round it: a trail on the tip of a turning pivot, emitting only for the swing (`Trail.set_emitting`), with a flash of light on the hills. Fireflies loop over the meadow on oscillators. M stops the meteors |
| 12 | **Pixel Perfect** | A pixel-art cliffside at dawn with a waterfall, drawn as a 320x180 game would be | Real pixel art - palette-indexed, outlined, dithered - each art pixel 4x4 world units, sampled nearest (the project filters everything else). The `pixelate` post effect point-samples the frame on the art's own grid, so the windmill's turning sails stay on it instead of breaking into finer "mixels" (P turns it off), and the grid moves with the world, so the slow camera pan scrolls smoothly without shimmer. The water flows by palette cycling alone - the `palette_cycle` graph turns eight colours kept in each texture's last row. C lays a CRT over it: curved glass, scanlines on the art's rows, an RGB mask. The captions are entity UI, sharp over the pixelated world |
| 13 | **Day & Night** | A small town from above, through a whole day in a minute | The sun is a `directional_light`: every shadow points the same way, swings from west to east as its entity turns, and stretches long at dawn and dusk as `Lighting.set_height` lowers it; the normal-mapped roofs catch it on one slope in the morning and the other in the evening. Ambient, grade and the pond (which shows the sky) run through dawn, noon, sunset and dusk from one script. At dusk the lamps come on one after another, the doorsteps light, the moon rises, stars show in the pond and lamp-lit fog gathers. Space pauses, F runs four times as fast; the status line reads the clock and the frame rate |
| 14 | **Toon & Outlines** | A cel-shaded meadow of round creatures | `toon_lighting` steps the light into three crisp bands, so every normal-mapped dome, rock and swell of the ground falls into flat patches of light, mid-tone and shade. Every shape carries an ink line drawn by the `ink_outline` graph, which samples its texture on eight sides a few texels out (a `TexelSize` node), so the line is one weight on every sprite; hovering a creature turns its line gold. A warm key light rides the mouse and a low cool rim light catches the backs of the domes. T and O turn the toon lighting and the lines off |
| 15 | **Still Water** | A mountain lake at dusk that mirrors everything above it | The water is one sprite and one graph: each pixel reads the scene at its mirror image across the surface - a `WorldToScreen` node on `(x, 2 * surface - y)` minus its own `ScreenPosition`, into `SceneColor` - so the mountains, pines, the cabin's lit windows, the pier's lanterns and the moon are reflected however the camera drifts and zooms (09's lake mirrors a fixed fraction of the screen and could not). Small waves bend the reflection, finer toward the viewer; a click drops a stone whose ring, squashed three to one as water seen low is, runs out bending it; the moon's glint breaks across the surface. W raises the wind |
| 16 | **Festival Night** | Fireworks over a harbour town | Every burst is hundreds of sparks that fly out fast and hang - particle `drag`, the same at any frame rate - then fall, each drawn along its flight (`stretch`), white-hot to colour to embers on a colour curve. Willows crackle as they fall; the smoke curls on a `turbulence` flow, the curl of noise, and is lit by the next burst. Each burst's light flares over the town, the boats and the smoke, and the harbour mirrors all of it with Still Water's graph. Shells rise on ribbons (`trail`). Click sends one to the mouse, Space a salvo, F the finale |
| 17 | **Murmuration** | Thousands of starlings over winter fields at sunset | 4,500 birds (Up to twelve thousand) flocking in the `jengine-swarm` package, each steering off its nearest neighbours through a spatial grid, none of them known to a script. The flock wheels after a point the script moves - the package's `target_x` / `target_y` - folding and stretching as it turns; every bird flaps on its own phase. The mouse is a falcon (it hunts alone when the mouse is still), and the flock tears open around it and closes behind |
| 18 | **Graveyard Watch** | A chapel graveyard at night in 320x180 pixel art | Lit as a palette-bound game had to be: `toon_lighting` with `dither` brings light in four levels with every step between two drawn as a 4x4 ordered pattern, one cell per art pixel, anchored to the world - so it sits on the art's own grid and never crawls as the lantern (the mouse) pushes its pool of light across stone, graves and grass, or as the torches breathe. The stained glass and the ghost are unlit and glow. Pixelate and a CRT over it all; D compares the dither with hard bands and smooth light |
| 19 | **God Rays** | A forest at morning, the sun behind the canopy, dust and leaves drifting through its light | Light shafts: the new `light_shafts` post effect marches every pixel to the sun and gathers what is brighter than its threshold, so only the sun casts, the canopy and the trunks cut it, and the beams are the gaps. The sun follows the mouse - behind the dense leaves it streams through in thin rays, in a clearing it floods the wood - and the source is a world point handed to the effect every frame. G turns the shafts off to see what they add; Space sends a gust through the leaves |
| 20 | **Storm** | A village street at night in a storm: rain, lightning, gusts, a lamp and lit windows on wet cobbles | The rain **lands** - on the roofs, the walls and the street, the same collider shapes physics uses - and a sub-emitter splashes each drop where it hit, with no script knowing where a roof is. Lightning lights the whole street for a flicker (ambient, sky and puddles together) and forks down behind the cloud; gusts swing the rain to a new slant through `Particles.set` and flatten the grass along the verge, a mesh reshaped every frame. L lightning, G a gust, R the rain |
| 21 | **The Creature** | A big cat crossing rolling ground in the afternoon, walking into a run and stopping to stand | One skeletal player walks into a run by pace - `Animation.set_blend` mixes the run clip in at the same phase, so the legs never lose step - and stops and starts with `Animation.crossfade`. Each paw's foot IK (`ik_two_bone.ground`) takes the foot where the clip put it and plants it on the terrain's collider, so a walk authored on a flat floor climbs hills and steps over rocks. One skinned mesh - far legs, body, near legs - bends at every joint. Rig, clips and skin are generated together (`tools/creature.py`), each leg's clip solved from a foot path with the engine's own IK rule. Up / Down pace, Space stop / go, I foot IK |
| 22 | **Menus & HUD** | A game's front end over the forest: a title menu, an inventory and an in-game HUD | Three `.gui` documents (`tools/menus.py`). Rounded cards, pill buttons and slots with soft drop shadows; buttons that ease into their hover colour and grow as the pointer comes onto them, labels centred; a change of screen fades and shrinks the old card away and fades the new one in from a little larger - one `opacity` and one `scale` on the card, inherited by all on it. The HUD's bars ease, its cooldowns drain, its gold counter pops on a pickup and a toast slides in. The arcade's 64 px font for titles. Click, or wait for the demo |
| 23 | **Tilt-Shift Diorama** | A small town from above that reads as a model of one | The new `tilt_shift` post effect: sharp along one street and blurring away above and below it, the band a world height that the mouse moves. Cars round the blocks and through the town on patrol loops, turning into every corner (`face_velocity` with a `turn_rate`, which a patroller honours now), boats up and down the river, chimney smoke, and cloud shadows sliding over the roofs. The town is laid out once (`tools/diorama.py`) and painted and populated from it, so the cars drive the roads that are drawn. T tilt-shift, Up / Down blur |
| 24 | **Collapse** | A stone tower in a courtyard at night, a boulder thrown at it, and the tower coming down | Every block is `breakable`: jolted hard enough it comes apart into pieces cut from its own sprite and normal map, each a body carrying on with its part's velocity, and dust bursts where it broke. Two braziers throw every block's and piece's shadow up the wall - and not onto the blocks beside it (`shadows_casters: false`), which is what lets a side view be lit at all. Space throws, R rebuilds; left alone it throws twice and rebuilds |
| 25 | **Cloth & Rope** | A rope bridge over a gorge late in the day, a traveller crossing it, flags and banners in a gusting wind | Every flag and banner is a `cloth`: a grid of points the engine moves each frame - gusting wind, curl-noise flutter, a travelling wave - and writes into the entity's mesh, folds shaded darker. The bridge is planks on revolute joints and the traveller a character whose `push_mass` weighs it down, so it dips under each step; the handrail, the deck rope and the ties are `rope`s pinned to the planks, so they ride it, and each lantern's rope follows the lantern swinging on its rapier rope joint. W wind (calm / breeze / gale, through `Cloth.set`), Space hurry |
| 26 | **Pixel Perfect II** | Scene 12's cliffside drawn at the art's own resolution | The camera has a `pixel_size`, so the world - sprites, the window's light, bloom - is drawn at 320x180 plus a texel of margin and scaled up nearest, instead of drawn at the screen's resolution and pixelated afterwards: the sails and the spray are whole art pixels and the frame costs what a 320x180 one does. The camera sits on the art's grid and the output pass slides the picture by what that snap took off, so the pan is smooth rather than stepping a pixel at a time (S speeds it up). M goes round the art's resolution, the screen's with `pixelate`, and the screen's with neither |
| 27 | **Two Views** | Scene 23's town seen by four cameras at once | Split screen: the left half is the main camera following one car, the right half a second camera - a camera with a `target` draws into a texture - following another, shown by a UI image. A third camera far above is the minimap - drawing only render layers 1 and 2, so it is a road map with the two players marked, where the street cameras draw the roofs and trees of layer 3 and never the markers; a fourth watches the river bridge, and a billboard by the road shows it, so the split views pass a picture of a picture. Every camera draws the whole world pass, lit and post-processed. M minimap, Space swaps the cars |
| 28 | **Through the Window** | A ruined wall at night with an arched window, and a portal carried by the mouse | One arch-shaped `sprite_mask` used both ways: the wall is masked *outside* it, so the arch is a hole, and a golden afternoon is masked *inside* it - a layer of the world drawn only there, not a picture of one. The portal is a tear in the world - a mask read from a painted texture's alpha, ragged and cracked, which no shape could be - with a nebula inside it, fixed in the world, so moving the portal looks into different parts of it; its rim, drawn from the same silhouette, and its sparks are its children. Up / Down sizes it |
| 29 | **Atlas of the Deep** | A parchment sea chart you zoom from the whole ocean down to one harbour's quays | Every name is world text in the built-in font, a signed distance field now, so a harbour's name at fourteen times its size and the turned island names are as sharp as at their own size, while the chart's own texture coarsens beside them. Names come and go by zoom as an atlas's do: the oceans from afar, the towns close, the quays closer. The chart is generated (`tools/chart.py`) and labelled from the same land. Wheel zooms toward the mouse, arrows pan; left alone it tours |
| 30 | **Spell Storm** | Spellcraft's mage in a storm of two hundred thousand sparks | Every spark is a `gpu_particles` particle - a closed form of its index and the time, computed in the vertex shader - so the storm is three draw calls and three uniforms: a four-armed vortex on the floor (squashed into its perspective by the emitter's scale), a curtain rising off the rune ring, a knot at the staff. They sort with the sprites by z, so the vortex passes behind the mage's feet. Up / Down from 50k to 400k; Space stops time |
| 31 | **The Storyteller** | A storybook on a desk at night, one candle, its fable typed out a letter at a time | Every page is **one** world text with `"rich": true`: a place in blue, a name in bold, a key drawn *in* the line as an icon, reeds that sing in a `[wave]`, a `[shake]`-ing crack, a `[rainbow]` hoard deep enough to read on paper - and a typewriter (`reveal_speed`) that waits at each `[pause]` where a reader would breathe. The ink is lit by the candle like the paper it is on. Space turns the page |
| 32 | **Mapmaker** | An island you paint with the mouse - sand, grass and rock over the sea - its coasts, beaches and cliff edges placing themselves | Every cell of the map is only a **terrain number**; an `autotile` component on the tilemap draws it on a dual grid, a tile per corner chosen from the four cells sharing it, rock over grass over sand. Foam rings the sand and a shadow the grass, all from three rows of sixteen tiles. Wheel / Q E pick a terrain, Space grows the brush, R builds a new island from Lua |
| 33 | **Hillside Hamlet** | An isometric village on a grassy hill: cottages, trees, a well and a pond, villagers walking its lanes behind and in front of the houses | One tilemap with `"orientation": "isometric"`. The ground and the autotiled lanes and pond are plain layers; the hill's blocks and everything standing on the ground are `y_sort` layers, drawn among the villagers and sorted by cell, and the hill's second storey is a layer lifted by `offset`. The mouse picks a diamond with `world_to_tile`; a click plants or clears a tree |
| 34 | **Crater Isle** | A cartoon island in a bright sea, bombed a crater at a time, crates and barrels tumbling into the holes | The island is one sprite with a `carvable`: a landing bomb calls `Sprite.carve`, which cuts the crater from the sprite's own pixels with a scorched rim and rebuilds its collider from what is left, so what stood on the carved-away ground falls. Click drops a bomb, right drag digs, R restores the island |
| 35 | **Neon Signs** | A night-market brick wall: NOODLE BAR in pink neon, a flickering OPEN, a chalk menu, a price card and a SALE! starburst, in the rain | Every word is world text. A neon tube is text brighter than white with an `outline` in the tube's colour and a soft `shadow` at no offset - its glow on the brick - in an HDR scene with bloom; the sticker is cartoon text with a fat outline and a hard shadow; the captions read over the wall with an outline and no strip behind them |
| 36 | **Rolling Hills** | A cart wheel rolling over grassy hills under a blue sky, a floating island above, a pale range behind | The hills, the island and the far range are `spline_shape`s: a handful of points each, a soil fill tiled by the engine, a strip of grass that follows the curve with its blades on top, and a solid collider of the same shape. Left / Right roll the wheel, Space hops |
| 37 | **Harbour at Night** | Boats and crates floating in a moonlit harbour that rises and falls with the tide, a tug's smoke drifting on a breeze | The water is one `area_effect` - buoyancy and drag - that the script raises and lowers, and everything floating rides it; the hulls float level because the lift pushes at the middle of the part under water. A second box is the breeze: the script swings its `force` from side to side, and it pushes the boats and bends the smoke alike. F the fan, W a whirlpool (an `attract` box), C drops a crate |
| 38 | **Hex Strategy Board** | A turn of a hex wargame on a wooden table: grass, sand, forest, hills, water and mountains in hexagons, blue and red pieces among the trees | One hexagonal tilemap - pointy hexes, every odd row pushed. Where a piece can go is a cheapest-first search over `Tilemap.neighbors` with forest and hills costing two, the path to the hovered hex is drawn from it, who it can strike is `Tilemap.distance`, and red's turn marches its pieces with `Tilemap.find_path`. Click a blue piece, then a hex or a red piece; E ends the turn |
| 39 | **Lantern Orchard** | A top-down orchard at night under a lantern you carry: apple trees, a picket fence, a scarecrow, hay bales and a cart, fireflies drifting | Every shadow is the shape of its sprite (`light_occluder.from_alpha`): traced from the pixels, so light falls between a crown's clumps and between the fence's posts, and the scarecrow throws its cross. Mouse carries the lantern; click hangs it |
| 40 | **Moonlit Study** | A study at night seen from above: moonlight through a window lying across the floorboards, a rose window's colours, a desk lamp's pool, a ceiling fan's shadows sweeping the rug | Every pattern of light is a **light cookie** - a texture the light shines through, turning as it turns: the moon drifts and the panes lean, the fan's light turns with its blades. L the lamp, F the fan |
| 41 | **Waterfall Grotto** | A cave in pixel tiles: a waterfall falling through a gap in the roof into a rippling pool, torches flickering on the walls, crystals pulsing and moss swaying on the ledges, daylight from above | Every moving thing is an **animated tile** (`animations` on the tileset, on the map's own clock): the waterfall's strands and the pool's surface move together, while the foam, torches, crystals and moss are `scatter`ed so no two cells flicker in step. Space puts the torches out |
| 42 | **Siege Blast** | Towers of stone and timber on a grassy field under a dusk sky, a cannon on the left, a blast's reach under the mouse | The blocks are physics bodies; the ones a blast would reach glow, found by `Physics.overlap_circle` - which asks the colliders, so a long timber whose middle is out of reach is still caught by its end - and a click throws them outward. The cannon aims at the mouse and `Physics.circle_cast` rings where a ball of its size would first strike; right click fires one |
| 43 | **Night Market** | A lantern-lit market street at night under strings of lights, wider than the screen: a lute player, a grill smoking over its coals, a fountain, a stall of wind chimes; rings drift out from each sound | You walk the figure that carries the `audio_listener`. Every sound is an `audio_source` with `spatial`: the lute, the grill, the water and the chimes swell as you pass and sit to your left or right; a click tosses a coin, heard where it lands (`Audio.play_sound_at`). The showcase's first sounds, synthesised by `tools/gen_sounds.py`. WASD / arrows walk |
| 44 | **Garden Party** | A formal garden at golden hour from above - a walled rose garden, a fountain, clipped topiary, a gazebo and tables on a striped lawn, long shadows - and forty guests mingling | Every guest is a `nav_agent` in one `nav_region` baked from the colliders: each finds its own way round hedges, beds, benches and tables, steps round the others and queues at a gap; a click calls them to a toast and they settle in a loose crowd, G shuts the rose garden's gate and they go the long way. The host's way is `Nav.find_path`, drawn in petals. F4 shows the navmesh |
| 45 | **The Polyglot Inn** | An inn's wall at night, front on: a carved signboard, a chalkboard menu, a pinned notice, a moonlit window, lanterns, tankards on the bar | Every word comes from one string table (`assets/locale/inn.data`, a data table: a row per key, a column per language), in English, Finnish, German, French, Spanish and Polish. The sign, the dishes and the headings carry a `text_key`; the prices, the guest's name and two counts go through `Locale.get` - `3,75 Silber`, and Polish's three plurals as guests arrive and last orders count down. 1-6 or a click on the bar picks the language |
| 46 | **Sea Cove** | A beach, a sea cave and the water under them, side on, late afternoon: surf, gulls, kelp swaying in shafts of light, fish, a swimmer you steer | You hear it: surf, gulls and a harp in the open; swim into the cave and its drips ring, because the cave is an `audio_zone` giving the sfx bus a big reverb; dive and everything goes dull, music too, because the water is one low-passing `master`. E switches the zones off to compare. Every sound synthesised by `tools/gen_sounds.py`. WASD / arrows swim |
| 47 | **Autumn Run** | A fox running and jumping across autumn hills, side on: falling leaves, trees in red and gold, hills and snowy mountains drifting behind | The camera is the subject. `camera_follow` has a **dead zone** (the yellow box: the camera holds while the fox moves inside it, so jumps do not bob the view) and **look-ahead** (it leads the way the fox runs). 1-4 switch follow, dead zone, look-ahead and both through `Camera.set_follow`; the parallax layers and the box are placed from `Camera.get_position`. A / D run, Space jump, B the box |
| 48 | **The Forge** | A blacksmith's forge at night: a roaring hearth, a glowing bar on the anvil cooling from yellow-white through orange to dark red, a hammer, a quenching bucket | Felt as well as seen: every blow throws sparks, flashes, shakes the camera and kicks the pad's strong motor (`Input.rumble`), harder the hotter the iron - or buzzes a phone (`Input.vibrate`); the bellows hum in the weak motor, a quench hisses in a long buzz. Space / pad A strike, B / pad X bellows, Q / pad B quench |
| 49 | **Stage Night** | A band in silhouette on a dark stage: six lamps on a truss throwing soft beams that cross and sweep, haze glowing where they pass, pools of light on the boards, to a synthesised groove | Every beam is a `freeform_light` - a trapezoid of light, narrow at the lamp, soft at its edges - swung by turning its entity, its colour and brightness set on the beat with `Lighting.set_color` / `set_intensity`. 1 warm, 2 cool, 3 rainbow, 4 chase, Space strobe |
| 50 | **Jellyfish Bloom** | Deep water at night: five glowing jellyfish rising in pulses, tentacles and frilled arms trailing, kelp swaying on the floor, lit only by the jellies and a little moonlight through the snow | Every tentacle, oral arm and kelp stalk is a chain of `jiggle` bones: the script moves each jellyfish and turns each stalk's foot, and the trailing, streaming and whipping are the engine's. Mouse leads a jellyfish, Space startle, C current |
| 51 | **Night Bloom** | A long meadow under the moon: glowing flowers open as the camera drifts up to them and close behind it, lighting the grass, fireflies over them | Every flower is a `visibility_notifier` with a script: `on_screen_entered` opens it, `on_screen_exited` closes it, and its fireflies are held still off screen (`pause_offscreen`); a small negative margin puts it all just inside the frame. Left / Right walk, Space drift |
| 52 | **Model Railway** | A model railway board from above, tilt-shifted: a little steam train with four carriages round a winding track, cars round a road both ways, a ferry over the pond | Everything that moves rides a drawn `vector_path` with `path_follow`: the train is one path and five riders at fixed distances, the cars keep left (`lateral`) with the far lane at a negative speed, and the ferry goes `once` and turns round in `on_path_end`. Up / Down train speed, Space stop, T tilt-shift |
| 53 | **Village Square** | A cobbled square at dusk, lamps lit and windows glowing: sixteen villagers stroll round the well and along the front, legs and arms swinging, passing in front of and behind each other, the lamps and the trees | Every villager is up to eight cut-out parts under one `sorting_group` with `y_sort`, so each sorts among the others and the y-sorted props as one, its own front arm still in front of its body; they walk with `path_follow`. Space pause |
| 54 | **Tesla Lab** | A dark laboratory lit by what it is doing: lightning crackling between two coils, a red laser bouncing between four mirrors, cables and hazard tape on the floor | Every line is a `line`: the bolts are jagged paths rebuilt each flicker, tapered by a `width_curve`, additive and emissive; the laser is traced through the mirrors by reflection in the script and handed over with `Line.set` every frame; the tape is a stripe `tile`d along its line. Space lightning, Left / Right turn the mirrors |
| 55 | **Dragon's Hoard** | A dark vault and a lantern you carry over a mound of gold: coins, a sword, a goblet, gems and two shields glint and the glints follow the light, a torch flickering on the wall | Every piece has a normal map made from one height field and a `specular` material; the normal map's alpha marks the metal, so the shields' painted faces, the sword's grip and the stone barely shine. HDR, so a glint blooms. Mouse carries the lantern |
| 56 | **Haunted Hall** | A candle-lit hall of portraits, and ghosts drifting through it, waving, fading in and out of being | Each ghost is a figure of parts - robe, head, an arm before and an arm behind, a glow - and a `canvas_group`, drawn as one picture and faded with `CanvasGroup.set`: half-there, it is a pale whole figure, never a stack of see-through parts. Space makes them all appear |
| 57 | **Picture House** | A small cinema in the dark: a film of a boat at sea at sunset on the screen, the projector's beam through dusty air, an audience in silhouette, the projectionist's monitor showing the same film | The film is a `video_player` playing an mp4 into a texture the screen's sprite names - the monitor names the same one. The film is made by `tools/gen_videos.py` and encoded with ffmpeg. Space pauses (`Video.set`), R starts it again |
| 58 | **Lamplit Mine** | A mine in cross-section by lantern light: lanterns on chains, a crystal vein in the far wall letting a buried cyan light out as a beam across the mine, a plank scaffold over a valley, boulders, the lamp you carry | All one tilemap. The tileset's `shapes` give slopes, rounded roof corners and thin planks their collision outlines and let the crystal cast no shadow; a `tilemap_occluder` makes the rock cast shadows, merged into a few long outlines. Click breaks or mends rock - the shadows and the collision follow. B rolls the boulders down the slopes |
| 59 | **Windswept Meadow** | A summer field on rolling hills: seven hundred blades of grass and flowers - daisies, poppies, cornflowers - bending as gusts roll across the field, a banner rippling on a hill, clouds, seeds on the wind | Every plant is a sprite drawn through one graph (`meadow_sway`) whose `Vertex Offset` leans its top: a breeze whose phase runs along the field plus gusts travelling through the world, so the grass bends in waves; the banner is a static mesh grid rippled by its own graph (`banner_wave`). No script moves anything - Up / Down only change the `wind` material parameter |
| 60 | **Moonlit Duel** | Two duellists on a cliff under a great moon, circling and clashing in sparks, lightning flashing the clouds | Light layers: the duellists are on layer 2 (`light_layers`) and every light names its layers (`light_mask`) - a cyan and a magenta key light that touch only them, a moon and lightning that touch only the world, so they stay black against the flash. L turns the layers off (`Lighting.set_mask`): the key lights wash the sky |
| 61 | **Firefly Glade** | A pine glade at dusk over a pond, fireflies drifting and looping through it, a sparkler on the mouse, shooting stars | Particle `trail`s: every firefly draws its loops and curls in light behind it as it drifts on a `turbulence` flow; the sparkler's sparks streak down in short tapering trails; a shooting star is one particle with a long one. The grass sways on Windswept Meadow's graph. T takes the trails away |
| 62 | **Phoenix** | A firebird of flame over a night sky, wings spread, rising and falling inside a ring of sparks | Emitters shaped by the entity's own sprite and path: flame pours off the bird's outline (`emit_shape` `sprite` with `edge`) and embers off its body, each from an invisible copy of its sprite on a child, so the fire keeps to its shape as it flaps; the ring throws sparks from every point of its `vector_path` (`emit_shape` `path`) on trails. Space: a great wingbeat |
| 63 | **Turning Seasons** | Windswept Meadow's field through the year - summer, golden autumn, a pale cold winter, a violet dusk - the season turning every few seconds | Not a sprite changes: four colour look-up strips (`color_lut`, made from the neutral strip by `ts63_luts`) grade the whole frame in the output pass, one fading out and the next in through `PostProcess.set_lut`; G lays `film_grain` over it. The strips sit small in the corner, which is what loads them |
| 64 | **Storming the Gate** | A catapult lobbing stones over a moat at a castle wall at dusk - knights with spears and banners, a rope bridge hung between two posts | Each knight is a skeleton with a `ragdoll`: when a stone reaches one the script sets it limp (`Ragdoll.set`) and it falls as capsules on hinges, landing on the wall or slumping against it. The bridge's planks hang on hinges with a `break_force`, and a stone through it snaps them by physics alone (`on_joint_break` counts them). Siege Blast's sky, hills, stone and ball; the knights' parts by `sg64_knights`. It starts over once the gate has fallen |
| 65 | **Two Views of the Railway** | Model Railway's board split down the middle - one half riding with the train, the other with a car on the road, each turned so it points up its half - and the whole board in a small map between them | Three cameras, each given its part of the screen by `screen_rect` and moved by the script; 2 re-splits them top and bottom with `Camera.set_screen_rect`. The main camera looks at nothing, so the gap between the halves is a clean line, and a table under the board fills what a turned view sees past its edge |
| 66 | **Puppet Theatre** | A toy theatre's play in a gilt proscenium: the velvet curtains part, a card knight walks on under a rising sun, night falls on a painted castle, a dragon enters, thunder, the end, confetti, curtains | The whole play is one `timeline` on the stage manager - curtains, the actors' walk and bob, the sun and moon, the night backdrop's fade, the house and spot lights, the captions - its tracks over five kinds of component, and its event keys call the script for the thunder and the applause. It loops |
| 67 | **Signs of the Silk Road** | A caravanserai at dusk under domes and minarets: wooden shop signs swinging from a beam - 茶屋, Чайхана, 丝绸之路, Ταβέρνα, 시장, 香料 - paper lanterns between them, more words painted over the doorways | Every word in another script is drawn straight from a `.ttf` its `font_id` names (Noto, subset to these words, `assets/fonts/noto/`), its glyphs baked into a distance field the first time it is drawn (`renderer::dynamic_font`); the English under each is the built-in face. The swing is a looping timeline |
| 68 | **Moon Lander** | A lunar module coming down over the grey craters with the Earth in a black sky, its flame swelling and shrinking, settling on the lit pad - then off from a new start, again and again, each landing logged | The autopilot is `fixed_update`: once before every physics step with the step's dt, reading where the module is and how fast (rapier's velocity) after the last step, and pushing with `apply_force` for that step - so it flies the same at any frame rate. It holds its height until it is over the pad, then comes down at a speed that shrinks with the height left. Moon gravity is `Physics.set_gravity(0, -1.62)` |
| 69 | **The Kraken** | A ship rolling in a storm, lightning over it, and four tentacles rising out of the sea to coil about its masts - Space and they let go and wave above the water, Space again and they take hold | Each tentacle is eleven bones with an `ik_chain` on the last: FABRIK bends the whole limb so its tip reaches the point the script sweeps about a mast (turning with the ship), and `max_bend` 0.45 from a slight resting curl keeps every joint from folding sharper than a curl. The ship rocks and the lightning flashes on a looping `timeline`, whose event key shakes the camera |
| 70 | **Eight Ways** | A top-down ranger crossing a forest clearing, aiming its bow at one hay target after another while it strolls, runs and stops in every direction - its feet stepping forwards, backwards, sideways and on every diagonal, whatever way it faces; an inset draws the blend space and where it is in it | The ranger's `skeletal_player` has an empty `clip` and a `blend_space` of nine clips (idle, and a walk and a run each way). Its body turns to aim; its velocity, turned into the body's frame and scaled so the walks sit at 1 and the runs at 2, is the blend position (`Animation.set_blend_position`) - so the gait follows the travel relative to the aim, and every clip steps at one phase |
| 71 | **The Quest Journal** | A journal open on a desk by candlelight: a quest log with names in colour, an icon among the words of every line, an urgent entry that shakes, a reward that shimmers and a new one whose tag bobs; a purse and a day count along the foot of the page; and on the facing page a letter that writes itself out, pausing between sentences, then gives way to the next | The log is entity-UI `ui_text` with `rich` on, the same markup world text reads; the footer is a `.gui` Text node whose markup wraps `{{variables}}`, so the gold count is coloured by the markup round it; the letter is a `ui_text` with `reveal_speed`, and `UI.set_text` with the next letter starts it over |
| 72 | **Scrap Yard** | A gantry crane runs along its beam at dusk and drops junk - a bicycle, a kettle, a gear, a tyre, a chair, an anvil, a desk lamp, a bent pipe - onto a heap that settles by the junk's real shapes: a wheel hooks over a gear, a chair tips onto its back, a tyre rolls down the slope of the pile | Every piece is a prefab with a `sprite_collider`: the outline of its sprite's opaque pixels, traced once its texture loads, is written into its `collider` as a convex decomposition, so the body never falls as the placeholder box it was authored with. F4 draws the traced outlines |
| 73 | **Spirograph** | Five wire figures - hypotrochoids, a gear rolling inside a ring with a pen through it - turning slowly on dark drawing paper in 2-pixel glowing lines, and a pen tracing the largest with a trail; every curve smooth at every angle | Every edge here is thin and turning, which is what stair-steps: the scene's `fxaa` post effect, drawn in the output pass on the tonemapped colour, blends the stairs while straight runs and the UI are untouched. F switches it to compare. MSAA is not needed for lines or meshes: FXAA sees every edge the display shows |
| 74 | **Baggage Claim** | An airport baggage hall at night, a plane parked outside the window: suitcases, a duffel, a guitar case and boxes come through the rubber flaps of a hatch, ride a sloping belt down onto the long belt and tumble off its far end onto a heap | Each belt is a static collider with a `conveyor`: rapier is told the contact's surface slides (its tangent velocity), so friction carries the bags while the belt never moves - and a bag on the slope rides it down rather than sliding. The slats are sprites with `velocity` and `wrap_bounds`, for the eye. The bags are prefabs with `sprite_collider` |
| 75 | **The Controls Room** | An arcade game's settings screen under a CRT: every action and its keys and pad buttons, a listening prompt that blinks while it waits for the next press, and beside it a playfield where a ship flies by those very bindings - and after a restart, the bindings are the ones you chose | `Input.get_just_pressed()` names what went down this frame the way an action binds it (`K`, `Pad0_A`, `Pad0_LeftStickX+`); `Input.set_bindings` takes it, and `Save.set` + `Save.commit` keep it, read back in `init`. The script keeps its own table of the bindings it set, because `set_bindings` is deferred and `get_bindings` answers the old keys until the frame ends. The menu skips the scene navigation while it listens, so H and Escape can be bound too |
| 76 | **Monster Zoo** | Four pens of round beasts strolling, bobbing and floating behind wooden fences - moss green ones, horned orange ones trailing embers, giant slow ones, winged blue ones - with newborns dropping in on Space | Every beast is **one prefab and its variants**: `MzEmber` is a `.prefab` whose own components carry a `prefab_instance` naming `MzBeast`, overriding its texture, size and colour and adding an ember emitter; `MzEmberGiant` extends `MzEmber` (scale, pace); `MzSky` extends the base (wings, colour, a floatier habit). The pens' beasts are scene instances of those with their own place and phase, and Space spawns each by name |
| 77 | **The Heist** | A museum at night on its security monitors: each room's camera tracks a thief down the carpets, round an L-shaped gallery's inside corner and an octagonal rotunda's diagonal walls, never looking past its walls; as the thief crosses a doorway the next room's camera takes the screen and the view eases across, zoom and all. C clears the confines (the garden shows past the walls), B makes every hand-over a cut | **Camera priorities, blends and confines** - four cameras `confine`d to their rooms' outlines; the script raises the `priority` of the room the thief is in with `Camera.set`, and the camera taking over `blend`s from the last. A CRT, film grain and a green grade over it |
| 78 | **Echo Canyon** | Dusk in a desert canyon: a hiker walks from camp into the canyon and calls out, and the walls answer - rings and fading words at each echo; the camp radio plays an old swing tune as a crackling AM set; no two footsteps sound alike. Space calls, E switches the echo, R the radio's set, V the steps' variation | **Bus echo, distortion, compressor and EQ, and clips with a variation** - the canyon is an `audio_zone` lending the sfx bus an `echo`, the radio's music bus is EQ'd, distorted and compressed by `Audio.set_bus_effect`, and the footstep clip has `Audio.set_variation`. Its sounds are synthesised by `tools/gen_sounds.py` |
| 79 | **Paper Dolls** | A paper doll on a toy-theatre stage changing clothes - summer, rain, winter, party - every piece a card cut-out with a white edge and a shadow, swaying as she waits, a burst of paper bits at each change; the outfit's tag lights on the rack. O changes the outfit, 1-3 a hat, a coat or boots alone, so outfits mix | **Sprite swap** - her root carries a `sprite_library` of four outfits (hat, coat, sleeve, boot), each clothing part a `sprite_resolver`; `Sprite.set_label(root, "winter")` dresses all six parts at once, `Sprite.set_label(root, label, "hat")` one category. The parts keep their place in the rig |
| 80 | **Treasure Isle** | A treasure island from above - sea that shimmers, sand with shells, grass in five looks, a lava field with crusts and bubbles, palms and chests - and an explorer walking his loop, smoking and glowing red where the lava burns him, opening each chest he passes | **Tile data** - the tileset draws grass, sand and lava as weighted `random` variants picked by each cell's place; its tiles carry `data` the explorer's script reads (`Tilemap.get_tile_data`: what is underfoot, whether it `burns`); and its X and sprout tiles are `spawns` that load as the `IsleChest` and `IslePalm` prefabs |
| 81 | **Sandstorm** | A desert at dusk in a storm: two hundred thousand grains of sand streaming over the dunes, parting round three rocks and packing against their windward faces, and a whirlwind wandering the dunes that draws the stream up and spins it. C lets the sand through the rocks, W stills the whirlwind | **GPU particles meeting the world** - one `gpu_particles` cloud, still stateless: `collide` puts a grain found inside a nearby collider out on its nearest side, and an attractor that `follow`s the twister pulls and spins the cloud as it ages, both closed forms in the vertex shader |
| 82 | **Stained Light** | A chapel side on: a low sun through four panes of coloured glass in the west wall throws red, blue, gold and green across the stone floor - and the light bounces, so the pillar standing in the patches glows with their colours and the dark wall between the beams takes a coloured wash. The sun drifts, and the glow follows the patches. G switches the bounce | **2D global illumination** - one `global_illumination` component: radiance cascades on the GPU let the occluders' lit outlines shine again into the light buffer before the composite. Neither Godot nor Unity has it built in for 2D |
| 83 | **Siege Workshop** | A castle's siege yard: a catapult, a ballista and a royal catapult on show, working - arms wind back and throw, the ballista's head tilts and looses bolts - while a parade of engines and supply wagons rolls out of the gate, wheels turning with the distance, pennants and banners swinging. Every engine is one `spawn_prefab`: up to nine entities each. Space marches them faster | **Prefabs with parts** - a `.prefab` is a hierarchy (`children`), a part can be a prefab (the wheels are), a variant adds parts to the ones it inherits, a scene can override one part by name, and `Entity.find_child` finds one. Godot's nested scenes, Unity's prefab hierarchies |
| 84 | **Options Menu** | A game's options screen over a lighthouse at dusk, its beam sweeping and the sea glinting: window mode, resolution, vsync, a frame cap, the cursor (system shapes, a quill drawn for the game, or none) and keeping it in the window. Every row is live and read back from the window at the foot of the panel; a row the runtime cannot do says NOT HERE; the choices come back on the next run | **Display and cursor settings** - `Display.set_mode` / `set_size` / `set_vsync` / `set_max_fps` / `get_resolutions` / `supports` and `Cursor.set_visible` / `set_confined` / `set_icon` / `set_image`, process-wide and applied by each runtime once a frame (Godot's `DisplayServer`, Unity's `Screen` and `Cursor`) |
| 85 | **Guest Book** | The guest book of an inn, open on a desk by candlelight: who has stayed on the left page, and on the right three lines to write on in ink - a caret where you click, selection by shift, drag or double click, ctrl+arrows by word, the system clipboard, a limit per line, an IME composing in place. Enter moves down the form and signs; the new entry inks in on the left and is kept for the next run. The cursor is a quill | **A real text field** - `ui_text_input` and `.gui` `TextInput` share one editing model (`ui::text_edit`): caret, selection, words, clipboard, Enter / Escape, `max_length`, `password`, IME composition, a phone's keyboard through a hidden textarea in a browser; the field owns the keyboard while focused |
| 86 | **The Long Corridor** | A traveller walks an endless castle corridor, and only three of its halls exist at a time: each hall is a scene of its own - its own wall painting with a window onto its own sky, banners, and torches whose flicker is a script in the hall - streamed in ahead (its painting decoded off the main thread) and dropped behind. The line at the foot shows each hall loading and loaded. Space hurries | **Async and additive scene loading** - `Scene.load_additive(path, x, y)` lays a scene into the running one as a chunk once its assets are in, `Scene.unload(chunk)`, `Scene.preload(path)`, `Scene.status` / `Scene.progress` (Unity's `LoadSceneAsync` + additive mode, Godot's threaded loading) |
| 87 | **Button Prompts** | A dojo's first lesson at dusk, its card naming the buttons in the player's hand: Space, F and D on a keyboard, A / X / RB on an Xbox pad, Cross / Square / R1 on a PlayStation one, B / Y / R on a Switch - swapped the moment another device is touched - and a strip showing Jump in every family. Three moves: a tap to jump, a hold that charges through three tiers and strikes the dummy on release, a double tap to dash. Each icon is a `sprite_resolver` under one `sprite_library` of glyphs | **Controller identity and input interactions** - `Input.get_gamepad_name` / `get_gamepad_type`, `get_last_device`, `Input.get_prompt(action [, style])` with a glyph id; `Input.is_action_held(a, s)`, `is_action_tapped`, `is_action_double_tapped`, `get_hold_time` (Unity's Hold / Tap / MultiTap interactions, Steam Input's glyphs) |
| 88 | **Night Drive** | A synthwave coast road at night - a striped sun, mountains and a lit skyline scrolling at their own depths, palms, a neon road - and a score that follows the car: four stems (pad, drums, bass, lead) started together and faded in one by one as it speeds up, an engine note that climbs through four gears and drops at each shift, and a radio that crossfades station to station and back to the score | **Music crossfades, playlists and layered music; a sound's pitch** - `Audio.crossfade_music`, `play_music_layers` / `set_music_layer`, `set_playlist` / `next_track`, `set_music_pitch`, `seek_music`, `Audio.play_sound(..., loop)` and `set_sound_pitch` by the id it returned (Unity's AudioMixer snapshots and FMOD-style stems, Godot's `AudioStreamInteractive` / `AudioStreamSynchronized`) |
| 89 | **Archer's Range** | A practice range at golden hour: an archer walks along it while straw targets come by down the range, and shoots each one **without breaking stride** - the legs are the base clip, the bow is a layer masked to the upper body that draws to full draw and looses over the walk, then fades back into the walk's arm swing; a breath is an additive layer on the torso, stronger when she stops. Space stops and walks on (the base crossfades, the layers play on); F shoots standing | **Animation layers with bone masks** - a `skeletal_player`'s `layers`, each a clip within a `mask` (a bone and everything below it), `override` by weight or `additive`; `Animation.play_layer` / `stop_layer` / `set_layer_weight`, `on_layer_end` (Unity's Animator layers and avatar masks, Godot's blend-tree filters) |
| 90 | **Fireworks Night** | Fireworks over a harbour town: rockets climb on trails and burst into a starburst that **blooms and thins over its 32 frames**, glitter that twinkles through a four-frame star, each from a frame of its own, and smoke that billows over sixteen frames and drifts; the sky and the harbour flash with each burst. Space fires a volley | **Particle flipbooks** - `flipbook` on a `particle_emitter`: the texture cut into `columns` x `rows` frames, played over each particle's life (`cycles`), at `fps`, or one `random` frame held; `random_start`, `frames` (Unity's Texture Sheet Animation, Godot's particle animation) |
| 91 | **Procedural Isles** | An archipelago drawn from noise and redrawn on R: cellular noise puts an island in each of its cells, simplex fbm roughens their coasts and the Mapmaker's autotiled terrains draw sand, grass and rock from the height; clouds ride the wind and thicken and melt where 3D Perlin (time as z) says, each with its shadow on the sea, and streaks of wind follow a Perlin flow field. Space is a gale | **Noise for scripts** - `Noise.perlin` / `perlin3` / `simplex` / `cellular` / `fractal` (fbm, ridged, turbulence) / `grid`, seeded, the same Rust in every runtime (Godot's FastNoiseLite, Unity's `Mathf.PerlinNoise`) |
| 92 | **Crane Yard** | A crane truck on a quay at dusk, every moving part a joint: wheels on **wheel joints** that bob as it drives over the bollards, a trolley that slides along the boom, a hook on a rope whose swing a **friction joint** damps and whose winch lets it down, a magnet that holds a crate under the hook with a **motor joint** so a heavy one sags and sways, and crates the mouse drags on **target joints** | **More joints** - `Wheel` (a sprung, driven axle), `Target` (pull toward a point or a named entity), `Motor` (an offset held with limited strength) and `Friction` (resistance up to a force) on `joint` (Unity's Wheel, Target, Relative and Friction joints) |
| 93 | **Ghost Lantern** | A haunted gallery by candlelight: ghosts in parts - body, arms, eyes - each faded as one picture and drawn through a `ghost_glow` material, so a green glow runs round the whole ghost (not round each limb where they overlap) and the picture ripples; a cursed lantern drifts after the mouse casting darkness that eats the candles' light, black at its heart, and the ghosts lean toward it. The candles flicker on `Noise`. Space lifts the curse | **A material on a canvas group** (an effect on a whole group) and **subtractive lights** (`subtract` on a point or spot light; Godot's Light2D subtract blend) |
| 94 | **Warehouse Robots** | Delivery robots shuttle parcels from the aisles to the dock, each route drawn as it goes: the racks are a **tilemap** the nav region bakes as walls, cross-aisles and all, and R swings a gate in the middle row open or shut (`set_tile`) - every robot replans; crates are **nav obstacles** you drag into an aisle with the mouse (a target joint), and the robots go round them | **Navigation that sees tilemap walls and moving obstacles** - solid tile layers in the bake, `nav_obstacle` for dynamic bodies (Unity's NavMeshObstacle carving) |
| 95 | **Equaliser** | A club driven by its own music (the Night Drive stems): the LED wall is the mix's **spectrum**, sixteen bands of ten cells climbing green to red; three lights follow the lows, mids and highs; the dance floor flashes and the crowd changes moves on each beat (the bass jumping above its recent average); A..K play a saw lead on the **synth**, which the wall shows like anything else | **Audio analysis and synthesis** - `Audio.get_spectrum`, `Audio.get_level`, `Audio.synth` (waveforms under an ADSR envelope) |
| 96 | **Photo Mode** | A scenic overlook at sunset - clouds drifting, birds crossing, the lake glinting. P enters photo mode: the world freezes (`Time.set_scale(0)`), a viewfinder appears, the arrows frame the shot and - / = zoom on the wall clock; Space hides the viewfinder and the gallery for one frame and takes the **screenshot** - into a texture the polaroid gallery shows, and natively a PNG | **Screenshots** - `Screenshot.take({ texture, file, scale })` and `Screenshot.ready` |
| 97 | **Bazaar Signs** | A market street at dusk whose shop signs swing on their chains in Arabic, Hebrew, Hindi and Thai - Arabic joined and dotted and right to left, a price with its digits in order, Hebrew right to left, Devanagari's conjuncts and vowel signs, Thai's stacked marks - on painted boards and neon; a banner over the street says hello in each script in turn | **Text shaping and bidirectional text** (`rustybuzz`, `unicode-bidi`), from Noto subsets that keep their layout tables |
| 98 | **Readable Menus** | A title screen at dawn - a castle on the far ridge, a lantern-lit road up to it - with a main menu and a settings card that a screen reader reads and works: every button, slider and toggle named, decoration skipped, presses announced; a pill at the foot prints what the reader is saying, and "Large text" grows both cards | **Accessibility** - the UI tree handed to the platform's screen reader through AccessKit (a hidden ARIA mirror in a browser); `accessible` names, `Accessibility.announce` / `speech` |
| 99 | **Night Watch** | A moonlit courtyard: three guards carry lanterns on their rounds while a thief works the chests in the torchlit corners - guards chase what their lantern shows and go to look where a chest was robbed, the thief flees any guard who comes near; each actor's current leaf floats over its head | **Behaviour trees** (`behavior_tree`): reactive selectors, sequences, `random` idles, `move_to` steering `nav_agent`s round the walls, a `call` handshake with the script; no actor is scripted |
| 100 | **Ember Cavern** | A cave in cross-section over a lava river: embers rise off it, bubbles burst into sprays of sparks, blue spores drift out of the dark - and the normal-mapped rock is lit only by them, warm patches rising and fading with the embers, a burst lighting the ledge round it, a cold pool round the spores | **Particle lights** (`light` on a `particle_emitter`): a shadowless point light per particle, in its colour, faded with its alpha and sized with it; L dims them all |
| 101 | **The Lit House** | A cottage at night from above: a lamp blinks inside like a failing bulb, and its warm light gets out only through the windows and the open door, lying across the moonlit grass in long wedges. A walker with a torch does the rounds and goes in - the roof lifts away and everything outside the house goes out of focus - then comes out again | **Walls as `light_occluder`s** with gaps for the windows and door; **light layers** keeping the lamp off the roof and the moon out of the room; a `spot_light` turned by `face_velocity`; the walker's rounds a `behavior_tree`; and `blur` with a **focus rectangle** (new) |
| 102 | **Firefight** | A night-time lot, two squads across it: raiders firing orange tracer bursts, security answering with cyan laser beams. Rounds spark off the cover, casings tumble, a burnt car and two barrels burn and smoke under drifting haze and a sweeping searchlight; a red barrel in the middle goes up now and then (B sets it off) with a fireball, a shockwave, debris and a shake - and every hit leaves its mark, blood splats and drips and burns **painted into the asphalt**, so the lot keeps the whole fight | **`paintable` + `Sprite.paint`** (new): stains written into the floor's own texture; tracers as a pool of sprites with `trail`s and lights; beams as `line`s in an HDR frame; sparks and debris with particle **collision** and **particle lights**; cover as occluders and colliders, read by `Physics.raycast`; soldiers on `nav_agent`s; the `shockwave` and `heat_haze` graph materials; film grain and a chromatic pulse |
| 103 | **Snowfield** | A snowfield under a low moon: a wolf pack trots a loop, a dog sled runs another and a walker with a lantern goes from a cabin to a frozen pond and back - and every one **presses its prints into the snow**, so the moon and the lantern catch each rim; the snow sparkles round the lantern; flakes fall and blow, smoke leaves the chimney | **`Sprite.press`** (new): dents in the snow's normal map; `Sprite.paint` (multiply) for each print's shadow; specular through the normal map's alpha as a sparkle mask; `path_follow` for every walker; **unlit particles** (new - `unlit` on an emitter did nothing before) for the snow; drop shadows as multiply sprites |
| 104 | **Wildfire** | Lightning starts a fire in a dry meadow at dusk: the wind drives it east (W turns it), a ploughed firebreak turns it, the river stops it, oaks catch and burn black, smoke rolls downwind, and a water bomber makes passes over the front, dousing it in steam - the land behind left as char, then ash; when it is out, it starts again | The fire is a grid in the script; everything it does is **painted**: each burning cell into three transparent canvases (**paint canvases**, new - `over` composites alpha, `erase` rubs out), and the flames, embers (with particle lights) and smoke start from them through a `sprite` **emit shape that follows the paint** (new); char and ash painted into the meadow; fire lights hung over the front |
| 105 | **Graffiti Alley** | A brick wall in a rainy alley at night, straight on: a street lamp, a stuttering neon sign, steam from a grate and now and then a car's headlights sweeping past; two taggers walk in and spray bubble letters along the lower wall - outline, fill, highlights, then drips that run - and the fresh paint shines wet under the lights and dries matt; when the wall is full a buffer rolls grey over it and they start again | **Wet paint** (new): every stroke is a solid stamp, a speckled `spray` overspray (new) and a `gloss` stamp that wets the wall's normal map, drying back over `dry_seconds`; the letters a stroke font in the script; old pieces painted in over the first frames; lit rain, a specular puddled floor |
| 106 | **The Foundry** | A foundry side on: a crucible on an overhead crane travels to three sand moulds and tips molten metal into each - it pours in a stream, splashes, throws sparks that bounce, runs into the cavity and levels out, then cools from white through yellow and orange to the dull grey of a casting, lighting the shop as it goes; steam and shimmer rise off the moulds, a furnace roars at the back, and when the castings are cold they are knocked out and it starts again | **Liquids** (new): the metal is a `fluid` - particles under double density relaxation, viscosity, colliders, cooling and setting - drawn as one surface by a **`canvas_group` threshold** (new); fluid lights; spark collisions and particle lights; the `heat_haze` material |
| 107 | **Night Pursuit** | A getaway car tears down a wet city street at night, weaving through traffic with two police cruisers on its tail, their lights washing the road red and blue, and a helicopter's searchlight and rotor shadow on it; the camera rides with the car, so the street, the lamps and the oncoming traffic stream past in motion blur while the car stays sharp; M compares | **Motion blur** (new): a velocity pass of each sprite's motion on screen and a gathering post effect; streets streamed past on plain `velocity` components; spot-light headlights, flashing police lights, a wet specular road |
| 108 | **Orbital Station** | A shielded station over a planet under attack: raiders make passes and fire bolts at it, and every bolt ripples the shield where it lands; defenders chase them with lasers and missiles that leave smoke, the station's turrets fire through the bubble, raiders go up in embers, smoke and a shockwave, rocks tumble by - and the low sun, lighting the planet's rim and the station from one side, throws lens flares across the frame, as every explosion does; L compares | **Lens flares** (new): a post effect of ghosts, halo, fringes and an anamorphic streak from whatever is past its threshold; hit ripples as rings and shockwaves clipped to the bubble by a `sprite_mask`; normal-mapped planet, station and rocks under one low sun; `line` lasers, particle smoke, trails |
| 109 | **Mirror Maze** | A carnival hall of mirrors from above: a small chequered room walled by four mirrors, so the room repeats outward on every side and the corners fill with reflections of reflections, each darker than the last; a laser turns in the middle and bounces round the walls - in every copy - marquee bulbs chase round the gilt frames, visitors wander, and one now and then steps into a portal in the floor and turns up in the sunny garden it looks down on; M compares | **Mirrors** (new): a `mirror` draws what is in front of its line into a texture of its own and shows it flipped behind the line, in other mirrors' views too, so mirrors see mirrors; `Mirror.set`; a camera into a texture clipped round by a `sprite_mask` for the portal |
| 110 | **Harbour Storm** | A harbour in a storm at night, side on: a swell rolls under the moored boats, which climb the crests, drop into the troughs and heel on the slopes, straining at their mooring ropes; rain chops the sea, gusts throw the pier's crates in and they splash, lightning strikes the water and flashes the clouds, the swell breaks in spray on the headland, and the lighthouse sweeps its beam through the rain, lighting the sea where it lands; S calms the storm | **A water surface that answers** (new): `water_surface` springs that what falls in splashes, with a swell and chop, floating what is in it on its own moving surface; `Water.set` / `Water.splash`; rope joints drawn by `rope`s, a beam that is a widening `line` with a spot light |
| 111 | **Arena Boss** | A stone golem against a knight in a ruined arena at night: the golem sways on a base clip with a breath layer, flinches on a layer when the knight's spear lands and roars on another before calling down rocks; its fist reaches by IK to slam and sweep, and every attack is told first by a shape on the floor that fills - a disc for a slam, a bar for a sweep - before the floor is carved where it lands; its health is a bar that empties, and when it does the golem goes limp into its own craters | **Sprite fills** (new): `bar` and `radial` fills and `Sprite.set_fill`; clip layers, an `ik_chain`, a `ragdoll` and a `carvable` floor together on one fight |
| 112 | **Haunted Manor** | A torch-lit walk through a manor, side on, streamed in room by room - the entrance hall, the library, the ballroom, the chapel - each its own scene with its own wall, candles and ghosts; the grade turns as the walker crosses from one room into the next (warm, green, cold, violet), the music crossfades to each room's own loop, and the ghosts darken the light where they drift; Space hurries | **Post volumes** (new): a `post_volume` in each streamed room, blended into the screen's chain by where the camera is; `Scene.load_additive` rooms, `Audio.crossfade_music`, subtractive lights carried by ghosts |
| 113 | **Falling Sands** | A cave in cross-section, every grain its own: sand pours from a crack in the ceiling into dunes, water from another fills a pool under a wooden bridge and runs on, a spark lights the oil on a shelf and it burns to smoke, a lava bomb sets the bridge alight and boils the water it lands in, and crates fall onto the dunes and the bridge and ride what is under them; the cave is set again after a while | **Pixel simulation** (new): one `pixel_sim` seeded from a picture of the cave, solid to bodies through its `carvable`, its fire and lava glowing; `PixelSim.paint` for the spark and the lava |
| 114 | **Smoke Chamber** | A vaulted chamber full of air that moves: a brazier's smoke and a pipe's steam rise, curl round each other and pool under the vault, an iron weight swings through them on its chain and carves a wake, and a fan on the far wall now and then blows the layer across the room; lamps on the pillars and a cold window beam light the smoke, the pillars' shadows cut through it | **Smoke** (new): one `smoke` grid for the whole room, its sources the brazier, the pipe and the fan, the pillars and the weight colliders it flows round and is pushed by; `Smoke.set` turns the fan and the steam, `Smoke.puff` |
| 115 | **Rewind Ruins** | A hilltop temple at dusk and a pilgrim walking past; a boulder comes down and brings it down - columns topple, the pediment tumbles, fire catches the rubble - then time runs backwards in a cold, faded grade: the blocks fly back up into place, the fire goes out, the boulder rolls back up the hill and the pilgrim walks backwards, her steps in reverse; held a moment, the world goes on again and the boulder comes another way | **Rewind** (new): every moving thing `rewindable`, one `rewind` clock recording, rewinding and holding them; `Rewind.set`, and post-process changes for the look |
| 116 | **Muffled Halls** | A castle in cross-section - a moonlit courtyard with a fountain, a guard room with its hearth, a great hall where a minstrel plays - and a knight walking through them: the lute rings out in the hall, goes dull and quiet behind one wall and faint behind two, and comes back as a door swings open between; the hall's reverb is long, the guard room's short, the courtyard's none. Rings drift out from each sound | **Walls and rooms** (new): `occlusion` and `muffle` on each spatial `audio_source`, `room_reverb` on the knight's `audio_listener` - walls counted and the room measured from the colliders, with nothing authored per room |
| 117 | **Moor at Midnight** | A moor at midnight under a low ground fog: a traveller crosses it with a lantern whose light lies in the mist past its own reach, the standing stones' shadows cut through it as shafts that swing as she walks; a crofter's window glows in the fog at the far side, wisps drift green over the heather; F thins, thickens or clears it | **Fog** (new): one `fog` - billowing, drifting, thickest on the ground - applied in the lighting composite, catching the scene's own light; stones whose shadows are traced from their pixels |
| 118 | **The Endless Dungeon** | A dungeon not drawn but decided: corridors settle cell by cell a few seconds ahead of an explorer with a torch, every tile fitting its neighbours and every new stretch fitting the end of the last, four hundred columns of it; she finds her way herself and breaks through where it has cut her off; crystals glow in its dead ends | **Wave function collapse** (new): a `wfc` generator on the tilemap, `Wfc.generate` asking for eight columns at a time, `on_wfc_done` lighting the dead ends |
| 119 | **Eyes in the Dark** | A museum's vault at night from above: three guards walk their rounds sweeping their lanterns, each seeing exactly what his lantern lights - a cone cut by the display cases and the pillars, drawn as a fan - while a thief creeps from shadow to shadow toward the jewel; caught in a cone, every cone turns red and the room flashes, and she slips back to where she began | **Vision** (new): a `vision` per guard the shape of his spot light, its target tag the thief, the count read from `Vision.seen`; `Vision.set` turns the cones red |
| 120 | **A Day on the Island** | One island in the sea through a whole day in a minute: the sky goes from dawn orange through noon blue to dusk red and night, the sun crosses and the palms' shadows swing under it, the hut's lamp lights at dusk, stars and the moon come out; 1-4 jump to morning, noon, dusk and night, Space hurries the day | **Day cycle** (new): a `day_cycle` easing keys into the sky gradient, the ambient and the sun's `directional_light`, moving the sun and moon discs and fading the stars; the lamp lit from `on_dusk`, put out from `on_dawn` |
| 121 | **The Endless Road** | A little red car driving on forever down a country road through an afternoon into night: poplars, round trees, telegraph poles, farmhouses and fences built ahead of it a stretch at a time from a hash of each stretch and dropped behind it, two bands of hills sliding by, dust blowing off the wheels, headlights at dusk - and every few seconds the whole world moved back under it, which nothing on screen shows | **Floating origin** (new): a `floating_origin` on the car moves the world back past 4,000 units; the script builds stretches in true distance (an x plus the custom `FloatingOrigin.x`), the hill bands scroll by their own counters, scenery goes with `despawn_offscreen`, and a `day_cycle` turns the sky |
| 122 | **Bullet Time** | A brick warehouse at night: two gunmen behind crates fire across it at a hero running between them, and every few seconds (or on Space) the world slows to an eighth - the frame goes grey, bullets crawl and leave ripples in the air, casings tumble, the muzzle flashes linger and the music bends down - while the hero runs on at full speed | **Time scale** (new): `Time.set_scale` eased to an eighth, `Time.get_scale` read back; the hero's `time_scale.ignore_world` keeps his trail and dust on real time and his script divides `dt` by the scale; `Audio.follow_time` bends the music and the shots; ripples are the shockwave graph dropped behind each bullet |
| 123 | **Sundial Plaza** | A town square from the side through a day in a minute: an obelisk whose shadow tip walks over the hour stones set in the paving, townsfolk crossing with their shadows at their feet - long at dawn and dusk, a footprint at noon - a fountain, and at night two lamp posts that throw the walkers' shadows away from themselves | **Cast shadow** (new): a `cast_shadow` on every sprite lays its silhouette on the paving along the sun a `day_cycle` moves, or away from the two nearest lamps at night; the hour stones are placed by the same rule the engine uses |
| 124 | **The Paper Theatre** | A toy theatre at night: paper cut-outs on their slides - two rows of hills, a castle with its gate and windows cut through, a tree, a red dragon bobbing on its rod, clouds and a crescent moon - inside a curtained proscenium, lit by a candle carried slowly to and fro in front of them, every rim on its side catching the flame | **Auto normal** (new): no piece has a normal map; each carries an `auto_normal`, so its edges are rounded from its own silhouette and lit by the candle through the ordinary normal pass; N lays them flat (`AutoNormal.set`) to compare |
| 125 | **Torchlit Depths** | A dug-out cave of tiles, 120 by 40: a miner with a lantern walks the tunnel past eighty-odd flickering torches on stakes and wall sconces, glowing mushrooms and crystal clusters, over plank bridges above shafts down to two lava lakes whose embers rise - light spilling round every corner, glowing on the rock faces and dying in the deep rock | **Tile light** (new): one `tile_light` on the map floods every torch (`tile_light_source`), the lantern and the lava, mushroom and crystal tiles (`emitters`) through the cells; L puts it out (`TileLight.set`) |
| 126 | **Billiard Hall** | A pool table from above under one lamp: mahogany rail, six pockets, sixteen numbered balls with their shadows. A player lines up shot after shot - the cue swings round onto a ball it can pot, a dotted line shows where the cue ball goes off the cushions and into it, a ring marks the touch, a gold line carries the struck ball on - then draws back and strikes, and the balls go where the lines said | **Trajectory** (new): the cue ball's `trajectory` is set to the shot's velocity as the aim swings, and the engine steps a copy of the table ahead to draw both lines; the shot is struck with `Physics.set_velocity` at exactly that velocity |
| 127 | **The Glass House** | A Victorian glasshouse at dusk - white iron frame, brick base, palms and ferns lit by a hanging lamp inside - and stones lobbed in from the garden: each pane struck crazes into small shards round the impact and long ones away from it, which fall glinting onto the ledge; the gable's sloped panes break the same way, and the gardener reglazes when most of the glass is gone | **Voronoi shatter** (new): every pane, box or gable polygon, is a `breakable` with `pattern: "voronoi"`; fixed in its frame it breaks on the stone's impulse over its mass, and its shards are loose convex bodies cut from the same glass picture. Panes meet only stones and the ground, so falling shards do not break the panes below |
| 128 | **Quicksave Workshop** | A toymaker's bench in motion: a brass pendulum swings into a run of dominoes, glass marbles drop from a hopper down three ramps into a jar. Partway through the workshop quicksaves - a polaroid of the moment pinned to the wall - and a few seconds later loads it: every domino, marble and the pendulum jump back, moving and spinning as they were, and the same run plays again | **Save states** (new): `State.save` / `State.load` write the whole world down and put it back under the same handles; the run's clock and marble count live in a custom component so the load rewinds them too, and the polaroid is `Screenshot.take` into a texture |
| 129 | **Carnival Banners** | A fairground at dusk: WONDERLAND lettered round the gateway's arch under its chasing bulbs, words riding two pennant strings that sag and swing in the breeze, a striped tent, and a carousel whose horses rise and fall while a marquee scrolls along the front of its rim | **Text along a path** (new): every sign is world text with a `text_path` - the arch's `vector_path`, the strings' `line`s the script bends each frame (the letters ride with them), and the rim with `scroll` for the marquee |
| 130 | **Photo Album** | A lamplit table of holiday snapshots with a deck at its corner: swipe to deal the next, drag one about, pinch to zoom, twist to turn, hold to lift it off the table, double tap to look closer - and, left alone, a touch marker plays those same gestures itself | **Gestures** (new): the script reads nothing but `Input.get_gestures()` - taps, double taps, long presses, swipes, pans, pinches and twists recognised by the engine from the fingers (the mouse and wheel on a desktop) - and its idle demo feeds rows of the same shape through the same handler |
| 131 | **Rack Focus** | A misty forest at dawn in layers - far hills, two rows of pines, a deer in the clearing, ferns by the lens - and a camera pulling focus from one to the next, each layer softening as the focus leaves it | **Depth of field** (new): `depth_of_field` blurs each layer by how far its z is from `focus`, read from the z the velocity pass now writes; the script only eases `focus` from layer to layer. 1-4 pick a layer, F compares without it |
| 132 | **Beat Hall** | A club at full tilt: a floor of glass tiles, speaker stacks, sweeping beams and a crowd in silhouette, all hitting on the beat of a 124 bpm loop and changing pattern on every bar | **Beat events** (new): a `beat_clock` follows the music's own playhead and calls `on_beat` / `on_bar`; everything that pulses eases from its `phase`. M pauses the music and the clock keeps time alone |
| 133 | **Ember Storm** | A courtyard at night: a brazier cart rolling back and forth throws sparks that string out behind it, bounce down stone steps and off a slanted roof and settle in glowing drifts on the cobbles, while an iron ball swings through them and sweeps them aside; ash drifts down over it all | **GPU particles that keep state** (new): `simulate: "state"` steps every spark's position and velocity in a compute pass - born where the brazier is, bounced (`bounce`, `friction`), pushed by the ball's velocity, thrown on by the cart's (`inherit`). Space shows the same emitters in closed form |
| 134 | **Meadow Trails** | A meadow in late sun: thousands of grass blades, reeds round a pond and flowers, swaying in the wind and parting round a fox running its figure of eight - and round the mouse - then springing back behind | **A field things move through** (new): a `displacement_field` the fox and the mouse press outward as `displacer`s, and every blade a `field_bend` leaning away from the push where it stands on a damped spring |
| 135 | **Smooth Caves** | A cave of rounded, moss-edged rock lit by glowing crystals: a drill tunnels through it on its own and boulders roll down into what it opens; the mouse digs and builds | **Smooth terrain** (new): a `smooth_terrain` density grid drawn and collided as its marching-squares contour, dug and built with `Terrain.edit`, its collider swapped on the same body as it changes |
| 136 | **Fog of War** | A campaign map under fog: scouts ride their rounds and clear it, explored land stays dimmed, and the enemy's tents, campfires and raider appear only while someone is looking; the mouse sends a rider | **Fog of war** (new): a `fog_of_war` cell grid cleared by every `sight` and drawn through its own sprite, and `fog_hidden` things left out of every drawing pass while unseen |
| 137 | **Village Chatter** | A village square at the gate: three villagers who talk, with portraits, lines that type themselves out and choices that change what happens - a riddle's coins buy the bread that gets the gate opened | **A dialogue system** (new): conversations as data tables under `dialogue/` (lines, choices, conditions, variables, events) played by a `dialogue` component into the scene's own text, with `Dialogue.start` / `advance` / `choose` / `stop` |
| 138 | **Loot Satchel** | An adventurer's kit on a tavern table: items dragged between two leather bags and onto an equipment board, potions and coins stacking, the board refusing what does not fit; left alone, the kit sorts itself | **UI drag and drop** (new): `ui_draggable` items and `ui_drop_slot` places in entity UI - a ghost, slots tinted yes or no, stack, swap or refuse - the same by focus and confirm from a controller, and `UI.drop` for scripts |
| 139 | **Toy Town** | A little town of voxel houses, trees, lamps and carts on a green board the camera slowly turns over - every model turning with it and still standing up, carts riding the road loop | **Sprite stacking** (new): a `sprite_stack` draws a strip of slices bottom to top, raised up the screen whichever way the camera has turned, sorted by where its foot is on the screen |
| 140 | **Kart Circuit** | Four karts racing round a curbed circuit seen from behind the leader's - your - kart, the track sweeping toward the horizon, trees, tyres and signs shrinking into the distance and the sky turning with every bend | **Mode 7** (new): a `mode7` ground plane drawn in perspective as one quad with a perspective weight a corner, and `mode7_sprite`s placed and scaled by their distance, each showing the view of itself the camera sees |
| 141 | **Pocket Palette** | A sunset bay with a castle and a waterfall through classic palettes - its own sixteen colours with the sea cycling, then PICO-8, Game Boy, CGA, EGA and Sweetie 16 - gulls drifting over it | **A palette post effect** (new): the frame quantised to a preset or custom palette, last in the output pass, with 4x4 ordered dithering and palette cycling |
| 142 | **Behind the Walls** | A manor at night from above: a thief creeping through its wings and gardens, drawn as a bright cyan outline wherever the roofs and tree crowns hide him, the lantern-carrying guards outlined orange | **Occlusion silhouettes** (new): a `silhouette` entity hidden by a `silhouette_occluder` drawn again over it - outlined and faintly filled - clipped to the occluder's own pixels |
| 143 | **Card Table** | A hand of playing cards on baize that lean toward the pointer, lift and turn over in true perspective to show their backs, three more turning over on the table and a gold coin spinning on its axis | **Sprite tilt** (new): a `sprite_tilt` turns a sprite about its own x and y axes in perspective - one quad whose corners carry their perspective weight - with a back of its own |
| 144 | **Dash Echoes** | Two ninjas running the rooftops at night and dashing across the gaps, each dash leaving a trail of fading, tinted copies of their run cycle's poses, and a wisp overhead streaking light | **Afterimages** (new): an `afterimage` leaves timed, fading copies of its sprite's pose where it has been - only past a `min_speed` if asked, and `flat` for a shape in one colour |
| 145 | **Oil and Ink** | A harbour town on a bright afternoon - painted houses, boats bobbing, clouds and gulls drifting - shown in turn as a photograph, an oil painting, a comic-book page of CMYK dots and newsprint | **Painterly post effects** (new): `paint`, a Kuwahara filter, and `halftone`, dots on turned screens (mono or CMYK), both in the output pass |
| 146 | **Curtain Call** | A toy theatre's four acts - a forest, a desert night, the sea floor, a winter village, a dancer in each - each its own scene, handing over to the next by an iris, diamonds, a slide and a dissolve through velvet folds | **Scene transitions** (new): `Scene.transition` keeps the outgoing frame (copied as it is presented) and wipes it away over the new scene |
| 147 | **Fruit Slicer** | Watermelons, oranges, apples, kiwis and lemons tossed up over a wooden board and cut in two by the swipe of a blade, the halves tumbling apart showing their insides, juice spraying | **Slicing** (new): `Physics.slice` cuts a `sliceable` body and its picture into two bodies along a line, each drawn with its half (or its `inside_texture`) |
| 148 | **Portal Lab** | A test chamber side on, a blue portal in the floor and an orange one in the wall, each showing the view out of the other; crates dropped into the floor fly out of the wall, crates fired at the wall shoot up out of the floor | **Portals** (new): a linked pair, each drawn as the view out of its partner, and what goes in carried out of the other with its velocity turned |
| 149 | **Jelly Bounce** | Glossy jelly blocks on a kitchen table and shelf that squash flat when they land, stretch as they spring back, wobble in flight and quiver where they are clicked | **Jelly sprites** (new): a `jelly` draws its sprite as a grid of points on springs, pushed by changes in the body's motion and by pokes |
| 150 | **Beacon Run** | A scout crossing a wide moor at dusk to light five stone beacons, gold arrows round the screen's edge pointing to those out of sight with their distances, a flame icon over one in view | **Off-screen markers** (new): an `offscreen_marker` kept at the screen's edge pointing at its target, with a distance label, and an icon over it once it is in view |
| 151 | **Sketchbook** | A village drawn in pencil on cream paper - houses, trees, a sun, a figure walking past and birds overhead - whose lines boil, redrawn with a small wobble eight times a second | **Line boil** (new): the `boil` post effect reads the frame through a smooth noise offset drawn afresh at a set rate |
| 152 | **Spell Wheel** | A wizard in a moonlit tower room choosing spells from a radial wheel of six badges with the mouse or a stick, the one pointed at growing and lighting up, a click casting it in its colour | **Radial menus** (new): a `ui_radial_menu` lays entity-UI sections round its middle and picks the one pointed at by direction |
| 153 | **Rainy Window** | A city at night seen through a wet, steamed window: neon signs, a tram and car lights bent by beads and by drops running down the glass, each with a glint | **Lens rain** (new): the `lens_rain` post effect - drops forming, running and leaving trails, worked out from the clock, bending the frame behind them, and mist they wipe away |
| 154 | **Talk of the Town** | A market square full of chatter - stallholders calling their wares and passers-by talking back, each in a bubble over their head that follows them as they walk, a click making anyone speak | **Speech bubbles** (new): a `speech_bubble` sized to its words, its tail on the speaker, kept on screen and pushed clear of the others' bubbles |
| 155 | **Unit Select** | Soldiers on a meadow seen from above, picked by dragging a box, each picked one ringed in a pulsing gold glow that follows his own shape - shield, spear and bow - the pointer ringing a soldier white and an enemy red | **Sprite outlines** (new): a `sprite_outline` rings any sprite's own shape, solid then glowing, breathing if asked |
| 156 | **Thunderhead** | A storm at night over a lighthouse at sea: rain, a sweeping lamp, and branching lightning - never the same twice - from the cloud to the waves or the lighthouse's rod, lighting the sea, the tower and the cloud for an instant, thunder following late by the distance | **Lightning** (new): a `lightning` grows a branching bolt from a seed at every strike, flickers, fades and lights the world while it shows |
| 157 | **Millrace** | A mill yard from above: a race running out from under the mill pond, round two bends, under a footbridge and past a turning millwheel, its water and foam running down it and carrying leaves and a paper boat that turn the way they drift | **Flow along a path** (new): a `flow_path` runs a ribbon of water along a drawn curve, its texture running downstream round the bends, and carries what floats in it |
| 158 | **First Snow** | A village lane at dusk as the first snow falls: two cottages with lit windows, a fence, a lamp, a bench, a cart and pines, snow settling on every roof, chimney, sill, rail and bough - deep on the flat, thin on the steep - and thawing again | **Snow cover** (new): a `snow_cover` settles snow on a sprite's own upward edges, worked out from its pixels, deepening and melting |
| 159 | **Rooftop Runners** | A city block from above: five couriers running between five roofs on routes of their own, jumping the alleys (bigger in the air, their shadows left below), crossing a plank and a footbridge over the street and riding a zipline down across it | **Navigation links** (new): `nav_link`s join regions (and points of one) with ways that are not ground - agents plan across them, cross them and say what they are crossing |
| 160 | **Shape Shifter** | One glowing shape on a dark stage turning into the next - circle, star, heart, bird, crescent, drop, hexagon - outline and colour eased, overshooting and settling, never twisting; a row of the shapes along the bottom to click | **Path morphing** (new): a `path_morph` eases a `vector_path` into another's shape, resampled and lined up |
| 161 | **Trophy Room** | A panelled trophy room: clicking a trophy rings it and slides its achievement in as a gold toast at the top right, while everyday notices - a friend online, an item found - come and go in blue at the bottom left | **Toasts** (new): a `toast_stack` slides notices in from a corner, stacks and queues them and slides them away |
| 162 | **Instant Replay** | A pinball table in a dark cabinet - flippers on motored joints, three pink bumpers, two slingshots, three rollover lanes and a plunger - with the backglass keeping score on the left and the replay on the right: R records a run from a fresh table, P plays it back exactly, the REPLAY bar filling, and P again takes over | **Input replays** (new): every frame's input and frame time kept and handed back, so a run plays back exactly |

## Planned

Each picked because it needs something the engine does not do yet - the
scene is how the gap is found and closed.

The eleventh list (October 2026, after Spell Wheel) is done: its rows are
scenes 153 to 162 above.

The tenth list (October 2026, after Behind the Walls) is done: its rows are
scenes 143 to 152 above.

The ninth list (October 2026, after Beat Hall) is done: its rows are
scenes 133 to 142 above.

The seventh list (October 2026, after Haunted Manor) is done: its rows are
scenes 113 to 122 above.

The sixth list (October 2026, after Firefight) is done: its rows are
scenes 103 to 112 above.

The fifth comparison with Godot 4 and Unity 6 2D (October 2026) is done:
its rows are scenes 83 to 98 above, and its two stretch goals followed:
behaviour trees (99) and particle lights (100). Networked replication is the
server-framework session's. Editor tools (a Lua debugger, a curve editor, a
sprite-sheet slicer, weight painting, a PSD importer) and the missing build
targets are not loop items: they are planned in
[`docs/editor-tooling-plan.md`](../../docs/editor-tooling-plan.md).

## Fixed along the way

- **A controller could not move a `.gui` slider** (fixed Oct 2026, found by
  Readable Menus). Navigation reached it and left and right walked off it to
  the next node, while an entity-UI slider has always taken them as its
  value. They are its value now, a tenth of the range a press. Its toggles
  drew square whatever their `corner_radius`, and the focus ring was four
  square edges round a pill button; both follow the node's corners now.
- **A faded text left its outline and shadow behind** (fixed Oct 2026,
  found by Bazaar Signs, whose banner cross-fades its greetings). The
  outline and shadow layers drew in their own colour's alpha whatever the
  text's was, so a text faded to nothing still showed a dark ghost of
  itself. They are multiplied by the text's alpha now, world text and UI
  alike; `a_faded_text_takes_its_outline_and_shadow_with_it`.
- **`Physics.get_velocity` read 0 natively for a falling body** (fixed Oct
  2026, found by Test Lab scene 212). The frame's script run lends the
  physics world rather than copying every body's velocity into the
  snapshot, and in doing so stopped filling the velocities - so a body's
  velocity was its `Velocity2D` seed, 0 for anything spawned at rest, and
  only `fixed_update` saw rapier's. The getter asks the lent world now;
  `scripts_read_physics_velocities`.
- **`color_grade` contrast crushed every dark scene to black** (fixed Sep
  2026). The post shaders see linear colour, where middle grey is 0.18, and
  contrast pivoted at 0.5. At `contrast: 1.14` everything below linear ~0.06
  (sRGB ~27%) went under zero, so Firelight was black outside the brazier
  and its dim floor turned red: green and blue reached zero before red did.
  Contrast is now applied in log space about 0.18, as Unity and Godot do.
  `cargo test -p jengine-core --features gpu-wgpu --test
  post_process_grades_in_linear` runs the real post stack on the GPU and
  fails on the old shader.

- **HDR, tonemapping and a real bloom** (Sep 2026). The scene rendered
  into the 8-bit swapchain format, so nothing was brighter than white: a fire
  core and a lit wall were the same value, light saturated at a surface's
  albedo, and bloom was one 9-tap pass a few pixels wide. The world now draws
  into a float target; a scene with `"hdr": true` keeps light and colour past
  1.0 and the output pass tonemaps them (ACES by default), and bloom is a
  13-tap mip chain that reaches across the screen. Firelight is HDR: its
  flames are three to four times brighter than white, so they - and only
  they - bloom. The UI is drawn after the post chain now, so captions made of
  entity UI or `.gui` never bloom or vignette. Test Lab scene 109;
  `post_process_hdr` checks it on the GPU.

- **Blend modes** (Sep 2026). No renderer read a sprite's `blend_mode`, so
  every "additive" glow - including all of Firelight's - drew alpha-blended,
  unlit or not; and particles had no blend at all. Sprites, meshes and
  `Unlit` sprites honour it now, emitters have `blend_mode`, and Multiply no
  longer turns transparent texels into black boxes. `blend_modes_reach_the_screen`.
- **Particle curves and stretch** (Sep 2026): `color_curve` and `size_curve`
  over a particle's life (so dust fades in instead of popping), and `stretch`
  for sparks and rain.
- **Normal maps in draw order** (Sep 2026). Normal-mapped sprites were a
  forward pass drawn over every other sprite, so a bumpy floor covered the
  fire on it. They are ordinary sprites now; their normals go into a normal
  buffer the deferred lights read. A normal map wants headroom: in an LDR
  scene, a face turned to a bright light clips - use a moderate light or HDR.

- **Shadows past sixteen boxes** (Sep 2026). The whole screen shared 64
  occluder edges - sixteen boxes - for every light, truncated in silence.
  Each light now gets the edges in its own reach from a 448-edge pool (or
  one shared list when many lights see little geometry), so a scene can have
  as many shadow casters as its lights can see. Firelight's six pillars were
  36 of the old 64.
- **An occluder no longer shadows its own inside** (Sep 2026). Every
  occluder did: the ray from a light to a point inside crosses the outline on
  its way out, so a lit column that cast a shadow drew black, and Crypt gave
  its columns zero-width occluders to get away with it. `LightOccluder`
  has `self_shadow` now, off by default as in Unity: the inside takes the
  light, and what is beyond is still shadowed. Test Lab scene 112;
  `occluders_do_not_shadow_themselves`.
- **A script can change a sprite's normal map** (Sep 2026):
  `Material.set_normal_map(e, id)`, `nil` to take it away. Crypt's N key
  authored every surface twice and faded one copy out until it existed.
- **Shader graphs have noise** (Sep 2026): `Noise` (gradient fbm, each octave
  turned against the last) and `Voronoi` (Distance, Cell, and Edge = F2 -
  F1). A graph had no noise at all, and every effect in the gallery starts
  from one. The first version used value noise, which a colour ramp turned
  into rectangles, and a float hash, which put diagonal seams in it on
  screen. Test Lab scene 115.
- **Graph materials draw on the web, and in lit scenes when unlit** (Sep
  2026). Web builds drew every graph material with the default shader - no
  code fetched one - and every runtime's emission pass (unlit sprites in a
  lit scene) flushed plain, so an unlit graph vanished once a scene had a
  light. `graph_materials_reach_every_runtime`.
- **A multiply sprite darkens glowing things behind it** (Sep 2026). Unlit
  things are drawn into an emission buffer, and multiply sprites were
  skipped there - Nebula's dust lanes darkened only the black behind the
  glow. Test Lab scene 111; `unlit_sorts_with_lit`.
- **A light has a height** (Sep 2026). Every light sat at the same
  elevation (about 27 degrees), so a normal-mapped planet was lit nearly all
  round and could not have a night side. `PointLight2D.height` /
  `SpotLight2D.height`; Test Lab scene 114.
- **A shadow can end** (Sep 2026). Lighting is flat land - every occluder is
  infinitely tall - so with the sun beyond Stained Glass's north wall the
  first row of pews shadowed every pew behind it, down to the altar.
  `LightOccluder.shadow_length` bounds a shadow and fades its tip. Test Lab
  scene 113; `shadows_have_a_length`.
- **A graph can read the scene behind it** (Sep 2026): the `Scene Color`
  node (Unity's Scene Color, Godot's screen texture), what heat haze and
  refraction are made of. The Shader Gallery could not have heat haze until
  it existed. A sprite using it draws after the rest of the world, over a
  copy, so it sits above every world sprite whatever its z. Its first
  version returned the target's alpha, which additive flames had pushed past
  1 in the float target - and an alpha-blended haze with `1 - a < 0` punched
  a black hole in the fire it sat over; it is opaque now. Test Lab scene 116;
  `scene_color_reads_what_is_behind`.
  It reads the scene *after* the lighting composite: the first version
  copied the unlit albedo in a lit scene, so Spellcraft's shockwaves bent the
  floor and none of the glows - and every one of them was a black box, the
  cutout an unlit sprite leaves in the lit pass, faithfully copied.
- **A world transform can shear** (Sep 2026). The hierarchy was translate /
  rotate / scale only, so a child turned under a squashed parent turned as a
  whole thin rectangle - Spellcraft's summoning circle could not spin *on*
  the floor. Test Lab scene 117; `turned_children_shear`.
- **A light can cast no shadows** (Sep 2026). Every light was shadowed by
  every occluder, so a crystal light on top of each of Spellcraft's posts
  threw a wedge of dark down the floor below its own post.
  `casts_shadows: false`, as Godot's `shadow_enabled`. Test Lab scene 118.
- **Ambient occlusion** (Sep 2026). There was none: ambient light was one
  flat colour, so a white room of white boxes read as shapes pasted on paper.
  An `ambient_occlusion` component darkens the ambient light near occluder
  outlines, multiplying where they crowd and sparing an object's own top -
  built for Clay. Its first draft darkened every box's own top: the inside
  test borrowed the shadows' ray test, which ignores crossings near the
  start of a ray. Test Lab scene 121; `ambient_occlusion`.
- **Toon lighting** (Sep 2026). Light could only fall off smoothly, so a
  cel-shaded look meant painting the shading into every texture. A
  `toon_lighting` component steps the summed light into bands in the
  composite, by its brightest channel so a light keeps its colour, and
  `light_at` steps it the same way. Test Lab scene 126;
  `toon_lighting_bands_the_light`.
- **A graph can see its texture's size** (Sep 2026). A graph sampling its
  neighbours - an outline, an edge detect, a blur - had no way to know how far
  one texel is. The `TexelSize` node gives one texel in UV and the size in
  pixels; the showcase's ink outline is built on it. `graph_texel_size`.
- **A sun** (Sep 2026). A sun was a point light put thousands of units off
  screen, whose shadows fanned out slightly and could not lengthen as it set.
  `directional_light` is the engine's own: drawn as a point light 40,000
  units out along its direction, so the GPU pass, the occluder packing and
  `light_at` needed no new path - but two things did: the shadow ray skipped
  its first thousandth, forty units on a sun's ray, so nothing close to a
  point shadowed it; and a sun's height now stretches bounded shadows.
  `Lighting.set_height` came with it. Test Lab scene 125;
  `suns_cast_parallel_shadows`.
- **A graph sprite's cutout keeps its alpha** (Sep 2026). A lit frame draws a
  lit sprite as a black cutout in the emission layer (so it hides unlit things
  behind it), and it did so through the default pipeline - a graph whose
  alpha is its shape cut out its whole quad. Day & Night's lit fog blacked
  out the pond and every lamp glow beneath it. The graph draws its own
  cutout now. `graph_cutouts_keep_their_alpha`.
- **A light that gives no light costs nothing** (Sep 2026). A lamp or a moon
  at intensity zero was still drawn - a full shadow test per pixel it
  covered - and still took occluder edges from the pool. Day & Night's
  daytime moon and unlit lamps held it at 16 frames a second; they are left
  out now, and it runs at the display's rate.
- **Texture filtering** (Sep 2026). Every texture was sampled nearest with no
  mipmaps, whatever it held: right for pixel art, and wrong for every glow,
  cloud and star field here - blocky when stretched, crawling when drawn
  small. `settings.texture_filter` gives a project a default and per-folder
  overrides; linear comes with a mip chain. This project is linear except
  `sprites/pixel/`, so every earlier scene got smoother the day Pixel Perfect
  needed its pixels hard. Test Lab scene 123; `texture_filters_reach_the_screen`.
- **Pixelate and CRT** (Sep 2026). There was no way to show pixel art at the
  art's own resolution: a rotated sprite broke into pixels finer than its
  art, and a low-resolution look meant drawing everything small by hand. The
  `pixelate` post effect point-samples the frame on the art's grid, anchored
  to the world so a pan does not shimmer; `crt` is the television. Test Lab
  scene 124.
- **Trails** (Sep 2026). There were none: a comet's tail or a sword's
  swing was a row of sprites a script spawned and faded itself. A `trail`
  component records where its entity has been and draws a ribbon through
  the points in the sprite pass, with `Trail.set_emitting` / `Trail.clear`
  for scripts. Building Trails took four more fixes, each of which a still
  scene would never have shown: a trail's texture was never loaded (the
  preload scan did not know the component); a fast swing was drawn as a
  polygon, because frames arrive in bursts and the spline through the
  bunched points bent only at the bunches; a slash folded back to where
  the blade had rested, because native runs scripts before motion and the
  recording read the previous frame's positions (it now runs after the
  hierarchy resolves, in every host); and - see the next entry - the
  meteors hung in the sky. Test Lab scene 122; `trails_reach_the_screen`,
  `trails_reach_every_runtime`.
- **Ten Thousand Lights' lanterns never rose** (fixed Sep 2026). Their
  prefabs said `velocity2d`; the component is `velocity`. A key a scene does
  not know becomes a custom component - that is how a game's own `Stats`
  bag works - so the typo loaded, saved and did nothing, and ten thousand
  lanterns hung still in a scene described as rising. Trails' meteors,
  written by the same generator, hung at the top of the sky and gave it
  away. `authored_components_exist` now fails on any lowercase component
  key in a shipped scene or prefab that nothing registers.
- **A frame draws the lights its view can see** (Sep 2026). The pass took
  the first 256 lights in world order and dropped the rest wherever they
  were, so with more than 256 a light on screen could go dark while one off
  it kept its slot. Ten Thousand Lights has 300; the pass now draws those
  that reach the view, nearest first. Lights also follow their parents now.
  Test Lab scene 120; `lights_past_the_limit`.
- **Scene Color draws in a lit scene** (Sep 2026). The scene pass ran only
  when the world flush met a Scene Color sprite, and a lit frame leaves them
  out before the flush - so Underwater's wobble and Spellcraft's shockwaves
  drew nothing, and the lake of Ten Thousand Lights reflected nothing, which
  is what gave it away. Test Lab scene 120.
- **A lightmap can be drawn by a shader graph** (Sep 2026). A lightmap was a
  still texture, so light could not move: Underwater's caustics could only
  have been a picture. A lightmap entity with a `material` draws through its
  graph into the light buffer now - animated, procedural light. Test Lab
  scene 119; `lightmap_graph_materials`.
- **`Length`, `Dot`, `Distance` and `Normalize` measured a vec2 wrong**
  (Sep 2026). Their pins declared `vec4`, and a `vec2` was padded with an
  alpha of 1 on the way in, so the length of `uv - 0.5` was at least 1:
  every radial effect was flat. Spellcraft's shockwave - built on `Length` -
  had never bent anything, which nobody saw because a flat shockwave just
  looks like no shockwave. Underwater's test-lab twin, a ring of light,
  drew nothing and gave it away. `geometry_is_not_padded_with_an_alpha`.
- **A multiply sprite never brightens** (Sep 2026). The multiply blend
  assumed a premultiplied colour and got a straight one, so a soft
  multiply sprite brightened its whole quad wherever it was transparent -
  Spellcraft's scorch marks were pale rectangles, and Nebula's dust lanes had
  been lifting the space around them. Test Lab scene 118.
- **Unlit things sort with lit ones** (Sep 2026). `Unlit` sprites and text
  were drawn after the lighting composite, so they sat above every lit
  sprite whatever their z: Neon Rain's lit windows on the far towers showed
  through the buildings in front. The world pass now draws an unlit thing as
  a cutout in the lit scene and as itself in an emission buffer the
  composite adds after the light - full bright, and hidden by what is in
  front of it. Test Lab scene 111; `unlit_sorts_with_lit`.

- **A graph did not know where it was** (Sep 2026). `PositionInput` said
  "world position" and returned the sprite's UV, and nothing gave a
  fragment's place on the screen - so `SceneColor`, which takes an offset
  in screen space, could only be aimed by a fraction of the screen fixed at
  authoring time, and 09's lake mirrors correctly for exactly one camera.
  `PositionInput` is the world position now (a varying from the vertex
  stage), `ScreenPosition` is the screen's, and `WorldToScreen` projects any
  world point through the camera - Still Water's reflection. Test Lab scene
  130, `graph_knows_where_it_is`.
- **A Custom code node could not name its outputs** (Sep 2026). Its WGSL
  had to assign `n<node id>_<pin>` - a name the author could not know - and
  its inputs were `let`s at function scope, so two custom nodes sharing a
  pin name did not compile and the graph fell back to the default shader in
  silence. Each custom node is a block of its own now, reading inputs and
  assigning outputs by their pin names. The water graph is two of them.
  `custom_code_is_scoped_and_assigns_by_pin_name`.
- **Particles had no air** (Sep 2026). A spark flew at the speed it was
  born with until it died, so a firework burst was a ring flying off the
  screen. `drag` slows it by `e^(-drag t)` - frame-rate independent - and
  `turbulence` carries it along the curl of noise, so smoke curls into
  eddies without clumping. Test Lab scene 129.
- **A swarm could not be steered** (Sep 2026). `jengine-swarm` flocked flat
  rectangles inside a box and nothing else. Its boids now take a texture and
  a wingbeat (`flap`), and a script steers the whole flock through the
  `Swarm` custom fields every runtime reads - a target to wheel after, a
  threat to flee. Test Lab scene 131, `flock::tests`.
- **Dithered lighting** (Sep 2026). `toon_lighting.dither` draws each band's
  edge as a 4x4 ordered pattern in world-anchored cells, the CPU's
  `light_at` agreeing pixel for pixel. Toon lighting and ambient occlusion
  gained script setters with it (`Lighting.set_toon`, `Lighting.set_occlusion`),
  so Clay and Toon no longer destroy and respawn an entity to turn them off.
  Test Lab scene 127, `toon_lighting_bands_the_light`, `scene_lighting_setters`.
- **Frames in bursts judder** (Sep 2026). On a hybrid-GPU laptop a native
  Vulkan window at 165 Hz delivers 10 / 2 ms pairs; the frame pacer snapped
  the long one and passed the short one raw, so motion jumped and crawled
  though every frame was shown a refresh apart. It now paces by the recent
  average when that is a whole number of periods - `JENGINE_DT_LOG=1` shows
  it. Trails' ribbons bunched because of it.

- **A Scene Color sprite covered everything in front of it** (Sep 2026).
  It drew after the whole world, over every other sprite whatever its z, so
  nothing could float on the water. Every runtime now draws the world up to
  the lowest reader, copies it, draws the readers, then draws the rest over
  them - lit frames composite that part over the frame with the same lights
  (`WgpuLightingPass::composite_over`). Still Water's boat; Test Lab scene
  132, `scene_readers_sort_with_the_world`. Additive sprites keep the alpha
  under them now, since light added covers nothing.
- **The light buffer grows** (Sep 2026) instead of being allocated for a
  fixed cap: points then spots in one buffer, one upload a frame, room
  doubling as a frame needs it. `the_light_buffer_grows_with_the_scene`.
- **Light shafts** (Sep 2026), which the engine had no way to draw: a
  `light_shafts` post effect (screen-space scattering, as GPU Gems 3 and
  most 2D engines' god rays). Every pixel marches to the source, a world
  point, and gathers what is past `threshold`, weakening with distance from
  it (`length`, where it is at half strength). A first version stopped each
  march at `length` and drew a hard arc around the sun where rays first
  failed to reach it; they go all the way now, jittered so long steps grain
  rather than band. God Rays, Test Lab scene 133,
  `light_shafts_stream_from_the_source`.
- **Particles passed through everything** (Sep 2026) - rain could not land
  and a spark could not bounce. An emitter's `collision` now hits the
  scene's colliders (kill or bounce), tested as outlines on the CPU so it
  needs no physics, and `sub_emitters` burst another emitter, named by
  entity, where a particle dies or hits: Storm's splashes. Three fixes came
  with it: the tick is one function every host calls
  (`particles::tick_particles`), and it places an emitter at its **world**
  position, where each host had used the local one; native drew one
  particle system while `Particles.emit_burst` went into another, ticked
  and never drawn; and native's `Particles.get_count` answered 0.
  `Particles.set(e, field, value)` writes any emitter field by name. Test
  Lab scene 134, `particles_meet_the_world`.
- **A script could not play a skeletal clip, and a skeleton could not blend**
  (Sep 2026). `Animation.play` reached only sprite animation in every
  runtime (natively the name was bound twice and the sprite version won), so
  a rig played what its scene said and nothing else; and a player held one
  clip, so a change of clip snapped the whole skeleton in a frame. Now a
  skeletal player blends a second clip at the same phase
  (`Animation.set_blend`), crossfades (`Animation.crossfade`), and an
  `ik_two_bone` can plant an animated foot on the colliders beneath it
  (`ground`), with a `weight` over the pose. Test Lab scene 135,
  `skeletal_animation_reaches_scripts`.
- **A `.gui` could not look like a game's menu** (Sep 2026): no rounded
  corners, no shadow, no way to fade or scale a whole screen, button
  labels stuck to the left - and a button's hover and pressed colours,
  authored on every `.gui` button, were drawn by nothing. Nodes have
  `corner_radius`, `shadow`, `opacity` and `scale` now (the last two
  inherited, scale applied by layout so a click lands where the node is
  drawn), buttons ease into their hover and grow, borders draw over the
  widget, and `GUI.set` takes a table (natively a table was an error, in
  the browsers it was dropped). Test Lab scene 136,
  `gui_styles_reach_the_screen`.
- **Nothing could blur, and nothing on a patrol turned** (Sep 2026). The
  post stack had no blur of any kind; `tilt_shift` and `blur` now share one
  disc shader, the tilt-shift band a world height. And a `patrol_path`
  moved an entity without ever turning it - patrol writes no velocity, so
  `face_velocity` saw nothing - so every car slid round every corner
  facing the way it was placed. A patroller with `face_velocity` faces its
  way now, smoothly with a `turn_rate`. Test Lab scene 137,
  `tilt_shift_blurs_away_from_its_band`.
- **Nothing broke, and a side view could not be lit** (Sep 2026). There
  was no fracture of any kind; a `breakable` body now comes apart into
  pieces of itself when its velocity is jolted past `break_speed` - the
  contact impulse was tried first, and a block low in a tower "feels" the
  whole tower resting on it. And lighting is flat land, where every
  occluder is infinitely tall: each block of a tower shadowed the blocks
  beside it and the tower drew black but for its edges. An occluder with
  `shadows_casters: false` shadows the wall and spares other casters,
  known from marks the normal pass writes. Building the Test Lab scene
  found `Physics.set_body_type` put back every frame by the component it
  did not change. Test Lab scene 138, `things_break_when_hit_hard`,
  `casters_can_spare_each_other`, `body_type_changes_stick`.
- **Nothing bent** (Sep 2026). A banner was a sprite that could sway as a
  whole at most, and anything deformed was a script calling `Mesh.set`
  every frame with its simulation written in Lua. `cloth` and `rope` are
  components now, stepped by the engine and written into the entity's
  mesh; pins follow entities, points can collide, and `Cloth.set` turns the
  wind. Two things the flags found on the way: a 2D sheet has nowhere to go
  out of its plane, so slack cloth crumpled onto itself until `bend`
  resisted folding - and a taut one flew as a flat board until `wave`
  pushed a ripple across it. Test Lab scene 139, `cloth_and_rope`.
- **Nothing drew the world small** (Sep 2026). Scene 12 got its look from
  `pixelate`, which point-samples a full-resolution frame: full cost, cells a
  pixel apart in size between whole multiples, and a pan that steps. A
  camera with `pixel_size` draws the world at the art's resolution now, on
  its grid, and slides the scaled-up picture by the sub-pixel remainder.
  Test Lab scene 140, `pixel_perfect_camera`.
- **There was one camera a frame** (Sep 2026), so a monitor, a minimap and
  split screen were all out of reach - the editor's picture-in-picture was
  the only second view, unlit and straight onto the display. A camera with a
  `target` is a view now: each host's world pass is a loop over the views and
  then the screen, each with its own lighting and post buffers, and the
  texture is an asset any sprite can name. Test Lab scene 141,
  `cameras_draw_into_textures`.
- **Every camera saw everything** (Sep 2026), so the minimap was the town
  shrunk - roofs, trees and all - with no way to mark the players on it
  alone. An entity's `render_layers` and a camera's `cull_mask` decide what
  each camera draws, through the frame view every pass already sets. Test
  Lab scene 145, `cameras_cull_by_layer`.
- **Text was one colour with no markup** (fixed Oct 2026): a line of
  dialogue naming a place in gold took three entities placed by hand, and
  went wrong the moment the line changed; nothing could type itself out.
  `"rich": true` reads colour, bold, wave, shake, rainbow, pause and icon
  markup, and `reveal_speed` is a typewriter. The Storyteller found it -
  and that a bright rainbow vanishes on paper, so a rainbow keeps the
  text's own brightness. Test Lab scene 150, `rich_text_draws_its_markup`.
- **Every tile was placed by hand** (fixed Oct 2026): a coast, a cliff or a
  path meant choosing each edge and corner piece, and a script that dug or
  flooded a cell had to redo its neighbours. An `autotile` component makes a
  layer's cells terrain numbers and draws their edges itself, on a dual grid
  (`tilemap::autotile`) - sixteen tiles a terrain, and terrains stack, so
  sand, grass and rock meet in every pairing from forty-eight tiles. Test
  Lab scene 151, `tilemaps_autotile_their_terrains`.
- **A tilemap was square and under every sprite** (fixed Oct 2026), so a
  village could not be built of tiles: no diamonds, and a villager walked
  over the houses whatever their places. A map can be isometric now, a
  layer can be lifted, and a `y_sort` layer's tiles sort with the sprites -
  and every reader of the grid's shape (drawing, picking in all four
  runtimes, autotiling, colliders, the editor's grid) goes through one pair
  of functions. Test Lab scene 152, `tilemaps_can_be_isometric`.
- **A texture could not change after it loaded** (fixed Oct 2026), so
  destructible land could only be faked with piles of small sprites. A
  `carvable` sprite carves holes in a copy of its texture, written to the GPU
  a rectangle at a time, and its collider is rebuilt from what is left. On
  the way, Crater Isle found three more: a script could not recognise a
  body it had spawned in a raycast hit or a contact (they named the real id,
  the script held the spawn handle); the browsers' raycast went straight
  through tile walls and carved land; and the first version's GPU write was
  queued where no host ever asked for it. Test Lab scene 153,
  `sprites_can_be_carved`, `rays_and_contacts_name_spawned_entities`.
- **Text was one colour** (fixed Oct 2026), so a caption over a busy picture
  needed a dark strip behind it - Mapmaker's hint has one - and a neon sign
  or a cartoon sticker could not be lettered. World and entity-UI text take
  an `outline` and a `shadow`; on the distance-field font they are the glyph
  with its edge moved out and softened, so they cost a quad a glyph. Test
  Lab scene 154, `text_has_outlines_and_shadows`.
- **Terrain had to be tiles, a hand-built mesh or a carved image** (fixed
  Oct 2026). A `spline_shape` makes it from a few points: a tiled fill, an
  edge strip along the curve and a solid collider, the tiling cut into the
  geometry because every sampler clamps. On the way, the editor's derived
  inspector learned to edit a list of points - it offered nothing for one, so
  the curve could not have been changed there at all. Test Lab scene 155,
  `terrain_follows_spline_shapes`.
- **Water, wind and a pull were a script applying forces every frame**
  (fixed Oct 2026). An `area_effect` box does it inside the physics step:
  buoyancy, a steady force that particles feel too, attraction, drag. Making
  boats float found two things that were wrong in a way nobody could see: a
  push written as an impulse does not cancel gravity in rapier (gravity is
  spread over the solver's substeps, the impulse is not), so every float sat
  a tenth of its height low and neutral water lifted things; and rapier's
  `reset_forces` does not reset torques, so the first righting torque spun
  the trawler like a propeller. Test Lab scene 156,
  `area_effects_push_what_is_in_them`.
- **A hex map from Tiled imported as square** (fixed Oct 2026). A tilemap
  can be hexagonal now - pointy or flat-topped, odd or even rows pushed -
  and drawing, picking, the colliders and `find_path` follow it, with
  `Tilemap.neighbors` and `Tilemap.distance` for the counting a hex game
  does. Test Lab scene 157, `tilemaps_can_be_hexagonal`.
- **A sprite's shadow was its box** (fixed Oct 2026), unless somebody typed
  in a polygon for it. `from_alpha` on an occluder traces the outline of the
  sprite's opaque pixels instead - one loop per island, holes left out,
  simplified - so a tree with gaps in its crown lets light through them.
  Test Lab scene 158, `occluders_take_their_sprites_shape`.
- **A light was a round falloff and nothing else** (fixed Oct 2026), so
  light through a window or a fan's turning shadow was a lightmap painted by
  hand that could not move with its light. A `cookie` is a texture the light
  shines through, turning with it - bound beside the normals, because the
  light pipelines already used every bind group WebGPU guarantees. Test Lab
  scene 159, `lights_shine_through_cookies`.
- **A tile was one still picture** (fixed Oct 2026), so moving water in a
  tile map was sprites laid over it. A tileset's `animations` draw a tile as
  its frames on the map's own clock - chosen where the tile is placed, so
  colliders, `get_tile` and pathfinding never see a frame - and Tiled's tile
  animations import. Test Lab scene 160, `tiles_animate`.
- **A script could ask the world about a line and nothing else** (fixed
  Oct 2026): `raycast` has no width, and `find_in_radius` reads transforms,
  so a blast missed a wall whose centre was out of reach. The overlap
  queries and `circle_cast` ask the colliders, with one implementation for
  the live physics world and the browsers' snapshot. Test Lab scene 161,
  `shape_queries_ask_the_colliders`.
- **Every sound played centred at its own volume, wherever it was** (fixed
  Oct 2026), and a looping `audio_source` played on after its entity was
  despawned. A spatial source fades with distance and pans to its side
  every frame, `Audio.play_sound_at` places a one-shot, and every looping
  source stops with its entity. Test Lab scene 162,
  `sounds_are_heard_from_where_they_are`.
- **Nothing could walk round a placed obstacle** (fixed Oct 2026): a tile map
  had a path, cell to cell, and a level of placed things - a fountain,
  benches at an angle - had none, so every game steered by hand and stuck on
  the first concave corner. A `nav_region` is baked from its colliders into
  exact shortest paths; `nav_agent`s walk them and keep out of each other's
  way. Test Lab scene 163, `agents_find_their_way`.
- **No text could be translated** (fixed Oct 2026): a game in two languages
  kept its own tables in Lua and rewrote every label itself. String tables
  are data tables under `locale/`; `text_key` keeps text in the current
  language, and `Locale.get` picks plurals and decimal separators. Along the
  way: a keyed Lua table (`{ n = 5 }`) reached every native function as
  `null`. Test Lab scene 164, `text_speaks_the_players_language`.
- **A sound could not be muffled or echo** (fixed Oct 2026): the mixer's
  buses were a volume calculation, with every sound played straight to the
  output. They are real tracks in both backends now, each with a low-pass and
  a reverb (`Audio.set_bus_effect`), and an `audio_zone` sets them while the
  listener is inside. Test Lab scene 165, `buses_carry_effects`.
- **The camera followed every step, and could not say where it was**
  (fixed Oct 2026): `camera_follow` eased onto its target with no dead zone or
  look-ahead, none of its fields could be changed from a script, and there
  was `Camera.get_zoom()` but no position - a following camera's centre was
  for the script to work out. `dead_zone`, `look_ahead`, `Camera.set_follow`
  and `Camera.get_position`. Test Lab scene 166, `the_camera_holds_and_leads`.
- **A pad could not rumble, nor a phone buzz** (fixed Oct 2026): gamepads
  were input only. `Input.rumble` drives a pad's two motors on every
  platform and `Input.vibrate` a phone's. Test Lab scene 167,
  `pads_rumble_and_phones_buzz`.
- **A skeleton played one pose at a time** (fixed Oct 2026): a clip, a
  blend of two, a blend space or a crossfade - but always over every bone,
  so a character could walk or draw a bow, not both, and anything more had
  to be keyed bone by bone in a script. Layers with bone masks now. Test
  Lab scene 209, `skeletons_play_layers`.
- **A sound's id reached no sound** (fixed Oct 2026): `Audio.play_sound`
  returned the script's own counter, which natively never met the
  backend's handle and in the browsers was 0 - so `stop_sound(id)` and
  `set_sound_volume(id)` had never stopped or turned down the sound they
  named, and a looping sound a script started could not be stopped at all.
  Music was one track swapped hard, kept in three hand-written copies.
  Test Lab scene 208, `music_crossfades_layers_and_playlists`.
- **No runtime knew what a controller was called** (fixed Oct 2026):
  every pad was "Pad0", so a prompt could only say "A" - which is Cross on
  a PlayStation pad and B on a Switch - and a hold, a tap or a double tap
  was a timer each script kept in `dt` beside the action. Pads are named
  and typed now, prompts follow the device in hand, and actions answer
  `is_action_held` / `tapped` / `double_tapped`. Test Lab scene 207,
  `actions_hold_tap_and_name_their_buttons`.
- **A scene came only whole, and all at once** (fixed Oct 2026):
  `Scene.load` read and decoded every texture on the frame it was asked
  for, and there was no adding a scene to the one running, so a level
  bigger than a scene was one enormous scene. `Scene.load_additive`,
  `Scene.unload` and `Scene.preload` now. Test Lab scene 206,
  `scenes_stream_in`.
- **A text field had no caret** (fixed Oct 2026): it appended what was
  typed and Backspace took the last character - no selection, no paste, no
  IME, and a long line ran off the field's edge. Test Lab scene 205,
  `text_fields_edit_like_text_fields`.
- **An entity-UI text field heard the keyboard only while the pointer was
  over a widget** (fixed Oct 2026): its update sat after an early return
  for "the pointer hit nothing".
- **Natively a game saw the keys typed into a field** (fixed Oct 2026): the
  native runner updated the UI after running the scripts, so H typed into a
  field also went back to the hub. It updates the UI first now, as the
  browser hosts always did.
- **A game could not leave its window** (fixed Oct 2026): no fullscreen,
  no window size, no vsync switch and no cursor control from a script, so an
  options menu had nothing to call. `Display.*` and `Cursor.*` now. Test Lab
  scene 204, `display_settings_reach_every_runtime`.
- **A frame cap with vsync off drew over the cap** (fixed Oct 2026): the
  cap waits 90% of its interval so a vsynced frame lands on the refresh it
  meant, and with no refresh to land on a cap of 30 drew 33.
  `FrameCap::exact` waits the whole interval when vsync is off.
- **A prefab was one entity** (fixed Oct 2026): a cart with wheels was a
  script spawning and parenting each wheel itself. A `.prefab` takes
  `children` now, in every runtime and both scene versions. Test Lab scene
  203, `prefabs_have_parts`.
- **Destroying a parent left its children behind** (fixed Oct 2026): nothing
  that despawns an entity walked its children, so a destroyed cart's wheels
  stayed, drawn at their local offsets about the origin, with a warning a
  frame each. A part goes with its parent now
  (`ecs::hierarchy::despawn_orphans`).
- **A v2 scene ignored `prefab_instance`** (fixed Oct 2026): the expansion
  returned early for v2, so an instance loaded with only what the scene
  itself wrote.
- **Light did not bounce** (fixed Oct 2026): a lit floor lit nothing but
  itself, so a coloured patch on the stone never warmed the wall above it
  and every shadow was the ambient colour. A `global_illumination`
  component bounces it by radiance cascades. Test Lab scene 202,
  `light_bounces`.
- **A GPU particle went through everything** (fixed Oct 2026): the
  stateless cloud could not collide or be drawn anywhere but its own centre.
  It now meets the nearest colliders (a particle inside one is put out on its
  nearest side) and up to four attractors that pull and spin it - both in
  closed form, so it is still one uniform and one draw. Test Lab scene 201,
  `gpu_particles_meet_the_world`.
- **A tile was only a picture** (fixed Oct 2026): it could animate and
  collide, but a game could not ask what it was, could not vary it, and could
  not paint a chest with the tile brush. A tileset now carries `data` per
  tile (Tiled's tile properties import as it; `Tilemap.get_tile_data`),
  `random` weighted variants, and `spawns` that load as prefab instances.
  Test Lab scene 200, `tiles_carry_data`.
- **A part could not be swapped by name** (fixed Oct 2026): changing a
  rigged figure's hat meant knowing the hat's texture path in every script
  that changed it. A `sprite_library` names parts by category and label, a
  `sprite_resolver` on a sprite shows one, and `Sprite.set_label` swaps a
  part or a whole outfit. Test Lab scene 199, `sprites_swap_by_label`.
- **A bus could only muffle and reverberate, and every footstep was the
  same** (fixed Oct 2026): a bus had a low-pass and a reverb, nothing else,
  and a one-shot played at exactly the pitch and volume asked for. A bus has
  an echo, a distortion, a compressor and a three-band EQ now, and
  `Audio.set_variation` gives a clip a random pitch and volume wherever it
  is played. Found with it: kira's own soft clip divides by its drive, so a
  fully driven bus came out a sixtieth as loud - the distortion is a `tanh`
  that keeps full scale. Test Lab scene 198, `buses_echo_distort_and_compress`.
- **A camera could only cut, and its bounds held only its centre** (fixed Oct
  2026): the main camera was the first active one the query met, so changing
  cameras was a cut, and `CameraFollow.bounds` clamped the view's *centre*
  to a rectangle - an L-shaped room showed the street at its inside corner.
  Cameras have `priority`, `blend` and `confine` now, and `Camera.set`. Found
  with it: a script's integer reaches native as a float, which serde refused
  for an integer field. Test Lab scene 197, `cameras_blend_and_stay_in_shapes`.
- **A prefab could not build on another prefab** (fixed Oct 2026): scene
  instances could override a prefab, but a variant had to be a full copy
  that drifted from its original. A prefab whose own components carry a
  `prefab_instance` extends the prefab it names. Test Lab scene 196,
  `prefab_variants_inherit`.
- **A rebinding screen could not ask which key was pressed** (fixed Oct
  2026), short of testing every key there is; `Input.get_just_pressed()`
  answers. On the way: `Input.get_bindings` and `Input.get_actions` returned
  **only their first item** in the editor and on the web - both were in
  `lua-runtime.ts`'s `MULTI_RETURN`, which spreads a list into separate
  values, while native returns one table. `multi_return_matches_native`
  checks that direction now too. Test Lab scene 195,
  `actions_rebind_at_runtime`.
- **No runtime read the middle mouse button** (fixed Oct 2026): native
  matched Left and Right, the browsers buttons 0 and 2, so a pan on the
  wheel did nothing anywhere. `Input.is_mouse_middle_pressed` and its two
  edges now, with the browsers' autoscroll stopped. A Test Lab scene only
  (194), `middle_mouse_reaches_scripts`.
- **A belt had to be a script pushing every box** (fixed Oct 2026): there
  was no moving surface. A `conveyor` on a collider slides what rests on it,
  by friction. Test Lab scene 193, `conveyors_carry_what_rests_on_them`.
- **A turned edge stair-stepped** (fixed Oct 2026): there was no
  anti-aliasing at all, so a slowly turning wire figure crawled. An `fxaa`
  post effect smooths it. Test Lab scene 192, `fxaa_smooths_edges`.
- **A collider was a box, a circle or a polygon typed in** (fixed Oct
  2026): odd-shaped junk stacked as the boxes round it, floating over the
  empty corners. `sprite_collider` traces the sprite's pixels, as shadows
  already were, and writes the outline into the collider. Test Lab scene
  191, `sprites_collide_by_their_pixels`.
- **Only world text had markup** (fixed Oct 2026): a quest log or a shop
  drawn in the UI was one colour per text, its icons placed beside it by hand.
  `ui_text` and `.gui` Text nodes take `rich` now, and `ui_text` the
  typewriter. On the way: a `[shake]` jittered three times as far as meant,
  mostly left, because a negative sine's `fract` is negative - a colon after
  a shaking word vanished into its last letter now and then. Test Lab scene
  190, `rich_text_reaches_the_ui`.
- **A skeleton blended along one axis** (fixed Oct 2026): `blend_clip` mixed
  two clips by one number, so a character that walks in eight directions and
  runs had to pick clips in a script and crossfade between them. A
  `blend_space` places any number of clips on two axes and mixes them by a
  position. Test Lab scene 189, `blend_spaces_mix_by_two_axes`.
- **IK was two bones or one** (fixed Oct 2026): a tail, a tentacle or a
  long arm had to be a hand-written solver in a script, a frame behind.
  `ik_chain` bends any number of bones (FABRIK) with a per-joint limit and a
  weight, and the existing `Skeleton.set_ik_target` / `set_ik_weight` reach
  it. Test Lab scene 188, `ik_chains_reach`.
- **No script could step with the physics** (fixed Oct 2026): `update`
  runs once a frame, so a controller in it ran 165 times a second on one
  display and 60 on another, against a physics that steps at 60, and a
  replay never came out the same. `fixed_update(entity, dt)` runs once
  before each step. On the way: a force could only push for a whole frame,
  and the first version's `get_velocity` read zero. Test Lab scene 187,
  `scripts_step_with_the_physics`.
- **Text stopped at Latin script** (fixed Oct 2026): a font was a pre-baked
  atlas, and the built-in one ends at Latin Extended-A, so Russian, Greek,
  Japanese, Chinese and Korean had no way onto the screen. A `font_id` can
  name a `.ttf` / `.otf` now, its glyphs baked as text asks for them. On the
  way: entity-UI text's own font was never loaded in any runtime. Test Lab
  scene 186, `fonts_load_from_font_files`.
- **Nothing could be keyframed** (fixed Oct 2026): a cutscene was a script
  of tweens and timers. A `timeline` holds clips of tracks over any
  component's fields and event keys that call its script. Test Lab scene
  185, `timelines_animate_any_field`.
- **Split screen took a target and a UI image per player** (fixed Oct
  2026): a view drew into a texture, and showing it on half the screen was
  the game's job. A camera's `screen_rect` does both, and the view keeps the
  full screen's scale rather than fitting the whole world into its half.
  Test Lab scene 184, `cameras_split_the_screen`.
- **Nothing fell like a body, and nothing gave way** (fixed Oct 2026): a
  skeleton was posed by clips, IK and jiggle and never by physics, and a
  joint held whatever pulled it. A `ragdoll` turns a skeleton's bones into
  capsules on hinges limited about the pose it fell in, and a joint's
  `break_force` / `break_torque` snaps it. On the way: a physics command on a
  spawned entity went nowhere natively - the `spawn_prefab` handle reached
  rapier as an id no body had - so Siege Blast's cannon dropped its ball at
  the muzzle; and a mirrored skeleton root stood a knight on its head. Test
  Lab scene 183, `joints_break_and_skeletons_go_limp`.
- **A look could not be painted** (fixed Oct 2026): the only grade was
  `color_grade`'s sliders, so a grade made in an image editor, as Unity and
  Godot take one, had no way in. A `color_lut` strip grades the frame in the
  output pass, after the tonemap, and `film_grain` adds grain there. Test
  Lab scene 182, `color_luts_grade_the_frame`.
- **Particles could only start from a point, a circle, a box or a line**
  (fixed Oct 2026): a burning outline took dozens of emitters placed by hand.
  `emit_shape` `path` starts them along the entity's own `vector_path`, and
  `sprite` over its sprite's opaque pixels or its outline. Test Lab scene
  181, `particles_start_on_shapes`.
- **A particle left no trace** (fixed Oct 2026): `stretch` drew one along its
  velocity, a straight streak with no memory of a curve. An emitter's `trail`
  records where each particle has been and draws a tapering, fading ribbon
  through it. Test Lab scene 180, `particle_trails_draw`.
- **Every light lit everything** (fixed Oct 2026): a character's key light
  washed the backdrop, and lightning lit the people it should have
  silhouetted. A light's `light_mask` and a sprite's `light_layers` decide
  who it lights; the layers are written beside the normals and read by the
  lights. Test Lab scene 179, `lights_light_their_layers`.
- **A shader could not move anything** (fixed Oct 2026): graphs were colour
  only, so grass, banners and anything that sways had to be moved sprite by
  sprite from a script, or drawn as frames. A graph's `Vertex Offset` node
  moves every vertex before projection, per vertex, on the GPU. Test Lab
  scene 178, `graphs_move_their_vertices`.
- **A tile was a whole box, and tiles cast no shadows** (fixed Oct 2026): a
  slope was a staircase, a plank a full block, and a lit cave had to be
  outlined again by hand in `light_occluder`s. A tileset's `shapes` give a
  tile its collision and shadow outline, and a `tilemap_occluder` casts a
  layer's shadows from its cells merged into a few long edges. Test Lab
  scene 177, `tiles_have_shapes_and_cast_shadows`.
- **Nothing could play a video** (fixed Oct 2026): a cutscene, a TV in a
  room or a film on a screen had to be a sprite sheet. A `video_player`
  plays a file into a texture through the platform's own decoder - Media
  Foundation natively, the browser's `<video>` in the editor and web - so
  whatever names the texture shows it. Test Lab scene 176,
  `videos_play_into_textures`.
- **A figure of parts could not fade** (fixed Oct 2026): fading each part
  showed the parts through each other. A `canvas_group` draws the hierarchy
  into a texture of its own - a view through the main camera that draws only
  its members onto transparent - and draws that once, at its opacity. Test
  Lab scene 175, `canvas_groups_fade_as_one`.
- **Nothing shone** (fixed Oct 2026): normal-mapped sprites took diffuse
  light only, so gold and stone differed only in colour. A material's
  `specular` adds highlights, masked by the normal map's alpha. The first
  try put them nowhere (the 2D light's fixed elevation gives one mirror angle
  everywhere) and then dimmed them by the surface's colour; they are added
  over it now, from a light hung `height * radius` up. Test Lab scene 174,
  `surfaces_shine`.
- **A line was one width and one colour** (fixed Oct 2026): `vector_path`
  strokes a path evenly, a trail only follows its entity. A `line` has a
  width curve, a gradient, a tiled or stretched texture, joints and caps.
  Its first bevel and round corners had holes - fanned from the corner point
  rather than the inner corner - which the Test Lab scene showed and a test
  now samples. Test Lab scene 173, `lines_are_drawn_with_style`.
- **A character of parts could not pass another** (fixed Oct 2026): with one
  z band for everyone, every front arm in a crowd drew over every body. A
  `sorting_group` sorts a whole hierarchy as one drawable, its parts in their
  own order inside. Test Lab scene 172, `sorting_groups_keep_parts_together`.
- **Nothing rode a curve** (fixed Oct 2026): `patrol_path` walks straight
  lines between waypoints, so a train on a curved track had to be a script
  sampling a bezier itself. `path_follow` rides any `vector_path` at an even
  speed, in lanes, round or once. Test Lab scene 171,
  `riders_follow_drawn_curves`.
- **Nothing knew it was seen** (fixed Oct 2026): a script that wanted to
  wake when its entity came into view compared itself against the camera
  every frame. A `visibility_notifier` calls `on_screen_entered` /
  `on_screen_exited`. Building the scene found two native-only bugs: a table
  sent with `Event.send` threw, and an event sent from inside such a
  callback (or a collision or animation one) reached no listener - Test
  Lab scene 170's counter stayed at zero natively while every lantern lit.
  `things_know_when_they_are_seen`.
- **Nothing hung loose** (fixed Oct 2026): a bone was where its clip or its
  author put it, so a tail, a tentacle or a strand of kelp had to be
  animated by hand or by a script turning every link. A `jiggle` bone lags
  its pose and springs back, parent first, so a chain trails and whips on its
  own - and found that a long soft chain needs a small `max_angle` per link,
  or it folds into kinks. Test Lab scene 169, `bones_jiggle`.
- **A light had to be round** (fixed Oct 2026): a point, a cone, a sun, or a
  lightmap's picture - nothing could light a window-shaped pool or a stage
  beam with soft edges. A `freeform_light` is a polygon of light with a
  falloff, which `light_at` sees too. Test Lab scene 168,
  `freeform_lights_glow`.
- **A pixel-perfect camera scaled up by whatever the window gave it**
  (fixed Oct 2026): between whole multiples of the art a window got art
  pixels four and five screen pixels wide by turns, as any nearest upscale
  does. `integer_scale` on the camera makes them whole, showing a little more
  world rather than black bars - so nothing converting a screen point needs
  to know. Test Lab scene 149, `pixel_art_scales_by_whole_pixels`.
- **Scene Color sprites could not see each other** (fixed Oct 2026): every
  reader read one copy taken below the lowest of them, so a heat haze over
  a reflecting lake bent the lake's floor and not its reflection (Godot's
  `BackBufferCopy` has the same rule). A host now cuts the world at every
  band of readers and copies at each (`SceneReaderPlan`), so a reader sees
  the ones below it. Test Lab scene 148, `scene_readers_sort_with_the_world`.
- **A mask was a shape** (fixed Oct 2026): a rectangle, an ellipse or a
  polygon of sixteen points, so Through the Window's portal was an oval -
  no painted silhouette could clip anything. A `sprite_mask` can be a
  texture's alpha now (`MaskShape::Texture`, Unity's `SpriteMask`); the
  portal is a ragged tear. Test Lab scene 147,
  `sprites_can_be_masked_by_a_texture`.
- **A game's own font stayed a bitmap** (fixed Oct 2026) after the built-in
  one became a distance field, so Menus & HUD's tabletop face softened
  wherever it was drawn larger than its 64 px. `jengine font-sdf` turns any
  bitmap `.font` into a field without moving its text, and a `"sdf": true`
  font draws as one in every runtime. The tabletop font is one now. Test Lab
  scene 146, `custom_fonts_can_be_distance_fields`.
- **Nothing could clip anything** (Sep 2026): a window onto another layer
  had to be painted into the wall, and a portal could only be a picture. A
  `sprite_mask` shape clips whatever is `masked` by it, inside or out, in
  every sprite shader including graphs, through the batch's globals - no
  stencil buffer and no host wiring. Test Lab scene 142,
  `sprites_can_be_masked`.
- **Text blurred when it grew** (Sep 2026): the built-in font was a 16 px
  bitmap, so a title or a zoomed-in label was sixteen-pixel glyphs
  stretched. It is a signed distance field now, baked at 48 px and drawn
  with a one-pixel edge at any size - by every text path at once, because
  the texture says what it is. Test Lab scene 143, `text_stays_sharp_scaled`.
- **Particles cost per particle** (Sep 2026): every CPU particle is
  simulated, sorted and written every frame, so a few thousand is the
  budget. A `gpu_particles` emitter is stateless - a closed form the vertex
  shader evaluates - and a cloud of two hundred thousand is one draw call.
  On the way: the web runtime never told its sprite batch the time, so
  every animated graph material stood still in a web build. Test Lab scene
  144, `gpu_particles_draw`.

## What the renderer cannot do yet

These were found while building the scenes above. Each one is a real
difference from Unity / Godot, and each is an engine fix rather than
something a scene can work around.

Nothing open right now - every gap the thirty scenes found is fixed and in the
list above. What is left, and worth knowing:

- **Stateful GPU particles need compute.** `simulate: "state"` (Ember
  Storm) bounces, pushes and trails them, but a device without compute -
  the WebGL2 fallback - draws them in closed form, which cannot.
- **Lights are bounded at 16,384 of each kind a frame** - a safety bound,
  not a budget: the lighting pass's buffer grows with the lights a frame
  draws (it was a fixed 256 until Sep 2026, then 1024). What a light costs
  is its quad of screen, so past a few hundred at once, glowing things want
  to be emissive sprites, as most of Ten Thousand Lights' lanterns are.
