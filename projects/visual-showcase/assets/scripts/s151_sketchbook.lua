-- 151 Sketchbook: the pencil village wobbles because of the `boil` post
-- effect - the frame read through a small noise offset that is drawn afresh
-- `rate` times a second - not because anything here moves its lines. The
-- script walks the figure and flies the birds; B and 1-3 change the boil.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/151-sketchbook.scene"
local RATES = { 4, 8, 14 }
local NAMES = { "slow", "lively", "frantic" }
local status
local captions = true
local on, rate_i = true, 2
local t, wx = 0, -700
local walkers, birds = {}, {}

local function apply()
    PostProcess.set_enabled("boil", on)
    PostProcess.set_float("boil", "rate", RATES[rate_i])
    PostProcess.set_float("boil", "amount", rate_i == 3 and 2.4 or 1.6)
    if on then
        UI.set_text(status, "boiling, " .. NAMES[rate_i] .. " (" .. RATES[rate_i] .. " drawings a second)")
    else
        UI.set_text(status, "still - one drawing")
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    for k = 0, 1 do walkers[k + 1] = Entity.find_by_name("Walker" .. k) end
    for i = 0, 2 do
        local a, b = Entity.find_by_name("Bird" .. i .. "_0"), Entity.find_by_name("Bird" .. i .. "_1")
        local x, y = Transform.get_position(a)
        birds[i + 1] = { a = a, b = b, x = x, y = y, phase = i * 0.7 }
    end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    if Input.is_key_just_pressed("B") then on = not on; apply() end
    for k = 1, 3 do
        if Input.is_key_just_pressed(tostring(k)) then rate_i = k; on = true; apply() end
    end
    -- The walker strolls across, in two poses, a pose a step.
    wx = wx + 70 * dt
    if wx > 700 then wx = -700 end
    local pose = math.floor(t * 3) % 2
    for k = 1, 2 do
        Transform.set_position(walkers[k], wx, -235)
        Sprite.set_color(walkers[k], 1, 1, 1, (k - 1) == pose and 1 or 0)
    end
    for _, b in ipairs(birds) do
        b.x = b.x + 60 * dt
        if b.x > 700 then b.x = -700 end
        local y = b.y + math.sin(t * 1.5 + b.phase) * 10
        local up = math.floor(t * 5 + b.phase * 3) % 2
        Transform.set_position(b.a, b.x, y)
        Transform.set_position(b.b, b.x, y)
        Sprite.set_color(b.a, 1, 1, 1, up == 0 and 1 or 0)
        Sprite.set_color(b.b, 1, 1, 1, up == 1 and 1 or 0)
    end
end
