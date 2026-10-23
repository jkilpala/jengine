-- 141 Pocket Palette: one sunset bay through classic palettes - the
-- `palette` post effect quantises the whole frame with ordered dithering,
-- after everything else. Mode 1 is the bay's own sixteen colours, whose last
-- four (the sea's blues) cycle; then PICO-8, Game Boy, CGA, EGA and Sweetie
-- 16 by preset; 7 is full colour. Left alone, it changes every few seconds.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/141-pocket-palette.scene"
local MODES = {
    { name = "the bay's own sixteen, the sea cycling", custom = true },
    { name = "PICO-8", preset = 1 },
    { name = "Game Boy", preset = 0 },
    { name = "CGA", preset = 2 },
    { name = "EGA", preset = 4 },
    { name = "Sweetie 16", preset = 3 },
    { name = "full colour", off = true },
}
local GULLS = { { -200.0, 220.0, 40.0 }, { 150.0, 260.0, 55.0 }, { 420.0, 190.0, 34.0 } }
local status
local captions = true
local mode, idle, t, since = 1, 6, 0, 0

local function apply(m)
    local spec = MODES[m]
    PostProcess.set_enabled("palette", not spec.off)
    if spec.off then return end
    if spec.custom then
        PostProcess.set_float("palette", "count", 16)
        PostProcess.set_float("palette", "cycle_end", 16)
    else
        PostProcess.set_float("palette", "count", 0)
        PostProcess.set_float("palette", "preset", spec.preset)
        PostProcess.set_float("palette", "cycle_end", 0)
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    apply(mode)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local picked = nil
    for k = 1, #MODES do
        if Input.is_key_just_pressed(tostring(k)) then picked = k end
    end
    if picked ~= nil then idle = 0 else idle = idle + dt end
    since = since + dt
    if picked == nil and idle > 6 and since > 5 then picked = mode % #MODES + 1 end
    if picked ~= nil and picked ~= mode then
        mode = picked
        since = 0
        apply(mode)
    end
    -- Gulls drift and flap.
    for i, g in ipairs(GULLS) do
        local x = ((g[1] + t * g[3] + 700) % 1400) - 700
        local y = g[2] + math.sin(t * 1.3 + i) * 12
        local up = math.floor(t * 4 + i) % 2
        for k = 0, 1 do
            local e = Entity.find_by_name("Gull" .. (i - 1) .. "_" .. k)
            Transform.set_position(e, x, y)
            Sprite.set_color(e, 1, 1, 1, (k == up) and 1 or 0)
        end
    end
    UI.set_text(status, mode .. "  " .. MODES[mode].name)
end
