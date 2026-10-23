-- Scene 137: the tilt-shift band follows the mouse; T toggles it, B swaps
-- to a plain blur. The cars need nothing: patrol + face_velocity.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/137-tilt-shift-and-turning-patrols.scene"
local tilt, blur = true, false
local status

local function show()
    if status then
        UI.set_prop(status, "text", (tilt and "tilt-shift" or "") .. (blur and "  blur" or "") .. ((not tilt and not blur) and "sharp" or ""))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("T") then tilt = not tilt; PostProcess.set_enabled("tilt_shift", tilt); show() end
    if Input.is_key_just_pressed("B") then blur = not blur; PostProcess.set_enabled("blur", blur); show() end
    local _, my = Input.get_mouse_world_position()
    PostProcess.set_float("tilt_shift", "focus_y", my)
end
