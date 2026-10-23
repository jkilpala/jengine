-- Scene 125: a sun (directional_light). The entity is turned to swing it
-- round, and Lighting.set_height lowers it - shadows lengthen as it does.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/125-directional-light.scene"

local sun, label
local t = 0.0

function init(entity)
    sun = Entity.find_by_name("Sun")
    label = Entity.find_by_name("State")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if not sun then return end
    t = t + dt
    -- Round once every 12 seconds; up and down every 6.
    Transform.set_rotation(sun, t * 0.52)
    local h = 0.25 + 0.95 * (0.5 + 0.5 * math.cos(t * 1.05))
    Lighting.set_height(sun, h)
    -- Redder near the horizon.
    local warm = math.min(1.0, h)
    Lighting.set_color(sun, 1.0, 0.55 + 0.4 * warm, 0.3 + 0.55 * warm)
    if label then
        UI.set_text(label, "height " .. math.floor(h * 100 + 0.5) / 100 .. "   shadow " .. math.floor(50 / h + 0.5) .. " long")
    end
end
