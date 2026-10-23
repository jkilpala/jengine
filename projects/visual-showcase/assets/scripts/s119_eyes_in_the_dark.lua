-- 119 Eyes in the Dark: each guard's `vision` is what his lantern lights,
-- cut by the cases and pillars; the engine counts the thief in it (the
-- guard's custom `Vision.seen`). This script walks the rounds, sweeps the
-- lanterns, creeps the thief from hiding place to hiding place, and raises
-- the alarm when a guard has her in sight.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/119-eyes-in-the-dark.scene"
local ROUNDS = {
    { { -520.0, 270.0 }, { -80.0, 270.0 }, { -80.0, 80.0 }, { -520.0, 80.0 } },
    { { 520.0, -270.0 }, { 80.0, -270.0 }, { 80.0, -80.0 }, { 520.0, -80.0 } },
    { { -250.0, -40.0 }, { 250.0, -40.0 }, { 250.0, 40.0 }, { -250.0, 40.0 } }
}
local HIDES = { { -560.0, -290.0 }, { -470.0, -80.0 }, { -250.0, -300.0 }, { -150.0, -80.0 }, { -60.0, -90.0 }, { 0.0, 0.0 } }
local LIT = { 1.0, 0.85, 0.5, 0.22 }
local RED = { 1.0, 0.15, 0.1, 0.3 }

local guards, eyes, legs, thief, mark, alarm, status = {}, {}, {}, nil, nil, nil, nil
local captions = true
local hurry = 1
local t = 0
local hide, wait = 1, 2
local alarm_left = 0
local caught = 0

function init(entity)
    for i = 1, #ROUNDS do
        guards[i] = Entity.find_by_name("Guard" .. (i - 1))
        eyes[i] = Entity.find_by_name("GuardEye" .. (i - 1))
        legs[i] = 2
    end
    thief = Entity.find_by_name("Thief")
    mark = Entity.find_by_name("ThiefMark")
    alarm = Entity.find_by_name("Alarm")
    status = Entity.find_by_name("Status")
end

local function seen_by_any()
    for i = 1, #guards do
        local n = Entity.get_custom(eyes[i], "Vision", "seen")
        if n ~= nil and n > 0 then return true end
    end
    return false
end

local function set_cones(c)
    for i = 1, #guards do
        Vision.set(eyes[i], "color", c)
        Lighting.set_color(eyes[i], c[1], c[2], c[3])
    end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("Space") then hurry = hurry == 1 and 2.5 or 1 end
    local step = dt * hurry
    -- Rounds: walk to the next point, the lantern on the way ahead and
    -- sweeping a little to either side.
    for i = 1, #guards do
        local x, y = Transform.get_position(guards[i])
        local p = ROUNDS[i][legs[i]]
        local dx, dy = p[1] - x, p[2] - y
        local d = math.sqrt(dx * dx + dy * dy)
        local speed = (alarm_left > 0 and 120 or 65) * step
        if d <= speed then
            legs[i] = legs[i] % #ROUNDS[i] + 1
        else
            x, y = x + dx / d * speed, y + dy / d * speed
            Transform.set_position(guards[i], x, y)
        end
        local heading = 0
        if math.abs(dx) > math.abs(dy) then
            heading = dx > 0 and 0 or math.pi
        else
            heading = dy > 0 and math.pi / 2 or -math.pi / 2
        end
        local look = heading + math.sin(t * 0.9 + i * 2) * 0.55
        Transform.set_rotation(guards[i], look)
        Transform.set_position(eyes[i], x, y)
        Transform.set_rotation(eyes[i], look)
    end
    -- The thief waits in her shadow, then creeps to the next.
    local tx, ty = Transform.get_position(thief)
    if alarm_left > 0 then
        alarm_left = alarm_left - dt
        local h = HIDES[1]
        local dx, dy = h[1] - tx, h[2] - ty
        local d = math.sqrt(dx * dx + dy * dy)
        if d > 2 then Transform.set_position(thief, tx + dx / d * math.min(d, 170 * step), ty + dy / d * math.min(d, 170 * step)) end
        Lighting.set_intensity(alarm, 0.6 + 0.6 * math.abs(math.sin(t * 6)))
        if alarm_left <= 0 then
            set_cones(LIT)
            Lighting.set_intensity(alarm, 0)
            hide, wait = 1, 2
        end
    else
        wait = wait - step
        if wait <= 0 and hide < #HIDES then
            local h = HIDES[hide + 1]
            local dx, dy = h[1] - tx, h[2] - ty
            local d = math.sqrt(dx * dx + dy * dy)
            if d <= 50 * step then
                hide = hide + 1
                wait = 1.5
            else
                Transform.set_position(thief, tx + dx / d * 50 * step, ty + dy / d * 50 * step)
            end
        elseif hide >= #HIDES and wait <= 0 then
            -- She has the jewel: back to the start, unseen.
            Transform.set_position(thief, HIDES[1][1], HIDES[1][2])
            hide, wait = 1, 3
        end
        if seen_by_any() then
            caught = caught + 1
            alarm_left = 4
            set_cones(RED)
        end
    end
    local mx, my = Transform.get_position(thief)
    Transform.set_position(mark, mx, my)
    UI.set_text(status, alarm_left > 0 and "alarm!" or ("seen " .. caught .. " times"))
end
