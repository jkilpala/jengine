-- 129 Carnival Banners: every sign here is world text laid along a curve
-- with a `text_path` - the arch's name, the words on the pennant strings
-- and the marquee scrolling round the carousel (its `scroll`). This script
-- only moves the curves: it swings the strings in the breeze (their letters
-- ride with them), bobs the horses and twinkles the bulbs.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/129-carnival-banners.scene"
local STRINGS = { { -640.0, 245.0, -60.0, 268.0, 44.0 }, { 40.0, 330.0, 640.0, 300.0, 52.0 } }
local strings, horses, bulbs = {}, {}, {}
local status
local captions, windy = true, true
local t, wind = 0, 1

function init(entity)
    for i = 0, #STRINGS - 1 do strings[#strings + 1] = Entity.find_by_name("String" .. i) end
    for i = 0, 3 do horses[#horses + 1] = Entity.find_by_name("Horse" .. i) end
    local k = 0
    while true do
        local b = Entity.find_by_name("Bulb" .. k)
        if b == nil then break end
        bulbs[#bulbs + 1] = b
        k = k + 1
    end
    status = Entity.find_by_name("Status")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then windy = not windy end
    t = t + dt
    wind = wind + ((windy and 1 or 0.15) - wind) * math.min(1, dt * 1.5)
    -- Each string sags and swings: a gust pushes its middle about.
    for i, s in ipairs(STRINGS) do
        local pts = {}
        local gust = math.sin(t * 1.3 + i * 2.1) * 0.6 + math.sin(t * 2.9 + i) * 0.4
        for k = 0, 24 do
            local f = k / 24
            local belly = math.sin(math.pi * f)
            local x = s[1] + (s[3] - s[1]) * f + gust * 18 * wind * belly
            local y = s[2] + (s[4] - s[2]) * f - s[5] * belly - math.abs(gust) * 10 * wind * belly
            pts[#pts + 1] = { x, y }
        end
        Line.set(strings[i], "points", pts)
    end
    -- Horses rise and fall, out of step.
    for i, h in ipairs(horses) do
        local x, _ = Transform.get_position(h)
        Transform.set_position(h, x, -150.0 + 20 + math.sin(t * 2.2 + i * 1.6) * 16)
    end
    -- Bulbs chase round the arch.
    for i, b in ipairs(bulbs) do
        local on = (math.floor(t * 6) + i) % 3 ~= 0
        Sprite.set_color(b, 2.5, 2.1, 1.25, on and 1 or 0.25)
    end
    UI.set_text(status, windy and "a breeze off the field" or "the wind drops")
end
