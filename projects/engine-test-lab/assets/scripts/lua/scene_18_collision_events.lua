-- Scene 18 - Collision Events.
--
-- Demonstrates `OnCollision` behavior-graph triggers fed by the
-- rapier2d contact stream, plus cross-graph `EmitEvent` -> `OnEvent`
-- routing (the runner re-dispatches behavior-emitted events the same
-- frame they fire, see `wgpu_scene_runner::render_frame`).
--
-- Scene contents (all authored in 18-collision-events.scene):
--   * Floor + two side walls + center bumper - static physics bodies.
--   * 4 dynamic balls dropped from above:
--       ColorBall   (cyan)   -> collision-color.behavior
--       DespawnBall (yellow) -> collision-despawn.behavior
--       PingBall1/2 (purple) -> collision-ping.behavior (EmitEvent)
--   * CounterBox (gray)      -> event-counter.behavior (OnEvent "scene18_ping")
--
-- This script does not move anything. It only refreshes a status text
-- reporting which balls still exist, plus handles R to reload the scene.

script = { tick_rate = 30 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/18-collision-events.scene"

local status_id, status2_id
local color_id, despawn_id, ping1_id, ping2_id, counter_id

local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

local function find_ids()
    status_id   = Entity.find_by_name("Status")
    status2_id  = Entity.find_by_name("Status2")
    color_id    = Entity.find_by_name("ColorBall")
    despawn_id  = Entity.find_by_name("DespawnBall")
    ping1_id    = Entity.find_by_name("PingBall1")
    ping2_id    = Entity.find_by_name("PingBall2")
    counter_id  = Entity.find_by_name("CounterBox")
end

local function exists(eid)
    if not eid then return false end
    local pos = Transform.get_position(eid)
    return pos ~= nil
end

local function alive_str(label, eid)
    if exists(eid) then
        return label .. ": alive"
    else
        return label .. ": gone"
    end
end

-- Global, not `local`: a runtime looks `init` / `update` up by name, so a
-- local one is never called. This file declared both local, and scene 18
-- ran no script at all - no status, no R, no H (BUGS #92).
function init(entity)
    find_ids()
end

function update(entity, dt)
    if not status_id then find_ids() end
    nav.tick(SCENE_PATH)

    if edge("R") then Scene.load(SCENE_PATH) end

    local s1 = string.format("%s   %s   %s   %s",
        alive_str("Color",   color_id),
        alive_str("Despawn", despawn_id),
        alive_str("Ping1",   ping1_id),
        alive_str("Ping2",   ping2_id))
    if status_id then Text.set(status_id, s1) end

    if status2_id then
        local hint
        if exists(counter_id) then
            hint = "Counter listening. Watch console for [Behavior] scene18 ping lines on every purple-ball contact."
        else
            hint = "Counter despawned?"
        end
        Text.set(status2_id, hint)
    end
end
