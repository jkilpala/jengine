-- The showcase's scene list and the navigation every scene shares.
--
-- SCENES is the one list: the hub draws its menu from it and each scene's
-- prev / next walk it, so adding a scene is a row here plus the scene itself
-- (tools/gen_scenes.py). The hub's list scrolls, so it has no limit. PgUp / PgDn step
-- the list; `[` / `]` work too, but a Finnish layout types them with AltGr.
--
-- `require` caches this module for the whole session natively (CLAUDE.md,
-- "A required module's state outlives a scene"), but the editor and web
-- give every script its own Lua state, so module state is per script there
-- and lost with the scene. So the module holds no state - only the list and
-- pure functions - and what the hub remembers across scenes (where its list
-- was scrolled to, and the scene last visited) goes in a save slot that is
-- never committed: the host keeps it in memory for the session, in every
-- runtime, and nothing reaches the disk.

local M = {}

local MEMORY = "showcase_nav"

M.HUB = "scenes/hub.scene"

M.SCENES = {
    {
        path = "scenes/01-firelight.scene",
        title = "01   Firelight",
        blurb = "A stone hall lit by one brazier. Flickering point light with soft "
            .. "shadows from six pillars, moonlight baked into a lightmap, lit dust, "
            .. "embers and smoke, and a lantern on the mouse casting a second set of "
            .. "shadows. Keys 1-4 toggle each post effect to see what it adds.",
    },
    {
        path = "scenes/02-neon-rain.scene",
        title = "02   Neon Rain",
        blurb = "A wet street at night. Neon tubes brighter than white bloom into "
            .. "glass, the signs light the street in their colours, rain shows only "
            .. "where light falls on it, and the road reflects every light as a "
            .. "trembling streak. One sign is broken. T calls the lightning.",
    },
    {
        path = "scenes/03-crypt.scene",
        title = "03   Crypt",
        blurb = "A crypt where every surface is normal-mapped: brick courses, fluted "
            .. "columns, barrels, crates and chains, lit per pixel by four torches, candles and "
            .. "a wisp you carry on the mouse. N takes every normal map away and back "
            .. "to see what the normal maps add.",
    },
    {
        path = "scenes/04-stained-glass.scene",
        title = "04   Stained Glass",
        blurb = "A cathedral nave with the sun beyond its windows. Every pane is a "
            .. "tinted occluder, so the coloured bands on the floor are the sun's "
            .. "own light through the glass - they sweep over the pews as it moves, "
            .. "and the dust glitters in their colours. Left / Right steer the sun.",
    },
    {
        path = "scenes/05-nebula.scene",
        title = "05   Nebula",
        blurb = "Deep space. Clouds of glowing gas brighter than white, dark dust "
            .. "lanes cut through them with a multiply blend, two depths of stars, "
            .. "and a normal-mapped planet whose day side follows a moving star. "
            .. "Move the mouse to see every layer shift by its depth.",
    },
    {
        path = "scenes/06-shader-gallery.scene",
        title = "06   Shader Gallery",
        blurb = "Six effects, each a shader graph of nodes: a crate dissolving along "
            .. "noise with a burning edge, a flickering hologram, water with "
            .. "caustics, an energy shield, flowing lava, and heat haze bending "
            .. "the wall behind a fire.",
    },
    {
        path = "scenes/07-spellcraft.scene",
        title = "07   Spellcraft",
        blurb = "A mage in a dark courtyard where every light is a spell. Fireballs "
            .. "carry their own light past the wards' shadows, blasts bend the scene "
            .. "behind them, lightning forks, and a frost nova sweeps the floor. "
            .. "Click, right-click and F to cast.",
    },
    {
        path = "scenes/08-underwater.scene",
        title = "08   Underwater",
        blurb = "A reef floor under a sunlit surface. The caustics rippling over the "
            .. "sand are light, drawn by a shader graph, so they light the fish swimming "
            .. "through them; god rays sway, kelp bends, bubbles rise, and the whole "
            .. "view wobbles as if through water. The jellyfish follows the mouse.",
    },
    {
        path = "scenes/09-ten-thousand-lights.scene",
        title = "09   Ten Thousand Lights",
        blurb = "A lantern festival: ten thousand sky lanterns rising over a lake in "
            .. "three depths, every one an entity the engine moves itself. Three hundred "
            .. "carry real lights that catch the hills, and the lake mirrors it all. "
            .. "The status line reads the frame rate.",
    },    {
        path = "scenes/10-clay.scene",
        title = "10   Clay",
        blurb = "A white room of white primitives, seen from above: the look of a clay "
            .. "render, where shape comes only from light. Ambient occlusion darkens "
            .. "where things meet - wall and floor, two boxes set close - and a soft sun "
            .. "on the mouse casts shadows that end. A toggles the occlusion.",
    },
    {
        path = "scenes/11-trails.scene",
        title = "11   Trails",
        blurb = "A meteor shower over dark hills, and things that leave light behind "
            .. "them - every streak a trail component. The mouse leads a wisp that "
            .. "paints a ribbon; click and a blade slashes round it, lighting the hills. "
            .. "Fireballs carry lights, fireflies loop over the grass.",
    },
    {
        path = "scenes/12-pixel-perfect.scene",
        title = "12   Pixel Perfect",
        blurb = "A pixel-art cliffside at dawn, drawn as a 320x180 game would be and "
            .. "shown crisply at any size: nearest-neighbour textures, a frame "
            .. "point-sampled on the art's own grid (P turns it off), a waterfall that "
            .. "flows by palette cycling, and a CRT to lay over it all (C).",
    },
    {
        path = "scenes/13-day-night.scene",
        title = "13   Day & Night",
        blurb = "A small town from above through a whole day in a minute. One sun "
            .. "swings every shadow round and stretches them at dawn and dusk; the "
            .. "roofs catch it on one slope, then the other. At dusk the lamps come "
            .. "on, the moon rises, stars show in the pond and fog gathers.",
    },
    {
        path = "scenes/14-toon.scene",
        title = "14   Toon & Outlines",
        blurb = "A cel-shaded meadow of round creatures: light stepped into three crisp "
            .. "bands, an ink line round every shape drawn by a graph that samples its "
            .. "neighbours, and a cool rim light behind. Hover a creature to light its "
            .. "outline; T and O turn the toon lighting and the lines off.",
    },
    {
        path = "scenes/15-still-water.scene",
        title = "15   Still Water",
        blurb = "A mountain lake at dusk that mirrors everything above it as the camera "
            .. "drifts - each pixel reads the scene at its mirror image across the surface. "
            .. "Click to drop a stone and watch the ring bend the reflection; W raises "
            .. "the wind.",
    },
    {
        path = "scenes/16-festival-night.scene",
        title = "16   Festival Night",
        blurb = "Fireworks over a harbour town: bursts of hundreds of sparks that fly out "
            .. "and hang on air drag, willows that crackle as they fall, smoke that curls "
            .. "on a turbulence flow, and every flash lighting the town and mirrored in the "
            .. "harbour. Click to send a shell to the mouse; F for the finale.",
    },
    {
        path = "scenes/17-murmuration.scene",
        title = "17   Murmuration",
        blurb = "Thousands of starlings over winter fields at sunset, each steering by its "
            .. "nearest neighbours and flapping on its own beat, wheeling after a point that "
            .. "wanders the sky. The mouse is a falcon: the flock tears open around it. Up "
            .. "and Down change the flock, to twelve thousand.",
    },
    {
        path = "scenes/18-graveyard-watch.scene",
        title = "18   Graveyard Watch",
        blurb = "A chapel graveyard at night in 320x180 pixel art, lit as a palette-bound "
            .. "game had to be: four levels of light with every step between them dithered, "
            .. "on the art's own grid. Carry the lantern with the mouse; D compares the "
            .. "dither with hard bands and smooth light.",
    },
    {
        path = "scenes/19-god-rays.scene",
        title = "19   God Rays",
        blurb = "Morning sun low behind a forest canopy, streaming through every gap in "
            .. "the leaves in shafts that trunks and hanging clumps cut. Move the mouse and "
            .. "the sun moves - the beams swing after it. Dust hangs in the light; leaves "
            .. "drift through it.",
    },
    {
        path = "scenes/20-storm.scene",
        title = "20   Storm",
        blurb = "A village street at night in a storm. The rain lands - on the roofs, the "
            .. "walls, the cobbles - and splashes where it hits; lightning lights the whole "
            .. "street for a flicker; gusts swing the rain and flatten the grass.",
    },
    {
        path = "scenes/21-the-creature.scene",
        title = "21   The Creature",
        blurb = "A big cat crossing rolling ground. It walks into a run and back - two "
            .. "skeletal clips blended by pace at one phase - stops and starts with a "
            .. "crossfade, and its feet stand on the hills and rocks: foot IK plants each "
            .. "animated paw on the ground. One skinned mesh bends at every joint.",
    },
    {
        path = "scenes/22-menus-and-hud.scene",
        title = "22   Menus & HUD",
        blurb = "A game's front end: a title menu, an inventory and a HUD over the forest. "
            .. "Rounded cards and pill buttons with soft shadows, buttons that ease into "
            .. "their hover and grow, screens that fade and scale in and out, bars that "
            .. "ease, cooldowns that drain and a toast that slides in.",
    },
    {
        path = "scenes/23-tilt-shift-diorama.scene",
        title = "23   Tilt-Shift Diorama",
        blurb = "A small town from above that reads as a model of one: sharp along one "
            .. "street and blurring away above and below it. Cars round the blocks turning "
            .. "into every corner, boats drift up the river, chimneys smoke and cloud "
            .. "shadows slide over the roofs. The mouse moves the focus.",
    },
    {
        path = "scenes/24-collapse.scene",
        title = "24   Collapse",
        blurb = "A stone tower in a courtyard at night, and a boulder thrown at it. Blocks "
            .. "hit hard enough break into pieces of themselves and dust bursts where they "
            .. "break; every block and piece throws its shadow onto the wall from the "
            .. "braziers, so the tower's shadow falls as it falls.",
    },
    {
        path = "scenes/25-cloth-and-rope.scene",
        title = "25   Cloth & Rope",
        blurb = "A rope bridge over a gorge late in the day, a traveller crossing it, flags "
            .. "and banners in a gusting wind. The cloth is a grid of points the engine moves "
            .. "and writes into a mesh; the bridge is planks on joints the traveller weighs "
            .. "down, and the ropes ride it.",
    },
    {
        path = "scenes/26-pixel-perfect-ii.scene",
        title = "26   Pixel Perfect II",
        blurb = "Scene 12's cliffside drawn at the art's own resolution: the camera has a "
            .. "pixel size, so the world is drawn at 320x180 and scaled up, on the art's grid, "
            .. "and the pan stays smooth. M compares it with pixelate and with neither.",
    },
    {
        path = "scenes/27-two-views.scene",
        title = "27   Two Views",
        blurb = "The diorama town seen by four cameras at once: split screen following two cars, "
            .. "a minimap from far above, and a billboard by the road showing the river bridge. "
            .. "Each extra camera draws the whole lit, post-processed world into a texture.",
    },
    {
        path = "scenes/28-through-the-window.scene",
        title = "28   Through the Window",
        blurb = "A ruined wall at night with an arched window onto a golden afternoon, and a "
            .. "portal you carry with the mouse onto a nebula. One arch-shaped mask cuts the "
            .. "hole in the wall and shows the day through it; the portal's soft-edged mask "
            .. "looks into a layer of the world drawn only there.",
    },
    {
        path = "scenes/29-atlas-of-the-deep.scene",
        title = "29   Atlas of the Deep",
        blurb = "A sea chart you zoom from the whole ocean down to one harbour. Every name is "
            .. "world text in the built-in font, a distance field now, so it is sharp at nine "
            .. "times its size and turned; the names come and go by zoom.",
    },
    {
        path = "scenes/30-spell-storm.scene",
        title = "30   Spell Storm",
        blurb = "Spellcraft's mage at the heart of two hundred thousand sparks: a vortex on "
            .. "the floor, a column off the rune ring, a knot at the staff. Every spark is "
            .. "computed in the vertex shader from its index and the time - three draw calls.",
    },
    {
        path = "scenes/31-the-storyteller.scene",
        title = "31   The Storyteller",
        blurb = "A storybook by candlelight, its fable typed out a letter at a time. Every page is "
            .. "one rich text: a place in blue, a name in bold, a key drawn in the line, reeds that "
            .. "sing in a wave, a crack that shakes, a hoard in every colour, and pauses where a "
            .. "reader would breathe.",
    },
    {
        path = "scenes/32-mapmaker.scene",
        title = "32   Mapmaker",
        blurb = "An island you paint. Every cell is only a terrain number - sea, sand, grass or "
            .. "rock - and the coasts, beaches and cliff edges between them are drawn by the "
            .. "engine's autotiling, a tile per corner, the frame a cell changes.",
    },
    {
        path = "scenes/33-hillside-hamlet.scene",
        title = "33   Hillside Hamlet",
        blurb = "An isometric village on a hill, built of tiles: its houses, trees and hill blocks "
            .. "are sorted with the villagers, who walk behind the cottages on the back path and in "
            .. "front of them on the lane. The mouse picks a diamond; a click plants a tree.",
    },
    {
        path = "scenes/34-crater-isle.scene",
        title = "34   Crater Isle",
        blurb = "A cartoon island you bomb a crater at a time. Every blast cuts a hole in the "
            .. "island's own pixels and its collider is rebuilt from what is left, so the crates "
            .. "and barrels standing on the carved-away ground tumble in.",
    },
    {
        path = "scenes/35-neon-signs.scene",
        title = "35   Neon Signs",
        blurb = "A night-market wall of shop signs where every word is world text: neon tubes "
            .. "with outlines in their own colour and a soft glow behind, a chalk menu with a drop "
            .. "shadow, a cartoon sticker with a fat outline - and captions that need no strip.",
    },
    {
        path = "scenes/36-rolling-hills.scene",
        title = "36   Rolling Hills",
        blurb = "A wheel rolling over hills drawn from a handful of points: each curve is filled "
            .. "with tiled soil, edged with a strip of grass that follows it, and made solid in the "
            .. "same shape - the hills, a floating island and the far range alike.",
    },
    {
        path = "scenes/37-harbour-at-night.scene",
        title = "37   Harbour at Night",
        blurb = "Boats and crates floating in a harbour whose water is a box with buoyancy and "
            .. "drag, riding the tide as it rises; a fan's wind that pushes the boats and bends "
            .. "the chimney smoke, and a whirlpool that pulls - no script applies a force.",
    },
    {
        path = "scenes/38-hex-strategy-board.scene",
        title = "38   Hex Strategy Board",
        blurb = "A turn of a hex wargame on a hexagonal tilemap: where a piece can go, the path it "
            .. "takes and who it can strike are counted in hexes by the engine, and red's pieces "
            .. "march across forest, hills and fords to meet yours.",
    },
    {
        path = "scenes/39-lantern-orchard.scene",
        title = "39   Lantern Orchard",
        blurb = "A top-down orchard at night under the lantern you carry: every tree, post, bale "
            .. "and the scarecrow casts the shape of its own sprite, so light falls between a "
            .. "crown's clumps and between the fence's posts.",
    },
    {
        path = "scenes/40-moonlit-study.scene",
        title = "40   Moonlit Study",
        blurb = "A study at night lit through light cookies: the moon through a window's panes, "
            .. "a rose window's colours, a desk lamp's shade, and a ceiling fan whose turning "
            .. "blades sweep shadows round the room.",
    },
    {
        path = "scenes/41-waterfall-grotto.scene",
        title = "41   Waterfall Grotto",
        blurb = "A cave built of tiles that move: the waterfall falls, the pool ripples, torch "
            .. "flames flicker, crystals pulse and moss sways - every one an animated tile, "
            .. "lit by the torches and the daylight through the gap above.",
    },
    {
        path = "scenes/42-siege-blast.scene",
        title = "42   Siege Blast",
        blurb = "Towers of stone and timber as physics bodies: the blocks a blast would reach "
            .. "glow under the mouse - asked of the colliders, so a long beam is caught by its "
            .. "end - and a cannon marks where its shot would first strike.",
    },
    {
        path = "scenes/43-night-market.scene",
        title = "43   Night Market",
        blurb = "A lantern-lit market you walk through and listen to: the lute player, the grill, "
            .. "the fountain and the chimes each swell as you pass and sit to your left or right, "
            .. "and a coin you toss is heard where it lands.",
    },
    {
        path = "scenes/44-garden-party.scene",
        title = "44   Garden Party",
        blurb = "Forty guests mingle in a formal garden at golden hour, each finding its own way "
            .. "round the hedges, the beds, the tables and the fountain and stepping round the others; "
            .. "call them and they gather for a toast, shut the gate and they go the long way.",
    },
    {
        path = "scenes/45-polyglot-inn.scene",
        title = "45   The Polyglot Inn",
        blurb = "An inn's wall at night in six languages - the sign, the menu and its prices, a notice "
            .. "whose counts take each language's plural - every word from one string table.",
    },
    {
        path = "scenes/46-sea-cove.scene",
        title = "46   Sea Cove",
        blurb = "Swim a cove you can hear: surf, gulls and music in the open, the drips ringing once you "
            .. "are in the sea cave, and everything going dull the moment you dive.",
    },
    {
        path = "scenes/47-autumn-run.scene",
        title = "47   Autumn Run",
        blurb = "A fox runs and jumps across autumn hills, and the camera is the subject: a dead zone "
            .. "that holds still through jumps, look-ahead that leads the way you run, keys 1-4 to compare.",
    },
    {
        path = "scenes/48-the-forge.scene",
        title = "48   The Forge",
        blurb = "A smithy at night you feel as well as see: every blow throws sparks, flashes and kicks the "
            .. "pad's strong motor (or buzzes a phone), harder the hotter the iron; the bellows hum, a quench hisses.",
    },
    {
        path = "scenes/49-stage-night.scene",
        title = "49   Stage Night",
        blurb = "A band on a dark stage under sweeping beams - each a freeform light, a soft trapezoid "
            .. "swung from its lamp - lighting the haze and the boards, in cues on the beat.",
    },
    {
        path = "scenes/50-jellyfish-bloom.scene",
        title = "50   Jellyfish Bloom",
        blurb = "Jellyfish rising through deep water in pulses, every tentacle, arm and kelp stalk a chain "
            .. "of jiggle bones that trails, streams and whips after what the script moves.",
    },
    {
        path = "scenes/51-night-bloom.scene",
        title = "51   Night Bloom",
        blurb = "A meadow at night whose flowers open and glow as they come into view and close as they "
            .. "leave - each a visibility notifier - with fireflies held still off screen.",
    },
    {
        path = "scenes/52-model-railway.scene",
        title = "52   Model Railway",
        blurb = "A tilt-shifted model railway board: a train, cars keeping left both ways round and a ferry, "
            .. "each riding a drawn curve with path_follow.",
    },
    {
        path = "scenes/53-village-square.scene",
        title = "53   Village Square",
        blurb = "A crowded square at dusk: every villager is eight cut-out parts and one sorting group, "
            .. "so each passes in front of and behind the others, the well and the lamps whole.",
    },
    {
        path = "scenes/54-tesla-lab.scene",
        title = "54   Tesla Lab",
        blurb = "A dark laboratory lit by its own lines: lightning between two coils, a laser bouncing "
            .. "off turning mirrors, cables and hazard tape - each a line, reshaped every frame.",
    },
    {
        path = "scenes/55-dragons-hoard.scene",
        title = "55   Dragon's Hoard",
        blurb = "Treasure in a dark vault and a lantern you carry over it: coins, a sword, a goblet, shields "
            .. "and gems glint with specular highlights that follow the light.",
    },
    {
        path = "scenes/56-haunted-hall.scene",
        title = "56   Haunted Hall",
        blurb = "Ghosts drifting through a candle-lit hall, fading in and out of being - each a figure of "
            .. "parts drawn and faded as one picture, a canvas group.",
    },
    {
        path = "scenes/57-picture-house.scene",
        title = "57   Picture House",
        blurb = "A film in a dark cinema: a video playing into a texture, shown on the screen and on the "
            .. "projectionist's monitor, lighting the audience through a dusty projector beam.",
    },
    {
        path = "scenes/58-lamplit-mine.scene",
        title = "58   Lamplit Mine",
        blurb = "A mine in lantern light, all one tilemap: its rock casts shadows, its slopes and planks "
            .. "collide as their shapes, a crystal vein lets a buried light out, and broken rock lets light in.",
    },
    {
        path = "scenes/59-windswept-meadow.scene",
        title = "59   Windswept Meadow",
        blurb = "Seven hundred blades of grass and flowers bending in waves as gusts roll across a summer "
            .. "field, and a banner rippling on the hill - every vertex moved by its material, no script.",
    },
    {
        path = "scenes/60-moonlit-duel.scene",
        title = "60   Moonlit Duel",
        blurb = "Two duellists on a cliff under a great moon, each in a coloured key light that does not "
            .. "touch the sky, black against the lightning - light layers. L turns them off.",
    },
    {
        path = "scenes/61-firefly-glade.scene",
        title = "61   Firefly Glade",
        blurb = "Fireflies drifting over a pond at dusk, each drawing its loops and curls in light behind it, "
            .. "a sparkler on the mouse and shooting stars - particle trails.",
    },
    {
        path = "scenes/62-phoenix.scene",
        title = "62   Phoenix",
        blurb = "A firebird made of fire: flame pours off its outline and embers off its body - emitters "
            .. "shaped by its own sprite - inside a ring that sparks from every point of its curve.",
    },
    {
        path = "scenes/63-turning-seasons.scene",
        title = "63   Turning Seasons",
        blurb = "The meadow through the year - summer, autumn, winter, dusk - without one sprite changing: "
            .. "four colour look-up strips graded over the whole frame, and film grain.",
    },
    {
        path = "scenes/64-storming-the-gate.scene",
        title = "64   Storming the Gate",
        blurb = "A catapult against a castle: every knight a skeleton that goes limp as a ragdoll when a stone "
            .. "reaches it, and a rope bridge whose hinges snap past their break force.",
    },
    {
        path = "scenes/65-two-views-of-the-railway.scene",
        title = "65   Two Views of the Railway",
        blurb = "The model railway as a split screen: one half rides with the train, the other with a car, "
            .. "each turned so it points up, and the whole board inset - three cameras, one field each.",
    },
    {
        path = "scenes/66-puppet-theatre.scene",
        title = "66   Puppet Theatre",
        blurb = "A toy theatre's whole play - curtains, card actors, sun and moon, lights, captions, thunder "
            .. "and applause - as one keyframe timeline, its event keys calling the script.",
    },
    {
        path = "scenes/67-signs-of-the-silk-road.scene",
        title = "67   Signs of the Silk Road",
        blurb = "A caravanserai at dusk hung with signs in Japanese, Russian, Chinese, Greek and Korean - "
            .. "each word drawn straight from a .ttf, its glyphs baked as they are first drawn.",
    },
    {
        path = "scenes/68-moon-lander.scene",
        title = "68   Moon Lander",
        blurb = "A lunar module lands itself on the pad from a new start each time - its autopilot is "
            .. "fixed_update, stepping with the physics, so it flies the same at any frame rate.",
    },
    {
        path = "scenes/69-the-kraken.scene",
        title = "69   The Kraken",
        blurb = "Tentacles rise out of a storm to coil about a rolling ship's masts - each nine bones "
            .. "bent by one ik_chain (FABRIK) so its tip reaches the point the script sweeps.",
    },
    {
        path = "scenes/70-eight-ways.scene",
        title = "70   Eight Ways",
        blurb = "A ranger aims at the targets while it walks and runs every way - its feet step from "
            .. "a 2D blend space of nine clips, steered by its velocity in the body's own frame.",
    },
    {
        path = "scenes/71-the-quest-journal.scene",
        title = "71   The Quest Journal",
        blurb = "A journal by candlelight: a quest log in coloured, shaking and shimmering UI text with icons "
            .. "among the words, a .gui footer, and letters that write themselves out.",
    },
    {
        path = "scenes/72-scrap-yard.scene",
        title = "72   Scrap Yard",
        blurb = "A crane drops junk onto a heap that settles by its real shapes - every piece's collider "
            .. "is the traced outline of its sprite (sprite_collider). F4 draws them.",
    },
    {
        path = "scenes/73-spirograph.scene",
        title = "73   Spirograph",
        blurb = "Thin wire figures turning on dark paper, smooth at every angle - the fxaa post effect "
            .. "blends the stairs a turned edge makes. F switches it to compare.",
    },
    {
        path = "scenes/74-baggage-claim.scene",
        title = "74   Baggage Claim",
        blurb = "Bags ride the belts and tumble off the end - each belt a collider with a conveyor, "
            .. "carrying what rests on it by friction while it never moves.",
    },
    {
        path = "scenes/75-the-controls-room.scene",
        title = "75   The Controls Room",
        blurb = "An arcade game's settings screen under a CRT: rebind every action with the next key "
            .. "you press, fly the ship by the new bindings, and find them still there after a restart.",
    },
    {
        path = "scenes/76-monster-zoo.scene",
        title = "76   Monster Zoo",
        blurb = "Four pens of one creature - a prefab, its variants (a prefab that extends a prefab) "
            .. "and a variant of a variant - each pen's beasts scene instances with their own place.",
    },
    {
        path = "scenes/77-the-heist.scene",
        title = "77   The Heist",
        blurb = "A museum through its security cameras: each room's camera confined to its walls, "
            .. "handing the screen on by priority and blending across as the thief walks through.",
    },
    {
        path = "scenes/78-echo-canyon.scene",
        title = "78   Echo Canyon",
        blurb = "A canyon that echoes a hiker's call, a camp radio made a crackling AM set by its bus's EQ, "
            .. "distortion and compressor, and footsteps that never sound the same twice.",
    },
    {
        path = "scenes/79-paper-dolls.scene",
        title = "79   Paper Dolls",
        blurb = "A paper doll changing clothes: a sprite library of four outfits on her root, a resolver on each "
            .. "part, and one call to swap a whole outfit or a single part.",
    },
    {
        path = "scenes/80-treasure-isle.scene",
        title = "80   Treasure Isle",
        blurb = "An island in one tilemap: grass, sand and lava drawn as weighted random variants, tiles that tell "
            .. "the explorer what they are (and that lava burns), and chest tiles that load as real chests.",
    },
    {
        path = "scenes/81-sandstorm.scene",
        title = "81   Sandstorm",
        blurb = "Two hundred thousand GPU grains on the wind, parting round the rocks they meet and drawn "
            .. "up and spun by a wandering whirlwind - with no state per grain.",
    },
    {
        path = "scenes/82-stained-light.scene",
        title = "82   Stained Light",
        blurb = "A chapel where the sun through coloured glass lands on the floor and bounces - radiance "
            .. "cascades, global illumination - tinting the wall and the pillars above the patches.",
    },
    {
        path = "scenes/83-siege-workshop.scene",
        title = "83   Siege Workshop",
        blurb = "A siege yard where every engine is one prefab built of parts - wheels that are prefabs "
            .. "themselves, a catapult's arm, bucket and stone - rolled out of the gate by the dozen.",
    },
    {
        path = "scenes/84-options-menu.scene",
        title = "84   Options Menu",
        blurb = "A game's options screen over a lighthouse at dusk: window mode, resolution, vsync, a frame "
            .. "cap and the cursor, every row live, read back from the window and saved for next time.",
    },
    {
        path = "scenes/85-guest-book.scene",
        title = "85   Guest Book",
        blurb = "An inn's guest book by candlelight: write your name, home and a word for the house straight "
            .. "onto the page - a real text field, caret, selection, clipboard and IME - and sign.",
    },
    {
        path = "scenes/86-the-long-corridor.scene",
        title = "86   The Long Corridor",
        blurb = "An endless corridor of halls, three at a time: each hall its own scene, streamed in ahead "
            .. "of the traveller with its texture decoded in the background, and dropped behind.",
    },
    {
        path = "scenes/87-button-prompts.scene",
        title = "87   Button Prompts",
        blurb = "A dojo's first lesson, its card naming the buttons in your hand - keyboard, Xbox, PlayStation "
            .. "or Switch - and three moves: a tap, a hold that charges in tiers, and a double tap.",
    },
    {
        path = "scenes/88-night-drive.scene",
        title = "88   Night Drive",
        blurb = "A synthwave coast road at night: the score's four stems fade in as the car speeds up, the "
            .. "engine climbs through its gears, and the radio crossfades station to station.",
    },
    {
        path = "scenes/89-archers-range.scene",
        title = "89   Archer's Range",
        blurb = "An archer shoots the targets coming by without breaking stride: the legs walk, and the "
            .. "bow is a layer masked to the upper body, drawn and loosed over the walk.",
    },
    {
        path = "scenes/90-fireworks-night.scene",
        title = "90   Fireworks Night",
        blurb = "Fireworks over a harbour town, all particles playing flipbooks: a starburst that blooms over "
            .. "its life, glitter twinkling from frames of its own, smoke that billows and drifts.",
    },
    {
        path = "scenes/91-procedural-isles.scene",
        title = "91   Procedural Isles",
        blurb = "An archipelago drawn from noise and redrawn on R: cellular noise places the islands, fbm "
            .. "roughens their coasts, clouds grow and melt in 3D Perlin and the wind follows a flow field.",
    },
    {
        path = "scenes/92-crane-yard.scene",
        title = "92   Crane Yard",
        blurb = "A crane truck of joints: sprung wheels that bob over bollards, a trolley along the boom, a hook "
            .. "whose swing friction damps, a magnet that holds a crate softly, and crates dragged by the mouse.",
    },
    {
        path = "scenes/93-ghost-lantern.scene",
        title = "93   Ghost Lantern",
        blurb = "A haunted gallery by candlelight: ghosts drawn as one picture each through a glow material, and "
            .. "a cursed lantern following the mouse whose light takes light away.",
    },
    {
        path = "scenes/94-warehouse-robots.scene",
        title = "94   Warehouse Robots",
        blurb = "Robots shuttle parcels round racks that are a tilemap, replanning when a gate swings or a crate "
            .. "you drag with the mouse blocks their aisle.",
    },
    {
        path = "scenes/95-equaliser.scene",
        title = "95   Equaliser",
        blurb = "A club driven by its own music: the LED wall is the mix's spectrum, the lights follow the lows, "
            .. "mids and highs, the crowd moves on the beat, and the keys play a synth.",
    },
    {
        path = "scenes/96-photo-mode.scene",
        title = "96   Photo Mode",
        blurb = "A scenic overlook at sunset with a photo mode: the world freezes, you frame and zoom, and the "
            .. "shot is copied into a gallery (and natively saved as a PNG).",
    },
    {
        path = "scenes/97-bazaar-signs.scene",
        title = "97   Bazaar Signs",
        blurb = "A market street at dusk whose signs are in Arabic, Hebrew, Hindi and Thai - shaped, joined and "
            .. "set right to left where they should be - swaying on their chains.",
    },
    {
        path = "scenes/98-readable-menus.scene",
        title = "98   Readable Menus",
        blurb = "A title screen a screen reader can read and work - every button, slider and toggle named, "
            .. "decoration skipped, presses announced; the pill prints what the reader says.",
    },
    {
        path = "scenes/99-night-watch.scene",
        title = "99   Night Watch",
        blurb = "Guards with lanterns patrol a moonlit courtyard and a thief works its chests - every one a "
            .. "behaviour tree: chase what the lantern shows, search where a chest was robbed, flee, sneak.",
    },
    {
        path = "scenes/100-ember-cavern.scene",
        title = "100  Ember Cavern",
        blurb = "A cave lit by what floats in it: embers off a lava river, bubbles bursting into sparks, "
            .. "drifting spores - every particle a light that rakes the rock as it rises and fades.",
    },
    {
        path = "scenes/101-lit-house.scene",
        title = "101  The Lit House",
        blurb = "A cottage at night whose blinking lamp lights the grass only through its windows and door; "
            .. "a walker with a torch goes in - the roof lifts and the world outside goes out of focus.",
    },
    {
        path = "scenes/102-firefight.scene",
        title = "102  Firefight",
        blurb = "Two squads across a burning lot: tracers and laser beams, sparks that bounce off the cover, "
            .. "smoke, a barrel that goes up - and every drop of blood and scorch painted into the asphalt.",
    },
    {
        path = "scenes/103-snowfield.scene",
        title = "103  Snowfield",
        blurb = "Wolves, a dog sled and a walker with a lantern cross a moonlit snowfield, pressing every print "
            .. "into the snow's normal map, so the low moon catches each rim and the lantern makes the snow sparkle.",
    },
    {
        path = "scenes/104-wildfire.scene",
        title = "104  Wildfire",
        blurb = "Lightning starts a fire in a dry meadow; the wind drives it, a firebreak turns it, the river stops it "
            .. "and a water bomber makes passes - flames, embers and smoke start from canvases the fire is painted into.",
    },
    {
        path = "scenes/105-graffiti-alley.scene",
        title = "105  Graffiti Alley",
        blurb = "Taggers spray bubble letters into a brick wall on a rainy night - outline, fill, highlights, drips - "
            .. "and the fresh paint shines wet under the lamp and passing headlights, then dries matt.",
    },
    {
        path = "scenes/106-foundry.scene",
        title = "106  The Foundry",
        blurb = "A crucible on a crane tips molten metal into three sand moulds - a liquid that pours, splashes, "
            .. "levels out and cools from white through orange to the grey of a casting, lighting the shop as it goes.",
    },
    {
        path = "scenes/107-night-pursuit.scene",
        title = "107  Night Pursuit",
        blurb = "A getaway car tears down a wet city street with the police and a helicopter on it; the street streams "
            .. "past in motion blur - each thing smeared by its own motion on screen, the car the camera follows sharp.",
    },
    {
        path = "scenes/108-orbital-station.scene",
        title = "108  Orbital Station",
        blurb = "Raiders make passes at a shielded station over a planet; every bolt ripples the shield where it lands, "
            .. "defenders answer with lasers and smoking missiles, and the low sun and every blast flare the lens.",
    },
    {
        path = "scenes/109-mirror-maze.scene",
        title = "109  Mirror Maze",
        blurb = "A carnival hall of mirrors from above: four mirror walls show the room again on every side and each "
            .. "other's reflections in the corners, a laser bounces round them, and a portal in the floor looks into a garden.",
    },
    {
        path = "scenes/110-harbour-storm.scene",
        title = "110  Harbour Storm",
        blurb = "A harbour in a storm at night: the sea's surface swells, chops and splashes, the moored boats ride its waves "
            .. "and strain at their ropes, a lighthouse sweeps its beam through the rain and lightning strikes the sea.",
    },
    {
        path = "scenes/111-arena-boss.scene",
        title = "111  Arena Boss",
        blurb = "A stone golem against a knight in a ruined arena: every slam, sweep and rock fall is told by a shape on the "
            .. "floor that fills, the floor is carved where they land, and the golem falls limp into its own craters.",
    },
    {
        path = "scenes/112-haunted-manor.scene",
        title = "112  Haunted Manor",
        blurb = "A torch-lit walk through a manor streamed in room by room: each room turns the grade as you come in "
            .. "and brings its own music, and its ghosts take the light away where they drift.",
    },
    {
        path = "scenes/113-falling-sands.scene",
        title = "113  Falling Sands",
        blurb = "A cave in cross-section where every grain is its own: sand pours and piles, water levels, oil burns "
            .. "to smoke, lava boils the pool to steam and cools to stone, and crates ride the dunes down.",
    },
    {
        path = "scenes/114-smoke-chamber.scene",
        title = "114  Smoke Chamber",
        blurb = "A chamber full of air that moves: smoke and steam rise, curl round the pillars and pool under the "
            .. "vault, a swinging weight carves a wake through them, and a fan blows the layer across the room.",
    },
    {
        path = "scenes/115-rewind-ruins.scene",
        title = "115  Rewind Ruins",
        blurb = "A boulder brings a hilltop temple down, then time runs back: the blocks fly up into place, the fire "
            .. "goes out and the pilgrim walks backwards - and the world goes on again from there.",
    },
    {
        path = "scenes/116-muffled-halls.scene",
        title = "116  Muffled Halls",
        blurb = "A knight walks a castle: the minstrel's lute rings in the great hall, goes dull behind a wall and "
            .. "faint behind two, and each room has the reverb its walls give it.",
    },
    {
        path = "scenes/117-moor-at-midnight.scene",
        title = "117  Moor at Midnight",
        blurb = "A traveller crosses a fog-bound moor: her lantern glows in the mist past its own reach, the standing "
            .. "stones' shadows are dark shafts through it, and wisps drift green over the heather.",
    },
    {
        path = "scenes/118-the-endless-dungeon.scene",
        title = "118  The Endless Dungeon",
        blurb = "A dungeon decided a few seconds ahead of the explorer walking it: every tile fits its neighbours, "
            .. "every new stretch fits the last, and where it cuts her off she breaks through.",
    },
    {
        path = "scenes/119-eyes-in-the-dark.scene",
        title = "119  Eyes in the Dark",
        blurb = "A vault at night: each guard sees exactly what his lantern lights, cut by the cases and pillars, "
            .. "and a thief creeps through the shadows between - until a cone finds her.",
    },
    {
        path = "scenes/120-a-day-on-the-island.scene",
        title = "120  A Day on the Island",
        blurb = "A whole day in a minute: the sky turns from night through a pink dawn to noon and a red dusk, "
            .. "the sun's light and the palms' shadows wheel over, the stars come out and the fisher lights his lamp.",
    },
    {
        path = "scenes/121-the-endless-road.scene",
        title = "121  The Endless Road",
        blurb = "A little red car drives on forever through a countryside built ahead of it, into the night - "
            .. "and every few seconds the whole world is moved back under it, which nothing on screen shows.",
    },
    {
        path = "scenes/122-bullet-time.scene",
        title = "122  Bullet Time",
        blurb = "A gunfight that slows to an eighth: bullets crawl and ripple the air, the music bends down, "
            .. "and the hero runs on through it at full speed - his own clock ignores the world's.",
    },
    {
        path = "scenes/123-sundial-plaza.scene",
        title = "123  Sundial Plaza",
        blurb = "A town square through a day: the obelisk's shadow walks round the hour stones, the townsfolk's "
            .. "lie at their feet, long at dawn and dusk, and at night the lamp posts throw them two ways at once.",
    },
    {
        path = "scenes/124-paper-theatre.scene",
        title = "124  The Paper Theatre",
        blurb = "A toy theatre of flat paper cut-outs lit by a candle carried across them - and every rim catches "
            .. "the light, rounded from its own silhouette with no normal map painted.",
    },
    {
        path = "scenes/125-torchlit-depths.scene",
        title = "125  Torchlit Depths",
        blurb = "A dug-out cave lit by eighty-odd torches, glowing mushrooms, crystals and two lava lakes - "
            .. "all one light flooded through the tiles, round every corner and dying in the rock.",
    },
    {
        path = "scenes/126-billiard-hall.scene",
        title = "126  Billiard Hall",
        blurb = "A pool table under one lamp: before every shot a dotted line shows where the cue ball "
            .. "will go and the ball it will send - the physics run ahead on a copy of the table.",
    },
    {
        path = "scenes/127-glass-house.scene",
        title = "127  The Glass House",
        blurb = "A glasshouse at dusk with stones lobbed at it: every pane they strike crazes small round "
            .. "the impact and falls in long shards - Voronoi fracture.",
    },
    {
        path = "scenes/128-quicksave-workshop.scene",
        title = "128  Quicksave Workshop",
        blurb = "A toymaker's bench in motion - a pendulum, dominoes, marbles down ramps - quicksaved partway "
            .. "and loaded back: every piece jumps back as it was, spinning as it was, and the run plays again.",
    },
    {
        path = "scenes/129-carnival-banners.scene",
        title = "129  Carnival Banners",
        blurb = "A fairground at dusk: a name lettered round the gateway's arch, words riding swinging pennant "
            .. "strings, a marquee scrolling round the carousel - text laid along curves.",
    },
    {
        path = "scenes/130-photo-album.scene",
        title = "130  Photo Album",
        blurb = "A table of holiday snapshots played with the fingers - swipe to deal, drag, pinch, twist, hold "
            .. "to lift, double tap to look closer - every move read from the engine's gestures.",
    },
    {
        path = "scenes/131-rack-focus.scene",
        title = "131  Rack Focus",
        blurb = "A misty forest at dawn in layers, and a lens pulling focus from the ferns to the deer to the "
            .. "pines to the far hills - depth of field by each layer's z.",
    },
    {
        path = "scenes/132-beat-hall.scene",
        title = "132  Beat Hall",
        blurb = "A club whose floor, speakers, beams and crowd hit exactly on the beat of its music and change "
            .. "pattern on every bar - a beat clock following the music's own playhead.",
    },
    {
        path = "scenes/133-ember-storm.scene",
        title = "133  Ember Storm",
        blurb = "A brazier cart's sparks trail behind it, bounce down steps and off a roof, settle on the cobbles "
            .. "and are swept by a swinging iron ball - GPU particles that keep state.",
    },
    {
        path = "scenes/134-meadow-trails.scene",
        title = "134  Meadow Trails",
        blurb = "A meadow where the grass, reeds and flowers part round a running fox - and the mouse - and "
            .. "spring back behind it: a field things move through.",
    },
    {
        path = "scenes/135-smooth-caves.scene",
        title = "135  Smooth Caves",
        blurb = "A cave of smooth rock, dug and built with the mouse while boulders roll down its slopes - "
            .. "marching-squares terrain with a collider rebuilt as it changes.",
    },
    {
        path = "scenes/136-fog-of-war.scene",
        title = "136  Fog of War",
        blurb = "A campaign map dark but for what the scouts see - explored land dimmed, and the enemy's camps "
            .. "drawn only while someone is looking at them.",
    },
    {
        path = "scenes/137-village-chatter.scene",
        title = "137  Village Chatter",
        blurb = "Villagers who talk - portraits, lines that type themselves out, and choices that change "
            .. "what happens next: conversations as data tables, played into the scene's own text.",
    },
    {
        path = "scenes/138-loot-satchel.scene",
        title = "138  Loot Satchel",
        blurb = "An adventurer's kit: items dragged between two bags and onto an equipment board - a ghost "
            .. "follows the pointer, potions and coins stack, the kit refuses what does not fit.",
    },
    {
        path = "scenes/139-toy-town.scene",
        title = "139  Toy Town",
        blurb = "A little town of voxel houses, trees and carts drawn as stacked slices - the camera turns "
            .. "over the board and every model turns with it, still standing up.",
    },
    {
        path = "scenes/140-kart-circuit.scene",
        title = "140  Kart Circuit",
        blurb = "Karts racing round a circuit seen from behind, the ground sweeping toward the horizon in "
            .. "perspective - a Mode 7 plane, and karts and trees scaled by their distance.",
    },
    {
        path = "scenes/141-pocket-palette.scene",
        title = "141  Pocket Palette",
        blurb = "A sunset bay through classic palettes - PICO-8, Game Boy, CGA, EGA - quantised with ordered "
            .. "dithering, the sea's colours cycling so the water flows without a redrawn pixel.",
    },
    {
        path = "scenes/142-behind-the-walls.scene",
        title = "142  Behind the Walls",
        blurb = "A thief creeping through a manor at night, drawn as a bright outline wherever the roofs "
            .. "and trees hide him - occlusion silhouettes, clipped to the walls' own pixels.",
    },
    {
        path = "scenes/143-card-table.scene",
        title = "143  Card Table",
        blurb = "A hand of cards that lean toward the pointer and turn over in true perspective, "
            .. "showing their backs - sprites tilted about their own axes.",
    },
    {
        path = "scenes/144-dash-echoes.scene",
        title = "144  Dash Echoes",
        blurb = "Ninjas dashing across the rooftops at night, leaving fading copies of each pose behind - "
            .. "afterimages, left only while they dash.",
    },
    {
        path = "scenes/145-oil-and-ink.scene",
        title = "145  Oil and Ink",
        blurb = "One harbour as a photograph, an oil painting, a comic-book page and newsprint - "
            .. "a Kuwahara filter and halftone screens, in the output pass.",
    },
    {
        path = "scenes/146-curtain-call.scene",
        title = "146  Curtain Call",
        blurb = "A toy theatre's four acts, each its own scene, each handing over to the next - "
            .. "the old frame kept and wiped away by an iris, diamonds, a slide or a dissolve.",
    },
    {
        path = "scenes/147-fruit-slicer.scene",
        title = "147  Fruit Slicer",
        blurb = "Fruit tossed up and cut in two by the swipe of a blade, the halves tumbling apart "
            .. "showing their insides - bodies sliced along a line.",
    },
    {
        path = "scenes/148-portal-lab.scene",
        title = "148  Portal Lab",
        blurb = "A test chamber with a linked pair of portals, each showing the view out of the other - "
            .. "crates dropped into the floor fly out of the wall.",
    },
    {
        path = "scenes/149-jelly-bounce.scene",
        title = "149  Jelly Bounce",
        blurb = "Jelly blocks that squash when they land, wobble as they fly and quiver when poked - "
            .. "sprites drawn as a grid of points on springs.",
    },
    {
        path = "scenes/150-beacon-run.scene",
        title = "150  Beacon Run",
        blurb = "A scout crossing a wide moor to light its beacons, gold arrows at the screen's edge "
            .. "pointing to those out of sight with their distance - off-screen markers.",
    },
    {
        path = "scenes/151-sketchbook.scene",
        title = "151  Sketchbook",
        blurb = "A village drawn in pencil whose lines boil, redrawn with a small wobble a few times a second - "
            .. "a post effect over the whole frame.",
    },
    {
        path = "scenes/152-spell-wheel.scene",
        title = "152  Spell Wheel",
        blurb = "A wizard choosing spells from a radial wheel with the mouse or a stick, the section "
            .. "pointed at growing and lighting up - a radial menu.",
    },
    {
        path = "scenes/153-rainy-window.scene",
        title = "153  Rainy Window",
        blurb = "A city at night seen through a wet window, drops forming, running down and bending the "
            .. "neon behind them - rain on the lens, a post effect.",
    },
    {
        path = "scenes/154-talk-of-the-town.scene",
        title = "154  Talk of the Town",
        blurb = "A market square full of chatter, every bubble sized to its words, its tail on the speaker, "
            .. "kept on screen and clear of the others.",
    },
    {
        path = "scenes/155-unit-select.scene",
        title = "155  Unit Select",
        blurb = "Soldiers on a meadow picked by dragging a box, each ringed in a pulsing glow that follows "
            .. "his own shape - shield, spear and bow.",
    },
    {
        path = "scenes/156-thunderhead.scene",
        title = "156  Thunderhead",
        blurb = "A storm over a lighthouse at sea, branching lightning that is never the same twice lighting "
            .. "the waves, the tower and the cloud for an instant.",
    },
    {
        path = "scenes/157-millrace.scene",
        title = "157  Millrace",
        blurb = "A mill race from above, water running round its bends past a turning millwheel, carrying "
            .. "leaves and a paper boat with it.",
    },
    {
        path = "scenes/158-first-snow.scene",
        title = "158  First Snow",
        blurb = "A village lane at dusk as the first snow falls, settling on every roof, sill, rail and bough "
            .. "- deep on the flat, thin on the steep - and melting again.",
    },
    {
        path = "scenes/159-rooftop-runners.scene",
        title = "159  Rooftop Runners",
        blurb = "Couriers running a city's rooftops, finding their own way across - jumping the alleys, a plank, "
            .. "a footbridge over the street and a zipline down across it.",
    },
    {
        path = "scenes/160-shape-shifter.scene",
        title = "160  Shape Shifter",
        blurb = "One glowing shape turning into the next - circle, star, heart, bird, crescent, drop, hexagon - "
            .. "its outline and colour eased, never twisting.",
    },
    {
        path = "scenes/161-trophy-room.scene",
        title = "161  Trophy Room",
        blurb = "Shelves of trophies to unlock, each achievement sliding in as a toast from the corner, everyday "
            .. "notices stacking up in another.",
    },
    {
        path = "scenes/162-instant-replay.scene",
        title = "162  Instant Replay",
        blurb = "A pinball table built from joints and kicks: R records a run from a fresh table, P plays it back "
            .. "exactly as it happened - every input and frame time - and P again takes over.",
    },
}

local function index_of(path)
    for i, s in ipairs(M.SCENES) do
        if s.path == path then return i end
    end
    return nil
end

-- The hub notes its scroll position as it changes, and asks for it back
-- (with the index of the scene last visited, if any) on init.
function M.remember_hub(top)
    if Save.get(MEMORY, "hub_top") ~= top then Save.set(MEMORY, "hub_top", top) end
end

function M.hub_memory()
    local top = tonumber(Save.get(MEMORY, "hub_top"))
    local scene = Save.get(MEMORY, "scene")
    return top and math.floor(top), scene and index_of(scene)
end

-- Leave for another scene in silence: music and the sounds a script started
-- belong to the audio backend, not the world, so they would play on into the
-- next scene (Night Drive's engine did).
local function go(path)
    Audio.stop_all()
    -- The cursor is the app's, not the scene's, so a quill (Guest Book) or a
    -- hidden or confined pointer (Options Menu) would follow you out. These
    -- write the display state at once, so nothing races the load.
    Cursor.set_image("")
    Cursor.set_icon("default")
    Cursor.set_visible(true)
    Cursor.set_confined(false)
    Scene.load(path)
end

-- Call once per frame from update(). H / Esc go to the hub; PageUp /
-- PageDown (or [ and ]) step through the list, wrapping at either end.
function M.tick(current_path)
    -- Note the scene once it is running, not as it is left: a write made in
    -- the frame that loads the next scene is a command racing the load.
    if Save.get(MEMORY, "scene") ~= current_path then
        Save.set(MEMORY, "scene", current_path)
    end
    if Input.is_action_just_pressed("hub") then
        go(M.HUB)
        return
    end
    local i = index_of(current_path)
    if not i then return end
    local n = #M.SCENES
    if Input.is_action_just_pressed("next_scene") then
        go(M.SCENES[i % n + 1].path)
    elseif Input.is_action_just_pressed("prev_scene") then
        go(M.SCENES[(i - 2) % n + 1].path)
    end
end

-- The caption colours `caption_entities` in tools/gen_scenes.py authors
-- (linear). There is no text-colour getter, so hiding a caption by its
-- alpha needs the colour to put back.
local CAPTIONS = {
    Title = { 0.911, 0.848, 0.711 },
    Hint = { 0.448, 0.477, 0.570 },
    Status = { 0.890, 0.604, 0.263 },
}

-- Tab hides a scene's captions, for a clean look (or a screenshot).
-- Returns the new visibility; the caller keeps it. `colours` is for a scene
-- whose captions are not the usual ones (Clay's are dark, on white).
function M.toggle_captions(visible, colours)
    visible = not visible
    local a = visible and 1.0 or 0.0
    for name, c in pairs(colours or CAPTIONS) do
        local e = Entity.find_by_name(name)
        if e then Text.set_color(e, c[1], c[2], c[3], a) end
    end
    return visible
end

-- The same for a scene whose captions are entity UI (`ui_captions` in
-- tools/gen_scenes.py): shown or hidden rather than faded.
function M.toggle_ui_captions(visible)
    visible = not visible
    for _, name in ipairs({ "Title", "Hint", "Status" }) do
        local e = Entity.find_by_name(name)
        if e then UI.set_visible(e, visible) end
    end
    return visible
end

-- The post-effect toggles every scene offers: 1-4 flip bloom, vignette,
-- color_grade and chromatic; 0 flips them all. `on` is the caller's table of
-- what is on (start it with M.post_all_on(SCENE)); returns true if anything
-- changed, so the caller can refresh its status line.
M.POST = { "bloom", "vignette", "color_grade", "chromatic" }

-- What each scene authors off, written by tools/gen_scenes.py with the
-- scenes: a script cannot ask the renderer.
local POST_DEFAULTS = require("post_defaults")

-- Every effect on, except those the scene starts with off. It used to be
-- every effect on, so where chromatic was authored off the status line said
-- "on" and the first press of 4 turned off what was already off.
function M.post_all_on(scene)
    local on = {}
    for _, name in ipairs(M.POST) do on[name] = true end
    for _, name in ipairs(POST_DEFAULTS[scene] or {}) do on[name] = false end
    return on
end

local function set_post(on, name, value)
    on[name] = value
    PostProcess.set_enabled(name, value)
end

function M.post_keys(on)
    local changed = false
    for i, name in ipairs(M.POST) do
        if Input.is_key_just_pressed(tostring(i)) then
            set_post(on, name, not on[name])
            changed = true
        end
    end
    if Input.is_key_just_pressed("0") then
        local any = false
        for _, name in ipairs(M.POST) do any = any or on[name] end
        for _, name in ipairs(M.POST) do set_post(on, name, not any) end
        changed = true
    end
    return changed
end

-- "1 bloom on   2 vignette on   ..." for a status line.
function M.post_status(on)
    local parts = {}
    for i, name in ipairs(M.POST) do
        parts[#parts + 1] = i .. " " .. name .. (on[name] and " on" or " off")
    end
    return table.concat(parts, "   ")
end

return M
