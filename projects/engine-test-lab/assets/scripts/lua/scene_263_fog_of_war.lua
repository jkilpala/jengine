-- Scene 263: `fog_of_war`. A scout and the mouse's torch are `sight`s that
-- clear the fog; land they leave stays explored; the red enemies are
-- `fog_hidden`, drawn only while a sight is on them. R resets the fog
-- (`FogOfWar.reset`: the whole map unseen and 0% explored again); C switches
-- the cell size between 10 and 25 through `FogOfWar.set`, which makes a new,
-- unseen grid.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/263-fog-of-war.scene"
local line, fog, scout, torch
local moving, t = true, 0
local coarse = false

function init(entity)
    line = Entity.find_by_name("Line0")
    fog = Entity.find_by_name("Fog")
    scout = Entity.find_by_name("Scout")
    torch = Entity.find_by_name("Torch")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("S") then moving = not moving end
    if Input.is_key_just_pressed("R") then FogOfWar.reset(fog) end
    if Input.is_key_just_pressed("C") then
        coarse = not coarse
        FogOfWar.set(fog, "cell", coarse and 25 or 10)
    end
    if moving then
        t = t + dt
        Transform.set_position(scout, math.sin(t * 0.35) * 420, math.sin(t * 0.9) * 150)
    end
    local mx, my = Input.get_mouse_world_position()
    Transform.set_position(torch, mx, my)
    local seen = 0
    for i = 0, 4 do
        if Entity.get_custom(Entity.find_by_name("Enemy" .. i), "Fog", "visible") == true then seen = seen + 1 end
    end
    local explored = Entity.get_custom(fog, "FogOfWar", "explored") or 0
    UI.set_text(line, "enemies in sight " .. seen .. " / 5    explored " .. math.floor(explored * 100 + 0.5) .. "%"
        .. "    cell " .. (coarse and "25" or "10")
        .. (moving and "" or "    (scout stopped)"))
end
