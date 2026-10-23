-- Scene 82 - Camera rotation: the view turns with the camera entity
--
-- `Camera2D` gained a rotation (Sep 2026) driven by the active camera
-- entity's own `Transform.rotation`, so there is no new script function -
-- `Transform.set_rotation(camera, radians)` is the whole API, the same call that
-- turns any other entity.
--
-- The car drives a circuit and the camera is set to its heading, which is
-- what a top-down racer wants: the car's nose holds still on screen and
-- the world swings around it. Press 1 to hold the camera level instead and
-- watch the car turn rather than the world.
--
-- Two things worth watching while it spins:
--   * the blue box follows `Input.get_mouse_world_position()`, which turns the
--     screen offset by the camera's rotation - it must stay under the
--     cursor at every angle, in every runtime;
--   * the panel is entity UI, which is screen space and stays upright.
--     The two bars are the world's axes, and they do turn.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/82-camera-turn.scene"

-- The circuit: a lazy ellipse, wide enough that the turn is never abrupt.
local RX, RY = 430, 250
local LAP = 11.0

local camera, car, pointer, upright
local readout = {}
local t = 0
local follow = true
local driving = true
local manual = 0
local heading = 0

-- Fengari (the editor and web) has a 2-arg `math.atan` and no `atan2`;
-- native Luau has `atan2` and a 1-arg `atan`. Probe once.
local atan2 = math.atan2
if atan2 == nil then
    atan2 = function(y, x) return math.atan(y, x) end
end

local function degrees(rad)
    local d = rad * 180 / math.pi
    -- Into -180..180, so the readout does not wind up to thousands.
    while d > 180 do d = d - 360 end
    while d < -180 do d = d + 360 end
    return d
end

function init(entity)
    camera = Entity.find_by_name("Camera")
    car = Entity.find_by_name("Car")
    pointer = Entity.find_by_name("Pointer")
    upright = Entity.find_by_name("Upright")
    for i = 1, 4 do
        readout[i] = Entity.find_by_name("Readout" .. i)
    end
    log("[scene_82] ready - 1 follow/level, 2/3 turn by hand, 4 level, 5 park")
end

local function say(i, text)
    if readout[i] ~= nil then
        UI.set_prop(readout[i], "text", text)
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if camera == nil then
        return
    end

    if Input.is_key_just_pressed("1") then follow = not follow end
    if Input.is_key_just_pressed("5") then driving = not driving end
    if Input.is_key_pressed("2") then manual = manual + dt * 1.6 end
    if Input.is_key_pressed("3") then manual = manual - dt * 1.6 end
    if Input.is_key_just_pressed("4") then manual = 0 end

    -- Drive the car round the circuit, and face it where it is going.
    if driving then
        t = t + dt
    end
    local a = (t / LAP) * math.pi * 2
    local x, y = math.cos(a) * RX, math.sin(a) * RY
    -- The tangent of the ellipse, which is the direction of travel.
    heading = atan2(math.cos(a) * RY, -math.sin(a) * RX)
    if car ~= nil then
        Transform.set_position(car, x, y)
        Transform.set_rotation(car, heading)
    end

    -- The camera follows the car, and turns to its heading less a quarter
    -- turn so "forward" is up the screen rather than to the right.
    local want = follow and (heading - math.pi / 2) or 0
    Transform.set_position(camera, x, y)
    Transform.set_rotation(camera, want + manual)

    -- World text turns with its own transform since Sep 2026 - before that
    -- the renderer ignored the rotation, so a label under a turned camera
    -- lay on its side with no way to correct it. The world's two axis
    -- labels are left alone and swing with the track; this one is turned
    -- back by the camera's own angle and stays readable.
    if upright ~= nil then
        local ang = want + manual
        local sin, cos = math.sin(ang), math.cos(ang)
        local ox, oy = 0, -190
        Transform.set_position(upright, x + ox * cos - oy * sin, y + ox * sin + oy * cos)
        Transform.set_rotation(upright, ang)
    end

    -- The marker under the cursor. This is the claim the scene exists to
    -- make: it must not drift off the pointer as the view turns.
    local mx, my = 0, 0
    if Input.get_mouse_world_position ~= nil then
        mx, my = Input.get_mouse_world_position()
        if pointer ~= nil then
            Transform.set_position(pointer, mx, my)
        end
    end
    say(1, string.format("camera rotation %6.1f deg   %s", degrees(want + manual), follow and "following the car" or "held level"))
    say(2, string.format("car heading     %6.1f deg   at %d, %d", degrees(heading), math.floor(x + 0.5), math.floor(y + 0.5)))
    say(3, string.format("by hand         %6.1f deg   [2] [3] turn  [4] level", degrees(manual)))
    say(4, string.format("mouse in world  %d, %d", math.floor(mx + 0.5), math.floor(my + 0.5)))
end
