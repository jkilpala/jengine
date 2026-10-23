-- 145 Oil and Ink: one harbour, four looks, all post effects in the
-- output pass - `paint` (a Kuwahara filter: each pixel the mean of the
-- calmest of the four squares meeting at it, so flat areas become strokes
-- and edges stay sharp) and `halftone` (dots on turned screens, sized by the
-- colour under them). Left alone it changes every five seconds.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/145-oil-and-ink.scene"
local BOATS = { { "BoatRed", -260.0, -205.0, 0.00 }, { "BoatSail", 280.0, -225.0, 1.70 } }
local GULLS = { { -200.0, 220.0, 40.0 }, { 150.0, 260.0, 55.0 }, { 420.0, 190.0, 34.0 } }
local MODES = { "a photograph", "an oil painting", "a comic-book page", "newsprint" }
local status
local captions = true
local mode, since, t = 1, 0, 0
local boats, clouds, gulls = {}, {}, {}

local function apply(m)
    mode = m
    since = 0
    local paint = m == 2 or m == 3
    PostProcess.set_enabled("paint", paint)
    if m == 2 then PostProcess.set_float("paint", "radius", 6) end
    if m == 3 then PostProcess.set_float("paint", "radius", 3) end
    PostProcess.set_enabled("halftone", m >= 3)
    if m == 3 then
        PostProcess.set_float("halftone", "mode", 1)
        PostProcess.set_float("halftone", "cell", 7)
        PostProcess.set_float("halftone", "strength", 0.85)
        PostProcess.set_vec4("halftone", "paper", 1.0, 0.97, 0.9, 1.0)
        PostProcess.set_vec4("halftone", "ink", 0.02, 0.02, 0.04, 1.0)
    elseif m == 4 then
        PostProcess.set_float("halftone", "mode", 0)
        PostProcess.set_float("halftone", "cell", 6)
        PostProcess.set_float("halftone", "angle", 45)
        PostProcess.set_float("halftone", "strength", 1)
        PostProcess.set_vec4("halftone", "paper", 0.80, 0.74, 0.60, 1.0)
        PostProcess.set_vec4("halftone", "ink", 0.03, 0.04, 0.10, 1.0)
    end
    UI.set_text(status, "the harbour as " .. MODES[m])
end

function init(entity)
    status = Entity.find_by_name("Status")
    for i, b in ipairs(BOATS) do
        boats[i] = { e = Entity.find_by_name(b[1]), x = b[2], y = b[3], phase = b[4] }
    end
    for i = 1, 2 do
        local c = Entity.find_by_name("Cloud" .. i)
        local x, y = Transform.get_position(c)
        clouds[i] = { e = c, x = x, y = y, speed = 9 + i * 5 }
    end
    for i, g in ipairs(GULLS) do
        gulls[i] = { a = Entity.find_by_name("Gull" .. (i - 1) .. "_0"), b = Entity.find_by_name("Gull" .. (i - 1) .. "_1"),
                     x = g[1], y = g[2] + 40, speed = g[3] }
    end
    apply(1)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    t = t + dt
    since = since + dt
    for k = 1, 4 do
        if Input.is_key_just_pressed(tostring(k)) then apply(k) end
    end
    if since > 5 then apply(mode % 4 + 1) end
    for _, b in ipairs(boats) do
        Transform.set_position(b.e, b.x + math.sin(t * 0.3 + b.phase) * 30, b.y + math.sin(t * 1.4 + b.phase) * 4)
        Transform.set_rotation(b.e, math.sin(t * 1.1 + b.phase) * 0.05)
    end
    for _, c in ipairs(clouds) do
        c.x = c.x + c.speed * dt
        if c.x > 820 then c.x = -820 end
        Transform.set_position(c.e, c.x, c.y)
    end
    for _, g in ipairs(gulls) do
        g.x = g.x + g.speed * dt
        if g.x > 700 then g.x = -700 end
        local up = math.floor(t * 4 + g.speed) % 2 == 0
        local y = g.y + math.sin(t * 1.3 + g.speed) * 12
        Transform.set_position(g.a, g.x, y)
        Transform.set_position(g.b, g.x, y)
        Sprite.set_color(g.a, 1, 1, 1, up and 1 or 0)
        Sprite.set_color(g.b, 1, 1, 1, up and 0 or 1)
    end
end
