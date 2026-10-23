-- 104 Wildfire: a fire on a grid of cells over the meadow. Each cell has
-- fuel (noise; none on the firebreaks or the river); a burning cell lights
-- its neighbours, downwind more readily, then smoulders, then is ash. The
-- script decides who burns - and paints it: each burning cell into three
-- transparent canvases (the glowing front, embers, smoke) that the flames,
-- embers and smoke are emitted from (`emit_shape: sprite`), rubbed out of
-- them as it dies (`Sprite.paint` with "erase"), and into the meadow as char
-- and then ash.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/104-wildfire.scene"

local CELL, GW, GH = 20, 96, 54
local X0, Y0 = -960, -540
local TREES = { { -560, 380 }, { -470, 430 }, { -640, -330 }, { -120, 300 }, { -60, 380 }, { -30, -360 }, { 60, -280 }, { -400, -20 }, { 120, 60 }, { 700, 300 }, { 760, -200 }, { 820, 120 }, { -820, -60 }, { -140, -120 } }
local BREAKS = { { -262, -560, -232, 300 }, { -960, -152, -500, -124 } }
local RIVER_HALF = 35.000000
local START = { -700, 260 }

local UNBURNT, BURNING, SMOULDER, ASH = 0, 1, 2, 3

local e = {}
local fuel, state, timer, tree_of, wet = {}, {}, {}, {}, {}
local active = {}          -- cells burning or smouldering
local tree_state = {}
local burning, burnt = 0, 0
local t, tick = 0, 0
local wind_a, wind_k = 0.15, 1.6
local lit = false
local quiet = 0
local captions = true
local rng = 104
local bomber = { x = -1400, y = 0, wait = 10, flying = false, dropping = false }
local strike = 0

local function rand()
    rng = (rng * 16807) % 2147483647
    return rng / 2147483647
end

local function river_x(y)
    return 360 + 140 * math.sin(y * 0.0042 + 0.6) + 35 * math.sin(y * 0.013)
end

local function centre(i, j) return X0 + (i - 0.5) * CELL, Y0 + (j - 0.5) * CELL end
local function index(i, j) return (j - 1) * GW + i end

local function on_break(x, y)
    for _, b in ipairs(BREAKS) do
        if x > b[1] - 10 and x < b[3] + 10 and y > b[2] - 10 and y < b[4] + 10 then return true end
    end
    return false
end

local function paint_all(x, y, size, r, g, b, a, blend)
    Sprite.paint(e.Heat, x, y, size, r, g, b, a, "disc", 0, blend)
    Sprite.paint(e.Embers, x, y, size * 0.9, 1, 1, 1, a, "disc", 0, blend)
    Sprite.paint(e.Smoke, x, y, size * 1.1, 1, 1, 1, a, "disc", 0, blend)
end

local function ignite(k, i, j)
    if state[k] ~= UNBURNT or fuel[k] <= 0.05 then return end
    state[k] = BURNING
    timer[k] = 1.2 + 2.4 * fuel[k] + rand() * 0.8
    active[#active + 1] = k
    burning = burning + 1
    local x, y = centre(i, j)
    Sprite.paint(e.Heat, x, y, 30, 1.0, 0.62, 0.25, 0.95, "disc", 0, "over")
    Sprite.paint(e.Embers, x, y, 28, 1, 1, 1, 1, "disc", 0, "over")
    Sprite.paint(e.Smoke, x, y, 34, 1, 1, 1, 1, "disc", 0, "over")
    local tr = tree_of[k]
    if tr and tree_state[tr] == 0 then
        tree_state[tr] = 1
        local p = TREES[tr]
        Sprite.paint(e.Heat, p[1], p[2], 110, 1.0, 0.7, 0.3, 1.0, "disc", 0, "over")
        Sprite.paint(e.Embers, p[1], p[2], 90, 1, 1, 1, 1, "disc", 0, "over")
        Sprite.set_color(e["Oak" .. (tr - 1)], 0.35, 0.22, 0.12, 1)
    end
end

local function douse(cx, cy, radius)
    local i0 = math.max(1, math.floor((cx - radius - X0) / CELL) + 1)
    local i1 = math.min(GW, math.floor((cx + radius - X0) / CELL) + 1)
    local j0 = math.max(1, math.floor((cy - radius - Y0) / CELL) + 1)
    local j1 = math.min(GH, math.floor((cy + radius - Y0) / CELL) + 1)
    local hissed = false
    for j = j0, j1 do
        for i = i0, i1 do
            local x, y = centre(i, j)
            if (x - cx) ^ 2 + (y - cy) ^ 2 < radius * radius then
                local k = index(i, j)
                if state[k] == BURNING or state[k] == SMOULDER then
                    if state[k] == BURNING then burning = burning - 1 end
                    state[k] = ASH
                    burnt = burnt + 1
                    paint_all(x, y, 46, 0, 0, 0, 1, "erase")
                    hissed = true
                elseif state[k] == UNBURNT then
                    state[k] = ASH
                end
                fuel[k] = 0
                -- Wet ground, darkened in - once a cell, or a drop held over
                -- it a second darkens it to black.
                if not wet[k] then
                    wet[k] = true
                    Sprite.paint(e.Meadow, x, y, 30, 0.6, 0.66, 0.76, 0.5, "disc", 0, "multiply")
                end
            end
        end
    end
    if hissed then
        Transform.set_position(e.Steam, cx, cy)
        Particles.emit_burst(e.Steam, 6)
    end
end

local function step_fire(dt)
    local wx, wy = math.cos(wind_a), math.sin(wind_a)
    -- The cells alight when the tick began; those ignited during it are
    -- kept for the next one.
    local n = #active
    local keep = {}
    for i = 1, n do
        local k = active[i]
        local ci = (k - 1) % GW + 1
        local cj = math.floor((k - 1) / GW) + 1
        timer[k] = timer[k] - dt
        local x, y = centre(ci, cj)
        if state[k] == BURNING then
            for dj = -1, 1 do
                for di = -1, 1 do
                    local ni, nj = ci + di, cj + dj
                    if (di ~= 0 or dj ~= 0) and ni >= 1 and ni <= GW and nj >= 1 and nj <= GH then
                        local nk = index(ni, nj)
                        if state[nk] == UNBURNT and fuel[nk] > 0.05 then
                            local len = math.sqrt(di * di + dj * dj)
                            local push = 1 + wind_k * (di * wx + dj * wy) / len
                            local p = 0.55 * fuel[nk] * math.max(0.05, push) ^ 2 * dt / len
                            if rand() < p then ignite(nk, ni, nj) end
                        end
                    end
                end
            end
            if timer[k] <= 0 then
                state[k] = SMOULDER
                timer[k] = 3 + rand() * 3
                burning = burning - 1
                Sprite.paint(e.Heat, x, y, 26, 0, 0, 0, 0.65, "disc", 0, "erase")
                Sprite.paint(e.Meadow, x, y, 30, 0.05, 0.04, 0.035, 0.85, "disc", 0, "over")
            end
            keep[#keep + 1] = k
        elseif state[k] ~= SMOULDER then
            -- Doused while alight: already ash, already rubbed out.
        elseif timer[k] <= 0 then
            state[k] = ASH
            burnt = burnt + 1
            paint_all(x, y, 46, 0, 0, 0, 1, "erase")
            Sprite.paint(e.Meadow, x + rand() * 8 - 4, y + rand() * 8 - 4, 36, 0.24, 0.23, 0.22, 0.28, "disc", 0, "over")
            local tr = tree_of[k]
            if tr and tree_state[tr] == 1 then
                tree_state[tr] = 2
                Sprite.set_color(e["Oak" .. (tr - 1)], 0.1, 0.08, 0.07, 1)
            end
        else
            keep[#keep + 1] = k
        end
    end
    for i = n + 1, #active do keep[#keep + 1] = active[i] end
    active = keep
end

local function set_wind(a)
    wind_a = a
    for _, name in ipairs({ "Heat", "Embers", "Smoke" }) do
        local spread = name == "Smoke" and 0.3 or 0.45
        Particles.set(e[name], "angle", { a - spread, a + spread })
    end
end

local function lights(dt)
    -- Hang the fire lights over burning cells, a few at a time.
    for n = 0, 7 do
        local light = e["FireLight" .. n]
        if burning > 0 and #active > 0 then
            if rand() < dt * 3 then
                for _ = 1, 6 do
                    local k = active[1 + math.floor(rand() * #active)]
                    if state[k] == BURNING then
                        local x, y = centre((k - 1) % GW + 1, math.floor((k - 1) / GW) + 1)
                        Transform.set_position(light, x, y)
                        break
                    end
                end
            end
            Lighting.set_intensity(light, 1.4 + 0.5 * math.sin(t * 17 + n * 2.3) + 0.3 * rand())
        else
            Lighting.set_intensity(light, 0)
        end
    end
end

local function fly(dt)
    local b = bomber
    if not b.flying then
        b.wait = b.wait - dt
        if b.wait <= 0 and burning > 8 then
            -- A pass along the middle of the front.
            local sy, n = 0, 0
            for _, k in ipairs(active) do
                if state[k] == BURNING then
                    sy = sy + (Y0 + (math.floor((k - 1) / GW) + 0.5) * CELL)
                    n = n + 1
                end
            end
            b.y = n > 0 and sy / n or 0
            b.x = -1250
            b.flying = true
        end
        return
    end
    b.x = b.x + 330 * dt
    Transform.set_position(e.Bomber, b.x, b.y + math.sin(t * 1.3) * 6)
    Transform.set_rotation(e.Bomber, math.sin(t * 1.3) * 0.03)
    -- Drop over fire.
    local i = math.floor((b.x - X0) / CELL) + 1
    local j = math.floor((b.y - Y0) / CELL) + 1
    local over = false
    for dj = -3, 3 do
        for di = -2, 2 do
            local ni, nj = i + di, j + dj
            if ni >= 1 and ni <= GW and nj >= 1 and nj <= GH and state[index(ni, nj)] == BURNING then over = true end
        end
    end
    if over ~= b.dropping then
        b.dropping = over
        Particles.set_active(e.Drop, over)
    end
    if over then douse(b.x - 20, b.y, 70) end
    if b.x > 1250 then
        b.flying, b.dropping = false, false
        Particles.set_active(e.Drop, false)
        b.wait = 7
    end
end

function init(entity)
    for _, n in ipairs({ "Meadow", "Heat", "Embers", "Smoke", "Strike", "Bomber", "Drop", "Steam", "Status" }) do
        e[n] = Entity.find_by_name(n)
    end
    for n = 0, 7 do e["FireLight" .. n] = Entity.find_by_name("FireLight" .. n) end
    for n = 1, #TREES do
        e["Oak" .. (n - 1)] = Entity.find_by_name("Oak" .. (n - 1))
        tree_state[n] = 0
    end
    for j = 1, GH do
        for i = 1, GW do
            local x, y = centre(i, j)
            local k = index(i, j)
            local f = 0.55 + 1.1 * Noise.perlin(x / 300, y / 300, 104)
            if on_break(x, y) or math.abs(x - river_x(y)) < RIVER_HALF + 12 then f = 0 end
            fuel[k] = math.max(0, math.min(1, f))
            state[k] = UNBURNT
            timer[k] = 0
        end
    end
    for n, p in ipairs(TREES) do
        for j = 1, GH do
            for i = 1, GW do
                local x, y = centre(i, j)
                if (x - p[1]) ^ 2 + (y - p[2]) ^ 2 < 50 * 50 then
                    local k = index(i, j)
                    tree_of[k] = n
                    fuel[k] = 1
                end
            end
        end
    end
    set_wind(wind_a)
end

function update(entity, dt)
    nav.tick(SCENE)
    t = t + dt
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("W") then set_wind(wind_a + math.pi / 4) end
    -- The lightning that starts it.
    if not lit and t > 1.2 then
        lit = true
        strike = 0.5
        Transform.set_position(e.Strike, START[1], START[2])
        Camera.flash(0.12, 0.85, 0.9, 1.0, 0.7)
        Camera.shake(6, 0.3, 30)
        Sprite.paint(e.Meadow, START[1], START[2], 70, 0.04, 0.035, 0.03, 0.9, "splat", 0.4, "over")
        local i = math.floor((START[1] - X0) / CELL) + 1
        local j = math.floor((START[2] - Y0) / CELL) + 1
        for dj = -1, 1 do
            for di = -1, 1 do ignite(index(i + di, j + dj), i + di, j + dj) end
        end
    end
    if strike > 0 then
        strike = math.max(0, strike - dt)
        Lighting.set_intensity(e.Strike, (strike / 0.5) ^ 2 * 6.0 * (0.6 + 0.4 * rand()))
    end
    -- The wind wanders a little round where it was set.
    tick = tick + dt
    if tick >= 0.1 then
        step_fire(tick)
        tick = 0
    end
    lights(dt)
    fly(dt)
    -- Once it is out, it starts again.
    if lit and #active == 0 then
        quiet = quiet + dt
        if quiet > 5 then Scene.load(SCENE) end
    else
        quiet = 0
    end
    local arrows = { "E", "NE", "N", "NW", "W", "SW", "S", "SE" }
    local w = arrows[(math.floor((wind_a % (2 * math.pi)) / (math.pi / 4) + 0.5) % 8) + 1]
    UI.set_text(e.Status, string.format("burning %d   burnt %d   wind %s", math.floor(burning), math.floor(burnt), w))
end
