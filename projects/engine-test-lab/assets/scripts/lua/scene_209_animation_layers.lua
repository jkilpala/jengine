-- Scene 209: animation layers with bone masks. The rig walks (its base
-- clip); the "upper" layer, masked to the arm, raises or waves over it; the
-- "breath" layer is additive on the body. Animation.play_layer / stop_layer
-- / set_layer_weight; the rig's own script hears on_layer_end.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/209-animation-layers.scene"
local lines = {}
local rig
local last = "-"
local breath = false
local walking = true

function init(entity)
    for k = 0, 2 do lines[k + 1] = Entity.find_by_name("Line" .. k) end
    rig = Entity.find_by_name("Rig")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("1") then Animation.play_layer(rig, "upper", "animations/lab209_raise.skelanim", 0.15, false); last = "raise (one-shot)" end
    if key("2") then Animation.play_layer(rig, "upper", "animations/lab209_wave.skelanim", 0.2, true); last = "wave (loop)" end
    if key("3") then Animation.stop_layer(rig, "upper", 0.3); last = "stop layer" end
    if key("4") then Animation.set_layer_weight(rig, "upper", 0.5, 0.3); last = "weight 0.5" end
    if key("5") then Animation.set_layer_weight(rig, "upper", 1, 0.3); last = "weight 1" end
    if key("B") then
        breath = not breath
        Animation.set_layer_weight(rig, "breath", breath and 1 or 0, 0.5)
        last = breath and "breath on" or "breath off"
    end
    if key("Space") then
        walking = not walking
        if walking then Animation.play(rig, "animations/lab209_walk.skelanim") else Animation.stop(rig) end
        last = walking and "walk on" or "walk off"
    end
    UI.set_text(lines[1], "last: " .. last)
    UI.set_text(lines[2], "base walk " .. (walking and "on" or "off") .. "    breath " .. (breath and "on" or "off"))
end
