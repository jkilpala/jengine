-- Scene 134: particle collision and sub-emitters. B swaps the sparks
-- between bouncing and dying (`Particles.set` on a nested field); Space
-- bursts the pink emitter.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/134-particle-collision.scene"

local sparks, burst, status
local bounce = true

function init(entity)
    sparks = Entity.find_by_name("Sparks")
    burst = Entity.find_by_name("Burst")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("B") and sparks then
        bounce = not bounce
        Particles.set(sparks, "collision.response", bounce and "bounce" or "kill")
        if status then UI.set_prop(status, "text", bounce and "sparks bounce" or "sparks die where they hit") end
    end
    if Input.is_key_just_pressed("Space") and burst then
        Particles.emit_burst(burst, 60)
    end
end
