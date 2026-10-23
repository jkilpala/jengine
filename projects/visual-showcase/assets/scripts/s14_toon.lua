-- 14 Toon & Outlines: the key light follows the mouse; the creature under
-- it gets a thicker gold ink line (the ink_outline graph's parameters). T
-- turns toon lighting off and on, and O turns every line off.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/14-toon.scene"

-- tools/gen_scenes.py: TOON_BLOBS, TOON_PROPS, TOON_WIDTH, TOON_INK.
local BLOBS = { 150, 130, 160, 120, 100 }   -- sizes
local PROPS = 7
local WIDTH = 6.0
local INK = { 0.03, 0.02, 0.04, 1.0 }
local GOLD = { 1.4, 0.95, 0.2, 1.0 }
-- tools/gen_scenes.py: the captions are dark, on the bright meadow.
local CAPTION = { 0.0049, 0.00719, 0.00394 }
local CAPTIONS = { Title = CAPTION, Hint = CAPTION, Status = CAPTION }

local captions = true
local post_on = nav.post_all_on(SCENE)
local toon_on, lines_on = true, true
local status, key
local blobs, props = {}, {}
local hovered = nil

local function refresh_status()
    if status then
        Text.set(status, "toon " .. (toon_on and "on" or "off") .. "   lines "
            .. (lines_on and "on" or "off") .. "   " .. nav.post_status(post_on))
    end
end

local function line(e, width, c)
    Material.set_vec4(e, "outline", width, 0, 0, 0)
    Material.set_vec4(e, "outline_color", c[1], c[2], c[3], c[4])
end

local function paint_lines()
    local w = lines_on and WIDTH or 0.0
    for i, e in ipairs(blobs) do
        if i == hovered and lines_on then line(e, WIDTH * 1.6, GOLD) else line(e, w, INK) end
    end
    for _, e in ipairs(props) do line(e, w, INK) end
end

function init(entity)
    status = Entity.find_by_name("Status")
    key = Entity.find_by_name("KeyLight")
    for i = 1, #BLOBS do blobs[i] = Entity.find_by_name("Blob" .. i) end
    for i = 1, PROPS do props[i] = Entity.find_by_name("Prop" .. i) end
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then captions = nav.toggle_captions(captions, CAPTIONS) end
    if Input.is_key_just_pressed("T") then
        toon_on = not toon_on
        -- tools/gen_scenes.py: the ToonBands entity.
        Lighting.set_toon(toon_on and 3 or 0, 0.06)
        refresh_status()
    end
    if Input.is_key_just_pressed("O") then
        lines_on = not lines_on
        paint_lines()
        refresh_status()
    end

    local mx, my = Input.get_mouse_world_position()
    if key then Transform.set_position(key, mx, my + 40) end

    -- The creature under the mouse.
    local over = nil
    for i, e in ipairs(blobs) do
        local x, y = Transform.get_position(e)
        if x and math.abs(mx - x) < BLOBS[i] * 0.4 and math.abs(my - y) < BLOBS[i] * 0.4 then over = i end
    end
    if over ~= hovered then
        hovered = over
        paint_lines()
    end
end
