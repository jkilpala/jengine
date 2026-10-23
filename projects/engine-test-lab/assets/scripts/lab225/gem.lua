-- Scene 225: the gem's own script. It, the module it requires, the table it
-- reads and the prefab it spawns all came out of pack lab225 - in a build
-- none of them is in the game until the pack is installed.

script = { tick_rate = 60 }

local glint = require("lab225/glint")

local spin, every, label = 0, 1, "?"
local t, next_spark = 0, 0
local note

function init(entity)
    spin = Data.get("data/lab225_vault", "gem", "spin") or 0
    every = Data.get("data/lab225_vault", "gem", "spark_every") or 1
    label = Data.get("data/lab225_vault", "gem", "label") or "(no table)"
    note = Entity.find_by_name("VaultNote")
    t, next_spark = 0, 0
    print("[lab225] gem script from the pack: spin " .. tostring(spin) .. ", label '" .. label .. "'")
end

function update(entity, dt)
    t = t + dt
    Transform.set_rotation(entity, t * spin)
    local s = glint.pulse(t)
    Transform.set_scale(entity, s, s)
    if t >= next_spark then
        next_spark = t + every
        local x, y = Transform.get_world_position(entity)
        local spark = Entity.spawn_prefab("Lab225Spark")
        if spark then
            Transform.set_position(spark, x + glint.offset(t) * 40, y)
        end
    end
    if note then
        Text.set(note, label)
    end
end
