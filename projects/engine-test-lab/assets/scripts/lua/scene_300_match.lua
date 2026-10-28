-- Scene 300 - a lockstep match (`lab300/play.scene`, `lab300/watch.scene`).
--
-- Every player's input comes from the session's `Lockstep` component - never
-- from `Input.*`, which is only this machine's player - and is the same on
-- every machine at every step, so both balls move the same everywhere. What
-- to see:
--
--   * both copies show both balls in the same places, moved by both players;
--   * the left stick moves a ball by how far it is pushed (`analog`), and
--     the mouse places each player's marker (`pointer`);
--   * close one player mid-match: the other is told it is away and plays on
--     (its ball holds still); start it again and press 1 - it replays the
--     match ("catching up") and takes its seat back;
--   * a third copy pressing 2 watches the same match from the start.
--
-- While the session waits for the room no `update` runs (the scene's steps
-- are held), so the status text is the scene's own; Esc / H then is caught
-- by the session itself (`leave`), which lets the scene step again with the
-- state `left`.

local nav = require("lua/nav")
local MENU = "scenes/300-lockstep.scene"

local session, status
local balls, marks = {}, {}
local flash = { 0, 0 }
local COLORS = { { 0.35, 0.6, 1.0 }, { 1.0, 0.6, 0.25 } }

local function has(list, name)
    for _, v in ipairs(list or {}) do
        if v == name then return true end
    end
    return false
end

local function axis(axes, name)
    return (axes and axes[name]) or 0
end

function init(entity)
    session = entity
    status = Entity.find_by_name("Status")
    balls = { Entity.find_by_name("Seat0"), Entity.find_by_name("Seat1") }
    marks = { Entity.find_by_name("Mark0"), Entity.find_by_name("Mark1") }
    flash = { 0, 0 }
end

local function lock(field)
    return Entity.get_custom(session, "Lockstep", field)
end

function update(entity, dt)
    local state = lock("state") or "connecting"
    if state == "left" then
        Scene.load(MENU)
        return
    end
    local held, pressed = lock("held") or {}, lock("pressed") or {}
    local axes, pointer = lock("axes") or {}, lock("pointer") or {}
    for k = 1, 2 do
        local h, a = held[k], axes[k]
        local dx = (has(h, "move_right") and 1 or 0) - (has(h, "move_left") and 1 or 0)
            + axis(a, "pad_right") - axis(a, "pad_left")
        local dy = (has(h, "move_up") and 1 or 0) - (has(h, "move_down") and 1 or 0)
            + axis(a, "pad_up") - axis(a, "pad_down")
        dx = math.max(-1, math.min(1, dx))
        dy = math.max(-1, math.min(1, dy))
        local x, y = Transform.get_position(balls[k])
        x = math.max(-600, math.min(600, x + dx * 260 * dt))
        y = math.max(-330, math.min(150, y + dy * 260 * dt))
        Transform.set_position(balls[k], x, y)
        local p = pointer[k]
        if p and p.left then
            Transform.set_position(marks[k], p.x, p.y)
        end
        if has(pressed[k], "confirm") then flash[k] = 0.25 end
        flash[k] = math.max(0, flash[k] - dt)
        local c = COLORS[k]
        local f = flash[k] > 0 and 1 or 0
        Sprite.set_color(balls[k], c[1] + (1 - c[1]) * f, c[2] + (1 - c[2]) * f, c[3] + (1 - c[3]) * f, 1)
    end

    local seat = lock("seat")
    local away, silent = lock("away") or {}, lock("silent") or {}
    local who = (seat == nil) and "watching" or ("seat " .. tostring(seat))
    local lines = { state .. "    " .. who .. "    step " .. tostring(lock("step") or 0) }
    local behind = lock("behind") or 0
    if behind > 0 then
        table.insert(lines, "replaying the match so far: " .. tostring(behind) .. " steps behind")
    end
    for k = 1, 2 do
        if away[k] then
            table.insert(lines, "seat " .. tostring(k - 1) .. " has left - played as nothing held")
        elseif silent[k] then
            table.insert(lines, "seat " .. tostring(k - 1) .. " is silent - played as nothing held")
        end
    end
    if state == "ended" or state == "desync" then
        table.insert(lines, "the match is over: " .. tostring(lock("why") or state) .. "   (Esc back)")
    end
    Text.set(status, table.concat(lines, "\n"))
    nav.tick("scenes/lab300/play.scene")
end
