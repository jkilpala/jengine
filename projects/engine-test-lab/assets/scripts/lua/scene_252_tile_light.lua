-- Scene 252: `tile_light`. Light flooded through a tilemap's cells from
-- a moving torch, a blue lamp and lava tiles; rock stops it.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/252-tile-light.scene"
local DECAYS = { 0.82, 0.7, 0.9 }
local cave, torch, blue, line
local t, lit, d, rock, blue_on = 0, true, 1, true, true

function init(entity)
    cave = Entity.find_by_name("Cave")
    torch = Entity.find_by_name("Torch")
    blue = Entity.find_by_name("Blue")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("L") then
        lit = not lit
        TileLight.set(cave, "intensity", lit and 1.6 or 0.0)
    end
    if Input.is_key_just_pressed("D") then
        d = d % #DECAYS + 1
        TileLight.set(cave, "decay", DECAYS[d])
    end
    if Input.is_key_just_pressed("R") then
        rock = not rock
        TileLight.set(cave, "solid_decay", rock and 0.45 or 0.0)
    end
    if Input.is_key_just_pressed("B") then
        blue_on = not blue_on
        TileLight.set(blue, "on", blue_on)
    end
    t = t + dt
    Transform.set_position(torch, -336.0 + math.cos(t * 0.7) * 100, -4.0 + math.sin(t * 0.7) * 150)
    UI.set_text(line, (lit and "lit" or "off") .. "   decay " .. DECAYS[d] .. "   rock " .. (rock and "lit" or "dark")
        .. "   blue " .. (blue_on and "on" or "off"))
end
