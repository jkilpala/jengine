-- Scene 210: particle flipbooks. Three emitters on one numbered 4 x 4
-- sheet: over a particle's life, at a frame rate from a random start, and
-- one random frame held. F rewrites the middle one's fps (Particles.set).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/210-particle-flipbooks.scene"
local e = {}
local fast = false

function init(entity)
    for _, n in ipairs({ "EmitterLife", "EmitterFps", "EmitterRandom", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("1") then Particles.emit_burst(e.EmitterLife, 6) end
    if key("2") then Particles.emit_burst(e.EmitterFps, 6) end
    if key("3") then Particles.emit_burst(e.EmitterRandom, 6) end
    if key("F") then
        fast = not fast
        Particles.set(e.EmitterFps, "flipbook.fps", fast and 16 or 4)
    end
    UI.set_text(e.Line0, "middle emitter: " .. (fast and "16" or "4") .. " frames a second")
end
