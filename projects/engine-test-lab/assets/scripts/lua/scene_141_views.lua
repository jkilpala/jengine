-- Scene 141: the cameras are the engine's (`camera.target`); V moves the
-- room camera away and back, so its screens show something else.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/141-cameras-into-textures.scene"
local cam, status
local away = false

function init(entity)
    cam = Entity.find_by_name("RoomCam")
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("V") then
        away = not away
        if cam then Transform.set_position(cam, away and 0 or 2000, 0) end
        if status then UI.set_prop(status, "text", away and "room camera looking here" or "room camera on the room") end
    end
end
