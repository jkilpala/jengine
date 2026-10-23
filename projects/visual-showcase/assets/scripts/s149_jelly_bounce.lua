-- 149 Jelly Bounce: every block's sprite is a `jelly`, so the engine
-- squashes it when it lands, wobbles it as it flies and quivers it where it
-- is poked; the bodies are ordinary physics boxes. The script only flings
-- them - and pokes the one clicked (`Jelly.poke`). Left alone, it plays by
-- itself.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/149-jelly-bounce.scene"
local NAMES = { "Cherry", "Lime", "Berry", "Lemon", "Grape" }
local status
local captions = true
local blocks = {}
local t, auto_in, idle = 0, 1.0, 99
local last_mx, last_my
local said = "they wobble"

function init(entity)
    status = Entity.find_by_name("Status")
    for i, n in ipairs(NAMES) do blocks[i] = Entity.find_by_name(n) end
end

local function fling(e, px, py)
    local x, y = Transform.get_position(e)
    Jelly.poke(e, px or x + 30, py or y + 40, 520, 70)
    Physics.set_velocity(e, (x < 0 and 1 or -1) * math.random(120, 320), math.random(780, 1050))
    Physics.set_angular_velocity(e, math.random(-3, 3))
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local mx, my = Input.get_mouse_world_position()
    if last_mx ~= nil and (math.abs(mx - last_mx) > 1 or math.abs(my - last_my) > 1) then idle = 0 else idle = idle + dt end
    last_mx, last_my = mx, my
    if Input.is_mouse_left_just_pressed() then
        idle = 0
        local best, bd = nil, 90
        for _, e in ipairs(blocks) do
            local x, y = Transform.get_position(e)
            local d = math.sqrt((x - mx) ^ 2 + (y - my) ^ 2)
            if d < bd then best, bd = e, d end
        end
        if best ~= nil then
            fling(best, mx, my)
            said = "poked " .. Entity.get_name(best)
        end
    end
    auto_in = auto_in - dt
    if auto_in <= 0 and idle > 3 then
        local e = blocks[math.random(#blocks)]
        fling(e)
        said = Entity.get_name(e) .. " jumps"
        auto_in = 0.9 + math.random() * 0.8
    end
    UI.set_text(status, said)
end
