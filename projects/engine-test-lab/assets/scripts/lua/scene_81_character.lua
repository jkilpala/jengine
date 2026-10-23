-- Scene 81 - Character controller: slopes, steps, one-way platforms,
-- moving platforms, pushing, and collision layers
--
-- Two kinematic capsules carry the engine's `character_controller`, and
-- this script moves them both the same way:
--
--   Physics.move_character(e, vx, vy)          -- what to move by next physics step
--   Physics.move_character(e, vx, vy, true)    -- ...falling through one-way platforms
--   local c = Physics.get_character(e)         -- grounded, wall, ceiling, what it touches
--
-- The level's collision is the tilemap's own: `tilemap_collider` names the
-- solid layer and the one-way layer, and the engine builds the bodies. The
-- lift is moved from here, to show a rider carried by a kinematic platform.
-- No package: this is all core, with rapier on.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/81-character.scene"
local SPEED = 240
local JUMP = 620
local GRAVITY = 1800
local FALL_GRAVITY = 2500

local hero, plain, lift, readout
local vy = { 0, 0 }
local lift_t = 0
local last = "-"

function init(entity)
    hero = entity
    plain = Entity.find_by_name("Plain")
    lift = Entity.find_by_name("Lift")
    readout = Entity.find_by_name("Readout")
    if Physics.move_character == nil then
        Text.set(readout, "move_character is missing from this runtime")
        hero = nil
        return
    end
    log("[scene_81] ready - arrows move, Space jumps, Down+Space drops through")
end

local function drive(e, index, x, jump, held, down, dt)
    local c = Physics.get_character(e)
    if c == nil then
        return
    end
    if c.grounded and vy[index] <= 0 then
        vy[index] = 0
        if jump and not down then
            vy[index] = JUMP
        end
    else
        local g = (vy[index] > 0 and held) and GRAVITY or FALL_GRAVITY
        vy[index] = math.max(-900, vy[index] - g * dt)
    end
    if c.ceiling and vy[index] > 0 then
        vy[index] = 0
    end
    Physics.move_character(e, x * SPEED, vy[index], down and jump)
    return c
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if hero == nil then
        return
    end
    -- The lift: a kinematic platform moved by hand, up and down.
    lift_t = lift_t + dt
    if lift ~= nil then
        Transform.set_position(lift, 200, -90 + math.sin(lift_t * 0.8) * 110)
    end

    local x = 0
    if Input.is_action_pressed("move_left") or Input.is_key_pressed("A") then x = x - 1 end
    if Input.is_action_pressed("move_right") or Input.is_key_pressed("D") then x = x + 1 end
    local jump = Input.is_key_just_pressed("Space")
    local held = Input.is_key_pressed("Space")
    local down = Input.is_action_pressed("move_down") or Input.is_key_pressed("S")

    local c = drive(hero, 1, x, jump, held, down, dt)
    drive(plain, 2, x, jump, held, down, dt)
    if c == nil then
        return
    end
    for _, id in ipairs(c.touching or {}) do
        local name = Entity.get_name(id)
        if name ~= nil and name ~= "" and name ~= "Level" then
            last = name
        end
    end
    local hx, hy = Transform.get_position(hero)
    Text.set(readout, table.concat({
        string.format("grounded %s   wall %d   ceiling %s   sliding %s", c.grounded and "yes" or "no",
            math.floor(c.wall), c.ceiling and "yes" or "no", c.sliding and "yes" or "no"),
        string.format("ground normal %.2f, %.2f   ground entity %s", c.ground_nx, c.ground_ny,
            tostring(c.ground_entity)),
        string.format("moved %.0f, %.0f   at %.0f, %.0f", c.vx, c.vy, hx, hy),
        string.format("touching %d   last: %s", #(c.touching or {}), last),
        "amber steps up ledges (step_height 14); blue does not",
    }, "\n"))
end
