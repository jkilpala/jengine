-- 153 Rainy Window: the drops on the glass are the `lens_rain` post
-- effect - beads that form and dry, drops that run down and leave trails,
-- each bending the frame behind it, worked out from the clock. The script
-- moves the tram and the cars, flickers the signs, and turns the rain up and
-- down.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/153-rainy-window.scene"
local status
local captions = true
local amount, mist = 0.6, true
local t = 0
local tram, open_sign
local cars = {}

local function apply()
    PostProcess.set_float("lens_rain", "amount", amount)
    PostProcess.set_float("lens_rain", "mist", mist and 0.35 or 0)
    UI.set_text(status, "rain " .. math.floor(amount * 100 + 0.5) .. "%" .. (mist and ", the glass steamed" or ""))
end

function init(entity)
    status = Entity.find_by_name("Status")
    tram = Entity.find_by_name("Tram")
    open_sign = Entity.find_by_name("Open")
    for i = 0, 3 do
        cars[i + 1] = { a = Entity.find_by_name("Car" .. i .. "_0"), b = Entity.find_by_name("Car" .. i .. "_1"),
                        x = -800 + i * 450, dir = (i % 2 == 0) and 1 or -1, speed = 160 + i * 35 }
    end
    apply()
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    if Input.is_key_just_pressed("Up") then amount = math.min(1, amount + 0.1); apply() end
    if Input.is_key_just_pressed("Down") then amount = math.max(0, amount - 0.1); apply() end
    if Input.is_key_just_pressed("M") then mist = not mist; apply() end
    -- The tram crosses slowly and comes round again.
    local tx = -900 + (t * 70) % 1800
    Transform.set_position(tram, tx, -170)
    -- Cars: two lights each, white one way and red the other.
    for _, c in ipairs(cars) do
        c.x = c.x + c.dir * c.speed * dt
        if c.x > 800 then c.x = -800 elseif c.x < -800 then c.x = 800 end
        local y = c.dir > 0 and -240 or -280
        Transform.set_position(c.a, c.x - 16, y)
        Transform.set_position(c.b, c.x + 16, y)
        local r, g, b = 2.5, 2.4, 2.1
        if c.dir < 0 then r, g, b = 2.6, 0.3, 0.25 end
        Sprite.set_color(c.a, r, g, b, 1)
        Sprite.set_color(c.b, r, g, b, 1)
    end
    -- The OPEN sign stutters now and then.
    local on = not (math.sin(t * 7.3) > 0.92 or math.sin(t * 2.1 + 1) > 0.97)
    Sprite.set_color(open_sign, on and 2.2 or 0.15, on and 1.65 or 0.1, on and 0.55 or 0.05, 1)
end
