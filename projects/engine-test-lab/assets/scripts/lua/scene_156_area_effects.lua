-- Scene 156: the floating, the updraft and the pull are the engine's
-- (`area_effect`); Space switches the updraft through `Area.set`, R reloads.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/156-area-effects.scene"
local updraft
local on = true

function init(entity)
    updraft = Entity.find_by_name("Updraft")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) end
    if Input.is_key_just_pressed("Space") and updraft then
        on = not on
        Area.set(updraft, "enabled", on)
        Sprite.set_color(updraft, 0.6, 0.9, 0.7, on and 0.18 or 0.05)
    end
end
