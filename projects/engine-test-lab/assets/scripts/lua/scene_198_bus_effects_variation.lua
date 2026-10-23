-- Scene 198: the sfx bus's newer effects (echo, distortion, compressor,
-- EQ) beside the older two, and a clip with a variation. A click plays
-- every second through whatever is on.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/198-bus-effects-variation.scene"
local CLICK = "sounds/blip.wav"
local ROWS = {
    { key = "1", effect = "echo", on = 0.6 },
    { key = "2", effect = "distortion", on = 0.7 },
    { key = "3", effect = "compress", on = 1.0 },
    { key = "4", effect = nil, on = nil },
    { key = "5", effect = "lowpass", on = 600.0 },
    { key = "6", effect = "reverb", on = 0.6 },
    { key = "V", effect = nil, on = nil }
}
local lamps, status, pulse = {}, nil, nil
local clock, since = 0, 10

local function apply(i)
    local r = ROWS[i]
    if r.key == "4" then
        Audio.set_bus_effect("sfx", "eq_low", r.lit and -20 or 0)
        Audio.set_bus_effect("sfx", "eq_high", r.lit and 12 or 0)
    elseif r.key == "V" then
        Audio.set_variation(CLICK, r.lit and 3 or 0, r.lit and 0.4 or 0)
    else
        Audio.set_bus_effect("sfx", r.effect, r.lit and r.on or 0)
    end
    Sprite.set_color(lamps[i], r.lit and 1 or 0.25, r.lit and 0.8 or 0.27, r.lit and 0.3 or 0.32, 1)
end

function init(entity)
    status = Entity.find_by_name("Status")
    pulse = Entity.find_by_name("Pulse")
    for i = 1, #ROWS do
        lamps[i] = Entity.find_by_name("Lamp" .. (i - 1))
        ROWS[i].lit = false
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for i, r in ipairs(ROWS) do
        if Input.is_key_just_pressed(r.key) then
            r.lit = not r.lit
            apply(i)
        end
    end
    clock = clock + dt
    since = since + dt
    if clock >= 1 or Input.is_key_just_pressed("Space") then
        clock = 0
        since = 0
        Audio.play_sound(CLICK, 0.8)
    end
    local s = 1 + math.max(0, 0.4 - since)
    Transform.set_scale(pulse, s, s)
    local on = 0
    for _, r in ipairs(ROWS) do if r.lit then on = on + 1 end end
    if status then UI.set_text(status, on .. " on") end
end
