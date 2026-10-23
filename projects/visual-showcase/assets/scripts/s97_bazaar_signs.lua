-- 97 Bazaar Signs: the shop signs are shaped text from font files -
-- Arabic, Hebrew, Hindi and Thai - and they sway on their chains. The
-- banner says hello in each script in turn (Space moves it on). Generated
-- by tools/gen_scenes.py.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/97-bazaar-signs.scene"
local SIGNS, HELLOS = 11, 4
local INK = { 1.0, 0.85, 0.45 }

local captions = true
local e = {}
local t = 0
local shown = 0
local since = 0
local alpha = {}

function init(entity)
    for i = 0, SIGNS - 1 do e["S" .. i] = Entity.find_by_name("Sign" .. i) end
    for i = 0, HELLOS - 1 do
        e["H" .. i] = Entity.find_by_name("Hello" .. i)
        alpha[i] = (i == 0) and 1 or 0
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    -- Each sign swings on its chain, a gust now and then.
    for i = 0, SIGNS - 1 do
        local gust = math.max(0, math.sin(t * 0.4 + i * 0.7)) ^ 3
        Transform.set_rotation(e["S" .. i], math.sin(t * (1.1 + i * 0.07) + i) * (0.04 + gust * 0.08))
    end
    -- The banner: one greeting at a time, cross-faded.
    since = since + dt
    if since > 3 or Input.is_key_just_pressed("Space") then
        since = 0
        shown = (shown + 1) % HELLOS
    end
    for i = 0, HELLOS - 1 do
        local want = (i == shown) and 1 or 0
        alpha[i] = alpha[i] + (want - alpha[i]) * math.min(1, dt * 5)
        Text.set_color(e["H" .. i], INK[1], INK[2], INK[3], alpha[i])
    end
end
