-- 27 Two Views: the cameras follow their cars. The drawing is the engine's
-- (a camera with a `target` draws into a texture); this moves the cameras.
-- M: minimap.  Space: swap which car each half follows.

script = { tick_rate = 60 }

local nav = require("nav")
local SCENE = "scenes/27-two-views.scene"
local HALF = 640 / 4            -- a quarter of the authored width: half of a half
local ZOOM = 2.0                -- tools/gen_scenes.py: TV_ZOOM

local post_on = nav.post_all_on(SCENE)
local captions = true
local status, cam, p2view, map_frame, map
local cars = {}
local swapped = false
local map_on = true

local function refresh_status()
    if status then
        UI.set_text(status, "four cameras   " .. nav.post_status(post_on))
    end
end

function init(entity)
    status = Entity.find_by_name("Status")
    cam = Entity.find_by_name("Camera")
    p2view = Entity.find_by_name("ViewP2")
    map_frame = Entity.find_by_name("MapFrame")
    map = Entity.find_by_name("Map")
    cars = { Entity.find_by_name("Car5"), Entity.find_by_name("Car3") }
    refresh_status()
end

function update(entity, dt)
    nav.tick(SCENE)
    if nav.post_keys(post_on) then refresh_status() end
    if Input.is_key_just_pressed("Tab") then
        captions = not captions
        for _, name in ipairs({ "Title", "Hint", "Status" }) do
            local e = Entity.find_by_name(name)
            if e then UI.set_visible(e, captions) end
        end
    end
    if Input.is_key_just_pressed("M") then
        map_on = not map_on
        if map_frame then UI.set_visible(map_frame, map_on) end
        if map then UI.set_visible(map, map_on) end
    end
    if Input.is_key_just_pressed("Space") then swapped = not swapped end

    local a, b = cars[1], cars[2]
    if swapped then a, b = b, a end
    -- The main camera draws the whole screen; its car sits in the middle of
    -- the left half, so the camera looks a quarter-screen to its right.
    if cam and a then
        local x, y = Transform.get_position(a)
        Transform.set_position(cam, x + HALF * 2 / ZOOM, y)
    end
    if p2view and b then
        local x, y = Transform.get_position(b)
        Transform.set_position(p2view, x, y)
    end
end
