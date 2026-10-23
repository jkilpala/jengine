-- Scene 256: `text_path`. Words along a line the script bends every frame
-- and along a closed ring, where `scroll` runs them round.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/256-text-path.scene"
local wave, ring_text, line
local t, moving, scrolling = 0, true, true

function init(entity)
    wave = Entity.find_by_name("Wave")
    ring_text = Entity.find_by_name("RingText")
    line = Entity.find_by_name("Line0")
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("S") then
        scrolling = not scrolling
        TextPath.set(ring_text, "scroll", scrolling and 60 or 0)
    end
    if Input.is_key_just_pressed("W") then moving = not moving end
    if moving then t = t + dt end
    local pts = {}
    for k = 0, 40 do
        pts[#pts + 1] = { -520 + 26 * k, 120 + 50 * math.sin(k * 0.45 + t * 2) }
    end
    Line.set(wave, "points", pts)
    UI.set_text(line, "wave " .. (moving and "moving" or "still") .. "   ring " .. (scrolling and "scrolling" or "still"))
end
