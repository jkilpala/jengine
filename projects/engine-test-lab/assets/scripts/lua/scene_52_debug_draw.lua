-- Scene 52 - Debug Draw
--
-- Three one-frame primitives in world space:
--
--   Debug.draw_line(x1, y1, x2, y2, r, g, b)
--   Debug.draw_circle(x, y, radius, r, g, b)
--   Debug.draw_rect(x, y, w, h, r, g, b)
--
-- The queue is drained every frame, so a shape lives exactly as long as you
-- keep submitting it. That is what makes them right for visualising live state
-- (a path, a radius, a hitbox) and wrong for scene content - stop calling and
-- it is gone.
--
-- Debug.set_overlay(bool) toggles the engine's own F3 frame overlay from
-- script, which is the same overlay the key toggles.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/52-debug-draw.scene"

local t = 0.0
local frozen = false
local overlay = false

function init(entity)
    log("[scene_52] ready - Space freezes the sweep, O toggles the F3 overlay")
end

function update(entity, dt)
    if Input.is_key_just_pressed("Space") then frozen = not frozen end
    if Input.is_key_just_pressed("O") then
        overlay = not overlay
        Debug.set_overlay(overlay)
    end
    if not frozen then t = t + dt end

    -- A static frame, redrawn every frame because that is the contract.
    Debug.draw_rect(-560, -240, 1120, 480, 0.25, 0.3, 0.45)
    Debug.draw_circle(0, 20, 200, 0.35, 0.45, 0.65)
    Debug.draw_circle(0, 20, 120, 0.3, 0.4, 0.6)

    -- A rotating sweep: the line and the dot at its end.
    local a = t * 1.1
    local ex, ey = math.cos(a) * 200, 20 + math.sin(a) * 200
    Debug.draw_line(0, 20, ex, ey, 1.0, 0.85, 0.3)
    Debug.draw_circle(ex, ey, 16, 1.0, 0.5, 0.3)

    -- A trail of fading squares behind the sweep.
    for i = 1, 8 do
        local ta = a - i * 0.12
        local tx, ty = math.cos(ta) * 200, 20 + math.sin(ta) * 200
        local f = 1.0 - i / 9.0
        Debug.draw_rect(tx - 6, ty - 6, 12, 12, f, f * 0.6, 0.2)
    end

    -- A pulsing box on the left and a lissajous on the right, so line, rect
    -- and circle are all visibly live at once.
    local s = 60 + math.sin(t * 2.0) * 30
    Debug.draw_rect(-430 - s / 2, 20 - s / 2, s, s, 0.5, 0.9, 0.5)
    local lx = 420 + math.sin(t * 1.7) * 90
    local ly = 20 + math.sin(t * 2.3) * 90
    Debug.draw_circle(lx, ly, 20, 0.9, 0.5, 1.0)
    Debug.draw_line(420, 20, lx, ly, 0.6, 0.35, 0.8)

    local r = Entity.find_by_name("Readout")
    if r then
        Text.set(r, string.format("%s   |   F3 overlay via script: %s",
            frozen and "frozen (Space)" or "sweeping",
            overlay and "on" or "off"))
    end

    nav.tick(SCENE_PATH)
end
