-- Scene 84 - Contact impulses: how hard two things hit, and where
--
-- A collision used to say only *that* something touched. Since Sep 2026 it
-- also says how hard - the impulse the solver applied to resolve the
-- contact - and where, which is what a physics game needs to break a crate
-- on a heavy landing and shrug off a nudge.
--
--   for _, c in ipairs(Physics.get_contacts()) do
--       local a, b, started, impulse, x, y = c[1], c[2], c[3], c[4], c[5], c[6]
--   end
--
-- The first three are what `get_contacts` always returned, so a script that
-- unpacks three values is unaffected. `on_collision_enter`'s payload carries
-- the same three new fields.
--
-- Two things to watch:
--   * the three crates land from three heights, and the numbers are in that
--     order - the impulse is a real measure, not a flag;
--   * the stack on the right is a column of live contacts that never stops
--     touching, and its impulses are tiny beside any landing. A game that
--     damaged on every contact would grind its own level to gravel.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/84-impulses.scene"

local crates, marks, readout, rest_label = {}, {}, nil, nil
local rest = {}
local landed = { 0, 0, 0 }
local rest_peak = 0
local show_rest = false
local gravity = -9.81
local dropped = 0

local START = { { -320, -40 }, { -120, 120 }, { 80, 300 } }

function init(entity)
    for i = 1, 3 do
        crates[i] = Entity.find_by_name("Crate" .. i)
        marks[i] = Entity.find_by_name("Mark" .. i)
        rest[i] = Entity.find_by_name("Rest" .. i)
    end
    readout = Entity.find_by_name("Readout")
    rest_label = Entity.find_by_name("RestLabel")
    Physics.set_gravity(0, gravity)
    log("[scene_84] ready - Space drops them again, 1/2 change gravity")
end

local function drop_again()
    dropped = dropped + 1
    landed = { 0, 0, 0 }
    rest_peak = 0
    for i = 1, 3 do
        if crates[i] ~= nil then
            Transform.set_position(crates[i], START[i][1], START[i][2])
            Physics.set_velocity(crates[i], 0, 0)
        end
    end
end

local function index_of(id)
    for i = 1, 3 do
        if crates[i] == id then
            return i
        end
    end
    return nil
end

local function is_rest(id)
    for i = 1, 3 do
        if rest[i] == id then
            return true
        end
    end
    return false
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    if Input.is_key_just_pressed("Space") then
        drop_again()
    end
    if Input.is_key_just_pressed("1") then
        gravity = math.max(-40, gravity - 5)
        Physics.set_gravity(0, gravity)
    end
    if Input.is_key_just_pressed("2") then
        gravity = math.min(-2, gravity + 5)
        Physics.set_gravity(0, gravity)
    end
    if Input.is_key_just_pressed("B") then
        show_rest = not show_rest
    end

    -- Every contact this frame, with how hard it was.
    if Physics.get_contacts ~= nil then
        for _, c in ipairs(Physics.get_contacts()) do
            local a, b, started, impulse, x, y = c[1], c[2], c[3], c[4], c[5], c[6]
            impulse = impulse or 0
            if started then
                for _, id in ipairs({ a, b }) do
                    local i = index_of(id)
                    if i ~= nil and landed[i] == 0 then
                        landed[i] = impulse
                        -- Where it landed, marked on the floor.
                        if marks[i] ~= nil and x ~= nil then
                            Transform.set_position(marks[i], x, -300)
                        end
                    end
                    if is_rest(id) then
                        rest_peak = math.max(rest_peak, impulse)
                    end
                end
            end
        end
    end

    Text.set(readout, string.format(
        "gravity %.1f      landed:  %.0f     %.0f     %.0f      drops %d",
        gravity, landed[1], landed[2], landed[3], dropped))
    if rest_label ~= nil then
        if show_rest then
            Text.set(rest_label, string.format("resting stack: peak %.0f", rest_peak))
        else
            Text.set(rest_label, "")
        end
    end
    for i = 1, 3 do
        if marks[i] ~= nil then
            Text.set(marks[i], landed[i] > 0 and string.format("%.0f", landed[i]) or "")
        end
    end
end
