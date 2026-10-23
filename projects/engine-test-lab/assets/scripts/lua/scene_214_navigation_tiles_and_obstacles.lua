-- Scene 214: navigation that sees a tilemap's walls (D digs a hole in
-- one) and a nav_obstacle crate (P pushes it into the aisle and out).

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/214-navigation-tiles-and-obstacles.scene"
local MX, MY, T = -560.000000, 160.000000, 40.000000
local X2 = 330.000000
local e = {}
local leg1, leg2 = 1, 1
local dug = false
local in_aisle = false

local A1 = { { MX + 60, MY - 60 }, { MX + 420, MY - 60 } }
local A2 = { { X2 - 200, 0 }, { X2 + 200, 0 } }

function init(entity)
    for _, n in ipairs({ "Map", "Robot1", "Robot2", "Crate", "Line0" }) do e[n] = Entity.find_by_name(n) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    if key("D") then
        dug = not dug
        Tilemap.set_tile(e.Map, "walls", 6, 1, dug and 0 or 1)
    end
    if key("P") then
        in_aisle = not in_aisle
        local _, cy = Transform.get_position(e.Crate)
        Physics.apply_impulse(e.Crate, 0, in_aisle and -2200 or 2200)
    end
    -- Back and forth, each robot.
    if not Nav.is_moving(e.Robot1) then
        leg1 = 3 - leg1
        Nav.set_target(e.Robot1, A1[leg1][1], A1[leg1][2])
    end
    if not Nav.is_moving(e.Robot2) then
        leg2 = 3 - leg2
        Nav.set_target(e.Robot2, A2[leg2][1], A2[leg2][2])
    end
    UI.set_text(e.Line0, "wall " .. (dug and "dug through" or "whole") .. "   crate " .. (in_aisle and "in the aisle" or "aside"))
end
