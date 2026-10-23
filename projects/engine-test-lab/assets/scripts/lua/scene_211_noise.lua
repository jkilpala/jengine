-- Scene 211: noise. Six panels of one tilemap, each 36 x 36 cells of a
-- sixteen-shade ramp: Perlin, simplex, cellular (f2 - f1), and fractal sums -
-- fbm, ridged, and turbulence over Perlin in 3D with time as z. Every panel
-- but the cellular one is one Noise.grid call.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/211-noise.scene"
local N, GAP = 36, 4

local map, line
local seed = 1
local ox, oy = 0, 0
local freq = 0.12
local t = 0
local churn = 0

local function shade(v, lo, hi)
    local u = (v - lo) / (hi - lo)
    return math.floor(math.max(0, math.min(0.999, u)) * 16) + 1
end

local function paint(panel, values, lo, hi)
    local c0 = (panel % 3) * (N + GAP)
    local r0 = math.floor(panel / 3) * (N + GAP)
    for r = 0, N - 1 do
        for c = 0, N - 1 do
            Tilemap.set_tile(map, "noise", c0 + c, r0 + r, shade(values[r * N + c + 1], lo, hi))
        end
    end
end

local function grid(opts)
    opts.seed = seed
    opts.frequency = freq
    return Noise.grid(ox, oy, N, N, 1, opts)
end

local function draw_still()
    paint(0, grid({ kind = "perlin", octaves = 1 }), -0.7, 0.7)
    paint(1, grid({ kind = "simplex", octaves = 1 }), -1, 1)
    local cells = {}
    for r = 0, N - 1 do
        for c = 0, N - 1 do
            local f1, f2 = Noise.cellular((ox + c) * freq * 1.4, (oy + r) * freq * 1.4, seed)
            cells[r * N + c + 1] = 1 - (f2 - f1)
        end
    end
    paint(2, cells, 0.45, 1)
    paint(3, grid({ kind = "simplex", octaves = 5 }), -0.6, 0.6)
    paint(4, grid({ kind = "perlin", mode = "ridged", octaves = 5 }), 0.2, 0.9)
    UI.set_text(line, "seed " .. seed .. "   offset (" .. ox .. ", " .. oy .. ")   frequency "
        .. string.format("%.2f", freq))
end

local function draw_churn()
    paint(5, grid({ kind = "perlin3", mode = "turbulence", octaves = 4, z = t * 0.6 }), 0, 0.6)
end

function init(entity)
    map = Entity.find_by_name("Map")
    line = Entity.find_by_name("Line0")
    draw_still()
    draw_churn()
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    local key = Input.is_key_just_pressed
    local changed = false
    if key("Space") then seed = seed + 1; changed = true end
    if key("Left") then ox = ox - 4; changed = true end
    if key("Right") then ox = ox + 4; changed = true end
    if key("Up") then oy = oy - 4; changed = true end
    if key("Down") then oy = oy + 4; changed = true end
    if key("Minus") then freq = math.min(0.5, freq * 1.25); changed = true end
    if key("Equal") then freq = math.max(0.02, freq / 1.25); changed = true end
    if changed then draw_still() end
    t = t + dt
    churn = churn + dt
    if churn >= 0.1 then
        churn = 0
        draw_churn()
    end
end
