-- Scene 101: wind, drag, impulse, torque, and an input axis that drives.
--
-- Nothing on this screen is moved by a script. `wind2d`, `drag2d`,
-- `impulse2d`, `force2d` and `torque2d` were registered, authorable and
-- documented as motion, and no runtime integrated any of them: batch-script
-- vocabulary only, so a `drag2d` on a sprite did nothing. And an
-- `input_axis` computed the arrow-key vector every frame for a script to
-- apply - nothing applied it without one. Both found by the arcade's Kite
-- Hill, which has no script at all. They run in the one shared motion step
-- (`behaviors::tick_motion`) every runtime calls now.
--
-- This script only reads numbers back for the readout. Compare native /
-- editor / web: the flag must settle at the same speed in all three.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/101-wind-and-drag.scene"

local flag, kicked, pinwheel, driven, readout

function init(entity)
    flag = Entity.find_by_name("Flag")
    kicked = Entity.find_by_name("Kicked")
    pinwheel = Entity.find_by_name("Pinwheel")
    driven = Entity.find_by_name("Driven")
    readout = Entity.find_by_name("Readout")
end

local function r1(v)
    return math.floor((v or 0) * 10 + 0.5) / 10
end

function update(entity, dt)
    local fx = Physics.get_velocity(flag)
    local _, ky = Physics.get_velocity(kicked)
    local dx, dy = Physics.get_velocity(driven)
    Text.set(readout, "flag speed " .. r1(fx) .. " (settles at 80)    kicked vy " .. r1(ky)
        .. "    pinwheel turn " .. r1(Transform.get_rotation(pinwheel))
        .. "    driven (" .. r1(dx) .. ", " .. r1(dy) .. ")")
    nav.tick(SCENE_PATH)
end
