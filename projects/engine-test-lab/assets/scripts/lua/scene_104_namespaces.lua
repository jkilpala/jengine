-- Scene 104: script namespaces - `Sprite.set_color` and the rest.
--
-- Every runtime builds its namespace tables from one list
-- (`crates/jengine-core/src/scripting/namespaces.rs`): the Luau adapter
-- directly, the editor and web builds from the copy their hosts publish. So
-- this counts what each table actually holds, here, in this runtime, and
-- prints it. Compare native / editor / web: the numbers must match (until Oct
-- 2026 `Skeleton` read one fewer outside native, for `spawn_rig`). A
-- namespace reading 0 means that runtime built no table for it.
--
-- The two boxes below are coloured once through `Sprite.set_color` and once
-- through the flat `set_sprite_color`: the same function, so they must match.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/104-namespaces.scene"

local NAMES = {
    "Entity", "Transform", "Sprite", "Material", "Text", "Animation", "Skeleton",
    "Tween", "Timer", "Event", "Input", "Physics", "Audio", "Scene", "Camera",
    "Time", "Tilemap", "Particles", "Lighting", "PostProcess", "UI", "GUI",
    "Save", "Persistent", "Debug", "Data", "Script", "Math",
}

local function count(t)
    local n = 0
    for _, v in pairs(t) do
        if type(v) == "function" then n = n + 1 end
    end
    return n
end

function init(entity)
    local lines, total = {}, 0
    local row = {}
    for i, name in ipairs(NAMES) do
        local t = _G[name]
        local n = (type(t) == "table") and count(t) or 0
        total = total + n
        row[#row + 1] = string.format("%s %d", name, n)
        if #row == 7 or i == #NAMES then
            lines[#lines + 1] = table.concat(row, "    ")
            row = {}
        end
    end
    lines[#lines + 1] = ""
    lines[#lines + 1] = string.format("%d namespaces, %d functions", #NAMES, total)
    lines[#lines + 1] = string.format("Sprite.set_color == set_sprite_color: %s",
        tostring(Sprite ~= nil and Sprite.set_color == set_sprite_color))
    Text.set(Entity.find_by_name("Readout"), table.concat(lines, "\n"))

    Sprite.set_color(Entity.find_by_name("Namespaced"), 0.3, 0.8, 1.0, 1.0)
    -- The old flat name on purpose: the scene compares the two spellings.
    local flat_set_color = set_sprite_color
    flat_set_color(Entity.find_by_name("Flat"), 0.3, 0.8, 1.0, 1.0)
    Text.set(Entity.find_by_name("MathReadout"),
        string.format("Math.lerp(0, 10, 0.25) = %s", tostring(Math.lerp(0, 10, 0.25))))
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
end
