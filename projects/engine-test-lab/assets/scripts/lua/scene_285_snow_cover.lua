-- Scene 285: `snow_cover` and `Snow.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/285-snow-cover.scene"
local line
local things = {}
local rate, limit = 0, 1.5

local function apply()
    for _, e in ipairs(things) do
        Snow.set(e, "rate", rate)
        Snow.set(e, "slope_limit", limit)
    end
    local what = rate > 0 and "snowing" or (rate < 0 and "melting" or "still")
    UI.set_text(line, what .. "   slope limit " .. limit)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    for _, n in ipairs({ "Steps", "Roof", "Post" }) do table.insert(things, Entity.find_by_name(n)) end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local changed = false
    if Input.is_key_just_pressed("S") then rate = 3; changed = true end
    if Input.is_key_just_pressed("M") then rate = -4; changed = true end
    if Input.is_key_just_pressed("Space") then rate = 0; changed = true end
    if Input.is_key_just_pressed("Up") then limit = math.min(4, limit + 0.5); changed = true end
    if Input.is_key_just_pressed("Down") then limit = math.max(0.5, limit - 0.5); changed = true end
    if changed then apply() end
end
