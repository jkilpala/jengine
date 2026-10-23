-- Scene 271: `afterimage`. Three arrows run back and forth: the first
-- leaves tinted copies, the second flat additive ones, the third only
-- while it dashes (its `min_speed`). Space stops and starts them all.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/271-afterimage.scene"
local NAMES = { "Tinted", "Flat", "Dashing" }
local ROWS = { 140, 20, -100 }
local arrows = {}
local line
local emitting = true
local t = 0
local dash = -1

function init(entity)
    line = Entity.find_by_name("Line0")
    for i, n in ipairs(NAMES) do
        arrows[i] = { e = Entity.find_by_name(n), x = -400, dir = 1 }
    end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if Input.is_key_just_pressed("Space") then
        emitting = not emitting
        for _, a in ipairs(arrows) do Afterimage.set(a.e, "emitting", emitting) end
    end
    if Input.is_key_just_pressed("D") or (dash < 0 and t % 2.5 < dt) then dash = 0 end
    for i, a in ipairs(arrows) do
        local speed = 260
        if i == 3 then
            if dash >= 0 then
                speed = 1400
                dash = dash + dt
                if dash > 0.25 then dash = -1 end
            else
                speed = 160
            end
        end
        a.x = a.x + a.dir * speed * dt
        if a.x > 420 then a.dir = -1 end
        if a.x < -420 then a.dir = 1 end
        Transform.set_position(a.e, a.x, ROWS[i] + math.sin(t * 3 + i) * 18)
        Transform.set_scale(a.e, a.dir, 1)
    end
    local state = "emitting"
    if not emitting then state = "stopped (the copies left fade out)" end
    if dash >= 0 then state = state .. "   dash!" end
    UI.set_text(line, state)
end
