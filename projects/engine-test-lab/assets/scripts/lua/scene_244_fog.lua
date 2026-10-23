-- Scene 244: `fog`. A lamp moves behind a row of posts in a fog; F, S and
-- N step the fog's density, scatter and billows through `Fog.set`.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/244-fog.scene"
local lamp, mist, line
local t = 0
local paused = false
local densities, scatters, noises = { 0.7, 0.95, 0.35, 0.0 }, { 30, 70, 0 }, { 0.7, 0.0 }
local di, si, ni = 1, 1, 1

function init(entity)
    lamp = Entity.find_by_name("Lamp")
    mist = Entity.find_by_name("Mist")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then paused = not paused end
    if Input.is_key_just_pressed("F") then di = di % #densities + 1; Fog.set(mist, "density", densities[di]) end
    if Input.is_key_just_pressed("S") then si = si % #scatters + 1; Fog.set(mist, "scatter", scatters[si]) end
    if Input.is_key_just_pressed("N") then ni = ni % #noises + 1; Fog.set(mist, "noise_amount", noises[ni]) end
    if not paused then t = t + dt end
    Transform.set_position(lamp, math.sin(t * 0.5) * 380, 120 + math.sin(t * 0.8) * 30)
    UI.set_text(line, string.format("density %.2f   scatter %d   billows %.1f", densities[di], scatters[si], noises[ni]))
end
