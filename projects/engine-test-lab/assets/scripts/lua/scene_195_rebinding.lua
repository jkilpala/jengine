-- Scene 195: rebinding at runtime. Input.get_just_pressed() names what
-- went down this frame the way an action binds it; Input.set_bindings takes
-- it; Save.set / Save.commit keep it for the next run.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/195-rebinding.scene"
local ACTION, DEFAULT, SLOT = "lab_jump", { "J" }, "lab_195"
local lamp, binding, pressed_text, status
local capturing, last = false, ""

-- Set and saved together: `Input.set_bindings` is deferred, so
-- `Input.get_bindings` still answers the old keys this frame.
local function bind(keys)
    Input.set_bindings(ACTION, keys)
    Save.set(SLOT, ACTION, keys)
    Save.commit(SLOT)
end

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    binding = Entity.find_by_name("Binding")
    pressed_text = Entity.find_by_name("Pressed")
    status = Entity.find_by_name("Status")
    -- What the last run saved, or the default.
    Input.set_bindings(ACTION, Save.get(SLOT, ACTION) or DEFAULT)
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local pressed = Input.get_just_pressed()
    if #pressed > 0 then last = table.concat(pressed, ", ") end
    if capturing then
        if #pressed > 0 then
            bind({ pressed[1] })
            capturing = false
        end
    elseif Input.is_key_just_pressed("Enter") then
        capturing = true
    elseif Input.is_key_just_pressed("Backspace") then
        bind(DEFAULT)
    elseif Input.is_key_just_pressed("F5") then
        Scene.load(SCENE_PATH)
    end
    if lamp then
        if Input.is_action_pressed(ACTION) then Sprite.set_color(lamp, 1.0, 0.8, 0.3, 1)
        else Sprite.set_color(lamp, 0.25, 0.27, 0.32, 1) end
    end
    local keys = Input.get_bindings(ACTION) or {}
    if binding then
        Text.set(binding, capturing and "press any key or button..." or (ACTION .. " = " .. table.concat(keys, ", ")))
    end
    if pressed_text then Text.set(pressed_text, "last pressed: " .. last) end
    if status then UI.set_text(status, "bindings saved in slot '" .. SLOT .. "'") end
end
