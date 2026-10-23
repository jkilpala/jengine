-- 103 Snowfield: everything that crosses the snow presses its prints into
-- it. The paths, the trotting and the turning are the engine's
-- (`path_follow`); this script only watches how far each thing has come and,
-- every stride, presses a print where its feet are - Sprite.press dents the
-- snow's normal map, so the moon and the lantern catch the rim - and paints
-- a faint blue shadow into it with Sprite.paint.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/103-snowfield.scene"
local atan2 = math.atan2 or function(y, x) return math.atan(y, x) end

local SHADE = { 0.72, 0.8, 0.94 }   -- a print's shadow, multiplied in

local e = {}
local walkers = {}
local prints = 0
local captions = true
local lantern_on = true

-- What leaves prints: `feet` are { ahead, aside } offsets from the entity in
-- its own frame, taken in turn every `stride` units; a `run` is pressed
-- every step at every foot at once (a sled's runners).
local KINDS = {
    wolf = { stride = 15, size = 8, depth = 1.6, shape = "disc",
             feet = { { 14, 5 }, { -12, -5 }, { 14, -5 }, { -12, 5 } } },
    walker = { stride = 21, size = 14, depth = 1.5, shape = "streak", feet = { { 0, 6 }, { 0, -6 } } },
    sled = { stride = 6, size = 12, depth = 1.0, shape = "streak", run = true, feet = { { -10, 15 }, { -10, -15 } } },
    dog = { stride = 13, size = 6, depth = 1.3, shape = "disc",
            feet = { { 76, 11 }, { 114, -11 }, { 76, -11 }, { 114, 11 } } },
}

local function track(name, kind)
    local id = Entity.find_by_name(name)
    local x, y = Transform.get_position(id)
    walkers[#walkers + 1] = { e = id, kind = KINDS[kind], x = x, y = y, gone = 0, foot = 1, heading = 0 }
end

local function press_at(w, foot, x, y)
    local k = w.kind
    local c, s = math.cos(w.heading), math.sin(w.heading)
    local px = x + c * foot[1] - s * foot[2]
    local py = y + s * foot[1] + c * foot[2]
    Sprite.press(e.Snow, px, py, k.size, k.depth, k.shape, w.heading)
    Sprite.paint(e.Snow, px, py, k.size * 1.3, SHADE[1], SHADE[2], SHADE[3], 0.5, k.shape, w.heading)
    prints = prints + 1
end

local function step(w)
    local x, y = Transform.get_position(w.e)
    local dx, dy = x - w.x, y - w.y
    local d = math.sqrt(dx * dx + dy * dy)
    w.x, w.y = x, y
    if d < 0.01 or d > 200 then return end
    w.heading = atan2(dy, dx)
    w.gone = w.gone + d
    while w.gone >= w.kind.stride do
        w.gone = w.gone - w.kind.stride
        if w.kind.run then
            for _, foot in ipairs(w.kind.feet) do press_at(w, foot, x, y) end
        else
            press_at(w, w.kind.feet[w.foot], x, y)
            w.foot = w.foot % #w.kind.feet + 1
        end
    end
end

function init(entity)
    for _, n in ipairs({ "Snow", "Lantern", "Status" }) do e[n] = Entity.find_by_name(n) end
    for i = 0, 2 do track("Wolf" .. i, "wolf") end
    track("Sled", "sled")
    track("Sled", "dog")
    track("Walker", "walker")
end

function update(entity, dt)
    nav.tick(SCENE)
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_ui_captions(captions) end
    if Input.is_key_just_pressed("L") then
        lantern_on = not lantern_on
        Lighting.set_intensity(e.Lantern, lantern_on and 1.3 or 0)
        Sprite.set_color(e.Lantern, 4.0, 3.0, 1.6, lantern_on and 1 or 0)
    end
    for _, w in ipairs(walkers) do step(w) end
    UI.set_text(e.Status, string.format("prints pressed: %d", math.floor(prints)))
end
