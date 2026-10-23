# Architectural debt

Structural problems in the engine, as opposed to individual bugs. Each entry
says what the shape is, what it costs, and what fixing it would involve.

Written Sep 2026 after a performance pass on `61-swarm` (16,000 flocking
entities) turned up several issues that were not local hot spots but
consequences of a design decision. Individual fixes are logged in
[`projects/engine-test-lab/BUGS.md`](../projects/engine-test-lab/BUGS.md);
this file is for the ones too broad to be a bug entry.

Ordered by value-for-effort as of writing.

**Where things stand (Oct 2026).** Re-ranked after profiling web builds of the
visual showcase with function names kept (Chrome's sampling profiler over CDP;
`wasm-pack build --profiling --no-opt` keeps the `name` section, which
`wasm-opt` otherwise strips). In the order to do them:

1. **#2, the hand-copied hosts** - the largest payoff: still the largest
   source of bugs, ~20,000 lines across the three hosts when this was
   written. Done since: the browser hosts' script mirror, command dispatch
   and script instances, and every host's frame drawing (`ecs::frame_draw`).
   The frame tick's runs that agree are shared too (`tick_combat_and_animation`,
   the physics frame, `pose_world_for_draw`), and the order is one since
   Oct 2026 (the browsers', native rearranged; `frame_order_is_the_same_everywhere`).
   Installing a scene's scripts is shared too (`scripts::attach`,
   `run_inits`, `forget_scene`). Left on purpose: the **input listeners**,
   which differ in policy rather than spelling - the editor takes input only
   while playing and keeps its own shortcuts and no touch; the web keeps keys
   out of page fields, prevents defaults, resumes audio on a gesture and
   toggles the overlays - so a shared function would be all parameters.
2. **#1, string identity** - re-measured in a browser it was several times
   what the native number said. **Done for textures and materials** (Oct
   2026, `asset::AssetId`): `memcmp` went from ~5.5% of a Ten Thousand Lights
   frame to nothing.

#9 (the sprite pass collecting once per lighting layer) was added and then
measured down: see its entry before picking it up.

#3 is mostly paid (Oct 2026), #4 and #7 are closed, #5 is niche, #6 is a rule,
and #8 is fixed.

**[Needs verification in a running editor](#needs-verification-in-a-running-editor)**
is at the bottom: work that is compiled and tested as far as a test here can
reach, but that nobody has watched run. Check it before trusting it.

---

## 1. Identity is `String` everywhere

**Shape.** `texture_id`, `material_id`, `sort_layer`, `clip_id` and `font_id`
are owned `String`s on components the renderer touches every frame.

**Cost.** Every render path buckets drawables by texture and material, so it
clones and compares strings per drawable per frame.

**Partly paid down (Sep 2026).** Three paths now borrow instead of cloning:
`render_sprites_wgpu_multi_materialized`, `render_lit_sprites_wgpu` (the forward normal-map pass, replaced by `render_normals_wgpu` in Sep 2026), and
`render_sprites_with_shadows_wgpu_materialized` — the last being the viewport's
**main** sprite path, which ran unconditionally whether or not any
`ShadowCaster` existed. Each had been cloning a whole `SpriteComponent` (an
owned texture id plus an optional sort layer), allocating a second `String` for
the resolved texture id, doing a random `MaterialComponent` lookup per entity,
and cloning that too. `InterleavedItem` gained a lifetime so it can hold
`&SpriteComponent` and `&str`. Sort keys are resolved once up front rather than
twice per comparison.

**Allocation half is now fully paid (Sep 2026).** The last two sites —
`render_shadows_wgpu` and `render_skinned_meshes_wgpu` — borrow as well:
their draw items carry `&'a str` and the query is bound to a local so the
borrow outlives the collection. No render path clones a texture id any more.
Doing so also made `render_shadows_wgpu` resolve the empty texture id to the
white pixel like every other path already did, so an untextured shadow caster
now draws instead of being silently discarded by the batch.

**Interning: measured, and not worth it.** The remaining cost is the `String`
comparison in the bucket sort. `tests/sort_key_cost.rs` sorts the same
drawables by string key and by `u32`:

| sprites | string ms | int ms | saved ms |
|---|---|---|---|
| 2,000 | 0.164 | 0.051 | 0.113 |
| 8,000 | 0.343 | 0.195 | 0.149 |
| 16,000 | 1.051 | 0.322 | 0.729 |

0.73 ms at 16,000 sprites, against a frame that measures ~16 ms at that count —
about 4%. The change that buys it touches every component field, the scene
format's serde, every render path, every script setter and the inspector, and
the two representations cannot coexist mid-refactor. That is wider and riskier
than the extraction in #2, which was backed out for being too large to verify
in one step, for a fraction of the payoff.

**Recommendation at the time: leave it.** Revisit only if a profile shows the
sort itself dominating — which needs either far more sprites than the engine
currently targets, or many more distinct texture ids than a real scene has.
This entry stays as a record of the measurement so it is not re-proposed from
first principles.

**Revisited in a browser (Oct 2026): it is worth doing, after #2.** The numbers
above are native. In a web build of the showcase's Ten Thousand Lights (10,000
sprites), comparing texture ids - the sort, then the batching walk asking
whether the next item shares the current texture - was **15-20% of the
frame**: `memcmp` alone was ~10%, because the lanterns share a z and nearly
every comparison falls through to their paths, which share a long prefix. Two
cheaper ideas were tried and backed out, and they say what the fix has to be:
caching each item's sort key (`sort_by_cached_key`, then an index sort) was
slower than the stable sort, which finds the lanterns already in runs; and
ranking the distinct texture ids once per frame cost what it saved, since it
hashes ten thousand strings to save comparing them. **Only ids interned before
the frame pay** - the `TextureId(u32)` design below. Do it after the browser
hosts are one (#2), because script setters and texture loading in each host
are among the sites it touches.

**Done for textures and materials (Oct 2026).** `asset::AssetId`
(`asset/asset_id.rs`; `TextureId` and `MaterialId` are aliases) is a `Copy`
handle - an index and a `&'static str` - interned in a process-wide table when
it is made, so every sprite, mesh, trail and material naming one texture holds
the **same** string. It reads, writes and serialises as that string, compares
with `&str` and `String` both ways, and orders as its text, so no draw order
moved and `render_golden` is byte-identical. The sort and the batch walk
compare through `same_str` / `cmp_str` (pointer first, `memcmp` only for two
different ids), wrapped in `wgpu_systems::TexKey`. It differs from the design
below in one way on purpose: the key stays a `&str` that is free to compare,
rather than a bare `u32`, because carved copies, canvas-group targets and a
font's atlas are ids made at runtime as `&str` and sort beside the interned
ones.

Measured on the same Ten Thousand Lights web build (a `--profiling --no-opt`
build, runs alternated), `memcmp` went from **4.2-5.8% of busy time to 0.0%**
and the frame rate from 64-67 to 72-75. Interning the texture alone moved nothing:
every lantern also carries a `material` named `default_sprite`, an owned
`String` each, and the material compare fell through to `memcmp` on every
pair - so the material id had to be interned too. What is left of the sort is
building the key itself (`sort_key`, ~5%), not comparing it.

Still `String`: `sort_layer`, `clip_id`, `font_id` and the other ids nothing
compares per drawable per frame. Intern one with `AssetId` the day a profile
shows it.

**Design, kept for reference.** Intern ids at load time into handle types
(`TextureId(u32)`, `MaterialId(u32)`), keeping the string only for authoring,
serialization and error messages. Components store handles; `AssetManager` owns
the interner and the reverse map. Bucketing becomes an integer compare and
sorting becomes trivial.

---

## 2. Four runtimes, one behaviour, copied by hand

**Shape.** `jengine-editor-viewport` (7.3k lines), `jengine-web/script_host.rs`
(3.3k) and `jengine-runtime/wgpu_scene_runner.rs` (2.9k) each implement the
same frame: hydrate scripts, dispatch commands, tick systems, write back.
**Oct 2026: 9.0k, 7.0k (`lib.rs` 3.7k + `script_host.rs` 3.3k) and 4.0k** -
nearly 20,000 lines, grown by every feature that needed a host. Each fix in the
Oct 2026 performance pass (names, velocities, the spawn map's reversed views)
had to be written once in core and then threaded into both browser hosts by
hand, which is the cost this entry describes, met again.

**Cost.** This is the single largest source of bugs in the repo, and it is not
close. Adding one script function means editing ~9 sites (CLAUDE.md documents
the checklist). During one working session the same logic had to be patched in
two or three files at a time for: script-namespace hydration, the writeback,
two new command arms, the package host, prefab expansion, and the sprite
texture fallback. Every one of those was an opportunity to fix two hosts and
forget the third.

The existing guards (`script_global_parity`, `preamble_command_tags`) compare
**names**, not behaviour — they cannot catch a viewport arm that does something
subtly different from the web arm.

**Guarded, not fixed (Sep 2026).** `tests/host_command_parity.rs` asserts that
both hosts dispatch the same tag set, and that every tag the preamble emits is
handled by both — with a `KNOWN_HOST_ONLY` list that must state a reason and is
itself checked for staleness. Measuring the drift first was worth it: the two
hosts turned out to be **108 and 107 arms with only `dd` and `lds` differing**,
so they had not actually diverged; there was simply nothing keeping them that
way. The guard was verified by deleting an arm and confirming both tests fail
with a message naming the tag.

That removes the *recurring* bug (add a function, wire one host, ship silence).
It does not remove the duplication, and it cannot see behaviour: two arms with
the same tag doing different things still passes.

### Scoping the extraction (Sep 2026) — attempted and backed out

Measured, so the next attempt does not start blind:

| | |
|---|---|
| command arms per host | **87** each |
| tags both handle | **83** |
| arms identical once comments and helper-call style are normalised | **44** |
| of those, needing nothing but the world | **~21** |
| arms that genuinely differ | **39** |

The 39 differing arms are the real work: each needs a decision about which
host's behaviour is correct before it can be shared. The identical ones can be
moved mechanically — but an attempt to bulk-generate that move hit six
transformation hazards, three of which only surfaced because the compiler
complained:

1. arm bodies written on the pattern line (`"stm" => app.audio.stop(),`) read as
   empty, so they were misclassified as needing nothing;
2. `SCRIPT_ENTITY_MAP` — per-host thread-local state, not the world;
3. the multi-line `app
    .world` form, which a `app.world` replacement misses;
4. `continue` inside an arm means "next command" in the host's loop and has no
   meaning in an extracted function;
5. paths written `jengine_core::` from a dependent crate;
6. imports the arms relied on being in scope at the host.

Each was catchable. The concern is the ones a compiler *cannot* catch, in a
change that no test in this repo exercises — these arms only run in a browser,
and the parity guard checks tags, not behaviour.

**Recommendation at the time:** do this with the editor open, a few arms at a
time — not as a bulk transformation. The guard above already prevents the bug
that made this debt urgent, so the remaining value is tidiness, and tidiness is
not worth a silent regression in 21 script commands.

### The 21 are now shared, and tested (Sep 2026)

That recommendation turned on "a silent regression in code no test exercises",
and the second half of it was the part worth attacking. Nothing tested those
arms because they live in `wasm_bindgen` functions, take a `js_sys::Object`,
and only run in a browser.

So they were not moved as-is. `jengine_core::scripting::host_commands` holds
them behind a `CommandArgs` trait — the only thing they needed a JS object for
was reading a few named fields. What is left is ordinary Rust over a
`hecs::World`, which a native test calls directly. **22 tests now cover these
commands, which had none.** They caught nothing during the move, but they pin
behaviour that was previously defined only by two hand-copied bodies:
`set_health`'s non-positive-max convention, `set_text`'s world-then-UI
fallback, `set_ik_target`'s two-bone-then-look-at order, and that `ssm` puts
the state machine back even when the entity has no animator.

Both hosts lost 21 arms (**660 lines deleted, 120 added**) and now call
`handle_world_command` before their own `match`. `JsArgs` and `js_value_json`
are one implementation instead of two byte-identical copies.

The guards grew with it, because the extraction created a new way to fail
silently — a host that stops calling the shared dispatcher would drop 21
commands while every tag-level check still passed:

* `both_hosts_call_the_shared_dispatcher` — verified by breaking the call and
  watching it fail;
* `neither_host_shadows_a_shared_command` — a leftover private arm would be
  unreachable, and would read as the live implementation;
* `every_shared_tag_is_one_the_preamble_emits` — a shared tag no script can
  call would silence the missing-command check for that name;
* `every_advertised_tag_is_actually_handled` (in the module) — `SHARED_WORLD_TAGS`
  is what the hosts trust to skip their own arm, so a tag listed but unhandled
  would make the command vanish.

**What did not move, and why.** Arms needing audio, tweens, the save system or
the scene stack stay in the hosts — they need host state, not just the world.
So does anything touching `SCRIPT_ENTITY_MAP`, which is per-host and hides
inside the innocuous-looking `find_entity_by_id`; that is why
`ScriptWorldHost::resolve_entity` takes the resolver from the host rather than
scanning.

### Auditing the "39 differing" arms found seven real bugs (Sep 2026)

The 39 were the stated remaining work: arms whose two versions differ, each
needing a decision about which behaviour is right. Reading them side by side
turned up **seven genuine divergences**, all of the same shape — a script
function that works in the runtime you develop in and does something else, or
nothing, in the one you ship. `BUGS.md` #41–#47.

Two things are worth carrying forward from doing it.

**The count was inflated.** Most of the 39 were whitespace and path
qualification; several differed by a single space after normalisation. The
figure came from a normaliser that was not normalising enough — the same
measurement sloppiness that produced "54 N+1 sites" and "six parity gaps"
earlier in this document. Sorting the arms by similarity and reading from the
bottom found every real bug in the first ten.

**Three of the seven were one bug in four places.** The viewport passed the
script's entity handle straight to the physics backend, which keys bodies by
`entity.id()`. Those are only the same number by coincidence — `spawn_entity`
and `spawn_prefab` return a script-allocated index. Fixing three and hand-listing
them in a guard missed the fourth (`set_velocity`); the guard now scrapes the
entity-keyed methods out of `PhysicsWorld`, so the next one is covered before
it is written.

Three more arms (`sprt`, `smu`, `scor`) turned out to be world-only and moved
into the shared module, which resolved their differences by construction rather
than by choosing a winner. **24 shared commands, 30 tests.** What remains is
genuinely host-coupled: audio, tweens, saves, the scene stack.

### The script mirror is one implementation (Oct 2026)

Measured before moving, as the last attempt taught: of the functions both
browser hosts define, `sync_world_to_play_namespace` (~500 lines each) was
86% identical by line and the same code once formatting was set aside;
`sync_play_namespace_to_world`, `publish_physics_state`, the dirty lists and
the JS conversion helpers were identical bar their signatures. The one real
difference was a parity gap (the web build never published a `.gui` node's
scroll offsets - `parity-gaps.md`). All of it is
`scripting::bridge::adapters::js::mirror` now, generated from the editor's
copy so the moved code is the code that ran: a host lends a `PlayWorld` (the
seven fields it reads, under the hosts' own names) and keeps a two-line
wrapper, so no call site changed. **The two hosts lost 2,057 lines** for a
1,150-line module. The source-scanning guards that asked "does this host
publish X" now count `mirror.rs` as part of a host only if the host calls it,
and `both_browser_hosts_use_the_shared_mirror` fails a host that stops.
Checked in Chrome on four showcase scenes, not only compiled: the published
names, transforms, velocities and `.gui` state, and no console errors.

**Next: the command dispatch, measured (Oct 2026).** `drain_play_commands_into`
(~1,100 lines each) dispatches 64 tags both hosts share; **29 arms are
identical** once formatting and paths are normalised, and the editor has four
of its own (`cup`, `dd`, `lds`, `sppp` - by the scan; the web host's
`sppp` / `cup` follow a blank line the scan misread, so check by hand). Read
side by side, the differing arms so far differ in *where a host keeps
things*, not in what they do: the post-process stack is `app.post_process`
on the web and `app.gpu.post_process` in the editor, and the web host guards
physics behind its own `physics` feature while the editor always has it. So
the shape of the next step is a small trait a host implements to hand the
shared dispatcher its post-process stack, physics world and audio - then the
29 move mechanically and the rest one at a time, each read before it moves.

**The 29 moved (Oct 2026).** `adapters::js::host_dispatch` holds them -
audio, tweens, saves, input bindings, particles bursts, fades, time scale, UI
creation - generated from the web host's copy, each arm with its comment
block. A host lends a `HostParts` (its fields, by their own names, plus its
spawn-map lookups as function pointers) so the arms moved verbatim; a
`continue` that meant "next command" became `return true`. The hosts lost
872 lines for 66. `HOST_DISPATCH_TAGS` lives in `host_commands`, where native
tests read it, and `host_command_parity` treats it like `SHARED_WORLD_TAGS`:
counted as handled by a host that calls the dispatcher, no host may keep a
dead arm for one, and `the_host_dispatch_list_matches_its_arms` holds the
list to the arms both ways. Checked in Chrome by calling the page's own
preamble functions over CDP and reading what they did - a tween moved its
entity, a rebinding was published, a save reached storage, the time scale
quartered `dt` - the same as the build before the move. Left: `evt` (calls
the host's script instances) and the 36 differing arms.

**The physics arms moved too (Oct 2026)** - `sv`, `af` / `ai` / `sav`, `sbt`,
`sg`, `sms` / `rj`, `aj`, nine tags. They differed only in spelling and in
one real way: the web host read a missing entity id as `0` and pushed entity
0, the editor ignored it; the shared arms are the editor's. `HostParts`
gained the host's physics world (`Option`, so a web build without the
`physics` feature passes `None` and the tags are consumed quietly - core's
stub `PhysicsWorld` makes the type the same either way) and a `warn` sink
(the editor's console panel, the browser console). The hosts lost 326 lines
for 139. Checked in Chrome on Baggage Claim against the build before the
move: `set_gravity`, `set_velocity` and `apply_impulse` on a spawned bag gave
the same velocities, with no warnings. Left: 27 differing arms.

**Then post-process and debug draw (Oct 2026)** - `sppe`, `sppp`, `ddl`,
`ddr`, `ddc`. The post arms differed only in where the stack lives
(`app.post_process` / `app.gpu.post_process`, lent as an `Option`). Debug
draw differed in what it kept: the web parsed each shape into a
`DebugDrawCommand` and drew it, the editor forwarded the raw object as JSON
to its overlay - so a shape drawn without a colour was white on the web and
`rgb(NaN, ..)`, no stroke, in the editor. Both parse now, through one arm;
the editor keeps `DebugDrawCommand`s and turns them into the overlay's JSON
when it drains them (`debug_command_json`, which the F4 outlines already
used). The editor's `"dd"` arm, which nothing emitted, went with it. The
module is gated on `gpu-wgpu` too: a package checked alone for wasm (core
with `web` and no renderer) had not compiled since the first move, for the
fades. Moving `sppp` also showed that `preamble_arms_are_handled` read
`ut: 'f'` as a tag `f`, satisfied only by the hosts' inner `"f" =>` arms.
Checked in Chrome against the build before: debug lines, a colourless line
drawn white, and a colour grade greying the frame, measured on the canvas.
Left: 22 differing arms.

**Nine more (Oct 2026), and what they hid.** Five needed only the world -
`sc`, `at` / `rt`, `smat`, `stl`, `gs` / `gh` - so they went to
`host_commands`' world arms, where `browser_world_commands` drives them
natively with `JsonArgs`; four more (`ssr`, `goc`, `svcm`, `svcl`) to
`host_dispatch`, which lends the host's `assets` and `gui_click_callbacks`.
Their differences were formatting, paths and where a warning goes - except
`sc`, whose value conversion turned a table into null in both hosts, fixed by
the move. Reading `smat` next to `get_material` found the larger thing: the
preamble's getters read eight `__jengine_play` fields no host had ever
published, and native's animation getters read snapshot maps nothing filled
(`parity-gaps.md`, three rows). The mirror publishes them, native reads the
world, and `every_play_field_the_preamble_reads_is_written` keeps it so.
Checked in Chrome on Lamplit Mine: a cleared and restored tile, a table read
back after the mirror rebuilt, tags. Left: `evt`, `lds` (editor only), and
11 differing arms (`log`, `quit`, `de`, `cmsh`, `spf`, `gsp`, `ps`, `ls`,
`csm`, `scrf`, `se`) - the ones that touch the hosts' script instances,
spawn maps and scene loading.

**Six of those followed (Oct 2026)**: `se`, `spf`, `cmsh`, `scrf`, `ps`,
`gsp`. They differed only in where a warning went; a spawn reaches the
host's spawn map through `note_spawn`, and `HostParts` lends the registry,
the camera-shake queue and `audio_warned` (and `assets` mutably, for
`.gui` documents). The hosts' `spawn_parts` helpers went with `spf`. The
command loop is now the shared world arms, then the shared host arms, then
each host's own: `log` and `quit` (where a log line goes, and what quitting
means in an editor or a page), and `de`, `ls`, `csm`, `evt` - every one of
which calls into the host's `ScriptInstance`s, a type each host defines.
Those are the next step, and a different one: sharing the script-instance
layer itself (compile, `init`, lifecycle handlers, event listeners), not
another arm. Checked in Chrome against the build before: a spawned prefab
and empty entity (and their handles), `gui_set` read back, a played sound
and one warning for a missing clip.

**The script instances are shared (Oct 2026)** - `adapters::js::scripts`: the
`ScriptInstance` type, the spawn map (`SCRIPT_ENTITY_MAP`, a thread-local in
core now, so the hosts' many `.with(...)` sites did not change), compile and
call, a named handler (`call_handler`), `on_animation_event`, the listener
dispatch and `fixed_update`. The copies differed only in where a warning
went, so a host's own function is now a line handing over its instances and
its sink. With those in core, `de`, `ls`, `csm` and `evt` moved to
`host_dispatch` too. The hosts lost about 740 lines between them. What is
left in each host's command loop is `log` and `quit`, host I/O by nature, and
the editor's `lds`. Still per host: installing a scene's scripts
(`install_scripts` / `load_and_run_scripts`, which differ in where sources
come from - fetched on the web, handed over by the editor) and `.gui` click
dispatch. The frame's script phase followed the same day: `scripts::run_frame`
(`on_spawn`, `update`, timers, triggers, contacts), the hosts keeping only
what is theirs around it - publishing the world before, draining commands
after. A probe script attached at runtime to Baggage Claim's belt counted the
same `on_spawn`, updates, timer ticks and collisions on both builds. Then
`.gui` clicks: the hit test was **three** copies, native's included, two of
them laying a document out again for every bound node - one function now,
`ui::layout::click_hits` (a document laid out once a click), which native
tests can call; and the call into the script, `scripts::call_gui_click`.
A sweep of real mouse clicks over Readable Menus found each toggle at the
same two grid points on both builds. Checked in
Chrome on Night Bloom, whose flowers' `on_screen_entered` sends `bloom` to a
driver counting them: the count, a `call` into the driver, `load_script`
and `destroy_entity`, the same as the build before.
`material_pipeline` (60%) differs for real reasons - a missing material is
queued for fetching on the web and requested from the editor in the
viewport - and is not worth a callback for ~40 lines.

**Fix.** Extract a `jengine-host` crate holding the parts that are genuinely
identical: the command dispatch table, the script mirror, asset seeding, the
frame ordering. Each runtime keeps only what is actually platform-specific —
window/canvas, GPU surface, audio backend, file access.

**Effort.** Large, and it needs care: the three have drifted, so extraction
means first deciding which behaviour is correct in each case. Worth doing
incrementally, starting with the command dispatch (the widest and most
mechanical).

### The frame is drawn once (Oct 2026)

`ecs::frame_draw::draw_frame` is what native, the editor viewport and the web
runtime each carried an ~800-line copy of: the views, the world in parts
around `SceneColor` readers, normals, lights, lightmaps, unlit emission, the
composite, the readers over a copy, the post chain, and the UI on the display.
The copies drew the same things in the same order - the differences were
policy, and policy is what a host lends through `DrawHost`: how it finds a
material's graph pipeline (native reads the `.material` from the project, the
web fetches it, the editor asks for it) and what it says about a texture it
has not loaded (each host's loader reads the `warned` set the draw fills).
Host-only extras are arguments: the editor's grid as an `underlay`, the
script and F4 debug shapes the editor sends to its own overlay instead, the
F3 overlay. The three hosts lost ~1,600 lines for a 440-line module.

It was checked by pixels, not just by compiling: 15 showcase scenes covering
lighting, scene readers, views, canvas groups, normal maps, lightmaps,
cookies, GI, split screen and UI, captured in Chrome with time frozen at the
first frame scripts run (two loads of the old build first, to measure the
noise between loads - at most ~1.5 mean difference), and 11 captured natively
through `PrintWindow` against a release build of the commit before. Every
scene differed from the old build by no more than two loads of the old build
differ from each other. The editor viewport goes through the same function,
but was not run here.

The source-reading guards follow the call: `tests/support::with_frame_draw`
inlines the module at a host's `draw_frame(` so a check of what a host draws,
and in what order, reads through it; `every_host_draws_its_frame_through_the_shared_function`
fails a host that forks it again.

---

## 3. The script bridge mirrors the whole world every frame

**Shape.** Browser-script hosts copy every entity's transform into typed arrays
each frame so a script *could* ask about any entity, then apply whatever the
scripts wrote.

**Cost.** After the rewrite this is cheap — three float writes per entity, four
bulk copies per frame, no allocation — but it is still `O(all entities)` for
work that is usually read by nobody. At 16k it is fine; it is the next ceiling.

**Fix.** Answer on demand instead of mirroring: back the accessors with calls
into WASM rather than a prebuilt array. The obstacle is reentrancy — scripts
run inside a `with_app` borrow, so a callback into the world would panic on a
double borrow. It needs the raw-pointer approach the native bridge already uses
(`BridgeReadSnapshot::world_ptr`), with the same discipline about when the
pointer is valid.

**Effort.** Moderate, and only worth it above ~50k entities. Note the current
design's one real virtue: a script's reads are consistent for the whole frame,
because it reads a snapshot. On-demand access loses that.

**The mirror's other cost is correctness, not speed (Sep 2026).** Indexing by
entity id means an entity that does not exist *yet* has no slot, and the four
setters that write the mirror silently discarded writes to one — so
`set_position` on the line after `spawn_prefab` did nothing at all (BUGS #52).
That is the most natural thing a script can do, and it had been broken in every
browser runtime since the mirror was introduced.

Fixed without touching the design: the setters fall back to a queued command
when the entity is not addressable, and commands drain in order so the spawn has
already happened. The fast path is unchanged. It is worth noting as a property
of the approach rather than a one-off bug — **anything keyed by a dense id
array has this shape**, and the next accessor added to the mirror will have it
too unless it copies the fallback.

**What was around the transforms cost more than they did (Oct 2026).**
Profiled in a web build of the showcase's Ten Thousand Lights (10,000 spawned
lanterns), the mirror was a quarter of a 22 ms frame - and the typed arrays
were not the cost. The hosts rebuilt `entity -> handle` maps from the spawn
table with SipHash three or four times a frame, published every `Name` as a
fresh JS string (a prefab's instances all carry its name), and sent
velocities as a JS array per entity. Now `scripting::spawn_map::SpawnMap`
keeps the reversed views built once per change, `publish::publish_names`
rebuilds the object only when a name or a handle changes, and
`publish::publish_velocities` sends flat `Float64Array`s (by entity id
since, see #6): the mirror went from
~5 ms to ~0.7 ms, and the frame to 9.6 ms. **Measure before reaching for
on-demand access** - it was the per-entity extras, not the shape.

**Measured again, and on-demand still waits (Oct 2026).** After the
materials map and the velocities (see #6), the mirror was ~9% of a Ten
Thousand Lights web frame: the transform arrays ~3%, the sprite-point table
~1.4%, names and the small per-entity maps ~1.5%, the rest scattered. The
transform arrays are already the minimum a copy can be - direct writes into
reused buffers, one bulk copy each - and answering them on demand needs a JS
overlay so a script reads back its own `set_position` in the same frame, plus
a world pointer lent to the browser hosts while scripts run, which the event
and GUI-click paths call with the world mutably borrowed. That is the native
bridge's `world_ptr` discipline repeated in two more hosts, for ~3% at ten
thousand entities: still not worth it below ~50k.

What was worth it: the sprite-point table walked every sprite every frame to
answer `Sprite.get_point`, and most projects have no named points at all.
`AssetManager::any_sprite_points` says so in a walk of the atlases, and the
table is not built (1.4% to 0).

---

## 4. `N+1` component lookups inside iteration

**Largely already clear (Sep 2026) — the original count was misleading.** "54
`world.get` calls" counted every such call in `ecs/`, not lookups inside a
loop. Scanning properly for the N+1 shape found: two genuinely hot ones
(`MaterialComponent` in the sprite path, `Tags` in `update_trigger_areas`),
both since fixed; **zero** in either runtime host; two guarded by a condition
so they run once per entity rather than per frame (`ensure_physics_bodies`);
and about twenty in `save_scene`, which runs on save rather than per frame.

The sweep did turn up a different shape worth having looked for, though:

### The registry probed every type for every entity

`ComponentRegistry::serialize_entity` looped over **every registered
descriptor** — 67 core components plus any package's — asking the world "does
this entity have you?". Cost followed the size of the registry rather than the
size of the entity: snapshotting 16,000 two-component entities took **66 ms**,
4.1 µs each. `push_scene` / `pop_scene` pay that as a hitch mid-game.

Fixed by asking the entity what it carries (`hecs::EntityRef::component_types`)
and looking each type up in the registry's existing `TypeId -> name` map:
**66 ms -> 40.8 ms**. Less than the 10x the lookup count suggested, because
what remains is `serde_json` value construction and a `String` clone per
component — inherent to "serialize everything to JSON", not to the probe.

Guarded by `scene_snapshot_cost.rs`, which requires every component a mixed
entity carries to survive the round trip: enumerating rather than probing means
a component `component_types()` failed to report would vanish from every save
with no error.

**Done (Sep 2026): snapshots skip `BulkEntity`.** 40.05 ms -> **0.35 ms** at
16,000 entities, which is the rest of it.

The hesitation was that this is a semantic change — a game might want its
bulk-spawned projectiles back after a scene push. Resolved by making it
explicit rather than implicit, and by not changing `save_scene_v2` at all:
`snapshot_scene_v2` is a separate entry point used by the four *snapshot* call
sites (play start, scene push, scene pop, scene transition), and a genuine save
still writes every entity. Dropping an entity from a save would be data loss;
dropping one from a transient in-play snapshot is a policy choice.

The policy follows the marker's existing meaning. `BulkEntity` says "spawned in
bulk, not addressed individually", and already governs the script mirror and the
editor's Scene panel for exactly that reason — this is the third thing it
decides, and its doc comment now says so along with the trade. A game that wants
its flock restored across a push simply does not set it.

---

## 5. Systems that assume small worlds

**Shape.** Several systems are `O(n²)` or allocate per entity, which is correct
for the ten-enemy scenes they were written for and wrong at flock scale.

- `apply_separation` — `O(n²)` over entities carrying `Separation`. Documented
  as such. `jengine-swarm` deliberately does not use it; it has a spatial grid.
- `update_trigger_areas` — was `O(all entities)` even with zero triggers, and
  cloned a `HashSet<String>` per entity when any trigger filtered on tags.
  Fixed, but the shape (build a candidate list over the world) still scales
  with entity count once a trigger exists.

**Done, with one part deliberately not done (Sep 2026).**
`jengine_core::ecs::spatial::UniformGrid` is the shared broadphase, generalised
from the swarm's: it derives bounds from the points rather than taking a fixed
play area, and grows the cell instead of the grid when a spread would need more
than 512 cells per axis.

* `apply_separation` now broadphases through it: **53.4 ms -> 5.07 ms at 8,000
  bodies**, with per-body cost rising 10x rather than 67x across the range.
  Results are identical to the all-pairs version, including the deterministic
  nudge for coincident bodies — which needed care, because the original seeds
  that nudge from the *lower index of each pair*, so a naive per-body rewrite
  sends two stacked bodies the same way and they never separate. Guarded by an
  equivalence test against the original implementation, kept in the test file
  as the reference.
* `jengine-swarm` dropped its private copy. Three implementations of one idea
  became one; the swarm measured unchanged (16k: 11.3 ms).

**Tilemap colliders were the worst case of the shape, and are fixed (Sep
2026).** `PhysicsWorld::sync_tilemap_colliders` ran every frame for every
colliding tilemap and decided whether to rebuild by **hashing every tile of
every colliding layer** - then, on any difference at all, removed the body and
rebuilt every collider on the map. Correct for the 868 tiles of a maze-chase
game's map, and ruinous at the scale a builder game needs: for a million tiles
it is a per-frame walk of a million cells to discover nothing moved, and one
pick swing rebuilds the whole level.

Unlike the systems above, the cost here was not `O(n^2)` - it was `O(n)` on
something that changes rarely, which is worse, because the work is pure waste
on almost every frame. The fix is therefore not a broadphase but a **revision
counter**: `TilemapLayer` carries a total and one counter per 32x32 chunk, and
the colliders are grouped per chunk, so an idle frame reads one integer per
layer and an edit rebuilds the chunks it touched. On a 1000 x 1000 map,
release: idle **966 us -> 884 ns**, one-tile edit **966 us + an 11.6 ms
rebuild -> 1.1 us**.

Two things were needed to make it trustworthy rather than merely fast, and
both are the interesting part. `TilemapLayer::set_tile` is the only runtime
path that changes a tile, so the counter is reliable - but `tiles` is a public
`Vec`, so a direct write bumps nothing and leaves the colliders stale;
`tiles_mut()` and `touch_all()` are the honest paths and the field documents
the hole. And a counter alone is not enough: a layer *replaced* rather than
edited has the same name, size and a total of zero, which is exactly what the
physics record last saw - so `LayerRevisions` carries an identity from a
process-wide counter, and a fresh layer can never pass for the one it
replaced. `crates/jengine-core/tests/tilemap_colliders_rebuild_per_chunk.rs`
(10, plus an `#[ignore]`d measurement), scene 94.

**Triggers were measured and left alone.** `update_trigger_areas` looked like
the obvious third caller, but the numbers say otherwise: at 8,000 entities it
costs 278 us with one trigger, 390 us with ten, 1,066 us with fifty — so the
per-trigger *scan* is only ~12-16 us and the ~265 us **candidate build** is the
real cost. A broadphase makes the cheap half cheaper. It would pay off at fifty
triggers over a large world; that is not the shape of the scenes here, and the
honest fix would instead be to avoid materialising candidates for entities
nowhere near a trigger.

**And then that fix was made (Oct 2026).** The triggers' boxes (a circle by
its square) are gathered first, and an entity becomes a candidate only inside
one of them - or, past sixteen triggers, inside their union - so the per-entity
cost is a position read and a box test, and the tag work and the push happen
for the few that can be inside anything. Release, 8,000 entities, this machine:

| | before | after |
|---|---|---|
| one trigger | 204 us | 11 us |
| one trigger filtering on tags | 395 us | 11 us |
| ten triggers | 268 us | 13 us |
| fifty triggers | 637 us | 47 us |

`trigger_area_cost.rs` has the timing (`--ignored`, release) and pins the
answers: triggers far apart, more than sixteen, and an entity exactly on a
circle's edge.

---

## 6. Per-frame JS allocation is invisible until it is not

**Shape.** Not a design flaw so much as a hazard the design invites: the WASM
hosts build JS objects to talk to scripts, and an allocation in a per-frame
path is silent until the collector runs.

**Cost.** A fresh `Uint32Array` for `live_ids` was 32 KB per frame at 8,000
entities — roughly a megabyte a second. It showed up as a regular sawtooth in
the profiler and a heap oscillating between 3.5 and 8.6 MB, with frame time far
above the sum of its parts. Nothing in the numbers said "allocation": the phase
timers looked healthy.

**Fix.** Already applied for the mirror (arrays and dirty lists are reused;
the alias object is rebuilt only when it changes), and
`script_mirror_contract.rs` guards it. The general rule is worth stating: **in
a per-frame host path, allocate once and reuse.** When frame time exceeds the
sum of its parts and the profiler shows a periodic sawtooth, suspect the
collector before suspecting the phases.

**A string crossing into JS is decoded every time** (Oct 2026).
`JsValue::from_str` copies the text out of linear memory and runs it through
`TextDecoder`, so a string published per entity per frame costs that per
entity per frame - in Ten Thousand Lights, the `materials` map (one
`default_sprite` per lantern) was **11% of the frame** in `decode`. An interned
id (#1) is made a JS string once for the life of the page
(`mirror::id_string`, cached by its index): `decode` fell from 12.8% to 1.7%
and the profiling build from 72-75 fps to 84-87. Publish any id the mirror
sends per entity through it.

**Nor is a property write free.** With the strings cached, the same map was
still a fifth of the frame in `Reflect::set` - twenty thousand keys (each
lantern's id and its alias) written into a fresh object every frame, though
no lantern's material ever changes. The map is now republished only when a
fingerprint of its entities, their interned material indices and the spawn
map's generation changes, or when `play.materials` is no longer the object
last published (`published_unchanged`): **84-87 fps to 128-134**. A per-entity
map whose values rarely change wants the same treatment; the others the mirror
publishes are small (`gui_ids`, `sprite_anchors`, the animation maps).

**Velocities go out by entity id** (Oct 2026), as positions always did: a
`Float32Array` with `[vx, vy]` at `id * 2` (and one for angular), NaN where an
entity has none, filled straight from the world into reused buffers and
refilled in place (`publish::publish_velocities`). They were built into a
`HashMap` of every velocity each frame, flattened into `[id, vx, vy]` rows,
and scanned row by row by every `get_velocity` - quadratic for a script that
asks each of its bodies. The physics publish fell from ~12% of a Ten Thousand
Lights frame to ~1%, ~127 fps to ~144.

---

## 7. The editor polls the engine with whole-world serialization

> **Closed on measurement (Sep 2026).** The remaining editor overhead is 0.12 ms
> of a 9 ms frame at 16,000 entities. See "Measured, and now closed" below
> before doing any more work here.

**Shape.** The editor asks the viewport for state by calling an exported
function that builds a JSON string. `viewport_get_runtime_entities` walks every
entity, classifies it, names it, serializes the lot, and the editor
`JSON.parse`s it into a Svelte store.

**Cost.** Measured: a sawtooth in the frame graph once every six frames, worth
~16 ms at 8,000 entities and still visible at 500. Together with the
unvirtualised panel below, this was **68 ms of a 93 ms frame** at 16,000
entities — nearly three times the engine's own work. After both fixes the same
scene runs at 16 ms/frame with ~2.6 ms unaccounted. It is invisible to the
engine's phase timers because the *editor* is the caller — the phases all
looked healthy while the frame was three times their sum. That is the trap
worth remembering: **a frame budget the phases do not add up to means the cost
is outside the phases.**

Mitigated with change detection (return nothing when the list is unchanged) and
by batching the per-entity classification.

**Ask the cheap question first (Sep 2026).** The change detection compared a
hash of the *finished* JSON, so the snapshot was still built in full every poll
in order to discover it had not been needed — four component sets over the
world, a name formatted per entity, and a serde pass. The poll now fingerprints
the world's topology first and returns immediately when it is unchanged:

| entities | snapshot ms | fingerprint ms | ratio |
|---|---|---|---|
| 500 | 0.639 | 0.019 | 34x |
| 2,000 | 2.391 | 0.078 | 31x |
| 8,000 | 11.145 | 0.349 | 32x |
| 16,000 | 23.149 | 0.639 | 36x |

(`jengine-core/tests/entity_list_poll_cost.rs`.) That 0.64 ms at 500 entities
is why the sawtooth was visible in small scenes and not only large ones.

`ecs::topology::topology_fingerprint` hashes what the panel actually shows —
which entities exist (by `to_bits`, so a respawn into a reused id registers),
which component *types* each carries, names, and parents — and deliberately not
component values, so a world that merely *moved* answers "unchanged". The
viewport mixes `authored_entity_count` in at the call site, because it decides
the `spawned` flag, does not live in the world, and every site that sets it
also swaps in a fresh `hecs::World` whose ids restart.

**The rest of the per-frame calls (Sep 2026).** With the entity list handled,
the remaining `viewport_get_*` calls were still building a JSON string each,
every frame. Fixing them needed no new machinery, only asking what each one is
actually for:

* **`get_pending_scene` was throwing an exception per frame.** It returned the
  string `"null"` when nothing was pending; `if (pendingJson)` treats that as an
  answer, `JSON.parse` gives `null`, and reading `.action` off it threw a
  TypeError straight into the surrounding `catch` — every frame of every play
  session. An exception is not a cheap way to say "nothing happened". It
  returns an empty string now.
* **`get_logs` and `get_debug_draws` return nothing when there is nothing**, so
  the editor skips the parse, the loop, and the store write. A script that logs
  every frame is a bug, so empty *is* the case.
* **The profiler and the Inspector poll on a timer, not per frame.** Both are
  panels a human reads. The profiler was parsing a 120-sample history, spreading
  it into `Math.max`, and rebuilding a 120-point SVG polyline 167 times a second
  so that someone could look at numbers changing faster than an eye can focus.
  The engine keeps the history itself, so a slower poll loses no samples — it
  draws the same data less often, and the numbers become legible. The Inspector
  serializes every component of the selected entity through the registry, and
  was doing that per frame for the same reason; a newly selected entity still
  fills the panel at once, only the live refresh is on the timer.

**And a correction, because it was nearly a wasted afternoon.** Scene 63
reported `Frame 6.00 ms` against 0.9 ms of engine phases, and I read that as
5 ms of hidden editor overhead. It is not. `frame_time_ms` is the wall-clock gap
between `requestAnimationFrame` callbacks, so it cannot read below the display's
refresh interval — 1000/167 = 5.99 ms, and the FPS readout beside it said 167.
**The gap was the browser waiting for vsync.** Subtracting phases from a
vsync-bounded frame time produces a number that looks like overhead and is
idle.

The lasting fix is that the editor can now measure itself: the profiler shows a
**CPU** row — wall time actually spent inside the render callback — and the
phase bar is proportioned against that rather than against frame time, so its
leftover segment means the editor's own work instead of vsync idle. Before this
there was no way to see the cost this entry is about, which is why the entry
kept being argued from inference. Judge further work here on **CPU minus the
phase total**, in a scene busy enough that the loop is not vsync-limited.

### Measured, and now closed (Sep 2026)

With the CPU row available, scene 61 at 16,000 boids reads:

| | ms |
|---|---|
| CPU (time inside the render callback) | **9.02** |
| Scripts | 1.20 |
| Physics | 0.10 |
| Update | 5.20 |
| Render | 2.40 |
| phases total | 8.90 |
| **editor overhead (CPU − phases)** | **0.12** |

Frame time was 10.00 ms against a 6 ms refresh interval, so the loop is
CPU-bound there and this is a real measurement rather than vsync idle.

**0.12 ms of a 9 ms frame is 1.3%, and the fix for it — push-don't-poll, a
spawn/despawn delta queue shared between the runtimes — is one of the larger
changes on this list.** So it should not be built. The polling that motivated
this entry cost ~68 ms a frame at 16,000 entities when it was written; it now
costs a rounding error, and the way to spend the next hour is not here.

For the record, the same subtraction before the per-frame work above was
removed was ~2.7 ms (frame 11.00 against 8.30 of phases, also CPU-bound). The
per-frame exception, the JSON and the 167 Hz SVG rebuild were most of what was
left.

**What remains is shape, not cost.** The editor's model of the world is still a
JSON snapshot rebuilt on a timer, and if a future feature needs the editor to
react to a spawn *promptly* rather than cheaply, the delta queue is still the
right answer — it just cannot be justified on performance. Anyone reopening
this should re-measure CPU minus phases first, in a scene busy enough not to be
vsync-limited.

**Fix.** Push, don't poll. The engine knows when an entity spawns or dies; it
could hand the editor a small delta instead of the editor asking for everything
on a timer. The same applies to the other `viewport_get_*` JSON calls in the
render loop (`frame_stats`, `frame_history`, `collider_bounds`).

**Effort.** Moderate, and it interacts with #2 — the delta queue is exactly the
sort of thing that should live in a shared host crate rather than in each
runtime.

### The panel itself does not virtualise

Separately from the polling: `SceneTree.svelte` renders one `<div>` per entity
with six event handlers and a per-row reactive lookup, for however many
entities exist. At 16,000 that was ~68 ms a frame of browser layout against
24 ms of engine work — the engine was not the bottleneck, the DOM was.

**Fixed (Sep 2026).** `SceneTree.svelte` now windows the list: only rows within
the scrolled viewport are rendered, with spacer divs preserving the scrollbar,
plus an overscan of 8 rows so fast scrolling shows no gaps. Below 150 entities
it renders everything as before, so small scenes take no new code path at all.
Row height is fixed at 24px because the windowing arithmetic needs a height it
can compute with rather than measure.

`BulkEntity` filtering and the 2,000 cap stay as a second line of defence:
windowing makes rendering cheap, but the engine still has to *build* and
serialize a list that long every time it changes.

## 8. Nine components can be authored in a scene but are not registered

> **Fixed.** All nine are registered in `core_registration.rs` now (checked
> Oct 2026), and `v1_components_are_registered` fails a `ComponentBundle`
> field the registry lacks (BUGS #151 found the v2 half). Kept as the record.

**Shape.** `ComponentBundle` in `scene/format.rs` has 76 optional component
fields; `core_registration.rs` registers 70 names. The nine in the first list
and not the second:

    anim_state_machine    despawn_out_of_bounds   ik_look_at
    ik_two_bone           light_occluder          skeletal_player
    skeleton_root         skinned_mesh            bone

**Cost, as far as it has been verified.** They load and save fine - the bundle
path handles them, which is why `32-light-occlusion.scene` works and why
shadow casting is genuinely wired end to end. What is *not* verified is the
consequence: registration is step 2 of CLAUDE.md's "Adding an ECS component"
recipe and is what the editor's generic path uses, so the likely effect is that
none of these can be added or edited in the editor's component picker, and that
the registry-driven generic spawn does not know them. The whole skeletal
animation family is in that list, which would mean skeletons are hand-authored
only.

**Found** 2026-09-22, while surveying which rendering features any game
actually uses, ahead of building a lighting-driven game. Not chased at the
time; recorded so it is not rediscovered.

**What fixing it involves.** Probably one line each in
`core_registration.rs` - but the first step is to *check the consequence*
rather than assume it, because a component may be deliberately unregistered
(a `bone` is arguably not a thing you add by hand, and neither is
`skeleton_root` on its own). Expect some of the nine to be correct as they are,
and the exercise to be deciding which. If any are deliberate, say so where the
list lives rather than leaving the next reader to work it out.

**Why it is here and not in `parity-gaps.md`.** That register is about the four
*runtimes* disagreeing. This is one authoring path disagreeing with another,
which is a different axis - and worth noticing that the register has no grade
for it.


## 9. The sprite pass collects the world once per layer

**Shape.** A lit frame draws the world in two layers (`WorldLayer::Scene`,
then `WorldLayer::Emission`), and every runtime calls
`render_sprites_with_shadows_wgpu_materialized` once for each. Each call walks
every drawable, culls it against the view, builds an `InterleavedItem` and
sorts the lot - then keeps only the items its layer draws. A frame with scene
readers (`SceneColor` graphs) cuts the world into further bands, each another
full call. The layer and band decide *which* items draw and *how* (cutout or
not, which blend); none of them changes which drawables exist or their order.

**Cost - measured, and small where it was expected to be large (Oct 2026).**
The estimate that prompted this entry ("the pass runs twice, so most of a run
is saved") assumed lit sprites. Ten Thousand Lights' lanterns are unlit and
additive, which the scene layer rejects in `layer_draw` before culling or
building anything: in its web profile the emission call took **~3.0 s of a
6 s recording and the scene call 0.2 s**. The lanterns are drawn once already.
The saving is real only in a scene of many *lit* sprites that also has unlit
things, where the emission layer re-collects every lit sprite as a cutout -
and no scene here has been measured to need it.

**A blocker the design above missed.** Items borrow components through hecs
`QueryBorrow` guards that are locals of the collecting function, so a
collected frame cannot outlive the call - it cannot be held across the
host's scene pass, its lights and its emission pass. The split needs either a
callback API (collect, then hand the frame to a closure that wraps the host's
whole lit-world block) or items rebuilt from entity lookups. Both are larger
than the moderate effort below assumed.

**A cheaper exact saving, if a scene ever needs it.** The composite reads only
the emission buffer's RGB (`light_composite.wgsl`), and the buffer is cleared
to transparent black, so a cutout - black, alpha-blended - is a no-op until
something emitting has been drawn beneath it. In the emission layer, every
cutout sorting before the first non-cutout item can be dropped before the
sort and the draw.

**Fix.** Split the function into *collect* (query, cull, build, sort - once a
frame per view) and *draw* (walk the sorted items for one layer or band, apply
the layer's rules, flush). The per-layer decisions (`leaves_out`,
`layer_draw`, `is_backdrop`, the z window) move from collection into the walk.
`render_golden` covers the lit and unlit orderings this must keep.

**Effort.** Moderate: one function in core, and three hosts whose two calls
become a collect and two draws.

**Done (Oct 2026), through the shared frame.** Once the three hosts drew
through one `ecs::frame_draw::draw_frame`, the closure API the blocker above
called for was one change: `wgpu_systems::render_world_items` collects a
camera's drawables - queries, culling, items - and hands them to a closure
that wraps the camera's whole lit block, which draws each layer and each
scene-reader band from them with `WorldItems::draw`. What a layer makes of an
item is recorded at collection (`LayerRule`: unlit, the material's blend,
whether it reads the scene, whether it is a shadow) and applied per draw -
cutout, blend, left out - so each layer still sorts its own order; the sort is
the cheap part since `ItemKey`. The emission layer also drops every cutout
before the first thing that glows, the exact saving named above.
`render_sprites_with_shadows_wgpu_materialized` stays as one collection drawn
once, for tests and one-layer callers. Busy CPU in the web profile:

| | before | after |
|---|---|---|
| Ten Thousand Lights | 9,664 | 8,583 (-11%) |
| Crypt (lit, normal maps) | 4,408 | 4,261 (-3%) |
| Clay (lit) | 3,472 | 3,295 (-5%) |

`render_golden` unchanged; web screenshots of seventeen scenes within the
noise between two loads of one build.

**Where the pass's time goes (Oct 2026).** Measured in the same web profile
after the string ids (#1), the mirror (#3, #6) and the velocity publish were
fixed, the sprite pass is **~59% of the busy time** of a Ten Thousand Lights
frame, and almost none of it is the sort any more:

| | share of the frame |
|---|---|
| building and uploading vertices (`VertexArena::push`: `writeBuffer`, and wgpu copying the slice into a JS array) | ~12% |
| queuing sprites (corner arithmetic per sprite) | ~5% |
| each item's sort key (`item_keys`) | ~9% |
| the sort itself | ~1.5% |
| collection (queries, culling, building items) and the walk | the rest |

The sort used to rebuild both items' keys on every comparison and, on the
ties identical sprites are, compare their 64-byte parameter blocks; each
item's key is now built once, as integers (`ItemKey`: textures, materials
and parameter blocks ranked by their order among the frame's few distinct
ones, with no hashing), and the pass sorts `(key, index)` pairs instead of
moving the items. That is a wash in this scene, whose lanterns arrive in
order, and a large saving wherever they arrive out of it (n log n comparisons
each building two keys). Two per-frame costs outside the pass were real:
`PackageHost::run_phase_with` built a new `AssetManager` for every phase it
lent the frame's to the packages (1.9% to 0.3%), and `compute_sprite_points`
looked every sprite's animator and atlas region up separately (2.5% to 1.4%).

What is left scales with the number of sprites: four vertices of 36 bytes
each, computed on the CPU and copied twice on their way to the GPU - ~1.4 MB
a frame here. **Instanced sprites** - one record per sprite (position, size,
rotation, UV rect, colour, anchor), the corners computed in the vertex shader
- would cut both the arithmetic and the bytes by more than half. It touches
every drawable the sprite batch carries (meshes, paths and text stay as
vertices) and the sprite shader every graph material is generated against,
so it is its own project; it matters only where the CPU, not the display,
limits the frame - this machine runs the scene at the display's rate either
way.

**Done (Oct 2026), and it found a bigger cost beside it.** Every rectangle
the batch draws (`draw_sprite_uv`, `draw_sprite_anchored`,
`draw_sprite_anchored_skewed`) is one 68-byte `SpriteInstance`;
`sprite_instance.wgsl`, appended to the default shader and to every generated
graph, makes its corners from `vertex_index` with the CPU's arithmetic, step
for step - `instanced_sprites_draw_as_vertices_did` draws a mixed batch both
ways and finds no pixel different, `render_golden` is unchanged, and web
screenshots of seventeen showcase scenes differ from the old build no more
than two loads of one build do. Meshes, triangles and free quads stay
vertices; a flush draws the two kinds in runs, in the order they were queued.
Each pipeline has an instanced twin: the default ones are built in pairs, and
a graph's is built by `WgpuSpriteBatch::build_graph_pipeline` (which
`GraphPipelineCache` calls) and **kept by the batch** - the first version kept
twins in a thread-local, which held GPU objects past their device and hung
every test thread that had built a graph on its way out. A pipeline with no
twin (`build_custom_pipeline`, a hand-written shader) gets its rectangles as
the vertices they always were.

The bigger cost was the items themselves: `InterleavedItem` carried the
`Gpu` variant's 620-byte particle uniform inline, so every sprite's item was
~640 bytes - six megabytes written, then read by the key builder and the walk,
per pass of ten thousand. Boxed, with the 64-byte material parameters boxed
too (`None` for none, which is nearly every sprite), an item is 160 bytes,
and `interleaved_items_stay_small` keeps it under 176. Together, in the
Ten Thousand Lights web profile:

| | busy CPU | sprite pass |
|---|---|---|
| before | 10,460 samples | 59% (~6,170) |
| instances | ~9,990 | ~53.5% |
| and small items | ~9,680 | ~49% (~4,740) |

- 23% off the pass and 7.5% off the frame's CPU. What is left of the pass is
its own collection and walk over every drawable, twice in a lit frame (the
scene layer and the emission layer); the per-sprite helpers it calls
(`in_view_layers`, `leaves_out`, the culling, sort-layer resolution) measured
at nothing in an attribution build.

Everything below is compiled, tested where a test can reach it, and believed
correct — but **nothing here has been seen working**. The repo has no frontend
test setup, the browser hosts' command arms only run in a browser, and no test
in this repo can tell whether a flock looks right or a force pushes the body a
player would expect. That is a standing limitation, not a temporary one: see
CLAUDE.md, "Nothing in this repo can confirm how a change *feels* in-game."

Each entry says what to open and what would prove it. Delete an entry once it
has been checked, or turn it into a BUGS.md row if it fails.

### Physics command fixes (BUGS #41–43)

The highest-risk items here, because they change what a command does rather
than how fast it does it, and because the tests that cover them are source
scans rather than behaviour.

* **A script-spawned body responds to `apply_force`.** Before the fix the
  editor passed the script handle straight to the backend, so it moved some
  other body or none; the only case that ever worked was an entity whose
  handle happened to equal its entity id.

  **Scene 63 now exists for exactly this** (`63-spawned-physics.scene`). It
  spawns crates with `spawn_prefab` and drives them with `apply_force`,
  `apply_impulse`, `set_velocity`, `set_angular_velocity`, `set_body_type` in
  both spellings, `set_gravity` and `set_sprite_corners`. It carries an
  **authored control crate** that takes the same commands, so a divergence
  shows up in one keypress: if the blue control moves and the orange spawned
  crate does not, the difference is handle resolution rather than physics.

  Scene 56 was the nearest existing coverage and drives bodies the *scene*
  authored, which is the path that always worked — which is how these survived.

  **Opening the scene immediately found a bigger one (BUGS #48).** Physics was
  dead in the editor after any scene transition: the viewport dropped the
  `PhysicsWorld` when replacing the world and never rebuilt it, and only
  `viewport_play` constructed one. So every scene reached from the hub ran with
  no simulation. That is the real reason #41–#47 survived — the commands were
  broken *and* nothing was stepping, so there was nothing to notice. The tell
  was the authored control crate not falling either: the control did its job,
  just not for the reason it was added.
* **`set_body_type(e, "static")` visibly stops a falling body in the editor.**
  It previously wrote a component nothing read again, so it did nothing at all
  there. A body that keeps falling means the fix did not take.
* **`set_body_type(e, "Static")` behaves identically to `"static"`** in all
  three runtimes, and an unknown name logs a warning instead of quietly
  producing a dynamic body.
* **`set_velocity` moves a script-spawned rigid body** (BUGS #45 — the same
  handle bug as #43, in a fourth place). The `Velocity2D` component always
  updated, so the tell is the body not moving while the component says it
  should.
* **`set_sprite_corners` works on a script-spawned entity in a web build**
  (BUGS #44). It already worked natively and in the editor; web was the odd
  one out, so this needs checking in a deployed build, not the editor.

### The 21 shared script commands

`scripting::host_commands` now backs these in both browser hosts. They have 22
native tests, which is 22 more than before, but those drive the shared function
directly — they cannot prove the *hosts* still route to it correctly.

Worth exercising once in the editor, then in a web build: `play_animation` /
`stop_animation`, `set_text` on both a world `TextComponent` and a `UIText`,
`set_ui_text` / `set_ui_visible` / `set_ui_anchor`, `set_health`,
`set_ambient_light`, `gui_focus`. Scenes 34 (skeleton/IK), 43 (GUI) and any
scene with a HUD readout cover most of it incidentally — if those still behave,
the routing is fine.

### Renderer

* **An untextured `ShadowCaster` now draws.** `render_shadows_wgpu` resolves an
  empty texture id to the white pixel like every other path already did;
  previously the batch discarded it silently. This is a behaviour *change*:
  a scene that had an invisible shadow caster will now show a shadow. Check
  nothing gained an unexpected one.

### Swarm

* **The flock still looks like a flock.** The grid now visits the centre cell
  first, so a capped boid steers off its nearest neighbours rather than
  whichever lay up and to the left. Measured 20% less clumping at 16k
  (`tests/flock_structure.rs` guards that it clumps at all), which is a real
  change to the emergent motion even though the statistic says the structure
  survived. Scene 61, at several population presets.

### Editor panel

* **`SceneTree` windowing** — scrolling, selection, drag-and-drop and search in
  a list past the 150-row threshold. Scene 62 exists for this. Typechecked
  only; the repo has no frontend test setup.

---

## Not debt

Two things that look like problems and are not:

- **`Draws: 2` at 8,000 sprites.** Batching works. Sprite rendering is not the
  bottleneck at this scale and did not need attention.
- **Deferred script commands.** `play_sound` cannot return a handle because it
  has not run when the script returns (BUGS.md #20). That is a real limitation
  but it is the correct trade for a bridge that batches; the fix is to offer
  addressable operations (`stop_sound_by_clip`) rather than to make the bridge
  synchronous.
