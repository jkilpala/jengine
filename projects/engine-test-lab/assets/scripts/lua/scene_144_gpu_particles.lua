-- Scene 144: the particles are the engine's (`gpu_particles`); this sets
-- their count and whether time runs, through `Particles.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/144-gpu-particles.scene"
local COUNTS = { 10000, 30000, 60000, 150000, 300000 }
local tier = 3
local frozen = false
local clouds = {}
local status

local function apply()
    for _, e in ipairs(clouds) do
        Particles.set(e, "count", COUNTS[tier])
        Particles.set(e, "playing", not frozen)
    end
    if status then
        UI.set_prop(status, "text", math.floor(COUNTS[tier] / 1000) .. "k each" .. (frozen and ", stopped" or ""))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    for _, n in ipairs({ "Fountain", "Galaxy", "Ring" }) do
        local e = Entity.find_by_name(n)
        if e then clouds[#clouds + 1] = e end
    end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Up") and tier < #COUNTS then tier = tier + 1; apply() end
    if Input.is_key_just_pressed("Down") and tier > 1 then tier = tier - 1; apply() end
    if Input.is_key_just_pressed("Space") then frozen = not frozen; apply() end
end
