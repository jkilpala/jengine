-- Scene 206: scene streaming. A lays the next room in beside the last
-- (Scene.load_additive), U takes the oldest out (Scene.unload), P preloads
-- a scene (Scene.preload); the lines read Scene.status / Scene.progress.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/206-scene-streaming.scene"
local ROOMS = { "a", "b", "c" }
local PRELOAD = "scenes/205-text-fields.scene"
local lines = {}
local chunks = {}
local n = 0
local preloaded = false

local function add()
    n = n + 1
    local name = ROOMS[(n - 1) % #ROOMS + 1]
    -- Four rooms across the view, then on to the right.
    local x = -480 + ((n - 1) % 4) * 320
    local h = Scene.load_additive("scenes/lab206/room_" .. name .. ".scene", x, -40)
    chunks[#chunks + 1] = { h = h, name = name }
end

function init(entity)
    for k = 0, 4 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
    add()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("A") then add() end
    if Input.is_key_just_pressed("U") and #chunks > 0 then
        Scene.unload(table.remove(chunks, 1).h)
    end
    if Input.is_key_just_pressed("P") then
        Scene.preload(PRELOAD)
        preloaded = true
    end
    local parts = {}
    for _, c in ipairs(chunks) do
        parts[#parts + 1] = string.format("%s#%d %s %d%%", c.name, c.h, Scene.status(c.h),
            math.floor(Scene.progress(c.h) * 100 + 0.5))
    end
    UI.set_text(lines[1], #chunks .. " chunk(s):  " .. table.concat(parts, "   "))
    UI.set_text(lines[2], preloaded and ("preload " .. PRELOAD .. ": " .. Scene.status(PRELOAD)
        .. " " .. math.floor(Scene.progress(PRELOAD) * 100 + 0.5) .. "%") or "")
end
