# Scene sweep

One pass over every scene in the lab, in tour order, with somewhere to record
what has actually been looked at.

This exists because "it worked when I wrote it" is not a claim anyone can check
later. A scene with no recorded pass is a scene nobody has re-run since the
commit that added it — which is exactly how five entity-UI widgets came to draw
nothing (BUGS #58) and a `.anim` came to preview as its whole sprite sheet
(#83) without anyone noticing.

## How to run it

```powershell
./run-editor.ps1                         # editor: open the lab, press Play
cargo run -p jengine-runtime             # native
wasm-pack build crates/jengine-web --target web --release   # web, then serve it
```

`H` / `Esc` returns to the hub, `PgUp` / `PgDn` step through the tour, so a sweep
is one long press of `PgDn`. (It was `[` / `]`, which a Finnish layout types
with AltGr - BUGS #88.) A cell reading `n/a` is a scene that does not run in
that runtime by design; the note says why.

**Three columns because that is the point of this project.** The lab is the
only place native / editor / web are compared side by side, and most of the
cross-runtime bugs in BUGS.md were one runtime behaving differently
from the other two while each looked fine alone. A scene passing in the editor
says nothing about the other two.

## How to use it

- Tick a runtime's box only after *looking* at the scene doing what its row
  says. A scene that loads without errors has not been checked.
- A divergence between runtimes is a finding even when neither looks broken —
  file it in BUGS.md and put the number in Notes.
- Clear the ticks after a change big enough to invalidate them (a renderer
  rewrite, a scripting-bridge change) and say so in Notes at the top.
- **Do not re-check by hand what a test already covers.** The automated suites
  are listed in [`docs/manual-verification.md`](../../docs/manual-verification.md);
  this sweep is for what only eyes can judge — whether it looks and feels
  right.

Last full sweep: _never_ &nbsp;·&nbsp; Ticks below are per scene, per runtime.

## The scenes

| # | Scene | What must be true | Ed | Nat | Web | Notes |
|---|-------|-------------------|:--:|:--:|:--:|-------|
| 01 | Transforms | Planet orbits the sun, moon orbits the planet, all three spinning smoothly with no drift or stutter. | [x] | [ ] | [ ] | |
| 02 | Sprites | Tint, anchor, z-order, blend modes and `uv_rect` all visibly differ between the sample sprites. Static. | [x] | [ ] | [ ] | |
| 03 | Camera | WASD moves the player; the camera follows with a visible smoothing lag rather than snapping. Q/E zoom in and out and hold at their limits. | [x] | [ ] | [ ] | |
| 04 | Input | Each action lights its own square, the mouse cursor sprite tracks the pointer, and nothing sticks after alt-tab. Both mouse squares light - **right as well as left** - and the readout under them shows held / pressed / released for each. | [x] | [ ] | [ ] | |
| 05 | Prefabs | 1/2/3 spawn Coin / Block / Spark at the mouse; left mouse re-spawns the last one. All three move at a readable pace and stay on screen: the coin damps to a stop, the block settles fast, the spark bounces on without losing speed. | [x] | [ ] | [ ] | |
| 06 | Animations | The uv-cycle walker animates, and the property-keyframe tracks (position / colour / scale) run alongside it. | [x] | [ ] | [ ] | |
| 07 | Particles | Fountain, Radial, Fire and Spiral each look distinct - the fire rises, the spiral twists. SPACE stops them all; 1/2/3 change the rate. | [x] | [ ] | [ ] | |
| 08 | Lighting | Three coloured lights orbit a checker floor; the floor brightens and colours under each. SPACE toggles them, R the orbit, 1/2/3 the ambient, C the palette, and +/- (or numpad +/-) the radii. | [x] | [ ] | [ ] | |
| 09 | Tilemap | Two tile layers draw correctly, and the script's `Tilemap.set_tile` edits show up live. | [x] | [ ] | [ ] | |
| 10 | GUI | Every `.gui` node type draws and responds: buttons click, slider drags, toggle flips, field takes typing. | [x] | [ ] | [ ] | |
| 11 | Text | World-space labels at several `font_scale` values, all legible and correctly placed; newlines break lines. | [x] | [ ] | [ ] | |
| 12 | Materials | Both rows are lit, and the bottom one is *obviously* different: it shades per pixel against the normal map while the top uses the flat fallback. `N` flips it back and forth. | [x] | [ ] | [ ] | BUGS #89, #90 |
| 13 | Behavior graphs | Everything moves without this scene's script doing anything: the `.behavior` assets drive it. | [x] | [ ] | [ ] | |
| 14 | Post-process | Scene opens with bloom + vignette + colour grade already on; 1/2/3/4 toggle them; **leaving clears them**. | [x] | [ ] | [ ] | |
| 15 | Atlas | Each sprite shows its own region of the sheet, not the whole sheet and not a neighbour's. | [x] | [ ] | [ ] | |
| 16 | Audio | The blip plays on demand and the hum loops; leaving the scene stops the hum. | [x] | [ ] | [ ] | |
| 17 | Physics | Crates stack and settle, balls bounce, the player walks and jumps, and falls *faster* than the balls (its `gravity_scale` is 1.8, for snappy jumps); T drops a ball on the player, G shakes the crates; nothing jitters or sinks through the floor. | [x] | [ ] | [ ] | BUGS #91 |
| 18 | Collision events | Within a second of landing: the cyan ball turns red, the yellow ball vanishes, the grey counter box (top right) turns green. The status line reads Despawn: gone; R reloads and everything resets. | [x] | [ ] | [ ] | BUGS #92 |
| 19 | Scorekeeper | Score survives entering and leaving a sub-scene — push, bump it, pop, and it is still there. | [x] | [ ] | [ ] | |
| 20 | Pause menu | Fade in/out is smooth, the pushed menu suspends the scene under it, and audio pauses with it. | [x] | [ ] | [ ] | hint said ESC; P toggled while held - both fixed |
| 21 | Shader graph | The pulsing sprite pulses, driven by the generated shader rather than a script. | [x] | [ ] | [ ] | |
| 22 | Vector paths | Quad and cubic segments are smooth, not faceted, and fill and stroke colours are both honoured. | [x] | [ ] | [ ] | |
| 23 | Behaviors | Oscillator, Tween, Flicker, Shake, FaceVelocity and AngularVelocity each do something distinct. A shaken entity that is also moving keeps moving. | [x] | [ ] | [ ] | |
| 24 | Steering | FollowTarget snaps; SeekTarget accelerates and decelerates into the target rather than orbiting it. | [x] | [ ] | [ ] | |
| 25 | Bounds & lifetime | Wrapping entities reappear on the far side, clamped ones stop at the edge, timed ones vanish on schedule. | [x] | [ ] | [ ] | |
| 26 | Joints | The pendulum chain swings, the rope hangs, and the spring rings then settles within a few seconds; no joint drifts apart. F4 outlines every collider and draws each joint as an orange line; R replays. | [x] | [ ] | [ ] | |
| 27 | Triggers | Entering and leaving a zone fires enter/exit exactly once each, per entity: the three status lines at the bottom flip between INSIDE and empty as the probe crosses, and each zone's enter and exit counts never differ by more than one. | [x] | [ ] | [ ] | |
| 28 | Spotlights | Three cones land on the floor with visible edges; the wandering point light adds to them. A warm baked glow (LightmapComponent) lights the floor between the red and green cones. The captions stay fully bright while the scene around them is dark: they carry `unlit`. | [x] | [ ] | [ ] | |
| 29 | Collision scripts | Each ball's own script reports its enter and exit, with the right partner named. Each ball's status line (top left) counts enters and exits; R drops the balls again. | [x] | [ ] | [ ] | |
| 30 | Sprite fill | NineSlice keeps the orange corners small and square at both sizes while Stretch squashes them into ovals; Tile repeats the checker and clips the last row and column at 48 units; the rotated row turns about each sprite's centre. | [x] | [ ] | [ ] | |
| 31 | Shadows | Skewed shadows sit under their casters and darken by coverage rather than painting flat black. | [x] | [ ] | [ ] | |
| 32 | Light occlusion | Occluders cast shadows; the soft light's penumbra is visibly softer than the hard one's. | [x] | [ ] | [ ] | |
| 33 | Scroll views | Wheel and drag scroll the lists; clipped rows do not draw outside their box, and a drag does not click the row it started on. | [x] | [ ] | [ ] | BUGS #97 |
| 34 | Skeleton & IK | The two-bone arm reaches for the cursor and the skinned mesh deforms with the bones. | [x] | [ ] | [ ] | |
| 35 | Hierarchy | Children inherit the parent's rotation and scale, not only its position. | [x] | [ ] | [ ] | |
| 36 | Combat | Hitboxes damage on overlap, i-frames suppress repeats, health bars track, dead entities despawn. | [x] | [ ] | [ ] | BUGS #154 |
| 37 | Custom components | The three boxes read `hp 100 lvl 1 gold 25` on load — real authored numbers, not zeros. | [x] | [ ] | [ ] | |
| 38 | Save | Values survive a commit and a reload, including nested tables; a named slot comes back under its own name. | [x] | [ ] | [ ] | |
| 39 | Data tables | The three cards are built from `.data` rows through the schema, with ref-following resolved. | [x] | [ ] | [ ] | |
| 40 | Entity UI | All eight widgets draw, and the four interactive ones answer the pointer: press, drag, flip, type. The readout agrees with the widgets. | [x] | [ ] | [ ] | |
| 41 | Tweens | All six targets and four easing curves run from the scene file alone; Stop / Loop / PingPong each behave. | [x] | [ ] | [ ] | |
| 42 | Flocking | Each swarm goes round its rock (never through it) to its own target; the left swarm (Separation) keeps its spacing, the right one piles into a single dot. | [x] | [ ] | [ ] | |
| 43 | Profiling | F3 shows plausible frame stats, F12 the entity inspector. Rows that are flat zero mean that runtime is not filling them. | [x] | [ ] | [ ] | |
| 44 | Gamepad | The analog marker moves smoothly while the thresholded one snaps; a hold-to-charge action sees its release. | [ ] | [ ] | [ ] | |
| 45 | Anim state machine | States switch on their conditions and the animator follows — no stuck state, no flicker between two. | [x] | [ ] | [ ] | |
| 46 | Sort layers | Sprites stack by their named bucket's `base_z`, then by `z_order` within it. | [x] | [ ] | [ ] | |
| 47 | Motion components | Acceleration2D, InputActionImpulse, ClampBounds and PrefabInstance all work with no script involved. | [x] | [ ] | [ ] | |
| 48 | Timers & events | One-shot timers fire once, intervals repeat, and `Event.send_to` reaches the named entity only. | [x] | [ ] | [ ] | |
| 49 | Tween API | Script-created tweens run and can be cancelled by id mid-flight. | [x] | [ ] | [ ] | |
| 50 | Tags & queries | `Entity.find_by_tag` and `Entity.find_in_radius` return what the labels claim, and the radius filter respects its tag. | [x] | [ ] | [ ] | |
| 51 | Tilemap pathfinding | The walker threads the maze (not round it) to the goal 40 steps away; clicking an open cell walls it and the path re-plans, clicking a wall clears it. M switches 4-/8-way, Space pauses. | [x] | [ ] | [ ] | |
| 52 | Debug draw | Lines, boxes and circles appear for one frame each in world space and do not accumulate. | [x] | [ ] | [ ] | |
| 53 | Camera FX | Shake decays rather than cutting off and the flash fades. Time scale 0 is a *true* pause - spinner, oscillator, lifetimes and animation all stop - while scripts keep running. | [x] | [ ] | [ ] | |
| 54 | Input rebinding | A rebound action responds on its new key and stops responding on the old one. | [x] | [ ] | [ ] | |
| 55 | Audio buses | A bus's volume multiplies its children's; muting a bus silences only what routes through it. | [x] | [ ] | [ ] | |
| 56 | Physics forces | Script-applied forces and impulses move authored bodies, and contacts read back. | [x] | [ ] | [ ] | BUGS #154, #156 |
| 57 | Particle bursts | `Particles.emit_burst` fires a one-shot puff, a `burst_count` emitter bursts on becoming active, and the live count is right. | [x] | [ ] | [ ] | |
| 58 | Batch scripts | The batch tier runs over many entities and writes land in the same frame. | n/a | [ ] | n/a | native only (threads + QuickJS); the drifters sit still in the editor and on web by design, and the scene says so |
| 59 | Packages | The parallax layers scroll at their own rates — an engine package's system actually running. | [x] | [ ] | [ ] | |
| 60 | Project packages | The spinner spins. **Needs `./build-project-viewport.ps1 -Project projects/engine-test-lab`** — a stock viewport leaves it inert, which is the expected symptom, not a bug. | [x] | [ ] | [ ] | |
| 61 | Swarm | The boids stay a flock at load-bearing counts and the frame time stays sane. | [x] | [ ] | [ ] | |
| 62 | Editor stress | 384 entities: the editor's entity list stays usable. Editor-only by nature. | [x] | — | — | |
| 63 | Spawned physics | Bodies a *script* created respond to forces exactly as authored ones do. | [x] | [ ] | [ ] | |
| 64 | Patrol paths | Loop, PingPong and Once each traverse the same waypoints differently and none stalls. | [x] | [ ] | [ ] | BUGS #157 |
| 65 | Custom fonts | Every pair differs — same string and scale, only `font_id` changes. The missing-font line stays readable, and the yellow custom-font line sits on top at z=5. | [x] | [ ] | [ ] | |
| 66 | Networking | Nothing connects on load. With no server, `1` reports `failed` — that is the state machine working. Needs the project viewport. | [ ] | [ ] | [ ] | |
| 67 | UI scaling | Resize the window: the green bar stays flush with both edges, the four boxes stay inside their border, the corner panels stay in their corners, and both buttons still count clicks. | [x] | [ ] | [ ] | |
| 68 | Graph commands | S: a gold coin appears at the teal box and bounces, with a blip. A starts the orange spin, X stops the purple one. A click drops a pink spark under the pointer. The clock hand steps once a second. At 4 s the red box vanishes with a blip and the grey box turns green. L reloads everything. | [x] | [ ] | [ ] | BUGS #93 |
| 81 | Character controller | The capsule runs with the arrows and jumps with Space: it climbs the ramp at walking pace, stops dead at the wall (and the readout says `wall 1`), lands on the one-way ledge from above but passes up through it from below, and `Down`+`Space` drops it through. The amber capsule steps onto the low ledge; the blue one, with no `step_height`, stops at it. It rides the lift up and down without sinking or being left behind, shoves the crate along, and walks through the violet post while the readout lists it under `touching`. | [x] | [ ] | [ ] | Core, no package |
| 82 | Camera turn | The camera follows the car round its circuit turned to its heading, so the car stays pointing up while the world swings round it; the HUD does not turn. | [x] | [ ] | [ ] | |
| 83 | Frame events | The punch plays a sound and opens its hitbox on frame 2 and closes it on frame 4 - the frames drive it, not a timer in the script. | [x] | [ ] | [ ] | |
| 84 | Contact impulses | Three crates land from three heights and the impulse written under each grows with its drop. | [x] | [ ] | [ ] | BUGS #158, #159 |
| 85 | Despawn offscreen | As the camera pans, the blue markers (despawn_offscreen) keep their count steady; the amber ones climb for ever. | [x] | [ ] | [ ] | |
| 86 | Disabled buttons | Each shop button is dimmed and ignores clicks until the wallet covers its price, then lights up and works. | [x] | [ ] | [ ] | |
| 87 | Prefab textures | Both columns of spawned prefabs are textured - including the left one, whose sheet only a prefab names. | [x] | [ ] | [ ] | BUGS #160, #161 |
| 88 | Y sorting | On the left the walker passes behind trees above it and in front of trees below; on the right it draws over all of them. | [x] | [ ] | [ ] | BUGS #162 |
| 89 | Tile id zero | The null floor and the 0 floor look and answer the same everywhere the probe walks. | [x] | [ ] | [ ] | |
| 90 | Continuous collision | At the same speed, the ball without ccd passes through the wall and the one with ccd is stopped. | [x] | [ ] | [ ] | |
| 91 | Joint anchors | The vehicle whose wheel joints set anchor_b keeps its wheels at the axles and drives; the other collapses them to the middle. | [x] | [ ] | [ ] | |
| 92 | Raycast | Forty-eight rays fan out and each stops at what it hits, with a dot on the hit point and a spur along the normal. | [x] | [ ] | [ ] | |
| 93 | Framing camera | The camera keeps both runners on screen, zooming out as they part and back in as they meet (the posts show the zoom). | [x] | [ ] | [ ] | |
| 94 | Tilemap chunks | Digging with the mouse is instant on the 160x90 map, and the crates fall into what you dig. | [x] | [ ] | [ ] | |
| 95 | Frame pacing | The three bars glide at constant speed with no judder, and the dt spread readout stays small. | [x] | [ ] | [ ] | |
| 96 | Scene churn | After twenty-five self-reloads the last copy hears exactly one ping and no stray timers fire. | [x] | [ ] | [ ] | |
| 97 | Occluder pool | All twenty numbered boxes under the light cast shadows (before Sep 2026, exactly sixteen did). | [ ] | [ ] | [ ] | Expectation changed with the per-light occluder pool; re-check native too. |
| 98 | Searchlight | The cone turned with set_rotation sweeps across the pillars and casts moving shadows; the one that only sets direction stays put. | [x] | [ ] | [ ] | |
| 99 | Textured mesh | The quad turns with its entity; the trapezoid with perspective weights keeps its rows straight, the one without bends them. | [x] | [ ] | [ ] | |
| 100 | Graph events | A script's send_event brightens the graph's box; a despawned pod's OnDespawn drops a ring where it was; the graph's EmitEvent reaches the script. | [x] | [ ] | [ ] | |
| 101 | Wind and drag | The flag settles at wind / drag, the ball is kicked once, the pinwheel spins up, and the arrows move the box - no script involved. | [x] | [ ] | [ ] | |
| 102 | Emit shapes | Snow falls along the whole top edge, and the point, ring and rectangle emitters each fill their own shape. | [x] | [ ] | [ ] | |
| 103 | Script inheritance | All three boxes spin and pulse once a second; the middle one also bobs; the right one is orange. | [ ] | [ ] | [ ] | |
| 104 | Script namespaces | Same counts in every runtime (Skeleton 4 native, 3 elsewhere); both boxes the same blue; the equality line reads true. | [ ] | [ ] | [ ] | |
| 105 | Touch controls | The stick moves the box (a diagonal names two actions), A turns it yellow, B grows it, and the readout names exactly what is held; the box is square in a window of any shape; the mouse works them (`show: "always"`). | [ ] | [ ] | [ ] | Seen on the Android emulator (native runtime): stick, diagonal, A. Two fingers at once not yet tried. |
| 106 | Safe area | On a desktop nothing moves. With a phone in the editor's device preview, or on Android with a cut-out, the corner panels and the controls sit clear of the notch and corners; the backdrop runs under them. | [ ] | [ ] | [ ] | Android emulator (tall cut-out): left inset 132 read, scene 105's stick moved clear and answered there. |
| 107 | Controller menus | Arrows / D-pad / stick walk the grid and the list (the list scrolls to follow), Enter / Space / A press and the readout names the button, ECHO refuses. [U]: entity UI - button, slider (left/right), toggle. After a mouse click, Space presses nothing until an arrow is used. | [ ] | [ ] | [ ] | |
| 108 | Multi-touch | On a touch screen: a ring under every finger with its id and phase (green began, blue moved, yellow held, red for the one frame it ended); two fingers show their distance and angle, which follow a pinch and a twist; the first free finger also moves the mouse line. A desktop shows 0 fingers. | [ ] | [ ] | [ ] | Android and a phone browser are the runtimes that matter. |
| 109 | HDR | With HDR on (the default): the lamp's pool falls off smoothly and is brighter than the grey floor it lights; the six dots step up in brightness and the brighter ones bloom wider, far past their edges. `T` turns HDR off: the lamp becomes a flat disc with a coloured ring where each channel clips at 1.0 - the pre-Sep-2026 look. `1`-`4` switch the tonemap (none clips the dots to one white), `Q`/`W` exposure, `A`/`S` bloom scatter (wider/softer). The white entity-UI box never blooms or vignettes, whatever the settings. | [ ] | [ ] | [ ] | A device without a float render target (some WebGL2 / old GLES) draws it as HDR off. |
| 110 | Blend modes & particles | Five red discs over five grey squares, one per blend mode, and each looks as its label says: Alpha covers, Additive brightens the grey pink, Multiply darkens it, Screen lightens, Premultiplied is opaque red. Below: the Additive fire has a bright core the Alpha one lacks; the sparks are streaks turned along their flight and falling; the dust fades in and grows, then fades out, never popping. | [ ] | [ ] | [ ] | Before Sep 2026 all five discs drew as Alpha and every particle was a square. |
| 111 | Unlit sorts with lit | A grey tower full of warm windows, half covered by a nearer building with a cyan OPEN sign. No window shows through the near building; Space slides it right and back, uncovering and covering windows as it goes. The lamp sweeping across brightens the buildings but never the windows or the sign, which stay full bright. A blue haze band across the tower darkens the windows under it. | [ ] | [ ] | [ ] | Before Sep 2026 the windows drew over the near building. |
| 112 | Occluder self-shadow | Two bumpy blocks under a lamp on the mouse. The left one is lit and its bumps shade toward the lamp; the right one is black inside. Both throw the same shadow away from the lamp. With the lamp inside the left block, the block is lit and everything outside it dark. | [ ] | [ ] | [ ] | Before Sep 2026 both blocks were black. |
| 113 | Shadow length | Four dark posts under a sun that follows the mouse. The first post's shadow runs off the bottom of the screen; the others end short - longer from left to right - and each fades out at its tip rather than stopping hard. | [ ] | [ ] | [ ] | Before Sep 2026 every shadow ran to the edge of the light. |
| 114 | Light height | Two grey spheres, each circled by a small light. Both have a day and a night side that turn with their light; on the left the day side reaches well past the middle, on the right the terminator sits near the middle. | [ ] | [ ] | [ ] | Before Sep 2026 every light sat at the left one's height. |
| 115 | Noise nodes | Three squares: soft drifting cloud noise with no grid pattern and no straight diagonal seams; round Voronoi cells, dark at each centre; the same cells as thin bright outlines. A flat grey or white square means a graph failed to build - on web too, where graph materials drew nothing before. | [ ] | [ ] | [ ] | Before Sep 2026 there were no noise nodes, and web drew no graph material. |
| 116 | Scene Color | Three squares over stripes: left, the stripes inside are offset and do not line up with those outside; middle, they wave side to side, moving; right, the red bar shows inside the square as well, higher than outside. A plain white square means the graph fell back to the default shader. | [ ] | [ ] | [ ] | Before Sep 2026 no graph could read the scene. |
| 117 | Sheared children | Left: the checker spins flat like a plate on a table, its corners on the blue ellipse, its squares leaning as it turns; it never becomes a thin rectangle standing up out of the ellipse. Right: an ordinary spin. The orange arrow turns with each disc. | [ ] | [ ] | [ ] | Before Sep 2026 the hierarchy had no shear. |
| 118 | Lights and decals | Left: the post whose light casts shadows throws a dark wedge down the floor below it; the other post has none. Right: a soft dark spot, and nothing at all where the white multiply decal sits - never a pale box. | [ ] | [ ] | [ ] | Before Sep 2026 every light cast shadows, and a soft multiply brightened its quad. |
| 119 | Lightmap graphs | Left: a still pool of light on the dim checker. Right: warm rings of light running outward over the checker. A white or empty square on the right means the graph did not reach the light pass. | [ ] | [ ] | [ ] | Before Sep 2026 a lightmap was a still texture. |
| 120 | Many lights | As the camera pans the long floor, every light in view is lit - no dark gaps. The orange light moves with the bobbing block. Top right, the lens shows the floor behind it shifted. | [ ] | [ ] | [ ] | Before Sep 2026 lights past 256 in world order were dropped wherever they were, and a lens drew nothing in a lit scene. |
| 121 | Ambient occlusion | Every two seconds: a soft grey ring around each white box, darkest in the narrow gap between the pair and in the L's inside corner, the boxes' tops staying white - then flat white. | [ ] | [ ] | [ ] | Before Sep 2026 there was no ambient occlusion. |
| 122 | Trails | Left: a white ribbon narrowing to a point and fading to blue behind the wandering dot; every two seconds it stops emitting and ages away ("not emitting"), then starts again. Middle: an orange arc glowing behind the spinning tip. Right: a green streak over the floor that passes behind the block. Nothing ahead of any dot. | [ ] | [ ] | [ ] | Before Sep 2026 there were no trails. |
| 123 | Texture filters | Top: the left heart in hard square blocks, the right one smoothly blended. Bottom, turning: the left checker a flickering moire, the right a steady mid grey. | [ ] | [ ] | [ ] | Before Sep 2026 every texture was nearest, with no mipmaps. |
| 124 | Pixelate and CRT | Every two seconds: plain; 8-pixel cells (the bar turns in whole cells, the green dot steps a cell at a time, world text blocky); a curved screen with scanlines and black corners; both. The state label and hint (UI) stay sharp throughout. | [ ] | [ ] | [ ] | Before Sep 2026 neither effect existed. |
| 125 | Directional light | Every box's shadow points the same way and swings round with the sun; as its height falls the shadows lengthen and the light reddens, at noon they are short. Light is even across the ground. | [ ] | [ ] | [ ] | Before Sep 2026 a sun was a far point light, and nothing lengthened a shadow. |
| 126 | Toon lighting and outlines | Every two seconds: the balls' shading is three flat bands with crisp edges, then a smooth gradient. The turning star keeps a dark line of even width round its points throughout. | [ ] | [ ] | [ ] | Before Sep 2026 neither toon lighting nor a texel size node existed. |
| 127 | Dithered lighting | Every two seconds: smooth light; four flat hard bands; the same bands with a fine dither pattern at each edge. As the lamp slides the pattern stays fixed to the floor. | [ ] | [ ] | [ ] | Before Sep 2026 there was no dithered lighting and no toon setter. |
| 128 | A mesh shaped by a script | A checkered flag waves from its pole, the squares bending with the cloth, under a blue band. On the right a white star with a tinted centre breathes. | [ ] | [ ] | [ ] | Before Sep 2026 a script could not shape a mesh at all. |
| 129 | Particle drag and turbulence | Every two seconds two rings of sparks: the left flies out of view, the middle slows and hangs about 90 units out. Right: rising smoke curls into drifting eddies. | [ ] | [ ] | [ ] | Before Sep 2026 particles had neither drag nor turbulence. |
| 130 | A graph knows where it is | The stripes stay put while the window over them slides. The ball and post are mirrored upside down below the line, touching their reflections at the waterline as the camera pans and zooms. | [ ] | [ ] | [ ] | Before Sep 2026 PositionInput returned UV and there was no screen position. |
| 131 | Steering a swarm | 1,500 arrows flock after the green dot (the mouse, or a circle when it is still), each pulsing on its own beat; the red disc clears a hole in the flock as it orbits. | [ ] | [ ] | [ ] | Before Sep 2026 a swarm could not be steered, textured or animated. |
| 132 | Scene Color in z order | The orange ball behind the rippling band is bent by it; the blue ball crossing in front stays sharp and is not bent. | [ ] | [ ] | [ ] | Before Sep 2026 both balls were under the band. |
| 133 | Light shafts | Beams stream from the swinging disc through the gaps in the grate, each bar cutting a dark wedge; G removes them. | [ ] | [ ] | [ ] | New Sep 2026. |
| 134 | Particles meet the world | Rain stops on every grey shape and splashes; sparks bounce down the ramp, B makes them die on it; the green trail follows the small orbiting dot; Space bursts pink. | [ ] | [ ] | [ ] | New Sep 2026; native bursts drew nothing before. |
| 135 | Skeletal blend and foot IK | 1/2/3 crossfade smoothly (no snap); Up/Down blend walk into run with no lost step; paws stand on the ramp and step; I lets them sink into the ramp. | [ ] | [ ] | [ ] | New Sep 2026. |
| 136 | GUI styles | Rounded cards with soft shadows; the middle fades with its child, the right breathes in scale; buttons ease to orange and grow on hover; Recolour recolours the first card. | [ ] | [ ] | [ ] | New Sep 2026; hover colours never drew before. |
| 137 | Tilt-shift and turning patrols | Checks sharp in a band that follows the mouse, blurred away from it; B blurs all; both cars face their way, the white turning smoothly. | [ ] | [ ] | [ ] | New Sep 2026. |
| 138 | Fracture and caster shadows | Left tower dark but for its lamp-side edges, right tower lit per block, both shadowing the wall; Space breaks blocks into quarter pieces with dust. | [ ] | [ ] | [ ] | New Sep 2026. |
| 139 | Cloth and rope | The flag flies and ripples and follows its swinging pole; the banner sways; the upper rope rides the weight; the lower one stops at the wall; W calms the cloth. | [ ] | [ ] | [ ] | New Sep 2026. |
| 140 | A pixel-perfect camera | At pixel size 6 the bar and the disc are whole 6x6 blocks and the pan slides smoothly; P turns it off; the text stays sharp. | [ ] | [ ] | [ ] | New Sep 2026. |
| 141 | Cameras into textures | The left monitor and the UI corner show the lamp-lit room with its turning bar; the right monitor is a tunnel of itself; V points the room camera here. | [ ] | [ ] | [ ] | New Sep 2026. |
| 142 | Sprite masks | An ellipse of orange checks, blue checks with a turning hole, a star of green checks with its text and sparks cut to the star. | [ ] | [ ] | [ ] | New Sep 2026. |
| 143 | Sharp text | Every line from x0.5 to x5.5 has one-pixel edges; the gold word turns sharp; the blue one breathes to five times its size, sharp throughout. | [ ] | [ ] | [ ] | New Sep 2026. |
| 144 | GPU particles | A fountain, a turning galaxy and a ring drawn inward; Up/Down to 300k each without slowing; Space freezes them. | [ ] | [ ] | [ ] | New Sep 2026. |
| 145 | Culling masks | The main view shows red, green and blue; the left window only red, the right one only green and blue. | [ ] | [ ] | [ ] | New Sep 2026. |
| 146 | SDF custom font | Left (bitmap) softens or steps scaled up; right (distance field) keeps one-pixel edges at x1.5, x3, x6 and while turning; both columns line up. | [ ] | [ ] | [ ] | New Oct 2026. |
| 147 | Texture masks | Checkerboard clipped inside a ragged tear (cracks too), the tear as a hole, a soft blob fading, and a turning tear. | [ ] | [ ] | [ ] | New Oct 2026. |
| 148 | Stacked readers | The overlap of two shifting readers is shifted two steps; the dark bar moves under the upper reader only. | [ ] | [ ] | [ ] | New Oct 2026. |
| 149 | Whole pixels | Every 1-pixel check is the same width at any window size; the drifting square steps by whole pixels. | [ ] | [ ] | [ ] | New Oct 2026. |
| 150 | Rich text | Six styled lines with no markup showing; the gold typewriter line pauses twice and restarts; the grey non-rich line shows its brackets. | [ ] | [ ] | [ ] | New Oct 2026. |
| 151 | Autotile | Left map: outlined grass and stone areas with rounded corners, stone over grass; right map: the same cells as flat squares. A click cycles a cell in both, the left re-edging its neighbours. | [ ] | [ ] | [ ] | New Oct 2026. |
| 152 | Isometric tilemap | A checked diamond map with pillars standing on cells; the red walker is hidden behind pillars and drawn over them in front; the outline and label follow the mouse, a click adds or removes a pillar. | [ ] | [ ] | [ ] | New Oct 2026. |
| 153 | Carvable sprite | A click cuts a round, rimmed hole in the slab; the balls above fall through it, roll on the cut edges and settle in the hollows without sinking. | [ ] | [ ] | [ ] | New Oct 2026. |
| 154 | Text outlines and shadows | Over the stripes only the plain row is hard to read; the others are outlined, shadowed or soft-shadowed, the rich row waves with its outline, the UI line is outlined and shadowed. | [ ] | [ ] | [ ] | New Oct 2026. |
| 155 | Spline shape | A smooth hill, a closed loop and a straight ridge, checker-filled in square tiles and edged yellow-out, brown-in; the balls land on the drawn surfaces and roll off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 156 | Area effects | Crate and ball float half under, the red hull rights itself; the green box and motes ride the updraft (Space drops them); the weightless ball is drawn to the middle of the pull. | [ ] | [ ] | [ ] | New Oct 2026. |
| 157 | Hex tilemap | Pointy (left) and flat-topped (right) hex maps; the hex under the mouse outlined, its six neighbours yellow, a cyan path from the corner round the walls; the balls settle on hexagons. | [ ] | [ ] | [ ] | New Oct 2026. |
| 158 | Sprite-shaped shadows | Top row: each shadow follows its sprite - light into the ring's mouth, between the crown's clumps and the pickets; bottom row: pickets and ring as boxes, solid wedges. | [ ] | [ ] | [ ] | New Oct 2026. |
| 159 | Light cookies | A turning checker-and-arrow disc of light that follows the mouse and stops at its edge; a spot throwing red, green and blue bands. | [ ] | [ ] | [ ] | New Oct 2026. |
| 160 | Animated tiles | Top row: a gold bar sweeping every tile together; bottom row: the same, each cell out of step. | [ ] | [ ] | [ ] | New Oct 2026. |
| 161 | Shape queries | Colliders the gold circle / box / point touches turn gold (the long wall from anywhere along it); a cyan disc swept down rests where it first touches. | [ ] | [ ] | [ ] | New Oct 2026. |
| 162 | Positional audio | Moving the square: each loop full inside its bright ring, silent past its faint one, from its side; a click's blip from where it was. | [ ] | [ ] | [ ] | New Oct 2026. Listen. |
| 163 | Navigation | Pink dots: the shortest way to the mouse, round every shape. Click: the agents walk there without overlapping, through the sensor; G closes the gate and they replan. F4 shows the navmesh. | [ ] | [ ] | [ ] | New Oct 2026. |
| 164 | Localization | 1-4 switch language: every line but the red key changes, the rooms line takes the right plural, the price its decimal comma, the grey line stays English. | [ ] | [ ] | [ ] | New Oct 2026. |
| 165 | Audio effects | Listen: the purple box rings in a big room, the blue one muffles everything (the blip too); L / R do it from a script; E switches the boxes off. | [ ] | [ ] | [ ] | New Oct 2026. Listen. |
| 166 | Camera follow | 1-4: the camera holds inside the yellow box and is dragged by its edge; with look-ahead it leads the way you run and swings round slowly on a turn. | [ ] | [ ] | [ ] | New Oct 2026. |
| 167 | Rumble | With a pad: 1/A a short buzz, 2/B a deep thud, 3/X held a steady engine, 4 stops. V buzzes a phone. Feel it. | [ ] | [ ] | [ ] | New Oct 2026. Feel. |
| 168 | Freeform lights | Each light fills its outline evenly and fades smoothly past it, no seam in the L's corner; the hexagon pulses; Space turns the window, R spins the L. | [ ] | [ ] | [ ] | New Oct 2026. |
| 169 | Jiggle bones | Moving the bar: the tails stream behind and swing back, soft loosest and stiff barely; the antenna springs upright; the weed leans with its current. | [ ] | [ ] | [ ] | New Oct 2026. |
| 170 | Visibility notifier | Panning: each lantern lights crossing into the orange lines and greys leaving them; upper ones stop sparking outside; the count matches the lit lanterns. | [ ] | [ ] | [ ] | New Oct 2026. |
| 171 | Path follow | Cars ride the loop evenly in three lanes, turned along it; the shuttle ping-pongs and T switches its track; the runner stops, flashes and restarts; Space pauses all. | [ ] | [ ] | [ ] | New Oct 2026. |
| 172 | Sorting groups | Left: red's front arm draws over blue's body as they cross. Right: the nearer character covers the other whole, its arms still before and behind it; Up/Down swap who is nearer. | [ ] | [ ] | [ ] | New Oct 2026. |
| 173 | Lines | Pointed, cut and rounded corners; flat, square and round ends; a tapered gradient line; whole arrows tiled; the checker once round a loop; the wave follows the mouse and 1/2/3 change its joints. | [ ] | [ ] | [ ] | New Oct 2026. |
| 174 | Specular highlights | No glint on the matt sphere; a glint toward the lamp on the others, broad to pin-point, following it round; only the plate's bright bands shine. | [ ] | [ ] | [ ] | New Oct 2026. |
| 175 | Canvas groups | All three fade together; the loose figure's parts show through each other, the grouped ones fade as one flat picture (the right one tinted), lit like the rest. | [ ] | [ ] | [ ] | New Oct 2026. |
| 176 | Video | The countdown plays on both screens; Space holds it, R restarts, Up/Down change speed; loop off stops it at 119 and says it ended. | [ ] | [ ] | [ ] | New Oct 2026. |
| 177 | Tile shapes & shadows | Balls roll down slopes, rest on the ledge, fall through grass; the rock shadows the lamp, the glass does not; a broken tile stops colliding and shadowing. | [ ] | [ ] | [ ] | New Oct 2026. |
| 178 | Vertex offset | The grass sways in a wave, roots still; the banner ripples from the pole; Up/Down change the grass's lean down to none. | [ ] | [ ] | [ ] | New Oct 2026. |
| 179 | Light layers | The torch lights the figures and not the wall, the moon the reverse; 1 / 2 add the wall / drop the figures. | [ ] | [ ] | [ ] | New Oct 2026. |
| 180 | Particle trails | Ribbons behind every particle: arcs, curls, bends at bounces; T takes them away and back. | [ ] | [ ] | [ ] | New Oct 2026. |
| 181 | Emission shapes | Sparks rise from the whole curve; the star's outline burns and its body mists, turning with it; E: outline / whole. | [ ] | [ ] | [ ] | New Oct 2026. |
| 182 | Colour grading | 1 unchanged, 2 warm, 3 night, 4 black and white; Up/Down fade it; G grain. | [ ] | [ ] | [ ] | New Oct 2026. |
| 183 | Ragdolls and breaking joints | Space knocks the figure over and stands it up; the 8 kg chain snaps; G (heavier gravity) snaps the 4 kg. | [ ] | [ ] | [ ] | New Oct 2026. |
| 184 | Split screen | Each half follows its player; 2 top / bottom, 3 inset; the overview shows in the gap. | [ ] | [ ] | [ ] | New Oct 2026. |
| 185 | Keyframe timeline | The box glides and turns, the square fades, the yellow one swaps, the word counts, beats count; Q a second clip, L loops. | [ ] | [ ] | [ ] | New Oct 2026. |
| 186 | Font files | Five greetings in five scripts and a Cyrillic / Greek UI line; T swaps in other words. | [ ] | [ ] | [ ] | New Oct 2026. |
| 187 | Fixed update | update at the display's rate, fixed_update 60 a second; the ball settles, the drone hovers; Space / K knock them. | [ ] | [ ] | [ ] | New Oct 2026. |
| 188 | IK chains | the white tips sit on their targets (status near 0 within reach); the middle one never folds; W eases the right one off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 189 | Blend spaces | on a point the hand does that clip alone; between, it mixes place and pace with no jump; Space fades to a spin and back. | [ ] | [ ] | [ ] | New Oct 2026. |
| 190 | Rich UI text | markup coloured, bold, moving and with icons in both columns; the non-rich lines show brackets; the letter types out and Space restarts it. | [ ] | [ ] | [ ] | New Oct 2026. |
| 191 | Sprite colliders | the left shapes rest on their own pixels, the right ones on their boxes; F4 shows the traced outlines; Space drops them again. | [ ] | [ ] | [ ] | New Oct 2026. |
| 192 | Anti-aliasing | turning edges smooth with FXAA, stair-stepped without (F); UI text crisp either way. | [ ] | [ ] | [ ] | New Oct 2026. |
| 193 | Conveyors | crates ride right, drop, ride left and climb the green belt; Space reverses the lower belt. | [ ] | [ ] | [ ] | New Oct 2026. |
| 194 | Middle mouse | each lamp lights while its button is held; a middle drag pans; no autoscroll in a browser. | [ ] | [ ] | [ ] | New Oct 2026. |
| 195 | Rebinding | Enter then a key rebinds lab_jump; the lamp follows the new key; F5 and a restart keep it; Backspace restores J. | [ ] | [ ] | [ ] | New Oct 2026. |
| 196 | Prefab variants | green; red + sparks; big slow red + sparks; blue + sparks; Space spawns the first three looks again below. | [ ] | [ ] | [ ] | New Oct 2026. |
| 197 | Camera blend & confine | the close view never shows red, round the L's corner too; 1 / 2 ease between cameras (zoom too); C shows red past the walls. | [ ] | [ ] | [ ] | New Oct 2026. |
| 198 | Bus effects & variation | (listen) 1 echoes the click, 2 crackles, 3 evens, 4 thins and brightens, 5 muffles, 6 rings; V: no two clicks alike. | [ ] | [ ] | [ ] | New Oct 2026. |
| 199 | Sprite swap | crown / ring / L; A-C swap the whole look, T / M / G one part; C's top a 70 px dot. | [ ] | [ ] | [ ] | New Oct 2026. |
| 200 | Tile data | grass in four looks, the same each run; three crates where the X tiles were; pointing reads kind / walk / burns / damage. | [ ] | [ ] | [ ] | New Oct 2026. |
| 201 | GPU particles meet the world | the stream parts round the disc and slides along the slab, none inside; C lets it through; A knots it round the mouse. | [ ] | [ ] | [ ] | New Oct 2026. |
| 202 | Global illumination | a red glow off the lit floor across the room, past the lamp's reach, stopped by the wall; G off: only the lamp. Needs a float frame (not WebGL2 without float targets). | [ ] | [ ] | [ ] | New Oct 2026. |
| 203 | Prefabs with parts | two carts of spinning wheels and a flag, the right one's second wheel red; Space: a whole cart rolls in, its flag recoloured by find_child; destroying it at the edge takes its parts. | [ ] | [ ] | [ ] | New Oct 2026. |
| 204 | Display and cursor | 1/2/3 window modes, R resolutions, V vsync (bars tear when off), F caps (30 steps), C cursors ending in a gold-striped arrow then none, L keeps it in; the readout follows. A browser: fullscreen and cursors only. | [ ] | [ ] | [ ] | New Oct 2026. |
| 205 | Text fields | a blinking caret where clicked; selection highlighted; clipboard round trip with another app; H typed not hub; Enter logs; password shows *; long message scrolls; IME composes (native, browser). | [ ] | [ ] | [ ] | New Oct 2026. |
| 206 | Scene streaming | A adds rooms side by side, each arrow spinning (its chunk's script); lines show loading then loaded; U removes the oldest; P preload reaches loaded. | [ ] | [ ] | [ ] | New Oct 2026. |
| 207 | Prompts and interactions | Device line follows the last input; a pad shows name and family; prompts per family; holds at 0.5/1/2 s once each, short press = tap, two quick = double tap. | [ ] | [ ] | [ ] | New Oct 2026. |
| 208 | Music director | 2 crossfades A to B; 3 stems in time, 4-6 fade one each; 7 playlist crossfades A into B; P holds; M seeks; - = bend pitch; L loop stops and bends by its id. | [ ] | [ ] | [ ] | New Oct 2026. |
| 209 | Animation layers | Legs walk whatever the arm does; 1 raises only the arm and falls back, end count rises; 2 waves until 3; 4 half; B rocks the body on the walk; Space stops the walk, layers play on. | [ ] | [ ] | [ ] | New Oct 2026. |
| 210 | Particle flipbooks | Left counts 0-15 over each life; middle counts on from frames of their own, unlike each other (F speeds them); right holds one of 0-7. | [ ] | [ ] | [ ] | New Oct 2026. |
| 211 | Noise | Six panels: soft blobs, cells with cracks, clouds, ridges, a churning one; Space reseeds all, arrows slide them seamlessly. Same seed looks the same in every runtime. | [ ] | [ ] | [ ] | New Oct 2026. |
| 212 | More joints | Cart bobs over bumps (1 reverses); blue snaps / red lags to the dot (mouse moves it); arm sways back after 2; pucks: falls / slides slowly / holds (3 resets). | [ ] | [ ] | [ ] | New Oct 2026. |
| 213 | Group materials and dark lights | Two ghosts fade as one each; the right one glows round its outline only and ripples. A dark pool glides past the lamp, black at its centre (1 toggles, 2 holds it). | [ ] | [ ] | [ ] | New Oct 2026. |
| 214 | Navigation: tiles and obstacles | Orange robot never crosses the wall (gap, or D's hole); blue robot uses the aisle unless P has pushed the crate in. | [ ] | [ ] | [ ] | New Oct 2026. |
| 215 | Spectrum and synth | Keys play notes and light a bar by pitch; square/saw light harmonics, noise all; M's loop makes the bars dance; level follows loudness. | [ ] | [ ] | [ ] | New Oct 2026. |
| 216 | Screenshots | P fills the next slot with the screen as it was; the line says ready; F also writes a PNG (a download in a browser). | [ ] | [ ] | [ ] | New Oct 2026. |
| 217 | Shaped text | Arabic joined and dotted right to left, Hebrew right to left, Hindi conjuncts and vowel signs, Thai marks stacked, a mixed line in both orders; both columns alike. | [ ] | [ ] | [ ] | New Oct 2026. |
| 218 | Screen readers | With Narrator / NVDA on: the menu read in order with names and roles, the icon as 'Options', no flourish or captions; pressing from the reader presses; N is spoken. | [ ] | [ ] | [ ] | New Oct 2026. |
| 219 | Behaviour trees | The guard patrols A, B, C idling at each; Space's noise sends it to search; the thief in its circle is chased mid-walk and caught; Line 1 names the running leaf. | [ ] | [ ] | [ ] | New Oct 2026. |
| 220 | Particle lights | The dark floor is lit only under the particles - orange sparks, green fireflies, shrinking embers; the lights follow and fade; L turns them off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 221 | Online zone | With `cargo run -p jengine-server --example zones` running: C signs in and joins the meadow; your square is yellow and named, the coins are where the server put them; a click walks you there smoothly (not in steps), a coin you cross vanishes and the count rises; WASD steers you at once (predicted) and the dark blocks stop you - the server's own tilemap, sent to the game; the red monsters come for you once you are near, finding their way round the blocks (the engine's A*, on the server), and a bite takes a coin; off the east edge the ground and the room name change to the cave; X disconnects and everything is hidden. With `--example bots` the others walk about, named. With no server it says offline and keeps retrying. | [ ] | [ ] | [ ] | New Oct 2026. Needs a server. |
| 222 | UI nine-slice | The left panel's iron corners are swollen and soft; the middle and right panels' corners are small and crisp (twice and three times the pixels), with straight wooden edges between them; the bar and the button keep their end caps at every width. | [ ] | [ ] | [ ] | New Oct 2026. |
| 223 | Typed text | Typing shows the characters on the left and the key names on the right; shift + a letter shows the capital on the left and the plain key on the right; `?` shows the green line, whatever the keyboard layout. | [ ] | [ ] | [ ] | New Oct 2026. |
| 224 | Runtime conformance | Says `57 probes, 0 raised an error` (count may grow) and lists each answer. The real check is `node tools/conformance.mjs`, which runs this scene natively and on the web and diffs every line - it should end `0 differ`. | [ ] | [ ] | [ ] | New Oct 2026. |
| 225 | Asset packs | Editor and native from the project folder: `pack 'lab225': installed 100%` and the blue vault with its gem below, the gem spinning and pulsing, pale sparks rising off it, and a green note naming the table it read. A **build** (`jengine windows-build` / `web-build`): `remote 0%`, D counts up to `installed`, the vault appears; R takes it away and says `remote`; restarting the game after a download says `installed` with no download. | [ ] | [ ] | [ ] | New Oct 2026. |
| 226 | Live ops | No service: `live ops: defaults`, a last problem naming the refused connection, "Hello from the game's own defaults", a pale square spinning slowly. With the lab's service running and seeded: `live ops: live`, "Hello from the live channel" (`dev` in the editor), a green shape spinning fast (the weekend event), a square or a diamond, "Faster weekend is on - ends in 59m ..." and "Coming soon starts in 9m ...". A value changed on `/admin/liveops` arrives within five minutes, or at once on F. Restart with the service stopped: `cached`, same values. | [ ] | [ ] | [ ] | New Oct 2026. |
| 227 | Blur focus | Crisp checks and words inside the wandering orange frame, blurred outside it with a short falloff; B turns the blur off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 228 | Paint | Red splats behind the yellow brush on the left floor (droplets thrown the way it moves), dark multiply streaks behind the blue one on the right; stains stay; the square holes stay empty; a click stamps the chosen shape (1/2/3). | [ ] | [ ] | [ ] | New Oct 2026. |
| 229 | Press | Footprints pressed into a plain floor by the blue brush: no colour, only relief - rims lit on the side facing the orbiting lamp and dark on the far side, turning with it; R raises ridges; a click presses the chosen shape. | [ ] | [ ] | [ ] | New Oct 2026. |
| 230 | Unlit particles | In the dark: a lit column of particles barely visible, an unlit one full white, lit grey smoke hiding an unlit green glow where it passes; L lights a lamp and only the lit ones change. | [ ] | [ ] | [ ] | New Oct 2026. |
| 231 | Paint canvas | Orange strokes painted onto a transparent canvas over the checks, rubbed out by the blue brush; sparks rise only where paint is now; left click paints green, right click erases. | [ ] | [ ] | [ ] | New Oct 2026. |
| 232 | Wet paint | The blue brush's trail is shine only, glinting under the orbiting lamp and drying matt in a few seconds; the pink brush sprays speckled paint; a click wets a spot. | [ ] | [ ] | [ ] | New Oct 2026. |
| 233 | Fluid | Blue liquid pours onto the ramp, runs into the tank round the step and levels out as one hard-edged surface; T shows the soft dots; C empties; P stops and starts the pour. | [ ] | [ ] | [ ] | New Oct 2026. |
| 234 | Motion blur | The camera pans along the posts: the yellow block riding with it stays sharp, the posts streak, the blue block streaks along its own path; B compares. | [ ] | [ ] | [ ] | New Oct 2026. |
| 235 | Lens flare | The circling light throws ghosts through the centre, a halo and a streak; the dim one none; L compares, S drops the streak. | [ ] | [ ] | [ ] | New Oct 2026. |
| 236 | Mirrors | The corridor repeats above and below, flipped, the block and the arrow in every copy, each copy bluer; M turns the mirrors off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 237 | Water surface | A swell rolls; the raft rides and tilts on it; the block drops in, splashes (waves both ways) and floats; Space pokes the surface. | [ ] | [ ] | [ ] | New Oct 2026. |
| 238 | Sprite fills | Three squares fill round (clockwise from the top, anticlockwise from the right, a checker cut with its texture), four bars fill from each side, 0 to 1 and back. | [ ] | [ ] | [ ] | New Oct 2026. |
| 239 | Post volumes | Panning left the swatches go grey-blue, right warm and saturated with a vignette, fading in over the blend; the middle is neutral. | [ ] | [ ] | [ ] | New Oct 2026. |
| 240 | Pixel simulation | Sand piles and after a few seconds the box drops onto it; water fills the basin; F burns the oil to smoke and lights the plank; L's lava glows and boils water to steam. | [ ] | [ ] | [ ] | New Oct 2026. |
| 241 | Smoke | Smoke rises, parts round the beam and pools at the top; the sweeping paddle pushes it aside and leaves a wake; F's fan blows it left; P puffs. | [ ] | [ ] | [ ] | New Oct 2026. |
| 242 | Rewind | Space throws the ball into the stack; holding R plays it backwards, the boxes flying back up; letting go goes on from there; G holds; 1/2/3 change the speed. | [ ] | [ ] | [ ] | New Oct 2026. |
| 243 | Wall occlusion | Listen: inside the room the chimes are clear and reverberant; outside with the door shut quieter and dull, through the red wall duller still; D opens the door. | [ ] | [ ] | [ ] | New Oct 2026. |
| 244 | Fog | The lamp glows in the fog past its reach; the posts' shadows are dark shafts that swing with it; the fog billows, drifts and greys the far floor; F/S/N change density, scatter, billows. | [ ] | [ ] | [ ] | New Oct 2026. |
| 245 | Wave function collapse | The map fills cell by cell, every corridor meeting its neighbour's, none off the edge; Space remakes it; 1/2 remake a half, which joins the other along the seam. | [ ] | [ ] | [ ] | New Oct 2026. |
| 246 | Vision | A fan sweeps round, cut sharp by every box and the circle; the circling dot is SEEN only inside it with nothing between; W sees all round. | [ ] | [ ] | [ ] | New Oct 2026. |
| 247 | Day Cycle | The sky goes orange, blue, red, black in a day of 24 s; the sun disc crosses and the posts' shadows swing under it; stars and moon at night; 1-4 jump, the dawn and dusk counts go up. | [ ] | [ ] | [ ] | New Oct 2026. |
| 248 | Floating Origin | The dot races past the posts; its x falls back near 0 about once a second while the true distance grows; posts, puffs and camera never jump; O turns it off. | [ ] | [ ] | [ ] | New Oct 2026. |
| 249 | Time Scale | Three dots: at a quarter the white crawls, the blue keeps its speed, the orange goes at half; 3 stops all; with A the hum and beep drop in pitch. | [ ] | [ ] | [ ] | New Oct 2026. |
| 250 | Cast Shadow | Three posts' shadows swing from long east through a footprint to long west as the sun crosses; L: the lamp throws them away from itself; T lays them flat. | [ ] | [ ] | [ ] | New Oct 2026. |
| 251 | Auto Normal | Three flat shapes rounded at their edges as the lamp circles - near rims bright, far rims dark, the ring's hole rimmed; N flat, B bevel shape. | [ ] | [ ] | [ ] | New Oct 2026. |
| 252 | Tile Light | The circling torch's light spills through the gap and round corners, the sealed pocket stays black, wall faces glow and fade into the rock; L, D, R, B change it. | [ ] | [ ] | [ ] | New Oct 2026. |
| 253 | Trajectory | The blue line shows the ball's flight off the pegs as the aim sweeps; thrown, its red trail runs down the blue line, and a gold line shows the crate's path when it will be hit; V hides it. | [ ] | [ ] | [ ] | New Oct 2026. |
| 254 | Voronoi Shatter | A ball strikes each pane: the left breaks into its grid, the right into shards small round the impact, each showing its part of the picture; they fall and the scene resets. | [ ] | [ ] | [ ] | New Oct 2026. |
| 255 | Save States | Balls bounce off a turning paddle; each load puts them and the paddle back as they were at the save, moving as they were, and the grey ball dropped in after the save is gone. | [ ] | [ ] | [ ] | New Oct 2026. |
| 256 | Text Path | Gold words stand on a moving wave, each letter turned to it; blue words scroll round a ring; a plain line of the same font for comparison. S, W. | [ ] | [ ] | [ ] | New Oct 2026. |
| 257 | Gestures | Tap, double tap, hold, drag, flick, pinch, twist (or wheel) are each logged and move a card. | [ ] | [ ] | [ ] | New Oct 2026. |
| 258 | Depth of Field | The card at the focus z is crisp, the others soft by distance; no soft-behind bleed over a sharp card. 1-3, R, F. | [ ] | [ ] | [ ] | New Oct 2026. |
| 259 | Beat Clock | One box per beat, the first gold on each bar; the bar shrinks every beat; counts go up. Up/Down, 3/4, P. | [ ] | [ ] | [ ] | New Oct 2026. |
| 260 | Stateful GPU Particles | Orange (state) bounces, settles, is pushed, trails; blue (closed) does not. M, P. Web on WebGL2: both closed. | [ ] | [ ] | [ ] | New Oct 2026. |
| 261 | Displacement Field | Blades lean away from the pusher on both sides and stand again behind it; back rows sway. M. | [ ] | [ ] | [ ] | New Oct 2026. |
| 262 | Smooth Terrain | A smooth surface; digging makes a round pit the balls fall into, building a mound. Mouse, A. | [ ] | [ ] | [ ] | New Oct 2026. |
| 263 | Fog of War | Black but round the scout and the torch; left land dimmed; red enemies only while seen; R turns the whole map black and 0% again; C coarsens the cells (unseen again). Mouse, S, R, C. | [ ] | [ ] | [ ] | New Oct 2026. |
| 264 | Dialogue | Typing line; Space finishes then advances; picks counted; third choice after two picks; 'finished'. Space, 1-3, R. | [ ] | [ ] | [ ] | New Oct 2026. |
| 265 | Drag and Drop | Ghost + tint; red stacks to 5; swaps; blue-only refuses others; arrows + Enter carry; D uses UI.drop. | [ ] | [ ] | [ ] | New Oct 2026. |
| 266 | Sprite Stack | Three hued towers; camera turn keeps them upright; lower one in front; S spins. Left/Right, S. | [ ] | [ ] | [ ] | New Oct 2026. |
| 267 | Mode 7 | Checkered floor to the horizon in perspective; posts shrink with distance, near over far. Arrows, A. | [ ] | [ ] | [ ] | New Oct 2026. |
| 268 | Palette | Ramps in only the palette's colours, dithered between; 6 cycles the custom two; 0 off; D dither. | [ ] | [ ] | [ ] | New Oct 2026. |
| 269 | Silhouette | Disc behind a brick wall: cyan outline over the brick, itself through the windows. Mouse, A. | [ ] | [ ] | [ ] | New Oct 2026. |
| 270 | Sprite Tilt | Flat, turned, tipped and a driven card; grid foreshortens, Space shows the blue back. Arrows, Space, A. | [ ] | [ ] | [ ] | New Oct 2026. |
| 271 | Afterimage | Three arrows: tinted copies, flat additive copies, copies only while dashing. Space stops / starts, D dashes. | [ ] | [ ] | [ ] | New Oct 2026. |
| 272 | Paint and Halftone | A test card: paint flattens the noisy block and keeps its edge; halftone dots grow toward black. 1-4, Up/Down. | [ ] | [ ] | [ ] | New Oct 2026. |
| 273 | Scene Transition | Each key reloads the scene as its other side (A blue / B orange) through a style; A cycles them. 1-6, A. | [ ] | [ ] | [ ] | New Oct 2026. |
| 274 | Slice | A box, a disc and a wedge on a floor; drag to cut them (or wait for automatic cuts); halves show orange rings inside. R resets. | [ ] | [ ] | [ ] | New Oct 2026. |
| 275 | Portal | A facing right, B facing up; each window shows the other's marks, turned; the ball goes into A and out of B. Space, C. | [ ] | [ ] | [ ] | New Oct 2026. |
| 276 | Jelly | A plain block and a jelly dropped on a floor: the jelly squashes and wobbles, and a poke dents it. D, Space. | [ ] | [ ] | [ ] | New Oct 2026. |
| 277 | Off-screen Marker | Three far dots, an arrow each at the edge with its distance, a ring icon over a dot in view. Arrows move, A wanders. | [ ] | [ ] | [ ] | New Oct 2026. |
| 278 | Line Boil | Ruled lines, a circle and world text wobbling a few times a second; the captions still. B, 1-3, Up/Down. | [ ] | [ ] | [ ] | New Oct 2026. |
| 279 | Radial Menu | Eight numbered sections round the middle, the one pointed at grown and gold, a click picks it, Q hides it. | [ ] | [ ] | [ ] | New Oct 2026. |
| 280 | Lens Rain | A grid of squares behind beads and running drops that bend it; mist (M), refraction (R), amount and size. | [ ] | [ ] | [ ] | New Oct 2026. |
| 281 | Speech Bubble | Three close speakers' bubbles stack without touching; one at the edge stays on screen; a long line wraps. Arrows, Space, T. | [ ] | [ ] | [ ] | New Oct 2026. |
| 282 | Sprite Outline | Two stars ringed round their points and their holes, the middle one breathing; a sheet cell ringed as its disc, not its neighbour's square. Arrows, C, O, R. | [ ] | [ ] | [ ] | New Oct 2026. |
| 283 | Lightning | New bolt each strike, flickering as it fades; the floor and posts lit near it, the room lifted, then dark. Click, A, B, C. | [ ] | [ ] | [ ] | New Oct 2026. |
| 284 | Flow Path | Chevrons run along the channel and round its bend; tagged floats carried round it, faster mid-stream; untagged ones stay. Arrows, R, Space. | [ ] | [ ] | [ ] | New Oct 2026. |
| 285 | Snow Cover | Snow on the steps' tops, not their risers; more on the roof's shallow side; a cap on the post only. S, M, Space, Up/Down. | [ ] | [ ] | [ ] | New Oct 2026. |
| 286 | Nav Links | The runner crosses the gap only by a link - the jump both ways, the chute west to east only - growing as it jumps; with both off it waits at its edge. Click, J, C. | [ ] | [ ] | [ ] | New Oct 2026. |
| 287 | Path Morph | The big shape eases into the one chosen, outline and colour, never twisting; back overshoots and settles. 1-4, E, Up/Down. | [ ] | [ ] | [ ] | New Oct 2026. |
| 288 | Toasts | Each toast slides in from its corner's edge, the newest nearest it; three at most, the rest waiting; out after three seconds; long text wraps. Space, 5, L, C. | [ ] | [ ] | [ ] | New Oct 2026. |
| 289 | Input Replay | R records from a fresh scene, P plays it back: the dot retraces its run, arrows do nothing meanwhile, and the recorded and replayed ends MATCH. S stops playback and hands the dot back. | [ ] | [ ] | [ ] | New Oct 2026. |
| 290 | Mouse Delta | Move the mouse: the dial spins and the ring moves by the MOTION; push past the window edge and they keep going while "position" stops. L locks the pointer (hidden; a browser needs a click first, Esc lets go) and they still move. | [ ] | [ ] | [ ] | New Oct 2026. |
| 291 | Sprite Size | LEFT: the blue disc breathes and its yellow child stays the same size (Sprite.set_size). MIDDLE: the red disc breathes by scale and its child grows with it. RIGHT: the green square changes width and height apart. | [ ] | [ ] | [ ] | New Oct 2026. |
| 292 | Tileset spacing | Two maps of the same two tiles (sand rows over blue rows), linear-filtered at 3x. Left (packed): a thin line of the other colour down every seam. Right (`margin` 2, `spacing` 4, gutters of each tile's edge): clean seams, no magenta, no tile shifted. | [ ] | [ ] | [ ] | New Oct 2026. |
| 293 | Tilemap parent, mesh tint | Left: the grass map, its two-cell-tall trees (standing on their cells, rising into the row above, not squashed) and the yellow flag all sway together with the raft; nothing ever sits in the red frame. Right: the checkered mesh cycles colour and swaps behind / in front of the red card once a second. | [ ] | [ ] | [ ] | New Oct 2026. |
| 294 | Tracker songs | 1 plays Sunny Fields at once (no hitch on the key); the readout climbs to 48.00 s, then jumps back to 6.86 s, not 0 - the intro is heard once - and "loops heard" counts 1. 2 crossfades to Deep Cavern over 2 s (loops 50.00 s back to 10.00 s). 3 plays the victory jingle over the music, once. Same notes and tempo in every runtime. | [ ] | [ ] | [ ] | New Oct 2026. Needs jengine-tracker in the viewport. |
| 295 | Web platform | The readout names the portal: "none" natively, in the editor and in a plain web build. 3 and 4 list "ad finished: midgame/rewarded, shown false, rewarded false" at once; 1, 2, 5, 6 and 7 only echo. In a build made with `--platform <portal>` the readout names it and the portal's SDK answers (locally: Poki and CrazyGames test ads, YouTube nothing) - the game freezes and goes quiet while an ad plays. | [ ] | [ ] | [ ] | New Oct 2026. docs/web-platforms.md. |
| 296 | 3D stage | A checkered floor and three boxes in perspective; turning (Left / Right, or A) hides the faces turned away and shows the others. Posts and the walker shrink with distance and draw in front of what they stand before, never under the floor; the lamp's pool is an ellipse. Q / W change the perspective; D toggles depth of field (the walker sharp, near and far soft). Each box throws one dark shadow away from the walker's lamp, sweeping round as the walker circles; Z toggles a blue haze thickening into the distance. | [ ] | [ ] | [ ] | New Oct 2026. Visual Showcase 2. |
| 297 | Mesh normals, squashed lights | The trapezoid (a mesh) and the square (a sprite) show the same bumps lit from the circling lamp; N flattens both. S squashes the pool into an ellipse half as tall as wide. | [ ] | [ ] | [ ] | New Oct 2026. |
| 298 | Script maths | The summary counts the numbers and the first lines show `name = mantissa p exponent`; the last probes are whole numbers past 2^31 and numbers' text; nothing reads ERROR. The real check is `node tools/conformance.mjs --scene scenes/298-script-maths.scene`: every line the same natively and on the web. | [ ] | [ ] | [ ] | New Oct 2026. Determinism phase 3. |
| 299 | Fixed steps | "update calls a second" reads about 20 at any frame rate (the scene sets `"simulation_hz": 20`); tapping Space fast counts every tap once; the ball, moved only in `update`, glides rather than jumping 20 times a second. | [ ] | [ ] | [ ] | New Oct 2026. Determinism phase 4. |
| 300 | Lockstep | With `cargo run -p jengine-server` running and two copies pressing 1: both show both balls in the same places at every moment, each moved by its own player (keys, the stick by how far it is pushed); the mouse places each marker. Close one copy: the other says it left and plays on with that ball still; start it again and press 1 - "catching_up", then its seat back. A third copy pressing 2 watches. Esc while waiting leaves. | [ ] | [ ] | [ ] | New Oct 2026. Determinism phase 5. |

## What a sweep is not

It is not a substitute for the automated suites, and anything here that becomes
testable should move there and be deleted from this list rather than ticked
forever. `render_golden` already pins the pixels for sprites, text, `.gui`
documents and entity UI; `engine_test_lab_scenes` proves every scene parses,
loads, is reachable from the hub and compiles under Luau. Neither can tell you
that a shadow looks wrong or that a drag feels detached from the cursor.
