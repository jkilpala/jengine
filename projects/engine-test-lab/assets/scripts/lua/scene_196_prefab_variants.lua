-- Scene 196: prefab variants. Lab196Red extends Lab196Base (a
-- prefab_instance in the prefab itself, with overrides), Lab196BigRed
-- extends Lab196Red. Space spawns each by name.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/196-prefab-variants.scene"
local NAMES = { { "Lab196Base", -420 }, { "Lab196Red", -140 }, { "Lab196BigRed", 140 } }
local status
local spawned = 0

function init(entity)
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then
        for _, n in ipairs(NAMES) do
            local e = Entity.spawn_prefab(n[1])
            Transform.set_position(e, n[2], -190)
        end
        spawned = spawned + 3
    end
    if status then UI.set_text(status, "spawned " .. spawned) end
end
