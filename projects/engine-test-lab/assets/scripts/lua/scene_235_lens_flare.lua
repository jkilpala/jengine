-- Scene 235: `lens_flare`. A bright light circles the frame and throws
-- ghosts, a halo and a streak; a dim one under the threshold throws nothing.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/235-lens-flare.scene"
local e = {}
local t = 0
local on = true
local streak = true

function init(entity)
    for _, n in ipairs({ "Bright", "Line0" }) do
        e[n] = Entity.find_by_name(n)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("L") then
        on = not on
        PostProcess.set_enabled("lens_flare", on)
    end
    if Input.is_key_just_pressed("S") then
        streak = not streak
        PostProcess.set_float("lens_flare", "streak", streak and 0.8 or 0.0)
    end
    Transform.set_position(e.Bright, math.cos(t * 0.5) * 420, math.sin(t * 0.5) * 200)
    UI.set_text(e.Line0, (on and "lens flare on" or "lens flare off") .. (streak and ", streak" or ""))
end
