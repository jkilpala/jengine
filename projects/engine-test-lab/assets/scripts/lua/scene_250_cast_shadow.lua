-- Scene 250: `cast_shadow`. The sun sweeps from east to west and the
-- posts lay their silhouettes on the floor along it; L hands the light to a
-- lamp, T switches the posts to a top view.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/250-cast-shadow.scene"
local sun, lamp, line
local posts = {}
local t, stopped, lamp_on, top = 0, false, false, false

function init(entity)
    sun = Entity.find_by_name("Sun")
    lamp = Entity.find_by_name("Lamp")
    line = Entity.find_by_name("Line0")
    for i = 0, 2 do posts[#posts + 1] = Entity.find_by_name("Post" .. i) end
end

function update(entity, dt)
    nav.tick(SCENE_PATH)
    if Input.is_key_just_pressed("Space") then stopped = not stopped end
    if Input.is_key_just_pressed("L") then
        lamp_on = not lamp_on
        Lighting.set_intensity(lamp, lamp_on and 1.2 or 0)
        Lighting.set_intensity(sun, lamp_on and 0 or 1)
    end
    if Input.is_key_just_pressed("T") then
        top = not top
        for _, p in ipairs(posts) do CastShadow.set(p, "view", top and "top" or "side") end
    end
    if not stopped then t = t + dt end
    -- The sun's travel, from just below east-going to just below west-going.
    local a = -math.pi * (0.05 + 0.9 * ((t * 0.12) % 1.0))
    Transform.set_rotation(sun, a)   -- a sun is aimed by turning it
    UI.set_text(line, string.format("sun travelling at %d degrees   %s   %s view", math.floor(math.deg(a) + 0.5),
        lamp_on and "lamp" or "sun", top and "top" or "side"))
end
