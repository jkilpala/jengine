-- Scene 289: `Replay.*` - an input replay. The dot is steered with
-- momentum on `dt`, so a run depends on every input and every frame time;
-- played back, it must end where the recording did. The ends are kept in an
-- uncommitted save slot, which outlives the scene reload in every runtime.
-- R, P and S are read through `Replay.live_key`, so they are never replayed.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/289-input-replay.scene"
local SLOT = "lab_replay"
local lines = {}
local x, y, vx, vy = 0, -40, 0, 0
local t = 0

local function fmt(a, b)
    return string.format("(%.3f, %.3f)", a, b)
end

function init(entity)
    for i = 0, 2 do lines[i] = Entity.find_by_name("Line" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    -- The run: arrows accelerate, Space dashes, a click jumps.
    local ax, ay = 0, 0
    if Input.is_key_pressed("Left") then ax = ax - 1 end
    if Input.is_key_pressed("Right") then ax = ax + 1 end
    if Input.is_key_pressed("Up") then ay = ay + 1 end
    if Input.is_key_pressed("Down") then ay = ay - 1 end
    vx, vy = (vx + ax * 1400 * dt) * (1 - 2 * dt), (vy + ay * 1400 * dt) * (1 - 2 * dt)
    if Input.is_key_just_pressed("Space") then vx, vy = vx * 2.5, vy * 2.5 end
    if Input.is_mouse_left_just_pressed() then x, y = Input.get_mouse_world_position() end
    x = math.max(-600, math.min(600, x + vx * dt))
    y = math.max(-300, math.min(220, y + vy * dt))
    Transform.set_position(entity, x, y)

    local status = Replay.status()
    if status == "recording" then Save.set(SLOT, "recorded", fmt(x, y)) end
    if status == "playing" then Save.set(SLOT, "replayed", fmt(x, y)) end
    if Replay.live_key("R") then
        if status == "recording" then
            Replay.stop()
        else
            Save.set(SLOT, "replayed", "")
            Replay.record()
            Scene.load(SCENE_PATH)
        end
    elseif Replay.live_key("P") and Replay.frames() > 0 then
        Save.set(SLOT, "replayed", "")
        Replay.play()
        Scene.load(SCENE_PATH)
    elseif Replay.live_key("S") then
        Replay.stop()
    end

    local rec, rep = Save.get(SLOT, "recorded") or "-", Save.get(SLOT, "replayed") or "-"
    if rep == "" then rep = "-" end
    UI.set_text(lines[0], "status " .. Replay.status() .. "   frames " .. math.floor(Replay.frames())
        .. "   progress " .. string.format("%.2f", Replay.progress()) .. "   t " .. string.format("%.2f", t))
    UI.set_text(lines[1], "recorded end " .. rec .. "   replayed end " .. rep)
    local verdict = ""
    if Replay.status() == "finished" then verdict = (rec == rep) and "MATCH" or "DIFFER" end
    UI.set_text(lines[2], verdict)
end
