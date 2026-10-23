-- Scene 268: the `palette` post effect. Ramps quantised to a preset (1-5),
-- a custom four colours whose last two cycle (6), or none (0); D toggles the
-- ordered dither.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/268-palette.scene"
local NAMES = { "Game Boy", "PICO-8", "CGA", "Sweetie 16", "EGA" }
local line
local mode, dither = 2, true

local function apply()
    PostProcess.set_enabled("palette", mode ~= 0)
    if mode == 6 then
        PostProcess.set_float("palette", "count", 4)
        PostProcess.set_float("palette", "cycle_end", 4)
    elseif mode >= 1 then
        PostProcess.set_float("palette", "count", 0)
        PostProcess.set_float("palette", "preset", mode - 1)
        PostProcess.set_float("palette", "cycle_end", 0)
    end
    PostProcess.set_float("palette", "dither", dither and 0.8 or 0)
end

function init(entity)
    line = Entity.find_by_name("Line0")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    for k = 0, 6 do
        if Input.is_key_just_pressed(tostring(k)) then mode = k; apply() end
    end
    if Input.is_key_just_pressed("D") then dither = not dither; apply() end
    local name = mode == 0 and "off" or (mode == 6 and "custom, cycling" or NAMES[mode])
    UI.set_text(line, "palette: " .. name .. "   dither " .. (dither and "on" or "off"))
end
