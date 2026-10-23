-- Scene 283: `lightning`, `Lightning.strike` / `Lightning.set` and
-- `on_strike`. The script is on the cloud, so the engine's callback reaches it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/283-lightning.scene"
local COLOURS = { { 0.55, 0.65, 1.0, 0.8 }, { 1.0, 0.45, 0.95, 0.8 }, { 1.0, 0.8, 0.3, 0.8 } }
local line
local cloud
local auto, branches, colour, strikes = true, 4, 1, 0

local function show()
    UI.set_text(line, strikes .. " strikes   auto " .. (auto and "on" or "off") .. "   branches " .. branches)
end

function init(entity)
    cloud = entity
    line = Entity.find_by_name("Line0")
    show()
end

function on_strike(entity)
    strikes = strikes + 1
    show()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_mouse_left_just_pressed() then
        local mx, my = Input.get_mouse_world_position()
        Lightning.strike(cloud, mx, math.min(my, 200))
    end
    if Input.is_key_just_pressed("A") then
        auto = not auto
        Lightning.set(cloud, "interval", auto and { 1.2, 2.2 } or { 0, 0 })
        show()
    end
    if Input.is_key_just_pressed("B") then
        branches = branches % 10 + 2
        Lightning.set(cloud, "branches", branches)
        show()
    end
    if Input.is_key_just_pressed("C") then
        colour = colour % #COLOURS + 1
        Lightning.set(cloud, "glow_color", COLOURS[colour])
    end
end
