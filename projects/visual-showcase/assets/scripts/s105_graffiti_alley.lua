-- 105 Graffiti Alley: taggers spray bubble letters into a brick wall.
-- Every stroke is stamped twice: a `spray` of colour (Sprite.paint), and a
-- `gloss` that wets the wall's normal map, so fresh paint shines under the
-- lamp and the passing headlights and dries matt (`dry_seconds` on the
-- paintable). The letters are strokes in a little font below; a piece is an
-- outline pass, a fill pass, highlights and drips.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/105-graffiti-alley.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end

local SLOTS = { { -660, -250 }, { -220, -250 }, { 220, -250 }, { 660, -250 } }
local OLD = { { "NEON", -380, 300, 190, { 0.85, 0.35, 0.6 }, { 0.18, 0.06, 0.2 } }, { "ZAP", 420, 330, 170, { 0.4, 0.75, 0.3 }, { 0.06, 0.12, 0.06 } }, { "OK", 820, 90, 150, { 0.9, 0.6, 0.2 }, { 0.15, 0.08, 0.04 } }, { "GIN", -760, 60, 140, { 0.3, 0.6, 0.85 }, { 0.04, 0.08, 0.16 } } }
local FEET = -440.000000
local SCALE = 1.600000
local SHOULDER = { -30 * SCALE / 1.6, 198 * SCALE / 1.6 }
local REACH = 108 * SCALE / 1.6
local BUFF = { 0.33, 0.31, 0.3 }

-- Strokes in a unit box, y up.
local CIRCLE = {}
for i = 0, 16 do
    local a = math.pi / 2 + i / 16 * 2 * math.pi
    CIRCLE[#CIRCLE + 1] = { 0.5 + 0.5 * math.cos(a), 0.5 + 0.5 * math.sin(a) }
end
local FONT = {
    Z = { { { 0, 1 }, { 1, 1 }, { 0, 0 }, { 1, 0 } } },
    A = { { { 0, 0 }, { 0.5, 1 }, { 1, 0 } }, { { 0.22, 0.4 }, { 0.78, 0.4 } } },
    P = { { { 0, 0 }, { 0, 1 }, { 0.75, 1 }, { 1, 0.86 }, { 1, 0.64 }, { 0.75, 0.5 }, { 0, 0.5 } } },
    O = { CIRCLE },
    K = { { { 0, 0 }, { 0, 1 } }, { { 1, 1 }, { 0, 0.45 }, { 1, 0 } } },
    E = { { { 1, 1 }, { 0, 1 }, { 0, 0 }, { 1, 0 } }, { { 0, 0.5 }, { 0.75, 0.5 } } },
    N = { { { 0, 0 }, { 0, 1 }, { 1, 0 }, { 1, 1 } } },
    J = { { { 1, 1 }, { 1, 0.25 }, { 0.8, 0 }, { 0.2, 0 }, { 0, 0.25 } } },
    I = { { { 0.5, 0 }, { 0.5, 1 } } },
    G = { { { 1, 0.85 }, { 0.8, 1 }, { 0.2, 1 }, { 0, 0.75 }, { 0, 0.25 }, { 0.2, 0 }, { 0.8, 0 }, { 1, 0.25 },
            { 1, 0.5 }, { 0.55, 0.5 } } },
    W = { { { 0, 1 }, { 0.22, 0 }, { 0.5, 0.6 }, { 0.78, 0 }, { 1, 1 } } },
    R = { { { 0, 0 }, { 0, 1 }, { 0.75, 1 }, { 1, 0.86 }, { 1, 0.64 }, { 0.75, 0.5 }, { 0, 0.5 }, { 1, 0 } } },
    S = { { { 1, 0.85 }, { 0.8, 1 }, { 0.2, 1 }, { 0, 0.8 }, { 0.15, 0.55 }, { 0.85, 0.45 }, { 1, 0.2 }, { 0.8, 0 },
            { 0.2, 0 }, { 0, 0.15 } } },
}
local WORDS = { "ZAP", "OKE", "NEON", "JIG", "WIRE", "SPIN", "GRAZ", "ROK" }
local PALETTES = {
    { { 1.0, 0.25, 0.55 }, { 0.12, 0.02, 0.1 } },
    { { 0.6, 0.95, 0.1 }, { 0.02, 0.05, 0.02 } },
    { { 1.0, 0.55, 0.05 }, { 0.03, 0.04, 0.14 } },
    { { 0.1, 0.85, 0.95 }, { 0.25, 0.02, 0.22 } },
    { { 0.95, 0.9, 0.2 }, { 0.2, 0.05, 0.02 } },
}

local e = {}
local t = 0
local rng = 105
local slot_taken = {}
local pieces = 0
local drips = {}
local backlog = {}
local captions = true
local crew = {}
local headlight_wait = 4
local headlight_x = nil

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

-- A word's strokes in world points: letters slanted, side by side.
local function word_strokes(word, cx, cy, h)
    local w = h * 0.62
    local adv = w * 1.32
    local total = adv * (#word - 1) + w
    local out = {}
    for n = 1, #word do
        local glyph = FONT[word:sub(n, n)] or FONT.O
        local ox = cx - total / 2 + (n - 1) * adv
        for _, stroke in ipairs(glyph) do
            local pts = {}
            for _, p in ipairs(stroke) do
                local y = cy - h / 2 + p[2] * h
                pts[#pts + 1] = { ox + p[1] * w + (p[2] - 0.5) * h * 0.22, y }
            end
            out[#out + 1] = pts
        end
    end
    return out
end

-- The wall at once (the old pieces): queued, drawn a little each frame,
-- since a sprite keeps a thousand stamps at most between two frames.
local function queue_word(word, cx, cy, h, fill, outline)
    for pass = 1, 2 do
        local size = pass == 1 and h * 0.34 or h * 0.22
        local col = pass == 1 and outline or fill
        for _, stroke in ipairs(word_strokes(word, cx, cy, h)) do
            for i = 1, #stroke - 1 do
                local a, b = stroke[i], stroke[i + 1]
                local len = math.sqrt((b[1] - a[1]) ^ 2 + (b[2] - a[2]) ^ 2)
                local n = math.max(1, math.floor(len / (size * 0.25)))
                for k = 0, n do
                    local f = k / n
                    backlog[#backlog + 1] = { a[1] + (b[1] - a[1]) * f, a[2] + (b[2] - a[2]) * f, size, col }
                end
            end
        end
    end
end

-- A stamp of paint: a solid core, a speckled overspray round it, and - when
-- fresh - the wet shine.
local function spray(x, y, size, col, wet)
    Sprite.paint(e.Wall, x, y, size * 1.35, col[1], col[2], col[3], 0.3, "spray", 0)
    Sprite.paint(e.Wall, x, y, size, col[1], col[2], col[3], 0.95, "disc", 0)
    if wet then Sprite.paint(e.Wall, x, y, size * 1.1, 1, 1, 1, 1.0, "disc", 0, "gloss") end
end

-- ── a crew member's job ─────────────────────────────────────────────────

local function free_slot()
    local free = {}
    for i = 1, #SLOTS do if not slot_taken[i] then free[#free + 1] = i end end
    if #free == 0 then return nil end
    return free[1 + math.floor(rand() * #free)]
end

local function plan_piece(m, slot)
    local s = SLOTS[slot]
    local word = WORDS[1 + math.floor(rand() * #WORDS)]
    local pal = PALETTES[1 + math.floor(rand() * #PALETTES)]
    local h = 140
    local strokes = word_strokes(word, s[1], s[2], h)
    local passes = {
        { strokes = strokes, size = h * 0.34, col = pal[2], speed = 520 },
        { strokes = strokes, size = h * 0.22, col = pal[1], speed = 380 },
    }
    -- Highlights: a thin white line up and to the left of a few strokes.
    local hl = {}
    for i, stroke in ipairs(strokes) do
        if i % 2 == 1 then
            local pts = {}
            for _, p in ipairs(stroke) do pts[#pts + 1] = { p[1] - h * 0.04, p[2] + h * 0.04 } end
            hl[#hl + 1] = pts
        end
    end
    passes[#passes + 1] = { strokes = hl, size = h * 0.05, col = { 1, 1, 1 }, speed = 300 }
    m.job = { kind = "piece", slot = slot, passes = passes, pass = 1, stroke = 1, seg = 1, along = 0, since = 0,
              fill = pal[1], drips = 3 + math.floor(rand() * 4) }
end

local function plan_buff(m)
    -- Rolled grey over the lower wall, a band at a time.
    local strokes = {}
    for row = 0, 3 do
        local y = -330 + row * 52
        strokes[#strokes + 1] = { { -930, y }, { 930, y } }
    end
    m.job = { kind = "buff", passes = { { strokes = strokes, size = 90, col = BUFF, speed = 700 } },
              pass = 1, stroke = 1, seg = 1, along = 0, since = 0 }
end

local function finish(m)
    local job = m.job
    if job.kind == "piece" then
        pieces = pieces + 1
        -- Drips from points along the fill.
        local fill = job.passes[2].strokes
        for _ = 1, job.drips do
            local stroke = fill[1 + math.floor(rand() * #fill)]
            local p = stroke[1 + math.floor(rand() * #stroke)]
            drips[#drips + 1] = { x = p[1] + rand() * 8 - 4, y = p[2] - 8, left = 30 + rand() * 70,
                                  speed = 22 + rand() * 30, col = job.fill }
        end
    else
        for i = 1, #SLOTS do slot_taken[i] = nil end
    end
    m.job = nil
    m.leaving = true
end

-- Advance a job's nozzle along its strokes; spray as it goes. Returns where
-- the nozzle is.
local function work(m, dt)
    local job = m.job
    local pass = job.passes[job.pass]
    local stroke = pass.strokes[job.stroke]
    local a, b = stroke[job.seg], stroke[job.seg + 1]
    local dx, dy = b[1] - a[1], b[2] - a[2]
    local len = math.max(1e-3, math.sqrt(dx * dx + dy * dy))
    job.along = job.along + pass.speed * dt
    local step = pass.size * 0.3
    while job.since <= job.along do
        local f = math.min(1, job.since / len)
        spray(a[1] + dx * f, a[2] + dy * f, pass.size, pass.col, true)
        job.since = job.since + step
    end
    local f = math.min(1, job.along / len)
    local nx, ny = a[1] + dx * f, a[2] + dy * f
    if job.along >= len then
        job.along, job.since = 0, 0
        job.seg = job.seg + 1
        if job.seg >= #stroke then
            job.seg = 1
            job.stroke = job.stroke + 1
            if job.stroke > #pass.strokes then
                job.stroke = 1
                job.pass = job.pass + 1
                if job.pass > #job.passes then finish(m) end
            end
        end
    end
    return nx, ny, pass.col
end

local function crew_member(name, buffer)
    return { name = name, body = Entity.find_by_name(name), arm = Entity.find_by_name(name .. "Arm"),
             mist = Entity.find_by_name(name .. "Mist"), x = -1300, buffer = buffer, job = nil,
             leaving = false, wait = buffer and 1e9 or 1 + rand() * 3, spraying = false }
end

local function step_crew(m, dt)
    if m.leaving then
        m.x = m.x - 320 * dt
        if m.x < -1300 then
            m.leaving = false
            m.wait = 2 + rand() * 4
        end
    elseif not m.job then
        m.wait = m.wait - dt
        if m.wait <= 0 then
            if m.buffer then
                plan_buff(m)
            else
                local slot = free_slot()
                if slot then
                    slot_taken[slot] = true
                    plan_piece(m, slot)
                else
                    m.wait = 3
                end
            end
        end
    end
    local nx, ny, col
    if m.job then
        -- Walk to where the nozzle is going before spraying.
        local job = m.job
        local stroke = job.passes[job.pass].strokes[job.stroke]
        local target = stroke[job.seg]
        local want = target[1] - SHOULDER[1] - 50
        if math.abs(want - m.x) > 40 and job.along == 0 and job.since == 0 then
            m.x = m.x + math.max(-320 * dt, math.min(320 * dt, want - m.x))
        else
            m.x = m.x + (want - m.x) * math.min(1, dt * 3)
            nx, ny, col = work(m, dt)
        end
    end
    local bob = (m.job or m.leaving) and math.abs(math.sin(t * 8)) * 6 or 0
    Transform.set_position(m.body, m.x, FEET + bob)
    local sx, sy = m.x + SHOULDER[1], FEET + bob + SHOULDER[2]
    Transform.set_position(m.arm, sx, sy)
    local spraying = nx ~= nil
    if spraying then
        local a = atan2(ny - sy, nx - sx)
        local d = math.sqrt((nx - sx) ^ 2 + (ny - sy) ^ 2)
        Transform.set_rotation(m.arm, a)
        Transform.set_scale(m.arm, math.max(0.4, math.min(1.3, d / REACH)), 1)
        Transform.set_position(m.mist, nx, ny)
        if col ~= m.mist_col then
            m.mist_col = col
            Particles.set(m.mist, "color_start", { col[1], col[2], col[3], 0.3 })
            Particles.set(m.mist, "color_end", { col[1], col[2], col[3], 0.0 })
        end
    else
        Transform.set_rotation(m.arm, -1.3)
        Transform.set_scale(m.arm, 0.8, 1)
    end
    if spraying ~= m.spraying then
        m.spraying = spraying
        Particles.set_active(m.mist, spraying)
    end
end

function init(entity)
    for _, n in ipairs({ "Wall", "Headlights", "Neon", "NeonLight", "Status" }) do e[n] = Entity.find_by_name(n) end
    crew = { crew_member("TaggerA", false), crew_member("TaggerB", false), crew_member("Buffer", true) }
    for _, o in ipairs(OLD) do queue_word(o[1], o[2], o[3], o[4], o[5], o[6]) end
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    -- The old pieces, a few hundred stamps a frame.
    for _ = 1, math.min(#backlog, 400) do
        local b = table.remove(backlog)
        spray(b[1], b[2], b[3], { b[4][1] * 0.75, b[4][2] * 0.75, b[4][3] * 0.75 }, false)
    end
    for _, m in ipairs(crew) do step_crew(m, dt) end
    -- The buffer comes when every slot is taken and the taggers are gone.
    local full = true
    for i = 1, #SLOTS do if not slot_taken[i] then full = false end end
    local buffer = crew[3]
    if full and not buffer.job and not buffer.leaving and buffer.wait > 1e8 then
        local busy = crew[1].job or crew[2].job
        if not busy then buffer.wait = 6 end
    end
    -- Drips run down the wall.
    for i = #drips, 1, -1 do
        local d = drips[i]
        local step = d.speed * dt
        d.y = d.y - step
        d.left = d.left - step
        d.speed = d.speed * (1 - dt * 0.4)
        spray(d.x, d.y, 5, d.col, true)
        if d.left <= 0 then
            Sprite.paint(e.Wall, d.x, d.y - 2, 8, d.col[1], d.col[2], d.col[3], 0.95, "disc", 0)
            table.remove(drips, i)
        end
    end
    -- A car goes by now and then; the neon stutters.
    if headlight_x then
        headlight_x = headlight_x - 900 * dt
        Transform.set_position(e.Headlights, headlight_x, -360)
        Lighting.set_intensity(e.Headlights, 2.4)
        if headlight_x < -1500 then
            headlight_x = nil
            Lighting.set_intensity(e.Headlights, 0)
            headlight_wait = 9 + rand() * 8
        end
    else
        headlight_wait = headlight_wait - dt
        if headlight_wait <= 0 then headlight_x = 1500 end
    end
    local flick = (math.sin(t * 37) > 0.93 and rand() < 0.5) and 0.15 or 1.0
    Lighting.set_intensity(e.NeonLight, 1.3 * flick)
    Text.set_color(e.Neon, 5.0 * flick, 1.25 * flick, 3.0 * flick, 1)
    UI.set_text(e.Status, string.format("pieces %d   drips running %d", math.floor(pieces), math.floor(#drips)))
end
