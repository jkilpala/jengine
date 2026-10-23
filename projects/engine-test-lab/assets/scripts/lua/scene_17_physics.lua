-- Scene 17 - Physics.
--
-- Exercises the rapier2d backend wired through RigidBody2D / Collider2D
-- components. Six static obstacles (floor + two angled ramps + sensor),
-- six dynamic crates, four bouncy balls, and a capsule "player" all
-- live in the .scene file itself. This driver script just translates
-- input into physics impulses / velocities and refreshes status text.
--
-- Controls:
--   WASD / Arrows  move player (horizontal velocity, gravity-Y preserved)
--   SPACE          jump (upward impulse)
--   1              kick all balls LEFT
--   2              kick all balls UP
--   3              kick all balls RIGHT
--   T              spawn a ball above the player (PhysBall prefab)
--   G              shake the crate stack (upward impulse)
--   R              reset scene
--   H / Esc        hub
--   [ / ]          prev / next scene
--
-- The player keeps gravity by reading its own vy back through
-- `get_velocity` every frame. In the editor and on the web that read
-- answered 0 until BUGS #91, which cancelled the fall each frame - the
-- player drifted down slowly while every other body fell normally.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/17-physics.scene"

local MOVE_SPEED      = 220.0   -- pixels/sec horizontal cap for player
local JUMP_IMPULSE_Y  = 320.0   -- upward impulse on SPACE
local BALL_KICK_X     = 180.0   -- horizontal impulse on 1 / 3
local BALL_KICK_Y     = 260.0   -- upward impulse on 2
local CRATE_SHAKE_Y   = 140.0   -- upward impulse on G
local SPAWN_ABOVE     = 60.0    -- a spawned ball appears this far above the player
local MAX_BALLS       = 24      -- stop spawning past this many

local player_id
local ball_ids  = {}
local crate_ids = {}
local status_id, status2_id, status3_id

local last_action = "(none)"
local crates_shaken = 0
local balls_spawned = 0
local balls_kicked  = 0

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local function held(key)
    return Input.is_key_pressed(key)
end

local function refresh()
    if status_id and status_id ~= 0 then
        local px, py = 0.0, 0.0
        local vx, vy = 0.0, 0.0
        if player_id then
            px, py = Transform.get_position(player_id)
            vx, vy = Physics.get_velocity(player_id)
        end
        Text.set(status_id, string.format(
            "player: pos (%.0f, %.0f)  vel (%.0f, %.0f)", px, py, vx, vy))
    end
    if status2_id and status2_id ~= 0 then
        Text.set(status2_id, string.format(
            "balls: %d (spawned %d)   crates: %d   shakes: %d   kicks: %d",
            #ball_ids, balls_spawned, #crate_ids, crates_shaken, balls_kicked))
    end
    if status3_id and status3_id ~= 0 then
        Text.set(status3_id, "last action: " .. last_action)
    end
end

local function kick_balls(dx, dy, label)
    for _, id in ipairs(ball_ids) do
        Physics.apply_impulse(id, dx, dy)
    end
    balls_kicked = balls_kicked + 1
    last_action = label
end

local function shake_crates()
    for _, id in ipairs(crate_ids) do
        -- Slight horizontal jitter so the stack doesn't go perfectly straight up.
        local jitter = ((id % 3) - 1) * 30.0
        Physics.apply_impulse(id, jitter, CRATE_SHAKE_Y)
    end
    crates_shaken = crates_shaken + 1
    last_action = "shake crate stack"
end

local function spawn_ball()
    if #ball_ids >= MAX_BALLS then
        last_action = "spawn skipped (" .. MAX_BALLS .. " balls)"
        return
    end
    local px, py = 0.0, 0.0
    if player_id then px, py = Transform.get_position(player_id) end
    local e = Entity.spawn_prefab("PhysBall")
    if e == nil then
        last_action = "spawn_prefab returned nil"
        return
    end
    Transform.set_position(e, px, py + SPAWN_ABOVE)
    -- Spawned balls answer the 1/2/3 kicks like the authored ones.
    table.insert(ball_ids, e)
    balls_spawned = balls_spawned + 1
    last_action = "spawn ball at player"
end

function init(entity)
    player_id  = Entity.find_by_name("Player")
    status_id  = Entity.find_by_name("Status")
    status2_id = Entity.find_by_name("Status2")
    status3_id = Entity.find_by_name("Status3")

    ball_ids = {}
    for i = 1, 4 do
        local id = Entity.find_by_name("Ball" .. i)
        if id and id ~= 0 then table.insert(ball_ids, id) end
    end

    crate_ids = {}
    for i = 1, 6 do
        local id = Entity.find_by_name("Crate" .. i)
        if id and id ~= 0 then table.insert(crate_ids, id) end
    end

    log(string.format(
        "[17-physics] ready - player=%s, balls=%d, crates=%d",
        tostring(player_id), #ball_ids, #crate_ids))
    refresh()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)

    if player_id and player_id ~= 0 then
        -- Read current vy so we don't fight gravity on the Y axis.
        local _vx, vy = Physics.get_velocity(player_id)
        local dir = 0.0
        if held("Left")  or held("A") then dir = dir - 1.0 end
        if held("Right") or held("D") then dir = dir + 1.0 end
        Physics.set_velocity(player_id, dir * MOVE_SPEED, vy)

        if edge("Space") or edge("W") or edge("Up") then
            Physics.apply_impulse(player_id, 0.0, JUMP_IMPULSE_Y)
            last_action = "jump"
        end
    end

    if edge("1") then kick_balls(-BALL_KICK_X, 60.0,  "kick balls left")  end
    if edge("2") then kick_balls(0.0,          BALL_KICK_Y, "kick balls up")    end
    if edge("3") then kick_balls( BALL_KICK_X, 60.0,  "kick balls right") end
    if edge("T") then spawn_ball() end
    if edge("G") then shake_crates() end
    if edge("R") then
        last_action = "reset"
        log("[17-physics] reload")
        Scene.load(SCENE_PATH)
        return
    end

    refresh()
end
