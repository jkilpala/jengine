-- Scene 25: Bounds & Lifetime
-- Pure-data demo: WrapBounds, ClampBounds, Lifetime, DespawnOutOfBounds.
-- R reloads the scene, which respawns the lifetime and out-of-bounds
-- entities. (The hint used to say PgUp/PgDn "reload" - they change scene.)

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/25-bounds-lifetime.scene"

function init(entity)
    log("[25-bounds-lifetime] ready - wrap / clamp / lifetime / despawn-oob")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then
        Scene.load(SCENE_PATH)
    end
end
