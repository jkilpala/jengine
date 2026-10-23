-- Scene 129: a burst every two seconds from the two ring emitters (a burst
-- fires when an emitter turns active), one without drag and one with it.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/129-particle-drag-turbulence.scene"

local rings = {}
local t = 0.0
local rearm = false

function init(entity)
    rings = { Entity.find_by_name("NoDrag"), Entity.find_by_name("Drag") }
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    -- Off one frame, on the next: the emitter has to be seen inactive for
    -- turning it on to count as a new burst.
    if rearm then
        rearm = false
        for _, e in ipairs(rings) do Particles.set_active(e, true) end
    end
    t = t + dt
    if t >= 2.0 then
        t = 0.0
        rearm = true
        for _, e in ipairs(rings) do Particles.set_active(e, false) end
    end
end
