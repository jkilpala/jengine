-- Scene 224 - runtime conformance
--
-- A getter that binds in every runtime can still answer wrongly in one: the
-- browsers read what their host published, native reads the world, and a
-- field one host never publishes answers a default in silence. Nothing that
-- checks names can see that; only running the same script everywhere can.
--
-- So this writes known state on frame 1, reads it back through as many
-- getters as it can reach on frame 4 (spawned things exist from the frame
-- after their spawn), and logs one `[conformance] name = value` line per
-- answer, then `[conformance] done`. `tools/conformance.mjs` runs the scene
-- natively and in a web build and diffs the two sets of lines. Entity ids
-- differ between runtimes, so a getter that returns one is reported as
-- `id` or `nil`, and a list of them as its length.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/224-runtime-conformance.scene"

local frame = 0
local a, child, mover, ball, map, summary_ui, lines_ui
local spawned, prefab, rig
local results = {}

-- Rounded so the dialects agree: no `%d` (Fengari throws on a fraction) and
-- no negative zero.
local function num(v)
    local r = math.floor(v * 1000 + 0.5) / 1000
    if math.abs(r) < 0.0005 then r = 0 end
    return string.format("%.3f", r)
end

local function show(v)
    local t = type(v)
    if t == "number" then return num(v) end
    if t == "nil" then return "nil" end
    if t == "boolean" or t == "string" then return tostring(v) end
    if t == "table" then
        local keys = {}
        for k in pairs(v) do keys[#keys + 1] = tostring(k) end
        table.sort(keys)
        local parts = {}
        for _, k in ipairs(keys) do
            local val = v[k]
            if val == nil then val = v[tonumber(k)] end
            parts[#parts + 1] = k .. ":" .. show(val)
        end
        return "{" .. table.concat(parts, ",") .. "}"
    end
    return t
end

local function pack(...)
    return { n = select("#", ...), ... }
end

-- One probe: its values, joined, or the error it raised.
local function probe(name, fn)
    local ok, out = pcall(function() return pack(fn()) end)
    local text
    if not ok then
        text = "ERROR " .. tostring(out)
    else
        -- Trailing nils dropped: native answers a missing entity's position
        -- with two, the browsers with one (on purpose - `lua-runtime.ts` makes
        -- a getter with no answer a single nil), and `local x, y = ...` reads
        -- both the same. Only `select('#', ...)` could tell them apart.
        local n = out.n
        while n > 0 and out[n] == nil do n = n - 1 end
        local parts = {}
        for i = 1, n do parts[#parts + 1] = show(out[i]) end
        text = n == 0 and "nil" or table.concat(parts, ", ")
    end
    results[#results + 1] = name .. " = " .. text
    log("[conformance] " .. name .. " = " .. text)
end

-- An entity id is runtime-specific: say whether there is one.
local function id(v)
    if v == nil then return nil end
    return "id"
end

local function count(list)
    if list == nil then return nil end
    return #list
end

local function write()
    Transform.set_position(a, 100, 50)
    Transform.set_rotation(a, 0.5)
    Transform.set_scale(a, 2, 1)
    Sprite.set_color(a, 0.2, 0.4, 0.6, 0.8)
    Material.set(a, "additive")
    Entity.set_custom(a, "Probe", "n", 7)
    Entity.add_tag(a, "probed")
    spawned = Entity.spawn("ProbeSpawned")
    Transform.set_position(spawned, -40, 25)
    prefab = Entity.spawn_prefab("Coin")
    rig = Skeleton.spawn_rig("rigs/spawned_arm.rig")
    Save.set("conformance", "answer", 42)
    Locale.set("fi")
    -- A dynamic body, steered by a script: the physics carries it, once.
    Physics.set_velocity(ball, -20, 30)
end

local function read()
    -- Transform
    probe("position", function() return Transform.get_position(a) end)
    probe("rotation", function() return Transform.get_rotation(a) end)
    probe("scale", function() return Transform.get_scale(a) end)
    probe("child_world_position", function() return Transform.get_world_position(child) end)
    probe("child_world_rotation", function() return Transform.get_world_rotation(child) end)
    probe("child_world_scale", function() return Transform.get_world_scale(child) end)
    probe("missing_position", function() return Transform.get_position(987654) end)
    -- Entity
    probe("find_by_name", function() return id(Entity.find_by_name("ProbeA")) end)
    probe("find_by_name_missing", function() return id(Entity.find_by_name("NoSuchThing")) end)
    probe("get_name", function() return Entity.get_name(a) end)
    probe("find_by_tag", function() return count(Entity.find_by_tag("probe")) end)
    probe("has_tag_added", function() return Entity.has_tag(a, "probed") end)
    probe("has_tag_absent", function() return Entity.has_tag(a, "nope") end)
    probe("find_child", function() return Entity.find_child(a, "ProbeChild") == child end)
    probe("find_in_radius", function() return count(Entity.find_in_radius(100, 50, 5, "probe")) end)
    probe("custom_written", function() return Entity.get_custom(a, "Probe", "n") end)
    probe("custom_missing", function() return Entity.get_custom(a, "Probe", "missing") end)
    probe("health", function() return Entity.get_health(a) end)
    -- Spawned this run, read through the handle the script holds
    probe("spawned_position", function() return Transform.get_position(spawned) end)
    probe("spawned_name", function() return Entity.get_name(spawned) end)
    probe("prefab_name", function() return Entity.get_name(prefab) end)
    probe("prefab_scale", function() return Transform.get_scale(prefab) end)
    probe("rig_bone", function() return id(Skeleton.find_bone(rig, "elbow")) end)
    probe("rig_wrist_world", function() return Transform.get_world_position(Skeleton.find_bone(rig, "wrist")) end)
    probe("rig_by_name", function() return id(Skeleton.find("spawned_arm")) end)
    -- Sprite and material
    probe("sprite_color", function() return Sprite.get_color(a) end)
    probe("sprite_anchor", function() return Sprite.get_anchor(a) end)
    probe("material", function() return Material.get(a) end)
    probe("animation_state", function() return Animation.get_state(a) end)
    probe("animation_playing", function() return Animation.is_playing(a) end)
    -- Physics
    probe("velocity", function() return Physics.get_velocity(mover) end)
    probe("velocity_none", function() return Physics.get_velocity(a) end)
    probe("body_velocity", function() return Physics.get_velocity(ball) end)
    probe("gravity", function() return Physics.get_gravity() end)
    probe("raycast", function()
        local hit = Physics.raycast(0, -120, 1, 0, 1000)
        if hit == nil then return nil end
        return hit.x, hit.y, hit.normal_x, hit.normal_y, hit.distance
    end)
    probe("raycast_miss", function() return Physics.raycast(0, 300, 1, 0, 100) end)
    probe("overlap_point", function() return count(Physics.overlap_point(400, -120)) end)
    probe("overlap_circle", function() return count(Physics.overlap_circle(400, -120, 10)) end)
    -- Camera
    probe("camera_zoom", function() return Camera.get_zoom() end)
    probe("camera_position", function() return Camera.get_position() end)
    -- Tilemap
    probe("tile", function() return Tilemap.get_tile(map, "ground", 1, 0) end)
    probe("tile_empty", function() return Tilemap.get_tile(map, "ground", 1, 1) end)
    probe("tile_to_world", function() return Tilemap.tile_to_world(map, 2, 1) end)
    probe("world_to_tile", function() return Tilemap.world_to_tile(map, -500, -130) end)
    probe("tile_at_world", function() return Tilemap.get_tile_at_world(map, "ground", -500, -130) end)
    -- Data, locale, save
    probe("data", function() return Data.get("data/enemies", "goblin", "health") end)
    probe("data_string", function() return Data.get("data/enemies", "goblin", "display_name") end)
    -- Live ops: keys only the game's defaults define, so the answer is the
    -- same whether or not the lab's service is running (or was cached).
    probe("liveops_get", function() return LiveOps.get("lab224_text") end)
    probe("liveops_number", function() return LiveOps.get("lab224_number") end)
    probe("liveops_table", function() return LiveOps.get("lab224_table") end)
    probe("liveops_fallback", function() return LiveOps.get("no_such_key", "fallback") end)
    probe("liveops_no_variant", function() return LiveOps.variant("no_such_experiment") end)
    -- No experiment named: refused before anything is sent, in every runtime.
    probe("liveops_expose_nothing", function() return LiveOps.expose("") end)
    probe("locale", function() return Locale.current() end)
    probe("locale_name", function() return Locale.name("fi") end)
    -- Asset packs: the list and an unknown pack agree whether the build has
    -- a manifest (web) or the project folder is read (native).
    probe("pack_list", function() return count(Packs.list()) end)
    probe("pack_unknown", function() return Packs.status("no-such-pack") end)
    probe("save", function() return Save.get("conformance", "answer") end)
    -- Pure functions, which must agree to the last digit
    probe("lerp", function() return Math.lerp(2, 10, 0.25) end)
    probe("clamp", function() return Math.clamp(12, 0, 10) end)
    probe("distance", function() return Math.distance(0, 0, 3, 4) end)
    probe("angle_between", function() return Math.angle_between(0, 0, 1, 1) end)
    probe("perlin", function() return Noise.perlin(1.5, 2.5, 7) end)
    probe("perlin3", function() return Noise.perlin3(1.5, 2.5, 0.5, 7) end)
    probe("simplex", function() return Noise.simplex(1.5, 2.5, 7) end)
    probe("particles", function() return Particles.get_count() end)
    probe("time_scale", function() return Time.get_scale() end)
    -- Nobody moves the mouse in a conformance run: both must answer (0, 0).
    probe("mouse_delta", function() return Input.get_mouse_delta() end)
    probe("can_lock_pointer", function() return Display.supports("cursor_lock") end)
end

function init(entity)
    a = Entity.find_by_name("ProbeA")
    child = Entity.find_by_name("ProbeChild")
    mover = Entity.find_by_name("ProbeMover")
    ball = Entity.find_by_name("ProbeBall")
    map = Entity.find_by_name("ProbeMap")
    summary_ui = Entity.find_by_name("Summary")
    lines_ui = Entity.find_by_name("Lines")
    frame = 0
    results = {}
end

function update(entity, dt)
    frame = frame + 1
    if frame == 1 then
        write()
    elseif frame == 4 then
        read()
        local errors = 0
        for _, line in ipairs(results) do
            if line:find(" = ERROR", 1, true) then errors = errors + 1 end
        end
        log("[conformance] done " .. #results)
        Text.set(summary_ui, string.format("%s probes, %s raised an error - compare runtimes with tools/conformance.mjs",
            tostring(#results), tostring(errors)))
        Text.set(lines_ui, table.concat(results, "\n"))
        Locale.set("en")
    end
    nav.tick(SCENE_PATH)
end
