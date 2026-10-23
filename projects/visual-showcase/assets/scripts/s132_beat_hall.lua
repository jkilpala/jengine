-- 132 Beat Hall: the floor, the speakers, the beams and the crowd move on
-- the beat of the music and change pattern on every bar. The Director's
-- `beat_clock` (124 bpm, four to the bar) follows the music's own playhead
-- and calls `on_beat` and `on_bar` here; between beats everything eases
-- from the clock's `phase` (Entity.get_custom(e, "BeatClock", "phase")), so
-- a pulse starts on the beat whatever the frame rate. M pauses the music:
-- the clock keeps time on its own until it comes back, then locks again.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/132-beat-hall.scene"
local COLS, ROWS = 13, 4
local PALETTES = {
    { 1.0, 0.15, 0.55 }, { 0.15, 0.75, 1.0 }, { 1.0, 0.65, 0.1 }, { 0.55, 0.25, 1.0 },
}
local tiles, cones, beams, dancers, dots = {}, {}, {}, {}, {}
local director, status, strip
local captions, music_on = true, true
local pattern, step, bar = 1, 0, 0
local sway = 1
local t = 0

function on_beat(entity)
    step = step + 1
    sway = -sway
end

function on_bar(entity)
    pattern = pattern % 4 + 1
    bar = bar + 1
end

-- How lit tile (r, c) is under the current pattern, 0 to 1.
local function lit(r, c)
    if pattern == 1 then
        -- A checkerboard that swaps on every beat.
        return ((r + c + step) % 2 == 0) and 1 or 0.08
    elseif pattern == 2 then
        -- Two columns chasing in from the sides.
        local k = step % COLS
        return (c == k or c == COLS - 1 - k) and 1 or 0.06
    elseif pattern == 3 then
        -- Rings opening out from the middle.
        local d = math.abs(c - (COLS - 1) / 2) + math.abs(r - (ROWS - 1) / 2) * 1.6
        return (math.floor(d) % 4 == (4 - step % 4) % 4) and 1 or 0.08
    end
    -- Sparkles: a hash of the tile and the beat.
    local h = (r * 73 + c * 151 + step * 977) % 101
    return h > 62 and 1 or 0.06
end

function init(entity)
    director = entity
    status = Entity.find_by_name("Status")
    strip = Entity.find_by_name("BoothStrip")
    for r = 0, ROWS - 1 do
        tiles[r] = {}
        for c = 0, COLS - 1 do tiles[r][c] = Entity.find_by_name("Tile" .. r .. "_" .. c) end
    end
    for i = 0, 1 do
        for j = 0, 1 do cones[#cones + 1] = Entity.find_by_name("Cone" .. i .. "_" .. j) end
    end
    for i = 0, 3 do beams[#beams + 1] = Entity.find_by_name("Beam" .. i) end
    for i = 0, 8 do
        local d = Entity.find_by_name("Dancer" .. i)
        local x, y = Transform.get_position(d)
        dancers[#dancers + 1] = { e = d, x = x, y = y, lean = 0 }
    end
    for k = 0, 3 do dots[#dots + 1] = Entity.find_by_name("Dot" .. k) end
    Audio.play_music("sounds/bh_club.wav", 0.8)
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("M") then
        music_on = not music_on
        if music_on then Audio.resume_music() else Audio.pause_music() end
    end
    t = t + dt
    local phase = Entity.get_custom(director, "BeatClock", "phase") or 0
    local in_bar = Entity.get_custom(director, "BeatClock", "beat_in_bar") or 0
    -- A kick's shape: full on the beat, gone by the next.
    local pulse = (1 - phase) ^ 2
    local col = PALETTES[bar % #PALETTES + 1]
    local alt = PALETTES[(bar + 1) % #PALETTES + 1]
    for r = 0, ROWS - 1 do
        for c = 0, COLS - 1 do
            local v = lit(r, c)
            local k = v * (0.45 + 1.6 * pulse) + 0.04
            local p = (pattern == 2 and c > (COLS - 1) / 2) and alt or col
            Sprite.set_color(tiles[r][c], p[1] * k, p[2] * k, p[3] * k, 1)
        end
    end
    for _, cone in ipairs(cones) do
        local s = 1 + 0.12 * pulse
        Transform.set_scale(cone, s, s)
    end
    for i, b in ipairs(beams) do
        local side = (i <= 2) and 1 or -1
        local a = side * (0.18 + 0.22 * math.sin((step + phase) * math.pi / 2 + i))
        Transform.set_rotation(b, a)
        local p = (i % 2 == 0) and col or alt
        Sprite.set_color(b, p[1] * 1.6, p[2] * 1.6, p[3] * 1.6, 0.25 + 0.6 * pulse)
    end
    for i, d in ipairs(dancers) do
        d.lean = d.lean + (sway * ((i % 2 == 0) and 0.07 or -0.05) - d.lean) * math.min(1, dt * 8)
        Transform.set_position(d.e, d.x, d.y + 12 * pulse)
        Transform.set_rotation(d.e, d.lean)
    end
    for k, dot in ipairs(dots) do
        local on = (k - 1) == in_bar
        local g = on and (1.2 + 2 * pulse) or 0.25
        Sprite.set_color(dot, col[1] * g, col[2] * g, col[3] * g, 1)
    end
    Sprite.set_color(strip, col[1] * 2, col[2] * 2, col[3] * 2, 0.3 + 0.5 * pulse)
    local beat = (Entity.get_custom(director, "BeatClock", "beat") or 0)
    UI.set_text(status, string.format("bar %d   beat %d   %s", math.floor(beat / 4) + 1, math.floor(in_bar) + 1,
        music_on and "on the music" or "music paused - keeping time alone"))
end
