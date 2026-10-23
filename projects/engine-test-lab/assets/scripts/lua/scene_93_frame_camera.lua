-- Scene 93 - a camera that frames a set
--
-- Two markers run away from each other and come back together, and the
-- camera keeps both on screen: it centres on the box that contains them and
-- zooms out until the box fits. The grid of posts is there so the zoom is
-- visible - without something static to judge against, a camera pulling
-- back looks exactly like everything shrinking.
--
-- `CameraFollow` followed exactly one entity until Sep 2026, and that is
-- the wrong shape for a whole genre: a two-player duel, a co-op screen, a
-- boss and the player running from it. Each has a *set* of things that must
-- all stay visible, and none is served by picking one to centre on. A game
-- that wanted it had to work out a centre and a zoom in script every frame
-- and write both back across the bridge.
--
-- What to look for, and what each thing is guarding against:
--
--   SPACE   toggles the third marker in and out of the set. The frame has
--           to cope with the set changing size - including the moment it
--           becomes empty, which in a duel is when somebody loses. With no
--           tagged entity it falls back to following the camera's own
--           entity rather than going blank.
--   Z       clears the tag from both runners, which is that empty case.
--   The zoom clamp is 0.25 to 1.1. Without a *maximum* the camera zooms in
--           until the screen is two sprites and a wall of empty space,
--           which hides everything either of them is about to run into.
--
-- One thing this scene found rather than demonstrates: `CameraFollow.
-- smoothing` is the fraction of the remaining distance covered per frame,
-- so **larger is faster** - and the component's doc comment said the exact
-- opposite ("0 = instant snap, 1 = very slow follow") from the day it was
-- written. A scene authored against that gets a camera that never moves,
-- which reads as a broken follow rather than a mistuned one.

script = { tick_rate = 60 }

local nav = require("lua/nav")
local SCENE_PATH = "scenes/93-frame-camera.scene"

local a, b, c, readout, keys, camera
local t = 0
local spare_in = false

function init(entity)
    a = Entity.find_by_name("RunnerA")
    b = Entity.find_by_name("RunnerB")
    c = Entity.find_by_name("RunnerC")
    readout = Entity.find_by_name("Readout")
    keys = Entity.find_by_name("Keys")
    camera = Entity.find_by_name("Camera")
    log("[scene_93] ready - both markers should stay on screen the whole time")
end

function update(entity, dt)
    t = t + dt

    -- The two run apart and back together on a slow cycle, and drift
    -- vertically on a different one so the frame is not always square.
    local spread = 260 + 1500 * (0.5 - 0.5 * math.cos(t * 0.5))
    local lift = 360 * math.sin(t * 0.31)
    if a ~= nil then
        Transform.set_position(a, -spread, lift)
    end
    if b ~= nil then
        Transform.set_position(b, spread, -lift * 0.6)
    end
    if c ~= nil then
        Transform.set_position(c, 700 * math.sin(t * 0.8), -520)
    end

    if Input.is_key_just_pressed("Space") and c ~= nil then
        spare_in = not spare_in
        if spare_in then
            Entity.add_tag(c, "runner")
        else
            Entity.remove_tag(c, "runner")
        end
    end
    if Input.is_key_just_pressed("Z") then
        -- The empty case: no tagged entity at all.
        for _, e in ipairs({ a, b, c }) do
            if e ~= nil then
                Entity.remove_tag(e, "runner")
            end
        end
        spare_in = false
    end
    if Input.is_key_just_pressed("X") then
        for _, e in ipairs({ a, b }) do
            if e ~= nil then
                Entity.add_tag(e, "runner")
            end
        end
    end

    -- The readout is world text, so it has to ride the view or it leaves the
    -- screen the moment the camera moves. The framing moves the *renderer's*
    -- camera, not the camera entity (Transform.get_position(camera) stays at 0, 0), and
    -- there is no get_camera_position - so the centre is worked out the way
    -- the frame works it out: the middle of the tagged set's box. Scaled by
    -- 1 / zoom so the text keeps its size on screen as the view pulls back.
    local zoom = Camera.get_zoom ~= nil and Camera.get_zoom() or 1
    if zoom <= 0 then zoom = 1 end
    local cx, cy = 0, 0
    local members = {}
    for _, e in ipairs({ a, b, c }) do
        if e ~= nil and Entity.has_tag(e, "runner") then members[#members + 1] = e end
    end
    if #members > 0 then
        local x0, y0, x1, y1 = math.huge, math.huge, -math.huge, -math.huge
        for _, e in ipairs(members) do
            local x, y = Transform.get_position(e)
            x0, y0 = math.min(x0, x), math.min(y0, y)
            x1, y1 = math.max(x1, x), math.max(y1, y)
        end
        cx, cy = (x0 + x1) * 0.5, (y0 + y1) * 0.5
    elseif camera ~= nil then
        cx, cy = Transform.get_position(camera)
    end
    local k = 1 / zoom
    local title = Entity.find_by_name("Title")
    if title ~= nil then
        Transform.set_position(title, cx, cy + 300 * k)
        Transform.set_scale(title, k, k)
    end
    if readout ~= nil then
        Transform.set_position(readout, cx, cy - 250 * k)
        Transform.set_scale(readout, k, k)
        local ax, ay = Transform.get_position(a)
        local bx, by = Transform.get_position(b)
        local apart = math.sqrt((bx - ax) * (bx - ax) + (by - ay) * (by - ay))
        Text.set(readout, string.format(
            "%.0f units apart   |   camera (%.0f, %.0f)   zoom %.2f   |   third marker %s",
            apart, cx, cy, zoom, spare_in and "IN the set" or "out of it"))
    end
    if keys ~= nil then
        Transform.set_position(keys, cx, cy - 310 * k)
        Transform.set_scale(keys, k, k)
        Text.set(keys,
            "[SPACE] third marker in/out   [Z] clear the set   [X] put it back   "
            .. "[H/Esc] hub   [PgUp/PgDn] prev/next")
    end

    nav.tick(SCENE_PATH)
end
