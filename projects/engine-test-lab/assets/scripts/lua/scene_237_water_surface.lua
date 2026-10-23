-- Scene 237: `water_surface`. A swell rolls under a raft; a block drops in
-- every few seconds and splashes; Space pokes the surface.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/237-water-surface.scene"
local e = {}
local t = 0
local next_drop = 3
local drops = 0

function init(entity)
    for _, n in ipairs({ "Water", "Raft", "Block", "Line0" }) do
        e[n] = Entity.find_by_name(n)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    next_drop = next_drop - dt
    if next_drop <= 0 then
        drops = drops + 1
        Transform.set_position(e.Block, 220 - (drops % 3) * 140, 220)
        Transform.set_rotation(e.Block, 0.3 * (drops % 2))
        Physics.set_velocity(e.Block, 0, 0)
        next_drop = 4
    end
    if Input.is_key_just_pressed("Space") then
        local x = Transform.get_position(e.Raft)
        Water.splash(e.Water, x + 120, -14)
    end
    UI.set_text(e.Line0, "drops: " .. drops)
end
