-- Scene 299 - fixed steps
--
-- The scene's header says `"simulation_hz": 20`, so this scene's gameplay -
-- motion, physics, timers, and this script's `update` - runs in steps of
-- exactly 1/20 s, as many as real time pays for (`ecs::fixed_step`;
-- docs/determinism-proposal.md, phase 4). What to see:
--
--   * "update calls a second" reads about 20 at any frame rate;
--   * every Space press is counted once, however fast you tap - a press made
--     in a frame that ran no step is kept for the next step, and a frame
--     that runs two steps does not count it twice;
--   * the ball, moved only here in `update`, still glides: what is drawn is
--     interpolated between the last two steps.

local nav = require("lua/nav")
local SCENE_PATH = "scenes/299-fixed-steps.scene"

local ball, summary
local t, calls, rate, since, presses = 0, 0, 0, nil, 0

function init(entity)
    ball = Entity.find_by_name("Ball")
    summary = Entity.find_by_name("Summary")
    t, calls, rate, since, presses = 0, 0, 0, nil, 0
end

function update(entity, dt)
    t = t + dt
    calls = calls + 1
    local now = Time.get_unix()
    if since == nil then since = now end
    if now - since >= 1 then
        rate = calls / (now - since)
        calls, since = 0, now
    end
    if Input.is_key_just_pressed("Space") then presses = presses + 1 end
    Transform.set_position(ball, math.cos(t * 1.5) * 260, -80 + math.sin(t * 3.0) * 110)
    Text.set(summary, "update calls a second: " .. tostring(math.floor(rate + 0.5))
        .. "    Space presses counted: " .. tostring(presses)
        .. "    step: " .. tostring(math.floor(dt * 1000 + 0.5)) .. " ms")
    nav.tick(SCENE_PATH)
end
