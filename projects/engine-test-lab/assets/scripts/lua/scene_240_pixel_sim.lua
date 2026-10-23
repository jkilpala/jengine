-- Scene 240: `pixel_sim`. The script paints the level - a stone floor, a
-- basin, a shelf of oil and a wooden plank - and two sources pour sand and
-- water. F lights the oil, L drops lava, R paints it all again.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/240-pixel-sim.scene"
local sim, crate
local t = 0
local dropped = false

local function line(x0, y0, x1, y1, r, m)
    local n = math.max(1, math.floor(math.max(math.abs(x1 - x0), math.abs(y1 - y0)) / r))
    for i = 0, n do
        local f = i / n
        PixelSim.paint(sim, x0 + (x1 - x0) * f, y0 + (y1 - y0) * f, r, m)
    end
end

local function build()
    line(-460, -270, 460, -270, 14, "stone")
    line(-470, -280, -470, 240, 12, "stone")
    line(470, -280, 470, 240, 12, "stone")
    -- The basin under the water.
    line(-40, -260, -40, -150, 8, "stone")
    line(300, -260, 300, -150, 8, "stone")
    -- A shelf, its lip and the oil on it.
    line(150, 40, 460, 40, 8, "stone")
    line(158, 40, 158, 70, 6, "stone")
    line(175, 58, 450, 58, 7, "oil")
    -- A plank on two posts.
    line(-20, -110, 280, -110, 5, "wood")
    line(-10, -150, -10, -110, 4, "wood")
    line(270, -150, 270, -110, 4, "wood")
end

function init(entity)
    sim = Entity.find_by_name("Sim")
    crate = Entity.find_by_name("Crate")
    build()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if not dropped and t > 8 then
        Physics.set_body_type(crate, "dynamic")
        dropped = true
    end
    if Input.is_key_just_pressed("F") then PixelSim.paint(sim, 200, 70, 8, "fire") end
    if Input.is_key_just_pressed("L") then PixelSim.paint(sim, 130, 120, 14, "lava") end
    if Input.is_key_just_pressed("R") then Scene.load(SCENE_PATH) end
end
