-- Scene 124: the pixelate and crt post effects, stepping through
-- off / pixelate / crt / both every two seconds.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/124-pixelate-crt.scene"

local STATES = {
    { false, false, "plain" },
    { true, false, "pixelate (90 lines: 8-pixel cells at 720p)" },
    { false, true, "crt" },
    { true, true, "pixelate + crt" },
}
local state, t, label = 1, 0.0, nil

local function apply()
    local s = STATES[state]
    PostProcess.set_enabled("pixelate", s[1])
    PostProcess.set_enabled("crt", s[2])
    if label then UI.set_text(label, s[3]) end
end

function init(entity)
    label = Entity.find_by_name("State")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t >= 2.0 then
        t = 0.0
        state = state % #STATES + 1
        apply()
    end
end
