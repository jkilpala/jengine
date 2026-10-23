-- Scene 122: trails. Everything moves by components (oscillator,
-- angular_velocity); the script turns the left trail's emitting off and on
-- every two seconds (Trail.set_emitting) - off, its ribbon ages away - and
-- recolours it each time it starts again (Trail.set), which keeps the
-- points it has, so the ribbon changes colour where it is.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/122-trails.scene"

local wanderer, label
local on = true
local t = 0.0
local hue = 0
-- Linear colours, one per spell of emitting.
local COLOURS = {
    { 1.0, 0.35, 0.1, 1.0 },
    { 0.1, 0.8, 1.0, 1.0 },
    { 0.9, 0.2, 0.9, 1.0 },
}

local function show()
    if label then Text.set(label, on and "emitting (2 s)" or "not emitting (2 s)") end
end

function init(entity)
    wanderer = Entity.find_by_name("Wanderer")
    label = Entity.find_by_name("Emitting")
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if t >= 2.0 and wanderer then
        t = 0.0
        on = not on
        Trail.set_emitting(wanderer, on)
        if on then
            hue = hue % #COLOURS + 1
            Trail.set(wanderer, "color", COLOURS[hue])
        end
        show()
    end
end
