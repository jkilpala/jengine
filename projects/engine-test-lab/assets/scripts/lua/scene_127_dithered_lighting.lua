-- Scene 127: toon lighting's dither, through Lighting.set_toon - smooth,
-- then four hard bands, then the same bands with every edge drawn as an
-- ordered dither on a grid anchored to the world.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/127-dithered-lighting.scene"

local MODES = {
    { "smooth light (no toon lighting)", 0 },
    { "toon lighting: 4 hard bands", 4, 0.0, 0 },
    { "dithered: 4 bands, edges a 4x4 ordered pattern of 3-unit cells", 4, 1.0, 3 },
}
local label
local mode = 1
local t = 0.0

local function apply()
    local m = MODES[mode]
    Lighting.set_toon(m[2], m[3], m[4])
    if label then UI.set_text(label, m[1]) end
end

function init(entity)
    label = Entity.find_by_name("State")
    apply()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t >= 2.0 then
        t = 0.0
        mode = mode % #MODES + 1
        apply()
    end
end
