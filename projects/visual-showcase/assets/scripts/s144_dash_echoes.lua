-- 144 Dash Echoes: each ninja carries an `afterimage` with a `min_speed`,
-- so the engine leaves fading copies of its pose only while it dashes - the
-- script just moves it. The wisp's echoes are `flat` and additive: its
-- shape in one colour, a streak of light.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/144-dash-echoes.scene"
local ROOFS = { { -640.0, -330.0 }, { -250.0, 90.0 }, { 190.0, 640.0 } }
local TOP = -150.0
local RUN, DASH = 230, 1350
local runners = {}
local wisp, status
local captions = true
local t = 0

local function on_roof(x)
    for _, r in ipairs(ROOFS) do
        if x >= r[1] and x <= r[2] then return true end
    end
    return false
end

local function make(name, dir, speed, calm)
    return { e = Entity.find_by_name(name), dir = dir, speed = speed, dash = -1, calm = calm, name = name }
end

function init(entity)
    runners = { make("Ninja", 1, RUN, 0.6), make("Rival", -1, RUN * 0.85, 1.4) }
    for _, r in ipairs(runners) do
        r.x = Transform.get_position(r.e)
    end
    wisp = Entity.find_by_name("Wisp")
    status = Entity.find_by_name("Status")
end

local function start_dash(r)
    r.dash = 0
    r.calm = 0
end

local function step(r, dt, want_dash)
    if r.dash < 0 then
        r.calm = r.calm + dt
        -- Off the edge of a roof, or now and then mid-roof, or when asked.
        local edge = not on_roof(r.x + r.dir * 34)
        local room = on_roof(r.x + r.dir * 260)
        if edge or want_dash or (r.calm > 2.2 and room) then start_dash(r) end
    end
    local speed = r.speed
    local y = TOP
    if r.dash >= 0 then
        r.dash = r.dash + dt
        speed = DASH
        y = TOP + math.sin(math.pi * math.min(1, r.dash / 0.24)) * 34
        if r.dash > 0.16 and on_roof(r.x) then r.dash = -1 end
    end
    r.x = r.x + r.dir * speed * dt
    if r.x > 600 then r.dir = -1 end
    if r.x < -600 then r.dir = 1 end
    Transform.set_position(r.e, r.x, y)
    Transform.set_scale(r.e, r.dir, 1)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    local asked = Input.is_key_just_pressed("Space")
    for i, r in ipairs(runners) do
        step(r, dt, asked and i == 1)
    end
    Transform.set_position(wisp, math.sin(t * 1.1) * 430, 150 + math.sin(t * 2.2) * 80)
    local n = runners[1]
    if n.dash >= 0 then
        UI.set_text(status, "dash! - each echo is the pose the ninja had, fading as it goes")
    else
        UI.set_text(status, "running - no echoes under the afterimage's min_speed")
    end
end
