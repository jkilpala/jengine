-- Scene 258: `depth_of_field`. Three checkered cards at z -20, 0 and 20;
-- the effect blurs each by how far its z is from `focus`. 1-3 focus on a
-- card, R racks between them, F turns the effect off.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/258-depth-of-field.scene"
local line
local focus, target, on, racking, t = 0, 0, true, false, 0

function init(entity)
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("1") then target, racking = -20, false end
    if Input.is_key_just_pressed("2") then target, racking = 0, false end
    if Input.is_key_just_pressed("3") then target, racking = 20, false end
    if Input.is_key_just_pressed("R") then racking = not racking end
    if Input.is_key_just_pressed("F") then
        on = not on
        PostProcess.set_enabled("depth_of_field", on)
    end
    t = t + dt
    if racking then target = math.sin(t * 0.8) * 20 end
    focus = focus + (target - focus) * math.min(1, dt * 6)
    PostProcess.set_float("depth_of_field", "focus", focus)
    UI.set_text(line, string.format("focus z %.1f   %s%s", focus, on and "depth of field on" or "off",
        racking and "   racking" or ""))
end
