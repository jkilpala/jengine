-- Scene 173: the lines are the engine's (`line`); this rebuilds the wave's
-- points every frame with Line.set and switches its joints.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/173-lines.scene"
local wave
local t = 0
local JOINTS = { "sharp", "bevel", "round" }

function init(entity)
    wave = Entity.find_by_name("Wave")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    for i = 1, 3 do
        if Input.is_key_just_pressed(tostring(i)) and wave then Line.set(wave, "joint", JOINTS[i]) end
    end
    if not wave then return end
    local mx, my = Input.get_mouse_world_position()
    local pull = math.max(-60, math.min(60, my + 190))
    local pts = {}
    for k = 0, 24 do
        local x = -500 + k * (1000 / 24)
        local near = math.exp(-((x - mx) / 160) ^ 2)
        pts[#pts + 1] = { x, math.sin(t * 2 + k * 0.6) * 14 + near * pull }
    end
    Line.set(wave, "points", pts)
end
