-- Scene 247: `day_cycle`. The engine turns the day; this reads the hour
-- from the cycle's custom `DayCycle.hour`, jumps it with `DayCycle.set`,
-- and counts the dawns and dusks the Day entity's own script is told of.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/247-day-cycle.scene"
local day, line0, line1
local stopped = false

function init(entity)
    day = Entity.find_by_name("Day")
    line0 = Entity.find_by_name("Line0")
    line1 = Entity.find_by_name("Line1")
end

local function on_key(k, hour)
    if Input.is_key_just_pressed(k) then DayCycle.set(day, "hour", hour) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    on_key("1", 6.0)
    on_key("2", 12.0)
    on_key("3", 18.0)
    on_key("4", 0.0)
    if Input.is_key_just_pressed("Space") then
        stopped = not stopped
        DayCycle.set(day, "hours_per_second", stopped and 0.0 or 1.0)
    end
    local hour = Entity.get_custom(day, "DayCycle", "hour") or 0
    local h = math.floor(hour)
    local m = math.floor((hour - h) * 60)
    UI.set_text(line0, string.format("%02d:%02d%s", h, m, stopped and "  (stopped)" or ""))
    local dawns = Entity.get_custom(day, "Counts", "dawns") or 0
    local dusks = Entity.get_custom(day, "Counts", "dusks") or 0
    UI.set_text(line1, string.format("dawns %d   dusks %d", math.floor(dawns + 0.5), math.floor(dusks + 0.5)))
end
