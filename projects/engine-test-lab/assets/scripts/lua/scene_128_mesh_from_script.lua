-- Scene 128: Mesh.set. A flag authored as a textured mesh with no
-- vertices, shaped by this script every frame into a waving grid; and an
-- entity with no mesh, given a star fan.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/128-mesh-from-script.scene"

local COLS, ROWS = 16, 6
local W, H = 560, 220
local flag, star
local t = 0.0

-- Triangles over the grid, counted from 1 as Lua counts: fixed, so built once.
local indices = {}
local function vid(c, r) return r * (COLS + 1) + c + 1 end
for r = 0, ROWS - 1 do
    for c = 0, COLS - 1 do
        local a, b, d, e = vid(c, r), vid(c + 1, r), vid(c + 1, r + 1), vid(c, r + 1)
        for _, i in ipairs({ a, b, d, a, d, e }) do indices[#indices + 1] = i end
    end
end

local function wave()
    local vs = {}
    for r = 0, ROWS do
        for c = 0, COLS do
            local u, v = c / COLS, r / ROWS
            -- Pinned at the pole (u = 0), swinging more toward the free edge.
            local swing = math.sin(t * 3.0 - u * 5.0) * 26.0 * u
            local x = u * W
            local y = H / 2 - v * H + swing - u * u * 30.0
            local shade = 0.75 + 0.25 * math.cos(t * 3.0 - u * 5.0)
            vs[#vs + 1] = { x = x, y = y, u = u, v = v, r = shade, g = shade, b = shade }
        end
    end
    Mesh.set(flag, vs, indices)
end

local function breathe()
    local s = 1.0 + 0.15 * math.sin(t * 2.0)
    local vs = { { x = 0, y = 0, u = 0.5, v = 0.5, r = 1.0, g = 0.6, b = 0.3 } }
    for i = 0, 10 do
        local a = math.pi / 2 + i * math.pi / 5
        local rad = (i % 2 == 0) and 110 or 45
        vs[#vs + 1] = { x = math.cos(a) * rad * s, y = math.sin(a) * rad * s, u = 0.5, v = 0.5 }
    end
    Mesh.set(star, vs)
end

function init(entity)
    flag = Entity.find_by_name("Flag")
    star = Entity.find_by_name("Star")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    t = t + dt
    if flag then wave() end
    if star then breathe() end
end
