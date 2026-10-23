-- Scene 88 - Y sorting
--
-- A sprite's draw order used to be `z_order` and nothing else: a number
-- fixed when the scene was authored. That is right for a backdrop or a HUD
-- and useless for the case where y *is* depth - a beat-em-up's floor, an
-- isometric room, a top-down field with trees in it. Two actors on the same
-- floor share a z_order, so the pair drew in whatever order the ECS query
-- returned them in, which in practice meant "whoever spawned last is in
-- front", forever, however they moved.
--
-- `y_sort` on the sprite sorts it against the others in its band by world
-- position: lower on the screen draws in front. It sorts *within* the band,
-- so the floor below still stays below.
--
-- Two identical groves, moved identically. The left one's sprites carry the
-- flag and the right one's do not. Watch the walker pass behind a tree it
-- is above and in front of one it is below - on the right it cuts through
-- every trunk at the same fixed depth.
--
-- What sorts is the entity's **origin**, not the middle of the quad, which
-- is why everything here is anchored at its feet. The trunk and canopy of
-- one tree share an origin, so a tree is never split by something walking
-- between its own two halves.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/88-ysort.scene"

local LEFT_X = -330.0
local RIGHT_X = 330.0
-- The walk stays inside the grove: the interesting frames are the ones
-- where the walker and a trunk are within a few pixels of the same y.
local RANGE_X = 175.0
local RANGE_Y = 135.0
local SPEED = 190.0

local walker_l, walker_r, readout
local wx, wy = 0.0, -60.0
local auto = true
local t = 0

-- Both walkers get the same offset from their own grove's centre, so any
-- difference on screen is the sorting and nothing else.
local function place(ox, oy)
    if walker_l then Transform.set_position(walker_l, LEFT_X + ox, oy) end
    if walker_r then Transform.set_position(walker_r, RIGHT_X + ox, oy) end
end

function init(entity)
    walker_l = Entity.find_by_name("WalkerL")
    walker_r = Entity.find_by_name("WalkerR")
    readout = Entity.find_by_name("Readout")
    place(wx, wy)
    log("[scene_88] ready - left grove is y-sorted, right grove is not")
end

function update(entity, dt)
    t = t + dt

    if Input.is_key_just_pressed("Space") then
        auto = not auto
    end

    local dx, dy = 0.0, 0.0
    if Input.is_key_pressed("Left") then dx = dx - 1.0 end
    if Input.is_key_pressed("Right") then dx = dx + 1.0 end
    if Input.is_key_pressed("Down") then dy = dy - 1.0 end
    if Input.is_key_pressed("Up") then dy = dy + 1.0 end

    if dx ~= 0.0 or dy ~= 0.0 then
        -- Taking the arrows stops the tour, or the walker would fight it.
        auto = false
        wx = wx + dx * SPEED * dt
        wy = wy + dy * SPEED * dt
    elseif auto then
        -- A slow loop that crosses the trees rather than circling them:
        -- the y's have to actually swap for the order to be worth drawing.
        wx = math.sin(t * 0.55) * RANGE_X
        wy = math.sin(t * 0.83) * RANGE_Y - 20.0
    end

    if wx < -RANGE_X then wx = -RANGE_X end
    if wx > RANGE_X then wx = RANGE_X end
    if wy < -RANGE_Y - 30.0 then wy = -RANGE_Y - 30.0 end
    if wy > RANGE_Y then wy = RANGE_Y end
    place(wx, wy)

    if readout then
        Text.set(readout, string.format(
            "walker y %.0f   %s   left: in front of anything above it   right: always behind",
            wy, auto and "auto" or "arrows"))
    end
    nav.tick(SCENE_PATH)
end
