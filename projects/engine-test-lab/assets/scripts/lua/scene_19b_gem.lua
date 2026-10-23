-- Scene 19b (gem sub-scene). Bumps the persistent score by +50 on
-- init and pops back to the ScoreKeeper hub on SPACE or B.

script = { tick_rate = 30 }

local DELTA = 50
local LABEL = "+50 gem"

local awarded = false
local prev = {}
local function edge(key)
    local now = Input.is_key_pressed(key)
    local pressed = now and not prev[key]
    prev[key] = now
    return pressed
end

function update(entity, dt)
    if not awarded then
        local s = Persistent.get_number("score19") or 0
        Persistent.set_number("score19", s + DELTA)
        Persistent.set_string("last19", LABEL)
        awarded = true
    end
    if edge("Space") or edge("B") then
        Scene.pop()
    end
end
